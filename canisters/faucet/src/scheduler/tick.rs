use super::*;
pub(super) struct MainGuard {
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

    fn finish(mut self, now_secs: u64, err: Option<u32>) {
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;
        if let Some(code) = err { log_error(code); }
        log_cycles();
        log_current_config();
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        self.release();
    }
}

pub(crate) fn install_timers() {
    let (main_s, rescue_s) = state::with_state(|st| (st.config.main_interval_seconds, st.config.rescue_interval_seconds));
    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s.max(60)), || async { main_tick(false).await; });
    ic_cdk_timers::set_timer_interval(Duration::from_secs(rescue_s.max(60)), || async { rescue_tick().await; });
}

pub(crate) fn schedule_immediate_resume_if_needed() {
    let has_active_job = state::with_state(|st| st.active_payout_job.is_some());
    if !has_active_job {
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

pub(super) async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = IcpIndexCanister::new(cfg.index_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let governance = NnsGovernanceCanister::new(cfg.governance_canister_id.expect("governance_canister_id configured"));
    let status_client = ManagementCanisterInfoClient;
    run_main_tick_with_clients(force, now_nanos, now_secs, &ledger, &index, &cmc, &governance, &status_client).await;
}

// The scheduler takes explicit clients so tests can verify async/state invariants without canister calls.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_main_tick_with_clients<L: LedgerClient, I: IndexClient, C: CmcClient, G: GovernanceClient, S: CanisterStatusClient>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    index: &I,
    cmc: &C,
    governance: &G,
    status_client: &S,
) {
    let Some(guard) = MainGuard::acquire(now_secs) else { return; };
    debug_reset_successful_transfer_counter();
    if !force {
        let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
        let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            guard.finish(now_secs, None);
            return;
        }
    }
    let ok = process_payout(ledger, index, cmc, governance, status_client, now_nanos, now_secs).await;
    if ok {
        attempt_rescue(now_secs).await;
    }
    guard.finish(now_secs, if ok { None } else { Some(3001) });
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
    Account { owner: self_canister_principal(), subaccount: payout_subaccount }
}
