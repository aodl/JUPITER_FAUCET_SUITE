use super::*;
pub(super) async fn process_route_indexing<I: IndexClient>(started_at_ts_nanos: u64, now_secs: u64, index: &I) -> Result<(), String> {
    let cfg = state::with_state(|st| st.config.clone());
    let routes = indexed_route_kinds();
    let active = state::with_root_state_mut(|st| {
        if st.active_route_sweep.is_none() {
            st.active_route_sweep = Some(ActiveRouteSweep {
                started_at_ts_nanos,
                next_index: 0,
            });
        }
        st.active_route_sweep.clone().expect("active route sweep")
    });
    if active.next_index as usize >= routes.len() {
        state::with_root_state_mut(|st| {
            st.active_route_sweep = None;
            st.last_completed_route_sweep_ts = Some(now_secs);
        });
        return Ok(());
    }

    let kind = &routes[active.next_index as usize];
    let source_id = account_identifier_text_for_account(&cfg.output_source_account);
    let route_id = { let account = indexed_route_account(&cfg, kind); account_identifier_text_for_account(&account) };
    let (mut latest_cursor, mut oldest_cursor, order_descending, mut backfill_complete) = state::with_state(|st| {
        (
            indexed_route_cursor(st, kind),
            indexed_route_oldest_cursor(st, kind),
            indexed_route_order_descending(st, kind),
            indexed_route_backfill_complete(st, kind),
        )
    });
    if latest_cursor.is_some() && oldest_cursor.is_none() {
        oldest_cursor = latest_cursor;
    }

    let mut completed_route = false;
    let mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse> = None;
    // Route indexing uses the same two-cursor model as commitment indexing in
    // descending mode: the latest cursor detects newer routed transfers, and the
    // oldest cursor continues the historical backfill through newest-first pages.
    let order = match order_descending {
        Some(true) => PageOrder::Descending,
        Some(false) => PageOrder::Ascending,
        None => {
            let page = index
                .get_account_identifier_transactions(route_id.clone(), None, PAGE_SIZE)
                .await
                .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?;
            let detected = infer_initial_page_order(&page, latest_cursor, oldest_cursor);
            first_page = Some(page);
            detected
        }
    };

    match order {
        PageOrder::Ascending => {
            let mut cursor = latest_cursor;
            for _ in 0..cfg.max_index_pages_per_tick.max(1) {
                let page = match first_page.take() {
                    Some(page) => page,
                    None => index
                        .get_account_identifier_transactions(route_id.clone(), cursor, PAGE_SIZE)
                        .await
                        .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?,
                };
                if page.transactions.is_empty() {
                    completed_route = true;
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
                                return Err(format!(
                                    "historian observed non-monotonic {}-route tx_id {} after cursor {:?}",
                                    indexed_route_name(kind),
                                    tx.id,
                                    cursor,
                                ));
                            }
                        }
                        if let Some(amount_e8s) = indexed_route_amount_from_tx(tx, &source_id, &route_id) {
                            state::with_root_state_mut(|st| add_indexed_route_amount(st, kind, amount_e8s));
                        }
                        cursor = Some(tx.id);
                        state::with_root_state_mut(|st| {
                            set_indexed_route_cursor(st, kind, cursor);
                            match kind {
                                IndexedRouteKind::Output => {
                                    st.oldest_indexed_output_tx_id = cursor;
                                    st.output_route_index_descending = Some(false);
                                    st.output_route_backfill_complete = Some(true);
                                }
                                IndexedRouteKind::Rewards => {
                                    st.oldest_indexed_rewards_tx_id = cursor;
                                    st.rewards_route_index_descending = Some(false);
                                    st.rewards_route_backfill_complete = Some(true);
                                }
                            }
                        });
                    }
                }
                if page.transactions.len() < PAGE_SIZE as usize {
                    completed_route = true;
                    break;
                }
            }
        }
        PageOrder::Descending => {
            let mut remaining_pages = cfg.max_index_pages_per_tick.max(1);

            if latest_cursor.is_some() && backfill_complete {
                let mut page_start = None;
                while remaining_pages > 0 {
                    let page = match first_page.take() {
                        Some(page) => page,
                        None => index
                            .get_account_identifier_transactions(route_id.clone(), page_start, PAGE_SIZE)
                            .await
                            .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?,
                    };
                    remaining_pages = remaining_pages.saturating_sub(1);
                    if page.transactions.is_empty() {
                        completed_route = true;
                        break;
                    }
                    let latest = latest_cursor.expect("latest cursor present in completed descending route scan");
                    let mut new_items = Vec::new();
                    let mut reached_boundary = false;
                    for tx in page.transactions.iter() {
                        if tx.id > latest {
                            new_items.push(tx.clone());
                        } else {
                            reached_boundary = true;
                            break;
                        }
                    }
                    if !new_items.is_empty() {
                        let _batch = state::begin_persistence_batch();
                        for tx in new_items.iter().rev() {
                            if let Some(amount_e8s) = indexed_route_amount_from_tx(tx, &source_id, &route_id) {
                                state::with_root_state_mut(|st| add_indexed_route_amount(st, kind, amount_e8s));
                            }
                        }
                        if let Some(max_seen) = new_items.iter().map(|tx| tx.id).max() {
                            latest_cursor = Some(latest_cursor.map(|existing| existing.max(max_seen)).unwrap_or(max_seen));
                        }
                        state::with_root_state_mut(|st| set_indexed_route_descending_progress(st, kind, latest_cursor, oldest_cursor, true));
                    }
                    if reached_boundary || page.transactions.len() < PAGE_SIZE as usize {
                        completed_route = true;
                        break;
                    }
                    page_start = page.transactions.last().map(|tx| tx.id);
                }
            } else {
                while remaining_pages > 0 && !backfill_complete {
                    let page = match first_page.take() {
                        Some(page) => page,
                        None => index
                            .get_account_identifier_transactions(route_id.clone(), oldest_cursor, PAGE_SIZE)
                            .await
                            .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?,
                    };
                    remaining_pages = remaining_pages.saturating_sub(1);
                    if page.transactions.is_empty() {
                        backfill_complete = true;
                        completed_route = true;
                        break;
                    }
                    let older_items: Vec<_> = match oldest_cursor {
                        Some(oldest) => page.transactions.iter().filter(|tx| tx.id < oldest).cloned().collect(),
                        None => page.transactions.clone(),
                    };
                    if older_items.is_empty() {
                        backfill_complete = true;
                        completed_route = true;
                        break;
                    }
                    {
                        let _batch = state::begin_persistence_batch();
                        for tx in older_items.iter().rev() {
                            if let Some(amount_e8s) = indexed_route_amount_from_tx(tx, &source_id, &route_id) {
                                state::with_root_state_mut(|st| add_indexed_route_amount(st, kind, amount_e8s));
                            }
                        }
                        if let Some(max_seen) = older_items.iter().map(|tx| tx.id).max() {
                            latest_cursor = Some(latest_cursor.map(|existing| existing.max(max_seen)).unwrap_or(max_seen));
                        }
                        if let Some(min_seen) = older_items.iter().map(|tx| tx.id).min() {
                            oldest_cursor = Some(oldest_cursor.map(|existing| existing.min(min_seen)).unwrap_or(min_seen));
                        }
                        state::with_root_state_mut(|st| set_indexed_route_descending_progress(st, kind, latest_cursor, oldest_cursor, backfill_complete));
                    }
                    if page.transactions.len() < PAGE_SIZE as usize {
                        backfill_complete = true;
                        completed_route = true;
                        break;
                    }
                }
                state::with_root_state_mut(|st| set_indexed_route_descending_progress(st, kind, latest_cursor, oldest_cursor, backfill_complete));
            }
        }
    }

    if completed_route {
        state::with_root_state_mut(|st| {
            if let Some(active) = st.active_route_sweep.as_mut() {
                active.next_index = active.next_index.saturating_add(1);
                if active.next_index as usize >= indexed_route_kinds().len() {
                    st.active_route_sweep = None;
                    st.last_completed_route_sweep_ts = Some(now_secs);
                }
            }
        });
    }
    Ok(())
}

