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
        TransferKind::CyclesTopUpRawFallback
        | TransferKind::RawIcp
        | TransferKind::RemainderToRelay => Account {
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
        TransferKind::Beneficiary => logic::cmc_deposit_account(cmc_id, pending.beneficiary),
    }
}

pub(super) fn transfer_memo_for_pending(pending: &PendingNotification) -> Vec<u8> {
    match pending.kind {
        TransferKind::CyclesTopUpRawFallback | TransferKind::RemainderToRelay => Vec::new(),
        TransferKind::Beneficiary => pending
            .transfer_memo
            .clone()
            .unwrap_or_else(|| logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec()),
        TransferKind::RawIcp => pending.transfer_memo.clone().unwrap_or_default(),
        TransferKind::NeuronStake => pending
            .transfer_memo
            .clone()
            .unwrap_or_else(|| logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferIdentityOutcome {
    Accepted(u64),
    DefiniteNoDebit,
    Uncertain,
    AcceptedWithUnusableBlockIndex,
}

async fn transfer_identity_with_one_retry(
    ledger: &impl LedgerClient,
    first_arg: TransferArg,
    second_arg: TransferArg,
) -> TransferIdentityOutcome {
    let first = transfer_once(ledger, first_arg).await;
    match first {
        TransferAttemptOutcome::Accepted(block_index) => {
            TransferIdentityOutcome::Accepted(block_index)
        }
        TransferAttemptOutcome::AcceptedWithUnusableBlockIndex => {
            TransferIdentityOutcome::AcceptedWithUnusableBlockIndex
        }
        TransferAttemptOutcome::DefiniteNoDebit { retryable: false } => {
            TransferIdentityOutcome::DefiniteNoDebit
        }
        TransferAttemptOutcome::DefiniteNoDebit { retryable: true } => {
            match transfer_once(ledger, second_arg).await {
                TransferAttemptOutcome::Accepted(block_index) => {
                    TransferIdentityOutcome::Accepted(block_index)
                }
                TransferAttemptOutcome::AcceptedWithUnusableBlockIndex => {
                    TransferIdentityOutcome::AcceptedWithUnusableBlockIndex
                }
                TransferAttemptOutcome::DefiniteNoDebit { .. } => {
                    TransferIdentityOutcome::DefiniteNoDebit
                }
                TransferAttemptOutcome::Uncertain => TransferIdentityOutcome::Uncertain,
            }
        }
        TransferAttemptOutcome::Uncertain => match transfer_once(ledger, second_arg).await {
            TransferAttemptOutcome::Accepted(block_index) => {
                TransferIdentityOutcome::Accepted(block_index)
            }
            TransferAttemptOutcome::AcceptedWithUnusableBlockIndex => {
                TransferIdentityOutcome::AcceptedWithUnusableBlockIndex
            }
            TransferAttemptOutcome::DefiniteNoDebit { .. } | TransferAttemptOutcome::Uncertain => {
                TransferIdentityOutcome::Uncertain
            }
        },
    }
}

fn fresh_transition_created_at_time(
    job: &mut ActivePayoutJob,
    now_nanos: u64,
    previous_created_at_time_nanos: u64,
) -> Option<u64> {
    let mut created_at_time_nanos = job.next_created_at_time_nanos;
    if !created_at_time_is_valid_for_ledger(created_at_time_nanos, now_nanos) {
        created_at_time_nanos = now_nanos;
    }
    if created_at_time_nanos == previous_created_at_time_nanos {
        created_at_time_nanos = created_at_time_nanos.checked_add(1)?;
    }
    if !created_at_time_is_valid_for_ledger(created_at_time_nanos, now_nanos) {
        return None;
    }
    job.next_created_at_time_nanos = created_at_time_nanos.checked_add(1)?;
    Some(created_at_time_nanos)
}

fn stage_raw_fallback_after_definite_rejection(now_nanos: u64) -> Result<bool, ()> {
    state::with_state_mut(|st| {
        let Some(job) = st.active_payout_job.as_mut() else {
            return Ok(false);
        };
        let Some(current) = job.pending_transfer.clone() else {
            return Ok(false);
        };
        if current.phase != PendingTransferPhase::AwaitingTransfer
            || current.notification.kind != TransferKind::Beneficiary
        {
            return Ok(false);
        }
        let Some(created_at_time_nanos) =
            fresh_transition_created_at_time(job, now_nanos, current.created_at_time_nanos)
        else {
            return Err(());
        };
        let mut notification = current.notification;
        notification.kind = TransferKind::CyclesTopUpRawFallback;
        notification.block_index = 0;
        notification.transfer_memo = Some(Vec::new());
        notification.destination_subaccount = None;
        notification.neuron_id = None;
        job.pending_transfer = Some(PendingTransfer {
            notification,
            created_at_time_nanos,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        Ok(true)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefundFallbackTransition {
    Staged,
    BelowFee,
    InvariantBroken,
}

fn transition_refunded_beneficiary_to_raw_fallback(
    accepted: &PendingNotification,
    fee_e8s: u64,
    now_nanos: u64,
) -> RefundFallbackTransition {
    state::with_state_mut(|st| {
        let Some(job) = st.active_payout_job.as_mut() else {
            return RefundFallbackTransition::InvariantBroken;
        };
        let Some(current) = job.pending_transfer.clone() else {
            return RefundFallbackTransition::InvariantBroken;
        };
        if current.phase != PendingTransferPhase::TransferAccepted
            || current.notification != *accepted
            || accepted.kind != TransferKind::Beneficiary
        {
            return RefundFallbackTransition::InvariantBroken;
        }

        let Some(original_amount_e8s) = accepted.gross_share_e8s.checked_sub(fee_e8s) else {
            return RefundFallbackTransition::InvariantBroken;
        };
        if original_amount_e8s != accepted.amount_e8s {
            return RefundFallbackTransition::InvariantBroken;
        }
        let Some(cmc_refund_top_up_fee_e8s) = fee_e8s.checked_mul(2) else {
            return RefundFallbackTransition::InvariantBroken;
        };
        let Some(refund_credit_e8s) = original_amount_e8s
            .checked_sub(fee_e8s)
            .and_then(|value| value.checked_sub(cmc_refund_top_up_fee_e8s))
        else {
            return RefundFallbackTransition::InvariantBroken;
        };
        let Some(released_gross_outflow_e8s) = job.gross_outflow_e8s.checked_sub(refund_credit_e8s)
        else {
            return RefundFallbackTransition::InvariantBroken;
        };

        if refund_credit_e8s <= fee_e8s {
            job.gross_outflow_e8s = released_gross_outflow_e8s;
            return RefundFallbackTransition::BelowFee;
        }

        let Some(created_at_time_nanos) =
            fresh_transition_created_at_time(job, now_nanos, current.created_at_time_nanos)
        else {
            return RefundFallbackTransition::InvariantBroken;
        };
        let Some(raw_amount_e8s) = refund_credit_e8s.checked_sub(fee_e8s) else {
            return RefundFallbackTransition::InvariantBroken;
        };
        job.gross_outflow_e8s = released_gross_outflow_e8s;
        job.pending_transfer = Some(PendingTransfer {
            notification: PendingNotification {
                kind: TransferKind::CyclesTopUpRawFallback,
                beneficiary: accepted.beneficiary,
                gross_share_e8s: refund_credit_e8s,
                amount_e8s: raw_amount_e8s,
                block_index: 0,
                next_start: accepted.next_start,
                transfer_memo: Some(Vec::new()),
                destination_subaccount: None,
                neuron_id: None,
            },
            created_at_time_nanos,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        RefundFallbackTransition::Staged
    })
}

fn record_accepted_transfer_with_unusable_block_index() -> bool {
    state::with_state_mut(|st| {
        let Some(job) = st.active_payout_job.as_mut() else {
            return false;
        };
        let Some(pending) = job.pending_transfer.take() else {
            return false;
        };
        let accounting_broken = match job
            .gross_outflow_e8s
            .checked_add(pending.notification.gross_share_e8s)
        {
            Some(total) if total <= job.pot_start_e8s => {
                job.gross_outflow_e8s = total;
                false
            }
            _ => {
                // The ledger accepted the transfer, so conservatively reserve every remaining e8
                // rather than risk releasing accepted value into remainder cleanup.
                job.gross_outflow_e8s = job.pot_start_e8s;
                true
            }
        };
        if pending.notification.kind.is_beneficiary_payout() {
            job.ambiguous_topups = job.ambiguous_topups.saturating_add(1);
        }
        accounting_broken
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmbiguousPendingGrossReservation {
    NotApplicable,
    Reserved,
    InvariantBroken,
}

fn reserve_ambiguous_awaiting_transfer_gross() -> AmbiguousPendingGrossReservation {
    state::with_state_mut(|st| {
        let Some(job) = st.active_payout_job.as_mut() else {
            return AmbiguousPendingGrossReservation::NotApplicable;
        };
        let Some(pending) = job.pending_transfer.as_ref() else {
            return AmbiguousPendingGrossReservation::NotApplicable;
        };
        if pending.phase != PendingTransferPhase::AwaitingTransfer
            || !matches!(
                pending.notification.kind,
                TransferKind::CyclesTopUpRawFallback | TransferKind::RemainderToRelay
            )
        {
            return AmbiguousPendingGrossReservation::NotApplicable;
        }

        match job
            .gross_outflow_e8s
            .checked_add(pending.notification.gross_share_e8s)
        {
            Some(total) if total <= job.pot_start_e8s => {
                job.gross_outflow_e8s = total;
                AmbiguousPendingGrossReservation::Reserved
            }
            _ => {
                job.gross_outflow_e8s = job.pot_start_e8s;
                AmbiguousPendingGrossReservation::InvariantBroken
            }
        }
    })
}

fn reserve_then_clear_ambiguous_awaiting_transfer() {
    if reserve_ambiguous_awaiting_transfer_gross()
        == AmbiguousPendingGrossReservation::InvariantBroken
    {
        state::latch_forced_rescue_reason(ForcedRescueReason::AccountingInvariantBroken);
    }
    clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
}

fn is_proven_refund_with_block(outcome: &NotifyAttemptOutcome) -> bool {
    matches!(
        outcome,
        NotifyAttemptOutcome::Error(jupiter_ic_clients::cmc::NotifyTopUpError::Terminal(
            jupiter_ic_clients::cmc::NotifyTerminalError::Refunded {
                block_index: Some(_),
                ..
            }
        ))
    )
}

fn is_terminal_notify_error(outcome: &NotifyAttemptOutcome) -> bool {
    matches!(
        outcome,
        NotifyAttemptOutcome::Error(jupiter_ic_clients::cmc::NotifyTopUpError::Terminal(_))
    )
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
    let mut fallback_transitions = 0u8;
    loop {
        let Some(staged) = current_pending_transfer() else {
            return true;
        };

        let accepted = match staged.phase {
            PendingTransferPhase::AwaitingTransfer => {
                if !created_at_time_is_valid_for_ledger(staged.created_at_time_nanos, now_nanos) {
                    // Once the created_at_time expires we can no longer safely distinguish “never accepted”
                    // from “accepted but the reply was lost”, so we surface this as ambiguous rather than failed.
                    reserve_then_clear_ambiguous_awaiting_transfer();
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

                let block_index =
                    match transfer_identity_with_one_retry(ledger, first_arg, second_arg).await {
                        TransferIdentityOutcome::Accepted(v) => v,
                        TransferIdentityOutcome::DefiniteNoDebit => {
                            if fallback_transitions == 0
                                && staged.notification.kind == TransferKind::Beneficiary
                            {
                                match stage_raw_fallback_after_definite_rejection(now_nanos) {
                                    Ok(true) => {
                                        fallback_transitions = 1;
                                        continue;
                                    }
                                    Ok(false) => {}
                                    Err(()) => {
                                        state::latch_forced_rescue_reason(
                                            ForcedRescueReason::AccountingInvariantBroken,
                                        );
                                    }
                                }
                            }
                            clear_pending_transfer(PendingTransferTerminalStatus::Failed);
                            return true;
                        }
                        TransferIdentityOutcome::Uncertain => {
                            reserve_then_clear_ambiguous_awaiting_transfer();
                            return true;
                        }
                        TransferIdentityOutcome::AcceptedWithUnusableBlockIndex => {
                            if record_accepted_transfer_with_unusable_block_index() {
                                state::latch_forced_rescue_reason(
                                    ForcedRescueReason::AccountingInvariantBroken,
                                );
                            }
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
        if matches!(first_notify, NotifyAttemptOutcome::Succeeded) {
            record_completed_transfer(now_secs, &accepted);
            return true;
        }
        if accepted.kind == TransferKind::Beneficiary && is_proven_refund_with_block(&first_notify)
        {
            match transition_refunded_beneficiary_to_raw_fallback(&accepted, fee_e8s, now_nanos) {
                RefundFallbackTransition::Staged => {
                    debug_assert_eq!(fallback_transitions, 0);
                    fallback_transitions = 1;
                    continue;
                }
                RefundFallbackTransition::BelowFee => {
                    clear_pending_transfer(PendingTransferTerminalStatus::Failed);
                    return true;
                }
                RefundFallbackTransition::InvariantBroken => {
                    state::latch_forced_rescue_reason(
                        ForcedRescueReason::AccountingInvariantBroken,
                    );
                    clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                    return true;
                }
            }
        }

        // Once the ledger transfer is accepted, a duplicate-safe notify retry can improve the
        // final classification without risking an extra outflow. A proven refund is already
        // terminal and deliberately bypasses this second notification.
        let second_notify = notify_once(cmc, &accepted).await;
        if matches!(second_notify, NotifyAttemptOutcome::Succeeded) {
            record_completed_transfer(now_secs, &accepted);
            return true;
        }
        if accepted.kind == TransferKind::Beneficiary && is_proven_refund_with_block(&second_notify)
        {
            match transition_refunded_beneficiary_to_raw_fallback(&accepted, fee_e8s, now_nanos) {
                RefundFallbackTransition::Staged => {
                    debug_assert_eq!(fallback_transitions, 0);
                    fallback_transitions = 1;
                    continue;
                }
                RefundFallbackTransition::BelowFee => {
                    clear_pending_transfer(PendingTransferTerminalStatus::Failed);
                    return true;
                }
                RefundFallbackTransition::InvariantBroken => {
                    state::latch_forced_rescue_reason(
                        ForcedRescueReason::AccountingInvariantBroken,
                    );
                    clear_pending_transfer(PendingTransferTerminalStatus::Ambiguous);
                    return true;
                }
            }
        }
        let status = if is_terminal_notify_error(&first_notify)
            && is_terminal_notify_error(&second_notify)
        {
            PendingTransferTerminalStatus::Failed
        } else {
            PendingTransferTerminalStatus::Ambiguous
        };
        clear_pending_transfer(status);
        return true;
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
                    .checked_add(pending.gross_share_e8s)
                    .map(|total| total > job.pot_start_e8s)
                    .unwrap_or(true)
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
