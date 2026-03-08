use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{GovernanceClient, LedgerClient};
use crate::{logic, policy, state};

use candid::Nat;
use ic_cdk::management_canister::{
    canister_status, update_settings, CanisterSettings, CanisterStatusArgs, UpdateSettingsArgs,
};
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

    // Instead of trapping (which can wedge async locks across await boundaries),
    // we abort the payout tick *after* the ledger reply but *before* persisting
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

fn controllers_equal_as_set(a: &[candid::Principal], b: &[candid::Principal]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|x| b.contains(x))
}

/// Install two independent interval timers:
/// - main tick (daily by default)
/// - rescue tick (daily by default)
pub fn install_timers() {
    let (main_s, rescue_s) =
        state::with_state(|st| (st.config.main_interval_seconds, st.config.rescue_interval_seconds));

    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s.max(60)), || async {
        main_tick().await;
    });

    ic_cdk_timers::set_timer_interval(Duration::from_secs(rescue_s.max(60)), || async {
        rescue_tick().await;
    });
}

/// MAIN TICK:
/// Logging:
/// - always logs "Cycles: <amount>" once per run
/// - logs only errors otherwise
async fn main_tick() {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;

    // lock: avoid overlapping executions
    let should_run = state::with_state_mut(|st| {
        if st.main_lock {
            return false;
        }
        st.main_lock = true;
        true
    });
    if !should_run {
        return;
    }

    // duplicate suppression if timer fires twice closely
    let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
    let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
    if recently_ran {
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs; // extend suppression window
            st.main_lock = false;
        });
        return;
    }


    let mut err: Option<u32> = None;

    #[cfg(feature = "debug_api")]
    if debug_simulate_low_cycles() {
        // Debug-only: simulate low cycles by refusing to perform any external calls.
        err = Some(1004);
        finish_main(now_secs, err);
        return;
    }


    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let gov = NnsGovernanceCanister::new(cfg.governance_canister_id);

    // Read neuron info (source of truth for whether a disbursement is still in progress)
    let neuron = match gov.get_full_neuron(cfg.neuron_id).await {
        Ok(n) => n,
        Err(_) => {
            err = Some(1001);
            finish_main(now_secs, err);
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
        let payout_ok = process_payout(&ledger, &cfg, now_nanos, now_secs).await;
        if !payout_ok {
            err = Some(1002);
            finish_main(now_secs, err);
            return;
        }

        // 2) initiate a new disbursement to default staging account (subaccount=None)
        #[cfg(feature = "debug_api")]
        if debug_skip_maturity_initiation() {
            finish_main(now_secs, err);
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
            finish_main(now_secs, err);
            return;
        }

        // 3) record age for next payout split
        state::with_state_mut(|st| st.prev_age_seconds = age_seconds);

        // 4) best-effort voting-power refresh after maturity work has been actioned.
        // This is intentionally last so a refresh API issue cannot block payout or
        // disbursement initiation.
        if gov.refresh_voting_power(cfg.neuron_id).await.is_err() {
            log_error(1005);
        }
    }

    finish_main(now_secs, err);
}

fn finish_main(now_secs: u64, err: Option<u32>) {
    state::with_state_mut(|st| {
        st.last_main_run_ts = now_secs;
        st.main_lock = false;
    });

    if let Some(code) = err {
        log_error(code);
    }

    // Always print cycles line (the only non-error informational log).
    log_cycles();
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
///   * healthy => controllers=[self]
///   * broken  => controllers=[rescue,self] (but gated to attempt at most once per 14 days)
///
/// This path does not need to confirm current ledger or governance health.
/// It only uses the persisted timestamp of the last confirmed successful transfer
/// plus management-canister controller updates.
async fn rescue_tick() {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;

    let should_run = state::with_state_mut(|st| {
        if st.rescue_lock {
            return false;
        }
        st.rescue_lock = true;
        true
    });
    if !should_run {
        return;
    }

    let (last_xfer_opt, rescue_controller, last_rescue_check_ts) = state::with_state(|st| {
        (
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.last_rescue_check_ts,
        )
    });

    let self_id = ic_cdk::api::canister_self();
    let desired_opt = policy::desired_controllers(now_secs, last_xfer_opt, self_id, rescue_controller);

    let Some(mut desired) = desired_opt else {
        state::with_state_mut(|st| st.rescue_lock = false);
        return;
    };

    // Gate BROKEN actions to once per 14 days; HEALTHY actions happen immediately.
    let is_broken_action = desired.len() > 1;
    if is_broken_action && !policy::should_attempt_broken_escalation(now_secs, last_rescue_check_ts) {
        state::with_state_mut(|st| st.rescue_lock = false);
        return;
    }
    if is_broken_action {
        state::with_state_mut(|st| st.last_rescue_check_ts = now_secs);
    }

    desired.sort_by(|a, b| a.to_text().cmp(&b.to_text()));

    let status = match canister_status(&CanisterStatusArgs { canister_id: self_id }).await {
        Ok(s) => s,
        Err(_) => {
            state::with_state_mut(|st| st.rescue_lock = false);
            log_error(2001);
            return;
        }
    };

    let mut current = status.settings.controllers;
    current.sort_by(|a, b| a.to_text().cmp(&b.to_text()));

    if controllers_equal_as_set(&current, &desired) {
        state::with_state_mut(|st| st.rescue_lock = false);
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
    } else {
        state::with_state_mut(|st| st.rescue_triggered = desired.len() > 1);
    }

    state::with_state_mut(|st| st.rescue_lock = false);
}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() {
    main_tick().await;
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



