use candid::{Nat, Principal};
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::time::Duration;

const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;

use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::index::{account_identifier_text, GetAccountIdentifierTransactionsResponse, IcpIndexCanister};
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{CmcClient, IndexClient, LedgerClient};
use crate::state::{
    ActivePayoutJob, ForcedRescueReason, PendingNotification, PendingTransfer, PendingTransferPhase,
    TransferKind,
};
use crate::{logic, policy, state};

const PAGE_SIZE: u64 = 500;
const LEDGER_CREATED_AT_MAX_AGE_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;
const LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS: u64 = 60 * 1_000_000_000;

#[cfg(feature = "debug_api")]
use std::cell::RefCell;

#[cfg(feature = "debug_api")]
thread_local! {
    static DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK: RefCell<u32> = RefCell::new(0);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_real_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
fn debug_reset_successful_transfer_counter() {
    DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|v| *v.borrow_mut() = 0);
}

#[cfg(feature = "debug_api")]
enum DebugSuccessfulTransferInjection {
    None,
    Abort,
    Trap,
}

#[cfg(feature = "debug_api")]
fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
    let abort_after_n = DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow());
    let trap_after_n = DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow());
    if abort_after_n.is_none() && trap_after_n.is_none() {
        return DebugSuccessfulTransferInjection::None;
    }

    let count = DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|c| {
        let mut c = c.borrow_mut();
        *c = c.saturating_add(1);
        *c
    });

    if trap_after_n == Some(count) {
        return DebugSuccessfulTransferInjection::Trap;
    }
    if abort_after_n == Some(count) {
        return DebugSuccessfulTransferInjection::Abort;
    }
    DebugSuccessfulTransferInjection::None
}

#[cfg(not(feature = "debug_api"))]
fn debug_reset_successful_transfer_counter() {}

#[cfg(not(feature = "debug_api"))]
enum DebugSuccessfulTransferInjection {
    None,
}

#[cfg(not(feature = "debug_api"))]
fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
    DebugSuccessfulTransferInjection::None
}

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
        "SUMMARY:topped_up_count={} failed_topups={} ambiguous_topups={} ignored_under_threshold={} ignored_bad_memo={} remainder_to_self_e8s={} pot_remaining_e8s={}",
        summary.topped_up_count,
        summary.failed_topups,
        summary.ambiguous_topups,
        summary.ignored_under_threshold,
        summary.ignored_bad_memo,
        summary.remainder_to_self_e8s,
        summary.pot_remaining_e8s
    ));
}

struct MainGuard {
    active: bool,
    lease_expires_at_ts: u64,
}

impl MainGuard {
    fn acquire(now_secs: u64) -> Option<Self> {
        state::with_state_mut(|st| {
            let lock_expires_at_ts = st.main_lock_expires_at_ts.unwrap_or(0);
            if lock_expires_at_ts > now_secs {
                return None;
            }
            let lease_expires_at_ts = now_secs.saturating_add(MAIN_TICK_LEASE_SECONDS);
            st.main_lock_expires_at_ts = Some(lease_expires_at_ts);
            Some(Self { active: true, lease_expires_at_ts })
        })
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            if st.main_lock_expires_at_ts == Some(lease_expires_at_ts) {
                st.main_lock_expires_at_ts = Some(0);
            }
        });
        self.active = false;
    }

    fn finish(mut self, now_secs: u64, err: Option<u32>) {
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            if st.main_lock_expires_at_ts == Some(lease_expires_at_ts) {
                st.main_lock_expires_at_ts = Some(0);
            }
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

pub fn schedule_immediate_resume_if_needed() {
    let has_active_job = state::with_state(|st| st.active_payout_job.is_some());
    if !has_active_job {
        return;
    }
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        main_tick(true).await;
    });
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
fn increment_cmc_attempts(pending: &PendingNotification) {
    if pending.kind != TransferKind::Beneficiary {
        return;
    }
    state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() {
        job.cmc_attempt_count = Some(job.cmc_attempt_count.unwrap_or(0).saturating_add(1));
    });
}

fn increment_cmc_successes(pending: &PendingNotification) {
    if pending.kind != TransferKind::Beneficiary {
        return;
    }
    state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() {
        job.cmc_success_count = Some(job.cmc_success_count.unwrap_or(0).saturating_add(1));
    });
}

fn current_pending_transfer() -> Option<PendingTransfer> {
    state::with_state(|st| st.active_payout_job.as_ref().and_then(|job| job.pending_transfer.clone()))
}

fn stage_pending_transfer(pending: PendingNotification, created_at_time_nanos: u64) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            job.pending_transfer = Some(PendingTransfer {
                notification: pending,
                created_at_time_nanos,
                phase: PendingTransferPhase::AwaitingTransfer,
            });
        }
    });
}

fn mark_pending_transfer_accepted(block_index: u64) -> Option<PendingNotification> {
    state::with_state_mut(|st| {
        let job = st.active_payout_job.as_mut()?;
        let pending = job.pending_transfer.as_mut()?;
        pending.notification.block_index = block_index;
        pending.phase = PendingTransferPhase::TransferAccepted;
        let accepted = pending.notification.clone();
        logic::record_ledger_accepted_transfer(job, &accepted);
        Some(accepted)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingTransferTerminalStatus {
    Failed,
    Ambiguous,
}

fn clear_pending_transfer(status: PendingTransferTerminalStatus) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            if matches!(job.pending_transfer.as_ref().map(|pending| &pending.notification.kind), Some(TransferKind::Beneficiary)) {
                match status {
                    PendingTransferTerminalStatus::Failed => {
                        job.failed_topups = job.failed_topups.saturating_add(1);
                    }
                    PendingTransferTerminalStatus::Ambiguous => {
                        job.ambiguous_topups = job.ambiguous_topups.saturating_add(1);
                    }
                }
            }
            // Remainder-to-self top-ups are intentional best-effort cleanup only. They are not
            // counted as beneficiary failures or ambiguities and should not bias rescue / summary policy.
            job.pending_transfer = None;
        }
    });
}
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotifyAttemptOutcome {
    Succeeded,
    Retryable,
    Terminal,
}

async fn notify_once(cmc: &impl CmcClient, pending: &PendingNotification) -> NotifyAttemptOutcome {
    increment_cmc_attempts(pending);
    match cmc.notify_top_up(pending.beneficiary, pending.block_index).await {
        Ok(()) => NotifyAttemptOutcome::Succeeded,
        Err(crate::clients::ClientError::TerminalNotify(_)) => NotifyAttemptOutcome::Terminal,
        Err(crate::clients::ClientError::RetryableNotify(_))
        | Err(crate::clients::ClientError::Call(_))
        | Err(crate::clients::ClientError::Convert(_)) => NotifyAttemptOutcome::Retryable,
    }
}
fn record_successful_notification(now_secs: u64, pending: &PendingNotification) {
    state::with_state_mut(|st| {
        st.last_successful_transfer_ts = Some(now_secs);
        if let Some(job) = st.active_payout_job.as_mut() {
            logic::apply_notified_transfer(job, pending);
            job.pending_transfer = None;
        }
    });
    increment_cmc_successes(pending);
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
    let memo_bytes = logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec();
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

async fn drive_pending_transfer(
    ledger: &impl LedgerClient,
    cmc: &impl CmcClient,
    cmc_id: Principal,
    fee_e8s: u64,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
    let Some(staged) = current_pending_transfer() else { return true; };

    let accepted = match staged.phase {
        PendingTransferPhase::AwaitingTransfer => {
            if !created_at_time_is_valid_for_ledger(staged.created_at_time_nanos, now_nanos) {
                // Once the created_at_time expires we can no longer safely distinguish “never accepted”
                // from “accepted but the reply was lost”, so we surface this as ambiguous rather than failed.
                clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                return true;
            }

            let to = deposit_account_for_pending(cmc_id, &staged.notification);
            let first_arg = transfer_arg(
                to.clone(),
                staged.notification.amount_e8s,
                fee_e8s,
                staged.created_at_time_nanos,
            );
            let second_arg = transfer_arg(
                to,
                staged.notification.amount_e8s,
                fee_e8s,
                staged.created_at_time_nanos,
            );

            let block_index = match transfer_once(ledger, first_arg).await {
                TransferAttemptOutcome::Accepted(v) => v,
                TransferAttemptOutcome::ImmediateRetryable => match transfer_once(ledger, second_arg).await {
                    TransferAttemptOutcome::Accepted(v) => v,
                    TransferAttemptOutcome::ImmediateRetryable | TransferAttemptOutcome::Failed => {
                        clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                        return true;
                    }
                },
                TransferAttemptOutcome::Failed => {
                    clear_pending_transfer(PendingTransferTerminalStatus::Failed);
                    return true;
                }
            };

            match debug_successful_transfer_injection() {
                DebugSuccessfulTransferInjection::None => {}
                #[cfg(feature = "debug_api")]
                DebugSuccessfulTransferInjection::Abort => return false,
                #[cfg(feature = "debug_api")]
                DebugSuccessfulTransferInjection::Trap => ic_cdk::trap("debug trap after successful faucet transfer"),
            };

            match mark_pending_transfer_accepted(block_index) {
                Some(accepted) => accepted,
                None => return true,
            }
        }
        PendingTransferPhase::TransferAccepted => staged.notification,
    };

    let first_notify = notify_once(cmc, &accepted).await;
    match first_notify {
        NotifyAttemptOutcome::Succeeded => {
            record_successful_notification(now_secs, &accepted);
            true
        }
        NotifyAttemptOutcome::Retryable | NotifyAttemptOutcome::Terminal => {
            // Once the ledger transfer is accepted, a duplicate-safe notify retry can improve the
            // final classification without risking an extra outflow. Two terminal replies mean the
            // beneficiary top-up deterministically failed; any transport/retryable uncertainty left
            // after the single inline retry is surfaced as ambiguous.
            match notify_once(cmc, &accepted).await {
                NotifyAttemptOutcome::Succeeded => {
                    record_successful_notification(now_secs, &accepted);
                    true
                }
                NotifyAttemptOutcome::Terminal if matches!(first_notify, NotifyAttemptOutcome::Terminal) => {
                    clear_pending_transfer(PendingTransferTerminalStatus::Failed);
                    true
                }
                NotifyAttemptOutcome::Retryable | NotifyAttemptOutcome::Terminal => {
                    clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                    true
                }
            }
        }
    }
}

async fn send_and_notify(
    ledger: &impl LedgerClient,
    cmc: &impl CmcClient,
    pending: PendingNotification,
    fee_e8s: u64,
    now_nanos: u64,
    now_secs: u64,
    cmc_id: Principal,
) -> bool {
    let created_at_time_nanos = allocate_created_at_time_nanos(now_nanos);
    stage_pending_transfer(pending, created_at_time_nanos);
    drive_pending_transfer(ledger, cmc, cmc_id, fee_e8s, now_nanos, now_secs).await
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
    let prev_balance = state::with_state(|st| st.last_observed_staking_balance_e8s);
    let prev_latest = state::with_state(|st| st.last_observed_latest_tx_id);
    let first_page = match index
        .get_account_identifier_transactions(staking_id.to_string(), None, 1)
        .await
    {
        Ok(resp) => resp,
        Err(_) => {
            state::with_state_mut(|st| {
                if prev_balance.is_none() {
                    st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
                    st.last_observed_latest_tx_id = None;
                    st.consecutive_index_latest_invariant_failures = Some(0);
                    st.consecutive_index_latest_unreadable_failures = Some(0);
                } else if prev_balance != Some(denom_balance_e8s) {
                    apply_latest_observation(st, denom_balance_e8s, LatestScan::Unreadable);
                }
            });
            return;
        }
    };

    state::with_state_mut(|st| apply_anchor_observation(st, first_page.oldest_tx_id));

    if prev_balance.is_none() {
        let latest_tx_id = scan_latest_tx_id(index, staking_id.to_string(), None).await;
        state::with_state_mut(|st| {
            st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
            st.last_observed_latest_tx_id = match latest_tx_id {
                LatestScan::Read(latest_tx_id) => latest_tx_id,
                LatestScan::Unreadable => None,
            };
            st.consecutive_index_latest_invariant_failures = Some(0);
            st.consecutive_index_latest_unreadable_failures = Some(0);
        });
        return;
    }

    if prev_balance == Some(denom_balance_e8s) {
        return;
    }

    let latest_tx_id = scan_latest_tx_id(index, staking_id.to_string(), prev_latest).await;
    state::with_state_mut(|st| apply_latest_observation(st, denom_balance_e8s, latest_tx_id));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LatestScan {
    Read(Option<u64>),
    Unreadable,
}

async fn scan_latest_tx_id(index: &impl IndexClient, staking_id: String, start: Option<u64>) -> LatestScan {
    let mut cursor = start;
    let mut latest = start;
    loop {
        let resp = match index
            .get_account_identifier_transactions(staking_id.clone(), cursor, PAGE_SIZE)
            .await
        {
            Ok(resp) => resp,
            Err(_) => return LatestScan::Unreadable,
        };
        let page_latest = resp.transactions.last().map(|tx| tx.id);
        if page_latest.is_none() {
            return LatestScan::Read(latest);
        }
        latest = page_latest;
        cursor = page_latest;
        if resp.transactions.len() < PAGE_SIZE as usize {
            return LatestScan::Read(latest);
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

fn apply_latest_observation(st: &mut state::State, denom_balance_e8s: u64, latest_scan: LatestScan) {
    match (st.last_observed_staking_balance_e8s, st.last_observed_latest_tx_id) {
        (None, _) => {
            st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
            st.last_observed_latest_tx_id = match latest_scan {
                LatestScan::Read(latest_tx_id) => latest_tx_id,
                LatestScan::Unreadable => None,
            };
            st.consecutive_index_latest_invariant_failures = Some(0);
            st.consecutive_index_latest_unreadable_failures = Some(0);
        }
        (Some(prev_balance), _prev_latest_tx_id) if prev_balance == denom_balance_e8s => {}
        (Some(_), prev_latest_tx_id) => match latest_scan {
            LatestScan::Unreadable => {
                st.consecutive_index_latest_invariant_failures = Some(0);
                st.consecutive_index_latest_unreadable_failures = Some(
                    st.consecutive_index_latest_unreadable_failures
                        .unwrap_or(0)
                        .saturating_add(1),
                );
                if st.consecutive_index_latest_unreadable_failures.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
                    st.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestUnreadable);
                }
            }
            LatestScan::Read(latest_tx_id) => {
                let latest_changed = match (prev_latest_tx_id, latest_tx_id) {
                    (Some(prev), Some(latest)) => latest != prev,
                    (None, Some(_)) => true,
                    (_, None) => false,
                };
                if latest_changed {
                    st.last_observed_staking_balance_e8s = Some(denom_balance_e8s);
                    st.last_observed_latest_tx_id = latest_tx_id;
                    st.consecutive_index_latest_invariant_failures = Some(0);
                    st.consecutive_index_latest_unreadable_failures = Some(0);
                } else {
                    st.consecutive_index_latest_unreadable_failures = Some(0);
                    st.consecutive_index_latest_invariant_failures = Some(
                        st.consecutive_index_latest_invariant_failures
                            .unwrap_or(0)
                            .saturating_add(1),
                    );
                    if st.consecutive_index_latest_invariant_failures.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
                        st.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestInvariantBroken);
                    }
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
    apply_latest_observation(st, job.denom_staking_balance_e8s, LatestScan::Read(job.observed_latest_tx_id));
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

    loop {
        let job = state::with_state(|st| st.active_payout_job.clone());
        let Some(job) = job else { maybe_latch_bootstrap_rescue(now_secs); return true; };
        if job.pending_transfer.is_some() {
            if !drive_pending_transfer(ledger, cmc, cfg.cmc_canister_id, job.fee_e8s, now_nanos, now_secs).await {
                return true;
            }
            continue;
        }
        if job.scan_complete {
            let remainder_gross_e8s = job.pot_start_e8s.saturating_sub(job.gross_outflow_e8s);
            if remainder_gross_e8s > job.fee_e8s && job.remainder_to_self_e8s == 0 {
                let self_id = self_canister_principal();
                let pending = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: remainder_gross_e8s, amount_e8s: remainder_gross_e8s.saturating_sub(job.fee_e8s), block_index: 0, next_start: None };
                if !send_and_notify(ledger, cmc, pending, job.fee_e8s, now_nanos, now_secs, cfg.cmc_canister_id).await {
                    return true;
                }
            }
            finalize_completed_job();
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        }

        let resp = match index.get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        note_index_page(&resp);
        if resp.transactions.is_empty() {
            state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.scan_complete = true; });
            continue;
        }

        let page_start = job.next_start;
        for tx in &resp.transactions {
            if let Some(last_seen) = page_start {
                if tx.id <= last_seen {
                    // Faucet history scans intentionally trust the ICP index cursor contract to be
                    // monotonic and exclusive across pages. Duplicate page-boundary delivery would
                    // be treated as an upstream indexing issue rather than something this single-pass
                    // scan tries to compensate for internally.
                    continue;
                }
            }
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
                    // one immediate inline retry at the accepted-ledger / notify boundary, and persist
                    // only the single in-flight transfer/notify phase so upgrades can resume safely.
                    // Exhausted terminal notify paths count as deterministic failures; any remaining
                    // transport/retryable uncertainty after the one safe retry is surfaced separately
                    // as ambiguous so operators can reconcile only the truly unknown cases.
                    let pending = PendingNotification { kind: TransferKind::Beneficiary, beneficiary, gross_share_e8s, amount_e8s, block_index: 0, next_start: Some(tx.id) };
                    if !send_and_notify(ledger, cmc, pending, snapshot.2, now_nanos, now_secs, cfg.cmc_canister_id).await {
                        return true;
                    }
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
        RetryableErr,
        TerminalErr,
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
                CmcStep::RetryableErr => Err(crate::clients::ClientError::Call("scripted cmc failure".to_string())),
                CmcStep::TerminalErr => Err(crate::clients::ClientError::TerminalNotify("scripted terminal cmc failure".to_string())),
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


    #[derive(Clone)]
    enum IndexResponseStep {
        Ok(GetAccountIdentifierTransactionsResponse),
        Err,
    }

    struct ScriptedIndex {
        steps: Arc<Mutex<VecDeque<IndexResponseStep>>>,
    }

    impl ScriptedIndex {
        fn new(steps: Vec<IndexResponseStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
            }
        }
    }

    #[async_trait]
    impl IndexClient for ScriptedIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            match self.steps.lock().unwrap().pop_front().expect("missing index step") {
                IndexResponseStep::Ok(resp) => Ok(resp),
                IndexResponseStep::Err => Err(crate::clients::ClientError::Call("scripted index failure".to_string())),
            }
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
            staking_account: Account { owner: Principal::management_canister(), subaccount: None },
            payout_subaccount: None,
            ledger_canister_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
            index_canister_id: Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").unwrap(),
            cmc_canister_id: Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap(),
            rescue_controller: Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
            blackhole_controller: Some(Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").unwrap()),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: None,
            main_interval_seconds,
            rescue_interval_seconds,
            min_tx_e8s: crate::MIN_MIN_TX_E8S,
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
    fn stale_main_lease_can_be_reclaimed_without_old_guard_clearing_the_new_lease() {
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
        assert_eq!(
            state::with_state(|st| st.main_lock_expires_at_ts),
            Some(now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        let second_now_secs = now_secs + MAIN_TICK_LEASE_SECONDS + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(false, second_now_nanos, second_now_secs, &ledger, &index, &cmc));
        assert!(matches!(poll_once(fut2.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            state::with_state(|st| st.main_lock_expires_at_ts),
            Some(second_now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        drop(fut1);
        assert_eq!(
            state::with_state(|st| st.main_lock_expires_at_ts),
            Some(second_now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        drop(fut2);
        assert_eq!(state::with_state(|st| st.main_lock_expires_at_ts), Some(0));
    }

    #[test]
    fn transfer_arg_uses_little_endian_top_up_memo() {
        state::set_state(state::State::new(test_config(), 0));
        let arg = transfer_arg(
            Account { owner: Principal::management_canister(), subaccount: Some([7u8; 32]) },
            123_456_789,
            10_000,
            42,
        );
        let memo = arg.memo.expect("memo should be present");
        assert_eq!(memo.0, logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec());
    }

    #[test]
    fn immediate_transfer_retry_reuses_created_at_time_and_succeeds_inline() {
        let now_secs = 1_000_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(1, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
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
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
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
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();
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
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 69_990_000);
    }

    #[test]
    fn transport_failure_retry_exhaustion_counts_once_and_sends_remainder() {
        let now_secs = 1_600_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(6, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
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
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn retryable_then_deterministic_transfer_failure_is_still_counted_as_ambiguous() {
        let now_secs = 1_650_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(61, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::CallErr,
            LedgerStep::PermanentErr,
            LedgerStep::Ok(292),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
    }

    #[test]
    fn deterministic_ledger_failure_does_not_retry_and_sends_remainder() {
        let now_secs = 1_700_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(7, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
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
        assert_eq!(summary.ambiguous_topups, 0);
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
        let cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1, "notify retry must not resend the ledger transfer");
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after inline retry");
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
    }

    #[test]
    fn immediate_notify_retry_failure_counts_once_and_finalizes() {
        let now_secs = 4_000_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(4, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(88), LedgerStep::Ok(188)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::RetryableErr, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2);
        assert_eq!(cmc.call_count(), 3);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after retry exhaustion");
        assert_eq!(summary.remainder_to_self_e8s, 39_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn exhausted_terminal_notify_failure_counts_as_failed_after_one_safe_retry() {
        let now_secs = 4_025_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(4025, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            contribution_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(89), LedgerStep::Ok(189)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::TerminalErr, CmcStep::TerminalErr, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "exhausted terminal notify failures should skip the beneficiary and still send the remainder");
        assert_eq!(cmc.call_count(), 3, "terminal notify failures should get one safe inline retry before the remainder notify");
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after exhausted terminal notify failure");
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 39_990_000);
        assert_eq!(summary.topped_up_count, 0);
    }

    #[test]
    fn completed_job_counts_beneficiary_zero_success_once_even_if_interrupted_across_ticks() {
        let now_secs = 4_050_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        let mut job = ActivePayoutJob::new(40, 10_000, 10_000, 1, now_secs * 1_000_000_000);
        job.pending_transfer = Some(PendingTransfer {
            notification: PendingNotification {
                kind: TransferKind::Beneficiary,
                beneficiary,
                gross_share_e8s: 10_000,
                amount_e8s: 0,
                block_index: 99,
                next_start: Some(99),
            },
            created_at_time_nanos: now_secs * 1_000_000_000,
            phase: PendingTransferPhase::TransferAccepted,
        });
        st.active_payout_job = Some(job);
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![]);
        let first_tick_cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::RetryableErr]);
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);

        assert!(!run_ready(process_payout(&ledger, &index, &first_tick_cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0, "accepted pending notifications should not resend ledger transfers");
        assert_eq!(first_tick_cmc.call_count(), 2);
        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(0));
            let job = st.active_payout_job.as_ref().expect("job should remain active until it completes");
            assert!(job.pending_transfer.is_none());
            assert_eq!(job.cmc_attempt_count, Some(2));
            assert_eq!(job.cmc_success_count, Some(0));
            assert_eq!(job.failed_topups, 0);
            assert_eq!(job.ambiguous_topups, 1);
        });

        state::with_state_mut(|st| st.active_payout_job.as_mut().expect("job should still exist").scan_complete = true);
        let second_tick_cmc = ScriptedCmc::new(vec![]);
        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &second_tick_cmc, now_secs * 1_000_000_000, now_secs)));

        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(1));
            assert!(st.active_payout_job.is_none());
            assert_eq!(st.forced_rescue_reason, None);
        });
    }

    #[test]
    fn remainder_success_does_not_reset_beneficiary_zero_success_streak() {
        let now_secs = 4_060_u64;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.consecutive_cmc_zero_success_runs = Some(1);
        let mut job = ActivePayoutJob::new(41, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
        job.scan_complete = true;
        job.cmc_attempt_count = Some(2);
        job.cmc_success_count = Some(0);
        st.active_payout_job = Some(job);
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(123)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);

        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::CmcZeroSuccessRuns));
            let summary = st.last_summary.as_ref().expect("summary should be finalized");
            assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
            assert_eq!(summary.failed_topups, 0);
        });
    }

    #[test]
    fn stale_pending_transfer_is_marked_ambiguous_without_reusing_an_expired_created_at_time() {
        let now_secs = 3 * 24 * 60 * 60;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let stale_created_at_nanos = (now_secs - 2 * 24 * 60 * 60) * 1_000_000_000;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        let mut job = ActivePayoutJob::new(42, 10_000, 80_000_000, 1, stale_created_at_nanos);
        job.pending_transfer = Some(PendingTransfer {
            notification: PendingNotification {
                kind: TransferKind::Beneficiary,
                beneficiary,
                gross_share_e8s: 40_000_000,
                amount_e8s: 39_990_000,
                block_index: 0,
                next_start: Some(7),
            },
            created_at_time_nanos: stale_created_at_nanos,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        st.active_payout_job = Some(job);
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);

        assert!(!run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0, "expired created_at_time should fail before touching the ledger");
        assert_eq!(cmc.call_count(), 0);
        state::with_state(|st| {
            let job = st.active_payout_job.as_ref().expect("job should remain active for inspection");
            assert!(job.pending_transfer.is_none());
            assert_eq!(job.failed_topups, 0);
            assert_eq!(job.ambiguous_topups, 1);
        });
    }

    #[test]
    fn summary_logging_emits_one_compact_line_without_per_transfer_noise() {
        let now_secs = 4_100_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(8, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();
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
        assert!(summary.contains("failed_topups=0"));
        assert!(summary.contains("ambiguous_topups=1"));
        assert!(summary.contains("remainder_to_self_e8s=69990000"));
        assert!(!summary.contains("ERR:"));
        assert!(!summary.contains("TOPUP"));
    }


    #[test]
    fn resumes_pending_transfer_after_upgrade_boundary_before_transfer_outcome_is_known() {
        let now_secs = 3_600_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(7, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::Beneficiary,
                    beneficiary,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 0,
                    next_start: Some(10),
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::AwaitingTransfer,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![LedgerStep::Duplicate(700)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn resumes_pending_notification_after_upgrade_boundary_without_retransferring() {
        let now_secs = 3_700_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(8, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.gross_outflow_e8s = 24_990_000;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::Beneficiary,
                    beneficiary,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 701,
                    next_start: Some(10),
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::TransferAccepted,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0, "accepted transfers should resume at notify without another ledger transfer");
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn debug_runtime_reset_and_inline_retry_leave_no_persisted_retry_footprint() {
        let now_secs = 4_200_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(9, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
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
    fn overlapping_index_pages_do_not_double_count_the_last_seen_tx() {
        let now_secs = 4_300_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(10, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let first = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let second = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();

        let mut first_page = Vec::new();
        for id in 1..500u64 {
            first_page.push(contribution_tx(id, &staking_id, 1, None));
        }
        first_page.push(contribution_tx(500, &staking_id, 50_000_000, Some(first.to_text().into_bytes())));

        let second_page = vec![
            contribution_tx(500, &staking_id, 50_000_000, Some(first.to_text().into_bytes())),
            contribution_tx(501, &staking_id, 50_000_000, Some(second.to_text().into_bytes())),
        ];

        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: first_page,
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: second_page,
            }),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(601), LedgerStep::Ok(602), LedgerStep::Ok(603)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "expected two beneficiary transfers plus one self remainder transfer");
        assert_eq!(cmc.call_count(), 3);

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 2, "overlapping page replay must not duplicate the tx id 500 contribution");
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ignored_under_threshold, 499);
        assert_eq!(summary.ignored_bad_memo, 0);
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
        assert_eq!(latest, LatestScan::Read(Some(12)));
        assert_eq!(index.starts(), vec![Some(10)]);
    }

    #[test]
    fn latest_invariant_break_still_requires_two_consecutive_observations() {
        let now_secs = 5_000_u64;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::set_state(st);

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Read(Some(10))));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(1));
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(0));
            assert_eq!(st.forced_rescue_reason, None);
        });

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Read(Some(10))));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestInvariantBroken));
        });
    }

    #[test]
    fn latest_unreadable_requires_two_consecutive_observations_and_uses_distinct_reason() {
        let now_secs = 5_100_u64;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::set_state(st);

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Unreadable));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(1));
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(0));
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.last_observed_staking_balance_e8s, Some(100));
            assert_eq!(st.last_observed_latest_tx_id, Some(10));
        });

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Unreadable));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestUnreadable));
            assert_eq!(st.last_observed_staking_balance_e8s, Some(100));
            assert_eq!(st.last_observed_latest_tx_id, Some(10));
        });
    }

    #[test]
    fn first_page_unreadable_also_requires_two_consecutive_observations() {
        let now_secs = 5_150_u64;
        let cfg = test_config();
        let staking_id = account_identifier_text(&cfg.staking_account);
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Err,
            IndexResponseStep::Err,
        ]);

        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::set_state(st);

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
        });

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestUnreadable));
        });
    }

    #[test]
    fn unreadable_latest_does_not_latch_if_next_observation_confirms_advancement() {
        let now_secs = 5_200_u64;
        let cfg = test_config();
        let staking_id = account_identifier_text(&cfg.staking_account);
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 200,
                oldest_tx_id: Some(10),
                transactions: vec![contribution_tx(10, &staking_id, 100, None)],
            }),
            IndexResponseStep::Err,
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 200,
                oldest_tx_id: Some(10),
                transactions: vec![contribution_tx(10, &staking_id, 100, None)],
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 200,
                oldest_tx_id: Some(10),
                transactions: vec![contribution_tx(11, &staking_id, 100, None)],
            }),
        ]);

        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::set_state(st);

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.last_observed_staking_balance_e8s, Some(100));
            assert_eq!(st.last_observed_latest_tx_id, Some(10));
        });

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(0));
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(0));
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.last_observed_staking_balance_e8s, Some(200));
            assert_eq!(st.last_observed_latest_tx_id, Some(11));
        });
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
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() { main_tick(true).await; }
#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() { rescue_tick().await; }
