use super::*;
pub(super) fn apply_verified_qualifying_commitment(
    st: &mut crate::state::State,
    commitment: crate::logic::IndexedCommitment,
    now_secs: u64,
) {
    let crate::logic::IndexedCommitmentTarget::CyclesTopUp { canister_id } = commitment.target
    else {
        return;
    };
    st.distinct_canisters.insert(canister_id);
    st.canister_tracking_reasons.insert(
        canister_id,
        logic::merge_tracking_reasons(
            st.canister_tracking_reasons.get(&canister_id),
            CanisterTrackingReason::MemoCommitment,
        ),
    );
    let recent_item = RecentCommitment {
        canister_id,
        raw_icp_memo_text: None,
        tx_id: commitment.tx_id,
        timestamp_nanos: commitment.timestamp_nanos,
        amount_e8s: commitment.amount_e8s,
        counts_toward_faucet: true,
    };
    crate::state::ensure_commitment_history_loaded(st, canister_id);
    let history = st.commitment_history.entry(canister_id).or_default();
    let inserted = logic::push_commitment(
        history,
        crate::state::CommitmentSample {
            tx_id: commitment.tx_id,
            timestamp_nanos: commitment.timestamp_nanos,
            amount_e8s: commitment.amount_e8s,
            counts_toward_faucet: true,
        },
        st.config.max_commitment_entries_per_canister,
    );
    if inserted {
        let meta = st.per_canister_meta.entry(canister_id).or_default();
        let needs_initial_cycles_probe = meta.last_cycles_probe_ts.is_none();
        logic::apply_commitment_seen(meta, commitment.timestamp_nanos, now_secs);
        let recent = st.recent_commitments.get_or_insert_with(Vec::new);
        push_recent_commitment(recent, recent_item, MAX_RECENT_QUALIFYING_COMMITMENTS);
        let count = st.qualifying_commitment_count.get_or_insert(0);
        *count = count.saturating_add(1);

        // Newly registered memo beneficiaries should get an early targeted
        // cycles probe without resetting or starving the normal full sweep.
        if needs_initial_cycles_probe {
            enqueue_initial_cycles_probe(st, canister_id);
        }
    }
    if inserted || st.canister_tracking_reasons.contains_key(&canister_id) {
        crate::refresh_registered_canister_summary(st, canister_id);
    }
}

pub(super) fn apply_recent_raw_or_neuron_commitment(
    st: &mut crate::state::State,
    commitment: crate::logic::IndexedCommitment,
    max_entries: usize,
) {
    match commitment.target {
        crate::logic::IndexedCommitmentTarget::RawIcp {
            canister_id,
            memo_text,
        } => {
            if commitment.counts_toward_faucet {
                crate::state::ensure_raw_icp_commitment_history_loaded(st, canister_id);
                let history = st
                    .raw_icp_commitment_history
                    .entry(canister_id)
                    .or_default();
                let inserted = logic::push_commitment(
                    history,
                    crate::state::CommitmentSample {
                        tx_id: commitment.tx_id,
                        timestamp_nanos: commitment.timestamp_nanos,
                        amount_e8s: commitment.amount_e8s,
                        counts_toward_faucet: true,
                    },
                    st.config.max_commitment_entries_per_canister,
                );
                if inserted {
                    let recent = st.recent_commitments.get_or_insert_with(Vec::new);
                    push_recent_commitment(
                        recent,
                        RecentCommitment {
                            canister_id,
                            raw_icp_memo_text: Some(memo_text),
                            tx_id: commitment.tx_id,
                            timestamp_nanos: commitment.timestamp_nanos,
                            amount_e8s: commitment.amount_e8s,
                            counts_toward_faucet: true,
                        },
                        max_entries,
                    );
                    let count = st.qualifying_commitment_count.get_or_insert(0);
                    *count = count.saturating_add(1);
                }
            } else {
                let recent = st
                    .recent_under_threshold_commitments
                    .get_or_insert_with(Vec::new);
                push_recent_commitment(
                    recent,
                    RecentCommitment {
                        canister_id,
                        raw_icp_memo_text: Some(memo_text),
                        tx_id: commitment.tx_id,
                        timestamp_nanos: commitment.timestamp_nanos,
                        amount_e8s: commitment.amount_e8s,
                        counts_toward_faucet: false,
                    },
                    max_entries,
                );
            }
        }
        crate::logic::IndexedCommitmentTarget::NeuronStake {
            neuron_id,
            memo_text,
        } => {
            if commitment.counts_toward_faucet {
                crate::state::ensure_neuron_commitment_history_loaded(st, neuron_id);
                let history = st.neuron_commitment_history.entry(neuron_id).or_default();
                let inserted = logic::push_commitment(
                    history,
                    crate::state::CommitmentSample {
                        tx_id: commitment.tx_id,
                        timestamp_nanos: commitment.timestamp_nanos,
                        amount_e8s: commitment.amount_e8s,
                        counts_toward_faucet: true,
                    },
                    st.config.max_commitment_entries_per_canister,
                );
                if inserted {
                    let recent = st.recent_neuron_commitments.get_or_insert_with(Vec::new);
                    push_recent_neuron_commitment(
                        recent,
                        RecentNeuronCommitment {
                            neuron_id,
                            memo_text,
                            tx_id: commitment.tx_id,
                            timestamp_nanos: commitment.timestamp_nanos,
                            amount_e8s: commitment.amount_e8s,
                            counts_toward_faucet: true,
                        },
                        max_entries,
                    );
                    let count = st.qualifying_commitment_count.get_or_insert(0);
                    *count = count.saturating_add(1);
                }
            } else {
                let recent = st
                    .recent_under_threshold_neuron_commitments
                    .get_or_insert_with(Vec::new);
                push_recent_neuron_commitment(
                    recent,
                    RecentNeuronCommitment {
                        neuron_id,
                        memo_text,
                        tx_id: commitment.tx_id,
                        timestamp_nanos: commitment.timestamp_nanos,
                        amount_e8s: commitment.amount_e8s,
                        counts_toward_faucet: false,
                    },
                    max_entries,
                );
            }
        }
        crate::logic::IndexedCommitmentTarget::CyclesTopUp { .. } => {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PageOrder {
    Ascending,
    Descending,
}

pub(super) fn detect_page_order(
    page: &crate::clients::index::GetAccountIdentifierTransactionsResponse,
) -> Option<PageOrder> {
    if page.transactions.len() < 2 {
        return None;
    }
    let first = page.transactions.first().expect("page has a first tx").id;
    let last = page.transactions.last().expect("page has a last tx").id;
    if first < last {
        Some(PageOrder::Ascending)
    } else if first > last {
        Some(PageOrder::Descending)
    } else {
        None
    }
}

pub(super) fn infer_initial_page_order(
    page: &crate::clients::index::GetAccountIdentifierTransactionsResponse,
    latest_cursor: Option<u64>,
    oldest_cursor: Option<u64>,
) -> PageOrder {
    if let Some(order) = detect_page_order(page) {
        return order;
    }
    // A single-item page is ambiguous unless a persisted cursor proves that the
    // item is older than already-indexed history. In that case the real ICP
    // index's newest-first pagination is the only ordering compatible with the
    // page; otherwise keep the legacy ascending path.
    let first_id = page.transactions.first().map(|tx| tx.id);
    if first_id
        .zip(oldest_cursor.or(latest_cursor))
        .map(|(tx_id, cursor)| tx_id < cursor)
        .unwrap_or(false)
    {
        PageOrder::Descending
    } else {
        PageOrder::Ascending
    }
}

pub(super) fn apply_indexed_commitment_tx(
    tx: &crate::clients::index::IndexTransactionWithId,
    staking_id: &str,
    min_tx_e8s: u64,
    now_secs: u64,
) {
    if let Some(commitment) = logic::indexed_commitment_from_tx(tx, staking_id, min_tx_e8s) {
        match commitment {
            logic::IndexedCommitmentEntry::Valid(commitment) => match commitment.target {
                crate::logic::IndexedCommitmentTarget::CyclesTopUp { canister_id }
                    if commitment.counts_toward_faucet =>
                {
                    state::with_root_registry_and_commitments_canister_state_mut(
                        canister_id,
                        |st| {
                            apply_verified_qualifying_commitment(st, commitment, now_secs);
                        },
                    );
                }
                crate::logic::IndexedCommitmentTarget::CyclesTopUp { canister_id } => {
                    state::with_root_state_mut(|st| {
                        let recent = st
                            .recent_under_threshold_commitments
                            .get_or_insert_with(Vec::new);
                        push_recent_commitment(
                            recent,
                            RecentCommitment {
                                canister_id,
                                raw_icp_memo_text: None,
                                tx_id: commitment.tx_id,
                                timestamp_nanos: commitment.timestamp_nanos,
                                amount_e8s: commitment.amount_e8s,
                                counts_toward_faucet: false,
                            },
                            MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS,
                        );
                    });
                }
                crate::logic::IndexedCommitmentTarget::RawIcp { canister_id, .. }
                    if commitment.counts_toward_faucet =>
                {
                    state::with_root_and_raw_icp_commitments_state_mut(canister_id, |st| {
                        apply_recent_raw_or_neuron_commitment(
                            st,
                            commitment,
                            MAX_RECENT_QUALIFYING_COMMITMENTS,
                        );
                    });
                }
                crate::logic::IndexedCommitmentTarget::NeuronStake { neuron_id, .. }
                    if commitment.counts_toward_faucet =>
                {
                    state::with_root_and_neuron_commitments_state_mut(neuron_id, |st| {
                        apply_recent_raw_or_neuron_commitment(
                            st,
                            commitment,
                            MAX_RECENT_QUALIFYING_COMMITMENTS,
                        );
                    });
                }
                crate::logic::IndexedCommitmentTarget::RawIcp { .. }
                | crate::logic::IndexedCommitmentTarget::NeuronStake { .. } => {
                    state::with_root_state_mut(|st| {
                        apply_recent_raw_or_neuron_commitment(
                            st,
                            commitment,
                            MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS,
                        );
                    });
                }
            },
            logic::IndexedCommitmentEntry::Invalid(commitment) => {
                state::with_root_state_mut(|st| {
                    let recent = st.recent_invalid_commitments.get_or_insert_with(Vec::new);
                    push_recent_invalid_commitment(
                        recent,
                        InvalidCommitment {
                            tx_id: commitment.tx_id,
                            timestamp_nanos: commitment.timestamp_nanos,
                            amount_e8s: commitment.amount_e8s,
                            memo_text: commitment.memo_text,
                        },
                    );
                });
            }
        }
    }
}

pub(super) fn apply_commitment_transactions_in_chronological_order(
    txs: &[crate::clients::index::IndexTransactionWithId],
    staking_id: &str,
    min_tx_e8s: u64,
    now_secs: u64,
) {
    for tx in txs.iter().rev() {
        apply_indexed_commitment_tx(tx, staking_id, min_tx_e8s, now_secs);
    }
}

pub(super) async fn process_commitment_indexing_ascending<I: IndexClient>(
    index: &I,
    now_secs: u64,
    cfg: &state::Config,
    had_fault: bool,
    staking_id: &str,
    mut cursor: Option<u64>,
    mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse>,
) -> Result<(), String> {
    for _ in 0..cfg.max_index_pages_per_tick.max(1) {
        let page = match first_page.take() {
            Some(page) => page,
            None => index
                .get_account_identifier_transactions(staking_id.to_string(), cursor, PAGE_SIZE)
                .await
                .map_err(|e| format!("index call failed: {e}"))?,
        };
        if page.transactions.is_empty() {
            break;
        }
        {
            let _batch = state::begin_persistence_batch();
            for tx in page.transactions.iter() {
                if let Some(prev) = cursor {
                    if tx.id == prev {
                        continue;
                    }
                    if tx.id < prev {
                        return Err(latch_commitment_index_fault(
                            now_secs,
                            cursor,
                            tx.id,
                            format!(
                                "historian observed non-monotonic staking-account tx_id {} after cursor {:?}",
                                tx.id, cursor,
                            ),
                        ));
                    }
                }
                apply_indexed_commitment_tx(tx, staking_id, cfg.min_tx_e8s, now_secs);
                cursor = Some(tx.id);
                state::with_root_state_mut(|st| {
                    st.last_indexed_staking_tx_id = cursor;
                    st.oldest_indexed_staking_tx_id = cursor;
                    st.staking_index_descending = Some(false);
                    st.staking_backfill_complete = Some(true);
                });
            }
        }
        if page.transactions.len() < PAGE_SIZE as usize {
            break;
        }
    }
    state::with_root_state_mut(|st| {
        st.last_index_run_ts = Some(now_secs);
        if had_fault {
            st.commitment_index_fault = None;
        }
    });
    Ok(())
}

// The real ICP index returns account history newest-first and uses the `start`
// cursor to walk toward older transactions. Descending mode therefore keeps two
// cursors: `latest_cursor` is the highest/newest tx id observed so future ticks
// can pick up new arrivals from the latest page, while `oldest_cursor` is the
// oldest tx id backfilled so older history can continue without treating normal
// lower tx ids as non-monotonic.
// Kept wide because tests seed every cursor/page boundary for the descending ICP index walk.
#[allow(clippy::too_many_arguments)]
pub(super) async fn process_commitment_indexing_descending_seeded<I: IndexClient>(
    index: &I,
    now_secs: u64,
    cfg: &state::Config,
    had_fault: bool,
    staking_id: &str,
    mut latest_cursor: Option<u64>,
    mut oldest_cursor: Option<u64>,
    mut backfill_complete: bool,
    mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse>,
) -> Result<(), String> {
    let mut remaining_pages = cfg.max_index_pages_per_tick.max(1);

    if latest_cursor.is_some() && oldest_cursor.is_none() {
        oldest_cursor = latest_cursor;
    }

    if let Some(latest) = latest_cursor {
        let mut page_start = None;
        while remaining_pages > 0 {
            let page = match first_page.take() {
                Some(page) => page,
                None => index
                    .get_account_identifier_transactions(
                        staking_id.to_string(),
                        page_start,
                        PAGE_SIZE,
                    )
                    .await
                    .map_err(|e| format!("index call failed: {e}"))?,
            };
            remaining_pages = remaining_pages.saturating_sub(1);
            if page.transactions.is_empty() {
                break;
            }
            let mut new_items = Vec::new();
            let mut reached_boundary = false;
            for tx in page.transactions.iter() {
                if tx.id == latest {
                    reached_boundary = true;
                    break;
                }
                if tx.id > latest {
                    new_items.push(tx.clone());
                    continue;
                }
                reached_boundary = true;
                break;
            }
            if !new_items.is_empty() {
                let _batch = state::begin_persistence_batch();
                apply_commitment_transactions_in_chronological_order(
                    &new_items,
                    staking_id,
                    cfg.min_tx_e8s,
                    now_secs,
                );
                if let Some(max_new) = new_items.iter().map(|tx| tx.id).max() {
                    latest_cursor = Some(
                        latest_cursor
                            .map(|existing| existing.max(max_new))
                            .unwrap_or(max_new),
                    );
                    state::with_root_state_mut(|st| st.last_indexed_staking_tx_id = latest_cursor);
                }
            }
            if reached_boundary || page.transactions.len() < PAGE_SIZE as usize {
                break;
            }
            page_start = page.transactions.last().map(|tx| tx.id);
        }
    }

    while remaining_pages > 0 && !backfill_complete {
        let page = match first_page.take() {
            Some(page) => page,
            None => index
                .get_account_identifier_transactions(
                    staking_id.to_string(),
                    oldest_cursor,
                    PAGE_SIZE,
                )
                .await
                .map_err(|e| format!("index call failed: {e}"))?,
        };
        remaining_pages = remaining_pages.saturating_sub(1);
        if page.transactions.is_empty() {
            backfill_complete = true;
            break;
        }
        let older_items: Vec<_> = match oldest_cursor {
            Some(oldest) => page
                .transactions
                .iter()
                .filter(|tx| tx.id < oldest)
                .cloned()
                .collect(),
            None => page.transactions.clone(),
        };
        if older_items.is_empty() {
            backfill_complete = true;
            break;
        }
        {
            let _batch = state::begin_persistence_batch();
            apply_commitment_transactions_in_chronological_order(
                &older_items,
                staking_id,
                cfg.min_tx_e8s,
                now_secs,
            );
            if let Some(max_seen) = older_items.iter().map(|tx| tx.id).max() {
                latest_cursor = Some(
                    latest_cursor
                        .map(|existing| existing.max(max_seen))
                        .unwrap_or(max_seen),
                );
            }
            if let Some(min_seen) = older_items.iter().map(|tx| tx.id).min() {
                oldest_cursor = Some(
                    oldest_cursor
                        .map(|existing| existing.min(min_seen))
                        .unwrap_or(min_seen),
                );
            }
            state::with_root_state_mut(|st| {
                st.last_indexed_staking_tx_id = latest_cursor;
                st.oldest_indexed_staking_tx_id = oldest_cursor;
                st.staking_index_descending = Some(true);
                st.staking_backfill_complete = Some(backfill_complete);
            });
        }
        if page.transactions.len() < PAGE_SIZE as usize {
            backfill_complete = true;
            break;
        }
    }

    state::with_root_state_mut(|st| {
        st.last_indexed_staking_tx_id = latest_cursor;
        st.oldest_indexed_staking_tx_id = oldest_cursor;
        st.staking_index_descending = Some(true);
        st.staking_backfill_complete = Some(backfill_complete);
        st.last_index_run_ts = Some(now_secs);
        if had_fault {
            st.commitment_index_fault = None;
        }
    });
    Ok(())
}
