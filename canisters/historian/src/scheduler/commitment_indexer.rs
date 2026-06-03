use super::*;
pub(super) async fn process_commitment_indexing<I: IndexClient>(
    index: &I,
    now_secs: u64,
) -> Result<(), String> {
    let cfg = state::with_state(|st| st.config.clone());
    let (had_fault, latest_cursor, oldest_cursor, order_descending, backfill_complete) =
        state::with_state(|st| {
            (
                st.commitment_index_fault.is_some(),
                st.last_indexed_staking_tx_id,
                st.oldest_indexed_staking_tx_id,
                st.staking_index_descending,
                st.staking_backfill_complete.unwrap_or(false),
            )
        });
    let staking_id = account_identifier_text_for_account(&cfg.staking_account);

    match order_descending {
        Some(false) => {
            process_commitment_indexing_ascending(
                index,
                now_secs,
                &cfg,
                had_fault,
                &staking_id,
                latest_cursor,
                None,
            )
            .await
        }
        Some(true) => {
            process_commitment_indexing_descending_seeded(
                index,
                now_secs,
                &cfg,
                had_fault,
                &staking_id,
                latest_cursor,
                oldest_cursor,
                backfill_complete,
                None,
            )
            .await
        }
        None => {
            let first_page = index
                .get_account_identifier_transactions(staking_id.clone(), None, PAGE_SIZE)
                .await
                .map_err(|e| format!("index call failed: {e}"))?;
            match infer_initial_page_order(&first_page, latest_cursor, oldest_cursor) {
                PageOrder::Ascending => {
                    state::with_root_state_mut(|st| st.staking_index_descending = Some(false));
                    process_commitment_indexing_ascending(
                        index,
                        now_secs,
                        &cfg,
                        had_fault,
                        &staking_id,
                        latest_cursor,
                        Some(first_page),
                    )
                    .await
                }
                PageOrder::Descending => {
                    state::with_root_state_mut(|st| st.staking_index_descending = Some(true));
                    process_commitment_indexing_descending_seeded(
                        index,
                        now_secs,
                        &cfg,
                        had_fault,
                        &staking_id,
                        latest_cursor,
                        oldest_cursor,
                        backfill_complete,
                        Some(first_page),
                    )
                    .await
                }
            }
        }
    }
}
