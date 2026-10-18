use super::*;
pub(super) const MAX_CMC_DIAGNOSTIC_WITNESSES: usize = 16;
#[cfg(test)]
pub(super) async fn probe_index_health(
    index: &impl IndexClient,
    staking_id: &str,
    denom_balance_e8s: u64,
    now_secs: u64,
) {
    probe_index_health_with_lease(
        index,
        staking_id,
        denom_balance_e8s,
        MainLeaseToken::capture_for_test(),
        now_secs,
    )
    .await;
}

pub(super) async fn probe_index_health_with_lease(
    index: &impl IndexClient,
    staking_id: &str,
    denom_balance_e8s: u64,
    lease: MainLeaseToken,
    now_secs: u64,
) {
    let prev_balance = state::with_state(|st| st.last_observed_staking_balance_e8s);
    let first_page = match index
        .get_account_identifier_transactions(staking_id.to_string(), None, 1)
        .await
    {
        Ok(resp) => resp,
        Err(_) => {
            state::with_state_mut(|st| {
                if !lease.is_current_in(st) {
                    return;
                }
                apply_latest_observation(st, denom_balance_e8s, LatestScan::Unreadable, now_secs);
            });
            return;
        }
    };

    if !lease.is_current() {
        return;
    }

    state::with_state_mut(|st| {
        if lease.is_current_in(st) {
            apply_anchor_observation(st, first_page.oldest_tx_id, now_secs);
        }
    });

    if prev_balance.is_none() {
        let latest_observation = scan_latest_observation(index, staking_id.to_string(), None).await;
        state::with_state_mut(|st| {
            if lease.is_current_in(st) {
                apply_latest_observation(st, denom_balance_e8s, latest_observation, now_secs);
            }
        });
        return;
    }

    if prev_balance == Some(denom_balance_e8s) {
        return;
    }

    let latest_observation = scan_latest_observation(index, staking_id.to_string(), None).await;
    state::with_state_mut(|st| {
        if lease.is_current_in(st) {
            apply_latest_observation(st, denom_balance_e8s, latest_observation, now_secs);
        }
    });
}

#[cfg(test)]
pub(super) async fn refresh_index_latest_health_after_payout(
    index: &impl IndexClient,
    staking_id: &str,
    denom_balance_e8s: u64,
    now_secs: u64,
) {
    refresh_index_latest_health_after_payout_with_lease(
        index,
        staking_id,
        denom_balance_e8s,
        MainLeaseToken::capture_for_test(),
        now_secs,
    )
    .await;
}

pub(super) async fn refresh_index_latest_health_after_payout_with_lease(
    index: &impl IndexClient,
    staking_id: &str,
    denom_balance_e8s: u64,
    lease: MainLeaseToken,
    now_secs: u64,
) {
    let prev_balance = state::with_state(|st| st.last_observed_staking_balance_e8s);
    if prev_balance == Some(denom_balance_e8s) {
        return;
    }

    let latest_observation = scan_latest_observation(index, staking_id.to_string(), None).await;
    state::with_state_mut(|st| {
        if lease.is_current_in(st) {
            apply_latest_observation(st, denom_balance_e8s, latest_observation, now_secs);
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LatestScan {
    Read {
        balance_e8s: u64,
        latest_tx_id: Option<u64>,
    },
    Unreadable,
    InvariantBroken,
}

thread_local! {
    static LAST_ANCHOR_FAILURE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static LAST_LATEST_FAILURE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

// A poke and its ten-second trailing callback are one operational episode.
// After upgrade a previously counted failure starts a fresh observation window;
// heap reset cannot turn two adjacent observations into a sustained outage.
fn count_sustained_failure(
    last: &std::cell::Cell<Option<u64>>,
    failures: u8,
    now_secs: u64,
) -> bool {
    if failures == 0 {
        last.set(Some(now_secs));
        return true;
    }
    match last.get() {
        Some(previous) if now_secs.saturating_sub(previous) >= 60 => {
            last.set(Some(now_secs));
            true
        }
        None => {
            last.set(Some(now_secs));
            false
        }
        _ => false,
    }
}

fn count_latest_failure(st: &state::State, now_secs: u64) -> bool {
    LAST_LATEST_FAILURE.with(|last| {
        count_sustained_failure(
            last,
            st.consecutive_index_latest_invariant_failures
                .unwrap_or(0)
                .max(st.consecutive_index_latest_unreadable_failures.unwrap_or(0)),
            now_secs,
        )
    })
}

pub(super) fn record_latest_unreadable_failure(st: &mut state::State, now_secs: u64) {
    if !count_latest_failure(st, now_secs) {
        return;
    }
    st.consecutive_index_latest_invariant_failures = Some(0);
    st.consecutive_index_latest_unreadable_failures = Some(
        st.consecutive_index_latest_unreadable_failures
            .unwrap_or(0)
            .saturating_add(1),
    );
    if st.consecutive_index_latest_unreadable_failures.unwrap_or(0) >= 2
        && st.forced_rescue_reason.is_none()
    {
        st.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestUnreadable);
    }
}

pub(super) fn record_latest_invariant_failure(st: &mut state::State, now_secs: u64) {
    if !count_latest_failure(st, now_secs) {
        return;
    }
    st.consecutive_index_latest_unreadable_failures = Some(0);
    st.consecutive_index_latest_invariant_failures = Some(
        st.consecutive_index_latest_invariant_failures
            .unwrap_or(0)
            .saturating_add(1),
    );
    if st.consecutive_index_latest_invariant_failures.unwrap_or(0) >= 2
        && st.forced_rescue_reason.is_none()
    {
        st.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestInvariantBroken);
    }
}

pub(super) fn index_page_descending(txs: &[IndexTransactionWithId]) -> bool {
    txs.first()
        .zip(txs.last())
        .map(|(first, last)| first.id > last.id)
        .unwrap_or(false)
}

/// Validate the entire newest-first, exclusive page before applying any of it.
/// Some(true) proves exhaustion; a short page alone never does. A cursor at the
/// known oldest record also proves traversal after its pending payment settles.
pub(super) fn validate_history_page(
    response: &GetAccountIdentifierTransactionsResponse,
    cursor: Option<u64>,
    expected_oldest: Option<u64>,
) -> Option<bool> {
    let oldest = response.oldest_tx_id;
    let txs = &response.transactions;
    if expected_oldest.is_some_and(|id| oldest != Some(id))
        || txs.len() > PAGE_SIZE as usize
        || txs.windows(2).any(|pair| pair[0].id <= pair[1].id)
        || txs
            .first()
            .is_some_and(|tx| cursor.is_some_and(|id| tx.id >= id))
        || txs.iter().any(|tx| oldest.is_none_or(|id| tx.id < id))
    {
        return None;
    }
    if txs.is_empty() {
        return match (cursor, oldest) {
            (None, None) => Some(true),
            (Some(cursor), Some(oldest)) if cursor == oldest => Some(true),
            _ => None,
        };
    }
    Some(txs.last().map(|tx| tx.id) == oldest)
}

pub(super) fn index_page_latest_tx_id(txs: &[IndexTransactionWithId]) -> Option<u64> {
    if index_page_descending(txs) {
        txs.first().map(|tx| tx.id)
    } else {
        txs.last().map(|tx| tx.id)
    }
}

pub(super) fn index_page_next_cursor(txs: &[IndexTransactionWithId]) -> Option<u64> {
    txs.last().map(|tx| tx.id)
}

pub(super) async fn scan_latest_observation(
    index: &impl IndexClient,
    staking_id: String,
    start: Option<u64>,
) -> LatestScan {
    let mut cursor = start;
    let mut latest = start;
    let mut latest_balance_e8s = None;
    let mut pages_scanned = 0u64;
    loop {
        if pages_scanned >= MAX_INDEX_PAGES_PER_LATEST_SCAN {
            return latest_balance_e8s
                .map(|balance_e8s| LatestScan::Read {
                    balance_e8s,
                    latest_tx_id: latest,
                })
                .unwrap_or(LatestScan::InvariantBroken);
        }
        pages_scanned = pages_scanned.saturating_add(1);
        let resp = match index
            .get_account_identifier_transactions(staking_id.clone(), cursor, PAGE_SIZE)
            .await
        {
            Ok(resp) => resp,
            Err(_) => return LatestScan::Unreadable,
        };
        latest_balance_e8s = Some(resp.balance);
        let page_latest = index_page_latest_tx_id(&resp.transactions);
        if page_latest.is_none() {
            return LatestScan::Read {
                balance_e8s: resp.balance,
                latest_tx_id: latest,
            };
        }
        let descending = index_page_descending(&resp.transactions);
        if descending {
            latest = match (latest, page_latest) {
                (Some(prev), Some(next)) => Some(prev.max(next)),
                (None, next) => next,
                (prev, None) => prev,
            };
            return LatestScan::Read {
                balance_e8s: resp.balance,
                latest_tx_id: latest,
            };
        }
        if cursor
            .zip(page_latest)
            .map(|(prev, next)| next <= prev)
            .unwrap_or(false)
        {
            return LatestScan::InvariantBroken;
        }
        latest = page_latest;
        cursor = index_page_next_cursor(&resp.transactions);
        if resp.transactions.len() < PAGE_SIZE as usize {
            return LatestScan::Read {
                balance_e8s: resp.balance,
                latest_tx_id: latest,
            };
        }
    }
}

pub(super) fn apply_anchor_observation(
    st: &mut state::State,
    observed_oldest: Option<u64>,
    now_secs: u64,
) {
    let Some(expected_first) = st.config.expected_first_staking_tx_id else {
        return;
    };
    if observed_oldest == Some(expected_first) {
        st.consecutive_index_anchor_failures = Some(0);
        LAST_ANCHOR_FAILURE.with(|last| last.set(None));
        return;
    }
    if !LAST_ANCHOR_FAILURE.with(|last| {
        count_sustained_failure(
            last,
            st.consecutive_index_anchor_failures.unwrap_or(0),
            now_secs,
        )
    }) {
        return;
    }
    st.consecutive_index_anchor_failures = Some(
        st.consecutive_index_anchor_failures
            .unwrap_or(0)
            .saturating_add(1),
    );
    if st.consecutive_index_anchor_failures.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
        st.forced_rescue_reason = Some(ForcedRescueReason::IndexAnchorMissing);
    }
}

pub(super) fn apply_latest_observation(
    st: &mut state::State,
    _denom_balance_e8s: u64,
    latest_scan: LatestScan,
    now_secs: u64,
) {
    let (balance_e8s, latest_tx_id) = match latest_scan {
        LatestScan::Unreadable => {
            record_latest_unreadable_failure(st, now_secs);
            return;
        }
        LatestScan::InvariantBroken => {
            record_latest_invariant_failure(st, now_secs);
            return;
        }
        LatestScan::Read {
            balance_e8s,
            latest_tx_id,
        } => (balance_e8s, latest_tx_id),
    };
    let previous_tip = st.last_observed_latest_tx_id;
    if st.last_observed_staking_balance_e8s.is_some() && latest_tx_id < previous_tip {
        // A lagging replica is not proof that the saved Index history is corrupt.
        return;
    }
    if st
        .last_observed_staking_balance_e8s
        .is_some_and(|balance| balance != balance_e8s)
        && latest_tx_id == previous_tip
    {
        record_latest_invariant_failure(st, now_secs);
        return;
    }
    // Only an Index observation can supply this pair. Never attach an unrelated
    // fresh Ledger balance to an old Index tip, including initial read failures.
    st.last_observed_staking_balance_e8s = Some(balance_e8s);
    st.last_observed_latest_tx_id = latest_tx_id;
    st.consecutive_index_latest_invariant_failures = Some(0);
    st.consecutive_index_latest_unreadable_failures = Some(0);
    LAST_LATEST_FAILURE.with(|last| last.set(None));
}

pub(super) fn apply_cmc_run_result(
    st: &mut state::State,
    attempts: u64,
    successes: u64,
    zero_success_run_counts: bool,
) {
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
    st.consecutive_cmc_zero_success_runs = Some(
        st.consecutive_cmc_zero_success_runs
            .unwrap_or(0)
            .saturating_add(1),
    );
    if st.consecutive_cmc_zero_success_runs.unwrap_or(0) >= 2 && st.forced_rescue_reason.is_none() {
        st.forced_rescue_reason = Some(ForcedRescueReason::CmcZeroSuccessRuns);
    }
}

pub(super) async fn zero_success_run_counts_toward_rescue(
    status_client: &impl CanisterStatusClient,
    job: &ActivePayoutJob,
) -> bool {
    if job.cmc_attempt_count.unwrap_or(0) == 0 || job.cmc_success_count.unwrap_or(0) > 0 {
        return false;
    }
    // Also bound work for a restored job written before witness retention was capped.
    for beneficiary in job
        .cmc_attempted_beneficiaries
        .iter()
        .flatten()
        .take(MAX_CMC_DIAGNOSTIC_WITNESSES)
    {
        if matches!(status_client.canister_exists(*beneficiary).await, Ok(true)) {
            return true;
        }
    }
    false
}

pub(super) fn apply_job_health_observations(
    st: &mut state::State,
    job: &ActivePayoutJob,
    zero_success_run_counts: bool,
    now_secs: u64,
) {
    apply_anchor_observation(st, job.observed_oldest_tx_id, now_secs);
    apply_cmc_run_result(
        st,
        job.cmc_attempt_count.unwrap_or(0),
        job.cmc_success_count.unwrap_or(0),
        zero_success_run_counts,
    );
}

pub(super) fn maybe_latch_bootstrap_rescue(now_secs: u64) {
    state::with_state_mut(|st| {
        if st.forced_rescue_reason.is_none()
            && policy::bootstrap_rescue_due(
                now_secs,
                st.autonomous_rescue_armed_since_ts,
                st.last_successful_transfer_ts,
            )
        {
            st.forced_rescue_reason = Some(ForcedRescueReason::BootstrapNoSuccess);
        }
    });
}
