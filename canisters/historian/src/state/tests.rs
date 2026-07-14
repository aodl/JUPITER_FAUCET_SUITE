use super::*;
#[cfg(test)]
// Stable-state tests intentionally mirror storage shapes and assert map absence directly.
#[allow(clippy::module_inception, clippy::unnecessary_get_then_check)]
mod tests {
    use super::*;
    use crate::RegisteredCanisterSummary;
    use std::collections::{BTreeMap, BTreeSet};

    fn reset_test_storage() {
        with_root_stable_cell(|cell| {
            cell.set(VersionedStableState::Uninitialized);
        });
        with_canister_sources_map(|map| map.clear_new());
        with_canister_meta_map(|map| map.clear_new());
        with_commitment_history_index_map(|map| map.clear_new());
        with_commitment_entry_map(|map| map.clear_new());
        with_cycles_history_index_map(|map| map.clear_new());
        with_cycles_entry_map(|map| map.clear_new());
        with_raw_icp_commitment_history_index_map(|map| map.clear_new());
        with_raw_icp_commitment_entry_map(|map| map.clear_new());
        with_neuron_commitment_history_index_map(|map| map.clear_new());
        with_neuron_commitment_entry_map(|map| map.clear_new());
        PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(0));
        clear_persistence_dirty();
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn principal(bytes: &[u8]) -> Principal {
        Principal::from_slice(bytes)
    }

    fn sample_config() -> Config {
        Config {
            staking_account: Account {
                owner: principal(&[1]),
                subaccount: None,
            },
            output_source_account: Account {
                owner: principal(&[11]),
                subaccount: None,
            },
            output_account: Account {
                owner: principal(&[12]),
                subaccount: Some([3u8; 32]),
            },
            rewards_account: Account {
                owner: principal(&[13]),
                subaccount: None,
            },
            ledger_canister_id: principal(&[2]),
            index_canister_id: principal(&[3]),
            cmc_canister_id: Some(principal(&[4])),
            faucet_canister_id: Some(principal(&[5])),
            blackhole_canister_id: principal(&[6]),
            cycles_probe_policy: None,
            sns_wasm_canister_id: principal(&[7]),
            xrc_canister_id: principal(&[8]),
            enable_sns_tracking: true,
            scan_interval_seconds: 60,
            cycles_interval_seconds: 120,
            min_tx_e8s: 100_000_000,
            max_cycles_entries_per_canister: 100,
            max_commitment_entries_per_canister: 100,
            max_index_pages_per_tick: 10,
            max_canisters_per_cycles_tick: 10,
            relay_factory_enabled: false,
            relay_setup_min_e8s: 300_000_000,
            relay_setup_dust_e8s: 10_000,
            relay_setup_refund_cooldown_seconds: 300,
            relay_initial_cycles: 2_000_000_000_000,
            relay_cycle_safety_margin_e8s: 5_000_000,
            relay_min_subaccount_one_seed_e8s: 100_020_000,
            self_service_relay_interval_seconds: 86400,
            self_service_relay_max_transfers_per_tick: Some(10),
            io_surplus_neuron_id: crate::DEFAULT_IO_SURPLUS_NEURON_ID,
            canonical_relay_canister_id: Some(crate::mainnet_relay_id()),
            canonical_relay_targets: crate::mainnet_canonical_relay_targets(),
        }
    }

    #[test]
    fn runtime_config_log_line_includes_all_config_fields() {
        let line = runtime_config_log_line(&sample_config());
        assert!(line.starts_with("CONFIG "));
        assert!(line.contains("staking_account="));
        assert!(line.contains("output_source_account="));
        assert!(line.contains("output_account="));
        assert!(line.contains("rewards_account="));
        assert!(line.contains("ledger_canister_id="));
        assert!(line.contains("index_canister_id="));
        assert!(line.contains("cmc_canister_id="));
        assert!(line.contains("faucet_canister_id="));
        assert!(line.contains("blackhole_canister_id="));
        assert!(line.contains("sns_wasm_canister_id="));
        assert!(line.contains("xrc_canister_id="));
        assert!(line.contains("enable_sns_tracking=true"));
        assert!(line.contains("scan_interval_seconds=60"));
        assert!(line.contains("cycles_interval_seconds=120"));
        assert!(line.contains("min_tx_e8s=100000000"));
        assert!(line.contains("max_cycles_entries_per_canister=100"));
        assert!(line.contains("max_commitment_entries_per_canister=100"));
        assert!(line.contains("max_index_pages_per_tick=10"));
        assert!(line.contains("max_canisters_per_cycles_tick=10"));
    }

    fn snapshot_sources_map() -> BTreeMap<Principal, BTreeSet<CanisterSource>> {
        with_canister_sources_map(|map| {
            let mut out = BTreeMap::new();
            for entry in map.iter() {
                let (key, value) = entry.into_pair();
                out.insert(key.to_principal(), value.0.clone());
            }
            out
        })
    }

    fn snapshot_meta_map() -> BTreeMap<Principal, StableCanisterMeta> {
        with_canister_meta_map(|map| {
            let mut out = BTreeMap::new();
            for entry in map.iter() {
                let (key, value) = entry.into_pair();
                out.insert(key.to_principal(), value.clone());
            }
            out
        })
    }

    fn snapshot_commitment_history_map() -> BTreeMap<Principal, Vec<CommitmentSample>> {
        with_commitment_history_index_map(|index_map| {
            let mut out = BTreeMap::new();
            for entry in index_map.iter() {
                let (key, ids) = entry.into_pair();
                let canister_id = key.to_principal();
                let mut samples = Vec::new();
                for tx_id in ids.0 {
                    if let Some(sample) = with_commitment_entry_map(|entry_map| {
                        entry_map.get(&CommitmentEntryKey::new(canister_id, tx_id))
                    }) {
                        samples.push(sample);
                    }
                }
                if !samples.is_empty() {
                    out.insert(canister_id, samples);
                }
            }
            out
        })
    }

    fn snapshot_cycles_history_map() -> BTreeMap<Principal, Vec<CyclesSample>> {
        with_cycles_history_index_map(|index_map| {
            let mut out = BTreeMap::new();
            for entry in index_map.iter() {
                let (key, timestamps) = entry.into_pair();
                let canister_id = key.to_principal();
                let mut samples = Vec::new();
                for timestamp_nanos in timestamps.0 {
                    if let Some(sample) = with_cycles_entry_map(|entry_map| {
                        entry_map.get(&CyclesEntryKey::new(canister_id, timestamp_nanos))
                    }) {
                        samples.push(sample);
                    }
                }
                if !samples.is_empty() {
                    out.insert(canister_id, samples);
                }
            }
            out
        })
    }

    fn snapshot_raw_icp_commitment_history_map() -> BTreeMap<Principal, Vec<CommitmentSample>> {
        with_raw_icp_commitment_history_index_map(|index_map| {
            let mut out = BTreeMap::new();
            for entry in index_map.iter() {
                let (key, ids) = entry.into_pair();
                let canister_id = key.to_principal();
                let mut samples = Vec::new();
                for tx_id in ids.0 {
                    if let Some(sample) = with_raw_icp_commitment_entry_map(|entry_map| {
                        entry_map.get(&CommitmentEntryKey::new(canister_id, tx_id))
                    }) {
                        samples.push(sample);
                    }
                }
                if !samples.is_empty() {
                    out.insert(canister_id, samples);
                }
            }
            out
        })
    }

    fn snapshot_neuron_commitment_history_map() -> BTreeMap<u64, Vec<CommitmentSample>> {
        with_neuron_commitment_history_index_map(|index_map| {
            let mut out = BTreeMap::new();
            for entry in index_map.iter() {
                let (neuron_id, ids) = entry.into_pair();
                let mut samples = Vec::new();
                for tx_id in ids.0 {
                    if let Some(sample) = with_neuron_commitment_entry_map(|entry_map| {
                        entry_map.get(&NeuronCommitmentEntryKey::new(neuron_id, tx_id))
                    }) {
                        samples.push(sample);
                    }
                }
                if !samples.is_empty() {
                    out.insert(neuron_id, samples);
                }
            }
            out
        })
    }

    fn snapshot_relay_registry_by_target_map() -> BTreeMap<Principal, RelayRegistryEntry> {
        with_relay_registry_by_target_map(|map| {
            let mut out = BTreeMap::new();
            for entry in map.iter() {
                let (key, registration) = entry.into_pair();
                out.insert(key.to_principal(), registration.clone());
            }
            out
        })
    }

    fn relay_entry(target: Principal, relay: Principal) -> RelayRegistryEntry {
        RelayRegistryEntry {
            relay_canister_id: relay,
            target_canister_id: target,
            kind: RelayRegistryKind::SelfService,
            status: RelayRegistryStatus::Active,
            setup_account: None,
            setup_account_identifier: None,
            setup_amount_e8s: None,
            setup_tx_ids: Vec::new(),
            relay_wasm_hash_hex: None,
            final_controllers: None,
            log_visibility_public: None,
            created_at_ts: None,
            activated_at_ts: None,
        }
    }

    #[test]
    fn relay_registry_remap_updates_target_entry() {
        reset_test_storage();
        let target = principal(&[30]);
        let relay_a = principal(&[31]);
        let relay_b = principal(&[32]);
        let mut registry = BTreeMap::new();
        registry.insert(target, relay_entry(target, relay_a));
        sync_relay_factory_maps(&registry, &BTreeMap::new(), None);

        registry.insert(target, relay_entry(target, relay_b));
        sync_relay_factory_maps(&registry, &BTreeMap::new(), Some(&BTreeSet::from([target])));

        let stored = snapshot_relay_registry_by_target_map();
        assert_eq!(stored.get(&target).unwrap().relay_canister_id, relay_b);
    }

    #[test]
    fn relay_registry_removal_deletes_target_entry() {
        reset_test_storage();
        let target = principal(&[33]);
        let relay = principal(&[34]);
        let mut registry = BTreeMap::new();
        registry.insert(target, relay_entry(target, relay));
        sync_relay_factory_maps(&registry, &BTreeMap::new(), None);

        registry.remove(&target);
        sync_relay_factory_maps(&registry, &BTreeMap::new(), Some(&BTreeSet::from([target])));

        assert!(snapshot_relay_registry_by_target_map().is_empty());
    }

    #[test]
    fn cleaned_historian_root_decodes_current_root_with_removed_legacy_fields() {
        #[derive(CandidType)]
        struct RootWithRemovedLegacyHistoryFields {
            config: StableConfig,
            last_indexed_staking_tx_id: Option<u64>,
            oldest_indexed_staking_tx_id: Option<u64>,
            staking_index_descending: Option<bool>,
            staking_backfill_complete: Option<bool>,
            last_indexed_output_tx_id: Option<u64>,
            oldest_indexed_output_tx_id: Option<u64>,
            output_route_index_descending: Option<bool>,
            output_route_backfill_complete: Option<bool>,
            last_indexed_rewards_tx_id: Option<u64>,
            oldest_indexed_rewards_tx_id: Option<u64>,
            rewards_route_index_descending: Option<bool>,
            rewards_route_backfill_complete: Option<bool>,
            last_sns_discovery_ts: u64,
            last_completed_cycles_sweep_ts: u64,
            last_completed_route_sweep_ts: Option<u64>,
            active_cycles_sweep: Option<ActiveCyclesSweep>,
            initial_cycles_probe_queue: Vec<Principal>,
            active_route_sweep: Option<ActiveRouteSweep>,
            active_sns_discovery: Option<ActiveSnsDiscovery>,
            main_lock_state_ts: Option<u64>,
            last_main_run_ts: u64,
            qualifying_commitment_count: Option<u64>,
            raw_icp_commitment_history: BTreeMap<Principal, Vec<CommitmentSample>>,
            neuron_commitment_history: BTreeMap<u64, Vec<CommitmentSample>>,
            total_output_e8s: Option<u64>,
            total_rewards_e8s: Option<u64>,
            icp_burned_e8s: Option<u64>,
            recent_commitments: Option<Vec<RecentCommitment>>,
            recent_under_threshold_commitments: Option<Vec<RecentCommitment>>,
            recent_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
            recent_under_threshold_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
            recent_invalid_commitments: Option<Vec<InvalidCommitment>>,
            recent_burns: Option<Vec<RecentBurn>>,
            last_index_run_ts: Option<u64>,
            commitment_index_fault: Option<CommitmentIndexFault>,
            icp_xdr_rate: Option<IcpXdrRateSnapshot>,
            last_icp_xdr_rate_attempt_ts: Option<u64>,
            last_icp_xdr_rate_error: Option<String>,
        }

        #[derive(CandidType)]
        enum VersionedRootWithRemovedLegacyHistoryFields {
            Current(RootWithRemovedLegacyHistoryFields),
        }

        let mut st = State::new(sample_config(), 12_345);
        st.last_indexed_staking_tx_id = Some(99);
        st.last_sns_discovery_ts = 88;
        st.last_main_run_ts = 12_345;
        st.qualifying_commitment_count = Some(77);
        let root = build_root_snapshot(&st);
        let old_root = RootWithRemovedLegacyHistoryFields {
            config: root.config,
            last_indexed_staking_tx_id: root.last_indexed_staking_tx_id,
            oldest_indexed_staking_tx_id: root.oldest_indexed_staking_tx_id,
            staking_index_descending: root.staking_index_descending,
            staking_backfill_complete: root.staking_backfill_complete,
            last_indexed_output_tx_id: root.last_indexed_output_tx_id,
            oldest_indexed_output_tx_id: root.oldest_indexed_output_tx_id,
            output_route_index_descending: root.output_route_index_descending,
            output_route_backfill_complete: root.output_route_backfill_complete,
            last_indexed_rewards_tx_id: root.last_indexed_rewards_tx_id,
            oldest_indexed_rewards_tx_id: root.oldest_indexed_rewards_tx_id,
            rewards_route_index_descending: root.rewards_route_index_descending,
            rewards_route_backfill_complete: root.rewards_route_backfill_complete,
            last_sns_discovery_ts: root.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: root.last_completed_cycles_sweep_ts,
            last_completed_route_sweep_ts: root.last_completed_route_sweep_ts,
            active_cycles_sweep: root.active_cycles_sweep,
            initial_cycles_probe_queue: root.initial_cycles_probe_queue,
            active_route_sweep: root.active_route_sweep,
            active_sns_discovery: root.active_sns_discovery,
            main_lock_state_ts: root.main_lock_state_ts,
            last_main_run_ts: root.last_main_run_ts,
            qualifying_commitment_count: root.qualifying_commitment_count,
            raw_icp_commitment_history: BTreeMap::from([(
                principal(&[90]),
                vec![CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(10),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                }],
            )]),
            neuron_commitment_history: BTreeMap::from([(
                123,
                vec![CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(20),
                    amount_e8s: 200,
                    counts_toward_faucet: true,
                }],
            )]),
            total_output_e8s: root.total_output_e8s,
            total_rewards_e8s: root.total_rewards_e8s,
            icp_burned_e8s: root.icp_burned_e8s,
            recent_commitments: root.recent_commitments,
            recent_under_threshold_commitments: root.recent_under_threshold_commitments,
            recent_neuron_commitments: root.recent_neuron_commitments,
            recent_under_threshold_neuron_commitments: root
                .recent_under_threshold_neuron_commitments,
            recent_invalid_commitments: root.recent_invalid_commitments,
            recent_burns: root.recent_burns,
            last_index_run_ts: root.last_index_run_ts,
            commitment_index_fault: root.commitment_index_fault,
            icp_xdr_rate: root.icp_xdr_rate,
            last_icp_xdr_rate_attempt_ts: root.last_icp_xdr_rate_attempt_ts,
            last_icp_xdr_rate_error: root.last_icp_xdr_rate_error,
        };
        let bytes = candid::encode_one(VersionedRootWithRemovedLegacyHistoryFields::Current(
            old_root,
        ))
        .expect("failed to encode root with removed legacy fields");
        let decoded: VersionedStableState = candid::decode_one(&bytes)
            .expect("cleaned historian root should decode extra legacy fields");

        match decoded {
            VersionedStableState::Current(root) => {
                assert_eq!(root.last_indexed_staking_tx_id, Some(99));
                assert_eq!(root.last_sns_discovery_ts, 88);
                assert_eq!(root.last_main_run_ts, 12_345);
                assert_eq!(root.qualifying_commitment_count, Some(77));
            }
            VersionedStableState::Uninitialized => panic!("expected decoded current root"),
        }
    }

    #[test]
    fn stable_restore_is_none_before_first_persist() {
        reset_test_storage();
        assert!(restore_state_from_stable().is_none());
    }

    #[test]
    fn set_state_round_trips_histories_without_persisting_derived_cache() {
        reset_test_storage();
        let canister_id = principal(&[9]);
        let mut st = State::new(sample_config(), 5_000);
        st.distinct_canisters.insert(canister_id);
        let mut sources = BTreeSet::new();
        sources.insert(CanisterSource::MemoCommitment);
        st.canister_sources.insert(canister_id, sources);
        st.commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 7,
                timestamp_nanos: Some(77),
                amount_e8s: 100_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.raw_icp_commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 8,
                timestamp_nanos: Some(78),
                amount_e8s: 200_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.neuron_commitment_history.insert(
            42,
            vec![CommitmentSample {
                tx_id: 9,
                timestamp_nanos: Some(79),
                amount_e8s: 300_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister_id,
            vec![CyclesSample {
                timestamp_nanos: 88,
                cycles: 123_456,
                source: CyclesSampleSource::BlackholeStatus,
            }],
        );
        st.per_canister_meta.insert(
            canister_id,
            CanisterMeta {
                first_seen_ts: Some(1),
                last_commitment_ts: Some(77),
                last_cycles_probe_ts: Some(88),
                last_cycles_probe_result: Some(CyclesProbeResult::Ok(
                    CyclesSampleSource::BlackholeStatus,
                )),
                last_burn_tx_id: Some(11),
                last_burn_scan_tx_id: Some(12),
                burned_e8s: 42,
            },
        );
        let mut cache = BTreeMap::new();
        cache.insert(
            canister_id,
            RegisteredCanisterSummary {
                canister_id,
                sources: vec![CanisterSource::MemoCommitment],
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 100_000_000,
                last_commitment_ts: Some(77),
                latest_cycles: Some(123_456),
                last_cycles_probe_ts: Some(88),
            },
        );
        st.registered_canister_summaries_cache = Some(cache);
        st.registered_canister_summaries_total_desc_index = Some(vec![canister_id]);
        set_state(st);

        let root_snapshot = with_root_stable_cell(|cell| cell.get().clone());
        match root_snapshot {
            VersionedStableState::Current(_) => {}
            VersionedStableState::Uninitialized => {
                panic!("expected persisted historian root state")
            }
        }
        let restored = restore_state_from_stable().expect("expected persisted historian state");
        assert_eq!(restored.distinct_canisters.len(), 1);
        assert!(restored.commitment_history.get(&canister_id).is_none());
        assert!(restored.cycles_history.get(&canister_id).is_none());
        assert!(restored
            .raw_icp_commitment_history
            .get(&canister_id)
            .is_none());
        assert!(restored.neuron_commitment_history.get(&42).is_none());
        assert_eq!(stable_commitment_history_for(canister_id)[0].tx_id, 7);
        assert_eq!(
            stable_raw_icp_commitment_history_for(canister_id)[0].tx_id,
            8
        );
        assert_eq!(stable_neuron_commitment_history_for(42)[0].tx_id, 9);
        assert_eq!(stable_cycles_history_for(canister_id)[0].cycles, 123_456);
        assert_eq!(
            restored
                .per_canister_meta
                .get(&canister_id)
                .expect("missing canister meta")
                .burned_e8s,
            42
        );
        assert!(restored.registered_canister_summaries_cache.is_none());
        assert!(restored
            .registered_canister_summaries_total_desc_index
            .is_none());
    }

    #[test]
    fn with_state_mut_persists_recent_feeds_to_stable_storage() {
        reset_test_storage();
        let canister_id = principal(&[10]);
        set_state(State::new(sample_config(), 6_000));

        with_state_mut(|st| {
            st.recent_invalid_commitments = Some(vec![InvalidCommitment {
                tx_id: 12,
                timestamp_nanos: Some(120),
                amount_e8s: 99,
                memo_text: "<invalid memo>".to_string(),
            }]);
            st.recent_burns = Some(vec![RecentBurn {
                canister_id,
                tx_id: 13,
                timestamp_nanos: Some(130),
                amount_e8s: 55,
            }]);
            st.main_lock_state_ts = Some(66);
        });

        let restored =
            restore_state_from_stable().expect("expected persisted historian state after mutation");
        assert_eq!(restored.main_lock_state_ts, Some(66));
        assert_eq!(
            restored
                .recent_invalid_commitments
                .as_ref()
                .expect("missing invalid commitments")[0]
                .tx_id,
            12
        );
        assert_eq!(
            restored
                .recent_burns
                .as_ref()
                .expect("missing recent burns")[0]
                .canister_id,
            canister_id
        );
    }

    #[test]
    fn persistence_batch_defers_writes_until_flush_boundary() {
        reset_test_storage();
        set_state(State::new(sample_config(), 7_000));

        {
            let _batch = begin_persistence_batch();
            with_state_mut(|st| {
                st.last_indexed_staking_tx_id = Some(88);
                st.main_lock_state_ts = Some(77);
            });
            let restored_mid = restore_state_from_stable()
                .expect("expected persisted state before batch mutation");
            assert_ne!(restored_mid.last_indexed_staking_tx_id, Some(88));
            assert_ne!(restored_mid.main_lock_state_ts, Some(77));
            persist_dirty_state();
        }

        let restored =
            restore_state_from_stable().expect("expected persisted state after batch flush");
        assert_eq!(restored.last_indexed_staking_tx_id, Some(88));
        assert_eq!(restored.main_lock_state_ts, Some(77));
    }

    #[test]
    fn section_scoped_mutation_only_flushes_target_sections() {
        reset_test_storage();
        let canister_id = principal(&[12]);
        let mut st = State::new(sample_config(), 9_000);
        st.canister_sources.insert(
            canister_id,
            BTreeSet::from([CanisterSource::MemoCommitment]),
        );
        st.commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 31,
                timestamp_nanos: Some(310),
                amount_e8s: 500,
                counts_toward_faucet: true,
            }],
        );
        st.raw_icp_commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 32,
                timestamp_nanos: Some(315),
                amount_e8s: 550,
                counts_toward_faucet: true,
            }],
        );
        st.neuron_commitment_history.insert(
            77,
            vec![CommitmentSample {
                tx_id: 33,
                timestamp_nanos: Some(318),
                amount_e8s: 575,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister_id,
            vec![CyclesSample {
                timestamp_nanos: 320,
                cycles: 600,
                source: CyclesSampleSource::SelfCanister,
            }],
        );
        st.per_canister_meta.insert(
            canister_id,
            CanisterMeta {
                first_seen_ts: Some(1),
                last_commitment_ts: Some(2),
                last_cycles_probe_ts: Some(3),
                last_cycles_probe_result: Some(CyclesProbeResult::Ok(
                    CyclesSampleSource::SelfCanister,
                )),
                last_burn_tx_id: Some(4),
                last_burn_scan_tx_id: Some(5),
                burned_e8s: 6,
            },
        );
        set_state(st);

        let restored = restore_state_from_stable()
            .expect("expected restored historian state before root-only mutation");
        assert!(restored.raw_icp_commitment_history.is_empty());
        assert!(restored.neuron_commitment_history.is_empty());
        set_state_root_only(restored);

        let sources_before = snapshot_sources_map();
        let meta_before = snapshot_meta_map();
        let commitments_before = snapshot_commitment_history_map();
        let cycles_before = snapshot_cycles_history_map();
        let raw_icp_commitments_before = snapshot_raw_icp_commitment_history_map();
        let neuron_commitments_before = snapshot_neuron_commitment_history_map();

        with_root_state_mut(|st| {
            st.main_lock_state_ts = Some(1234);
        });

        let restored_after = restore_state_from_stable()
            .expect("expected restored historian state after root-only mutation");
        assert_eq!(restored_after.main_lock_state_ts, Some(1234));
        assert_eq!(snapshot_sources_map(), sources_before);
        assert_eq!(snapshot_meta_map(), meta_before);
        assert_eq!(snapshot_commitment_history_map(), commitments_before);
        assert_eq!(snapshot_cycles_history_map(), cycles_before);
        assert_eq!(
            snapshot_raw_icp_commitment_history_map(),
            raw_icp_commitments_before
        );
        assert_eq!(
            snapshot_neuron_commitment_history_map(),
            neuron_commitments_before
        );
    }

    #[test]
    fn clear_loaded_history_caches_after_flush_preserves_stable_histories() {
        reset_test_storage();
        let canister_id = principal(&[13]);
        let raw_canister_id = principal(&[14]);
        let neuron_id = 91;
        let mut st = State::new(sample_config(), 9_100);
        st.commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 41,
                timestamp_nanos: Some(410),
                amount_e8s: 1_000,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister_id,
            vec![CyclesSample {
                timestamp_nanos: 420,
                cycles: 2_000,
                source: CyclesSampleSource::BlackholeStatus,
            }],
        );
        st.raw_icp_commitment_history.insert(
            raw_canister_id,
            vec![CommitmentSample {
                tx_id: 43,
                timestamp_nanos: Some(430),
                amount_e8s: 3_000,
                counts_toward_faucet: true,
            }],
        );
        st.neuron_commitment_history.insert(
            neuron_id,
            vec![CommitmentSample {
                tx_id: 44,
                timestamp_nanos: Some(440),
                amount_e8s: 4_000,
                counts_toward_faucet: false,
            }],
        );
        set_state(st);

        clear_loaded_history_caches_after_flush();

        with_state(|st| {
            assert!(st.commitment_history.is_empty());
            assert!(st.cycles_history.is_empty());
            assert!(st.raw_icp_commitment_history.is_empty());
            assert!(st.neuron_commitment_history.is_empty());
        });
        assert_eq!(stable_commitment_history_for(canister_id)[0].tx_id, 41);
        assert_eq!(stable_cycles_history_for(canister_id)[0].cycles, 2_000);
        assert_eq!(
            stable_raw_icp_commitment_history_for(raw_canister_id)[0].tx_id,
            43
        );
        assert_eq!(stable_neuron_commitment_history_for(neuron_id)[0].tx_id, 44);
    }

    #[test]
    fn clear_loaded_history_caches_then_append_preserves_existing_stable_entries() {
        reset_test_storage();
        let canister_id = principal(&[15]);
        let raw_canister_id = principal(&[16]);
        let neuron_id = 92;
        let mut st = State::new(sample_config(), 9_200);
        st.commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 51,
                timestamp_nanos: Some(510),
                amount_e8s: 1_100,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister_id,
            vec![CyclesSample {
                timestamp_nanos: 520,
                cycles: 2_100,
                source: CyclesSampleSource::SelfCanister,
            }],
        );
        st.raw_icp_commitment_history.insert(
            raw_canister_id,
            vec![CommitmentSample {
                tx_id: 53,
                timestamp_nanos: Some(530),
                amount_e8s: 3_100,
                counts_toward_faucet: true,
            }],
        );
        st.neuron_commitment_history.insert(
            neuron_id,
            vec![CommitmentSample {
                tx_id: 54,
                timestamp_nanos: Some(540),
                amount_e8s: 4_100,
                counts_toward_faucet: false,
            }],
        );
        set_state(st);
        clear_loaded_history_caches_after_flush();

        with_root_registry_and_commitments_canister_state_mut(canister_id, |st| {
            ensure_commitment_history_loaded(st, canister_id);
            st.commitment_history
                .entry(canister_id)
                .or_default()
                .push(CommitmentSample {
                    tx_id: 55,
                    timestamp_nanos: Some(550),
                    amount_e8s: 1_200,
                    counts_toward_faucet: true,
                });
        });
        with_root_registry_and_cycles_canister_state_mut(canister_id, |st| {
            ensure_cycles_history_loaded(st, canister_id);
            st.cycles_history
                .entry(canister_id)
                .or_default()
                .push(CyclesSample {
                    timestamp_nanos: 560,
                    cycles: 2_200,
                    source: CyclesSampleSource::BlackholeStatus,
                });
        });
        with_root_and_raw_icp_commitments_state_mut(raw_canister_id, |st| {
            ensure_raw_icp_commitment_history_loaded(st, raw_canister_id);
            st.raw_icp_commitment_history
                .entry(raw_canister_id)
                .or_default()
                .push(CommitmentSample {
                    tx_id: 57,
                    timestamp_nanos: Some(570),
                    amount_e8s: 3_200,
                    counts_toward_faucet: true,
                });
        });
        with_root_and_neuron_commitments_state_mut(neuron_id, |st| {
            ensure_neuron_commitment_history_loaded(st, neuron_id);
            st.neuron_commitment_history
                .entry(neuron_id)
                .or_default()
                .push(CommitmentSample {
                    tx_id: 58,
                    timestamp_nanos: Some(580),
                    amount_e8s: 4_200,
                    counts_toward_faucet: true,
                });
        });

        assert_eq!(
            stable_commitment_history_for(canister_id)
                .iter()
                .map(|sample| sample.tx_id)
                .collect::<Vec<_>>(),
            vec![51, 55]
        );
        assert_eq!(
            stable_cycles_history_for(canister_id)
                .iter()
                .map(|sample| sample.timestamp_nanos)
                .collect::<Vec<_>>(),
            vec![520, 560]
        );
        assert_eq!(
            stable_raw_icp_commitment_history_for(raw_canister_id)
                .iter()
                .map(|sample| sample.tx_id)
                .collect::<Vec<_>>(),
            vec![53, 57]
        );
        assert_eq!(
            stable_neuron_commitment_history_for(neuron_id)
                .iter()
                .map(|sample| sample.tx_id)
                .collect::<Vec<_>>(),
            vec![54, 58]
        );
    }

    #[test]
    fn root_only_cache_clear_does_not_rewrite_stable_history_maps() {
        reset_test_storage();
        let canister_id = principal(&[17]);
        let raw_canister_id = principal(&[18]);
        let neuron_id = 93;
        let mut st = State::new(sample_config(), 9_300);
        st.commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 61,
                timestamp_nanos: Some(610),
                amount_e8s: 1_300,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister_id,
            vec![CyclesSample {
                timestamp_nanos: 620,
                cycles: 2_300,
                source: CyclesSampleSource::BlackholeStatus,
            }],
        );
        st.raw_icp_commitment_history.insert(
            raw_canister_id,
            vec![CommitmentSample {
                tx_id: 63,
                timestamp_nanos: Some(630),
                amount_e8s: 3_300,
                counts_toward_faucet: true,
            }],
        );
        st.neuron_commitment_history.insert(
            neuron_id,
            vec![CommitmentSample {
                tx_id: 64,
                timestamp_nanos: Some(640),
                amount_e8s: 4_300,
                counts_toward_faucet: false,
            }],
        );
        set_state(st);

        let commitments_before = snapshot_commitment_history_map();
        let cycles_before = snapshot_cycles_history_map();
        let raw_icp_commitments_before = snapshot_raw_icp_commitment_history_map();
        let neuron_commitments_before = snapshot_neuron_commitment_history_map();

        clear_loaded_history_caches_after_flush();

        assert_eq!(snapshot_commitment_history_map(), commitments_before);
        assert_eq!(snapshot_cycles_history_map(), cycles_before);
        assert_eq!(
            snapshot_raw_icp_commitment_history_map(),
            raw_icp_commitments_before
        );
        assert_eq!(
            snapshot_neuron_commitment_history_map(),
            neuron_commitments_before
        );
    }

    #[test]
    #[should_panic(
        expected = "cannot clear loaded history caches while persistence sections are dirty"
    )]
    fn clear_loaded_history_caches_rejects_dirty_persistence_sections() {
        reset_test_storage();
        set_state(State::new(sample_config(), 9_400));
        PERSISTENCE_DIRTY_SECTIONS.with(|dirty| dirty.set(DIRTY_ROOT));

        clear_loaded_history_caches_after_flush();
    }

    #[test]
    fn restore_keeps_bulk_histories_in_stable_storage_until_requested() {
        reset_test_storage();
        let canister_id = principal(&[31]);
        let mut st = State::new(sample_config(), 10_000);
        st.canister_sources.insert(
            canister_id,
            BTreeSet::from([CanisterSource::MemoCommitment]),
        );
        st.commitment_history.insert(
            canister_id,
            vec![CommitmentSample {
                tx_id: 91,
                timestamp_nanos: Some(910),
                amount_e8s: 111,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister_id,
            vec![CyclesSample {
                timestamp_nanos: 920,
                cycles: 222,
                source: CyclesSampleSource::SelfCanister,
            }],
        );
        set_state(st);

        let restored = restore_state_from_stable().expect("expected restored historian state");
        assert!(restored.commitment_history.is_empty());
        assert!(restored.cycles_history.is_empty());
        assert_eq!(stable_commitment_history_for(canister_id)[0].tx_id, 91);
        assert_eq!(stable_cycles_history_for(canister_id)[0].cycles, 222);
    }

    #[test]
    fn canister_scoped_commitment_flush_only_rewrites_target_canister_history() {
        reset_test_storage();
        let canister_a = principal(&[21]);
        let canister_b = principal(&[22]);
        let mut st = State::new(sample_config(), 9_500);
        for canister_id in [canister_a, canister_b] {
            st.canister_sources.insert(
                canister_id,
                BTreeSet::from([CanisterSource::MemoCommitment]),
            );
            st.per_canister_meta
                .insert(canister_id, CanisterMeta::default());
        }
        st.commitment_history.insert(
            canister_a,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(10),
                amount_e8s: 100,
                counts_toward_faucet: true,
            }],
        );
        st.commitment_history.insert(
            canister_b,
            vec![CommitmentSample {
                tx_id: 2,
                timestamp_nanos: Some(20),
                amount_e8s: 200,
                counts_toward_faucet: true,
            }],
        );
        set_state(st);

        let commitments_before = snapshot_commitment_history_map();
        assert_eq!(commitments_before.get(&canister_b).unwrap()[0].tx_id, 2);

        with_root_registry_and_commitments_canister_state_mut(canister_a, |st| {
            st.commitment_history
                .get_mut(&canister_a)
                .unwrap()
                .push(CommitmentSample {
                    tx_id: 3,
                    timestamp_nanos: Some(30),
                    amount_e8s: 300,
                    counts_toward_faucet: true,
                });
            st.per_canister_meta
                .entry(canister_a)
                .or_default()
                .last_commitment_ts = Some(30);
        });

        let commitments_after = snapshot_commitment_history_map();
        assert_eq!(commitments_after.get(&canister_a).unwrap().len(), 2);
        assert_eq!(commitments_after.get(&canister_a).unwrap()[1].tx_id, 3);
        assert_eq!(
            commitments_after.get(&canister_b),
            commitments_before.get(&canister_b)
        );
    }
}
