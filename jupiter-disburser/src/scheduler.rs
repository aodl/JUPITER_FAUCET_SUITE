use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{GovernanceClient, LedgerClient};
use crate::{logic, policy, state};

use candid::Nat;
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::{cell::RefCell, time::Duration};

thread_local! {
    // Prevent repeated identical error spam from eating the small log buffer.
    static LAST_ERR_CODE: RefCell<Option<u32>> = RefCell::new(None);
}

#[cfg(feature = "debug_api")]
thread_local! {
    // Debug-only fault injection used by PocketIC E2E tests.
    // These are intentionally *not* persisted in stable memory.
    static DEBUG_PAUSE_AFTER_PLANNING: RefCell<bool> = RefCell::new(false);
    static DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK: RefCell<u32> = RefCell::new(0);

    // Simulates "canister too low on cycles" without depending on PocketIC cycle accounting.
    // When enabled, main tick will refuse to perform any external calls.
    static DEBUG_SIMULATE_LOW_CYCLES: RefCell<bool> = RefCell::new(false);

    // Allows payout-only cycles in PocketIC without constantly initiating new maturity disbursements.
    // Useful for state-size regression tests.
    static DEBUG_SKIP_MATURITY_INITIATION: RefCell<bool> = RefCell::new(false);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_pause_after_planning(enabled: bool) {
    DEBUG_PAUSE_AFTER_PLANNING.with(|v| *v.borrow_mut() = enabled);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_simulate_low_cycles(enabled: bool) {
    DEBUG_SIMULATE_LOW_CYCLES.with(|v| *v.borrow_mut() = enabled);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_skip_maturity_initiation(enabled: bool) {
    DEBUG_SKIP_MATURITY_INITIATION.with(|v| *v.borrow_mut() = enabled);
}

#[cfg(feature = "debug_api")]
fn debug_pause_after_planning() -> bool {
    DEBUG_PAUSE_AFTER_PLANNING.with(|v| *v.borrow())
}

#[cfg(feature = "debug_api")]
fn debug_simulate_low_cycles() -> bool {
    DEBUG_SIMULATE_LOW_CYCLES.with(|v| *v.borrow())
}

#[cfg(feature = "debug_api")]
fn debug_skip_maturity_initiation() -> bool {
    DEBUG_SKIP_MATURITY_INITIATION.with(|v| *v.borrow())
}

#[cfg(feature = "debug_api")]
fn debug_maybe_abort_after_successful_transfer() -> bool {
    let maybe_n = DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow());
    let Some(n) = maybe_n else { return false };

    let count = DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|c| {
        let mut c = c.borrow_mut();
        *c = c.saturating_add(1);
        *c
    });

    // Instead of trapping (which can leave persisted async lock state behind),
    // abort the payout tick *after* the ledger reply but *before* persisting
    // the transfer status. This simulates a crash and forces the next run to
    // observe a Duplicate and complete deterministically.
    count == n
}

fn log_error(code: u32) {
    LAST_ERR_CODE.with(|c| {
        let mut c = c.borrow_mut();
        if *c == Some(code) {
            return;
        }
        *c = Some(code);
        ic_cdk::println!("ERR:{}", code);
    });
}

fn log_cycles() {
    let cycles: u128 = ic_cdk::api::canister_cycle_balance();
    ic_cdk::println!("Cycles: {}", cycles);
}

struct MainGuard {
    active: bool,
}

impl MainGuard {
    fn acquire() -> Option<Self> {
        state::with_state_mut(|st| {
            if st.main_lock_expires_at_ts.unwrap_or(0) != 0 {
                return None;
            }
            // Stored as 0/1 for stable-state compatibility with existing deployments.
            st.main_lock_expires_at_ts = Some(1);
            Some(Self { active: true })
        })
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        state::with_state_mut(|st| st.main_lock_expires_at_ts = Some(0));
        self.active = false;
    }

    fn finish(mut self, now_secs: u64, err: Option<u32>) {
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            st.main_lock_expires_at_ts = Some(0);
        });
        self.active = false;

        if let Some(code) = err {
            log_error(code);
        }

        // Always print cycles line (the only non-error informational log).
        log_cycles();
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

/// MAIN TICK:
/// Logging:
/// - always logs "Cycles: <amount>" once per run
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
    let Some(guard) = MainGuard::acquire() else {
        return;
    };

    if !force {
        // duplicate suppression if timer fires twice closely
        let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
        let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            guard.finish(now_secs, None);
            return;
        }
    }

    let mut err: Option<u32> = None;

    #[cfg(feature = "debug_api")]
    if debug_simulate_low_cycles() {
        // Debug-only: simulate low cycles by refusing to perform any external calls.
        err = Some(1004);
        guard.finish(now_secs, err);
        return;
    }

    // Read neuron info (source of truth for whether a disbursement is still in progress)
    let neuron = match gov.get_full_neuron(cfg.neuron_id).await {
        Ok(n) => n,
        Err(_) => {
            err = Some(1001);
            guard.finish(now_secs, err);
            return;
        }
    };

    let in_flight = neuron
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    if !in_flight {
        // 1) payout stage
        let payout_ok = process_payout(ledger, cfg, now_nanos, now_secs).await;
        if !payout_ok {
            err = Some(1002);
            guard.finish(now_secs, err);
            return;
        }

        // 2) initiate a new disbursement to default staging account (subaccount=None)
        #[cfg(feature = "debug_api")]
        if debug_skip_maturity_initiation() {
            if gov.claim_or_refresh_neuron(cfg.neuron_id).await.is_err() {
                log_error(1006);
            }
            guard.finish(now_secs, err);
            return;
        }

        let canister_owner = ic_cdk::api::canister_self();
        let age_seconds = now_secs.saturating_sub(neuron.aging_since_timestamp_seconds);

        let disb_ok = gov
            .disburse_maturity_to_account(cfg.neuron_id, 100, canister_owner, None)
            .await
            .is_ok();

        if !disb_ok {
            // do not update prev_age_seconds if initiation failed
            err = Some(1003);
            guard.finish(now_secs, err);
            return;
        }

        // 3) record age for next payout split
        state::with_state_mut(|st| st.prev_age_seconds = age_seconds);

        // 4) best-effort voting-power refresh after maturity work has been actioned.
        // This is intentionally late so a refresh API issue cannot block payout or
        // disbursement initiation.
        if gov.refresh_voting_power(cfg.neuron_id).await.is_err() {
            log_error(1005);
        }
    }

    // 5) best-effort neuron stake refresh on every successful tick. This runs after
    // any main work above so user / protocol top-ups into the staking subaccount are
    // recognized without requiring a user-side governance call.
    if gov.claim_or_refresh_neuron(cfg.neuron_id).await.is_err() {
        log_error(1006);
    }

    guard.finish(now_secs, err);
}

/// PAYOUT:
/// - uses default staging account
/// - persists a payout plan for deterministic retries
/// - plans up to 3 transfers; skips any share <= fee (leaves it in staging)
async fn process_payout<L: LedgerClient>(
    ledger: &L,
    cfg: &state::Config,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
    #[cfg(feature = "debug_api")]
    DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|c| *c.borrow_mut() = 0);

    let staging = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: None,
    };

    let balance = match ledger.balance_of_e8s(staging).await {
        Ok(b) => b,
        Err(_) => return false,
    };

    // If empty, clear any stale plan and succeed.
    if balance == 0 {
        state::with_state_mut(|st| st.payout_plan = None);
        return true;
    }

    // Create plan if none exists.
    let need_plan = state::with_state(|st| st.payout_plan.is_none());
    if need_plan {
        let fee = match ledger.fee_e8s().await {
            Ok(f) => f,
            Err(_) => return false,
        };

        let (payout_id, prev_age) = state::with_state_mut(|st| {
            let id = st.payout_nonce;
            st.payout_nonce = st.payout_nonce.saturating_add(1);
            (id, st.prev_age_seconds)
        });

        let (_gross, planned) = logic::plan_payout_transfers(
            payout_id,
            now_nanos,
            balance,
            fee,
            prev_age,
            &cfg.normal_recipient,
            &cfg.age_bonus_recipient_1,
            &cfg.age_bonus_recipient_2,
        );

        let transfers = planned
            .into_iter()
            .map(|p| state::PlannedTransfer {
                to: p.to,
                gross_share_e8s: p.gross_share_e8s,
                amount_e8s: p.amount_e8s,
                created_at_time_nanos: p.created_at_time_nanos,
                memo: p.memo.to_vec(),
                status: state::TransferStatus::Pending,
            })
            .collect::<Vec<_>>();

        state::with_state_mut(|st| {
            st.payout_plan = Some(state::PayoutPlan {
                id: payout_id,
                fee_e8s: fee,
                created_at_base_nanos: now_nanos,
                transfers,
            });
        });
    }

    #[cfg(feature = "debug_api")]
    if debug_pause_after_planning() {
        // Keep plan persisted and force the caller to retry later.
        return false;
    }

    // Execute the plan until done or a transient error occurs.
    loop {
        let plan_opt = state::with_state(|st| st.payout_plan.clone());
        let Some(plan) = plan_opt else { return true };

        let next_idx = plan
            .transfers
            .iter()
            .position(|t| matches!(t.status, state::TransferStatus::Pending));

        let Some(i) = next_idx else {
            state::with_state_mut(|st| st.payout_plan = None);
            return true;
        };

        let t = &plan.transfers[i];
        let arg = TransferArg {
            from_subaccount: None,
            to: t.to.clone(),
            fee: Some(Nat::from(plan.fee_e8s)),
            created_at_time: Some(t.created_at_time_nanos),
            memo: Some(Memo::from(t.memo.clone())),
            amount: Nat::from(t.amount_e8s),
        };

        let res = match ledger.transfer(arg).await {
            Ok(r) => r,
            Err(_) => return false,
        };

        match res {
            Ok(block) => {
                #[cfg(feature = "debug_api")]
                if debug_maybe_abort_after_successful_transfer() { return false; }
                let block_str = block.to_string();
                state::with_state_mut(|st| {
                    if let Some(p) = st.payout_plan.as_mut() {
                        if let Some(tt) = p.transfers.get_mut(i) {
                            tt.status = state::TransferStatus::Sent { block_index: block_str };
                        }
                    }
                    st.last_successful_transfer_ts = Some(now_secs);
                });
            }
            Err(TransferError::Duplicate { duplicate_of }) => {
                #[cfg(feature = "debug_api")]
                if debug_maybe_abort_after_successful_transfer() { return false; }
                let block_str = duplicate_of.to_string();
                state::with_state_mut(|st| {
                    if let Some(p) = st.payout_plan.as_mut() {
                        if let Some(tt) = p.transfers.get_mut(i) {
                            tt.status = state::TransferStatus::Sent { block_index: block_str };
                        }
                    }
                    st.last_successful_transfer_ts = Some(now_secs);
                });
            }
        
            // transient, keep plan and retry later
            Err(TransferError::TemporarilyUnavailable) => return false,
        
            // These can wedge a persisted plan forever if we never rebuild it.
            // The simplest bulletproof behavior is: clear plan and rebuild next run.
            Err(TransferError::BadFee { .. })
            | Err(TransferError::TooOld)
            | Err(TransferError::CreatedInFuture { .. }) => {
                state::with_state_mut(|st| st.payout_plan = None);
                return false;
            }
        
            // other errors: keep plan (or not) doesn't matter much; simplest is retry later.
            Err(_) => return false,
        }
    }
}

/// RESCUE TICK:
/// - errors-only logs
/// - policy-driven decision:
///   * healthy => controllers=[self] when rescue is currently active
///   * broken  => controllers=[rescue,self]
///
/// This path is intentionally driven by persisted local state plus a management-canister
/// controller update. It does not require fresh ledger, governance, or canister-status
/// health checks at the point of escalation.
async fn rescue_tick() {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;

    state::with_state_mut(|st| {
        if st.forced_rescue_reason.is_none()
            && policy::bootstrap_rescue_due(now_secs, st.blackhole_armed_since_ts, st.last_successful_transfer_ts)
        {
            st.forced_rescue_reason = Some(state::ForcedRescueReason::BootstrapNoSuccess);
        }
    });

    let (blackhole_armed, last_xfer_opt, rescue_controller, rescue_triggered, forced_rescue_reason) = state::with_state(|st| {
        (
            st.config.blackhole_armed.unwrap_or(false),
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.rescue_triggered,
            st.forced_rescue_reason.clone(),
        )
    });

    if !blackhole_armed {
        return;
    }

    let self_id = ic_cdk::api::canister_self();
    let mut desired = if forced_rescue_reason.is_some() {
        vec![rescue_controller, self_id]
    } else {
        let Some(desired) = policy::desired_controllers(now_secs, last_xfer_opt, self_id, rescue_controller) else {
            return;
        };
        desired
    };

    desired.sort_by(|a, b| a.to_text().cmp(&b.to_text()));
    desired.dedup();

    let rescue_active = desired.iter().any(|p| *p == rescue_controller);

    if !rescue_active && !rescue_triggered {
        return;
    }

    let arg = UpdateSettingsArgs {
        canister_id: self_id,
        settings: CanisterSettings {
            controllers: Some(desired.clone()),
            ..Default::default()
        },
    };

    if update_settings(&arg).await.is_err() {
        log_error(2002);
        return;
    }

    state::with_state_mut(|st| {
        st.rescue_triggered = rescue_active;
        st.last_rescue_check_ts = now_secs;
    });
}


#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use candid::Principal;
    use crate::nns_types::{GovernanceError, Neuron};
    use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferError};
    use std::future::{pending, Future};
    use std::pin::Pin;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    struct UnexpectedLedger;

    #[async_trait]
    impl LedgerClient for UnexpectedLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { panic!("ledger should not be called") }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { panic!("ledger should not be called") }
        async fn transfer(&self, _arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> { panic!("ledger should not be called") }
    }

    struct PendingGovernance {
        get_full_neuron_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl GovernanceClient for PendingGovernance {
        async fn get_full_neuron(&self, _neuron_id: u64) -> Result<Neuron, GovernanceError> {
            self.get_full_neuron_calls.fetch_add(1, Ordering::SeqCst);
            pending::<Result<Neuron, GovernanceError>>().await
        }

        async fn disburse_maturity_to_account(
            &self,
            _neuron_id: u64,
            _percentage: u32,
            _to_owner: Principal,
            _to_subaccount: Option<Vec<u8>>,
        ) -> Result<Option<u64>, GovernanceError> {
            panic!("disburse_maturity_to_account should not be called")
        }

        async fn refresh_voting_power(&self, _neuron_id: u64) -> Result<(), GovernanceError> {
            panic!("refresh_voting_power should not be called")
        }

        async fn claim_or_refresh_neuron(&self, _neuron_id: u64) -> Result<(), GovernanceError> {
            panic!("claim_or_refresh_neuron should not be called")
        }
    }

    fn test_config() -> state::Config {
        state::Config {
            neuron_id: 1,
            normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
            age_bonus_recipient_1: Account { owner: Principal::anonymous(), subaccount: None },
            age_bonus_recipient_2: Account { owner: Principal::anonymous(), subaccount: None },
            ledger_canister_id: Principal::anonymous(),
            governance_canister_id: Principal::anonymous(),
            rescue_controller: Principal::anonymous(),
            blackhole_armed: Some(false),
            main_interval_seconds: 60,
            rescue_interval_seconds: 60,
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker { RawWaker::new(std::ptr::null(), &VTABLE) }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        future.poll(&mut cx)
    }

    #[test]
    fn main_lock_prevents_overlap_after_old_lease_window_passes() {
        let now_secs = 1_000_u64;
        state::set_state(state::State::new(test_config(), now_secs));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = UnexpectedLedger;
        let calls = Arc::new(AtomicUsize::new(0));
        let gov = PendingGovernance { get_full_neuron_calls: calls.clone() };

        let first_now_nanos = now_secs * 1_000_000_000;
        let mut fut1 = Box::pin(run_main_tick_with_clients(false, first_now_nanos, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut1.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(1));

        let second_now_secs = now_secs + (15 * 60) + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(false, second_now_nanos, second_now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut2.as_mut()), Poll::Ready(())));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(1));

        drop(fut1);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(0));
    }
}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() {
    main_tick(true).await;
}

#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() {
    rescue_tick().await;
}

#[cfg(feature = "debug_api")]
pub async fn debug_build_payout_plan_impl() -> bool {
    let now_nanos = ic_cdk::api::time() as u64;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);

    let staging = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: None,
    };

    let balance = match ledger.balance_of_e8s(staging).await {
        Ok(b) => b,
        Err(_) => return false,
    };

    if balance == 0 {
        state::with_state_mut(|st| st.payout_plan = None);
        return true;
    }

    let already = state::with_state(|st| st.payout_plan.is_some());
    if already {
        return true;
    }

    let fee = match ledger.fee_e8s().await {
        Ok(f) => f,
        Err(_) => return false,
    };

    let (payout_id, prev_age) = state::with_state_mut(|st| {
        let id = st.payout_nonce;
        st.payout_nonce = st.payout_nonce.saturating_add(1);
        (id, st.prev_age_seconds)
    });

    let (_gross, planned) = logic::plan_payout_transfers(
        payout_id,
        now_nanos,
        balance,
        fee,
        prev_age,
        &cfg.normal_recipient,
        &cfg.age_bonus_recipient_1,
        &cfg.age_bonus_recipient_2,
    );

    let transfers = planned
        .into_iter()
        .map(|p| state::PlannedTransfer {
            to: p.to,
            gross_share_e8s: p.gross_share_e8s,
            amount_e8s: p.amount_e8s,
            created_at_time_nanos: p.created_at_time_nanos,
            memo: p.memo.to_vec(),
            status: state::TransferStatus::Pending,
        })
        .collect::<Vec<_>>();

    state::with_state_mut(|st| {
        st.payout_plan = Some(state::PayoutPlan {
            id: payout_id,
            fee_e8s: fee,
            created_at_base_nanos: now_nanos,
            transfers,
        });
    });

    true
}

