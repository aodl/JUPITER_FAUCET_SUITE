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


fn index_page_descending(txs: &[IndexTransactionWithId]) -> bool {
    txs.first().zip(txs.last()).map(|(first, last)| first.id > last.id).unwrap_or(false)
}

fn index_page_descending_from_cursor(txs: &[IndexTransactionWithId], cursor: Option<u64>) -> bool {
    if txs.len() >= 2 {
        return index_page_descending(txs);
    }
    match (cursor, txs.first()) {
        (Some(last_seen), Some(tx)) => tx.id < last_seen,
        _ => false,
    }
}

fn index_page_latest_tx_id(txs: &[IndexTransactionWithId]) -> Option<u64> {
    if index_page_descending(txs) {
        txs.first().map(|tx| tx.id)
    } else {
        txs.last().map(|tx| tx.id)
    }
}

fn index_page_next_cursor(txs: &[IndexTransactionWithId]) -> Option<u64> {
    txs.last().map(|tx| tx.id)
}

fn tx_is_after_cursor_for_page(tx_id: u64, cursor: Option<u64>, descending: bool) -> bool {
    match cursor {
        None => true,
        Some(last_seen) if descending => tx_id < last_seen,
        Some(last_seen) => tx_id > last_seen,
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
        let page_latest = index_page_latest_tx_id(&resp.transactions);
        if page_latest.is_none() {
            return LatestScan::Read(latest);
        }
        let descending = index_page_descending(&resp.transactions);
        if descending {
            latest = match (latest, page_latest) {
                (Some(prev), Some(next)) => Some(prev.max(next)),
                (None, next) => next,
                (prev, None) => prev,
            };
            return LatestScan::Read(latest);
        }
        if cursor.zip(page_latest).map(|(prev, next)| next <= prev).unwrap_or(false) {
            return LatestScan::InvariantBroken;
        }
        latest = page_latest;
        cursor = index_page_next_cursor(&resp.transactions);
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

