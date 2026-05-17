use super::*;
pub(super) fn mark_registry_principal_dirty(canister_id: Principal) {
    DIRTY_REGISTRY_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

pub(super) fn mark_commitment_principal_dirty(canister_id: Principal) {
    DIRTY_COMMITMENT_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

pub(super) fn mark_cycles_principal_dirty(canister_id: Principal) {
    DIRTY_CYCLES_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

pub(super) fn mark_raw_icp_commitment_principal_dirty(canister_id: Principal) {
    DIRTY_RAW_ICP_COMMITMENT_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

pub(super) fn mark_neuron_commitment_id_dirty(neuron_id: u64) {
    DIRTY_NEURON_COMMITMENT_IDS.with(|dirty| {
        dirty.borrow_mut().insert(neuron_id);
    });
}

pub(super) fn dirty_registry_principals() -> BTreeSet<Principal> {
    DIRTY_REGISTRY_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

pub(super) fn dirty_commitment_principals() -> BTreeSet<Principal> {
    DIRTY_COMMITMENT_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

pub(super) fn dirty_cycles_principals() -> BTreeSet<Principal> {
    DIRTY_CYCLES_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

pub(super) fn dirty_raw_icp_commitment_principals() -> BTreeSet<Principal> {
    DIRTY_RAW_ICP_COMMITMENT_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

pub(super) fn dirty_neuron_commitment_ids() -> BTreeSet<u64> {
    DIRTY_NEURON_COMMITMENT_IDS.with(|dirty| dirty.borrow().clone())
}

pub(super) fn stable_commitment_history_keys_internal() -> BTreeSet<Principal> {
    with_commitment_history_index_map(|map| map.iter().map(|(key, _)| key.to_principal()).collect())
}

pub(super) fn stable_cycles_history_keys_internal() -> BTreeSet<Principal> {
    with_cycles_history_index_map(|map| map.iter().map(|(key, _)| key.to_principal()).collect())
}

pub(super) fn stable_raw_icp_commitment_history_keys_internal() -> BTreeSet<Principal> {
    with_raw_icp_commitment_history_index_map(|map| map.iter().map(|(key, _)| key.to_principal()).collect())
}

pub(super) fn stable_neuron_commitment_history_keys_internal() -> BTreeSet<u64> {
    with_neuron_commitment_history_index_map(|map| map.iter().map(|(key, _)| key).collect())
}

pub(super) fn load_stable_commitment_history_internal(canister_id: Principal) -> Vec<CommitmentSample> {
    with_commitment_history_index_map(|index_map| {
        index_map
            .get(&PrincipalKey::from(canister_id))
            .map(|ids| {
                ids.0
                    .into_iter()
                    .filter_map(|tx_id| {
                        with_commitment_entry_map(|entry_map| {
                            entry_map.get(&CommitmentEntryKey::new(canister_id, tx_id))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub(super) fn load_stable_raw_icp_commitment_history_internal(canister_id: Principal) -> Vec<CommitmentSample> {
    with_raw_icp_commitment_history_index_map(|index_map| {
        index_map
            .get(&PrincipalKey::from(canister_id))
            .map(|ids| {
                ids.0
                    .into_iter()
                    .filter_map(|tx_id| {
                        with_raw_icp_commitment_entry_map(|entry_map| {
                            entry_map.get(&CommitmentEntryKey::new(canister_id, tx_id))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub(super) fn load_stable_neuron_commitment_history_internal(neuron_id: u64) -> Vec<CommitmentSample> {
    with_neuron_commitment_history_index_map(|index_map| {
        index_map
            .get(&neuron_id)
            .map(|ids| {
                ids.0
                    .into_iter()
                    .filter_map(|tx_id| {
                        with_neuron_commitment_entry_map(|entry_map| {
                            entry_map.get(&NeuronCommitmentEntryKey::new(neuron_id, tx_id))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub(super) fn load_stable_cycles_history_internal(canister_id: Principal) -> Vec<CyclesSample> {
    with_cycles_history_index_map(|index_map| {
        index_map
            .get(&PrincipalKey::from(canister_id))
            .map(|ids| {
                ids.0
                    .into_iter()
                    .filter_map(|timestamp_nanos| {
                        with_cycles_entry_map(|entry_map| {
                            entry_map.get(&CyclesEntryKey::new(canister_id, timestamp_nanos))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

