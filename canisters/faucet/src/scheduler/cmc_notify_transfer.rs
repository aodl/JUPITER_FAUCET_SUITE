use super::*;
pub(super) fn transfer_arg(
    to: Account,
    amount_e8s: u64,
    fee_e8s: u64,
    created_at_time_nanos: u64,
    memo_bytes: Vec<u8>,
) -> TransferArg {
    TransferArg {
        from_subaccount: state::with_state(|st| st.config.payout_subaccount),
        to,
        fee: Some(Nat::from(fee_e8s)),
        created_at_time: Some(created_at_time_nanos),
        memo: Some(Memo::from(memo_bytes)),
        amount: Nat::from(amount_e8s),
    }
}

pub(super) fn deposit_account_for_pending(
    cmc_id: candid::Principal,
    pending: &PendingNotification,
) -> Account {
    match pending.kind {
        TransferKind::RawIcp => Account {
            owner: pending.beneficiary,
            subaccount: None,
        },
        TransferKind::NeuronStake => Account {
            owner: pending.beneficiary,
            subaccount: Some(
                pending
                    .destination_subaccount
                    .expect("neuron stake pending transfer must include staking subaccount"),
            ),
        },
        TransferKind::Beneficiary | TransferKind::RemainderToSelf => {
            logic::cmc_deposit_account(cmc_id, pending.beneficiary)
        }
    }
}

pub(super) fn transfer_memo_for_pending(pending: &PendingNotification) -> Vec<u8> {
    pending
        .transfer_memo
        .clone()
        .unwrap_or_else(|| logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec())
}

pub(super) async fn drive_pending_transfer(
    ledger: &impl LedgerClient,
    cmc: &impl CmcClient,
    governance: &impl GovernanceClient,
    cmc_id: Principal,
    fee_e8s: u64,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
    let Some(staged) = current_pending_transfer() else {
        return true;
    };

    let accepted = match staged.phase {
        PendingTransferPhase::AwaitingTransfer => {
            if !created_at_time_is_valid_for_ledger(staged.created_at_time_nanos, now_nanos) {
                // Once the created_at_time expires we can no longer safely distinguish “never accepted”
                // from “accepted but the reply was lost”, so we surface this as ambiguous rather than failed.
                clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                return true;
            }

            let to = deposit_account_for_pending(cmc_id, &staged.notification);
            let memo_bytes = transfer_memo_for_pending(&staged.notification);
            let first_arg = transfer_arg(
                to,
                staged.notification.amount_e8s,
                fee_e8s,
                staged.created_at_time_nanos,
                memo_bytes.clone(),
            );
            let second_arg = transfer_arg(
                to,
                staged.notification.amount_e8s,
                fee_e8s,
                staged.created_at_time_nanos,
                memo_bytes,
            );

            let block_index = match transfer_once(ledger, first_arg).await {
                TransferAttemptOutcome::Accepted(v) => v,
                TransferAttemptOutcome::ImmediateRetryable => {
                    match transfer_once(ledger, second_arg).await {
                        TransferAttemptOutcome::Accepted(v) => v,
                        TransferAttemptOutcome::ImmediateRetryable
                        | TransferAttemptOutcome::Failed => {
                            clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                            return true;
                        }
                    }
                }
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
                DebugSuccessfulTransferInjection::Trap => {
                    ic_cdk::trap("debug trap after successful faucet transfer")
                }
            };

            match mark_pending_transfer_accepted(block_index) {
                Some(accepted) => accepted,
                None => return true,
            }
        }
        PendingTransferPhase::TransferAccepted => staged.notification,
    };

    if !accepted.kind.requires_cmc_notify() {
        if let TransferKind::NeuronStake = accepted.kind {
            if let Some(neuron_id) = accepted.neuron_id {
                debug_assert!(
                    !state::persistence_batch_active(),
                    "persistence batch must be dropped before neuron claim/refresh"
                );
                let _ = governance.claim_or_refresh_neuron(neuron_id).await;
            }
        }
        record_completed_transfer(now_secs, &accepted);
        return true;
    }

    let first_notify = notify_once(cmc, &accepted).await;
    match first_notify {
        NotifyAttemptOutcome::Succeeded => {
            record_completed_transfer(now_secs, &accepted);
            true
        }
        NotifyAttemptOutcome::Retryable | NotifyAttemptOutcome::Terminal => {
            // Once the ledger transfer is accepted, a duplicate-safe notify retry can improve the
            // final classification without risking an extra outflow. Two terminal replies mean the
            // beneficiary top-up deterministically failed; any transport/retryable uncertainty left
            // after the single inline retry is surfaced as ambiguous.
            match notify_once(cmc, &accepted).await {
                NotifyAttemptOutcome::Succeeded => {
                    record_completed_transfer(now_secs, &accepted);
                    true
                }
                NotifyAttemptOutcome::Terminal
                    if matches!(first_notify, NotifyAttemptOutcome::Terminal) =>
                {
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

// This helper keeps the ledger/CMC/governance clients explicit for scheduler unit tests.
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_and_notify(
    ledger: &impl LedgerClient,
    cmc: &impl CmcClient,
    governance: &impl GovernanceClient,
    pending: PendingNotification,
    fee_e8s: u64,
    now_nanos: u64,
    now_secs: u64,
    cmc_id: Principal,
) -> bool {
    let invariant_broken = state::with_state(|st| {
        st.active_payout_job
            .as_ref()
            .map(|job| {
                job.gross_outflow_e8s
                    .saturating_add(pending.gross_share_e8s)
                    > job.pot_start_e8s
            })
            .unwrap_or(false)
    });
    if invariant_broken {
        state::latch_forced_rescue_reason(ForcedRescueReason::AccountingInvariantBroken);
        return false;
    }
    let created_at_time_nanos = allocate_created_at_time_nanos(now_nanos);
    stage_pending_transfer(pending, created_at_time_nanos);
    drive_pending_transfer(
        ledger, cmc, governance, cmc_id, fee_e8s, now_nanos, now_secs,
    )
    .await
}
