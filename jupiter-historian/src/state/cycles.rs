fn sync_all_cycles_history_maps(current: &BTreeMap<Principal, Vec<CyclesSample>>) {
    with_cycles_history_index_map(|map| map.clear_new());
    with_cycles_entry_map(|map| map.clear_new());
    for (principal, samples) in current {
        let timestamps: Vec<u64> = samples.iter().map(|sample| sample.timestamp_nanos).collect();
        if !timestamps.is_empty() {
            with_cycles_history_index_map(|map| {
                map.insert(PrincipalKey::from(principal), StableU64List(timestamps));
            });
            with_cycles_entry_map(|map| {
                for sample in samples {
                    map.insert(CyclesEntryKey::new(principal, sample.timestamp_nanos), sample.clone());
                }
            });
        }
    }
}

fn sync_cycles_history_principals(
    current: &BTreeMap<Principal, Vec<CyclesSample>>,
    principals: &BTreeSet<Principal>,
) {
    for principal in principals {
        let principal_key = PrincipalKey::from(principal);
        let existing_timestamps = with_cycles_history_index_map(|map| {
            map.get(&principal_key)
                .map(|ids| ids.0.clone())
                .unwrap_or_default()
        });
        let current_samples = current.get(principal).cloned().unwrap_or_default();
        let current_timestamps: Vec<u64> = current_samples.iter().map(|sample| sample.timestamp_nanos).collect();
        let current_timestamp_set: BTreeSet<u64> = current_timestamps.iter().copied().collect();

        with_cycles_entry_map(|map| {
            for timestamp_nanos in &existing_timestamps {
                if !current_timestamp_set.contains(timestamp_nanos) {
                    map.remove(&CyclesEntryKey::new(principal, *timestamp_nanos));
                }
            }
            for sample in &current_samples {
                let key = CyclesEntryKey::new(principal, sample.timestamp_nanos);
                let needs_update = map.get(&key).map(|existing| existing != *sample).unwrap_or(true);
                if needs_update {
                    map.insert(key, sample.clone());
                }
            }
        });

        with_cycles_history_index_map(|map| {
            if current_timestamps.is_empty() {
                map.remove(&principal_key);
            } else {
                let desired = StableU64List(current_timestamps);
                let needs_update = map.get(&principal_key).map(|existing| existing != desired).unwrap_or(true);
                if needs_update {
                    map.insert(principal_key, desired);
                }
            }
        });
    }
}

fn sync_all_raw_icp_commitment_history_maps(current: &BTreeMap<Principal, Vec<CommitmentSample>>) {
    with_raw_icp_commitment_history_index_map(|map| map.clear_new());
    with_raw_icp_commitment_entry_map(|map| map.clear_new());
    for (principal, samples) in current {
        let ids: Vec<u64> = samples.iter().map(|sample| sample.tx_id).collect();
        if !ids.is_empty() {
            with_raw_icp_commitment_history_index_map(|map| {
                map.insert(PrincipalKey::from(principal), StableU64List(ids));
            });
            with_raw_icp_commitment_entry_map(|map| {
                for sample in samples {
                    map.insert(CommitmentEntryKey::new(principal, sample.tx_id), sample.clone());
                }
            });
        }
    }
}

fn sync_raw_icp_commitment_history_principals(
    current: &BTreeMap<Principal, Vec<CommitmentSample>>,
    principals: &BTreeSet<Principal>,
) {
    for principal in principals {
        let principal_key = PrincipalKey::from(principal);
        let existing_ids = with_raw_icp_commitment_history_index_map(|map| {
            map.get(&principal_key)
                .map(|ids| ids.0.clone())
                .unwrap_or_default()
        });
        let current_samples = current.get(principal).cloned().unwrap_or_default();
        let current_ids: Vec<u64> = current_samples.iter().map(|sample| sample.tx_id).collect();
        let current_id_set: BTreeSet<u64> = current_ids.iter().copied().collect();

        with_raw_icp_commitment_entry_map(|map| {
            for tx_id in &existing_ids {
                if !current_id_set.contains(tx_id) {
                    map.remove(&CommitmentEntryKey::new(principal, *tx_id));
                }
            }
            for sample in &current_samples {
                let key = CommitmentEntryKey::new(principal, sample.tx_id);
                let needs_update = map.get(&key).map(|existing| existing != *sample).unwrap_or(true);
                if needs_update {
                    map.insert(key, sample.clone());
                }
            }
        });

        with_raw_icp_commitment_history_index_map(|map| {
            if current_ids.is_empty() {
                map.remove(&principal_key);
            } else {
                let desired = StableU64List(current_ids);
                let needs_update = map.get(&principal_key).map(|existing| existing != desired).unwrap_or(true);
                if needs_update {
                    map.insert(principal_key, desired);
                }
            }
        });
    }
}

fn sync_all_neuron_commitment_history_maps(current: &BTreeMap<u64, Vec<CommitmentSample>>) {
    with_neuron_commitment_history_index_map(|map| map.clear_new());
    with_neuron_commitment_entry_map(|map| map.clear_new());
    for (neuron_id, samples) in current {
        let ids: Vec<u64> = samples.iter().map(|sample| sample.tx_id).collect();
        if !ids.is_empty() {
            with_neuron_commitment_history_index_map(|map| {
                map.insert(*neuron_id, StableU64List(ids));
            });
            with_neuron_commitment_entry_map(|map| {
                for sample in samples {
                    map.insert(NeuronCommitmentEntryKey::new(*neuron_id, sample.tx_id), sample.clone());
                }
            });
        }
    }
}

fn sync_neuron_commitment_history_ids(
    current: &BTreeMap<u64, Vec<CommitmentSample>>,
    neuron_ids: &BTreeSet<u64>,
) {
    for neuron_id in neuron_ids {
        let existing_ids = with_neuron_commitment_history_index_map(|map| {
            map.get(neuron_id)
                .map(|ids| ids.0.clone())
                .unwrap_or_default()
        });
        let current_samples = current.get(neuron_id).cloned().unwrap_or_default();
        let current_ids: Vec<u64> = current_samples.iter().map(|sample| sample.tx_id).collect();
        let current_id_set: BTreeSet<u64> = current_ids.iter().copied().collect();

        with_neuron_commitment_entry_map(|map| {
            for tx_id in &existing_ids {
                if !current_id_set.contains(tx_id) {
                    map.remove(&NeuronCommitmentEntryKey::new(*neuron_id, *tx_id));
                }
            }
            for sample in &current_samples {
                let key = NeuronCommitmentEntryKey::new(*neuron_id, sample.tx_id);
                let needs_update = map.get(&key).map(|existing| existing != *sample).unwrap_or(true);
                if needs_update {
                    map.insert(key, sample.clone());
                }
            }
        });

        with_neuron_commitment_history_index_map(|map| {
            if current_ids.is_empty() {
                map.remove(neuron_id);
            } else {
                let desired = StableU64List(current_ids);
                let needs_update = map.get(neuron_id).map(|existing| existing != desired).unwrap_or(true);
                if needs_update {
                    map.insert(*neuron_id, desired);
                }
            }
        });
    }
}
