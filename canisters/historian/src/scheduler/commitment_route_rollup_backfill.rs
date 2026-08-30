use super::*;

#[derive(Debug)]
struct ValidatedProjectionPage {
    transactions: Vec<crate::clients::index::IndexTransactionWithId>,
    next_cursor_tx_id: Option<u64>,
    complete: bool,
}

fn project_commitment_route_from_tx(
    tx: &crate::clients::index::IndexTransactionWithId,
    staking_account_id: &str,
    min_tx_e8s: u64,
) {
    let Some(logic::IndexedCommitmentEntry::Valid(commitment)) =
        logic::indexed_commitment_from_tx(tx, staking_account_id, min_tx_e8s)
    else {
        return;
    };
    if !commitment.counts_toward_faucet {
        return;
    }
    if let Some(key) = state::CommitmentRouteKey::from_indexed(&commitment.target) {
        state::increment_commitment_route_rollup(key, commitment.amount_e8s);
    }
}

fn validate_strict_page_order(
    page: &crate::clients::index::GetAccountIdentifierTransactionsResponse,
    descending: bool,
) -> Result<(), String> {
    for pair in page.transactions.windows(2) {
        let valid = if descending {
            pair[0].id > pair[1].id
        } else {
            pair[0].id < pair[1].id
        };
        if !valid {
            return Err(format!(
                "commitment-route roll-up backfill received non-{} page IDs {} and {}",
                if descending {
                    "descending"
                } else {
                    "ascending"
                },
                pair[0].id,
                pair[1].id
            ));
        }
    }
    Ok(())
}

fn validate_descending_projection_page(
    page: crate::clients::index::GetAccountIdentifierTransactionsResponse,
    active: &state::ActiveCommitmentRouteRollupBackfill,
) -> Result<ValidatedProjectionPage, String> {
    validate_strict_page_order(&page, true)?;
    if page.transactions.is_empty() {
        if active.cursor_tx_id.is_none() {
            return Err(format!(
                "commitment-route roll-up backfill could not find boundary transaction {}",
                active.boundary_tx_id
            ));
        }
        return Ok(ValidatedProjectionPage {
            transactions: Vec::new(),
            next_cursor_tx_id: active.cursor_tx_id,
            complete: true,
        });
    }
    if page
        .transactions
        .iter()
        .any(|tx| tx.id > active.boundary_tx_id)
    {
        return Err(format!(
            "commitment-route roll-up descending page crossed fixed boundary {}",
            active.boundary_tx_id
        ));
    }

    let first_id = page.transactions[0].id;
    if let Some(requested_cursor) = active.cursor_tx_id {
        if first_id > requested_cursor {
            return Err(format!(
                "commitment-route roll-up descending page moved above requested cursor {requested_cursor}; first tx was {first_id}"
            ));
        }
    } else if first_id != active.boundary_tx_id {
        return Err(format!(
            "commitment-route roll-up descending page did not begin at boundary {}; first tx was {first_id}",
            active.boundary_tx_id
        ));
    }

    let page_len = page.transactions.len();
    let repeats_cursor = active.cursor_tx_id == Some(first_id);
    let transactions: Vec<_> = page
        .transactions
        .into_iter()
        .skip(usize::from(repeats_cursor))
        .collect();
    if let Some(requested_cursor) = active.cursor_tx_id {
        if transactions.iter().any(|tx| tx.id >= requested_cursor) {
            return Err(format!(
                "commitment-route roll-up descending cursor {requested_cursor} did not progress strictly older"
            ));
        }
    }
    let complete = page_len < PAGE_SIZE as usize;
    if transactions.is_empty() && !complete {
        return Err(format!(
            "commitment-route roll-up descending cursor {} made no progress on a full page",
            active.cursor_tx_id.unwrap_or(active.boundary_tx_id)
        ));
    }
    let next_cursor_tx_id = transactions.last().map(|tx| tx.id).or(active.cursor_tx_id);
    Ok(ValidatedProjectionPage {
        transactions,
        next_cursor_tx_id,
        complete,
    })
}

fn validate_ascending_projection_page(
    page: crate::clients::index::GetAccountIdentifierTransactionsResponse,
    active: &state::ActiveCommitmentRouteRollupBackfill,
) -> Result<ValidatedProjectionPage, String> {
    validate_strict_page_order(&page, false)?;
    if page.transactions.is_empty() {
        return Err(format!(
            "commitment-route roll-up ascending history ended before boundary {}",
            active.boundary_tx_id
        ));
    }
    if let Some(cursor) = active.cursor_tx_id {
        let first_id = page.transactions[0].id;
        if first_id < cursor {
            return Err(format!(
                "commitment-route roll-up ascending page moved behind requested cursor {cursor}; first tx was {first_id}"
            ));
        }
    }

    let page_len = page.transactions.len();
    let mut saw_boundary = false;
    let mut saw_above_boundary = false;
    let transactions: Vec<_> = page
        .transactions
        .into_iter()
        .filter(|tx| {
            if tx.id == active.boundary_tx_id {
                saw_boundary = true;
            } else if tx.id > active.boundary_tx_id {
                saw_above_boundary = true;
            }
            tx.id <= active.boundary_tx_id
                && active
                    .cursor_tx_id
                    .map(|cursor| tx.id > cursor)
                    .unwrap_or(true)
        })
        .collect();
    if saw_above_boundary && !saw_boundary {
        return Err(format!(
            "commitment-route roll-up ascending page crossed fixed boundary {} without observing it",
            active.boundary_tx_id
        ));
    }
    if transactions.is_empty() && !saw_boundary {
        return Err(format!(
            "commitment-route roll-up ascending cursor {:?} made no progress toward boundary {}",
            active.cursor_tx_id, active.boundary_tx_id
        ));
    }
    if !saw_boundary && page_len < PAGE_SIZE as usize {
        return Err(format!(
            "commitment-route roll-up ascending history ended before boundary {}",
            active.boundary_tx_id
        ));
    }
    let next_cursor_tx_id = transactions.last().map(|tx| tx.id).or(active.cursor_tx_id);
    Ok(ValidatedProjectionPage {
        transactions,
        next_cursor_tx_id,
        complete: saw_boundary,
    })
}

pub(super) fn activate_commitment_route_rollup_backfill_if_eligible() -> Result<(), String> {
    let (
        marker,
        active,
        commitment_index_fault,
        boundary_tx_id,
        descending,
        staking_backfill_complete,
    ) = state::with_state(|st| {
        (
            st.commitment_route_rollups_complete_from_genesis,
            st.active_commitment_route_rollup_backfill.clone(),
            st.commitment_index_fault.clone(),
            st.last_indexed_staking_tx_id,
            st.staking_index_descending,
            st.staking_backfill_complete,
        )
    });
    if active.is_some() || marker.is_some() || commitment_index_fault.is_some() {
        return Ok(());
    }

    let Some(boundary_tx_id) = boundary_tx_id else {
        state::clear_commitment_route_rollups();
        state::with_root_state_mut(|st| {
            st.commitment_route_rollups_complete_from_genesis = Some(true);
            st.active_commitment_route_rollup_backfill = None;
        });
        log_info("historian commitment-route roll-up backfill completed for empty staking history");
        return Ok(());
    };
    let Some(descending) = descending else {
        return Err(format!(
            "commitment-route roll-up backfill cannot capture boundary {boundary_tx_id} while staking index order is unknown"
        ));
    };
    if descending && staking_backfill_complete != Some(true) {
        return Ok(());
    }

    state::clear_commitment_route_rollups();
    state::with_root_state_mut(|st| {
        st.commitment_route_rollups_complete_from_genesis = Some(false);
        st.active_commitment_route_rollup_backfill =
            Some(state::ActiveCommitmentRouteRollupBackfill {
                boundary_tx_id,
                cursor_tx_id: None,
                descending,
            });
    });
    log_info(&format!(
        "historian commitment-route roll-up backfill activated: boundary_tx_id={boundary_tx_id} order={}",
        if descending { "descending" } else { "ascending" }
    ));
    Ok(())
}

pub(super) async fn process_commitment_route_rollup_backfill_if_needed<I: IndexClient>(
    index: &I,
) -> Result<(), String> {
    activate_commitment_route_rollup_backfill_if_eligible()?;
    let (cfg, mut active) = state::with_state(|st| {
        (
            st.config.clone(),
            st.active_commitment_route_rollup_backfill.clone(),
        )
    });
    let Some(mut active_state) = active.take() else {
        return Ok(());
    };
    let staking_id = account_identifier_text_for_account(&cfg.staking_account);

    for _ in 0..cfg.max_index_pages_per_tick.max(1) {
        let start = if active_state.descending {
            active_state
                .cursor_tx_id
                .or_else(|| active_state.boundary_tx_id.checked_add(1))
        } else {
            active_state.cursor_tx_id
        };
        let page = index
            .get_account_identifier_transactions(staking_id.clone(), start, PAGE_SIZE)
            .await
            .map_err(|e| format!("commitment-route roll-up index call failed: {e}"))?;
        let validated = if active_state.descending {
            validate_descending_projection_page(page, &active_state)?
        } else {
            validate_ascending_projection_page(page, &active_state)?
        };

        {
            let _batch = state::begin_persistence_batch();
            for tx in &validated.transactions {
                project_commitment_route_from_tx(tx, &staking_id, cfg.min_tx_e8s);
            }
            state::with_root_state_mut(|st| {
                if validated.complete {
                    st.commitment_route_rollups_complete_from_genesis = Some(true);
                    st.active_commitment_route_rollup_backfill = None;
                } else if let Some(current) = st.active_commitment_route_rollup_backfill.as_mut() {
                    current.cursor_tx_id = validated.next_cursor_tx_id;
                }
            });
        }

        if validated.complete {
            log_info(&format!(
                "historian commitment-route roll-up backfill completed: boundary_tx_id={}",
                active_state.boundary_tx_id
            ));
            return Ok(());
        }
        active_state.cursor_tx_id = validated.next_cursor_tx_id;
    }
    Ok(())
}
