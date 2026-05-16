struct MainGuard {
    active: bool,
    lease_expires_at_ts: u64,
}

impl MainGuard {
    fn acquire(now_secs: u64) -> Option<Self> {
        state::with_state_mut(|st| {
            let lock_expires_at_ts = st.main_lock_state_ts.unwrap_or(0);
            if lock_expires_at_ts > now_secs {
                return None;
            }
            let lease_expires_at_ts = now_secs.saturating_add(MAIN_TICK_LEASE_SECONDS);
            st.main_lock_state_ts = Some(lease_expires_at_ts);
            Some(Self { active: true, lease_expires_at_ts })
        })
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;
    }

    fn finish(mut self, now_secs: u64, err: Option<u32>, log_config: bool) {
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;

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

/// Install two independent interval timers:
/// - main tick (daily by default)
/// - rescue tick (daily by default)
pub fn install_timers() {
    let (main_s, rescue_s) =
        state::with_state(|st| (st.config.main_interval_seconds, st.config.rescue_interval_seconds));

    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s.max(60)), || async {
        main_tick(false).await;
    });

    ic_cdk_timers::set_timer_interval(Duration::from_secs(rescue_s.max(60)), || async {
        rescue_tick().await;
    });
}

pub fn schedule_immediate_resume_if_needed() {
    let has_payout_plan = state::with_state(|st| st.payout_plan.is_some());
    if !has_payout_plan {
        return;
    }
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        main_tick(true).await;
    });
}

pub fn schedule_immediate_rescue_reconcile() {
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        rescue_tick().await;
    });
}

/// MAIN TICK:
/// Logging:
/// - always logs "Cycles: <amount>" once per run
/// - logs "CONFIG ..." only when the tick reaches the payout / maturity-disbursement path
/// - logs only errors otherwise
async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let gov = NnsGovernanceCanister::new(cfg.governance_canister_id);
    run_main_tick_with_clients(force, now_nanos, now_secs, &cfg, &ledger, &gov).await;
}

async fn run_main_tick_with_clients<L: LedgerClient, G: GovernanceClient>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    cfg: &state::Config,
    ledger: &L,
    gov: &G,
) {
    let Some(guard) = MainGuard::acquire(now_secs) else {
        return;
    };

    if !force {
        // duplicate suppression if timer fires twice closely
        let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
        let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            guard.finish(now_secs, None, false);
            return;
        }
    }

    let mut err: Option<u32> = None;

    #[cfg(feature = "debug_api")]
    if debug_simulate_low_cycles() {
        // Debug-only: simulate low cycles by refusing to perform any external calls.
        err = Some(1004);
        guard.finish(now_secs, err, false);
        return;
    }

    #[cfg(feature = "debug_api")]
    if debug_pause_after_planning() {
        // Debug-only isolation for persisted-plan recovery tests: build and persist the
        // payout plan, then stop before transfers, maturity initiation, or unrelated
        // governance maintenance. Production builds never take this branch.
        let payout_ok = process_payout(ledger, cfg, now_nanos, now_secs).await;
        guard.finish(now_secs, if payout_ok { None } else { Some(1002) }, true);
        return;
    }

    // Best-effort maintenance runs independently of the payout / maturity path. These
    // governance calls only need the configured neuron id, so a temporary failure to read
    // the full neuron must not prevent stake or voting-power refresh.
    if gov.claim_or_refresh_neuron(cfg.neuron_id).await.is_err() {
        log_error(1006);
    }

    if gov.refresh_voting_power(cfg.neuron_id).await.is_err() {
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
            return;
        }
    };

    let in_flight = neuron
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    let log_config = !in_flight;
    if !in_flight {
        // 1) payout stage
        let payout_ok = process_payout(ledger, cfg, now_nanos, now_secs).await;
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
                let canister_owner = self_canister_principal();
                let age_seconds = now_secs.saturating_sub(neuron.aging_since_timestamp_seconds);

                let disb_ok = gov
                    .disburse_maturity_to_account(cfg.neuron_id, 100, canister_owner, None)
                    .await
                    .is_ok();

                if disb_ok {
                    // 3) record age for next payout split
                    state::with_state_mut(|st| st.prev_age_seconds = age_seconds);
                } else {
                    // do not update prev_age_seconds if initiation failed
                    err = Some(1003);
                }
            }
        }
    }

    guard.finish(now_secs, err, log_config);
}
