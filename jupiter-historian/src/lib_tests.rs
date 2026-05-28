use super::*;
#[cfg(test)]
// API tests keep fixture construction explicit even when values are Copy.
#[allow(clippy::clone_on_copy, clippy::useless_conversion)]
mod tests {
    use super::*;
    use candid::encode_args;
    use crate::state::{CanisterMeta, CyclesSampleSource, InvalidCommitment, RecentNeuronCommitment};
    use std::collections::{BTreeMap, BTreeSet};

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn sample_account() -> Account {
        Account {
            owner: principal("22255-zqaaa-aaaas-qf6uq-cai"),
            subaccount: None,
        }
    }

    fn alternate_account() -> Account {
        Account {
            owner: principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
            subaccount: Some([7u8; 32]),
        }
    }

    fn sample_init_args() -> InitArgs {
        InitArgs {
            staking_account: sample_account(),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: Some(600),
            cycles_interval_seconds: Some(604800),
            min_tx_e8s: Some(100_000_000),
            max_cycles_entries_per_canister: Some(100),
            max_commitment_entries_per_canister: Some(100),
            max_index_pages_per_tick: Some(10),
            max_canisters_per_cycles_tick: Some(25),
        }
    }

    fn expect_decode_err(raw: &[u8]) -> String {
        match decode_post_upgrade_args_from_bytes(raw) {
            Ok(_) => panic!("decode unexpectedly succeeded"),
            Err(err) => err,
        }
    }

    #[test]
    fn decode_post_upgrade_args_treats_empty_as_none() {
        assert!(decode_post_upgrade_args_from_bytes(&[]).unwrap().is_none());
    }

    #[test]
    fn decode_post_upgrade_args_treats_zero_args_as_none() {
        let raw = encode_args(()).unwrap();
        assert!(decode_post_upgrade_args_from_bytes(&raw).unwrap().is_none());
    }

    #[test]
    fn decode_post_upgrade_args_treats_null_as_none() {
        let raw = encode_args((Option::<UpgradeArgs>::None,)).unwrap();
        assert!(decode_post_upgrade_args_from_bytes(&raw).unwrap().is_none());
    }

    #[test]
    fn decode_post_upgrade_args_wrapper_decodes_valid_none() {
        let raw = encode_args((Option::<UpgradeArgs>::None,)).unwrap();
        assert!(decode_post_upgrade_args(raw).is_none());
    }

    #[test]
    fn decode_post_upgrade_args_decodes_upgrade_record() {
        let raw = encode_args((Some(UpgradeArgs {
            staking_account: Some(alternate_account()),
            ledger_canister_id: None,
            index_canister_id: None,
            enable_sns_tracking: Some(true),
            clear_commitment_index_fault: Some(true),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
            scan_interval_seconds: Some(120),
            cycles_interval_seconds: None,
            min_tx_e8s: None,
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: Some(2),
            max_canisters_per_cycles_tick: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            xrc_canister_id: None,
        }),))
        .unwrap();
        let decoded = decode_post_upgrade_args_from_bytes(&raw).unwrap().unwrap();
        assert_eq!(decoded.staking_account, Some(alternate_account()));
        assert_eq!(decoded.enable_sns_tracking, Some(true));
        assert_eq!(decoded.clear_commitment_index_fault, Some(true));
        assert_eq!(decoded.scan_interval_seconds, Some(120));
        assert_eq!(decoded.max_index_pages_per_tick, Some(2));
    }

    #[test]
    fn decode_post_upgrade_args_rejects_install_args() {
        let raw = encode_args((sample_init_args(),)).unwrap();
        let err = expect_decode_err(&raw);
        assert!(err.contains("received InitArgs in historian post_upgrade"));
    }

    #[test]
    fn decode_post_upgrade_args_rejects_malformed_bytes() {
        let err = expect_decode_err(b"not candid");
        assert!(err.contains("failed to decode historian UpgradeArgs"));
    }

    fn base_state() -> State {
        State {
            config: Config {
                staking_account: sample_account(),
                output_source_account: Account { owner: principal("uccpi-cqaaa-aaaar-qby3q-cai"), subaccount: None },
                output_account: Account { owner: principal("acjuz-liaaa-aaaar-qb4qq-cai"), subaccount: None },
                rewards_account: Account { owner: principal("alk7f-5aaaa-aaaar-qb4ra-cai"), subaccount: None },
                ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
                index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
                cmc_canister_id: Some(principal("rkp4c-7iaaa-aaaaa-aaaca-cai")),
                faucet_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                blackhole_canister_id: principal("77deu-baaaa-aaaar-qb6za-cai"),
                sns_wasm_canister_id: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
                xrc_canister_id: mainnet_xrc_id(),
                enable_sns_tracking: false,
                scan_interval_seconds: 600,
                cycles_interval_seconds: 604800,
                min_tx_e8s: 100_000_000,
                max_cycles_entries_per_canister: 100,
                max_commitment_entries_per_canister: 100,
                max_index_pages_per_tick: 10,
                max_canisters_per_cycles_tick: 25,
            },
            distinct_canisters: BTreeSet::new(),
            canister_sources: BTreeMap::new(),
            commitment_history: BTreeMap::new(),
            cycles_history: BTreeMap::new(),
            per_canister_meta: BTreeMap::new(),
            registered_canister_summaries_cache: None,
            registered_canister_summaries_total_desc_index: None,
            last_indexed_staking_tx_id: None,
            oldest_indexed_staking_tx_id: None,
            staking_index_descending: None,
            staking_backfill_complete: Some(false),
            last_indexed_output_tx_id: None,
            oldest_indexed_output_tx_id: None,
            output_route_index_descending: None,
            output_route_backfill_complete: Some(false),
            last_indexed_rewards_tx_id: None,
            oldest_indexed_rewards_tx_id: None,
            rewards_route_index_descending: None,
            rewards_route_backfill_complete: Some(false),
            last_sns_discovery_ts: 0,
            last_completed_cycles_sweep_ts: 0,
            last_completed_route_sweep_ts: Some(0),
            active_cycles_sweep: None,
            initial_cycles_probe_queue: Vec::new(),
            active_route_sweep: None,
            active_sns_discovery: None,
            main_lock_state_ts: Some(0),
            last_main_run_ts: 1,
            qualifying_commitment_count: None,
            raw_icp_commitment_history: BTreeMap::new(),
            neuron_commitment_history: BTreeMap::new(),
            total_output_e8s: None,
            total_rewards_e8s: None,
            icp_burned_e8s: None,
            recent_commitments: None,
            recent_under_threshold_commitments: None,
            recent_neuron_commitments: None,
            recent_under_threshold_neuron_commitments: None,
            recent_invalid_commitments: None,
            recent_burns: None,
            last_index_run_ts: None,
            commitment_index_fault: None,
            icp_xdr_rate: None,
            last_icp_xdr_rate_attempt_ts: None,
            last_icp_xdr_rate_error: None,
            canister_module_hash_cache: Vec::new(),
            canister_module_hash_cache_updated_ts: None,
            canister_module_hash_refresh_lock_ts: None,
        }
    }


    #[test]
    fn config_from_init_args_uses_mainnet_defaults_for_optional_canisters() {
        let cfg = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: None,
            cycles_interval_seconds: None,
            min_tx_e8s: None,
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });

        assert_eq!(cfg.ledger_canister_id, mainnet_ledger_id());
        assert_eq!(cfg.index_canister_id, mainnet_index_id());
        assert_eq!(cfg.blackhole_canister_id, mainnet_blackhole_id());
        assert_eq!(cfg.output_source_account, mainnet_disburser_staging_account());
        assert_eq!(cfg.output_account, mainnet_output_account());
        assert_eq!(cfg.rewards_account, mainnet_rewards_account());
        assert_eq!(cfg.cmc_canister_id, Some(mainnet_cmc_id()));
        assert_eq!(cfg.faucet_canister_id, Some(mainnet_faucet_id()));
        assert_eq!(cfg.sns_wasm_canister_id, mainnet_sns_wasm_id());
        assert_eq!(cfg.scan_interval_seconds, 600);
        assert_eq!(cfg.cycles_interval_seconds, 604800);
        assert_eq!(cfg.min_tx_e8s, 100_000_000);
    }

    #[test]
    fn config_validation_accepts_minimum_supported_threshold() {
        let mut cfg = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: Some(600),
            cycles_interval_seconds: Some(604800),
            min_tx_e8s: Some(MIN_MIN_TX_E8S),
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });
        cfg.min_tx_e8s = MIN_MIN_TX_E8S;
        validate_config(&cfg);
    }


    #[test]
    fn production_canister_detection_matches_expected_id() {
        assert!(is_production_canister(production_canister_id()));
        assert!(!is_production_canister(principal("aaaaa-aa")));
    }

    #[test]
    fn refresh_registered_canister_summary_updates_cache_incrementally() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 7,
                timestamp_nanos: Some(9_000_000_000),
                amount_e8s: 123_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                last_commitment_ts: Some(9),
                last_cycles_probe_ts: Some(10),
                ..CanisterMeta::default()
            },
        );
        st.cycles_history.insert(
            canister,
            vec![CyclesSample {
                timestamp_nanos: 10_000_000_000,
                cycles: 777,
                source: CyclesSampleSource::BlackholeStatus,
            }],
        );

        refresh_registered_canister_summary(&mut st, canister);
        let cached = st
            .registered_canister_summaries_cache
            .as_ref()
            .and_then(|cache| cache.get(&canister))
            .cloned()
            .expect("cached summary should exist");

        assert_eq!(cached.canister_id, canister);
        assert_eq!(cached.qualifying_commitment_count, 1);
        assert_eq!(cached.total_qualifying_committed_e8s, 123_000_000);
        assert_eq!(cached.last_commitment_ts, Some(9));
        assert_eq!(cached.latest_cycles, Some(777));
        assert_eq!(cached.last_cycles_probe_ts, Some(10));
        assert_eq!(
            st.registered_canister_summaries_total_desc_index,
            Some(vec![canister]),
        );
    }

    #[test]
    fn refresh_registered_canister_summary_keeps_total_desc_index_in_dashboard_order() {
        let first = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let second = principal("uxrrr-q7777-77774-qaaaq-cai");
        let mut st = base_state();

        for (canister, amount_e8s) in [(first, 123_000_000), (second, 456_000_000)] {
            st.canister_sources
                .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
                    tx_id: amount_e8s / 1_000_000,
                    timestamp_nanos: Some(9_000_000_000),
                    amount_e8s,
                    counts_toward_faucet: true,
                }],
            );
            refresh_registered_canister_summary(&mut st, canister);
        }

        assert_eq!(
            st.registered_canister_summaries_total_desc_index,
            Some(vec![second, first]),
        );

        st.commitment_history.insert(
            first,
            vec![CommitmentSample {
                tx_id: 999,
                timestamp_nanos: Some(10_000_000_000),
                amount_e8s: 789_000_000,
                counts_toward_faucet: true,
            }],
        );
        refresh_registered_canister_summary(&mut st, first);

        assert_eq!(
            st.registered_canister_summaries_total_desc_index,
            Some(vec![first, second]),
        );
    }


    #[test]
    fn list_registered_canister_summaries_falls_back_to_slow_path_when_total_desc_index_drifts() {
        let first = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let second = principal("uxrrr-q7777-77774-qaaaq-cai");
        let mut cache = BTreeMap::new();
        cache.insert(
            first,
            RegisteredCanisterSummary {
                canister_id: first,
                sources: vec![CanisterSource::MemoCommitment],
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 123_000_000,
                last_commitment_ts: Some(1),
                latest_cycles: None,
                last_cycles_probe_ts: None,
            },
        );
        cache.insert(
            second,
            RegisteredCanisterSummary {
                canister_id: second,
                sources: vec![CanisterSource::MemoCommitment],
                qualifying_commitment_count: 2,
                total_qualifying_committed_e8s: 456_000_000,
                last_commitment_ts: Some(2),
                latest_cycles: None,
                last_cycles_probe_ts: None,
            },
        );

        let mut st = base_state();
        st.registered_canister_summaries_cache = Some(cache);
        st.registered_canister_summaries_total_desc_index = Some(vec![first]);
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        });

        assert_eq!(response.total, 2);
        assert_eq!(response.items.iter().map(|item| item.canister_id).collect::<Vec<_>>(), vec![second, first]);
    }

    #[test]
    #[should_panic(expected = "min_tx_e8s must be at least")]
    fn config_from_init_args_rejects_threshold_below_minimum() {
        let _ = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: Some(600),
            cycles_interval_seconds: Some(604800),
            min_tx_e8s: Some(MIN_MIN_TX_E8S - 1),
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });
    }

    #[test]
    fn apply_upgrade_args_updates_tuning_fields_and_preserves_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 10,
                counts_toward_faucet: true,
            }],
        );
        st.main_lock_state_ts = Some(99);
        st.commitment_index_fault = Some(CommitmentIndexFault {
            observed_at_ts: 77,
            last_cursor_tx_id: Some(66),
            offending_tx_id: 77,
            message: "latched".to_string(),
        });

        let original_account = st.config.staking_account.clone();
        let original_ledger = st.config.ledger_canister_id;
        let original_index = st.config.index_canister_id;

        apply_upgrade_args(
            &mut st,
            Some(UpgradeArgs {
                enable_sns_tracking: Some(true),
                clear_commitment_index_fault: Some(true),
                scan_interval_seconds: Some(123),
                cycles_interval_seconds: Some(456),
                min_tx_e8s: Some(MIN_MIN_TX_E8S),
                max_cycles_entries_per_canister: Some(11),
                max_commitment_entries_per_canister: Some(12),
                max_index_pages_per_tick: Some(13),
                max_canisters_per_cycles_tick: Some(14),
                blackhole_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                sns_wasm_canister_id: Some(principal("qaa6y-5yaaa-aaaaa-aaafa-cai")),
                ..UpgradeArgs::default()
            }),
        );

        assert_eq!(st.config.staking_account, original_account);
        assert_eq!(st.config.ledger_canister_id, original_ledger);
        assert_eq!(st.config.index_canister_id, original_index);
        assert!(st.config.enable_sns_tracking);
        assert_eq!(st.config.scan_interval_seconds, 123);
        assert_eq!(st.config.cycles_interval_seconds, 456);
        assert_eq!(st.config.min_tx_e8s, MIN_MIN_TX_E8S);
        assert_eq!(st.config.max_cycles_entries_per_canister, 11);
        assert_eq!(st.config.max_commitment_entries_per_canister, 12);
        assert_eq!(st.config.max_index_pages_per_tick, 13);
        assert_eq!(st.config.max_canisters_per_cycles_tick, 14);
        assert_eq!(st.commitment_history.get(&canister).map(|v| v.len()), Some(1));
        assert_eq!(st.main_lock_state_ts, Some(0));
    }

    #[test]
    fn get_public_status_reflects_effective_runtime_config() {
        let mut st = base_state();
        st.config.staking_account = alternate_account();
        st.config.ledger_canister_id = principal("jufzc-caaaa-aaaar-qb5da-cai");
        st.last_index_run_ts = Some(777);
        st.last_completed_cycles_sweep_ts = 888;
        state::set_state(st);

        let status = get_public_status();
        assert_eq!(status.staking_account, alternate_account());
        assert_eq!(status.ledger_canister_id, principal("jufzc-caaaa-aaaar-qb5da-cai"));
        assert_eq!(status.last_index_run_ts, Some(777));
        assert_eq!(status.last_completed_cycles_sweep_ts, Some(888));
        assert!(status.heap_memory_bytes.is_some());
        assert!(status.stable_memory_bytes.is_some());
        assert_eq!(
            status.total_memory_bytes,
            Some(status.heap_memory_bytes.unwrap_or(0).saturating_add(status.stable_memory_bytes.unwrap_or(0))),
        );
    }

    #[test]
    fn get_canister_module_hashes_returns_cached_snapshot_without_live_refresh() {
        let mut st = base_state();
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let controller = principal("77deu-baaaa-aaaar-qb6za-cai");
        st.canister_module_hash_cache = vec![CanisterModuleHash {
            canister_id,
            module_hash_hex: Some("abc123".to_string()),
            controllers: Some(vec![controller]),
            heap_memory_bytes: Some(1024),
            stable_memory_bytes: Some(2048),
            total_memory_bytes: Some(3072),
        }];
        st.canister_module_hash_cache_updated_ts = Some(42);
        state::set_state(st);

        let hashes = get_canister_module_hashes();

        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].canister_id, canister_id);
        assert_eq!(hashes[0].module_hash_hex.as_deref(), Some("abc123"));
        assert_eq!(hashes[0].controllers, Some(vec![controller]));
        assert_eq!(hashes[0].heap_memory_bytes, Some(1024));
        assert_eq!(hashes[0].stable_memory_bytes, Some(2048));
        assert_eq!(hashes[0].total_memory_bytes, Some(3072));
    }

    #[test]
    fn source_module_hash_canister_ids_include_relay_as_suite_canister() {
        let ids: BTreeSet<_> = source_module_hash_canister_ids().into_iter().collect();

        assert_eq!(ids.len(), 7);
        assert!(ids.contains(&principal("uccpi-cqaaa-aaaar-qby3q-cai")));
        assert!(ids.contains(&principal("acjuz-liaaa-aaaar-qb4qq-cai")));
        assert!(ids.contains(&principal("j5gs6-uiaaa-aaaar-qb5cq-cai")));
        assert!(ids.contains(&principal("afisn-gqaaa-aaaar-qb4qa-cai")));
        assert!(ids.contains(&principal("alk7f-5aaaa-aaaar-qb4ra-cai")));
        assert!(ids.contains(&principal("u2qkp-aqaaa-aaaar-qb7ea-cai")));
        assert!(ids.contains(&principal("jufzc-caaaa-aaaar-qb5da-cai")));
    }

    #[test]
    fn canister_module_hash_success_requires_some_loaded_field() {
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let failed = CanisterModuleHash {
            canister_id,
            module_hash_hex: None,
            controllers: None,
            heap_memory_bytes: None,
            stable_memory_bytes: None,
            total_memory_bytes: None,
        };
        let mut successful = failed.clone();
        successful.controllers = Some(Vec::new());

        assert!(!canister_module_hash_has_any_success(&failed));
        assert!(canister_module_hash_has_any_success(&successful));
    }

    #[test]
    fn finish_canister_module_hash_refresh_ignores_stale_lock_owner() {
        let mut st = base_state();
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        st.canister_module_hash_cache = vec![CanisterModuleHash {
            canister_id,
            module_hash_hex: Some("old".to_string()),
            controllers: None,
            heap_memory_bytes: None,
            stable_memory_bytes: None,
            total_memory_bytes: None,
        }];
        st.canister_module_hash_cache_updated_ts = Some(5);
        st.canister_module_hash_refresh_lock_ts = Some(10);
        state::set_state(st);

        let replacement = vec![CanisterModuleHash {
            canister_id,
            module_hash_hex: Some("new".to_string()),
            controllers: None,
            heap_memory_bytes: None,
            stable_memory_bytes: None,
            total_memory_bytes: None,
        }];

        finish_canister_module_hash_refresh(9, 20, replacement.clone());
        state::with_state(|st| {
            assert_eq!(st.canister_module_hash_cache[0].module_hash_hex.as_deref(), Some("old"));
            assert_eq!(st.canister_module_hash_cache_updated_ts, Some(5));
            assert_eq!(st.canister_module_hash_refresh_lock_ts, Some(10));
        });

        finish_canister_module_hash_refresh(10, 21, replacement);
        state::with_state(|st| {
            assert_eq!(st.canister_module_hash_cache[0].module_hash_hex.as_deref(), Some("new"));
            assert_eq!(st.canister_module_hash_cache_updated_ts, Some(21));
            assert_eq!(st.canister_module_hash_refresh_lock_ts, None);
        });
    }

    #[test]
    fn registered_canister_count_requires_qualifying_memo_commitment_history() {
        let memo_only = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let sns_only = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let both = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");

        let mut st = base_state();
        st.canister_sources.insert(memo_only, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        st.canister_sources.insert(
            both,
            BTreeSet::from([CanisterSource::MemoCommitment, CanisterSource::SnsDiscovery]),
        );
        st.commitment_history.insert(
            memo_only,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 80_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.commitment_history.insert(
            both,
            vec![CommitmentSample {
                tx_id: 2,
                timestamp_nanos: Some(2_000_000_000),
                amount_e8s: 50_000_000,
                counts_toward_faucet: true,
            }],
        );

        assert_eq!(count_registered_canisters(&st), 2);
    }

    #[test]
    fn get_public_counts_surfaces_expected_frontend_metrics() {
        let memo_canister = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let sns_only = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.canister_sources.insert(memo_canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        st.commitment_history.insert(
            memo_canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 80_000_000,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 5_000_000,
                    counts_toward_faucet: false,
                },
            ],
        );
        state::set_state(st);

        let counts = get_public_counts();
        assert_eq!(counts.registered_canister_count, 1);
        assert_eq!(counts.qualifying_commitment_count, 1);
        assert_eq!(counts.sns_discovered_canister_count, 1);
        assert_eq!(counts.total_output_e8s, 0);
        assert_eq!(counts.total_rewards_e8s, 0);
    }

    #[test]
    fn get_public_counts_excludes_non_qualifying_memo_canisters_from_registered_totals() {
        let memo_canister = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let mut st = base_state();
        st.canister_sources.insert(memo_canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            memo_canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        let counts = get_public_counts();
        assert_eq!(counts.registered_canister_count, 0);
        assert_eq!(counts.qualifying_commitment_count, 0);
        assert_eq!(counts.sns_discovered_canister_count, 0);
    }

    #[test]
    fn list_registered_canister_summaries_excludes_sns_only_canisters() {
        let sns_only = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        });
        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }


    #[test]
    fn list_registered_canister_summaries_excludes_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        });
        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }

    #[test]
    fn get_canister_overview_hides_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        assert!(get_canister_overview(canister).is_none());
    }

    #[test]
    fn get_canister_overview_counts_lazy_stable_history_points_after_restore() {
        let canister = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 200_000_000,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.cycles_history.insert(
            canister,
            vec![CyclesSample {
                timestamp_nanos: 3_000_000_000,
                cycles: 123,
                source: CyclesSampleSource::BlackholeStatus,
            }],
        );
        state::set_state(st);

        let restored = state::restore_state_from_stable().expect("expected stable root state");
        assert!(restored.commitment_history.is_empty());
        assert!(restored.cycles_history.is_empty());
        state::set_state_root_only(restored);

        let overview = get_canister_overview(canister).expect("registered canister should be visible");
        assert_eq!(overview.commitment_points, 2);
        assert_eq!(overview.cycles_points, 1);
    }


    #[test]
    fn list_registered_canister_summaries_uses_canister_id_as_tie_breaker_for_stable_pagination() {
        let a = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let b = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let mut st = base_state();
        for canister in [a, b] {
            st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000),
                    amount_e8s: 50_000_000,
                    counts_toward_faucet: true,
                }],
            );
            st.per_canister_meta.insert(
                canister,
                CanisterMeta {
                    last_commitment_ts: Some(1_000),
                    ..CanisterMeta::default()
                },
            );
        }
        state::set_state(st);

        let first_page = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(1),
        });
        let second_page = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(1),
            page_size: Some(1),
        });

        assert_eq!(first_page.total, 2);
        assert_eq!(second_page.total, 2);
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(first_page.items[0].canister_id, b.min(a));
        assert_eq!(second_page.items[0].canister_id, b.max(a));
    }

    #[test]
    fn list_registered_canister_summaries_returns_empty_pages_past_the_end() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000),
                amount_e8s: 50_000_000,
                counts_toward_faucet: true,
            }],
        );
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(5),
            page_size: Some(1),
        });
        assert_eq!(response.total, 1);
        assert!(response.items.is_empty());
    }

    #[test]
    fn list_recent_commitments_returns_qualifying_and_non_qualifying_commitments() {
        let qualifying = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let low_amount = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.recent_commitments = Some(vec![
            RecentCommitment {
                canister_id: qualifying,
                raw_icp_memo_text: None,
                tx_id: 11,
                timestamp_nanos: Some(11),
                amount_e8s: 20_000_000,
                counts_toward_faucet: true,
            },
        ]);
        st.recent_under_threshold_commitments = Some(vec![
            RecentCommitment {
                canister_id: low_amount,
                raw_icp_memo_text: None,
                tx_id: 10,
                timestamp_nanos: Some(10),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            },
        ]);
        st.recent_invalid_commitments = Some(vec![InvalidCommitment {
            tx_id: 12,
            timestamp_nanos: Some(12),
            amount_e8s: 20_000_000,
            memo_text: crate::logic::INVALID_MEMO_PLACEHOLDER.to_string(),
        }]);
        state::set_state(st);

        let all = list_recent_commitments(ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        });
        assert_eq!(all.items.len(), 3);
        assert_eq!(all.items[0].tx_id, 12);
        assert_eq!(all.items[0].canister_id, None);
        assert_eq!(all.items[0].memo_text.as_deref(), Some(crate::logic::INVALID_MEMO_PLACEHOLDER));
        assert!(!all.items[0].counts_toward_faucet);
        assert_eq!(all.items[0].outcome_category, RecentCommitmentOutcomeCategory::InvalidTargetMemo);
        assert_eq!(all.items[1].tx_id, 11);
        assert_eq!(all.items[1].canister_id, Some(qualifying));
        assert!(all.items[1].counts_toward_faucet);
        assert_eq!(all.items[1].outcome_category, RecentCommitmentOutcomeCategory::QualifyingCommitment);
        assert_eq!(all.items[2].tx_id, 10);
        assert_eq!(all.items[2].canister_id, Some(low_amount));
        assert!(!all.items[2].counts_toward_faucet);
        assert_eq!(all.items[2].outcome_category, RecentCommitmentOutcomeCategory::UnderThresholdCommitment);

        let qualifying_only = list_recent_commitments(ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(true),
        });
        assert_eq!(qualifying_only.items.len(), 1);
        assert_eq!(qualifying_only.items[0].tx_id, 11);
        assert!(qualifying_only.items[0].counts_toward_faucet);
        assert_eq!(qualifying_only.items[0].outcome_category, RecentCommitmentOutcomeCategory::QualifyingCommitment);
    }

    #[test]
    fn list_recent_commitments_returns_raw_icp_and_neuron_metadata() {
        let raw_canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.recent_commitments = Some(vec![RecentCommitment {
            canister_id: raw_canister,
            raw_icp_memo_text: Some("vault42".to_string()),
            tx_id: 21,
            timestamp_nanos: Some(21),
            amount_e8s: 100_000_000,
            counts_toward_faucet: true,
        }]);
        st.recent_neuron_commitments = Some(vec![RecentNeuronCommitment {
            neuron_id: 11_614_578_985_374_291_210,
            memo_text: Some("vault.memo".to_string()),
            tx_id: 22,
            timestamp_nanos: Some(22),
            amount_e8s: 200_000_000,
            counts_toward_faucet: true,
        }]);
        state::set_state(st);

        let response = list_recent_commitments(ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        });

        let neuron = response.items.iter().find(|item| item.neuron_id.is_some()).expect("neuron item");
        assert_eq!(neuron.neuron_id, Some(11_614_578_985_374_291_210));
        assert_eq!(neuron.canister_id, None);
        assert_eq!(neuron.memo_text.as_deref(), Some("11614578985374291210"));
        assert_eq!(neuron.neuron_memo_text.as_deref(), Some("vault.memo"));

        let raw = response.items.iter().find(|item| item.raw_icp_memo_text.is_some()).expect("raw item");
        assert_eq!(raw.canister_id, Some(raw_canister));
        assert_eq!(raw.raw_icp_memo_text.as_deref(), Some("vault42"));
        assert_eq!(raw.neuron_id, None);
    }

    #[test]
    fn derived_aggregates_fallback_from_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(10),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(20),
                    amount_e8s: 50,
                    counts_toward_faucet: false,
                },
                CommitmentSample {
                    tx_id: 3,
                    timestamp_nanos: Some(30),
                    amount_e8s: 200,
                    counts_toward_faucet: true,
                },
            ],
        );
        initialize_derived_state_if_missing(&mut st);
        assert_eq!(st.qualifying_commitment_count, Some(2));
        assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 3);
    }

    #[test]
    fn derived_aggregates_fallback_includes_raw_icp_and_neuron_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(10),
                amount_e8s: 100,
                counts_toward_faucet: true,
            }],
        );
        st.raw_icp_commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 2,
                timestamp_nanos: Some(20),
                amount_e8s: 200,
                counts_toward_faucet: true,
            }],
        );
        st.neuron_commitment_history.insert(
            42,
            vec![
                CommitmentSample {
                    tx_id: 3,
                    timestamp_nanos: Some(30),
                    amount_e8s: 300,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 4,
                    timestamp_nanos: Some(40),
                    amount_e8s: 400,
                    counts_toward_faucet: false,
                },
            ],
        );

        initialize_derived_state_if_missing(&mut st);

        assert_eq!(st.qualifying_commitment_count, Some(3));
    }


    #[test]
    fn normalize_runtime_state_moves_non_qualifying_commitments_out_of_registered_history() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.config.max_commitment_entries_per_canister = 1;
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 5_000_000,
                    counts_toward_faucet: false,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 50_000_000,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.recent_commitments = Some(vec![RecentCommitment {
            canister_id: canister,
            raw_icp_memo_text: None,
            tx_id: 1,
            timestamp_nanos: Some(1_000_000_000),
            amount_e8s: 5_000_000,
            counts_toward_faucet: false,
        }]);
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                first_seen_ts: Some(1),
                ..CanisterMeta::default()
            },
        );

        normalize_runtime_state(&mut st);

        assert_eq!(st.qualifying_commitment_count, Some(1));
        assert_eq!(st.commitment_history.get(&canister).map(|items| items.len()), Some(1));
        assert_eq!(st.commitment_history.get(&canister).unwrap()[0].tx_id, 2);
        assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(1));
        assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 2);
        assert_eq!(
            st.recent_under_threshold_commitments
                .as_ref()
                .map(|items| items.iter().map(|item| item.tx_id).collect::<Vec<_>>()),
            Some(vec![1]),
        );
        assert_eq!(st.per_canister_meta.get(&canister).and_then(|meta| meta.last_commitment_ts), Some(2));
        assert_eq!(count_registered_canisters(&st), 1);
    }

    #[test]
    fn normalize_runtime_state_prunes_memo_only_registration_when_history_is_non_qualifying() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );

        normalize_runtime_state(&mut st);

        assert_eq!(count_registered_canisters(&st), 0);
        assert!(!st.canister_sources.contains_key(&canister));
        assert!(!st.distinct_canisters.contains(&canister));
        assert!(!st.commitment_history.contains_key(&canister));
        assert!(!st.cycles_history.contains_key(&canister));
        assert!(!st.per_canister_meta.contains_key(&canister));
        assert_eq!(
            st.recent_under_threshold_commitments
                .as_ref()
                .map(|items| items.iter().map(|item| item.tx_id).collect::<Vec<_>>()),
            Some(vec![1]),
        );
    }


    #[test]
    fn normalize_runtime_state_preserves_large_beneficiary_registry() {
        let mut st = base_state();
        for idx in 0..=2_100u32 {
            let canister = Principal::from_slice(&[((idx % 250) + 1) as u8, ((idx / 250) + 1) as u8]);
            st.distinct_canisters.insert(canister);
            st.canister_sources
                .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
                    tx_id: idx as u64 + 1,
                    timestamp_nanos: Some((idx as u64 + 1) * 1_000_000_000),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                }],
            );
        }

        normalize_runtime_state(&mut st);

        assert_eq!(st.distinct_canisters.len(), 2_101);
        assert_eq!(st.canister_sources.len(), 2_101);
        assert_eq!(st.commitment_history.len(), 2_101);
    }

    #[test]
    fn apply_upgrade_args_clamps_runtime_caps() {
        let mut st = base_state();
        apply_upgrade_args(
            &mut st,
            Some(UpgradeArgs {
                max_cycles_entries_per_canister: Some(u32::MAX),
                max_commitment_entries_per_canister: Some(u32::MAX),
                max_index_pages_per_tick: Some(u32::MAX),
                max_canisters_per_cycles_tick: Some(u32::MAX),
                ..UpgradeArgs::default()
            }),
        );

        assert_eq!(st.config.max_cycles_entries_per_canister, MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP);
        assert_eq!(
            st.config.max_commitment_entries_per_canister,
            MAX_COMMITMENT_ENTRIES_PER_CANISTER_HARD_CAP,
        );
        assert_eq!(st.config.max_index_pages_per_tick, MAX_INDEX_PAGES_PER_TICK_HARD_CAP);
        assert_eq!(
            st.config.max_canisters_per_cycles_tick,
            MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP,
        );
    }

    #[test]
    fn public_query_limits_are_clamped() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            (1..=150)
                .map(|tx_id| CommitmentSample {
                    tx_id,
                    timestamp_nanos: Some(tx_id * 1_000_000_000),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                })
                .collect(),
        );
        st.cycles_history.insert(
            canister,
            (1..=150)
                .map(|idx| CyclesSample {
                    timestamp_nanos: idx,
                    cycles: idx as u128,
                    source: CyclesSampleSource::BlackholeStatus,
                })
                .collect(),
        );
        state::set_state(st);

        let canisters = list_canisters(ListCanistersArgs {
            start_after: None,
            limit: Some(5_000),
            source_filter: None,
        });
        assert_eq!(canisters.items.len(), 1);

        let commitments = get_commitment_history(GetCommitmentHistoryArgs {
            canister_id: canister,
            start_after_tx_id: None,
            limit: Some(5_000),
            descending: Some(false),
        });
        assert_eq!(commitments.items.len(), MAX_PUBLIC_QUERY_LIMIT as usize);
        assert_eq!(commitments.next_start_after_tx_id, Some(100));

        let cycles = get_cycles_history(GetCyclesHistoryArgs {
            canister_id: canister,
            start_after_ts: None,
            limit: Some(5_000),
            descending: Some(false),
        });
        assert_eq!(cycles.items.len(), MAX_PUBLIC_QUERY_LIMIT as usize);
        assert_eq!(cycles.next_start_after_ts, Some(100));
    }

    #[test]
    fn list_canisters_pagination_round_trips_without_skips() {
        let canisters = [
            principal("22255-zqaaa-aaaas-qf6uq-cai"),
            principal("r7inp-6aaaa-aaaaa-aaabq-cai"),
            principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
        ];
        let mut st = base_state();
        for canister in canisters {
            st.distinct_canisters.insert(canister);
            st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
                    tx_id: canister.as_slice()[0] as u64,
                    timestamp_nanos: Some(canister.as_slice()[0] as u64),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                }],
            );
        }
        state::set_state(st);

        let first = list_canisters(ListCanistersArgs { start_after: None, limit: Some(2), source_filter: None });
        let second = list_canisters(ListCanistersArgs { start_after: first.next_start_after, limit: Some(2), source_filter: None });
        let returned: Vec<_> = first.items.into_iter().chain(second.items.into_iter()).map(|item| item.canister_id).collect();
        let mut expected = canisters.to_vec();
        expected.sort();
        assert_eq!(returned, expected);
    }

    #[test]
    fn cycles_history_pagination_round_trips_without_skips_in_both_directions() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 100_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister,
            vec![
                CyclesSample { timestamp_nanos: 10, cycles: 1, source: CyclesSampleSource::BlackholeStatus },
                CyclesSample { timestamp_nanos: 20, cycles: 2, source: CyclesSampleSource::BlackholeStatus },
                CyclesSample { timestamp_nanos: 30, cycles: 3, source: CyclesSampleSource::BlackholeStatus },
            ],
        );
        state::set_state(st);

        let first = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: None, limit: Some(2), descending: Some(false) });
        let second = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: first.next_start_after_ts, limit: Some(2), descending: Some(false) });
        let asc: Vec<_> = first.items.iter().chain(second.items.iter()).map(|item| item.timestamp_nanos).collect();
        assert_eq!(asc, vec![10, 20, 30]);

        let first_desc = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: None, limit: Some(2), descending: Some(true) });
        let second_desc = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: first_desc.next_start_after_ts, limit: Some(2), descending: Some(true) });
        let desc: Vec<_> = first_desc.items.iter().chain(second_desc.items.iter()).map(|item| item.timestamp_nanos).collect();
        assert_eq!(desc, vec![30, 20, 10]);
    }

    #[test]
    fn commitment_history_pagination_round_trips_without_skips_in_both_directions() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample { tx_id: 10, timestamp_nanos: Some(10), amount_e8s: 1, counts_toward_faucet: true },
                CommitmentSample { tx_id: 20, timestamp_nanos: Some(20), amount_e8s: 1, counts_toward_faucet: true },
                CommitmentSample { tx_id: 30, timestamp_nanos: Some(30), amount_e8s: 1, counts_toward_faucet: true },
            ],
        );
        state::set_state(st);

        let first = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: None, limit: Some(2), descending: Some(false) });
        let second = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: first.next_start_after_tx_id, limit: Some(2), descending: Some(false) });
        let asc: Vec<_> = first.items.iter().chain(second.items.iter()).map(|item| item.tx_id).collect();
        assert_eq!(asc, vec![10, 20, 30]);

        let first_desc = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: None, limit: Some(2), descending: Some(true) });
        let second_desc = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: first_desc.next_start_after_tx_id, limit: Some(2), descending: Some(true) });
        let desc: Vec<_> = first_desc.items.iter().chain(second_desc.items.iter()).map(|item| item.tx_id).collect();
        assert_eq!(desc, vec![30, 20, 10]);
    }


    #[test]
    fn registered_canister_summaries_roll_up_qualifying_only() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 50,
                    counts_toward_faucet: false,
                },
                CommitmentSample {
                    tx_id: 3,
                    timestamp_nanos: Some(3_000_000_000),
                    amount_e8s: 250,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.cycles_history.insert(
            canister,
            vec![
                CyclesSample {
                    timestamp_nanos: 100,
                    cycles: 5,
                    source: CyclesSampleSource::BlackholeStatus,
                },
                CyclesSample {
                    timestamp_nanos: 200,
                    cycles: 8,
                    source: CyclesSampleSource::BlackholeStatus,
                },
            ],
        );
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                first_seen_ts: Some(1),
                last_commitment_ts: Some(3),
                last_cycles_probe_ts: Some(9),
                last_cycles_probe_result: None,
                ..Default::default()
            },
        );

        let summaries = registered_canister_summaries(&st);
        assert_eq!(summaries.len(), 1);
        let item = &summaries[0];
        assert_eq!(item.qualifying_commitment_count, 2);
        assert_eq!(item.total_qualifying_committed_e8s, 350);
        assert_eq!(item.last_commitment_ts, Some(3));
        assert_eq!(item.latest_cycles, Some(8));
    }
}
