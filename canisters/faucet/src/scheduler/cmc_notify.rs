use super::*;
pub(super) fn created_at_time_is_valid_for_ledger(
    created_at_time_nanos: u64,
    now_nanos: u64,
) -> bool {
    let not_too_old =
        now_nanos.saturating_sub(created_at_time_nanos) <= LEDGER_CREATED_AT_MAX_AGE_NANOS;
    let not_too_far_in_future =
        created_at_time_nanos <= now_nanos.saturating_add(LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS);
    not_too_old && not_too_far_in_future
}

pub(super) fn allocate_created_at_time_nanos(now_nanos: u64) -> u64 {
    state::with_state_mut(|st| {
        let job = st
            .active_payout_job
            .as_mut()
            .expect("active payout job missing");
        if !created_at_time_is_valid_for_ledger(job.next_created_at_time_nanos, now_nanos) {
            job.next_created_at_time_nanos = now_nanos;
        }
        let created_at_time_nanos = job.next_created_at_time_nanos;
        job.next_created_at_time_nanos = created_at_time_nanos.saturating_add(1);
        created_at_time_nanos
    })
}
pub(super) fn increment_cmc_attempts(pending: &PendingNotification) {
    if !pending.kind.requires_cmc_notify() || pending.kind == TransferKind::RemainderToSelf {
        return;
    }
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            job.cmc_attempt_count = Some(job.cmc_attempt_count.unwrap_or(0).saturating_add(1));
        }
    });
}

pub(super) fn note_attempted_beneficiary(pending: &PendingNotification) {
    if !pending.kind.requires_cmc_notify() || pending.kind == TransferKind::RemainderToSelf {
        return;
    }
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            let beneficiaries = job.cmc_attempted_beneficiaries.get_or_insert_with(Vec::new);
            if !beneficiaries.contains(&pending.beneficiary) {
                beneficiaries.push(pending.beneficiary);
            }
        }
    });
}

pub(super) fn increment_cmc_successes(pending: &PendingNotification) {
    if !pending.kind.requires_cmc_notify() || pending.kind == TransferKind::RemainderToSelf {
        return;
    }
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            job.cmc_success_count = Some(job.cmc_success_count.unwrap_or(0).saturating_add(1));
        }
    });
}

pub(super) fn current_pending_transfer() -> Option<PendingTransfer> {
    state::with_state(|st| {
        st.active_payout_job
            .as_ref()
            .and_then(|job| job.pending_transfer.clone())
    })
}

pub(super) fn stage_pending_transfer(pending: PendingNotification, created_at_time_nanos: u64) {
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

pub(super) fn mark_pending_transfer_accepted(block_index: u64) -> Option<PendingNotification> {
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
pub(super) enum PendingTransferTerminalStatus {
    Failed,
    Ambiguous,
}

pub(super) fn clear_pending_transfer(status: PendingTransferTerminalStatus) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            if job
                .pending_transfer
                .as_ref()
                .map(|pending| pending.notification.kind.is_beneficiary_payout())
                .unwrap_or(false)
            {
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
pub(super) fn note_index_page(resp: &GetAccountIdentifierTransactionsResponse) {
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
pub(super) enum TransferAttemptOutcome {
    Accepted(u64),
    ImmediateRetryable,
    Failed,
}

pub(super) async fn transfer_once(
    ledger: &impl LedgerClient,
    arg: TransferArg,
) -> TransferAttemptOutcome {
    debug_assert!(
        !state::persistence_batch_active(),
        "persistence batch must be dropped before ledger transfer"
    );
    match ledger.transfer(arg).await {
        Err(_) => TransferAttemptOutcome::ImmediateRetryable,
        Ok(Ok(block)) => match u64::try_from(block.0.clone()) {
            Ok(v) => TransferAttemptOutcome::Accepted(v),
            Err(_) => TransferAttemptOutcome::Failed,
        },
        Ok(Err(TransferError::Duplicate { duplicate_of })) => {
            match u64::try_from(duplicate_of.0.clone()) {
                Ok(v) => TransferAttemptOutcome::Accepted(v),
                Err(_) => TransferAttemptOutcome::Failed,
            }
        }
        Ok(Err(TransferError::TemporarilyUnavailable)) => {
            TransferAttemptOutcome::ImmediateRetryable
        }
        Ok(Err(_)) => TransferAttemptOutcome::Failed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NotifyAttemptOutcome {
    Succeeded,
    Retryable,
    Terminal,
}

pub(super) async fn notify_once(
    cmc: &impl CmcClient,
    pending: &PendingNotification,
) -> NotifyAttemptOutcome {
    debug_assert!(
        !state::persistence_batch_active(),
        "persistence batch must be dropped before CMC notify"
    );
    note_attempted_beneficiary(pending);
    increment_cmc_attempts(pending);
    match cmc
        .notify_top_up(pending.beneficiary, pending.block_index)
        .await
    {
        Ok(()) => NotifyAttemptOutcome::Succeeded,
        Err(crate::clients::ClientError::TerminalNotify(_)) => NotifyAttemptOutcome::Terminal,
        Err(crate::clients::ClientError::RetryableNotify(_))
        | Err(crate::clients::ClientError::Call(_))
        | Err(crate::clients::ClientError::Convert(_)) => NotifyAttemptOutcome::Retryable,
    }
}
pub(super) fn record_completed_transfer(now_secs: u64, pending: &PendingNotification) {
    state::with_state_mut(|st| {
        if pending.kind.requires_cmc_notify() {
            st.last_successful_transfer_ts = Some(now_secs);
        }
        if let Some(job) = st.active_payout_job.as_mut() {
            logic::apply_notified_transfer(job, pending);
            job.pending_transfer = None;
        }
    });
    if pending.kind.requires_cmc_notify() {
        increment_cmc_successes(pending);
    }
}
pub(super) async fn finalize_completed_job(status_client: &impl CanisterStatusClient) {
    let Some(job) = state::with_state_mut(|st| st.active_payout_job.take()) else {
        return;
    };
    let zero_success_run_counts = zero_success_run_counts_toward_rescue(status_client, &job).await;
    let summary = state::with_state_mut(|st| {
        apply_job_health_observations(st, &job, zero_success_run_counts);
        if let Some(round_end_time_nanos) = job.round_end_time_nanos {
            st.current_round_start_time_nanos = Some(round_end_time_nanos);
            st.current_round_start_staking_balance_e8s = Some(
                job.round_end_staking_balance_e8s
                    .unwrap_or(job.denom_staking_balance_e8s),
            );
            st.current_round_start_latest_tx_id =
                job.round_end_latest_tx_id.or(job.observed_latest_tx_id);
        }
        if let Some(funding_tx_id) = job.funding_tx_id {
            st.last_processed_funding_tx_id = Some(funding_tx_id);
        }
        let summary = logic::summary_from_job(&job);
        st.last_summary = Some(summary.clone());
        summary
    });
    log_summary(&summary);
}
