use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, Memo, TransferArg, TransferError};

use crate::clients::{ClientError, CmcClient, LedgerClient};
use crate::logic;
use crate::state::{self, PendingTransfer, PendingTransferKind, PendingTransferPhase};

const LEDGER_CREATED_AT_VALID_WINDOW_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;

#[cfg(feature = "debug_api")]
thread_local! {
    static ABORT_AFTER_SUCCESSFUL_TRANSFER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TRAP_AFTER_SUCCESSFUL_TRANSFER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_abort_after_successful_transfer(v: bool) {
    ABORT_AFTER_SUCCESSFUL_TRANSFER.with(|cell| cell.set(v));
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_trap_after_successful_transfer(v: bool) {
    TRAP_AFTER_SUCCESSFUL_TRANSFER.with(|cell| cell.set(v));
}

#[cfg(feature = "debug_api")]
fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
    if TRAP_AFTER_SUCCESSFUL_TRANSFER.with(|cell| cell.replace(false)) {
        return DebugSuccessfulTransferInjection::Trap;
    }
    if ABORT_AFTER_SUCCESSFUL_TRANSFER.with(|cell| cell.replace(false)) {
        return DebugSuccessfulTransferInjection::Abort;
    }
    DebugSuccessfulTransferInjection::None
}

#[cfg(not(feature = "debug_api"))]
fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
    DebugSuccessfulTransferInjection::None
}

#[allow(dead_code)]
enum DebugSuccessfulTransferInjection {
    None,
    Abort,
    Trap,
}

enum TransferAttemptOutcome {
    Accepted(u64),
    ImmediateRetryable,
    Failed,
}

enum NotifyAttemptOutcome {
    Succeeded,
    Retryable,
    Terminal,
}

fn block_index_to_u64(block: &BlockIndex) -> Option<u64> {
    u64::try_from(block.0.clone()).ok()
}

fn transfer_arg(
    to: Account,
    amount_e8s: u64,
    fee_e8s: u64,
    created_at_time_nanos: u64,
    memo_bytes: Vec<u8>,
) -> TransferArg {
    TransferArg {
        from_subaccount: None,
        to,
        fee: Some(Nat::from(fee_e8s)),
        created_at_time: Some(created_at_time_nanos),
        memo: Some(Memo::from(memo_bytes)),
        amount: Nat::from(amount_e8s),
    }
}

fn destination_for_pending(cmc_id: Principal, pending: &PendingTransfer) -> Account {
    match &pending.kind {
        PendingTransferKind::CmcTopUp { canister_id } => {
            logic::cmc_deposit_account(cmc_id, *canister_id)
        }
        PendingTransferKind::RawIcp { account, .. } => *account,
    }
}

fn memo_for_pending(pending: &PendingTransfer) -> Vec<u8> {
    match &pending.kind {
        PendingTransferKind::CmcTopUp { .. } => {
            logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec()
        }
        PendingTransferKind::RawIcp { memo, .. } => memo.clone().unwrap_or_default(),
    }
}

fn created_at_time_is_valid(created_at_time_nanos: u64, now_nanos: u64) -> bool {
    now_nanos.saturating_sub(created_at_time_nanos) <= LEDGER_CREATED_AT_VALID_WINDOW_NANOS
}

async fn transfer_once<L: LedgerClient>(ledger: &L, arg: TransferArg) -> TransferAttemptOutcome {
    match ledger.transfer(arg).await {
        Ok(Ok(block_index)) => block_index_to_u64(&block_index)
            .map(TransferAttemptOutcome::Accepted)
            .unwrap_or(TransferAttemptOutcome::ImmediateRetryable),
        Ok(Err(TransferError::Duplicate { duplicate_of })) => block_index_to_u64(&duplicate_of)
            .map(TransferAttemptOutcome::Accepted)
            .unwrap_or(TransferAttemptOutcome::ImmediateRetryable),
        Ok(Err(
            TransferError::TemporarilyUnavailable
            | TransferError::CreatedInFuture { .. }
            | TransferError::GenericError { .. },
        )) => TransferAttemptOutcome::ImmediateRetryable,
        Ok(Err(
            TransferError::BadFee { .. }
            | TransferError::BadBurn { .. }
            | TransferError::InsufficientFunds { .. }
            | TransferError::TooOld,
        )) => TransferAttemptOutcome::Failed,
        Err(_) => TransferAttemptOutcome::ImmediateRetryable,
    }
}

async fn notify_once<C: CmcClient>(
    cmc: &C,
    canister_id: Principal,
    block_index: u64,
) -> NotifyAttemptOutcome {
    match cmc.notify_top_up(canister_id, block_index).await {
        Ok(()) => NotifyAttemptOutcome::Succeeded,
        Err(ClientError::TerminalNotify(_)) => NotifyAttemptOutcome::Terminal,
        Err(ClientError::RetryableNotify(_) | ClientError::Call(_) | ClientError::Convert(_)) => {
            NotifyAttemptOutcome::Retryable
        }
    }
}

pub(super) async fn drive_pending_transfer<L: LedgerClient, C: CmcClient>(
    ledger: &L,
    cmc: &C,
    cmc_id: Principal,
    now_nanos: u64,
) -> bool {
    let Some(staged) = state::with_state(|st| {
        st.active_job
            .as_ref()
            .and_then(|job| job.pending_transfer.clone())
    }) else {
        return true;
    };

    let accepted = match staged.phase {
        PendingTransferPhase::AwaitingTransfer => {
            if !created_at_time_is_valid(staged.created_at_time_nanos, now_nanos) {
                mark_pending_ambiguous();
                return true;
            }
            let destination = destination_for_pending(cmc_id, &staged);
            let memo = memo_for_pending(&staged);
            let first_arg = transfer_arg(
                destination,
                staged.amount_e8s,
                state::with_state(|st| st.active_job.as_ref().unwrap().fee_e8s),
                staged.created_at_time_nanos,
                memo.clone(),
            );
            let second_arg = transfer_arg(
                destination,
                staged.amount_e8s,
                state::with_state(|st| st.active_job.as_ref().unwrap().fee_e8s),
                staged.created_at_time_nanos,
                memo,
            );
            let block_index = match transfer_once(ledger, first_arg).await {
                TransferAttemptOutcome::Accepted(v) => v,
                TransferAttemptOutcome::ImmediateRetryable => {
                    match transfer_once(ledger, second_arg).await {
                        TransferAttemptOutcome::Accepted(v) => v,
                        TransferAttemptOutcome::ImmediateRetryable
                        | TransferAttemptOutcome::Failed => {
                            mark_pending_ambiguous();
                            return true;
                        }
                    }
                }
                TransferAttemptOutcome::Failed => {
                    mark_pending_failed();
                    return true;
                }
            };
            match debug_successful_transfer_injection() {
                DebugSuccessfulTransferInjection::None => {}
                DebugSuccessfulTransferInjection::Abort => return false,
                DebugSuccessfulTransferInjection::Trap => {
                    ic_cdk::trap("debug trap after successful relay transfer")
                }
            }
            mark_pending_ledger_accepted(block_index);
            state::with_state(|st| {
                st.active_job
                    .as_ref()
                    .unwrap()
                    .pending_transfer
                    .clone()
                    .unwrap()
            })
        }
        PendingTransferPhase::TransferAccepted { .. } => staged,
    };

    let PendingTransferPhase::TransferAccepted { block_index } = accepted.phase else {
        return false;
    };

    if let PendingTransferKind::CmcTopUp { canister_id } = accepted.kind {
        let first = notify_once(cmc, canister_id, block_index).await;
        match first {
            NotifyAttemptOutcome::Succeeded => {
                mark_pending_completed(true);
                true
            }
            NotifyAttemptOutcome::Retryable | NotifyAttemptOutcome::Terminal => {
                match notify_once(cmc, canister_id, block_index).await {
                    NotifyAttemptOutcome::Succeeded => {
                        mark_pending_completed(true);
                        true
                    }
                    NotifyAttemptOutcome::Terminal
                        if matches!(first, NotifyAttemptOutcome::Terminal) =>
                    {
                        mark_pending_failed_after_acceptance();
                        true
                    }
                    NotifyAttemptOutcome::Retryable | NotifyAttemptOutcome::Terminal => {
                        mark_pending_ambiguous_after_acceptance();
                        true
                    }
                }
            }
        }
    } else {
        mark_pending_completed(false);
        true
    }
}

fn mark_pending_ledger_accepted(block_index: u64) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            if let Some(pending) = job.pending_transfer.as_mut() {
                pending.phase = PendingTransferPhase::TransferAccepted { block_index };
                job.summary.transfer_count = job.summary.transfer_count.saturating_add(1);
                job.summary.ledger_transfer_count =
                    job.summary.ledger_transfer_count.saturating_add(1);
                job.summary.ledger_sent_e8s = job
                    .summary
                    .ledger_sent_e8s
                    .saturating_add(pending.amount_e8s);
                job.summary.ledger_fees_e8s =
                    job.summary.ledger_fees_e8s.saturating_add(job.fee_e8s);
            }
        }
    });
}

fn mark_pending_completed(cmc_notify_succeeded: bool) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            if job.pending_transfer.take().is_some() && cmc_notify_succeeded {
                job.summary.cmc_notify_success_count =
                    job.summary.cmc_notify_success_count.saturating_add(1);
            }
        }
    });
}

fn mark_pending_failed() {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            if let Some(pending) = job.pending_transfer.take() {
                job.summary.failed_transfers = job.summary.failed_transfers.saturating_add(1);
                job.summary.known_unspent_e8s = job
                    .summary
                    .known_unspent_e8s
                    .saturating_add(pending.gross_share_e8s);
            }
        }
    });
}

fn mark_pending_failed_after_acceptance() {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            if job.pending_transfer.take().is_some() {
                job.summary.failed_transfers = job.summary.failed_transfers.saturating_add(1);
                job.summary.cmc_notify_failed_count =
                    job.summary.cmc_notify_failed_count.saturating_add(1);
            }
        }
    });
}

fn mark_pending_ambiguous() {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            if let Some(pending) = job.pending_transfer.take() {
                job.summary.ambiguous_transfers = job.summary.ambiguous_transfers.saturating_add(1);
                job.summary.ambiguous_e8s = job
                    .summary
                    .ambiguous_e8s
                    .saturating_add(pending.gross_share_e8s);
            }
        }
    });
}

fn mark_pending_ambiguous_after_acceptance() {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            if let Some(pending) = job.pending_transfer.take() {
                job.summary.ambiguous_transfers = job.summary.ambiguous_transfers.saturating_add(1);
                job.summary.cmc_notify_ambiguous_count =
                    job.summary.cmc_notify_ambiguous_count.saturating_add(1);
                job.summary.ambiguous_e8s = job
                    .summary
                    .ambiguous_e8s
                    .saturating_add(pending.gross_share_e8s);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::state::{ActiveRelayJob, ActiveRelayMode, Config, RelayMode, RelaySummary, State};

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn install_pending_job(phase: PendingTransferPhase) {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let cfg = Config {
            managed_canisters: vec![canister],
            ledger_canister_id: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
            cmc_canister_id: principal("rkp4c-7iaaa-aaaaa-aaaca-cai"),
            blackhole_canister_id: principal("77deu-baaaa-aaaar-qb6za-cai"),
            main_interval_seconds: 60,
            max_transfers_per_tick: None,
            raw_icp_mode: None,
        };
        let mut summary = RelaySummary::started(RelayMode::CyclesTopUp, 1, 1);
        summary.planned_retained_e8s = 100;
        summary.known_unspent_e8s = 100;
        let mut st = State::new(cfg, 1);
        st.active_job = Some(ActiveRelayJob {
            id: 1,
            mode: ActiveRelayMode::CyclesTopUp,
            started_at_ts_nanos: 1,
            fee_e8s: 10,
            balance_start_e8s: 1_000,
            current_cycles: BTreeMap::new(),
            canisters: Vec::new(),
            raw_recipients: Vec::new(),
            pending_transfer: Some(PendingTransfer {
                kind: PendingTransferKind::CmcTopUp {
                    canister_id: canister,
                },
                gross_share_e8s: 900,
                amount_e8s: 890,
                created_at_time_nanos: 1,
                phase,
            }),
            next_transfer_index: 0,
            next_created_at_time_nanos: 2,
            summary,
        });
        state::set_state(st);
    }

    #[test]
    fn ledger_acceptance_is_counted_before_cmc_notify_finishes() {
        install_pending_job(PendingTransferPhase::AwaitingTransfer);
        mark_pending_ledger_accepted(7);
        state::with_state(|st| {
            let summary = &st.active_job.as_ref().unwrap().summary;
            assert_eq!(summary.ledger_transfer_count, 1);
            assert_eq!(summary.ledger_sent_e8s, 890);
            assert_eq!(summary.ledger_fees_e8s, 10);
            assert_eq!(summary.cmc_notify_success_count, 0);
            assert_eq!(summary.known_unspent_e8s, 100);
        });
    }

    #[test]
    fn cmc_ambiguous_after_acceptance_keeps_ledger_spend_and_marks_ambiguous_gross() {
        install_pending_job(PendingTransferPhase::AwaitingTransfer);
        mark_pending_ledger_accepted(7);
        mark_pending_ambiguous_after_acceptance();
        state::with_state(|st| {
            let summary = &st.active_job.as_ref().unwrap().summary;
            assert_eq!(summary.ledger_transfer_count, 1);
            assert_eq!(summary.cmc_notify_ambiguous_count, 1);
            assert_eq!(summary.ambiguous_transfers, 1);
            assert_eq!(summary.ambiguous_e8s, 900);
            assert_eq!(summary.known_unspent_e8s, 100);
        });
    }

    #[test]
    fn failed_before_acceptance_is_known_unspent_not_ledger_spend() {
        install_pending_job(PendingTransferPhase::AwaitingTransfer);
        mark_pending_failed();
        state::with_state(|st| {
            let summary = &st.active_job.as_ref().unwrap().summary;
            assert_eq!(summary.ledger_transfer_count, 0);
            assert_eq!(summary.failed_transfers, 1);
            assert_eq!(summary.known_unspent_e8s, 1_000);
            assert_eq!(summary.ambiguous_e8s, 0);
        });
    }

    #[test]
    fn terminal_cmc_failure_after_acceptance_keeps_ledger_spend_separate_from_unspent() {
        install_pending_job(PendingTransferPhase::TransferAccepted { block_index: 7 });
        mark_pending_failed_after_acceptance();
        state::with_state(|st| {
            let summary = &st.active_job.as_ref().unwrap().summary;
            assert_eq!(summary.cmc_notify_failed_count, 1);
            assert_eq!(summary.failed_transfers, 1);
            assert_eq!(summary.known_unspent_e8s, 100);
            assert_eq!(summary.ambiguous_e8s, 0);
        });
    }
}
