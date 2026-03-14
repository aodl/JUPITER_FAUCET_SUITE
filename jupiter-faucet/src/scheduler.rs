use candid::Nat;
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::{cell::RefCell, time::Duration};

use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::index::{account_identifier_text, GetAccountIdentifierTransactionsResponse, IcpIndexCanister};
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{CmcClient, IndexClient, LedgerClient};
use crate::state::{ActivePayoutJob, ForcedRescueReason, PendingNotification, RetryState, RetryStep, TransferKind};
use crate::{logic, policy, state};

thread_local! { static LAST_ERR_CODE: RefCell<Option<u32>> = RefCell::new(None); }
const PAGE_SIZE: u64 = 500;
const RETRY_DELAY_SECS: u64 = 60;

fn log_error(code: u32) {
    LAST_ERR_CODE.with(|c| {
        let mut c = c.borrow_mut();
        if *c == Some(code) { return; }
        *c = Some(code);
        ic_cdk::println!("ERR:{}", code);
    });
}
fn log_cycles() { let cycles: u128 = ic_cdk::api::canister_cycle_balance(); ic_cdk::println!("Cycles: {}", cycles); }

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
        if let Some(code) = err { log_error(code); }
        log_cycles();
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn install_timers() {
    let (main_s, rescue_s) = state::with_state(|st| (st.config.main_interval_seconds, st.config.rescue_interval_seconds));
    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s.max(60)), || async { main_tick(false).await; });
    ic_cdk_timers::set_timer_interval(Duration::from_secs(rescue_s.max(60)), || async { rescue_tick().await; });
}

async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = IcpIndexCanister::new(cfg.index_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    run_main_tick_with_clients(force, now_nanos, now_secs, &ledger, &index, &cmc).await;
}

async fn run_main_tick_with_clients<L: LedgerClient, I: IndexClient, C: CmcClient>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    index: &I,
    cmc: &C,
) {
    let Some(guard) = MainGuard::acquire() else { return; };
    if !force {
        let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
        let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            guard.finish(now_secs, None);
            return;
        }
    }
    let ok = process_payout(ledger, index, cmc, now_nanos, now_secs).await;
    if ok {
        attempt_rescue(now_secs).await;
    }
    guard.finish(now_secs, if ok { None } else { Some(3001) });
}

fn payout_account() -> Account {
    let payout_subaccount = state::with_state(|st| st.config.payout_subaccount);
    Account { owner: ic_cdk::api::canister_self(), subaccount: payout_subaccount }
}

fn next_created_at_time_nanos() -> u64 {
    state::with_state(|st| st.active_payout_job.as_ref().expect("active payout job missing").next_created_at_time_nanos)
}
fn advance_created_at_time_nanos() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.next_created_at_time_nanos = job.next_created_at_time_nanos.saturating_add(1); }); }
fn increment_failed_topups() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.failed_topups = job.failed_topups.saturating_add(1); }); }
fn increment_cmc_attempts() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.cmc_attempt_count = Some(job.cmc_attempt_count.unwrap_or(0).saturating_add(1)); }); }
fn increment_cmc_successes() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.cmc_success_count = Some(job.cmc_success_count.unwrap_or(0).saturating_add(1)); }); }
fn note_index_page(resp: &GetAccountIdentifierTransactionsResponse) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            if job.observed_oldest_tx_id.is_none() {
                job.observed_oldest_tx_id = resp.oldest_tx_id;
            }
            if let Some(latest) = resp.transactions.last().map(|tx| tx.id) {
                job.observed_latest_tx_id = Some(latest);
            }
        }
    });
}
fn set_retry_state(retry: RetryState) -> bool {
    state::with_state_mut(|st| {
        let Some(job) = st.active_payout_job.as_mut() else { return false; };
        if job.retry_state.is_some() { return false; }
        job.retry_state = Some(retry);
        true
    })
}
fn clear_retry_state() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.retry_state = None; }); }
fn retry_state_due(now_secs: u64) -> bool {
    state::with_state(|st| st.active_payout_job.as_ref().and_then(|j| j.retry_state.as_ref()).map(|r| now_secs >= r.retry_at_secs).unwrap_or(false))
}
fn retry_state_present() -> bool { state::with_state(|st| st.active_payout_job.as_ref().and_then(|j| j.retry_state.as_ref()).is_some()) }
fn record_successful_notification(now_secs: u64, pending: &PendingNotification) {
    state::with_state_mut(|st| {
        st.last_successful_transfer_ts = Some(now_secs);
        if let Some(job) = st.active_payout_job.as_mut() { logic::apply_notified_transfer(job, pending); }
    });
    increment_cmc_successes();
}
fn record_transfer_outflow(pending: &PendingNotification) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            logic::record_ledger_accepted_transfer(job, pending);
        }
    });
}
fn finalize_completed_job() {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.take() {
            apply_job_health_observations(st, &job);
            st.last_summary = Some(logic::summary_from_job(&job));
        }
    });
}
fn set_next_start(next_start: Option<u64>) { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.next_start = next_start; }); }

fn transfer_arg(to: Account, amount_e8s: u64, fee_e8s: u64, created_at_time_nanos: u64) -> TransferArg {
    let memo_bytes = logic::MEMO_TOP_UP_CANISTER_U64.to_be_bytes().to_vec();
    TransferArg {
        from_subaccount: state::with_state(|st| st.config.payout_subaccount),
        to,
        fee: Some(Nat::from(fee_e8s)),
        created_at_time: Some(created_at_time_nanos),
        memo: Some(Memo::from(memo_bytes)),
        amount: Nat::from(amount_e8s),
    }
}

fn deposit_account_for_pending(cmc_id: candid::Principal, pending: &PendingNotification) -> Account {
    logic::cmc_deposit_account(cmc_id, pending.beneficiary)
}

fn schedule_transfer_retry(pending: PendingNotification, fee_e8s: u64, created_at_time_nanos: u64, now_secs: u64) -> bool {
    set_retry_state(RetryState {
        step: RetryStep::Transfer,
        pending,
        fee_e8s,
        created_at_time_nanos,
        retry_at_secs: now_secs.saturating_add(RETRY_DELAY_SECS),
    })
}

fn schedule_notify_retry(pending: PendingNotification, now_secs: u64) -> bool {
    set_retry_state(RetryState {
        step: RetryStep::Notify,
        pending,
        fee_e8s: 0,
        created_at_time_nanos: 0,
        retry_at_secs: now_secs.saturating_add(RETRY_DELAY_SECS),
    })
}

async fn send_and_notify(
    ledger: &impl LedgerClient,
    cmc: &impl CmcClient,
    pending: PendingNotification,
    to: Account,
    fee_e8s: u64,
    now_secs: u64,
    allow_retry: bool,
    created_at_override: Option<u64>,
) {
    let created_at_time_nanos = created_at_override.unwrap_or_else(next_created_at_time_nanos);
    let arg = transfer_arg(to, pending.amount_e8s, fee_e8s, created_at_time_nanos);
    let ledger_res = ledger.transfer(arg).await;
    if created_at_override.is_none() {
        advance_created_at_time_nanos();
    }

    let block_index = match ledger_res {
        Err(_) => {
            if allow_retry && schedule_transfer_retry(pending, fee_e8s, created_at_time_nanos, now_secs) {
                return;
            }
            increment_failed_topups();
            return;
        }
        Ok(Ok(block)) => match u64::try_from(block.0.clone()) {
            Ok(v) => v,
            Err(_) => { increment_failed_topups(); return; }
        },
        Ok(Err(TransferError::Duplicate { duplicate_of })) => match u64::try_from(duplicate_of.0.clone()) {
            Ok(v) => v,
            Err(_) => { increment_failed_topups(); return; }
        },
        Ok(Err(TransferError::TemporarilyUnavailable)) => {
            if allow_retry && schedule_transfer_retry(pending, fee_e8s, created_at_time_nanos, now_secs) {
                return;
            }
            increment_failed_topups();
            return;
        }
        Ok(Err(_)) => {
            increment_failed_topups();
            return;
        }
    };

    let accepted = PendingNotification { block_index, ..pending };
    record_transfer_outflow(&accepted);
    increment_cmc_attempts();
    if cmc.notify_top_up(accepted.beneficiary, accepted.block_index).await.is_err() {
        if allow_retry && schedule_notify_retry(accepted, now_secs) {
            return;
        }
        increment_failed_topups();
        return;
    }
    record_successful_notification(now_secs, &accepted);
}

async fn process_due_retry(ledger: &impl LedgerClient, cmc: &impl CmcClient, now_secs: u64) {
    if !retry_state_due(now_secs) { return; }
    let retry = state::with_state(|st| st.active_payout_job.as_ref().and_then(|j| j.retry_state.clone()));
    let Some(retry) = retry else { return; };
    clear_retry_state();
    match retry.step {
        RetryStep::Transfer => {
            let cfg = state::with_state(|st| st.config.clone());
            let to = deposit_account_for_pending(cfg.cmc_canister_id, &retry.pending);
            send_and_notify(
                ledger,
                cmc,
                retry.pending,
                to,
                retry.fee_e8s,
                now_secs,
                false,
                Some(retry.created_at_time_nanos),
            ).await;
        }
        RetryStep::Notify => {
            increment_cmc_attempts();
            if cmc.notify_top_up(retry.pending.beneficiary, retry.pending.block_index).await.is_ok() {
                record_successful_notification(now_secs, &retry.pending);
            } else {
                increment_failed_topups();
            }
        }
    }
}

fn ensure_active_job(now_nanos: u64, fee_e8s: u64, pot_start_e8s: u64, denom_e8s: u64) {
    state::with_state_mut(|st| {
        if st.active_payout_job.is_some() { return; }
        let id = st.payout_nonce;
        st.payout_nonce = st.payout_nonce.saturating_add(1);
        st.active_payout_job = Some(ActivePayoutJob::new(id, fee_e8s, pot_start_e8s, denom_e8s, now_nanos));
    });
}

async fn probe_index_health(index: &impl IndexClient, staking_id: &str, denom_balance_e8s: u64) {
    let first_page = match index
        .get_account_identifier_transactions(staking_id.to_string(), None, 1)
        .await
    {
        Ok(resp) => resp,
        Err(_) => return,
    };

    state::with_state_mut(|st| apply_anchor_observation(st, first_page.oldest_tx_id));

    let prev_balance = state::with_state(|st| st.last_observed_staking_balance_e8s);
    let prev_latest = state::with_state(|st| st.last_observed_latest_tx_id);
    if prev_balance.is_none() {
        let latest_tx_id = scan_latest_tx_id(index, staking_id.to_string(), None).await;
        state::with_state_mut(|st| {
            st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
            st.last_observed_latest_tx_id = latest_tx_id;
            st.consecutive_index_latest_invariant_failures = Some(0);
        });
        return;
    }

    if prev_balance == Some(denom_balance_e8s) {
        return;
    }

    let latest_tx_id = scan_latest_tx_id(index, staking_id.to_string(), prev_latest).await;
    state::with_state_mut(|st| apply_latest_observation(st, denom_balance_e8s, latest_tx_id));
}

async fn scan_latest_tx_id(index: &impl IndexClient, staking_id: String, start: Option<u64>) -> Option<u64> {
    let mut cursor = start;
    let mut latest = start;
    loop {
        let resp = index
            .get_account_identifier_transactions(staking_id.clone(), cursor, PAGE_SIZE)
            .await
            .ok()?;
        let page_latest = resp.transactions.last().map(|tx| tx.id);
        if page_latest.is_none() {
            return latest;
        }
        latest = page_latest;
        cursor = page_latest;
        if resp.transactions.len() < PAGE_SIZE as usize {
            return latest;
        }
    }
}

fn apply_anchor_observation(st: &mut state::State, observed_oldest: Option<u64>) {
    let Some(expected_first) = st.config.expected_first_staking_tx_id else { return; };
    if observed_oldest == Some(expected_first) {
        st.consecutive_index_anchor_failures = Some(0);
        return;
    }
    st.consecutive_index_anchor_failures = Some(st.consecutive_index_anchor_failures.unwrap_or(0).saturating_add(1));
    if st.consecutive_index_anchor_failures.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
        st.forced_rescue_reason = Some(ForcedRescueReason::IndexAnchorMissing);
    }
}

fn apply_latest_observation(st: &mut state::State, denom_balance_e8s: u64, latest_tx_id: Option<u64>) {
    match (st.last_observed_staking_balance_e8s, st.last_observed_latest_tx_id) {
        (None, _) => {
            st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
            st.last_observed_latest_tx_id = latest_tx_id;
            st.consecutive_index_latest_invariant_failures = Some(0);
        }
        (Some(prev_balance), _prev_latest_tx_id) if prev_balance == denom_balance_e8s => {}
        (Some(_), prev_latest_tx_id) => {
            let latest_changed = match (prev_latest_tx_id, latest_tx_id) {
                (Some(prev), Some(latest)) => latest != prev,
                (None, Some(_)) => true,
                (_, None) => false,
            };
            if latest_changed {
                st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
                st.last_observed_latest_tx_id = latest_tx_id;
                st.consecutive_index_latest_invariant_failures = Some(0);
            } else {
                st.consecutive_index_latest_invariant_failures = Some(st.consecutive_index_latest_invariant_failures.unwrap_or(0).saturating_add(1));
                if st.consecutive_index_latest_invariant_failures.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
                    st.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestInvariantBroken);
                }
            }
        }
    }
}

fn apply_cmc_run_result(st: &mut state::State, attempts: u64, successes: u64) {
    if attempts == 0 { return; }
    if successes > 0 {
        st.consecutive_cmc_zero_success_runs = Some(0);
        return;
    }
    st.consecutive_cmc_zero_success_runs = Some(st.consecutive_cmc_zero_success_runs.unwrap_or(0).saturating_add(1));
    if st.consecutive_cmc_zero_success_runs.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
        st.forced_rescue_reason = Some(ForcedRescueReason::CmcZeroSuccessRuns);
    }
}

fn apply_job_health_observations(st: &mut state::State, job: &ActivePayoutJob) {
    apply_anchor_observation(st, job.observed_oldest_tx_id);
    apply_latest_observation(st, job.denom_staking_balance_e8s, job.observed_latest_tx_id);
    apply_cmc_run_result(st, job.cmc_attempt_count.unwrap_or(0), job.cmc_success_count.unwrap_or(0));
}

fn maybe_latch_bootstrap_rescue(now_secs: u64) {
    state::with_state_mut(|st| {
        if st.forced_rescue_reason.is_none()
            && policy::bootstrap_rescue_due(now_secs, st.blackhole_armed_since_ts, st.last_successful_transfer_ts)
        {
            st.forced_rescue_reason = Some(ForcedRescueReason::BootstrapNoSuccess);
        }
    });
}

async fn process_payout(ledger: &impl LedgerClient, index: &impl IndexClient, cmc: &impl CmcClient, now_nanos: u64, now_secs: u64) -> bool {
    let cfg = state::with_state(|st| st.config.clone());
    let staking_id = account_identifier_text(&cfg.staking_account);

    if state::with_state(|st| st.active_payout_job.is_none()) {
        let fee_e8s = match ledger.fee_e8s().await { Ok(v) => v, Err(_) => return false };
        let pot_start_e8s = match ledger.balance_of_e8s(payout_account()).await { Ok(v) => v, Err(_) => return false };
        let denom_e8s = match ledger.balance_of_e8s(cfg.staking_account.clone()).await { Ok(v) => v, Err(_) => return false };
        if pot_start_e8s <= fee_e8s || denom_e8s == 0 {
            probe_index_health(index, &staking_id, denom_e8s).await;
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        }
        ensure_active_job(now_nanos, fee_e8s, pot_start_e8s, denom_e8s);
    }

    process_due_retry(ledger, cmc, now_secs).await;

    loop {
        let job = state::with_state(|st| st.active_payout_job.clone());
        let Some(job) = job else { maybe_latch_bootstrap_rescue(now_secs); return true; };
        if job.scan_complete {
            if retry_state_present() {
                maybe_latch_bootstrap_rescue(now_secs);
                return true;
            }
            let remainder_gross_e8s = job.pot_start_e8s.saturating_sub(job.gross_outflow_e8s);
            if remainder_gross_e8s > job.fee_e8s && job.remainder_to_self_e8s == 0 {
                let self_id = ic_cdk::api::canister_self();
                let pending = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: remainder_gross_e8s, amount_e8s: remainder_gross_e8s.saturating_sub(job.fee_e8s), block_index: 0, next_start: None };
                let to = deposit_account_for_pending(cfg.cmc_canister_id, &pending);
                send_and_notify(ledger, cmc, pending, to, job.fee_e8s, now_secs, true, None).await;
                if retry_state_present() {
                    maybe_latch_bootstrap_rescue(now_secs);
                    return true;
                }
            }
            finalize_completed_job();
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        }

        let resp = match index.get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE).await { Ok(v) => v, Err(_) => return false };
        note_index_page(&resp);
        if resp.transactions.is_empty() {
            state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.scan_complete = true; });
            continue;
        }

        for tx in &resp.transactions {
            let Some(contribution) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                set_next_start(Some(tx.id));
                continue;
            };
            let snapshot = state::with_state(|st| {
                let job = st.active_payout_job.as_ref().expect("active payout job missing");
                (job.pot_start_e8s, job.denom_staking_balance_e8s, job.fee_e8s, st.config.min_tx_e8s)
            });
            match logic::evaluate_contribution(snapshot.0, snapshot.1, snapshot.2, snapshot.3, &contribution) {
                logic::ContributionDecision::IgnoreUnderThreshold => state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.ignored_under_threshold = job.ignored_under_threshold.saturating_add(1); job.next_start = Some(tx.id); }),
                logic::ContributionDecision::IgnoreBadMemo => state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.ignored_bad_memo = job.ignored_bad_memo.saturating_add(1); job.next_start = Some(tx.id); }),
                logic::ContributionDecision::NoTransfer => set_next_start(Some(tx.id)),
                logic::ContributionDecision::Eligible { beneficiary, gross_share_e8s, amount_e8s } => {
                    set_next_start(Some(tx.id));
                    let pending = PendingNotification { kind: TransferKind::Beneficiary, beneficiary, gross_share_e8s, amount_e8s, block_index: 0, next_start: Some(tx.id) };
                    let to = deposit_account_for_pending(cfg.cmc_canister_id, &pending);
                    send_and_notify(ledger, cmc, pending, to, snapshot.2, now_secs, true, None).await;
                }
            }
        }
        let last_id = resp.transactions.last().map(|t| t.id);
        state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() {
            if resp.transactions.len() < PAGE_SIZE as usize || last_id.is_none() { job.scan_complete = true; } else { job.next_start = last_id; }
        });
    }
}

async fn attempt_rescue(now_secs: u64) {
    maybe_latch_bootstrap_rescue(now_secs);
    let (blackhole_armed, last_xfer_opt, rescue_controller, rescue_triggered, forced_reason) = state::with_state(|st| {
        (
            st.config.blackhole_armed.unwrap_or(false),
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.rescue_triggered,
            st.forced_rescue_reason.clone(),
        )
    });
    if !blackhole_armed { return; }
    let self_id = ic_cdk::api::canister_self();
    let mut desired = if forced_reason.is_some() {
        vec![rescue_controller, self_id]
    } else {
        let Some(desired) = policy::desired_controllers(now_secs, last_xfer_opt, self_id, rescue_controller) else { return; };
        desired
    };
    desired.sort_by(|a, b| a.to_text().cmp(&b.to_text()));
    desired.dedup();
    let rescue_active = desired.iter().any(|p| *p == rescue_controller);
    if !rescue_active && !rescue_triggered { return; }
    let arg = UpdateSettingsArgs { canister_id: self_id, settings: CanisterSettings { controllers: Some(desired), ..Default::default() } };
    if update_settings(&arg).await.is_err() { log_error(3101); return; }
    state::with_state_mut(|st| { st.last_rescue_check_ts = now_secs; st.rescue_triggered = rescue_active; });
}

async fn rescue_tick() {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    attempt_rescue(now_secs).await;
}


#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use candid::Principal;
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

    struct UnexpectedIndex;

    #[async_trait]
    impl IndexClient for UnexpectedIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            panic!("index should not be called")
        }
    }

    struct PendingCmc {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl CmcClient for PendingCmc {
        async fn notify_top_up(&self, _canister_id: Principal, _block_index: u64) -> Result<(), crate::clients::ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            pending::<Result<(), crate::clients::ClientError>>().await
        }
    }

    fn test_config() -> state::Config {
        state::Config {
            staking_account: Account { owner: Principal::anonymous(), subaccount: None },
            payout_subaccount: None,
            ledger_canister_id: Principal::anonymous(),
            index_canister_id: Principal::anonymous(),
            cmc_canister_id: Principal::anonymous(),
            rescue_controller: Principal::anonymous(),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: None,
            main_interval_seconds: 60,
            rescue_interval_seconds: 60,
            min_tx_e8s: 1,
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
        let mut st = state::State::new(test_config(), now_secs);
        let mut job = ActivePayoutJob::new(1, 10_000, 1_000_000, 2_000_000, now_secs * 1_000_000_000);
        job.retry_state = Some(RetryState {
            step: RetryStep::Notify,
            pending: PendingNotification {
                kind: TransferKind::Beneficiary,
                beneficiary: Principal::anonymous(),
                gross_share_e8s: 100_000,
                amount_e8s: 90_000,
                block_index: 7,
                next_start: None,
            },
            fee_e8s: 0,
            created_at_time_nanos: 0,
            retry_at_secs: 0,
        });
        st.active_payout_job = Some(job);
        state::set_state(st);

        let ledger = UnexpectedLedger;
        let index = UnexpectedIndex;
        let calls = Arc::new(AtomicUsize::new(0));
        let cmc = PendingCmc { calls: calls.clone() };

        let first_now_nanos = now_secs * 1_000_000_000;
        let mut fut1 = Box::pin(run_main_tick_with_clients(false, first_now_nanos, now_secs, &ledger, &index, &cmc));
        assert!(matches!(poll_once(fut1.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(1));

        let second_now_secs = now_secs + (15 * 60) + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(false, second_now_nanos, second_now_secs, &ledger, &index, &cmc));
        assert!(matches!(poll_once(fut2.as_mut()), Poll::Ready(())));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(1));

        drop(fut1);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(0));
    }
}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() { main_tick(true).await; }
#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() { rescue_tick().await; }
