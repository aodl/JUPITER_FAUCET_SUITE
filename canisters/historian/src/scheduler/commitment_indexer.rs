use super::*;
#[cfg(test)]
pub(super) async fn process_commitment_indexing<I: IndexClient>(
    index: &I,
    now_secs: u64,
) -> Result<(), String> {
    let max_pages = state::with_state(|st| st.config.max_index_pages_per_tick);
    process_commitment_indexing_bounded(index, now_secs, max_pages, None, &|| now_secs).await
}

pub(super) async fn process_commitment_indexing_bounded<I: IndexClient>(
    index: &I,
    now_secs: u64,
    max_pages: u32,
    mut lease: Option<CommitmentIndexLeaseToken>,
    lease_now_secs: &dyn Fn() -> u64,
) -> Result<(), String> {
    let mut cfg = state::with_state(|st| st.config.clone());
    cfg.max_index_pages_per_tick = max_pages.max(1);
    let (latest_cursor, oldest_cursor, order_descending, backfill_complete) =
        state::with_state(|st| {
            (
                st.last_indexed_staking_tx_id,
                st.oldest_indexed_staking_tx_id,
                st.staking_index_descending,
                st.staking_backfill_complete.unwrap_or(false),
            )
        });
    let staking_id = account_identifier_text_for_account(&cfg.staking_account);

    if order_descending == Some(false) {
        let message = "unsupported persisted ascending staking-index pagination state; historical coverage cannot be proven under the configured newest-first ICP Index contract".to_string();
        state::with_root_state_mut(|st| {
            st.commitment_route_rollups_complete_from_genesis = Some(false);
        });
        return Err(latch_commitment_index_fault(
            now_secs,
            latest_cursor,
            latest_cursor.unwrap_or(0),
            message,
        ));
    }

    let first_page = if order_descending.is_none() {
        renew_current_commitment_index_lease(&mut lease, lease_now_secs())?;
        let first_page = index
            .get_account_identifier_transactions(staking_id.clone(), None, PAGE_SIZE)
            .await
            .map_err(|e| format!("index call failed: {e}"))?;
        require_current_commitment_index_lease(lease)?;
        state::with_root_state_mut(|st| st.staking_index_descending = Some(true));
        Some(first_page)
    } else {
        None
    };

    process_commitment_indexing_descending_seeded(
        index,
        now_secs,
        &cfg,
        &staking_id,
        latest_cursor,
        oldest_cursor,
        backfill_complete,
        first_page,
        lease,
        lease_now_secs,
    )
    .await
}
