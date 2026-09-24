use super::*;
use crate::{EndowmentTransactionStatusResponse, ExpectedEndowmentStatus};
use std::cell::Cell;

pub(super) const ENDOWMENT_POKE_MIN_INTERVAL_SECONDS: u64 = 10;
pub(super) const ENDOWMENT_POKE_MAX_PAGES: u32 = 1;
pub(super) const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Copy, Default)]
struct PokeDeferredState {
    due_at_ns: Option<u64>,
    timer_pending: bool,
}

thread_local! {
    static POKE_DEFERRED: Cell<PokeDeferredState> = const { Cell::new(PokeDeferredState {
        due_at_ns: None, timer_pending: false,
    }) };
}

pub(super) fn record_poke_hint(now_ns: u64) -> bool {
    POKE_DEFERRED.with(|cell| {
        let mut deferred = cell.get();
        deferred.due_at_ns =
            Some(now_ns.saturating_add(
                ENDOWMENT_POKE_MIN_INTERVAL_SECONDS.saturating_mul(NANOS_PER_SECOND),
            ));
        let should_schedule = !deferred.timer_pending;
        deferred.timer_pending = true;
        cell.set(deferred);
        should_schedule
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferredTimerAction {
    Stop,
    Reschedule(Duration),
    Check,
}

pub(super) fn deferred_timer_action(now_ns: u64) -> DeferredTimerAction {
    POKE_DEFERRED.with(|cell| {
        let mut deferred = cell.get();
        let action = match deferred.due_at_ns {
            Some(due_at_ns) if due_at_ns > now_ns => {
                DeferredTimerAction::Reschedule(Duration::from_nanos(due_at_ns - now_ns))
            }
            Some(_) => {
                // Clear before the Index await so a concurrent poke can install a
                // fresh trailing-edge timer while this deferred check is in flight.
                deferred.due_at_ns = None;
                deferred.timer_pending = false;
                DeferredTimerAction::Check
            }
            None => {
                deferred.timer_pending = false;
                DeferredTimerAction::Stop
            }
        };
        cell.set(deferred);
        action
    })
}

#[cfg(test)]
pub(super) fn poke_deferred_state_for_test() -> (Option<u64>, bool) {
    POKE_DEFERRED.with(|cell| {
        let deferred = cell.get();
        (deferred.due_at_ns, deferred.timer_pending)
    })
}

#[cfg(test)]
pub(super) fn reset_poke_deferred_for_test() {
    POKE_DEFERRED.with(|cell| cell.set(PokeDeferredState::default()));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PokeAdmission {
    Available,
    Busy,
    RateLimited,
}

pub(crate) fn poke_admission(now_secs: u64) -> PokeAdmission {
    state::with_state(|st| {
        if st.endowment_refresh_next_allowed_ts > now_secs {
            return PokeAdmission::RateLimited;
        }
        if st.commitment_index_lock_expires_at_ts.unwrap_or(0) > now_secs {
            return PokeAdmission::Busy;
        }
        PokeAdmission::Available
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PokeAttempt {
    Skipped,
    Indexed,
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

pub(super) async fn handle_endowment_poke_with_client<I: IndexClient>(
    index: &I,
    now_secs: u64,
) -> PokeAttempt {
    if poke_admission(now_secs) != PokeAdmission::Available {
        return PokeAttempt::Skipped;
    }
    let guard = {
        let _batch = state::begin_persistence_batch();
        let Some(guard) = CommitmentIndexGuard::acquire(
            now_secs,
            state::CommitmentIndexLeaseOwner::EndowmentRefresh,
        ) else {
            return PokeAttempt::Skipped;
        };
        guard
    };
    let result = process_commitment_indexing_bounded(
        index,
        now_secs,
        ENDOWMENT_POKE_MAX_PAGES,
        Some(guard.token()),
        &|| now_secs,
    )
    .await;
    if !guard.token().is_current() {
        drop(guard);
        state::persist_dirty_state();
        state::clear_loaded_history_caches_after_flush();
        return PokeAttempt::Skipped;
    }
    if let Err(message) = &result {
        ic_cdk::println!("Event Horizon poke Index check failed: {message}");
    }
    state::with_root_state_mut(|st| {
        st.endowment_refresh_next_allowed_ts =
            now_secs.saturating_add(ENDOWMENT_POKE_MIN_INTERVAL_SECONDS);
    });
    drop(guard);
    state::persist_dirty_state();
    state::clear_loaded_history_caches_after_flush();
    PokeAttempt::Indexed
}

fn schedule_poke_deferred_check(delay: Duration) {
    ic_cdk_timers::set_timer(delay, async {
        match deferred_timer_action(ic_cdk::api::time()) {
            DeferredTimerAction::Stop => {}
            DeferredTimerAction::Reschedule(remaining) => {
                schedule_poke_deferred_check(remaining);
            }
            DeferredTimerAction::Check => {
                let now_secs = ic_cdk::api::time() / 1_000_000_000;
                let index_id = state::with_state(|st| st.config.index_canister_id);
                let index = IcpIndexCanister::new(index_id);
                handle_endowment_poke_with_client(&index, now_secs).await;
            }
        }
    });
}

pub(crate) async fn handle_endowment_poke() {
    // Establish the trailing check before an Index await or a busy lease can
    // coalesce this call. New hints move the one pending deadline forward.
    let now_ns = ic_cdk::api::time();
    if record_poke_hint(now_ns) {
        schedule_poke_deferred_check(Duration::from_secs(ENDOWMENT_POKE_MIN_INTERVAL_SECONDS));
    }
    let now_secs = now_ns / NANOS_PER_SECOND;
    let index_id = state::with_state(|st| st.config.index_canister_id);
    let index = IcpIndexCanister::new(index_id);
    handle_endowment_poke_with_client(&index, now_secs).await;
}
