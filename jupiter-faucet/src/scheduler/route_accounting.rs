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
    if round_end_latest_tx_id.map(|end| tx_id > end).unwrap_or(false) {
        return None;
    }
    // A missing commitment timestamp affects only that commitment's recognition
    // status. Treat it as unrecognized rather than making the whole tranche
    // unreadable.
    let recognized = tx_timestamp_nanos
        .map(|timestamp| logic::conservative_effective_timestamp_nanos(timestamp, recognition_delay_seconds) <= round_end_time_nanos)
        .unwrap_or(false);
    Some(if recognized { commitment.amount_e8s } else { 0 })
}

fn commitment_delta_for_job_effective_denominator_e8s(
    job: &ActivePayoutJob,
    commitment: &logic::Commitment,
    tx_id: u64,
    tx_timestamp_nanos: Option<u64>,
    now_nanos: u64,
    recognition_delay_seconds: u64,
) -> Option<u64> {
    let round_end_time_nanos = job.round_end_time_nanos.unwrap_or(now_nanos);
    logic::commitment_delta_for_effective_denominator_e8s(
        commitment,
        tx_id,
        tx_timestamp_nanos,
        job.round_start_time_nanos,
        job.round_end_latest_tx_id,
        round_end_time_nanos,
        recognition_delay_seconds,
    )
}

fn commitment_round_end_staking_delta_for_job_e8s(
    job: &ActivePayoutJob,
    commitment: &logic::Commitment,
    tx_id: u64,
    tx_timestamp_nanos: Option<u64>,
    now_nanos: u64,
    recognition_delay_seconds: u64,
) -> Option<u64> {
    logic::commitment_round_end_staking_delta_e8s(
        commitment,
        tx_id,
        tx_timestamp_nanos,
        job.round_start_time_nanos,
        job.round_end_latest_tx_id,
        job.round_end_time_nanos.unwrap_or(now_nanos),
        recognition_delay_seconds,
    )
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

pub(super) async fn discover_oldest_unprocessed_funding_tranche(
    index: &impl IndexClient,
    payout_account_identifier: String,
    funding_source_account_identifier: String,
    last_processed_funding_tx_id: Option<u64>,
    fee_e8s: u64,
) -> FundingDiscovery {
    let mut scan = state::with_state_mut(|st| {
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
        st.active_funding_scan.clone().expect("funding scan state should exist")
    });
    let mut pages_scanned = 0u64;
    loop {
        if pages_scanned >= MAX_FUNDING_SCAN_PAGES_PER_TICK {
            state::with_state_mut(|st| st.active_funding_scan = Some(scan));
            return FundingDiscovery::InProgress;
        }
        pages_scanned = pages_scanned.saturating_add(1);
        let resp = index
            .get_account_identifier_transactions(payout_account_identifier.clone(), scan.cursor, PAGE_SIZE)
            .await
            .ok();
        let Some(resp) = resp else {
            state::with_state_mut(|st| st.active_funding_scan = Some(scan));
            return FundingDiscovery::Unreadable(FundingDiscoveryUnreadableReason::IndexReadFailed);
        };
        let descending = index_page_descending_from_cursor(&resp.transactions, scan.cursor);
        for tx in &resp.transactions {
            if !tx_is_after_cursor_for_page(tx.id, scan.cursor, descending) {
                continue;
            }
            if last_processed_funding_tx_id.map(|last| tx.id <= last).unwrap_or(false) {
                if descending {
                    state::with_state_mut(|st| st.active_funding_scan = None);
                    return scan
                        .candidate
                        .map(|candidate| FundingDiscovery::Found(candidate.into()))
                        .unwrap_or(FundingDiscovery::Empty);
                }
                continue;
            }
            let IndexOperation::Transfer { from, to, amount, .. } = &tx.transaction.operation else {
                continue;
            };
            if from == &funding_source_account_identifier
                && to == &payout_account_identifier
                && amount.e8s() > fee_e8s
            {
                // A funding transfer timestamp defines the tranche boundary. If a qualifying
                // funding transfer lacks a timestamp, processing later funding transfers would
                // risk violating chronological tranche order, so discovery fails closed.
                let Some(timestamp_nanos) = logic::index_tx_timestamp_nanos(tx) else {
                    state::with_state_mut(|st| st.active_funding_scan = Some(scan));
                    return FundingDiscovery::Unreadable(
                        FundingDiscoveryUnreadableReason::QualifyingFundingTransferMissingTimestamp,
                    );
                };
                let tranche = FundingTranche {
                    tx_id: tx.id,
                    timestamp_nanos,
                    amount_e8s: amount.e8s(),
                };
                if !descending {
                    state::with_state_mut(|st| st.active_funding_scan = None);
                    return FundingDiscovery::Found(tranche);
                }
                if scan
                    .candidate
                    .map(|existing| tranche.tx_id < existing.tx_id)
                    .unwrap_or(true)
                {
                    scan.candidate = Some(tranche.into());
                }
            }
        }
        if resp.transactions.len() < PAGE_SIZE as usize || resp.transactions.is_empty() {
            state::with_state_mut(|st| st.active_funding_scan = None);
            return scan
                .candidate
                .map(|candidate| FundingDiscovery::Found(candidate.into()))
                .unwrap_or(FundingDiscovery::Empty);
        }
        scan.cursor = index_page_next_cursor(&resp.transactions);
    }
}

pub(super) async fn process_payout(
    ledger: &impl LedgerClient,
    index: &impl IndexClient,
    cmc: &impl CmcClient,
    governance: &impl GovernanceClient,
    status_client: &impl CanisterStatusClient,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
    let cfg = state::with_state(|st| st.config.clone());
    let staking_id = account_identifier_text_for_account(&cfg.staking_account);

    if state::with_state(|st| st.active_payout_job.is_none()) {
        let fee_e8s = match ledger.fee_e8s().await { Ok(v) => v, Err(_) => return false };
        let payout_balance_e8s = match ledger.balance_of_e8s(payout_account()).await { Ok(v) => v, Err(_) => return false };
        let denom_e8s = match ledger.balance_of_e8s(cfg.staking_account.clone()).await { Ok(v) => v, Err(_) => return false };
        if payout_balance_e8s <= fee_e8s || denom_e8s == 0 {
            probe_index_health(index, &staking_id, denom_e8s).await;
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
        }
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let last_processed = state::with_state(|st| st.last_processed_funding_tx_id);
        let funding_tranche = match discover_oldest_unprocessed_funding_tranche(
            index,
            payout_id,
            funding_source_id,
            last_processed,
            fee_e8s,
        )
        .await {
            FundingDiscovery::Found(tranche) => tranche,
            FundingDiscovery::InProgress | FundingDiscovery::Empty => {
                probe_index_health(index, &staking_id, denom_e8s).await;
                maybe_latch_bootstrap_rescue(now_secs);
                return true;
            }
            FundingDiscovery::Unreadable(FundingDiscoveryUnreadableReason::IndexReadFailed) => return false,
            FundingDiscovery::Unreadable(FundingDiscoveryUnreadableReason::QualifyingFundingTransferMissingTimestamp) => {
                state::latch_forced_rescue_reason(ForcedRescueReason::FundingDiscoveryUnreadable);
                return true;
            }
        };
        if payout_balance_e8s < funding_tranche.amount_e8s {
            state::latch_forced_rescue_reason(ForcedRescueReason::FundingTrancheBalanceMismatch);
            return true;
        }
        let pot_start_e8s = funding_tranche.amount_e8s;
        let round_end_latest_tx_id = Some(funding_tranche.tx_id);
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

    // Skip ranges are a durable cache of barren tx-id spans discovered by earlier jobs.
    // They deliberately optimize replay work only; if future maintenance changes the rules
    // for what counts as a commitment, the cache must be cleared before relying on it.
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
            if !drive_pending_transfer(ledger, cmc, governance, cfg.cmc_canister_id, job.fee_e8s, now_nanos, now_secs).await {
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
            let descending = index_page_descending_from_cursor(&resp.transactions, job.next_start);
            let mut page_next_start = job.next_start;
            let mut denom_delta_e8s = 0u64;
            let mut round_end_staking_delta_e8s = 0u64;
            let mut reached_round_end = false;
            let min_tx_e8s = state::with_state(|st| st.config.min_tx_e8s);
            let recognition_delay_seconds = recognition_delay_seconds();

            for tx in &resp.transactions {
                if !tx_is_after_cursor_for_page(tx.id, job.next_start, descending) {
                    continue;
                }
                page_next_start = Some(tx.id);
                if job.round_end_latest_tx_id.map(|end| tx.id > end).unwrap_or(false) {
                    if descending {
                        continue;
                    }
                    reached_round_end = true;
                    break;
                }
                let Some(commitment) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                    continue;
                };
                if !matches!(logic::classify_commitment(min_tx_e8s, &commitment), logic::CommitmentValidity::Valid { .. }) {
                    continue;
                }
                // ICP Index TransactionWithId.id is the ICP ledger block index / global
                // transaction id, not a per-account sequence number. Strict tranche accounting
                // compares staking-account commitment tx ids with payout-account funding tx ids
                // only to prove a commitment existed before the funding transfer boundary.
                let tx_timestamp_nanos = logic::index_tx_timestamp_nanos(tx);
                let weighted_amount_e8s = commitment_delta_for_job_effective_denominator_e8s(
                    &job,
                    &commitment,
                    tx.id,
                    tx_timestamp_nanos,
                    now_nanos,
                    recognition_delay_seconds,
                ).unwrap_or(0);
                denom_delta_e8s = denom_delta_e8s.saturating_add(weighted_amount_e8s);
                round_end_staking_delta_e8s = round_end_staking_delta_e8s.saturating_add(
                    commitment_round_end_staking_delta_for_job_e8s(
                        &job,
                        &commitment,
                        tx.id,
                        tx_timestamp_nanos,
                        now_nanos,
                        recognition_delay_seconds,
                    ).unwrap_or(0)
                );
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
                    active_job.round_end_staking_balance_e8s = Some(
                        active_job
                            .round_end_staking_balance_e8s
                            .unwrap_or(active_job.round_start_staking_balance_e8s.unwrap_or(active_job.denom_staking_balance_e8s))
                            .saturating_add(round_end_staking_delta_e8s)
                    );
                    if scan_complete {
                        active_job.effective_denom_scan_complete = Some(true);
                        // The effective-denominator pre-scan only needs to visit current-round
                        // transactions. The payout scan that follows still needs to revisit the
                        // full beneficiary history up to the round-end boundary so pre-existing
                        // committers continue to receive payouts each round.
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
                let pending = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: remainder_gross_e8s, amount_e8s: remainder_gross_e8s.saturating_sub(job.fee_e8s), block_index: 0, next_start: None, transfer_memo: None, destination_subaccount: None, neuron_id: None };
                if !send_and_notify(ledger, cmc, governance, pending, job.fee_e8s, now_nanos, now_secs, cfg.cmc_canister_id).await {
                    return true;
                }
            }
            finalize_completed_job(status_client).await;
            maybe_latch_bootstrap_rescue(now_secs);
            return true;
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
        let descending = index_page_descending_from_cursor(&resp.transactions, job.next_start);
        let last_id = index_page_next_cursor(&resp.transactions);
        let cursor_invariant_broken = job.next_start.zip(last_id).map(|(prev, latest)| {
            if descending { latest >= prev } else { latest <= prev }
        }).unwrap_or(false);
        if cursor_invariant_broken {
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
            if !tx_is_after_cursor_for_page(tx.id, page_start, descending) {
                continue;
            }
            if job.round_end_latest_tx_id.map(|end| tx.id > end).unwrap_or(false) {
                if descending {
                    continue;
                }
                reached_round_end = true;
                break;
            }
            page_next_start = Some(tx.id);
            let Some(commitment) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
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
                    job.clone(),
                )
            });
            match logic::classify_commitment(snapshot.3, &commitment) {
                logic::CommitmentValidity::IgnoreUnderThreshold => {
                    skip_candidate.note_skippable(tx.id);
                    ignored_under_threshold_delta = ignored_under_threshold_delta.saturating_add(1);
                }
                logic::CommitmentValidity::IgnoreBadMemo => {
                    skip_candidate.note_skippable(tx.id);
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
                    ).unwrap_or(0);
                    let gross_share_e8s = logic::compute_raw_share_e8s(amount_for_round_e8s, snapshot.0, snapshot.1);
                    if gross_share_e8s <= snapshot.2 {
                        record_completed_skip_range(&mut skip_candidate, &mut pending_skip_ranges);
                        continue;
                    }
                    if snapshot.4.gross_outflow_e8s.saturating_add(gross_share_e8s) > snapshot.0 {
                        state::latch_forced_rescue_reason(ForcedRescueReason::AccountingInvariantBroken);
                        return true;
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
                    let (kind, beneficiary, transfer_memo, destination_subaccount, neuron_id) = match target {
                        logic::PayoutTarget::CyclesTopUp { canister_id } => (TransferKind::Beneficiary, canister_id, None, None, None),
                        logic::PayoutTarget::RawIcp { canister_id, memo } => (TransferKind::RawIcp, canister_id, Some(memo), None, None),
                        logic::PayoutTarget::NeuronStake { neuron_id, memo } => {
                            let subaccount = match governance.neuron_staking_subaccount(neuron_id).await {
                                Ok(subaccount) => subaccount,
                                Err(_) => {
                                    state::with_state_mut(|st| {
                                        if let Some(job) = st.active_payout_job.as_mut() {
                                            job.failed_topups = job.failed_topups.saturating_add(1);
                                        }
                                    });
                                    scan_batch = Some(state::begin_persistence_batch());
                                    continue;
                                }
                            };
                            (
                                TransferKind::NeuronStake,
                                cfg.governance_canister_id.expect("governance_canister_id configured"),
                                memo,
                                Some(subaccount),
                                Some(neuron_id),
                            )
                        }
                    };
                    let pending = PendingNotification { kind, beneficiary, gross_share_e8s, amount_e8s: gross_share_e8s.saturating_sub(snapshot.2), block_index: 0, next_start: Some(tx.id), transfer_memo, destination_subaccount, neuron_id };
                    if !send_and_notify(ledger, cmc, governance, pending, snapshot.2, now_nanos, now_secs, cfg.cmc_canister_id).await {
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
