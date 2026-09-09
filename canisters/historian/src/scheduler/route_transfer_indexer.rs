use super::*;
pub(super) async fn process_route_indexing<I: IndexClient>(
    started_at_ts_nanos: u64,
    now_secs: u64,
    index: &I,
) -> Result<(), String> {
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
    let route_id = {
        let account = indexed_route_account(&cfg, kind);
        account_identifier_text_for_account(&account)
    };
    let (mut latest_cursor, mut oldest_cursor, order_descending, mut backfill_complete) =
        state::with_state(|st| {
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
    if backfill_complete && latest_cursor.is_none() {
        backfill_complete = false;
        state::with_root_state_mut(|st| {
            set_indexed_route_descending_progress(st, kind, None, None, false)
        });
    }

    if order_descending == Some(false) {
        return Err(format!(
            "unsupported persisted ascending {} route-index pagination state; historical coverage cannot be proven under the configured newest-first ICP Index contract",
            indexed_route_name(kind),
        ));
    }

    let mut completed_route = false;
    // Route indexing uses the same two-cursor model as commitment indexing in
    // descending mode: the latest cursor detects newer routed transfers, and the
    // oldest cursor continues the historical backfill through newest-first pages.
    let mut first_page = if order_descending.is_none() {
        Some(
            index
                .get_account_identifier_transactions(route_id.clone(), None, PAGE_SIZE)
                .await
                .map_err(|e| {
                    format!("{} route index call failed: {e}", indexed_route_name(kind))
                })?,
        )
    } else {
        None
    };

    let mut remaining_pages = cfg.max_index_pages_per_tick.max(1);

    if latest_cursor.is_some() && backfill_complete {
        let mut catch_up = state::with_state(|st| indexed_route_catch_up(st, kind));
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
                None => index
                    .get_account_identifier_transactions(
                        route_id.clone(),
                        progress.next_start_tx_id,
                        PAGE_SIZE,
                    )
                    .await
                    .map_err(|e| {
                        format!("{} route index call failed: {e}", indexed_route_name(kind))
                    })?,
            };
            remaining_pages = remaining_pages.saturating_sub(1);
            if page.transactions.is_empty() {
                let _batch = state::begin_persistence_batch();
                latest_cursor = Some(progress.observed_head_tx_id);
                state::with_root_state_mut(|st| {
                    set_indexed_route_catch_up(st, kind, None);
                    set_indexed_route_descending_progress(
                        st,
                        kind,
                        latest_cursor,
                        oldest_cursor,
                        true,
                    );
                });
                completed_route = true;
                break;
            }
            let mut new_items = Vec::new();
            let mut reached_boundary = false;
            for tx in page.transactions.iter() {
                if tx.id > progress.boundary_tx_id {
                    new_items.push(tx.clone());
                } else {
                    reached_boundary = true;
                    break;
                }
            }
            let page_is_terminal = reached_boundary || page.transactions.len() < PAGE_SIZE as usize;
            {
                let _batch = state::begin_persistence_batch();
                if !new_items.is_empty() {
                    for tx in new_items.iter().rev() {
                        if let Some(amount_e8s) =
                            indexed_route_amount_from_tx(tx, &source_id, &route_id)
                        {
                            state::with_root_state_mut(|st| {
                                add_indexed_route_amount(st, kind, amount_e8s)
                            });
                        }
                    }
                    if let Some(max_seen) = new_items.iter().map(|tx| tx.id).max() {
                        progress.observed_head_tx_id = progress.observed_head_tx_id.max(max_seen);
                    }
                }
                progress.next_start_tx_id = page.transactions.last().map(|tx| tx.id);
                if page_is_terminal {
                    latest_cursor = Some(progress.observed_head_tx_id);
                    catch_up = None;
                    completed_route = true;
                } else {
                    catch_up = Some(progress.clone());
                }
                state::with_root_state_mut(|st| {
                    set_indexed_route_catch_up(st, kind, catch_up.clone());
                    if page_is_terminal {
                        set_indexed_route_descending_progress(
                            st,
                            kind,
                            latest_cursor,
                            oldest_cursor,
                            true,
                        );
                    }
                });
            }
            if page_is_terminal {
                break;
            }
        }
    } else {
        while remaining_pages > 0 && !backfill_complete {
            let page = match first_page.take() {
                Some(page) => page,
                None => index
                    .get_account_identifier_transactions(route_id.clone(), oldest_cursor, PAGE_SIZE)
                    .await
                    .map_err(|e| {
                        format!("{} route index call failed: {e}", indexed_route_name(kind))
                    })?,
            };
            remaining_pages = remaining_pages.saturating_sub(1);
            if page.transactions.is_empty() {
                backfill_complete = true;
                completed_route = true;
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
                completed_route = true;
                break;
            }
            {
                let _batch = state::begin_persistence_batch();
                for tx in older_items.iter().rev() {
                    if let Some(amount_e8s) =
                        indexed_route_amount_from_tx(tx, &source_id, &route_id)
                    {
                        state::with_root_state_mut(|st| {
                            add_indexed_route_amount(st, kind, amount_e8s)
                        });
                    }
                }
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
                    set_indexed_route_descending_progress(
                        st,
                        kind,
                        latest_cursor,
                        oldest_cursor,
                        backfill_complete,
                    )
                });
            }
            if page.transactions.len() < PAGE_SIZE as usize {
                backfill_complete = true;
                completed_route = true;
                break;
            }
        }
        state::with_root_state_mut(|st| {
            set_indexed_route_descending_progress(
                st,
                kind,
                latest_cursor,
                oldest_cursor,
                backfill_complete,
            )
        });
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
