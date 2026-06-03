use super::*;
pub(super) fn rebuild_distinct_canisters(st: &mut State) {
    st.distinct_canisters = st
        .canister_sources
        .keys()
        .copied()
        .chain(st.commitment_history.keys().copied())
        .chain(stable_commitment_history_keys_internal())
        .chain(st.cycles_history.keys().copied())
        .chain(stable_cycles_history_keys_internal())
        .chain(st.per_canister_meta.keys().copied())
        .collect();
}

pub(super) fn sync_canister_sources_map(
    current: &BTreeMap<Principal, BTreeSet<CanisterSource>>,
    scope: Option<&BTreeSet<Principal>>,
) {
    with_canister_sources_map(|map| match scope {
        Some(principals) => {
            for principal in principals {
                let key = PrincipalKey::from(principal);
                match current.get(principal) {
                    Some(sources) => {
                        let desired = StableSourceSet(sources.clone());
                        let needs_update = map
                            .get(&key)
                            .map(|existing| existing != desired)
                            .unwrap_or(true);
                        if needs_update {
                            map.insert(key, desired);
                        }
                    }
                    None => {
                        map.remove(&key);
                    }
                }
            }
        }
        None => {
            let existing_keys: Vec<_> = map.iter().map(|(key, _)| key).collect();
            for key in existing_keys {
                if !current.contains_key(&key.to_principal()) {
                    map.remove(&key);
                }
            }
            for (principal, sources) in current {
                let key = PrincipalKey::from(principal);
                let desired = StableSourceSet(sources.clone());
                let needs_update = map
                    .get(&key)
                    .map(|existing| existing != desired)
                    .unwrap_or(true);
                if needs_update {
                    map.insert(key, desired);
                }
            }
        }
    });
}

pub(super) fn sync_canister_meta_map(
    current: &BTreeMap<Principal, CanisterMeta>,
    scope: Option<&BTreeSet<Principal>>,
) {
    with_canister_meta_map(|map| match scope {
        Some(principals) => {
            for principal in principals {
                let key = PrincipalKey::from(principal);
                match current.get(principal) {
                    Some(meta) => {
                        let desired: StableCanisterMeta = meta.clone().into();
                        let needs_update = map
                            .get(&key)
                            .map(|existing| existing != desired)
                            .unwrap_or(true);
                        if needs_update {
                            map.insert(key, desired);
                        }
                    }
                    None => {
                        map.remove(&key);
                    }
                }
            }
        }
        None => {
            let existing_keys: Vec<_> = map.iter().map(|(key, _)| key).collect();
            for key in existing_keys {
                if !current.contains_key(&key.to_principal()) {
                    map.remove(&key);
                }
            }
            for (principal, meta) in current {
                let key = PrincipalKey::from(principal);
                let desired: StableCanisterMeta = meta.clone().into();
                let needs_update = map
                    .get(&key)
                    .map(|existing| existing != desired)
                    .unwrap_or(true);
                if needs_update {
                    map.insert(key, desired);
                }
            }
        }
    });
}

pub(super) fn sync_all_commitment_history_maps(
    current: &BTreeMap<Principal, Vec<CommitmentSample>>,
) {
    with_commitment_history_index_map(|map| map.clear_new());
    with_commitment_entry_map(|map| map.clear_new());
    for (principal, samples) in current {
        let ids: Vec<u64> = samples.iter().map(|sample| sample.tx_id).collect();
        if !ids.is_empty() {
            with_commitment_history_index_map(|map| {
                map.insert(PrincipalKey::from(principal), StableU64List(ids));
            });
            with_commitment_entry_map(|map| {
                for sample in samples {
                    map.insert(
                        CommitmentEntryKey::new(principal, sample.tx_id),
                        sample.clone(),
                    );
                }
            });
        }
    }
}

pub(super) fn sync_commitment_history_principals(
    current: &BTreeMap<Principal, Vec<CommitmentSample>>,
    principals: &BTreeSet<Principal>,
) {
    for principal in principals {
        let principal_key = PrincipalKey::from(principal);
        let existing_ids = with_commitment_history_index_map(|map| {
            map.get(&principal_key)
                .map(|ids| ids.0.clone())
                .unwrap_or_default()
        });
        let current_samples = current.get(principal).cloned().unwrap_or_default();
        let current_ids: Vec<u64> = current_samples.iter().map(|sample| sample.tx_id).collect();
        let current_id_set: BTreeSet<u64> = current_ids.iter().copied().collect();

        with_commitment_entry_map(|map| {
            for tx_id in &existing_ids {
                if !current_id_set.contains(tx_id) {
                    map.remove(&CommitmentEntryKey::new(principal, *tx_id));
                }
            }
            for sample in &current_samples {
                let key = CommitmentEntryKey::new(principal, sample.tx_id);
                let needs_update = map
                    .get(&key)
                    .map(|existing| existing != *sample)
                    .unwrap_or(true);
                if needs_update {
                    map.insert(key, sample.clone());
                }
            }
        });

        with_commitment_history_index_map(|map| {
            if current_ids.is_empty() {
                map.remove(&principal_key);
            } else {
                let desired = StableU64List(current_ids);
                let needs_update = map
                    .get(&principal_key)
                    .map(|existing| existing != desired)
                    .unwrap_or(true);
                if needs_update {
                    map.insert(principal_key, desired);
                }
            }
        });
    }
}
