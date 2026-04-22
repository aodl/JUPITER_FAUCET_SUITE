use candid::{Nat, Principal};
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::time::Duration;

const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;

use crate::clients::canister_info::ManagementCanisterInfoClient;
use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::index::{account_identifier_text, GetAccountIdentifierTransactionsResponse, IcpIndexCanister};
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{CanisterStatusClient, CmcClient, IndexClient, LedgerClient};
use crate::state::{
    ActivePayoutJob, ForcedRescueReason, PendingNotification, PendingTransfer, PendingTransferPhase,
    SkipRange, TransferKind,
};
use crate::{logic, policy, state};


const PAGE_SIZE: u64 = 500;
const MAX_INDEX_PAGES_PER_PAYOUT_TICK: u64 = 64;
const MAX_INDEX_PAGES_PER_LATEST_SCAN: u64 = 128;
// Only persist large barren spans so the durable skip-range cache stays small and a
// one-time adversarial history scan remains much more expensive for the attacker than for
// the faucet. These ranges are only valid while contribution-classification policy is
// unchanged; if min_tx_e8s or memo-policy semantics ever change, the cache must be reset.
const MIN_SKIP_RANGE_TX_COUNT: u64 = 10_000;
const LEDGER_CREATED_AT_MAX_AGE_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;
const LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS: u64 = 60 * 1_000_000_000;
const DEFAULT_STAKE_RECOGNITION_DELAY_SECONDS: u64 = 24 * 60 * 60;

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
        "SUMMARY:topped_up_count={} failed_topups={} ambiguous_topups={} ignored_under_threshold={} ignored_bad_memo={} remainder_to_self_e8s={} pot_remaining_e8s={} effective_denom_e8s={}",
        summary.topped_up_count,
        summary.failed_topups,
        summary.ambiguous_topups,
        summary.ignored_under_threshold,
        summary.ignored_bad_memo,
        summary.remainder_to_self_e8s,
        summary.pot_remaining_e8s,
        summary.effective_denom_staking_balance_e8s.unwrap_or(summary.denom_staking_balance_e8s)
    ));
}

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

pub fn schedule_immediate_rescue_reconcile() {
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        rescue_tick().await;
    });
}

async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = IcpIndexCanister::new(cfg.index_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let status_client = ManagementCanisterInfoClient;
    run_main_tick_with_clients(force, now_nanos, now_secs, &ledger, &index, &cmc, &status_client).await;
}

async fn run_main_tick_with_clients<L: LedgerClient, I: IndexClient, C: CmcClient, S: CanisterStatusClient>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    index: &I,
    cmc: &C,
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
    let ok = process_payout(ledger, index, cmc, status_client, now_nanos, now_secs).await;
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

fn note_attempted_beneficiary(pending: &PendingNotification) {
    if pending.kind != TransferKind::Beneficiary {
        return;
    }
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            let beneficiaries = job.cmc_attempted_beneficiaries.get_or_insert_with(Vec::new);
            if !beneficiaries.iter().any(|existing| *existing == pending.beneficiary) {
                beneficiaries.push(pending.beneficiary);
            }
        }
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
    note_attempted_beneficiary(pending);
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
async fn finalize_completed_job(status_client: &impl CanisterStatusClient) {
    let Some(job) = state::with_state_mut(|st| st.active_payout_job.take()) else { return; };
    let zero_success_run_counts = zero_success_run_counts_toward_rescue(status_client, &job).await;
    let summary = state::with_state_mut(|st| {
        apply_job_health_observations(st, &job, zero_success_run_counts);
        if let Some(round_end_time_nanos) = job.round_end_time_nanos {
            st.current_round_start_time_nanos = Some(round_end_time_nanos);
            st.current_round_start_staking_balance_e8s = Some(job.denom_staking_balance_e8s);
            st.current_round_start_latest_tx_id = job.round_end_latest_tx_id.or(job.observed_latest_tx_id);
        }
        let summary = logic::summary_from_job(&job);
        st.last_summary = Some(summary.clone());
        summary
    });
    log_summary(&summary);
}

#[derive(Clone, Debug, Default)]
struct LocalSkipCandidate {
    start_tx_id: Option<u64>,
    end_tx_id: Option<u64>,
    tx_count: u64,
}

impl LocalSkipCandidate {
    fn from_job(job: &ActivePayoutJob) -> Self {
        Self {
            start_tx_id: job.skip_candidate_start_tx_id,
            end_tx_id: job.skip_candidate_end_tx_id,
            tx_count: job.skip_candidate_tx_count,
        }
    }

    fn note_skippable(&mut self, tx_id: u64) {
        if self.tx_count == 0 {
            self.start_tx_id = Some(tx_id);
            self.end_tx_id = Some(tx_id);
            self.tx_count = 1;
            return;
        }
        self.end_tx_id = Some(tx_id);
        self.tx_count = self.tx_count.saturating_add(1);
    }

    fn finish_span(&mut self) -> Option<SkipRange> {
        let range = if self.tx_count >= MIN_SKIP_RANGE_TX_COUNT {
            Some(SkipRange {
                start_tx_id: self.start_tx_id.expect("skip span start missing"),
                end_tx_id: self.end_tx_id.expect("skip span end missing"),
            })
        } else {
            None
        };
        *self = Self::default();
        range
    }
}

fn initial_skip_range_index(skip_ranges: &[SkipRange], cursor: Option<u64>) -> usize {
    let Some(last_seen) = cursor else { return 0; };
    for (idx, range) in skip_ranges.iter().enumerate() {
        if range.end_tx_id > last_seen {
            return idx;
        }
    }
    skip_ranges.len()
}

fn next_skip_jump_target(cursor: Option<u64>, skip_ranges: &[SkipRange], skip_range_idx: &mut usize) -> Option<u64> {
    let Some(last_seen) = cursor else { return None; };
    while let Some(range) = skip_ranges.get(*skip_range_idx) {
        if last_seen >= range.end_tx_id {
            *skip_range_idx += 1;
            continue;
        }
        let next_unread = last_seen.saturating_add(1);
        if next_unread >= range.start_tx_id && next_unread <= range.end_tx_id {
            return Some(range.end_tx_id);
        }
        return None;
    }
    None
}

fn record_completed_skip_range(
    skip_candidate: &mut LocalSkipCandidate,
    pending_skip_ranges: &mut Vec<SkipRange>,
) {
    if let Some(range) = skip_candidate.finish_span() {
        pending_skip_ranges.push(range);
    }
}

fn persist_new_skip_ranges(
    skip_ranges: &mut Vec<SkipRange>,
    pending_skip_ranges: &mut Vec<SkipRange>,
) -> Result<(), state::SkipRangeInsertError> {
    let mut simulated = skip_ranges.clone();
    for range in pending_skip_ranges.iter() {
        state::validate_skip_range_insertion(&simulated, range)?;
        let insert_pos = simulated.partition_point(|candidate| candidate.start_tx_id < range.start_tx_id);
        simulated.insert(insert_pos, range.clone());
    }
    for range in pending_skip_ranges.drain(..) {
        state::insert_skip_range(range.clone())?;
        let insert_pos = skip_ranges.partition_point(|candidate| candidate.start_tx_id < range.start_tx_id);
        skip_ranges.insert(insert_pos, range);
    }
    Ok(())
}

fn latch_skip_range_invariant_rescue() {
    log_error(3111);
    state::latch_skip_range_invariant_fault();
}
fn flush_scan_progress(
    ignored_under_threshold_delta: &mut u64,
    ignored_bad_memo_delta: &mut u64,
    next_start: Option<u64>,
    skip_candidate: &LocalSkipCandidate,
) {
    if *ignored_under_threshold_delta == 0
        && *ignored_bad_memo_delta == 0
        && next_start.is_none()
        && skip_candidate.tx_count == 0
    {
        return;
    }
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            job.ignored_under_threshold = job
                .ignored_under_threshold
                .saturating_add(*ignored_under_threshold_delta);
            job.ignored_bad_memo = job
                .ignored_bad_memo
                .saturating_add(*ignored_bad_memo_delta);
            if next_start.is_some() {
                job.next_start = next_start;
            }
            job.skip_candidate_start_tx_id = skip_candidate.start_tx_id;
            job.skip_candidate_end_tx_id = skip_candidate.end_tx_id;
            job.skip_candidate_tx_count = skip_candidate.tx_count;
        }
    });
    *ignored_under_threshold_delta = 0;
    *ignored_bad_memo_delta = 0;
}

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

fn recognition_delay_seconds() -> u64 {
    state::with_state(|st| st.config.stake_recognition_delay_seconds.unwrap_or(DEFAULT_STAKE_RECOGNITION_DELAY_SECONDS))
}

fn effective_denom_scan_complete(job: &ActivePayoutJob) -> bool {
    job.effective_denom_scan_complete.unwrap_or(true)
}

fn effective_denom_e8s(job: &ActivePayoutJob) -> u64 {
    job.effective_denom_staking_balance_e8s.unwrap_or(job.denom_staking_balance_e8s)
}

fn ensure_active_job(
    now_nanos: u64,
    fee_e8s: u64,
    pot_start_e8s: u64,
    denom_e8s: u64,
    round_end_latest_tx_id: Option<u64>,
) {
    state::with_state_mut(|st| {
        if st.active_payout_job.is_some() {
            return;
        }
        let id = st.payout_nonce;
        st.payout_nonce = st.payout_nonce.saturating_add(1);
        let mut job = ActivePayoutJob::new(id, fee_e8s, pot_start_e8s, denom_e8s, now_nanos);
        match (
            st.current_round_start_time_nanos,
            st.current_round_start_staking_balance_e8s,
            st.current_round_start_latest_tx_id,
        ) {
            (Some(round_start_time_nanos), Some(round_start_staking_balance_e8s), round_start_latest_tx_id) => {
                let stake_unchanged_since_round_start = round_start_staking_balance_e8s == denom_e8s;
                let effective_round_end_latest_tx_id = if stake_unchanged_since_round_start {
                    round_start_latest_tx_id
                } else {
                    round_end_latest_tx_id
                };
                job.configure_round_accounting(
                    Some(round_start_time_nanos),
                    Some(round_start_staking_balance_e8s),
                    round_start_latest_tx_id,
                    now_nanos,
                    effective_round_end_latest_tx_id,
                    round_start_staking_balance_e8s,
                    stake_unchanged_since_round_start,
                );
                if !stake_unchanged_since_round_start {
                    job.next_start = round_start_latest_tx_id;
                }
            }
            _ => {
                // Fresh installs / upgrades do not yet know the prior round boundary. Fall back to
                // the legacy live-denominator model for exactly one transition payout, then store
                // the current boundary as the next round's start snapshot when this job finalizes.
                job.configure_round_accounting(
                    Some(now_nanos),
                    Some(denom_e8s),
                    round_end_latest_tx_id,
                    now_nanos,
                    round_end_latest_tx_id,
                    denom_e8s,
                    true,
                );
            }
        }
        st.active_payout_job = Some(job);
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
                LatestScan::Unreadable | LatestScan::InvariantBroken => None,
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
    InvariantBroken,
}

fn record_latest_unreadable_failure(st: &mut state::State) {
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

fn record_latest_invariant_failure(st: &mut state::State) {
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

async fn scan_latest_tx_id(index: &impl IndexClient, staking_id: String, start: Option<u64>) -> LatestScan {
    let mut cursor = start;
    let mut latest = start;
    let mut pages_scanned = 0u64;
    loop {
        if pages_scanned >= MAX_INDEX_PAGES_PER_LATEST_SCAN {
            return LatestScan::Read(latest);
        }
        pages_scanned = pages_scanned.saturating_add(1);
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
        if cursor.zip(page_latest).map(|(prev, next)| next <= prev).unwrap_or(false) {
            return LatestScan::InvariantBroken;
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
                LatestScan::Unreadable | LatestScan::InvariantBroken => None,
            };
            st.consecutive_index_latest_invariant_failures = Some(0);
            st.consecutive_index_latest_unreadable_failures = Some(0);
        }
        (Some(prev_balance), _prev_latest_tx_id) if prev_balance == denom_balance_e8s => {}
        (Some(_), prev_latest_tx_id) => match latest_scan {
            LatestScan::Unreadable => record_latest_unreadable_failure(st),
            LatestScan::InvariantBroken => record_latest_invariant_failure(st),
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
                    record_latest_invariant_failure(st);
                }
            }
        }
    }
}

fn apply_cmc_run_result(st: &mut state::State, attempts: u64, successes: u64, zero_success_run_counts: bool) {
    if attempts == 0 {
        return;
    }
    if successes > 0 {
        st.consecutive_cmc_zero_success_runs = Some(0);
        return;
    }
    if !zero_success_run_counts {
        return;
    }
    st.consecutive_cmc_zero_success_runs = Some(st.consecutive_cmc_zero_success_runs.unwrap_or(0).saturating_add(1));
    if st.consecutive_cmc_zero_success_runs.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
        st.forced_rescue_reason = Some(ForcedRescueReason::CmcZeroSuccessRuns);
    }
}

async fn zero_success_run_counts_toward_rescue(
    status_client: &impl CanisterStatusClient,
    job: &ActivePayoutJob,
) -> bool {
    if job.cmc_attempt_count.unwrap_or(0) == 0 || job.cmc_success_count.unwrap_or(0) > 0 {
        return false;
    }
    let beneficiaries = job.cmc_attempted_beneficiaries.clone().unwrap_or_default();
    for beneficiary in beneficiaries {
        if matches!(status_client.canister_exists(beneficiary).await, Ok(true)) {
            return true;
        }
    }
    false
}

fn apply_job_health_observations(st: &mut state::State, job: &ActivePayoutJob, zero_success_run_counts: bool) {
    apply_anchor_observation(st, job.observed_oldest_tx_id);
    apply_latest_observation(st, job.denom_staking_balance_e8s, LatestScan::Read(job.observed_latest_tx_id));
    apply_cmc_run_result(
        st,
        job.cmc_attempt_count.unwrap_or(0),
        job.cmc_success_count.unwrap_or(0),
        zero_success_run_counts,
    );
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

async fn process_payout(
    ledger: &impl LedgerClient,
    index: &impl IndexClient,
    cmc: &impl CmcClient,
    status_client: &impl CanisterStatusClient,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
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
        let (have_round_snapshot, stake_unchanged_since_round_start, round_start_latest_tx_id) = state::with_state(|st| {
            let have_snapshot = st.current_round_start_time_nanos.is_some() && st.current_round_start_staking_balance_e8s.is_some();
            let stake_unchanged = st.current_round_start_staking_balance_e8s == Some(denom_e8s);
            (have_snapshot, stake_unchanged, st.current_round_start_latest_tx_id)
        });
        let round_end_latest_tx_id = if have_round_snapshot {
            if stake_unchanged_since_round_start {
                round_start_latest_tx_id
            } else {
                match scan_latest_tx_id(index, staking_id.clone(), state::with_state(|st| st.last_observed_latest_tx_id)).await {
                    LatestScan::Read(latest_tx_id) => latest_tx_id,
                    LatestScan::Unreadable | LatestScan::InvariantBroken => return false,
                }
            }
        } else {
            None
        };
        ensure_active_job(now_nanos, fee_e8s, pot_start_e8s, denom_e8s, round_end_latest_tx_id);
    }

    // Skip ranges are a durable cache of barren tx-id spans discovered by earlier jobs.
    // They deliberately optimize replay work only; if future maintenance changes the rules
    // for what counts as a contribution, the cache must be cleared before relying on it.
    let mut skip_ranges = state::list_skip_ranges();
    let mut skip_range_idx = initial_skip_range_index(
        &skip_ranges,
        state::with_state(|st| st.active_payout_job.as_ref().and_then(|job| job.next_start)),
    );
    let mut pages_scanned = 0u64;

    loop {
        let job = state::with_state(|st| st.active_payout_job.clone());
        let Some(job) = job else { maybe_latch_bootstrap_rescue(now_secs); return true; };
        if job.pending_transfer.is_some() {
            if !drive_pending_transfer(ledger, cmc, cfg.cmc_canister_id, job.fee_e8s, now_nanos, now_secs).await {
                return true;
            }
            continue;
        }

        if !effective_denom_scan_complete(&job) {
            let resp = match index.get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            pages_scanned = pages_scanned.saturating_add(1);
            let mut page_next_start = job.next_start;
            let mut denom_delta_e8s = 0u64;
            let mut reached_round_end = false;
            let min_tx_e8s = state::with_state(|st| st.config.min_tx_e8s);
            let round_start_time_nanos = job.round_start_time_nanos.unwrap_or(job.round_end_time_nanos.unwrap_or(now_nanos));
            let round_end_time_nanos = job.round_end_time_nanos.unwrap_or(now_nanos);
            let recognition_delay_seconds = recognition_delay_seconds();

            for tx in &resp.transactions {
                if let Some(last_seen) = job.next_start {
                    if tx.id <= last_seen {
                        continue;
                    }
                }
                if job.round_end_latest_tx_id.map(|end| tx.id > end).unwrap_or(false) {
                    reached_round_end = true;
                    break;
                }
                page_next_start = Some(tx.id);
                let Some(contribution) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                    continue;
                };
                if !matches!(logic::classify_contribution(min_tx_e8s, &contribution), logic::ContributionValidity::Valid { .. }) {
                    continue;
                }
                let weighted_amount_e8s = logic::contribution_amount_for_round_e8s(
                    &contribution,
                    tx.id,
                    logic::index_tx_timestamp_nanos(tx),
                    job.round_start_latest_tx_id,
                    job.round_end_latest_tx_id,
                    round_start_time_nanos,
                    round_end_time_nanos,
                    recognition_delay_seconds,
                ).unwrap_or(0);
                denom_delta_e8s = denom_delta_e8s.saturating_add(weighted_amount_e8s);
            }

            let scan_complete = reached_round_end || resp.transactions.len() < PAGE_SIZE as usize || resp.transactions.is_empty();
            state::with_state_mut(|st| {
                if let Some(active_job) = st.active_payout_job.as_mut() {
                    active_job.effective_denom_staking_balance_e8s = Some(
                        active_job
                            .effective_denom_staking_balance_e8s
                            .unwrap_or(active_job.round_start_staking_balance_e8s.unwrap_or(active_job.denom_staking_balance_e8s))
                            .saturating_add(denom_delta_e8s)
                    );
                    if scan_complete {
                        active_job.effective_denom_scan_complete = Some(true);
                        // The effective-denominator pre-scan only needs to visit current-round
                        // transactions. The payout scan that follows still needs to revisit the
                        // full beneficiary history up to the round-end boundary so pre-existing
                        // contributors continue to receive payouts each round.
                        active_job.next_start = None;
                        active_job.skip_candidate_start_tx_id = None;
                        active_job.skip_candidate_end_tx_id = None;
                        active_job.skip_candidate_tx_count = 0;
                    } else if let Some(next_start) = page_next_start {
                        active_job.next_start = Some(next_start);
                    }
                }
            });
            if !scan_complete && pages_scanned >= MAX_INDEX_PAGES_PER_PAYOUT_TICK {
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
            finalize_completed_job(status_client).await;
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        }

        if job.round_end_latest_tx_id.zip(job.next_start).map(|(end, cursor)| cursor >= end).unwrap_or(false) {
            state::with_state_mut(|st| if let Some(active_job) = st.active_payout_job.as_mut() {
                active_job.scan_complete = true;
            });
            continue;
        }

        if let Some(skip_to) = next_skip_jump_target(job.next_start, &skip_ranges, &mut skip_range_idx) {
            if job.round_end_latest_tx_id.map(|end| skip_to >= end).unwrap_or(false) {
                state::with_state_mut(|st| if let Some(active_job) = st.active_payout_job.as_mut() {
                    active_job.scan_complete = true;
                    active_job.next_start = active_job.round_end_latest_tx_id;
                });
            } else {
                state::with_state_mut(|st| {
                    if let Some(active_job) = st.active_payout_job.as_mut() {
                        active_job.next_start = Some(skip_to);
                    }
                });
            }
            continue;
        }

        let resp = match index.get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        pages_scanned = pages_scanned.saturating_add(1);
        let last_id = resp.transactions.last().map(|t| t.id);
        if job.next_start.zip(last_id).map(|(prev, latest)| latest <= prev).unwrap_or(false) {
            state::with_state_mut(|st| {
                if let Some(active_job) = st.active_payout_job.as_mut() {
                    if active_job.observed_oldest_tx_id.is_none() {
                        active_job.observed_oldest_tx_id = resp.oldest_tx_id;
                    }
                }
                record_latest_invariant_failure(st);
            });
            return false;
        }
        note_index_page(&resp);
        if resp.transactions.is_empty() {
            let mut skip_candidate = LocalSkipCandidate::from_job(&job);
            let mut pending_skip_ranges = Vec::new();
            record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
            state::with_state_mut(|st| {
                if let Some(active_job) = st.active_payout_job.as_mut() {
                    active_job.scan_complete = true;
                    active_job.skip_candidate_start_tx_id = skip_candidate.start_tx_id;
                    active_job.skip_candidate_end_tx_id = skip_candidate.end_tx_id;
                    active_job.skip_candidate_tx_count = skip_candidate.tx_count;
                }
            });
            if persist_new_skip_ranges(&mut skip_ranges, &mut pending_skip_ranges).is_err() {
                latch_skip_range_invariant_rescue();
                return true;
            }
            continue;
        }

        let page_start = job.next_start;
        let mut ignored_under_threshold_delta = 0u64;
        let mut ignored_bad_memo_delta = 0u64;
        let mut page_next_start = page_start;
        let mut skip_candidate = LocalSkipCandidate::from_job(&job);
        let mut pending_skip_ranges = Vec::new();
        let mut reached_round_end = false;
        let mut scan_batch = Some(state::begin_persistence_batch());
        for tx in &resp.transactions {
            if let Some(last_seen) = page_start {
                if tx.id <= last_seen {
                    continue;
                }
            }
            if job.round_end_latest_tx_id.map(|end| tx.id > end).unwrap_or(false) {
                reached_round_end = true;
                break;
            }
            page_next_start = Some(tx.id);
            let Some(contribution) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                skip_candidate.note_skippable(tx.id);
                continue;
            };
            let snapshot = state::with_state(|st| {
                let job = st.active_payout_job.as_ref().expect("active payout job missing");
                (
                    job.pot_start_e8s,
                    effective_denom_e8s(job),
                    job.fee_e8s,
                    st.config.min_tx_e8s,
                    job.round_start_latest_tx_id,
                    job.round_end_latest_tx_id,
                    job.round_start_time_nanos.unwrap_or(job.round_end_time_nanos.unwrap_or(now_nanos)),
                    job.round_end_time_nanos.unwrap_or(now_nanos),
                )
            });
            match logic::classify_contribution(snapshot.3, &contribution) {
                logic::ContributionValidity::IgnoreUnderThreshold => {
                    skip_candidate.note_skippable(tx.id);
                    ignored_under_threshold_delta = ignored_under_threshold_delta.saturating_add(1);
                }
                logic::ContributionValidity::IgnoreBadMemo => {
                    skip_candidate.note_skippable(tx.id);
                    ignored_bad_memo_delta = ignored_bad_memo_delta.saturating_add(1);
                }
                logic::ContributionValidity::Valid { beneficiary } => {
                    let amount_for_round_e8s = logic::contribution_amount_for_round_e8s(
                        &contribution,
                        tx.id,
                        logic::index_tx_timestamp_nanos(tx),
                        snapshot.4,
                        snapshot.5,
                        snapshot.6,
                        snapshot.7,
                        recognition_delay_seconds(),
                    ).unwrap_or(0);
                    let gross_share_e8s = logic::compute_raw_share_e8s(amount_for_round_e8s, snapshot.0, snapshot.1);
                    if gross_share_e8s <= snapshot.2 {
                        record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
                        continue;
                    }
                    record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
                    flush_scan_progress(
                        &mut ignored_under_threshold_delta,
                        &mut ignored_bad_memo_delta,
                        page_next_start,
                        &skip_candidate,
                    );
                    drop(scan_batch.take());
                    if persist_new_skip_ranges(&mut skip_ranges, &mut pending_skip_ranges).is_err() {
                        latch_skip_range_invariant_rescue();
                        return true;
                    }
                    let pending = PendingNotification { kind: TransferKind::Beneficiary, beneficiary, gross_share_e8s, amount_e8s: gross_share_e8s.saturating_sub(snapshot.2), block_index: 0, next_start: Some(tx.id) };
                    if !send_and_notify(ledger, cmc, pending, snapshot.2, now_nanos, now_secs, cfg.cmc_canister_id).await {
                        return true;
                    }
                    scan_batch = Some(state::begin_persistence_batch());
                }
            }
        }
        let scan_complete = reached_round_end || resp.transactions.len() < PAGE_SIZE as usize || last_id.is_none();
        if scan_complete {
            record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
        }
        state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() {
            job.ignored_under_threshold = job.ignored_under_threshold.saturating_add(ignored_under_threshold_delta);
            job.ignored_bad_memo = job.ignored_bad_memo.saturating_add(ignored_bad_memo_delta);
            if let Some(next_start) = page_next_start {
                job.next_start = Some(next_start);
            }
            job.skip_candidate_start_tx_id = skip_candidate.start_tx_id;
            job.skip_candidate_end_tx_id = skip_candidate.end_tx_id;
            job.skip_candidate_tx_count = skip_candidate.tx_count;
            if scan_complete { job.scan_complete = true; } else if !reached_round_end { job.next_start = last_id; }
        });
        drop(scan_batch);
        if persist_new_skip_ranges(&mut skip_ranges, &mut pending_skip_ranges).is_err() {
            latch_skip_range_invariant_rescue();
            return true;
        }
        if !scan_complete && pages_scanned >= MAX_INDEX_PAGES_PER_PAYOUT_TICK {
            return true;
        }
    }
}
fn desired_rescue_controllers(
    now_secs: u64,
    blackhole_armed: bool,
    blackhole_controller: Option<Principal>,
    last_xfer_opt: Option<u64>,
    rescue_controller: Principal,
    forced_reason_present: bool,
    skip_range_fault: bool,
    self_id: Principal,
) -> Result<Option<Vec<Principal>>, u32> {
    if !blackhole_armed {
        return Ok(None);
    }
    let Some(blackhole_controller) = blackhole_controller else {
        return Err(3107);
    };
    let mut desired = if forced_reason_present || skip_range_fault {
        vec![blackhole_controller, rescue_controller, self_id]
    } else {
        let Some(desired) = policy::desired_controllers(now_secs, last_xfer_opt, self_id, Some(blackhole_controller), rescue_controller) else {
            return Ok(None);
        };
        desired
    };
    desired.sort_by(|a: &Principal, b: &Principal| a.to_text().cmp(&b.to_text()));
    desired.dedup();
    Ok(Some(desired))
}

async fn attempt_rescue(now_secs: u64) {
    maybe_latch_bootstrap_rescue(now_secs);
    let (blackhole_armed, blackhole_controller, last_xfer_opt, rescue_controller, forced_reason, skip_range_fault) = state::with_state(|st| {
        (
            st.config.blackhole_armed.unwrap_or(false),
            st.config.blackhole_controller,
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.forced_rescue_reason.clone(),
            st.skip_range_invariant_fault.unwrap_or(false),
        )
    });
    let self_id = self_canister_principal();
    let desired_opt = match desired_rescue_controllers(
        now_secs,
        blackhole_armed,
        blackhole_controller,
        last_xfer_opt,
        rescue_controller,
        forced_reason.is_some(),
        skip_range_fault,
        self_id,
    ) {
        Ok(desired) => desired,
        Err(code) => {
            log_error(code);
            return;
        }
    };
    let Some(desired) = desired_opt else {
        return;
    };
    let rescue_active = desired.iter().any(|p| *p == rescue_controller);
    let arg = UpdateSettingsArgs { canister_id: self_id, settings: CanisterSettings { controllers: Some(desired), ..Default::default() } };
    if update_settings(&arg).await.is_err() { log_error(3101); return; }
    state::with_state_mut(|st| { st.last_rescue_check_ts = now_secs; st.rescue_triggered = rescue_active; });
}

async fn rescue_tick() {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    rescue_tick_with_resume_at(now_secs, || async {
        main_tick(true).await;
    }).await;
}

async fn rescue_tick_with_resume_at<F, Fut>(now_secs: u64, resume_active_job: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Preserve the rescue/controller-reconciliation ordering first, then use the
    // same daily cadence as a bounded fallback resume opportunity for any
    // unfinished payout job that remains persisted.
    attempt_rescue(now_secs).await;
    resume_active_job_if_present(resume_active_job).await;
}

async fn resume_active_job_if_present<F, Fut>(resume_active_job: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let has_active_job = state::with_state(|st| st.active_payout_job.is_some());
    if has_active_job {
        resume_active_job().await;
    }
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

    struct ExistingCanisterStatus {
        existing: Vec<Principal>,
    }

    impl ExistingCanisterStatus {
        fn new(existing: Vec<Principal>) -> Self {
            Self { existing }
        }
    }

    #[async_trait]
    impl CanisterStatusClient for ExistingCanisterStatus {
        async fn canister_exists(&self, canister_id: Principal) -> Result<bool, crate::clients::ClientError> {
            Ok(self.existing.iter().any(|existing| *existing == canister_id))
        }
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
    struct BalanceRecordingLedger {
        fee_e8s: u64,
        payout_balance_e8s: u64,
        staking_balance_e8s: u64,
        transfer_blocks: Arc<Mutex<VecDeque<u64>>>,
        transfer_amounts: Arc<Mutex<Vec<u64>>>,
    }

    impl BalanceRecordingLedger {
        fn new(fee_e8s: u64, payout_balance_e8s: u64, staking_balance_e8s: u64, transfer_blocks: Vec<u64>) -> Self {
            Self {
                fee_e8s,
                payout_balance_e8s,
                staking_balance_e8s,
                transfer_blocks: Arc::new(Mutex::new(transfer_blocks.into())),
                transfer_amounts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn transfer_amounts(&self) -> Vec<u64> {
            self.transfer_amounts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LedgerClient for BalanceRecordingLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { Ok(self.fee_e8s) }
        async fn balance_of_e8s(&self, account: Account) -> Result<u64, crate::clients::ClientError> {
            let staking = state::with_state(|st| st.config.staking_account.clone());
            if account == staking {
                Ok(self.staking_balance_e8s)
            } else {
                Ok(self.payout_balance_e8s)
            }
        }
        async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            let amount_u64 = arg.amount.0.to_string().parse::<u64>().unwrap_or(0);
            self.transfer_amounts.lock().unwrap().push(amount_u64);
            let block = self.transfer_blocks.lock().unwrap().pop_front().unwrap_or(1);
            Ok(Ok(BlockIndex::from(block)))
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


    struct RecordingIndex {
        txs: Vec<IndexTransactionWithId>,
        starts: Arc<Mutex<Vec<Option<u64>>>>,
    }

    impl RecordingIndex {
        fn new(txs: Vec<IndexTransactionWithId>) -> Self {
            Self { txs, starts: Arc::new(Mutex::new(Vec::new())) }
        }

        fn starts(&self) -> Vec<Option<u64>> {
            self.starts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for RecordingIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            self.starts.lock().unwrap().push(start);
            let transactions = self
                .txs
                .iter()
                .filter(|tx| start.map(|last_seen| tx.id > last_seen).unwrap_or(true))
                .take(max_results as usize)
                .cloned()
                .collect();
            Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: self.txs.first().map(|tx| tx.id),
                transactions,
            })
        }
    }


    struct BarrenPagedIndex {
        page_count: u64,
        starts: Arc<Mutex<Vec<Option<u64>>>>,
        staking_id: String,
    }

    impl BarrenPagedIndex {
        fn new(page_count: u64, staking_id: String) -> Self {
            Self {
                page_count,
                starts: Arc::new(Mutex::new(Vec::new())),
                staking_id,
            }
        }

        fn starts(&self) -> Vec<Option<u64>> {
            self.starts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for BarrenPagedIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            self.starts.lock().unwrap().push(start);
            let page_idx = start.map(|last_seen| last_seen / PAGE_SIZE).unwrap_or(0);
            if page_idx >= self.page_count {
                return Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    oldest_tx_id: Some(1),
                    transactions: Vec::new(),
                });
            }
            let first_id = page_idx * PAGE_SIZE + 1;
            let transactions = (0..max_results)
                .map(|offset| contribution_tx(first_id + offset, &self.staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
                .collect();
            Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions,
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

    fn contribution_tx_at(id: u64, staking_id: &str, amount_e8s: u64, memo: Option<Vec<u8>>, timestamp_nanos: u64) -> IndexTransactionWithId {
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
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }

    fn contribution_tx(id: u64, staking_id: &str, amount_e8s: u64, memo: Option<Vec<u8>>) -> IndexTransactionWithId {
        contribution_tx_at(id, staking_id, amount_e8s, memo, 0)
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
            stake_recognition_delay_seconds: Some(24 * 60 * 60),
        }
    }

    fn test_config() -> state::Config {
        test_config_with_intervals(60, 60)
    }

    fn set_active_job(now_secs: u64, job: ActivePayoutJob) -> state::Config {
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
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
    fn resume_active_job_if_present_runs_when_active_job_exists() {
        let now_secs = 1_000_u64;
        let job = ActivePayoutJob::new(1, 10_000, 1_000_000, 2_000_000, now_secs * 1_000_000_000);
        let _cfg = set_active_job(now_secs, job);

        let resume_calls = Arc::new(AtomicUsize::new(0));
        let resume_calls_clone = resume_calls.clone();
        run_ready(resume_active_job_if_present(move || {
            let resume_calls = resume_calls_clone.clone();
            async move {
                resume_calls.fetch_add(1, Ordering::SeqCst);
            }
        }));

        assert_eq!(resume_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resume_active_job_if_present_skips_when_no_active_job_exists() {
        let now_secs = 1_001_u64;
        let cfg = test_config();
        state::set_state(state::State::new(cfg, now_secs));

        let resume_calls = Arc::new(AtomicUsize::new(0));
        let resume_calls_clone = resume_calls.clone();
        run_ready(resume_active_job_if_present(move || {
            let resume_calls = resume_calls_clone.clone();
            async move {
                resume_calls.fetch_add(1, Ordering::SeqCst);
            }
        }));

        assert_eq!(resume_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stale_main_lease_can_be_reclaimed_without_old_guard_clearing_the_new_lease() {
        let now_secs = 1_000_u64;
        let mut st = state::State::new(test_config(), now_secs);
        let mut job = ActivePayoutJob::new(1, 10_000, 1_000_000, 2_000_000, now_secs * 1_000_000_000);
        job.scan_complete = true;
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(7)]);
        let index = UnexpectedIndex;
        let calls = Arc::new(AtomicUsize::new(0));
        let cmc = PendingCmc { calls: calls.clone() };

        let first_now_nanos = now_secs * 1_000_000_000;
        let mut fut1 = Box::pin(run_main_tick_with_clients(false, first_now_nanos, now_secs, &ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient));
        assert!(matches!(poll_once(fut1.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        let second_now_secs = now_secs + MAIN_TICK_LEASE_SECONDS + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(false, second_now_nanos, second_now_secs, &ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient));
        assert!(matches!(poll_once(fut2.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(second_now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        drop(fut1);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(second_now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        drop(fut2);
        assert_eq!(state::with_state(|st| st.main_lock_state_ts), Some(0));
    }

    #[test]
    fn transfer_arg_uses_little_endian_top_up_memo() {
        state::clear_skip_ranges();
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        let status_client = ExistingCanisterStatus::new(vec![Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap()]);
        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &status_client, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![]);
        let first_tick_cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::RetryableErr]);
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);

        assert!(!run_ready(process_payout(&ledger, &index, &first_tick_cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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
        let status_client = ExistingCanisterStatus::new(vec![beneficiary]);
        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &second_tick_cmc, &status_client, now_secs * 1_000_000_000, now_secs)));

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
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let mut job = ActivePayoutJob::new(41, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
        job.scan_complete = true;
        job.cmc_attempt_count = Some(2);
        job.cmc_success_count = Some(0);
        job.cmc_attempted_beneficiaries = Some(vec![beneficiary]);
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(123)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let status_client = ExistingCanisterStatus::new(vec![beneficiary]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &status_client, now_secs * 1_000_000_000, now_secs)));
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
    fn zero_success_runs_for_non_canister_targets_do_not_advance_rescue_threshold() {
        let now_secs = 4_065_u64;
        let mut st = state::State::new(test_config(), now_secs);
        st.consecutive_cmc_zero_success_runs = Some(1);
        let beneficiary = Principal::from_text("uuc56-gyb").unwrap();
        let mut job = ActivePayoutJob::new(41065, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
        job.scan_complete = true;
        job.cmc_attempt_count = Some(2);
        job.cmc_success_count = Some(0);
        job.cmc_attempted_beneficiaries = Some(vec![beneficiary]);
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(123)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let status_client = ExistingCanisterStatus::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &status_client, now_secs * 1_000_000_000, now_secs)));
        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
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
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);

        assert!(!run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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
        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
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

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "expected two beneficiary transfers plus one self remainder transfer");
        assert_eq!(cmc.call_count(), 3);

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 2, "overlapping page replay must not duplicate the tx id 500 contribution");
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ignored_under_threshold, 499);
        assert_eq!(summary.ignored_bad_memo, 0);
    }

    #[test]
    fn scan_latest_tx_id_detects_non_advancing_full_page() {
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: (11..=(10 + PAGE_SIZE)).map(|id| contribution_tx(id, "staking", 1, None)).collect(),
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: (11..=(10 + PAGE_SIZE)).map(|id| contribution_tx(id, "staking", 1, None)).collect(),
            }),
        ]);

        let latest = run_ready(scan_latest_tx_id(&index, "staking".to_string(), Some(10)));
        assert_eq!(latest, LatestScan::InvariantBroken);
    }

    #[test]
    fn process_payout_stops_and_records_invariant_failure_when_index_page_does_not_advance() {
        let now_secs = 4_600_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(12, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000));
        state::with_state_mut(|st| {
            let job = st.active_payout_job.as_mut().expect("active job");
            job.next_start = Some(10);
        });
        let staking_id = account_identifier_text(&cfg.staking_account);
        let repeated_page: Vec<_> = (11..=(10 + PAGE_SIZE)).map(|id| contribution_tx(id, &staking_id, 1, None)).collect();
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: repeated_page.clone(),
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: repeated_page,
            }),
        ]);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(!run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
        });

        let second_index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: (11..=(10 + PAGE_SIZE)).map(|id| contribution_tx(id, &staking_id, 1, None)).collect(),
            }),
        ]);
        assert!(!run_ready(process_payout(&ledger, &second_index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestInvariantBroken));
        });
    }

    #[test]
    fn process_payout_yields_after_bounded_number_of_barren_pages() {
        let now_secs = 4_700_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(13, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text(&cfg.staking_account);
        let index = BarrenPagedIndex::new(MAX_INDEX_PAGES_PER_PAYOUT_TICK + 1, staking_id);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        let job = state::with_state(|st| st.active_payout_job.clone()).expect("job should remain active after bounded yield");
        assert_eq!(job.scan_complete, false);
        assert_eq!(job.next_start, Some(MAX_INDEX_PAGES_PER_PAYOUT_TICK * PAGE_SIZE));
        assert_eq!(index.starts().len(), MAX_INDEX_PAGES_PER_PAYOUT_TICK as usize);
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
        state::clear_skip_ranges();
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
        state::clear_skip_ranges();
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
        state::clear_skip_ranges();
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
        state::clear_skip_ranges();
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

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }


    #[test]
    fn large_skippable_history_persists_a_single_skip_range() {
        let now_secs = 10_000;
        let job = ActivePayoutJob::new(100, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        let _cfg = set_active_job(now_secs, job);

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
            }]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.ignored_under_threshold, MIN_SKIP_RANGE_TX_COUNT);
        assert_eq!(summary.ignored_bad_memo, 0);
        assert_eq!(index.starts().first().copied(), Some(None));
    }

    #[test]
    fn history_below_skip_threshold_does_not_persist_range() {
        let now_secs = 10_100;
        let job = ActivePayoutJob::new(101, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        let _cfg = set_active_job(now_secs, job);

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let txs: Vec<_> = (1..MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert!(state::list_skip_ranges().is_empty());
    }

    #[test]
    fn repeated_below_threshold_history_replays_from_start_and_still_reaches_later_qualifying_contribution_without_persisting_skip_ranges() {
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();

        for round in 0..2_u64 {
            let now_secs = 10_150 + round;
            let job = ActivePayoutJob::new(150 + round, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000);
            let cfg = set_active_job(now_secs, job);
            let staking_id = account_identifier_text(&cfg.staking_account);
            let txs: Vec<_> = (1..MIN_SKIP_RANGE_TX_COUNT)
                .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
                .chain(std::iter::once(contribution_tx(
                    MIN_SKIP_RANGE_TX_COUNT,
                    &staking_id,
                    crate::MIN_MIN_TX_E8S,
                    Some(beneficiary.to_text().into_bytes()),
                )))
                .collect();
            let index = RecordingIndex::new(txs);
            let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(400 + round), LedgerStep::Ok(500 + round)]);
            let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

            assert!(run_ready(process_payout(
                &ledger,
                &index,
                &cmc,
                &ExistingCanisterStatus::new(vec![beneficiary.clone()]),
                now_secs * 1_000_000_000,
                now_secs,
            )));

            assert_eq!(index.starts().first().copied(), Some(None), "round {round} should replay from the beginning when no skip range is persisted");
            assert!(state::list_skip_ranges().is_empty(), "round {round} should not persist sub-threshold barren history");
            let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
            assert_eq!(summary.ignored_under_threshold, MIN_SKIP_RANGE_TX_COUNT - 1, "round {round} should still rescan and ignore the same barren span");
            assert_eq!(summary.topped_up_count, 1, "round {round} should still reach the qualifying contribution after replay");
        }
    }

    #[test]
    fn persisted_skip_range_causes_next_run_to_jump_before_fetching_inside_it() {
        let now_secs = 10_200;
        let mut job = ActivePayoutJob::new(7, 10_000, 500_000_000, 500_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(0);
        let _cfg = set_active_job(now_secs, job);
        state::insert_skip_range(SkipRange {
            start_tx_id: 1,
            end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
        })
        .expect("preexisting skip range should persist");

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let txs = vec![contribution_tx(
            MIN_SKIP_RANGE_TX_COUNT + 1,
            &staking_id,
            500_000_000,
            Some(beneficiary.to_text().into_bytes()),
        )];
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(42)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &ExistingCanisterStatus::new(vec![beneficiary.clone()]),
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(index.starts().first().copied(), Some(Some(MIN_SKIP_RANGE_TX_COUNT)));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.topped_up_count, 1);
    }

    #[test]
    fn skip_range_persistence_fault_latches_sticky_fault_instead_of_trapping() {
        let now_secs = 10_225;
        let job = ActivePayoutJob::new(8, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        let cfg = set_active_job(now_secs, job);

        state::insert_skip_range(SkipRange {
            start_tx_id: MIN_SKIP_RANGE_TX_COUNT + 1,
            end_tx_id: MIN_SKIP_RANGE_TX_COUNT + 10,
        })
        .expect("conflicting persisted skip range should be installed for the test");

        let staking_id = account_identifier_text(&cfg.staking_account);
        let txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        state::with_state(|st| {
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.skip_range_invariant_fault, Some(true));
            assert!(st.active_payout_job.is_some(), "job should remain available for rescue inspection");
        });
        assert_eq!(ledger.transfer_calls(), 0);
        assert_eq!(cmc.call_count(), 0);
    }

    #[test]
    fn desired_rescue_controllers_widens_controllers_when_skip_range_fault_is_latched() {
        let rescue_controller = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let blackhole_controller = Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").unwrap();
        let self_id = Principal::anonymous();

        let desired = desired_rescue_controllers(
            10_000,
            true,
            Some(blackhole_controller),
            Some(9_000),
            rescue_controller,
            false,
            true,
            self_id,
        )
        .expect("skip-range fault should not error")
        .expect("armed blackhole mode should produce a controller set");

        let mut expected = vec![blackhole_controller, rescue_controller, self_id];
        expected.sort_by(|a, b| a.to_text().cmp(&b.to_text()));
        expected.dedup();
        assert_eq!(desired, expected);
    }

    #[test]
    fn desired_rescue_controllers_returns_error_when_armed_without_blackhole_controller() {
        let err = desired_rescue_controllers(
            10_000,
            true,
            None,
            Some(9_000),
            Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
            false,
            true,
            Principal::anonymous(),
        )
        .expect_err("armed blackhole mode without a controller should error");
        assert_eq!(err, 3107);
    }

    #[test]
    fn interrupted_multi_page_skip_candidate_resumes_and_persists_single_range() {
        let now_secs = 10_250;
        let mut job = ActivePayoutJob::new(9, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        job.next_start = None;
        let _cfg = set_active_job(now_secs, job);

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let first_page = GetAccountIdentifierTransactionsResponse {
            balance: 0,
            oldest_tx_id: Some(1),
            transactions: txs.iter().take(PAGE_SIZE as usize).cloned().collect(),
        };
        let interrupted_index = ScriptedIndex::new(vec![IndexResponseStep::Ok(first_page), IndexResponseStep::Err]);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(!run_ready(process_payout(
            &ledger,
            &interrupted_index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let interrupted_job = state::with_state(|st| st.active_payout_job.clone()).expect("job should remain active after retryable index failure");
        assert_eq!(interrupted_job.next_start, Some(PAGE_SIZE));
        assert_eq!(interrupted_job.skip_candidate_start_tx_id, Some(1));
        assert_eq!(interrupted_job.skip_candidate_end_tx_id, Some(PAGE_SIZE));
        assert_eq!(interrupted_job.skip_candidate_tx_count, PAGE_SIZE);
        assert!(state::list_skip_ranges().is_empty());

        let resuming_index = RecordingIndex::new(txs);
        assert!(run_ready(process_payout(
            &ledger,
            &resuming_index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
            }]
        );
        assert_eq!(resuming_index.starts().first().copied(), Some(Some(PAGE_SIZE)));
    }

    #[test]
    fn no_transfer_breaks_skip_span_so_only_long_barren_sides_are_persisted() {
        let now_secs = 10_300;
        let mut job = ActivePayoutJob::new(11, 10_000, 10_000, 1_000_000_000_000, now_secs * 1_000_000_000);
        job.next_start = None;
        let _cfg = set_active_job(now_secs, job);

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let mut txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        txs.push(contribution_tx(
            MIN_SKIP_RANGE_TX_COUNT + 1,
            &staking_id,
            crate::MIN_MIN_TX_E8S,
            Some(beneficiary.to_text().into_bytes()),
        ));
        txs.extend(
            ((MIN_SKIP_RANGE_TX_COUNT + 2)..=(2 * MIN_SKIP_RANGE_TX_COUNT + 1))
                .map(|id| contribution_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None)),
        );
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(
            state::list_skip_ranges(),
            vec![
                SkipRange {
                    start_tx_id: 1,
                    end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
                },
                SkipRange {
                    start_tx_id: MIN_SKIP_RANGE_TX_COUNT + 2,
                    end_tx_id: 2 * MIN_SKIP_RANGE_TX_COUNT + 1,
                },
            ]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.ignored_under_threshold, 2 * MIN_SKIP_RANGE_TX_COUNT);
        assert_eq!(summary.topped_up_count, 0);
    }

    #[test]
    fn process_payout_uses_weighted_effective_denom_and_ignores_post_boundary_tx_ids() {
        let now_secs = 2_000;
        let mut job = ActivePayoutJob::new(77, 10_000, 100_000_000, 1_900_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(1);
        job.configure_round_accounting(
            Some(0),
            Some(1_000_000_000),
            Some(1),
            100_000_000_000,
            Some(2),
            1_000_000_000,
            false,
        );
        let _cfg = set_active_job(now_secs, job);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let beneficiary_c = Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap();
        let index = RecordingIndex::new(vec![
            contribution_tx_at(1, &staking_id, 1_000_000_000, Some(beneficiary_a.to_text().into_bytes()), 0),
            contribution_tx_at(2, &staking_id, 900_000_000, Some(beneficiary_b.to_text().into_bytes()), 80_000_000_000),
            // Same timestamp as tx 2 on purpose: tx-id, not timestamp, defines the round range.
            contribution_tx_at(3, &staking_id, 900_000_000, Some(beneficiary_c.to_text().into_bytes()), 80_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 1_900_000_000, vec![11, 12]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(1_090_000_000));
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 1);
        assert_eq!(ledger.transfer_amounts(), vec![91_733_119, 8_246_880]);
        assert_eq!(cmc.call_count(), 2);
    }

    #[test]
    fn process_payout_still_pays_pre_round_contributions_after_effective_denom_prescan() {
        let now_secs = 2_500;
        let mut job = ActivePayoutJob::new(78, 10_000, 100_000_000, 1_400_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(1);
        job.configure_round_accounting(
            Some(0),
            Some(1_400_000_000),
            Some(1),
            100_000_000_000,
            Some(1),
            1_400_000_000,
            false,
        );
        let _cfg = set_active_job(now_secs, job);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text(&state::with_state(|st| st.config.staking_account.clone()));
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = RecordingIndex::new(vec![
            contribution_tx_at(1, &staking_id, 400_000_000, Some(beneficiary.to_text().into_bytes()), 0),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 1_400_000_000, vec![31, 32]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(1_400_000_000));
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.remainder_to_self_e8s, 71_418_572);
        assert_eq!(ledger.transfer_amounts(), vec![28_561_428, 71_418_572]);
    }


    #[test]
    fn first_transition_payout_falls_back_to_live_denom_and_records_next_round_snapshot() {
        let now_secs = 3_000;
        let cfg = test_config();
        let st = state::State::new(cfg.clone(), now_secs);
        state::clear_skip_ranges();
        state::set_state(st);

        let staking_id = account_identifier_text(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = RecordingIndex::new(vec![
            contribution_tx_at(1, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), now_secs * 1_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![21, 22]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(200_000_000));
        assert_eq!(summary.remainder_to_self_e8s, 49_990_000);
        assert_eq!(ledger.transfer_amounts(), vec![49_990_000, 49_990_000]);
        let (round_start_time_nanos, round_start_staking_balance_e8s, round_start_latest_tx_id) = state::with_state(|st| (
            st.current_round_start_time_nanos,
            st.current_round_start_staking_balance_e8s,
            st.current_round_start_latest_tx_id,
        ));
        assert_eq!(round_start_time_nanos, Some(now_secs * 1_000_000_000));
        assert_eq!(round_start_staking_balance_e8s, Some(200_000_000));
        assert_eq!(round_start_latest_tx_id, Some(1));
    }

}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() { main_tick(true).await; }
#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() { rescue_tick().await; }
