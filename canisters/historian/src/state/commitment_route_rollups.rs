use super::*;

pub(crate) fn get_commitment_route_rollup(key: &CommitmentRouteKey) -> CommitmentRouteRollup {
    with_commitment_route_rollup_map(|map| map.get(key)).unwrap_or_default()
}

pub(crate) fn increment_commitment_route_rollup(key: CommitmentRouteKey, amount_e8s: u64) {
    with_commitment_route_rollup_map(|map| {
        let mut rollup = map.get(&key).unwrap_or_default();
        rollup.qualifying_commitment_count = rollup.qualifying_commitment_count.saturating_add(1);
        rollup.total_qualifying_committed_e8s = rollup
            .total_qualifying_committed_e8s
            .saturating_add(amount_e8s);
        map.insert(key, rollup);
    });
}

pub(crate) fn clear_commitment_route_rollups() {
    with_commitment_route_rollup_map(|map| map.clear_new());
}

#[cfg(test)]
pub(crate) fn commitment_route_rollup_entry_count() -> u64 {
    with_commitment_route_rollup_map(|map| map.len())
}
