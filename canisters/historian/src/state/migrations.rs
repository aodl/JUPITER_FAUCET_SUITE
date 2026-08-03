use super::*;

pub(super) const AUTHORIZED_ABANDONED_RELAY_TARGET: &str = "2lo52-kiaaa-aaaar-qaqta-cai";
pub(super) const PRODUCTION_HISTORIAN_ID: &str = "j5gs6-uiaaa-aaaar-qb5cq-cai";
pub(super) const LEGACY_SETUP_ACCOUNT_IDENTIFIER: &str =
    "54467f93c896278cad2d7a6190b021c7f69e393389d96e8a2b47a1ee5e2ad5ab";
pub(super) const CMC_SETUP_ACCOUNT_IDENTIFIER: &str =
    "7a09fb19b536cb547f8e345b46413b213da91c48c58f93565bb474615fc52e21";
pub(super) const LOW_MINTED_CYCLES_DIAGNOSTIC: &str = "CMC notify minted 1590792300000 cycles, below configured relay_initial_cycles 2000000000000; refusing create_canister to avoid historian subsidy after conversion";
pub(super) const FIRST_SETUP_PAYMENT_BLOCK: u64 = 37_414_222;
pub(super) const REFUND_BLOCK: u64 = 37_414_223;
pub(super) const SECOND_SETUP_PAYMENT_BLOCK: u64 = 37_414_358;
pub(super) const CMC_TRANSFER_BLOCK: u64 = 37_414_364;

pub(crate) fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub(super) fn authorized_abandoned_relay_target() -> Principal {
    Principal::from_text(AUTHORIZED_ABANDONED_RELAY_TARGET)
        .expect("invalid authorized abandoned Relay target")
}

pub(super) fn production_historian_id() -> Principal {
    Principal::from_text(PRODUCTION_HISTORIAN_ID).expect("invalid production Historian principal")
}

fn is_authorized_abandoned_prelaunch_relay_job(
    key: &PrincipalKey,
    job: &RetiredRelaySetupJob,
) -> bool {
    let target = authorized_abandoned_relay_target();
    let historian = production_historian_id();
    let legacy_subaccount = jupiter_ic_clients::account::relay_setup_subaccount(target);
    let setup_account = Account {
        owner: historian,
        subaccount: Some(legacy_subaccount),
    };
    let cmc_account = Account {
        owner: jupiter_ic_clients::constants::cycles_minting_canister_id(),
        subaccount: Some(jupiter_ic_clients::account::principal_to_subaccount(
            historian,
        )),
    };

    let first_payment = job
        .payments
        .iter()
        .find(|payment| payment.tx_id == FIRST_SETUP_PAYMENT_BLOCK);
    let second_payment = job
        .payments
        .iter()
        .find(|payment| payment.tx_id == SECOND_SETUP_PAYMENT_BLOCK);
    let payments_match = match (first_payment, second_payment) {
        (Some(first), Some(second)) => {
            !first.from_account_identifier.is_empty()
                && first.from_account_identifier == second.from_account_identifier
                && first.target_canister_id == target
                && first.amount_e8s == 100_000_000
                && first.timestamp_nanos == Some(1_783_774_380_958_013_045)
                && !first.processed
                && first.refunded
                && second.target_canister_id == target
                && second.amount_e8s == 200_000_000
                && second.timestamp_nanos == Some(1_783_775_049_246_080_846)
                && !second.processed
                && !second.refunded
        }
        _ => false,
    };

    let cycle_transfer_matches = job.cycle_transfer.as_ref().is_some_and(|transfer| {
        transfer.kind == RetiredRelaySetupTransferKind::CmcConversion
            && transfer.from_subaccount == setup_account.subaccount
            && transfer.from_account_identifier == LEGACY_SETUP_ACCOUNT_IDENTIFIER
            && transfer.to == cmc_account
            && transfer.to_account_identifier == CMC_SETUP_ACCOUNT_IDENTIFIER
            && transfer.amount_e8s == 94_950_000
            && transfer.fee_e8s == 10_000
            && transfer.memo == Some(1_347_768_404u64.to_le_bytes().to_vec())
            && transfer.created_at_time_nanos == 1_783_775_072_224_720_476
            && transfer.block_index == Some(CMC_TRANSFER_BLOCK)
            && transfer.completed
    });

    let refund_matches = job.refund_transfers.first().is_some_and(|transfer| {
        let Some(first) = first_payment else {
            return false;
        };
        transfer.kind == RetiredRelaySetupTransferKind::Refund
            && transfer.from_subaccount == setup_account.subaccount
            && transfer.from_account_identifier == LEGACY_SETUP_ACCOUNT_IDENTIFIER
            && transfer.to
                == (Account {
                    owner: Principal::anonymous(),
                    subaccount: None,
                })
            && transfer.to_account_identifier == first.from_account_identifier
            && transfer.amount_e8s == 99_990_000
            && transfer.fee_e8s == 10_000
            && transfer.memo == Some(0x4a52_5246u64.to_le_bytes().to_vec())
            && transfer.created_at_time_nanos == 1_783_774_405_254_587_251
            && transfer.block_index == Some(REFUND_BLOCK)
            && transfer.completed
    });

    key == &PrincipalKey::from(target)
        && job.target_canister_id == target
        && job.setup_account == setup_account
        && job.setup_account_identifier == LEGACY_SETUP_ACCOUNT_IDENTIFIER
        && job.status == RetiredRelaySetupStatus::ManualRecoveryRequired
        && job.last_error.as_deref() == Some(LOW_MINTED_CYCLES_DIAGNOSTIC)
        && job.last_indexed_setup_tx_id == Some(SECOND_SETUP_PAYMENT_BLOCK)
        && job.setup_tx_ids.len() == 2
        && job.setup_tx_ids.contains(&FIRST_SETUP_PAYMENT_BLOCK)
        && job.setup_tx_ids.contains(&SECOND_SETUP_PAYMENT_BLOCK)
        && job.setup_amount_seen_e8s == 300_000_000
        && job.setup_amount_processed_e8s == 0
        && job.payments.len() == 2
        && payments_match
        && job.cycle_conversion_e8s == Some(94_950_000)
        && job.cycle_transfer_block_index == Some(CMC_TRANSFER_BLOCK)
        && job.cycles_minted == Some(1_590_792_300_000)
        && job.relay_initial_cycles.is_none()
        && job.relay_funding_e8s.is_none()
        && job.relay_funding_block_index.is_none()
        && job.phase == Some(RetiredRelaySetupPhase::CycleNotifySucceeded)
        && cycle_transfer_matches
        && job.relay_funding_transfer.is_none()
        && job.existing_relay_sweep_transfer.is_none()
        && job.refund_transfers.len() == 1
        && refund_matches
        && job.relay_create_attempt.as_ref().is_some_and(|attempt| {
            attempt.target_canister_id == target
                && attempt.created_at_ts == 1_783_784_987
                && attempt.initial_cycles == 1_000_000_000_000
        })
        && job.relay_canister_id.is_none()
        && !job.code_installed
        && !job.relay_funding_accepted
        && !job.blackhole_update_attempted
        && !job.blackhole_confirmed
        && job.refund_attempt_count == 1
        && job.last_refund_attempt_ts == Some(1_783_774_405)
        && job.refund_blocks == [REFUND_BLOCK]
        && job.created_at_ts == 1_783_774_396
        && job.updated_at_ts == 1_783_851_665
}

pub(crate) fn validate_retired_relay_factory_state(config: &Config) {
    let abandoned_job_key = with_retired_relay_setup_jobs_map(|map| {
        assert!(
            map.len() <= 1,
            "retired Relay setup-job memory contains unexpected pre-launch state"
        );
        map.iter().next().map(|entry| {
            let (key, job) = entry.into_pair();
            assert!(
                is_authorized_abandoned_prelaunch_relay_job(&key, &job),
                "retired Relay setup-job memory contains unexpected pre-launch state"
            );
            key
        })
    });
    with_retired_relay_registry_map(|map| {
        assert!(
            map.get(&PrincipalKey::from(authorized_abandoned_relay_target()))
                .is_none(),
            "retired Relay registry contains unexpected self-service state"
        );
        if map.is_empty() {
            return;
        }
        let Some(canonical_relay_canister_id) = config.canonical_relay_canister_id else {
            panic!("retired Relay registry contains unexpected self-service state");
        };
        assert_eq!(
            map.len(),
            config.canonical_relay_targets.len() as u64,
            "retired Relay registry contains unexpected self-service state"
        );
        for target in &config.canonical_relay_targets {
            let entry = map
                .get(&PrincipalKey::from(*target))
                .expect("retired Relay registry contains unexpected self-service state");
            assert!(
                entry.relay_canister_id == canonical_relay_canister_id
                    && entry.target_canister_id == *target
                    && entry.kind == RetiredRelayRegistryKind::Canonical
                    && entry.status == RetiredRelayRegistryStatus::Active,
                "retired Relay registry contains unexpected self-service state"
            );
        }
    });
    if let Some(key) = abandoned_job_key {
        with_retired_relay_setup_jobs_map(|map| {
            assert!(
                map.remove(&key).is_some(),
                "authorized retired Relay setup job disappeared during cutover"
            );
            assert!(
                map.is_empty(),
                "retired Relay setup-job memory was not emptied during cutover"
            );
        });
    }
    with_retired_relay_setup_jobs_map(|map| {
        assert!(
            map.is_empty(),
            "retired Relay setup-job memory was not emptied during cutover"
        );
    });
    with_relay_setup_entries_map(|map| {
        assert!(
            map.is_empty(),
            "new Relay setup memory contains unexpected first-cutover state"
        );
    });
}

pub(super) fn restore_state_current(root: StableRootState) -> State {
    let canister_tracking_reasons = with_canister_tracking_reasons_map(|map| {
        let mut out = BTreeMap::new();
        for entry in map.iter() {
            let (key, value) = entry.into_pair();
            out.insert(key.to_principal(), value.0.clone());
        }
        out
    });
    let commitment_history = BTreeMap::new();
    let cycles_history = BTreeMap::new();
    let per_canister_meta = with_canister_meta_map(|map| {
        let mut out = BTreeMap::new();
        for entry in map.iter() {
            let (key, value) = entry.into_pair();
            out.insert(key.to_principal(), value.clone().into());
        }
        out
    });
    let mut st = State {
        config: root.config.into(),
        distinct_canisters: BTreeSet::new(),
        canister_tracking_reasons,
        commitment_history,
        cycles_history,
        per_canister_meta,
        cached_cycles_probe_routes: BTreeMap::new(),
        memo_registered_canister_summaries_cache: None,
        memo_registered_canister_summaries_total_desc_index: None,
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
