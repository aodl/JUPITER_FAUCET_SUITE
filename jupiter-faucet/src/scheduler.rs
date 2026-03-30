use candid::{Nat, Principal};
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::time::Duration;

use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::index::{account_identifier_text, GetAccountIdentifierTransactionsResponse, IcpIndexCanister};
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{CmcClient, IndexClient, LedgerClient};
use crate::state::{ActivePayoutJob, ForcedRescueReason, PendingNotification, TransferKind};
use crate::{logic, policy, state};

const PAGE_SIZE: u64 = 500;
const LEDGER_CREATED_AT_MAX_AGE_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;
const LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS: u64 = 60 * 1_000_000_000;

#[cfg(test)]
thread_local! {
    static TEST_LOG_LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn emit_log_line(line: String) {
    #[cfg(test)]
    {
        TEST_LOG_LINES.with(|logs| logs.borrow_mut().push(line));
        return;
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("{}", line);
    }
}

fn log_error(code: u32) {
    emit_log_line(format!("ERR:{}", code));
}
fn log_cycles() {
    #[cfg(test)]
    {
        return;
    }
    #[cfg(not(test))]
    {
        let cycles: u128 = ic_cdk::api::canister_cycle_balance();
        emit_log_line(format!("Cycles: {}", cycles));
    }
}

fn log_summary(summary: &state::Summary) {
    emit_log_line(format!(
        "SUMMARY:topped_up_count={} failed_topups={} ignored_under_threshold={} ignored_bad_memo={} remainder_to_self_e8s={} pot_remaining_e8s={}",
        summary.topped_up_count,
        summary.failed_topups,
        summary.ignored_under_threshold,
        summary.ignored_bad_memo,
        summary.remainder_to_self_e8s,
        summary.pot_remaining_e8s
    ));
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

fn self_canister_principal() -> Principal {
    #[cfg(test)]
    {
        Principal::anonymous()
    }
    #[cfg(not(test))]
    {
        ic_cdk::api::canister_self()
    }
}

fn payout_account() -> Account {
    let payout_subaccount = state::with_state(|st| st.config.payout_subaccount);
    Account { owner: self_canister_principal(), subaccount: payout_subaccount }
}

fn created_at_time_is_valid_for_ledger(created_at_time_nanos: u64, now_nanos: u64) -> bool {
    let not_too_old = now_nanos.saturating_sub(created_at_time_nanos) <= LEDGER_CREATED_AT_MAX_AGE_NANOS;
    let not_too_far_in_future = created_at_time_nanos <= now_nanos.saturating_add(LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS);
    not_too_old && not_too_far_in_future
}

fn allocate_created_at_time_nanos(now_nanos: u64) -> u64 {
    state::with_state_mut(|st| {
        let job = st.active_payout_job.as_mut().expect("active payout job missing");
        if !created_at_time_is_valid_for_ledger(job.next_created_at_time_nanos, now_nanos) {
            job.next_created_at_time_nanos = now_nanos;
        }
        let created_at_time_nanos = job.next_created_at_time_nanos;
        job.next_created_at_time_nanos = created_at_time_nanos.saturating_add(1);
        created_at_time_nanos
    })
}
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferAttemptOutcome {
    Accepted(u64),
    ImmediateRetryable,
    Failed,
}

async fn transfer_once(ledger: &impl LedgerClient, arg: TransferArg) -> TransferAttemptOutcome {
    match ledger.transfer(arg).await {
        Err(_) => TransferAttemptOutcome::ImmediateRetryable,
        Ok(Ok(block)) => match u64::try_from(block.0.clone()) {
            Ok(v) => TransferAttemptOutcome::Accepted(v),
            Err(_) => TransferAttemptOutcome::Failed,
        },
        Ok(Err(TransferError::Duplicate { duplicate_of })) => match u64::try_from(duplicate_of.0.clone()) {
            Ok(v) => TransferAttemptOutcome::Accepted(v),
            Err(_) => TransferAttemptOutcome::Failed,
        },
        Ok(Err(TransferError::TemporarilyUnavailable)) => TransferAttemptOutcome::ImmediateRetryable,
        Ok(Err(_)) => TransferAttemptOutcome::Failed,
    }
}

async fn notify_once(cmc: &impl CmcClient, pending: &PendingNotification) -> bool {
    increment_cmc_attempts();
    cmc.notify_top_up(pending.beneficiary, pending.block_index).await.is_ok()
}
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
    let summary = state::with_state_mut(|st| {
        let Some(job) = st.active_payout_job.take() else { return None; };
        apply_job_health_observations(st, &job);
        let summary = logic::summary_from_job(&job);
        st.last_summary = Some(summary.clone());
        Some(summary)
    });
    if let Some(summary) = summary.as_ref() {
        log_summary(summary);
    }
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

async fn send_and_notify(
    ledger: &impl LedgerClient,
    cmc: &impl CmcClient,
    pending: PendingNotification,
    to: Account,
    fee_e8s: u64,
    now_nanos: u64,
    now_secs: u64,
) {
    let created_at_time_nanos = allocate_created_at_time_nanos(now_nanos);
    let first_arg = transfer_arg(to.clone(), pending.amount_e8s, fee_e8s, created_at_time_nanos);
    let second_arg = transfer_arg(to, pending.amount_e8s, fee_e8s, created_at_time_nanos);

    let block_index = match transfer_once(ledger, first_arg).await {
        TransferAttemptOutcome::Accepted(v) => v,
        TransferAttemptOutcome::ImmediateRetryable => match transfer_once(ledger, second_arg).await {
            TransferAttemptOutcome::Accepted(v) => v,
            TransferAttemptOutcome::ImmediateRetryable | TransferAttemptOutcome::Failed => {
                increment_failed_topups();
                return;
            }
        },
        TransferAttemptOutcome::Failed => {
            increment_failed_topups();
            return;
        }
    };

    let accepted = PendingNotification { block_index, ..pending };
    record_transfer_outflow(&accepted);

    if !notify_once(cmc, &accepted).await {
        if !notify_once(cmc, &accepted).await {
            increment_failed_topups();
            return;
        }
    }
    record_successful_notification(now_secs, &accepted);
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
}


fn note_cmc_run_result(start_attempts: u64, start_successes: u64, end_attempts: u64, end_successes: u64) {
    let attempts = end_attempts.saturating_sub(start_attempts);
    let successes = end_successes.saturating_sub(start_successes);
    if attempts == 0 {
        return;
    }
    state::with_state_mut(|st| apply_cmc_run_result(st, attempts, successes));
}

fn current_job_cmc_counts() -> (u64, u64) {
    state::with_state(|st| {
        st.active_payout_job
            .as_ref()
            .map(|job| (job.cmc_attempt_count.unwrap_or(0), job.cmc_success_count.unwrap_or(0)))
            .unwrap_or((0, 0))
    })
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

    let start_cmc = state::with_state(|st| {
        st.active_payout_job
            .as_ref()
            .map(|job| (job.cmc_attempt_count.unwrap_or(0), job.cmc_success_count.unwrap_or(0)))
            .unwrap_or((0, 0))
    });


    loop {
        let job = state::with_state(|st| st.active_payout_job.clone());
        let Some(job) = job else { maybe_latch_bootstrap_rescue(now_secs); return true; };
        if job.scan_complete {
            let remainder_gross_e8s = job.pot_start_e8s.saturating_sub(job.gross_outflow_e8s);
            if remainder_gross_e8s > job.fee_e8s && job.remainder_to_self_e8s == 0 {
                let self_id = self_canister_principal();
                let pending = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: remainder_gross_e8s, amount_e8s: remainder_gross_e8s.saturating_sub(job.fee_e8s), block_index: 0, next_start: None };
                let to = deposit_account_for_pending(cfg.cmc_canister_id, &pending);
                send_and_notify(ledger, cmc, pending, to, job.fee_e8s, now_nanos, now_secs).await;
            }
            let (end_attempts, end_successes) = current_job_cmc_counts();
            note_cmc_run_result(start_cmc.0, start_cmc.1, end_attempts, end_successes);
            finalize_completed_job();
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        }

        let resp = match index.get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE).await {
            Ok(v) => v,
            Err(_) => {
                let (end_attempts, end_successes) = current_job_cmc_counts();
                note_cmc_run_result(start_cmc.0, start_cmc.1, end_attempts, end_successes);
                return false;
            }
        };
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
                    // Best-effort policy: attempt each eligible top-up independently, allow at most
                    // one immediate inline retry at ambiguous ledger / notify boundaries, and then
                    // continue streaming the remaining contributions without persisting deferred retry state.
                    let pending = PendingNotification { kind: TransferKind::Beneficiary, beneficiary, gross_share_e8s, amount_e8s, block_index: 0, next_start: Some(tx.id) };
                    let to = deposit_account_for_pending(cfg.cmc_canister_id, &pending);
                    send_and_notify(ledger, cmc, pending, to, snapshot.2, now_nanos, now_secs).await;
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
    let (blackhole_armed, blackhole_controller, last_xfer_opt, rescue_controller, forced_reason) = state::with_state(|st| {
        (
            st.config.blackhole_armed.unwrap_or(false),
            st.config.blackhole_controller,
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.forced_rescue_reason.clone(),
        )
    });
    if !blackhole_armed { return; }
    let Some(blackhole_controller) = blackhole_controller else {
        log_error(3107);
        return;
    };
    let self_id = self_canister_principal();
    let mut desired = if forced_reason.is_some() {
        vec![blackhole_controller, rescue_controller, self_id]
    } else {
        let Some(desired) = policy::desired_controllers(now_secs, last_xfer_opt, self_id, Some(blackhole_controller), rescue_controller) else { return; };
        desired
    };
    desired.sort_by(|a: &Principal, b: &Principal| a.to_text().cmp(&b.to_text()));
    desired.dedup();
    let rescue_active = desired.iter().any(|p| *p == rescue_controller);
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
    use crate::clients::index::{GetAccountIdentifierTransactionsResponse, IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId, Tokens, account_identifier_text};
    use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferError};
    use std::collections::VecDeque;
    use std::future::{pending, Future};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};


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

    #[derive(Clone)]
    enum LedgerStep {
        Ok(u64),
        Duplicate(u64),
        TemporarilyUnavailable,
        CallErr,
        PermanentErr,
    }

    struct ScriptedLedger {
        steps: Arc<Mutex<VecDeque<LedgerStep>>>,
        transfer_calls: Arc<AtomicUsize>,
        created_at_times: Arc<Mutex<Vec<Option<u64>>>>,
    }

    impl ScriptedLedger {
        fn new(steps: Vec<LedgerStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
                transfer_calls: Arc::new(AtomicUsize::new(0)),
                created_at_times: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn transfer_calls(&self) -> usize {
            self.transfer_calls.load(Ordering::SeqCst)
        }

        fn created_at_times(&self) -> Vec<Option<u64>> {
            self.created_at_times.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LedgerClient for ScriptedLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { panic!("fee_e8s should not be called") }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { panic!("balance_of_e8s should not be called") }
        async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            self.created_at_times.lock().unwrap().push(arg.created_at_time);
            match self.steps.lock().unwrap().pop_front().expect("missing ledger step") {
                LedgerStep::Ok(block) => Ok(Ok(BlockIndex::from(block))),
                LedgerStep::Duplicate(block) => Ok(Err(TransferError::Duplicate { duplicate_of: BlockIndex::from(block) })),
                LedgerStep::TemporarilyUnavailable => Ok(Err(TransferError::TemporarilyUnavailable)),
                LedgerStep::CallErr => Err(crate::clients::ClientError::Call("scripted ledger transport failure".to_string())),
                LedgerStep::PermanentErr => Ok(Err(TransferError::BadFee { expected_fee: 10_000u64.into() })),
            }
        }
    }

    #[derive(Clone)]
    enum CmcStep {
        Ok,
        Err,
    }

    struct ScriptedCmc {
        steps: Arc<Mutex<VecDeque<CmcStep>>>,
        calls: Arc<AtomicUsize>,
    }

    impl ScriptedCmc {
        fn new(steps: Vec<CmcStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl CmcClient for ScriptedCmc {
        async fn notify_top_up(&self, _canister_id: Principal, _block_index: u64) -> Result<(), crate::clients::ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.steps.lock().unwrap().pop_front().expect("missing cmc step") {
                CmcStep::Ok => Ok(()),
                CmcStep::Err => Err(crate::clients::ClientError::Call("scripted cmc failure".to_string())),
            }
        }
    }

    struct ExclusiveIndex {
        txs: Vec<IndexTransactionWithId>,
        starts: Arc<Mutex<Vec<Option<u64>>>>,
    }

    impl ExclusiveIndex {
        fn new(txs: Vec<IndexTransactionWithId>) -> Self {
            Self { txs, starts: Arc::new(Mutex::new(Vec::new())) }
        }

        fn starts(&self) -> Vec<Option<u64>> {
            self.starts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for ExclusiveIndex {
        async fn get_account_identifier_transactions(
            &self,
            account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            self.starts.lock().unwrap().push(start);
            let start_idx = match start {
                None => 0,
                Some(last_seen) => self.txs.iter().position(|t| t.id == last_seen).map(|i| i + 1).unwrap_or(self.txs.len()),
            };
            let mut out = Vec::new();
            for tx in self.txs[start_idx..].iter() {
                let include = matches!(&tx.transaction.operation, IndexOperation::Transfer { to, .. } if to == &account_identifier);
                if include {
                    out.push(tx.clone());
                }
                if out.len() >= max_results as usize {
                    break;
                }
            }
            Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: self.txs.first().map(|tx| tx.id),
                transactions: out,
            })
        }
    }

    fn contribution_tx(id: u64, staking_id: &str, amount_e8s: u64, memo: Option<Vec<u8>>) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: memo,
                operation: IndexOperation::Transfer {
                    to: staking_id.to_string(),
                    fee: Tokens::new(10_000),
                    from: "mock-sender".to_string(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos: 0 }),
            },
        }
    }

    fn test_config_with_intervals(main_interval_seconds: u64, rescue_interval_seconds: u64) -> state::Config {
        state::Config {
            staking_account: Account { owner: Principal::anonymous(), subaccount: None },
            payout_subaccount: None,
            ledger_canister_id: Principal::anonymous(),
            index_canister_id: Principal::anonymous(),
            cmc_canister_id: Principal::anonymous(),
            rescue_controller: Principal::anonymous(),
            blackhole_controller: Some(Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").unwrap()),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: None,
            main_interval_seconds,
            rescue_interval_seconds,
            min_tx_e8s: 1,
        }
    }

    fn test_config() -> state::Config {
        test_config_with_intervals(60, 60)
    }

    fn set_active_job(now_secs: u64, job: ActivePayoutJob) -> state::Config {
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.active_payout_job = Some(job);
        state::set_state(st);
        cfg
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

    fn run_ready<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        match poll_once(future.as_mut()) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn take_test_logs() -> Vec<String> {
        TEST_LOG_LINES.with(|logs| std::mem::take(&mut *logs.borrow_mut()))
    }

    #[test]
    fn main_lock_prevents_overlap_after_old_lease_window_passes() {
        let now_secs = 1_000_u64;
        let mut st = state::State::new(test_config(), now_secs);
        let mut job = ActivePayoutJob::new(1, 10_000, 1_000_000, 2_000_000, now_secs * 1_000_000_000);
        job.scan_complete = true;
        st.active_payout_job = Some(job);
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(7)]);
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

    #[test]
    fn immediate_transfer_retry_reuses_created_at_time_and_succeeds_inline() {
        let now_secs = 1_000_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(1, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("aaaaa-aa").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(91),
            LedgerStep::Ok(92),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "first contribution should retry inline once and the job should still send the remainder");
        let created_at_times = ledger.created_at_times();
        assert_eq!(created_at_times.len(), 3);
        assert_eq!(created_at_times[0], created_at_times[1], "immediate retry must reuse the original transfer identity");
        assert_ne!(created_at_times[1], created_at_times[2], "later transfers should allocate fresh created_at_time values");
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_990_000);
        assert_eq!(summary.remainder_to_self_e8s, 74_990_000);
        assert_eq!(summary.failed_topups, 0);
    }

    #[test]
    fn immediate_transfer_retry_duplicate_is_treated_as_success() {
        let now_secs = 1_250_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(2, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("aaaaa-aa").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Duplicate(91),
            LedgerStep::Ok(92),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "duplicate-on-retry should still allow the same job to send the remainder");
        let created_at_times = ledger.created_at_times();
        assert_eq!(created_at_times.len(), 3);
        assert_eq!(created_at_times[0], created_at_times[1], "duplicate retry must reuse the original transfer identity");
        assert_ne!(created_at_times[1], created_at_times[2], "remainder transfer should get its own created_at_time");
        assert_eq!(cmc.call_count(), 2);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 74_990_000);
    }

    #[test]
    fn immediate_transfer_retry_failure_counts_once_and_moves_on() {
        let now_secs = 1_500_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(5, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary_a = Principal::from_text("aaaaa-aa").unwrap();
        let beneficiary_b = Principal::from_text("2vxsx-fae").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 50_000_000, Some(beneficiary_a.to_text().into_bytes())),
            contribution_tx(11, &staking_id, 60_000_000, Some(beneficiary_b.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(191),
            LedgerStep::Ok(192),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 4);
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 29_990_000);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 69_990_000);
    }

    #[test]
    fn transport_failure_retry_exhaustion_counts_once_and_sends_remainder() {
        let now_secs = 1_600_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(6, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("aaaaa-aa").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::CallErr,
            LedgerStep::CallErr,
            LedgerStep::Ok(291),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "beneficiary should get one immediate retry and then the remainder should still be sent");
        assert_eq!(cmc.call_count(), 1);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn deterministic_ledger_failure_does_not_retry_and_sends_remainder() {
        let now_secs = 1_700_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(7, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("aaaaa-aa").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::PermanentErr,
            LedgerStep::Ok(391),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "deterministic ledger rejection should not trigger an immediate retry");
        assert_eq!(cmc.call_count(), 1);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn immediate_notify_retry_does_not_repeat_ledger_transfer() {
        let now_secs = 3_000_u64;
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(3, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job
        });
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(55)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Err, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1, "notify retry must not resend the ledger transfer");
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after inline retry");
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 0);
    }

    #[test]
    fn immediate_notify_retry_failure_counts_once_and_finalizes() {
        let now_secs = 4_000_u64;
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(4, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job
        });
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(88)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Err, CmcStep::Err]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after retry exhaustion");
        assert_eq!(summary.remainder_to_self_e8s, 0);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn summary_logging_emits_one_compact_line_without_per_transfer_noise() {
        let now_secs = 4_100_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(8, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary_a = Principal::from_text("aaaaa-aa").unwrap();
        let beneficiary_b = Principal::from_text("2vxsx-fae").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 50_000_000, Some(beneficiary_a.to_text().into_bytes())),
            contribution_tx(11, &staking_id, 60_000_000, Some(beneficiary_b.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(491),
            LedgerStep::Ok(492),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        take_test_logs();
        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        let logs = take_test_logs();
        assert_eq!(logs.len(), 1, "expected exactly one compact summary log line, got {logs:?}");
        let summary = &logs[0];
        assert!(summary.starts_with("SUMMARY:"), "expected summary log prefix, got {summary}");
        assert!(summary.contains("topped_up_count=1"));
        assert!(summary.contains("failed_topups=1"));
        assert!(summary.contains("remainder_to_self_e8s=69990000"));
        assert!(!summary.contains("ERR:"));
        assert!(!summary.contains("TOPUP"));
    }

    #[test]
    fn debug_runtime_reset_and_inline_retry_leave_no_persisted_retry_footprint() {
        let now_secs = 4_200_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(9, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("aaaaa-aa").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(591),
            LedgerStep::Ok(592),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        let (active_job_present, summary_present) = state::with_state(|st| (st.active_payout_job.is_some(), st.last_summary.is_some()));
        assert!(!active_job_present, "inline retry flow should not leave an active job behind once complete");
        assert!(summary_present, "completed job should finalize exactly one summary");
    }

    #[test]
    fn scan_latest_tx_id_uses_exclusive_start_cursor_contract() {
        let cfg = test_config();
        let staking_id = account_identifier_text(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 1, None),
            contribution_tx(11, &staking_id, 1, None),
            contribution_tx(12, &staking_id, 1, None),
        ]);

        let latest = run_ready(scan_latest_tx_id(&index, staking_id, Some(10)));
        assert_eq!(latest, Some(12));
        assert_eq!(index.starts(), vec![Some(10)]);
    }

    #[test]
    fn remainder_duplicate_still_notifies_and_finalizes_summary() {
        let now_secs = 2_000_u64;
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(2, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job
        });
        let ledger = ScriptedLedger::new(vec![LedgerStep::Duplicate(77)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() { main_tick(true).await; }
#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() { rescue_tick().await; }
