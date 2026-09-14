use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FundingDiscovery {
    Found(FundingTranche),
    InProgress,
    Empty,
    Unreadable(FundingDiscoveryUnreadableReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FundingDiscoveryUnreadableReason {
    IndexReadFailed,
    QualifyingFundingTransferMissingTimestamp,
}

fn genesis_round_amount_for_commitment_e8s(
    commitment: &logic::Commitment,
    tx_id: u64,
    tx_timestamp_nanos: Option<u64>,
    round_end_latest_tx_id: Option<u64>,
    round_end_time_nanos: u64,
    recognition_delay_seconds: u64,
) -> Option<u64> {
    if round_end_latest_tx_id
        .map(|end| tx_id > end)
        .unwrap_or(false)
    {
        return None;
    }
    // A missing commitment timestamp affects only that commitment's recognition
    // status. Treat it as unrecognized rather than making the whole tranche
    // unreadable.
    let recognized = tx_timestamp_nanos
        .map(|timestamp| {
            logic::conservative_effective_timestamp_nanos(timestamp, recognition_delay_seconds)
                <= round_end_time_nanos
        })
        .unwrap_or(false);
    Some(if recognized { commitment.amount_e8s } else { 0 })
}

fn commitment_amount_for_job_payout_e8s(
    job: &ActivePayoutJob,
    commitment: &logic::Commitment,
    tx_id: u64,
    tx_timestamp_nanos: Option<u64>,
    now_nanos: u64,
    recognition_delay_seconds: u64,
) -> Option<u64> {
    let round_end_time_nanos = job.round_end_time_nanos.unwrap_or(now_nanos);
    if job.round_start_time_nanos.is_none()
        && job.round_start_latest_tx_id.is_none()
        && job.round_start_staking_balance_e8s == Some(0)
    {
        return genesis_round_amount_for_commitment_e8s(
            commitment,
            tx_id,
            tx_timestamp_nanos,
            job.round_end_latest_tx_id,
            round_end_time_nanos,
            recognition_delay_seconds,
        );
    }
    logic::commitment_amount_for_payout_e8s(
        commitment,
        tx_id,
        tx_timestamp_nanos,
        job.round_start_time_nanos,
        job.round_end_latest_tx_id,
        round_end_time_nanos,
        recognition_delay_seconds,
    )
}

fn assert_no_persistence_batch_for_async() {
    debug_assert!(
        !state::persistence_batch_active(),
        "persistence batch must be dropped before async client calls"
    );
}

async fn neuron_staking_subaccount_retry_once(
    governance: &impl GovernanceClient,
    neuron_id: u64,
) -> Result<[u8; 32], crate::clients::ClientError> {
    assert_no_persistence_batch_for_async();
    match governance.neuron_staking_subaccount(neuron_id).await {
        Ok(subaccount) => Ok(subaccount),
        Err(_) => {
            assert_no_persistence_batch_for_async();
            governance.neuron_staking_subaccount(neuron_id).await
        }
    }
}

#[cfg(test)]
pub(super) async fn discover_oldest_unprocessed_funding_tranche(
    index: &impl IndexClient,
    payout_account_identifier: String,
    funding_source_account_identifier: String,
    last_processed_funding_tx_id: Option<u64>,
    fee_e8s: u64,
) -> FundingDiscovery {
    discover_oldest_unprocessed_funding_tranche_with_lease(
        index,
        payout_account_identifier,
        funding_source_account_identifier,
        last_processed_funding_tx_id,
        fee_e8s,
        MainLeaseToken::capture_for_test(),
    )
    .await
    .expect("test funding discovery lease was superseded")
}

pub(super) async fn discover_oldest_unprocessed_funding_tranche_with_lease(
    index: &impl IndexClient,
    payout_account_identifier: String,
    funding_source_account_identifier: String,
    last_processed_funding_tx_id: Option<u64>,
    fee_e8s: u64,
    lease: MainLeaseToken,
) -> Option<FundingDiscovery> {
    if !lease.is_current() {
        return None;
    }
    let mut scan = state::with_state_mut(|st| {
        if !lease.is_current_in(st)
            || st.last_processed_funding_tx_id != last_processed_funding_tx_id
            || st.active_payout_job.is_some()
        {
            return None;
        }
        let reset = st
            .active_funding_scan
            .as_ref()
            .map(|scan| scan.anchor_last_processed_funding_tx_id != last_processed_funding_tx_id)
            .unwrap_or(true);
        if reset {
            st.active_funding_scan = Some(state::FundingScanState {
                anchor_last_processed_funding_tx_id: last_processed_funding_tx_id,
                cursor: None,
                candidate: None,
            });
        }
        Some(
            st.active_funding_scan
                .clone()
                .expect("funding scan state should exist"),
        )
    })?;
    let owns_scan = |st: &state::State, expected: &state::FundingScanState| {
        lease.is_current_in(st)
            && st.last_processed_funding_tx_id == last_processed_funding_tx_id
            && st.active_payout_job.is_none()
            && st.active_funding_scan.as_ref() == Some(expected)
    };
    let mut observed_oldest = None;
    for _ in 0..MAX_FUNDING_SCAN_PAGES_PER_TICK {
        if !state::with_state(|st| owns_scan(st, &scan)) {
            return None;
        }
        assert_no_persistence_batch_for_async();
        let response = index
            .get_account_identifier_transactions(
                payout_account_identifier.clone(),
                scan.cursor,
                PAGE_SIZE,
            )
            .await;
        if !state::with_state(|st| owns_scan(st, &scan)) {
            return None;
        }
        let Ok(resp) = response else {
            return Some(FundingDiscovery::Unreadable(
                FundingDiscoveryUnreadableReason::IndexReadFailed,
            ));
        };
        if scan.candidate.is_some() && resp.oldest_tx_id.is_none() {
            return Some(FundingDiscovery::Unreadable(
                FundingDiscoveryUnreadableReason::IndexReadFailed,
            ));
        }
        // This anchor belongs to the payout account, never to staking history.
        let Some(exhausted) = validate_history_page(&resp, scan.cursor, observed_oldest) else {
            return Some(FundingDiscovery::Unreadable(
                FundingDiscoveryUnreadableReason::IndexReadFailed,
            ));
        };
        observed_oldest = resp.oldest_tx_id;
        let mut next_scan = scan.clone();
        let mut reached_processed = false;
        for tx in &resp.transactions {
            if last_processed_funding_tx_id.is_some_and(|last| tx.id <= last) {
                reached_processed = true;
                break;
            }
            let IndexOperation::Transfer {
                from, to, amount, ..
            } = &tx.transaction.operation
            else {
                continue;
            };
            if from == &funding_source_account_identifier
                && to == &payout_account_identifier
                && amount.e8s() > fee_e8s
            {
                // A qualifying tranche without a time boundary blocks selection;
                // choosing a later one would violate chronological processing.
                let Some(timestamp_nanos) = logic::index_tx_timestamp_nanos(tx) else {
                    return Some(FundingDiscovery::Unreadable(
                        FundingDiscoveryUnreadableReason::QualifyingFundingTransferMissingTimestamp,
                    ));
                };
                let tranche = FundingTranche {
                    tx_id: tx.id,
                    timestamp_nanos,
                    amount_e8s: amount.e8s(),
                };
                if next_scan
                    .candidate
                    .is_none_or(|old| tranche.tx_id < old.tx_id)
                {
                    next_scan.candidate = Some(tranche.into());
                }
            }
        }
        let complete = exhausted || reached_processed;
        next_scan.cursor = resp.transactions.last().map(|tx| tx.id).or(scan.cursor);
        let applied = state::with_state_mut(|st| {
            if !owns_scan(st, &scan) {
                return false;
            }
            st.active_funding_scan = if complete {
                None
            } else {
                Some(next_scan.clone())
            };
            true
        });
        if !applied {
            return None;
        }
        if complete {
            return Some(
                next_scan
                    .candidate
                    .map(|candidate| FundingDiscovery::Found(candidate.into()))
                    .unwrap_or(FundingDiscovery::Empty),
            );
        }
        scan = next_scan;
    }
    Some(FundingDiscovery::InProgress)
}

#[cfg(test)]
pub(super) async fn process_payout(
    ledger: &impl LedgerClient,
    index: &impl IndexClient,
    cmc: &impl CmcClient,
    governance: &impl GovernanceClient,
    status_client: &impl CanisterStatusClient,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
    process_payout_with_lease(
        ledger,
        index,
        cmc,
        governance,
        status_client,
        now_nanos,
        now_secs,
        MainLeaseToken::capture_for_test(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn process_payout_with_lease(
    ledger: &impl LedgerClient,
    index: &impl IndexClient,
    cmc: &impl CmcClient,
    governance: &impl GovernanceClient,
    status_client: &impl CanisterStatusClient,
    now_nanos: u64,
    now_secs: u64,
    lease: MainLeaseToken,
) -> bool {
    if !lease.is_current() {
        return true;
    }
    let cfg = state::with_state(|st| st.config.clone());
    let staking_id = account_identifier_text_for_account(&cfg.staking_account);

    if state::with_state(|st| st.active_payout_job.is_none()) {
        assert_no_persistence_batch_for_async();
        let fee_e8s = match ledger.fee_e8s().await {
            Ok(v) => v,
            Err(_) => return false,
        };
        if !lease.is_current() {
            return true;
        }
        assert_no_persistence_batch_for_async();
        let payout_balance_e8s = match ledger.balance_of_e8s(payout_account()).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        if !lease.is_current() {
            return true;
        }
        assert_no_persistence_batch_for_async();
        let denom_e8s = match ledger.balance_of_e8s(cfg.staking_account).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        if !lease.is_current() {
            return true;
        }
        if payout_balance_e8s <= fee_e8s || denom_e8s == 0 {
            assert_no_persistence_batch_for_async();
            probe_index_health_with_lease(index, &staking_id, denom_e8s, lease).await;
            if lease.is_current() {
                maybe_latch_bootstrap_rescue(now_secs);
            }
            return true;
        }
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let last_processed = state::with_state(|st| st.last_processed_funding_tx_id);
        assert_no_persistence_batch_for_async();
        let Some(funding_discovery) = discover_oldest_unprocessed_funding_tranche_with_lease(
            index,
            payout_id,
            funding_source_id,
            last_processed,
            fee_e8s,
            lease,
        )
        .await
        else {
            return true;
        };
        log_funding_discovery(funding_discovery, last_processed);
        let funding_tranche = match funding_discovery {
            FundingDiscovery::Found(tranche) => tranche,
            FundingDiscovery::InProgress | FundingDiscovery::Empty => {
                assert_no_persistence_batch_for_async();
                probe_index_health_with_lease(index, &staking_id, denom_e8s, lease).await;
                if lease.is_current() {
                    maybe_latch_bootstrap_rescue(now_secs);
                }
                return true;
            }
            FundingDiscovery::Unreadable(FundingDiscoveryUnreadableReason::IndexReadFailed) => {
                return false
            }
            FundingDiscovery::Unreadable(
                FundingDiscoveryUnreadableReason::QualifyingFundingTransferMissingTimestamp,
            ) => {
                state::latch_forced_rescue_reason(ForcedRescueReason::FundingDiscoveryUnreadable);
                return true;
            }
        };
        if payout_balance_e8s < funding_tranche.amount_e8s {
            log_funding_balance_mismatch(funding_tranche, payout_balance_e8s, last_processed);
            state::latch_forced_rescue_reason(ForcedRescueReason::FundingTrancheBalanceMismatch);
            return true;
        }
        let pot_start_e8s = funding_tranche.amount_e8s;
        let round_end_latest_tx_id = Some(funding_tranche.tx_id);
        if !lease.is_current()
            || state::with_state(|st| st.last_processed_funding_tx_id != last_processed)
        {
            return true;
        }
        ensure_active_job_with_boundary(
            now_nanos,
            fee_e8s,
            pot_start_e8s,
            denom_e8s,
            funding_tranche.timestamp_nanos,
            round_end_latest_tx_id,
            Some(funding_tranche),
        );
    }

    // Each page or cached jump consumes one bounded driver step.
    let mut pages_scanned = 0u64;

    loop {
        if !lease.is_current() {
            return true;
        }
        let job = state::with_state(|st| st.active_payout_job.clone());
        let Some(job) = job else {
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        };
        if job.pending_transfer.is_some() {
            assert_no_persistence_batch_for_async();
            if !drive_pending_transfer(
                ledger,
                cmc,
                governance,
                cfg.cmc_canister_id,
                job.fee_e8s,
                now_nanos,
                now_secs,
            )
            .await
            {
                return true;
            }
            continue;
        }

        if !job.scan_complete {
            if pages_scanned >= MAX_INDEX_PAGES_PER_PAYOUT_TICK {
                return true;
            }
            match skip_cached_history(&job, lease) {
                Ok(true) => {
                    pages_scanned += 1;
                    continue;
                }
                Ok(false) => {}
                Err(_) => {
                    latch_skip_range_invariant_rescue();
                    return true;
                }
            }
        }

        if !effective_denom_scan_complete(&job) {
            assert_no_persistence_batch_for_async();
            let resp = match index
                .get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE)
                .await
            {
                Ok(v) => v,
                Err(_) => return false,
            };
            if !state::with_state(|st| {
                lease.is_current_in(st)
                    && st.active_payout_job.as_ref().is_some_and(|active| {
                        active.id == job.id
                            && active.next_start == job.next_start
                            && active.effective_denom_scan_complete == Some(false)
                    })
            }) {
                return true;
            }
            pages_scanned = pages_scanned.saturating_add(1);
            // The ICP Index contract is descending with an exclusive cursor. A short
            // response is not proof of exhaustion: servers may cap max_results.
            let expected_oldest = state::with_state(|st| st.config.expected_first_staking_tx_id);
            let oldest = resp.oldest_tx_id;
            if expected_oldest.is_some_and(|id| oldest != Some(id)) {
                state::with_state_mut(|st| {
                    if lease.is_current_in(st)
                        && st.active_payout_job.as_ref().is_some_and(|active| {
                            active.id == job.id
                                && active.next_start == job.next_start
                                && active.effective_denom_scan_complete == Some(false)
                        })
                    {
                        apply_anchor_observation(st, oldest);
                    }
                });
                // Preserve the existing rescue reconciliation while holding all funds.
                return true;
            }
            let Some(scan_complete) = validate_history_page(
                &resp,
                job.next_start,
                job.observed_oldest_tx_id.or(expected_oldest),
            ) else {
                return false;
            };
            let page_next_start = resp.transactions.last().map(|tx| tx.id);
            let mut denom_delta_e8s = 0u64;
            let mut round_end_staking_delta_e8s = 0u64;
            let min_tx_e8s = state::with_state(|st| st.config.min_tx_e8s);
            let recognition_delay_seconds = recognition_delay_seconds();

            let mut skip_candidate = LocalSkipCandidate::from_job(&job);
            let mut pending_skip_ranges = Vec::new();
            for tx in &resp.transactions {
                if job.round_end_latest_tx_id.is_some_and(|end| tx.id > end) {
                    record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
                    continue;
                }
                match state::skip_range_containing(tx.id) {
                    Ok(Some(_)) => {
                        record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
                        continue;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        latch_skip_range_invariant_rescue();
                        return true;
                    }
                }
                let Some(commitment) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                    skip_candidate.note_skippable(tx.id);
                    continue;
                };
                if !matches!(
                    logic::classify_commitment(min_tx_e8s, &commitment),
                    logic::CommitmentValidity::Valid { .. }
                ) {
                    skip_candidate.note_skippable(tx.id);
                    continue;
                }
                // Eligibility, not time weight or delivery, terminates a barren span.
                record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
                let tx_timestamp_nanos = logic::index_tx_timestamp_nanos(tx);
                let weighted_amount_e8s = commitment_amount_for_job_payout_e8s(
                    &job,
                    &commitment,
                    tx.id,
                    tx_timestamp_nanos,
                    now_nanos,
                    recognition_delay_seconds,
                )
                .unwrap_or(0);
                let Some(next_denom) = denom_delta_e8s.checked_add(weighted_amount_e8s) else {
                    return false;
                };
                denom_delta_e8s = next_denom;
                let recognized = genesis_round_amount_for_commitment_e8s(
                    &commitment,
                    tx.id,
                    tx_timestamp_nanos,
                    job.round_end_latest_tx_id,
                    job.round_end_time_nanos.unwrap_or(now_nanos),
                    recognition_delay_seconds,
                )
                .unwrap_or(0);
                let Some(next_ending) = round_end_staking_delta_e8s.checked_add(recognized) else {
                    return false;
                };
                round_end_staking_delta_e8s = next_ending;
            }
            let Some(denominator) = job
                .effective_denom_staking_balance_e8s
                .unwrap_or(0)
                .checked_add(denom_delta_e8s)
            else {
                return false;
            };
            let Some(ending_balance) = job
                .round_end_staking_balance_e8s
                .unwrap_or(0)
                .checked_add(round_end_staking_delta_e8s)
            else {
                return false;
            };
            if scan_complete {
                record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
            }
            if persist_new_skip_ranges(&mut pending_skip_ranges).is_err() {
                latch_skip_range_invariant_rescue();
                return true;
            }
            state::with_state_mut(|st| {
                if !lease.is_current_in(st) {
                    return;
                }
                if let Some(active_job) = st.active_payout_job.as_mut().filter(|active| {
                    active.id == job.id
                        && active.next_start == job.next_start
                        && active.effective_denom_scan_complete == Some(false)
                }) {
                    active_job.effective_denom_staking_balance_e8s = Some(denominator);
                    active_job.round_end_staking_balance_e8s = Some(ending_balance);
                    active_job.skip_candidate_start_tx_id = skip_candidate.start_tx_id;
                    active_job.skip_candidate_end_tx_id = skip_candidate.end_tx_id;
                    active_job.skip_candidate_tx_count = skip_candidate.tx_count;
                    active_job.observed_oldest_tx_id = oldest;
                    if let Some(tx) = resp.transactions.first() {
                        active_job.observed_latest_tx_id =
                            Some(active_job.observed_latest_tx_id.unwrap_or(tx.id).max(tx.id));
                    }
                    if scan_complete {
                        active_job.effective_denom_scan_complete = Some(true);
                        // Both authoritative sums are complete before the payout scan can
                        // send funds. Replay the same pinned history for beneficiary weights.
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
            let remainder_gross_e8s = job
                .pot_start_e8s
                .checked_sub(job.gross_outflow_e8s)
                .expect("validated payout accounting");
            if remainder_gross_e8s > job.fee_e8s && job.remainder_to_relay_e8s == 0 {
                let pending = PendingNotification {
                    kind: TransferKind::RemainderToRelay,
                    beneficiary: crate::canonical_relay_canister_id(),
                    gross_share_e8s: remainder_gross_e8s,
                    amount_e8s: remainder_gross_e8s
                        .checked_sub(job.fee_e8s)
                        .expect("remainder exceeds ledger fee"),
                    block_index: 0,
                    next_start: None,
                    transfer_memo: Some(Vec::new()),
                    destination_subaccount: None,
                    neuron_id: None,
                };
                assert_no_persistence_batch_for_async();
                if !send_and_notify(
                    ledger,
                    cmc,
                    governance,
                    pending,
                    job.fee_e8s,
                    now_nanos,
                    now_secs,
                    cfg.cmc_canister_id,
                )
                .await
                {
                    return true;
                }
            }
            assert_no_persistence_batch_for_async();
            if !finalize_completed_job(status_client, lease, job.id).await {
                return true;
            }
            assert_no_persistence_batch_for_async();
            if let Ok(post_payout_balance_e8s) = ledger.balance_of_e8s(cfg.staking_account).await {
                if !lease.is_current() {
                    return true;
                }
                assert_no_persistence_batch_for_async();
                refresh_index_latest_health_after_payout_with_lease(
                    index,
                    &staking_id,
                    post_payout_balance_e8s,
                    lease,
                )
                .await;
            }
            if lease.is_current() {
                maybe_latch_bootstrap_rescue(now_secs);
            }
            return true;
        }

        // Reuse negative evidence learned by the denominator pass.
        assert_no_persistence_batch_for_async();
        let resp = match index
            .get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE)
            .await
        {
            Ok(v) => v,
            Err(_) => return false,
        };
        if !state::with_state(|st| {
            lease.is_current_in(st)
                && st.active_payout_job.as_ref().is_some_and(|active| {
                    active.id == job.id
                        && active.next_start == job.next_start
                        && !active.scan_complete
                        && effective_denom_scan_complete(active)
                })
        }) {
            return true;
        }
        pages_scanned = pages_scanned.saturating_add(1);
        let expected_oldest = state::with_state(|st| st.config.expected_first_staking_tx_id);
        let Some(scan_complete) = validate_history_page(
            &resp,
            job.next_start,
            job.observed_oldest_tx_id.or(expected_oldest),
        ) else {
            state::with_state_mut(|st| {
                if lease.is_current_in(st)
                    && st.active_payout_job.as_ref().is_some_and(|active| {
                        active.id == job.id
                            && active.next_start == job.next_start
                            && !active.scan_complete
                    })
                {
                    record_latest_invariant_failure(st);
                }
            });
            return false;
        };
        let last_id = resp.transactions.last().map(|tx| tx.id).or(job.next_start);
        {
            let _batch = state::begin_persistence_batch();
            note_index_page_with_lease(&resp, lease, job.id, job.next_start);
        }

        let page_start = job.next_start;
        let mut ignored_under_threshold_delta = 0u64;
        let mut ignored_bad_memo_delta = 0u64;
        let mut page_next_start = page_start;
        let skip_candidate = LocalSkipCandidate::default();
        let mut last_persisted_cursor = job.next_start;
        for tx in &resp.transactions {
            // Advance over newer records without mistaking them for an end
            // boundary: older eligible commitments are on subsequent pages.
            page_next_start = Some(tx.id);
            if job.round_end_latest_tx_id.is_some_and(|end| tx.id > end) {
                continue;
            }
            match state::skip_range_containing(tx.id) {
                Ok(Some(_)) => continue,
                Ok(None) => {}
                Err(_) => {
                    latch_skip_range_invariant_rescue();
                    return true;
                }
            }
            let Some(commitment) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                continue;
            };
            let snapshot = state::with_state(|st| {
                let job = st
                    .active_payout_job
                    .as_ref()
                    .expect("active payout job missing");
                (
                    job.pot_start_e8s,
                    effective_denom_e8s(job),
                    job.fee_e8s,
                    st.config.min_tx_e8s,
                    job.clone(),
                )
            });
            match logic::classify_commitment(snapshot.3, &commitment) {
                logic::CommitmentValidity::IgnoreUnderThreshold => {
                    ignored_under_threshold_delta = ignored_under_threshold_delta.saturating_add(1);
                }
                logic::CommitmentValidity::IgnoreBadMemo => {
                    ignored_bad_memo_delta = ignored_bad_memo_delta.saturating_add(1);
                }
                logic::CommitmentValidity::Valid { target } => {
                    let amount_for_round_e8s = commitment_amount_for_job_payout_e8s(
                        &snapshot.4,
                        &commitment,
                        tx.id,
                        logic::index_tx_timestamp_nanos(tx),
                        now_nanos,
                        recognition_delay_seconds(),
                    )
                    .unwrap_or(0);
                    let gross_share_e8s =
                        logic::compute_raw_share_e8s(amount_for_round_e8s, snapshot.0, snapshot.1);
                    if gross_share_e8s <= snapshot.2 {
                        continue;
                    }
                    if snapshot.4.gross_outflow_e8s.saturating_add(gross_share_e8s) > snapshot.0 {
                        state::latch_forced_rescue_reason(
                            ForcedRescueReason::AccountingInvariantBroken,
                        );
                        return true;
                    }
                    let (kind, beneficiary, transfer_memo, destination_subaccount, neuron_id) =
                        match target {
                            logic::PayoutTarget::CyclesTopUp { canister_id } => {
                                (TransferKind::Beneficiary, canister_id, None, None, None)
                            }
                            logic::PayoutTarget::RawIcp { canister_id, memo } => {
                                (TransferKind::RawIcp, canister_id, Some(memo), None, None)
                            }
                            logic::PayoutTarget::NeuronStake { neuron_id, memo } => {
                                let subaccount_result =
                                    neuron_staking_subaccount_retry_once(governance, neuron_id)
                                        .await;
                                if !state::with_state(|st| {
                                    lease.is_current_in(st)
                                        && st.active_payout_job.as_ref().is_some_and(|active| {
                                            active.id == job.id
                                                && active.next_start == last_persisted_cursor
                                                && !active.scan_complete
                                        })
                                }) {
                                    return true;
                                }
                                let subaccount = match subaccount_result {
                                    Ok(subaccount) => subaccount,
                                    Err(_) => {
                                        state::with_state_mut(|st| {
                                            if lease.is_current_in(st) {
                                                if let Some(active) = st
                                                    .active_payout_job
                                                    .as_mut()
                                                    .filter(|active| active.id == job.id)
                                                {
                                                    active.failed_topups =
                                                        active.failed_topups.saturating_add(1);
                                                }
                                            }
                                        });
                                        continue;
                                    }
                                };
                                if !state::with_state(|st| {
                                    lease.is_current_in(st)
                                        && st
                                            .active_payout_job
                                            .as_ref()
                                            .is_some_and(|active| active.id == job.id)
                                }) {
                                    return true;
                                }
                                (
                                    TransferKind::NeuronStake,
                                    cfg.governance_canister_id
                                        .expect("governance_canister_id configured"),
                                    memo,
                                    Some(subaccount),
                                    Some(neuron_id),
                                )
                            }
                        };
                    {
                        let _batch = state::begin_persistence_batch();
                        flush_scan_progress(
                            &mut ignored_under_threshold_delta,
                            &mut ignored_bad_memo_delta,
                            page_next_start,
                            &skip_candidate,
                            lease,
                            job.id,
                            last_persisted_cursor,
                        );
                    }
                    if page_next_start.is_some() {
                        last_persisted_cursor = page_next_start;
                    }
                    if !state::with_state(|st| {
                        lease.is_current_in(st)
                            && st.active_payout_job.as_ref().is_some_and(|active| {
                                active.id == job.id && active.next_start == page_next_start
                            })
                    }) {
                        return true;
                    }
                    let pending = PendingNotification {
                        kind,
                        beneficiary,
                        gross_share_e8s,
                        amount_e8s: gross_share_e8s.saturating_sub(snapshot.2),
                        block_index: 0,
                        next_start: Some(tx.id),
                        transfer_memo,
                        destination_subaccount,
                        neuron_id,
                    };
                    assert_no_persistence_batch_for_async();
                    if !send_and_notify(
                        ledger,
                        cmc,
                        governance,
                        pending,
                        snapshot.2,
                        now_nanos,
                        now_secs,
                        cfg.cmc_canister_id,
                    )
                    .await
                    {
                        return true;
                    }
                    if !state::with_state(|st| {
                        lease.is_current_in(st)
                            && st.active_payout_job.as_ref().is_some_and(|active| {
                                active.id == job.id
                                    && active.next_start == last_persisted_cursor
                                    && !active.scan_complete
                            })
                    }) {
                        return true;
                    }
                }
            }
        }
        {
            let _batch = state::begin_persistence_batch();
            state::with_state_mut(|st| {
                if !lease.is_current_in(st) {
                    return;
                }
                if let Some(active) = st.active_payout_job.as_mut().filter(|active| {
                    active.id == job.id && active.next_start == last_persisted_cursor
                }) {
                    active.ignored_under_threshold = active
                        .ignored_under_threshold
                        .saturating_add(ignored_under_threshold_delta);
                    active.ignored_bad_memo = active
                        .ignored_bad_memo
                        .saturating_add(ignored_bad_memo_delta);
                    if let Some(next_start) = page_next_start {
                        active.next_start = Some(next_start);
                    }
                    active.skip_candidate_start_tx_id = skip_candidate.start_tx_id;
                    active.skip_candidate_end_tx_id = skip_candidate.end_tx_id;
                    active.skip_candidate_tx_count = skip_candidate.tx_count;
                    if scan_complete {
                        active.scan_complete = true;
                    } else {
                        active.next_start = last_id;
                    }
                }
            });
        }
        if !state::with_state(|st| {
            lease.is_current_in(st)
                && st
                    .active_payout_job
                    .as_ref()
                    .is_some_and(|active| active.id == job.id)
        }) {
            return true;
        }
        if !scan_complete && pages_scanned >= MAX_INDEX_PAGES_PER_PAYOUT_TICK {
            return true;
        }
    }
}
