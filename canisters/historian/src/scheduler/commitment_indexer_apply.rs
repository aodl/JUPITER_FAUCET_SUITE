use super::*;
pub(super) fn apply_verified_qualifying_commitment(
    st: &mut crate::state::State,
    commitment: crate::logic::IndexedCommitment,
    now_secs: u64,
) {
    let route_key = crate::state::CommitmentRouteKey::from_indexed(&commitment.target);
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
        if let Some(route_key) = route_key {
            crate::state::increment_commitment_route_rollup(route_key, commitment.amount_e8s);
        }
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
        crate::refresh_memo_registered_canister_summary(st, canister_id);
    }
}

pub(super) fn apply_recent_raw_or_neuron_commitment(
    st: &mut crate::state::State,
    commitment: crate::logic::IndexedCommitment,
    max_entries: usize,
) {
    let route_key = crate::state::CommitmentRouteKey::from_indexed(&commitment.target);
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
                    if let Some(route_key) = route_key {
                        crate::state::increment_commitment_route_rollup(
                            route_key,
                            commitment.amount_e8s,
                        );
                    }
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
                    if let Some(route_key) = route_key {
                        crate::state::increment_commitment_route_rollup(
                            route_key,
                            commitment.amount_e8s,
                        );
                    }
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

fn advance_commitment_index_revision() {
    state::with_root_state_mut(|st| {
        st.commitment_index_revision = st.commitment_index_revision.saturating_add(1);
    });
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
    staking_id: &str,
    mut latest_cursor: Option<u64>,
    mut oldest_cursor: Option<u64>,
    mut backfill_complete: bool,
    mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse>,
    mut lease: Option<CommitmentIndexLeaseToken>,
    lease_now_secs: &dyn Fn() -> u64,
) -> Result<(), String> {
    let mut remaining_pages = cfg.max_index_pages_per_tick.max(1);

    if latest_cursor.is_some() && oldest_cursor.is_none() {
        oldest_cursor = latest_cursor;
    }

    // An empty newest-first account is complete at that moment but has no cursor
    // from which to detect its first later transaction. Re-open the bounded
    // genesis walk so each subsequent invocation samples the newest page.
    if backfill_complete && latest_cursor.is_none() {
        backfill_complete = false;
        state::with_root_state_mut(|st| {
            st.staking_backfill_complete = Some(false);
            st.commitment_route_rollups_complete_from_genesis = Some(false);
        });
    }

    // Finish a started historical backfill before looking for new arrivals. This
    // prevents a steady stream at the head from starving the finite older range.
    if backfill_complete {
        let mut catch_up = state::with_state(|st| st.active_staking_catch_up.clone());
        if catch_up.is_none() {
            catch_up = latest_cursor.map(|latest| DescendingIndexCatchUp {
                boundary_tx_id: latest,
                observed_head_tx_id: latest,
                next_start_tx_id: None,
            });
        }
        while remaining_pages > 0 {
            let Some(mut progress) = catch_up.clone() else {
                break;
            };
            let page = match first_page.take() {
                Some(page) => page,
                None => {
                    renew_current_commitment_index_lease(&mut lease, lease_now_secs())?;
                    index
                        .get_account_identifier_transactions(
                            staking_id.to_string(),
                            progress.next_start_tx_id,
                            PAGE_SIZE,
                        )
                        .await
                        .map_err(|e| format!("index call failed: {e}"))?
                }
            };
            require_current_commitment_index_lease(lease)?;
            remaining_pages = remaining_pages.saturating_sub(1);
            if page.transactions.is_empty() {
                let _batch = state::begin_persistence_batch();
                latest_cursor = Some(progress.observed_head_tx_id);
                state::with_root_state_mut(|st| {
                    st.last_indexed_staking_tx_id = latest_cursor;
                    st.active_staking_catch_up = None;
                });
                advance_commitment_index_revision();
                break;
            }
            let mut new_items = Vec::new();
            let mut reached_boundary = false;
            for tx in page.transactions.iter() {
                if tx.id > progress.boundary_tx_id {
                    new_items.push(tx.clone());
                    continue;
                }
                reached_boundary = true;
                break;
            }
            let page_is_terminal = reached_boundary || page.transactions.len() < PAGE_SIZE as usize;
            {
                let _batch = state::begin_persistence_batch();
                if !new_items.is_empty() {
                    apply_commitment_transactions_in_chronological_order(
                        &new_items,
                        staking_id,
                        cfg.min_tx_e8s,
                        now_secs,
                    );
                    if let Some(max_new) = new_items.iter().map(|tx| tx.id).max() {
                        progress.observed_head_tx_id = progress.observed_head_tx_id.max(max_new);
                    }
                }
                progress.next_start_tx_id = page.transactions.last().map(|tx| tx.id);
                if page_is_terminal {
                    latest_cursor = Some(progress.observed_head_tx_id);
                    catch_up = None;
                } else {
                    catch_up = Some(progress.clone());
                }
                state::with_root_state_mut(|st| {
                    st.active_staking_catch_up = catch_up.clone();
                    if page_is_terminal {
                        st.last_indexed_staking_tx_id = latest_cursor;
                    }
                });
                advance_commitment_index_revision();
            }
            if page_is_terminal {
                break;
            }
        }
    }

    while remaining_pages > 0 && !backfill_complete {
        let page = match first_page.take() {
            Some(page) => page,
            None => {
                renew_current_commitment_index_lease(&mut lease, lease_now_secs())?;
                index
                    .get_account_identifier_transactions(
                        staking_id.to_string(),
                        oldest_cursor,
                        PAGE_SIZE,
                    )
                    .await
                    .map_err(|e| format!("index call failed: {e}"))?
            }
        };
        require_current_commitment_index_lease(lease)?;
        remaining_pages = remaining_pages.saturating_sub(1);
        if page.transactions.is_empty() {
            backfill_complete = true;
            advance_commitment_index_revision();
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
            advance_commitment_index_revision();
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
        if backfill_complete && st.commitment_route_rollups_complete_from_genesis == Some(false) {
            st.commitment_route_rollups_complete_from_genesis = Some(true);
        }
        st.last_index_run_ts = Some(now_secs);
        // A previously latched fault is not cleared merely because a bounded
        // newest-first pass returned successfully. Operators may clear it
        // explicitly after establishing that the gap/fault has been resolved.
    });
    Ok(())
}
