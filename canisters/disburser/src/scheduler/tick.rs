use super::*;
use jupiter_ic_clients::timer_guard::{LeaseFinish, TimerLeaseGuard};
use std::cell::Cell;
pub(super) const SCHEDULED_MAIN_RETRY_SECONDS: u64 = 60;

thread_local! {
    // A one-shot retry owns the marker it reserved. Main admission/completion
    // retires older intents; callbacks consume only their own reservation before await.
    static SCHEDULED_MAIN_RETRY_PENDING: Cell<Option<u64>> = const { Cell::new(None) };
    static MAIN_RUN_MARKER: Cell<u64> = const { Cell::new(0) };
    #[cfg(test)]
    pub(super) static TEST_NOW_SECONDS: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MainTickOutcome {
    Completed,
    MainGuardBusy,
    Deferred,
}
pub(super) struct MainGuard {
    inner: TimerLeaseGuard,
}

impl MainGuard {
    pub(super) fn is_current(&self) -> bool {
        #[cfg(test)]
        let now_secs = TEST_NOW_SECONDS.with(Cell::get);
        #[cfg(not(test))]
        let now_secs = ic_cdk::api::time() / 1_000_000_000;
        state::with_state(|st| {
            self.inner.is_active()
                && now_secs < self.inner.lease_expires_at_ts()
                && st.main_lock_state_ts == Some(self.inner.lease_expires_at_ts())
        })
    }

    pub(super) fn acquire(now_secs: u64) -> Option<Self> {
        #[cfg(test)]
        TEST_NOW_SECONDS.with(|clock| clock.set(now_secs));
        state::with_state_mut(|st| {
            let inner =
                TimerLeaseGuard::acquire(now_secs, MAIN_TICK_LEASE_SECONDS, st.main_lock_state_ts)?;
            let lease_expires_at_ts = inner.lease_expires_at_ts();
            st.main_lock_state_ts = Some(lease_expires_at_ts);
            Some(Self { inner })
        })
    }

    fn release(&mut self) {
        if !self.inner.is_active() {
            return;
        }
        state::with_state_mut(|st| {
            if self.inner.release(st.main_lock_state_ts) == LeaseFinish::Released {
                st.main_lock_state_ts = Some(0);
            }
        });
    }

    fn finish(mut self, now_secs: u64, err: Option<u32>, log_config: bool) {
        if !self.is_current() {
            return;
        }
        state::with_state_mut(|st| {
            if self.inner.release(st.main_lock_state_ts) == LeaseFinish::Released {
                advance_main_run_marker();
                st.last_main_run_ts = now_secs;
                st.main_lock_state_ts = Some(0);
            }
        });

        if let Some(code) = err {
            log_error(code);
        }

        // Always print the cycles health line; config logging is limited to payout-cadence ticks.
        log_cycles();
        if log_config {
            log_current_config();
        }
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        self.release();
    }
}

pub(super) async fn run_poke_with_clients<L: LedgerClient, G: GovernanceClient>(
    now_nanos: u64,
    now_secs: u64,
    cfg: &state::Config,
    ledger: &L,
    gov: &G,
) {
    let Some(guard) = MainGuard::acquire(now_secs) else {
        return;
    };
    let Ok(neuron) = gov.get_full_neuron(cfg.neuron_id).await else {
        return;
    };
    if !guard.is_current() {
        return;
    }
    let in_flight = neuron
        .maturity_disbursements_in_progress
        .as_ref()
        .is_some_and(|items| !items.is_empty());
    if !in_flight {
        let _ = process_payout(ledger, cfg, now_nanos, now_secs, &guard).await;
    }
}

/// Install two independent interval timers:
/// - main tick (daily by default)
/// - rescue tick (daily by default)
pub(crate) fn install_timers() {
    let (main_s, rescue_s) = state::with_state(|st| {
        (
            st.config.main_interval_seconds,
            st.config.rescue_interval_seconds,
        )
    });

    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s.max(60)), || async {
        scheduled_main_tick().await;
    });

    ic_cdk_timers::set_timer_interval(Duration::from_secs(rescue_s.max(60)), || async {
        rescue_tick().await;
    });
}

pub(crate) fn schedule_immediate_resume_if_needed() {
    let has_payout_plan = state::with_state(|st| st.payout_plan.is_some());
    if !has_payout_plan {
        return;
    }
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        main_tick(true).await;
    });
}

pub(crate) fn schedule_immediate_rescue_reconcile() {
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        rescue_tick().await;
    });
}

/// MAIN TICK:
/// Logging:
/// - always logs "Cycles: <amount>" once per run
/// - logs "CONFIG ..." only when the tick reaches the payout / maturity-disbursement path
/// - logs only errors otherwise
async fn scheduled_main_tick() {
    run_scheduled_main_tick(main_tick(false), schedule_scheduled_main_retry).await;
}

// Keep the production admission decision and callback wiring testable with existing clients.
pub(super) async fn run_scheduled_main_tick(
    run: impl std::future::Future<Output = MainTickOutcome>,
    schedule: impl FnOnce(u64),
) -> MainTickOutcome {
    let marker = MAIN_RUN_MARKER.with(Cell::get);
    let outcome = run.await;
    if matches!(
        outcome,
        MainTickOutcome::MainGuardBusy | MainTickOutcome::Deferred
    ) && MAIN_RUN_MARKER.with(Cell::get) == marker
    {
        if let Some(reservation) = reserve_scheduled_main_retry() {
            schedule(reservation);
        }
    }
    outcome
}

fn schedule_scheduled_main_retry(reservation: u64) {
    ic_cdk_timers::set_timer(
        Duration::from_secs(SCHEDULED_MAIN_RETRY_SECONDS),
        async move {
            run_scheduled_main_retry(
                reservation,
                || main_tick(false),
                schedule_scheduled_main_retry,
            )
            .await;
        },
    );
}

pub(super) async fn run_scheduled_main_retry<F: std::future::Future<Output = MainTickOutcome>>(
    reservation: u64,
    run: impl FnOnce() -> F,
    schedule: impl FnOnce(u64),
) {
    let owns = SCHEDULED_MAIN_RETRY_PENDING.with(|pending| {
        if pending.get() != Some(reservation) {
            return false;
        }
        pending.set(None);
        true
    });
    if owns && MAIN_RUN_MARKER.with(Cell::get) == reservation {
        run_scheduled_main_tick(run(), schedule).await;
    }
}

pub(super) fn reserve_scheduled_main_retry() -> Option<u64> {
    let marker = MAIN_RUN_MARKER.with(Cell::get);
    SCHEDULED_MAIN_RETRY_PENDING.with(|pending| {
        if pending.get() == Some(marker) {
            return None;
        }
        pending.set(Some(marker));
        Some(marker)
    })
}

fn advance_main_run_marker() {
    MAIN_RUN_MARKER.with(|marker| {
        marker.set(
            marker
                .get()
                .checked_add(1)
                .expect("main run marker exhausted"),
        )
    });
    SCHEDULED_MAIN_RETRY_PENDING.with(|pending| pending.set(None));
}

#[cfg(test)]
pub(super) fn clear_scheduled_main_retry() {
    SCHEDULED_MAIN_RETRY_PENDING.with(|pending| pending.set(None));
}

pub(super) async fn main_tick(force: bool) -> MainTickOutcome {
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let gov = NnsGovernanceCanister::new(cfg.governance_canister_id);
    run_main_tick_with_clients(force, now_nanos, now_secs, &cfg, &ledger, &gov).await
}

pub(super) async fn run_main_tick_with_clients<L: LedgerClient, G: GovernanceClient>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    cfg: &state::Config,
    ledger: &L,
    gov: &G,
) -> MainTickOutcome {
    let Some(guard) = MainGuard::acquire(now_secs) else {
        return MainTickOutcome::MainGuardBusy;
    };

    if !force {
        // duplicate suppression if timer fires twice closely
        let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
        let recently_ran =
            state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            drop(guard);
            return MainTickOutcome::Deferred;
        }
    }

    advance_main_run_marker();
    let mut err: Option<u32> = None;

    #[cfg(feature = "debug_api")]
    if debug_simulate_low_cycles() {
        // Debug-only: simulate low cycles by refusing to perform any external calls.
        err = Some(1004);
        guard.finish(now_secs, err, false);
        return MainTickOutcome::Completed;
    }

    #[cfg(feature = "debug_api")]
    if debug_pause_after_planning() {
        // Debug-only isolation for persisted-plan recovery tests: build and persist the
        // payout plan, then stop before transfers, maturity initiation, or unrelated
        // governance maintenance. Production builds never take this branch.
        let payout_ok = process_payout(ledger, cfg, now_nanos, now_secs, &guard).await;
        guard.finish(now_secs, if payout_ok { None } else { Some(1002) }, true);
        return MainTickOutcome::Completed;
    }

    // Best-effort maintenance runs independently of the payout / maturity path. These
    // governance calls only need the configured neuron id, so a temporary failure to read
    // the full neuron must not prevent stake or voting-power refresh.
    let claim_result = gov.claim_or_refresh_neuron(cfg.neuron_id).await;
    if !guard.is_current() {
        return MainTickOutcome::Completed;
    }
    if claim_result.is_err() {
        log_error(1006);
    }

    let refresh_result = gov.refresh_voting_power(cfg.neuron_id).await;
    if !guard.is_current() {
        return MainTickOutcome::Completed;
    }
    if refresh_result.is_err() {
        log_error(1005);
    }

    // Read neuron info after maintenance. This source-of-truth read is needed only for
    // payout / maturity logic: it tells us whether a disbursement is still in flight and
    // supplies the aging timestamp used to snapshot the age-bonus split.
    let neuron = match gov.get_full_neuron(cfg.neuron_id).await {
        Ok(n) => n,
        Err(_) => {
            err = Some(1001);
            guard.finish(now_secs, err, false);
            return MainTickOutcome::Completed;
        }
    };

    let in_flight = neuron
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    if !guard.is_current() {
        return MainTickOutcome::Completed;
    }

    let log_config = !in_flight;
    if !in_flight {
        // 1) payout stage
        let payout_ok = process_payout(ledger, cfg, now_nanos, now_secs, &guard).await;
        if !payout_ok {
            err = Some(1002);
        } else {
            // 2) initiate a new disbursement to default staging account (subaccount=None)
            let skip_maturity_initiation = {
                #[cfg(feature = "debug_api")]
                {
                    debug_skip_maturity_initiation()
                }
                #[cfg(not(feature = "debug_api"))]
                {
                    false
                }
            };

            if !skip_maturity_initiation {
                if !guard.is_current() {
                    return MainTickOutcome::Completed;
                }
                let canister_owner = self_canister_principal();
                let age_seconds = now_secs.saturating_sub(neuron.aging_since_timestamp_seconds);
                // Persist before dispatch: a successful Governance operation may
                // lose its reply. A late callback must never rewrite this snapshot.
                state::with_state_mut(|st| st.prev_age_seconds = age_seconds);

                let disb_ok = gov
                    .disburse_maturity_to_account(cfg.neuron_id, 100, canister_owner, None)
                    .await
                    .is_ok();

                if !disb_ok {
                    err = Some(1003);
                }
            }
        }
    }

    guard.finish(now_secs, err, log_config);
    MainTickOutcome::Completed
}
