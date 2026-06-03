use super::*;
pub(super) fn indexed_route_kinds() -> [IndexedRouteKind; 2] {
    [IndexedRouteKind::Output, IndexedRouteKind::Rewards]
}

pub(super) fn indexed_route_name(kind: &IndexedRouteKind) -> &'static str {
    match kind {
        IndexedRouteKind::Output => "output",
        IndexedRouteKind::Rewards => "rewards",
    }
}

pub(super) fn indexed_route_account(
    cfg: &state::Config,
    kind: &IndexedRouteKind,
) -> icrc_ledger_types::icrc1::account::Account {
    match kind {
        IndexedRouteKind::Output => cfg.output_account,
        IndexedRouteKind::Rewards => cfg.rewards_account,
    }
}

pub(super) fn indexed_route_cursor(st: &state::State, kind: &IndexedRouteKind) -> Option<u64> {
    match kind {
        IndexedRouteKind::Output => st.last_indexed_output_tx_id,
        IndexedRouteKind::Rewards => st.last_indexed_rewards_tx_id,
    }
}

pub(super) fn set_indexed_route_cursor(
    st: &mut state::State,
    kind: &IndexedRouteKind,
    cursor: Option<u64>,
) {
    match kind {
        IndexedRouteKind::Output => st.last_indexed_output_tx_id = cursor,
        IndexedRouteKind::Rewards => st.last_indexed_rewards_tx_id = cursor,
    }
}

pub(super) fn indexed_route_oldest_cursor(
    st: &state::State,
    kind: &IndexedRouteKind,
) -> Option<u64> {
    match kind {
        IndexedRouteKind::Output => st.oldest_indexed_output_tx_id,
        IndexedRouteKind::Rewards => st.oldest_indexed_rewards_tx_id,
    }
}

pub(super) fn indexed_route_order_descending(
    st: &state::State,
    kind: &IndexedRouteKind,
) -> Option<bool> {
    match kind {
        IndexedRouteKind::Output => st.output_route_index_descending,
        IndexedRouteKind::Rewards => st.rewards_route_index_descending,
    }
}

pub(super) fn indexed_route_backfill_complete(st: &state::State, kind: &IndexedRouteKind) -> bool {
    match kind {
        IndexedRouteKind::Output => st.output_route_backfill_complete.unwrap_or(false),
        IndexedRouteKind::Rewards => st.rewards_route_backfill_complete.unwrap_or(false),
    }
}

pub(super) fn set_indexed_route_descending_progress(
    st: &mut state::State,
    kind: &IndexedRouteKind,
    latest: Option<u64>,
    oldest: Option<u64>,
    backfill_complete: bool,
) {
    match kind {
        IndexedRouteKind::Output => {
            st.last_indexed_output_tx_id = latest;
            st.oldest_indexed_output_tx_id = oldest;
            st.output_route_index_descending = Some(true);
            st.output_route_backfill_complete = Some(backfill_complete);
        }
        IndexedRouteKind::Rewards => {
            st.last_indexed_rewards_tx_id = latest;
            st.oldest_indexed_rewards_tx_id = oldest;
            st.rewards_route_index_descending = Some(true);
            st.rewards_route_backfill_complete = Some(backfill_complete);
        }
    }
}

pub(super) fn add_indexed_route_amount(
    st: &mut state::State,
    kind: &IndexedRouteKind,
    amount_e8s: u64,
) {
    match kind {
        IndexedRouteKind::Output => {
            let total = st.total_output_e8s.get_or_insert(0);
            *total = total.saturating_add(amount_e8s);
        }
        IndexedRouteKind::Rewards => {
            let total = st.total_rewards_e8s.get_or_insert(0);
            *total = total.saturating_add(amount_e8s);
        }
    }
}

pub(super) fn indexed_route_amount_from_tx(
    tx: &crate::clients::index::IndexTransactionWithId,
    expected_from: &str,
    expected_to: &str,
) -> Option<u64> {
    match &tx.transaction.operation {
        crate::clients::index::IndexOperation::Transfer {
            from, to, amount, ..
        } if from == expected_from && to == expected_to => Some(amount.e8s()),
        crate::clients::index::IndexOperation::TransferFrom {
            from, to, amount, ..
        } if from == expected_from && to == expected_to => Some(amount.e8s()),
        _ => None,
    }
}
