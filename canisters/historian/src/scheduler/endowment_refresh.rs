use super::*;
use crate::{
    EndowmentIndexProgress, EndowmentTransactionStatusResponse, ExpectedEndowmentStatus,
    RefreshEndowmentsOutcome, RefreshEndowmentsResponse,
};

pub(super) const ENDOWMENT_REFRESH_MIN_INTERVAL_SECONDS: u64 = 60;
pub(super) const ENDOWMENT_REFRESH_MAX_PAGES: u32 = 1;
pub(super) const ENDOWMENT_REFRESH_MAX_BACKOFF_SECONDS: u64 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndowmentRefreshAdmission {
    Available,
    Busy { retry_after_ts: u64 },
    RateLimited { retry_after_ts: u64 },
}

pub(crate) fn endowment_refresh_admission(now_secs: u64) -> EndowmentRefreshAdmission {
    state::with_state(|st| {
        if st.endowment_refresh_next_allowed_ts > now_secs {
            return EndowmentRefreshAdmission::RateLimited {
                retry_after_ts: st.endowment_refresh_next_allowed_ts,
            };
        }
        if st.commitment_index_lock_expires_at_ts.unwrap_or(0) > now_secs {
            return EndowmentRefreshAdmission::Busy {
                retry_after_ts: st.commitment_index_lock_expires_at_ts.unwrap_or(now_secs),
            };
        }
        EndowmentRefreshAdmission::Available
    })
}

fn expected_transaction_status(expected_transaction_id: u64) -> ExpectedEndowmentStatus {
    state::with_state(|st| {
        let qualifying = st
            .recent_commitments
            .iter()
            .flatten()
            .any(|item| item.tx_id == expected_transaction_id)
            || st
                .recent_neuron_commitments
                .iter()
                .flatten()
                .any(|item| item.tx_id == expected_transaction_id);
        if qualifying {
            return ExpectedEndowmentStatus::KnownIndexed;
        }
        let non_qualifying = st
            .recent_under_threshold_commitments
            .iter()
            .flatten()
            .any(|item| item.tx_id == expected_transaction_id)
            || st
                .recent_under_threshold_neuron_commitments
                .iter()
                .flatten()
                .any(|item| item.tx_id == expected_transaction_id)
            || st
                .recent_invalid_commitments
                .iter()
                .flatten()
                .any(|item| item.tx_id == expected_transaction_id);
        if non_qualifying {
            return ExpectedEndowmentStatus::ObservedNotQualifying;
        }
        if let Some(progress) = st.active_staking_catch_up.as_ref() {
            if expected_transaction_id > progress.observed_head_tx_id
                || (expected_transaction_id > progress.boundary_tx_id
                    && progress
                        .next_start_tx_id
                        .is_some_and(|next| expected_transaction_id < next))
            {
                return ExpectedEndowmentStatus::NotYetObserved;
            }
            return ExpectedEndowmentStatus::NotFoundInRetainedEvidence;
        }
        match (
            st.last_indexed_staking_tx_id,
            st.oldest_indexed_staking_tx_id,
        ) {
            (Some(head), _) if expected_transaction_id > head => {
                ExpectedEndowmentStatus::NotYetObserved
            }
            (_, Some(oldest))
                if st.staking_backfill_complete != Some(true)
                    && expected_transaction_id < oldest =>
            {
                ExpectedEndowmentStatus::NotYetObserved
            }
            (Some(_), _) => ExpectedEndowmentStatus::NotFoundInRetainedEvidence,
            _ => ExpectedEndowmentStatus::NotYetObserved,
        }
    })
}

pub(crate) fn progress_snapshot(
    newly_indexed_qualifying_endowments: u64,
    retry_after_ts: Option<u64>,
) -> EndowmentIndexProgress {
    state::with_state(|st| EndowmentIndexProgress {
        revision: st.commitment_index_revision,
        newly_indexed_qualifying_endowments,
        complete_from_genesis: state::commitment_index_is_complete(st),
        committed_head_staking_tx_id: st.last_indexed_staking_tx_id,
        oldest_indexed_staking_tx_id: st.oldest_indexed_staking_tx_id,
        observed_head_staking_tx_id: st
            .active_staking_catch_up
            .as_ref()
            .map(|progress| progress.observed_head_tx_id)
            .or(st.last_indexed_staking_tx_id),
        next_staking_start_tx_id: st
            .active_staking_catch_up
            .as_ref()
            .and_then(|progress| progress.next_start_tx_id),
        commitment_index_fault: st.commitment_index_fault.clone(),
        retry_after_ts,
    })
}

pub(crate) fn endowment_transaction_status(
    transaction_id: u64,
) -> EndowmentTransactionStatusResponse {
    let status = expected_transaction_status(transaction_id);
    state::with_state(|st| EndowmentTransactionStatusResponse {
        transaction_id,
        status,
        revision: st.commitment_index_revision,
        complete_from_genesis: state::commitment_index_is_complete(st),
        commitment_index_fault: st.commitment_index_fault.clone(),
    })
}

fn denied_response(
    outcome: RefreshEndowmentsOutcome,
    retry_after_ts: u64,
) -> RefreshEndowmentsResponse {
    RefreshEndowmentsResponse {
        outcome,
        progress: progress_snapshot(0, Some(retry_after_ts)),
    }
}

fn ineffective_backoff_seconds(streak: u8) -> u64 {
    match streak {
        0 | 1 => ENDOWMENT_REFRESH_MIN_INTERVAL_SECONDS,
        2 => 120,
        3 => 240,
        4 => 480,
        _ => ENDOWMENT_REFRESH_MAX_BACKOFF_SECONDS,
    }
}

fn finish_endowment_refresh_attempt(now_secs: u64, productive: bool) -> u64 {
    state::with_root_state_mut(|st| {
        let delay = if productive {
            st.endowment_refresh_ineffective_streak = 0;
            ENDOWMENT_REFRESH_MIN_INTERVAL_SECONDS
        } else {
            st.endowment_refresh_ineffective_streak =
                st.endowment_refresh_ineffective_streak.saturating_add(1);
            ineffective_backoff_seconds(st.endowment_refresh_ineffective_streak)
        };
        st.endowment_refresh_next_allowed_ts = now_secs.saturating_add(delay);
        st.endowment_refresh_next_allowed_ts
    })
}

fn bounded_failure_message(message: String) -> String {
    if message.len() <= 512 {
        return message;
    }
    let mut end = 512;
    while !message.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    message[..end].to_string()
}

fn finish_reserved_without_index(
    guard: CommitmentIndexGuard,
    response: RefreshEndowmentsResponse,
) -> RefreshEndowmentsResponse {
    drop(guard);
    state::persist_dirty_state();
    state::clear_loaded_history_caches_after_flush();
    response
}

pub(super) async fn refresh_endowments_with_client<I: IndexClient>(
    index: &I,
    now_secs: u64,
) -> RefreshEndowmentsResponse {
    match endowment_refresh_admission(now_secs) {
        EndowmentRefreshAdmission::Busy { retry_after_ts } => {
            return denied_response(RefreshEndowmentsOutcome::Busy, retry_after_ts);
        }
        EndowmentRefreshAdmission::RateLimited { retry_after_ts } => {
            return denied_response(RefreshEndowmentsOutcome::RateLimited, retry_after_ts);
        }
        EndowmentRefreshAdmission::Available => {}
    }

    let guard = {
        let _batch = state::begin_persistence_batch();
        let Some(guard) = CommitmentIndexGuard::acquire(
            now_secs,
            state::CommitmentIndexLeaseOwner::EndowmentRefresh,
        ) else {
            return denied_response(
                RefreshEndowmentsOutcome::Busy,
                now_secs.saturating_add(ENDOWMENT_REFRESH_MIN_INTERVAL_SECONDS),
            );
        };
        guard
    };

    let before_count = state::with_state(|st| st.qualifying_commitment_count.unwrap_or(0));
    let result = process_commitment_indexing_bounded(
        index,
        now_secs,
        ENDOWMENT_REFRESH_MAX_PAGES,
        Some(guard.token()),
        &|| now_secs,
    )
    .await;

    if !guard.token().is_current() {
        return finish_reserved_without_index(
            guard,
            denied_response(
                RefreshEndowmentsOutcome::Busy,
                now_secs.saturating_add(ENDOWMENT_REFRESH_MIN_INTERVAL_SECONDS),
            ),
        );
    }

    let after_count = state::with_state(|st| st.qualifying_commitment_count.unwrap_or(0));
    let newly_indexed = after_count.saturating_sub(before_count);
    let productive = newly_indexed > 0;
    let retry_after_ts = finish_endowment_refresh_attempt(now_secs, productive);
    let complete = state::with_state(state::commitment_index_is_complete);
    let outcome = match result {
        Ok(()) if !complete => RefreshEndowmentsOutcome::IncompleteProgress,
        Ok(()) if productive => RefreshEndowmentsOutcome::Updated,
        Ok(()) => RefreshEndowmentsOutcome::NoQualifyingChange,
        Err(message) => RefreshEndowmentsOutcome::UpstreamFailure {
            message: bounded_failure_message(message),
        },
    };
    let response = RefreshEndowmentsResponse {
        outcome,
        progress: progress_snapshot(newly_indexed, Some(retry_after_ts)),
    };
    finish_reserved_without_index(guard, response)
}

pub(crate) async fn refresh_endowments() -> RefreshEndowmentsResponse {
    let now_secs = ic_cdk::api::time() / 1_000_000_000;
    let index_id = state::with_state(|st| st.config.index_canister_id);
    let index = IcpIndexCanister::new(index_id);
    refresh_endowments_with_client(&index, now_secs).await
}
