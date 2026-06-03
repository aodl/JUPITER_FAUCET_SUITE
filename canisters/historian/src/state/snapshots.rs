use super::*;
pub(super) fn build_root_snapshot(st: &State) -> StableRootState {
    StableRootState {
        config: st.config.clone().into(),
        last_indexed_staking_tx_id: st.last_indexed_staking_tx_id,
        oldest_indexed_staking_tx_id: st.oldest_indexed_staking_tx_id,
        staking_index_descending: st.staking_index_descending,
        staking_backfill_complete: st.staking_backfill_complete,
        last_indexed_output_tx_id: st.last_indexed_output_tx_id,
        oldest_indexed_output_tx_id: st.oldest_indexed_output_tx_id,
        output_route_index_descending: st.output_route_index_descending,
        output_route_backfill_complete: st.output_route_backfill_complete,
        last_indexed_rewards_tx_id: st.last_indexed_rewards_tx_id,
        oldest_indexed_rewards_tx_id: st.oldest_indexed_rewards_tx_id,
        rewards_route_index_descending: st.rewards_route_index_descending,
        rewards_route_backfill_complete: st.rewards_route_backfill_complete,
        last_sns_discovery_ts: st.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: st.last_completed_cycles_sweep_ts,
        last_completed_route_sweep_ts: st.last_completed_route_sweep_ts,
        active_cycles_sweep: st.active_cycles_sweep.clone(),
        initial_cycles_probe_queue: st.initial_cycles_probe_queue.clone(),
        active_route_sweep: st.active_route_sweep.clone(),
        active_sns_discovery: st.active_sns_discovery.clone(),
        main_lock_state_ts: st.main_lock_state_ts,
        last_main_run_ts: st.last_main_run_ts,
        qualifying_commitment_count: st.qualifying_commitment_count,
        total_output_e8s: st.total_output_e8s,
        total_rewards_e8s: st.total_rewards_e8s,
        icp_burned_e8s: st.icp_burned_e8s,
        recent_commitments: st.recent_commitments.clone(),
        recent_under_threshold_commitments: st.recent_under_threshold_commitments.clone(),
        recent_neuron_commitments: st.recent_neuron_commitments.clone(),
        recent_under_threshold_neuron_commitments: st
            .recent_under_threshold_neuron_commitments
            .clone(),
        recent_invalid_commitments: st.recent_invalid_commitments.clone(),
        recent_burns: st.recent_burns.clone(),
        last_index_run_ts: st.last_index_run_ts,
        commitment_index_fault: st.commitment_index_fault.clone(),
        icp_xdr_rate: st.icp_xdr_rate.clone(),
        last_icp_xdr_rate_attempt_ts: st.last_icp_xdr_rate_attempt_ts,
        last_icp_xdr_rate_error: st.last_icp_xdr_rate_error.clone(),
    }
}

pub(super) fn persist_snapshot_sections_scoped(
    st: &State,
    dirty_sections: u8,
    registry_scope: Option<&BTreeSet<Principal>>,
    commitment_scope: Option<&BTreeSet<Principal>>,
    cycles_scope: Option<&BTreeSet<Principal>>,
    raw_icp_commitment_scope: Option<&BTreeSet<Principal>>,
    neuron_commitment_scope: Option<&BTreeSet<u64>>,
) {
    if dirty_sections & DIRTY_REGISTRY != 0 {
        sync_canister_sources_map(&st.canister_sources, registry_scope);
        sync_canister_meta_map(&st.per_canister_meta, registry_scope);
    }
    if dirty_sections & DIRTY_COMMITMENTS != 0 {
        if let Some(scope) = commitment_scope {
            sync_commitment_history_principals(&st.commitment_history, scope);
        } else {
            sync_all_commitment_history_maps(&st.commitment_history);
        }
    }
    if dirty_sections & DIRTY_CYCLES != 0 {
        if let Some(scope) = cycles_scope {
            sync_cycles_history_principals(&st.cycles_history, scope);
        } else {
            sync_all_cycles_history_maps(&st.cycles_history);
        }
    }
    if dirty_sections & DIRTY_RAW_ICP_COMMITMENTS != 0 {
        if let Some(scope) = raw_icp_commitment_scope {
            sync_raw_icp_commitment_history_principals(&st.raw_icp_commitment_history, scope);
        } else {
            sync_all_raw_icp_commitment_history_maps(&st.raw_icp_commitment_history);
        }
    }
    if dirty_sections & DIRTY_NEURON_COMMITMENTS != 0 {
        if let Some(scope) = neuron_commitment_scope {
            sync_neuron_commitment_history_ids(&st.neuron_commitment_history, scope);
        } else {
            sync_all_neuron_commitment_history_maps(&st.neuron_commitment_history);
        }
    }
    if dirty_sections & DIRTY_ROOT != 0 {
        // Commit the root section last so the durable root always points at fully written
        // bulk sections. This keeps the root as the final commit marker if a trap occurs before
        // the map-backed writes complete.
        with_root_stable_cell(|cell| {
            cell.set(VersionedStableState::Current(build_root_snapshot(st)))
                .expect("failed to persist historian root stable state");
        });
    }
}

pub(super) fn persist_snapshot_sections(st: &State, dirty_sections: u8) {
    persist_snapshot_sections_scoped(st, dirty_sections, None, None, None, None, None);
}

pub(super) fn persist_snapshot(st: &State) {
    persist_snapshot_sections(st, DIRTY_ALL);
}
