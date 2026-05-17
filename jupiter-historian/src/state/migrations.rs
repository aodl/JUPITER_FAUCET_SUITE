use super::*;
pub(crate) fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub(super) fn restore_state_current(root: StableRootState) -> State {
    let canister_sources = with_canister_sources_map(|map| {
        let mut out = BTreeMap::new();
        for (key, value) in map.iter() {
            out.insert(key.to_principal(), value.0.clone());
        }
        out
    });
    let commitment_history = BTreeMap::new();
    let cycles_history = BTreeMap::new();
    let per_canister_meta = with_canister_meta_map(|map| {
        let mut out = BTreeMap::new();
        for (key, value) in map.iter() {
            out.insert(key.to_principal(), value.clone().into());
        }
        out
    });

    let mut st = State {
        config: root.config.into(),
        distinct_canisters: BTreeSet::new(),
        canister_sources,
        commitment_history,
        cycles_history,
        per_canister_meta,
        registered_canister_summaries_cache: None,
        registered_canister_summaries_total_desc_index: None,
        last_indexed_staking_tx_id: root.last_indexed_staking_tx_id,
        oldest_indexed_staking_tx_id: root.oldest_indexed_staking_tx_id,
        staking_index_descending: root.staking_index_descending,
        staking_backfill_complete: root.staking_backfill_complete.or(Some(false)),
        last_indexed_output_tx_id: root.last_indexed_output_tx_id,
        oldest_indexed_output_tx_id: root.oldest_indexed_output_tx_id,
        output_route_index_descending: root.output_route_index_descending,
        output_route_backfill_complete: root.output_route_backfill_complete.or(Some(false)),
        last_indexed_rewards_tx_id: root.last_indexed_rewards_tx_id,
        oldest_indexed_rewards_tx_id: root.oldest_indexed_rewards_tx_id,
        rewards_route_index_descending: root.rewards_route_index_descending,
        rewards_route_backfill_complete: root.rewards_route_backfill_complete.or(Some(false)),
        last_sns_discovery_ts: root.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: root.last_completed_cycles_sweep_ts,
        last_completed_route_sweep_ts: root.last_completed_route_sweep_ts.or(Some(0)),
        active_cycles_sweep: root.active_cycles_sweep,
        initial_cycles_probe_queue: root.initial_cycles_probe_queue,
        active_route_sweep: root.active_route_sweep,
        active_sns_discovery: root.active_sns_discovery,
        main_lock_state_ts: root.main_lock_state_ts,
        last_main_run_ts: root.last_main_run_ts,
        qualifying_commitment_count: root.qualifying_commitment_count,
        raw_icp_commitment_history: BTreeMap::new(),
        neuron_commitment_history: BTreeMap::new(),
        total_output_e8s: root.total_output_e8s.or(Some(0)),
        total_rewards_e8s: root.total_rewards_e8s.or(Some(0)),
        icp_burned_e8s: root.icp_burned_e8s,
        recent_commitments: root.recent_commitments,
        recent_under_threshold_commitments: root.recent_under_threshold_commitments,
        recent_neuron_commitments: root.recent_neuron_commitments,
        recent_under_threshold_neuron_commitments: root.recent_under_threshold_neuron_commitments,
        recent_invalid_commitments: root.recent_invalid_commitments,
        recent_burns: root.recent_burns,
        last_index_run_ts: root.last_index_run_ts,
        commitment_index_fault: root.commitment_index_fault,
        icp_xdr_rate: root.icp_xdr_rate,
        last_icp_xdr_rate_attempt_ts: root.last_icp_xdr_rate_attempt_ts,
        last_icp_xdr_rate_error: root.last_icp_xdr_rate_error,
        canister_module_hash_cache: Vec::new(),
        canister_module_hash_cache_updated_ts: None,
        canister_module_hash_refresh_lock_ts: None,
    };
    rebuild_distinct_canisters(&mut st);
    st
}

pub(crate) fn restore_state_from_stable() -> Option<State> {
    let snapshot = with_root_stable_cell(|cell| cell.get().clone());
    match snapshot {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::Current(root) => Some(restore_state_current(root)),
    }
}
