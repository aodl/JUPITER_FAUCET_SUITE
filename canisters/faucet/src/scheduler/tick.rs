use super::*;
use jupiter_ic_clients::timer_guard::{LeaseFinish, TimerLeaseGuard};
use std::cell::Cell;
pub(super) const SCHEDULED_MAIN_RETRY_SECONDS: u64 = 60;

thread_local! {
    // A one-shot retry owns the marker it reserved. Main admission/completion
    // retires older intents; callbacks consume only their own reservation before await.
    static SCHEDULED_MAIN_RETRY_PENDING: Cell<Option<u64>> = const { Cell::new(None) };
    static MAIN_RUN_MARKER: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MainTickOutcome {
    Completed,
    MainGuardBusy,
    Deferred(u64),
}
pub(super) struct MainGuard {
    inner: TimerLeaseGuard,
}

impl MainGuard {
    pub(super) fn acquire(now_secs: u64) -> Option<Self> {
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

    pub(super) fn finish(mut self, now_secs: u64, err: Option<u32>) {
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
        log_cycles();
        log_current_state();
        log_current_config();
    }

    pub(super) fn lease_token(&self) -> MainLeaseToken {
        MainLeaseToken(Some(self.inner.lease_expires_at_ts()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MainLeaseToken(Option<u64>);

impl MainLeaseToken {
    pub(super) fn capture() -> Self {
        Self(state::with_state(|st| st.main_lock_state_ts))
    }

    pub(super) fn is_current(self) -> bool {
        state::with_state(|st| self.is_current_in(st))
    }

    pub(super) fn is_current_in(self, st: &state::State) -> bool {
        st.main_lock_state_ts == self.0
    }

    #[cfg(test)]
    pub(super) fn capture_for_test() -> Self {
        Self::capture()
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        self.release();
    }
}

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

pub(crate) fn schedule_immediate_rescue_reconcile() {
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        rescue_tick().await;
    });
}

async fn scheduled_main_tick() {
    run_scheduled_main_tick(main_tick(false), schedule_scheduled_main_retry).await;
}

// Keep the production admission decision and callback wiring testable with existing clients.
pub(super) async fn run_scheduled_main_tick(
    run: impl std::future::Future<Output = MainTickOutcome>,
    schedule: impl FnOnce(u64, u64),
) -> MainTickOutcome {
    let marker = MAIN_RUN_MARKER.with(Cell::get);
    let outcome = run.await;
    let retry_after_seconds = match outcome {
        MainTickOutcome::Completed => return outcome,
        MainTickOutcome::MainGuardBusy => SCHEDULED_MAIN_RETRY_SECONDS,
        MainTickOutcome::Deferred(remaining) => remaining,
    };
    if MAIN_RUN_MARKER.with(Cell::get) == marker {
        if let Some(reservation) = reserve_scheduled_main_retry() {
            schedule(reservation, retry_after_seconds);
        }
    }
    outcome
}

fn schedule_scheduled_main_retry(reservation: u64, retry_after_seconds: u64) {
    ic_cdk_timers::set_timer(Duration::from_secs(retry_after_seconds), async move {
        run_scheduled_main_retry(
            reservation,
            || main_tick(false),
            schedule_scheduled_main_retry,
        )
        .await;
    });
}

pub(super) async fn run_scheduled_main_retry<F: std::future::Future<Output = MainTickOutcome>>(
    reservation: u64,
    run: impl FnOnce() -> F,
    schedule: impl FnOnce(u64, u64),
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
    let index = IcpIndexCanister::new(cfg.index_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let governance = NnsGovernanceCanister::new(
        cfg.governance_canister_id
            .expect("governance_canister_id configured"),
    );
    let status_client = ManagementCanisterInfoClient;
    run_main_tick_with_clients(
        force,
        now_nanos,
        now_secs,
        &ledger,
        &index,
        &cmc,
        &governance,
        &status_client,
    )
    .await
}

// The scheduler takes explicit clients so tests can verify async/state invariants without canister calls.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_main_tick_with_clients<
    L: LedgerClient,
    I: IndexClient,
    C: CmcClient,
    G: GovernanceClient,
    S: CanisterStatusClient,
>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    index: &I,
    cmc: &C,
    governance: &G,
    status_client: &S,
) -> MainTickOutcome {
    let Some(guard) = MainGuard::acquire(now_secs) else {
        return MainTickOutcome::MainGuardBusy;
    };
    let lease = guard.lease_token();
    debug_reset_successful_transfer_counter();
    if !force {
        let remaining = state::with_state(|st| {
            st.last_main_run_ts
                .saturating_add(st.config.main_interval_seconds.saturating_sub(60))
                .saturating_sub(now_secs)
        });
        if remaining > 0 {
            drop(guard);
            return MainTickOutcome::Deferred(remaining);
        }
    }
    advance_main_run_marker();
    let ok = process_payout_with_lease(
        ledger,
        index,
        cmc,
        governance,
        status_client,
        now_nanos,
        now_secs,
        lease,
    )
    .await;
    if ok && lease.is_current() {
        attempt_rescue_with_main_lease(now_secs, lease).await;
    }
    guard.finish(now_secs, if ok { None } else { Some(3001) });
    MainTickOutcome::Completed
}

pub(super) fn self_canister_principal() -> Principal {
    #[cfg(test)]
    {
        Principal::anonymous()
    }
    #[cfg(not(test))]
    {
        ic_cdk::api::canister_self()
    }
}

pub(super) fn payout_account() -> Account {
    let payout_subaccount = state::with_state(|st| st.config.payout_subaccount);
    Account {
        owner: self_canister_principal(),
        subaccount: payout_subaccount,
    }
}
