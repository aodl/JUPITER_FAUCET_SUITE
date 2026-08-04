use super::*;

pub(super) const AUTHORIZED_ABANDONED_RELAY_TARGET: &str = "2lo52-kiaaa-aaaar-qaqta-cai";
pub(super) const PRODUCTION_HISTORIAN_ID: &str = "j5gs6-uiaaa-aaaar-qb5cq-cai";
pub(super) const LEGACY_SETUP_ACCOUNT_IDENTIFIER: &str =
    "54467f93c896278cad2d7a6190b021c7f69e393389d96e8a2b47a1ee5e2ad5ab";
pub(super) const CMC_SETUP_ACCOUNT_IDENTIFIER: &str =
    "7a09fb19b536cb547f8e345b46413b213da91c48c58f93565bb474615fc52e21";
pub(super) const LOW_MINTED_CYCLES_DIAGNOSTIC: &str = "CMC notify minted 1590792300000 cycles, below configured relay_initial_cycles 2000000000000; refusing create_canister to avoid historian subsidy after conversion";
#[cfg(test)]
pub(super) const FIRST_SETUP_PAYMENT_BLOCK: u64 = 37_414_222;
#[cfg(test)]
pub(super) const REFUND_BLOCK: u64 = 37_414_223;
#[cfg(test)]
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

fn validate_authorized_abandoned_prelaunch_relay_job(
    key: &PrincipalKey,
    job: &RetiredRelaySetupJob,
) -> Result<(), &'static str> {
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

    if key != &PrincipalKey::from(target) || job.target_canister_id != target {
        return Err("target");
    }
    if job.setup_account != setup_account
        || job.setup_account_identifier != LEGACY_SETUP_ACCOUNT_IDENTIFIER
    {
        return Err("setup_account");
    }
    if job.status != RetiredRelaySetupStatus::ManualRecoveryRequired {
        return Err("status");
    }
    if job.last_error.as_deref() != Some(LOW_MINTED_CYCLES_DIAGNOSTIC) {
        return Err("diagnostic");
    }
    let Some(cycle_transfer) = job.cycle_transfer.as_ref() else {
        return Err("cmc_transfer");
    };
    if job.cycle_conversion_e8s != Some(94_950_000)
        || job.cycle_transfer_block_index != Some(CMC_TRANSFER_BLOCK)
        || cycle_transfer.kind != RetiredRelaySetupTransferKind::CmcConversion
        || cycle_transfer.from_subaccount != setup_account.subaccount
        || cycle_transfer.from_account_identifier != LEGACY_SETUP_ACCOUNT_IDENTIFIER
        || cycle_transfer.to != cmc_account
        || cycle_transfer.to_account_identifier != CMC_SETUP_ACCOUNT_IDENTIFIER
        || cycle_transfer.amount_e8s != 94_950_000
        || cycle_transfer.fee_e8s != 10_000
        || cycle_transfer.memo != Some(1_347_768_404u64.to_le_bytes().to_vec())
        || cycle_transfer.block_index != Some(CMC_TRANSFER_BLOCK)
        || !cycle_transfer.completed
    {
        return Err("cmc_transfer");
    }
    if job.cycles_minted != Some(1_590_792_300_000) {
        return Err("cycles_minted");
    }
    let Some(create_attempt) = job.relay_create_attempt.as_ref() else {
        return Err("create_attempt");
    };
    if create_attempt.target_canister_id != target
        || create_attempt.initial_cycles != 1_000_000_000_000
    {
        return Err("create_attempt");
    }
    if job.relay_canister_id.is_some() {
        return Err("relay_child");
    }
    if job.code_installed {
        return Err("relay_install");
    }
    if job.relay_funding_transfer.is_some()
        || job.relay_funding_block_index.is_some()
        || job.relay_funding_accepted
    {
        return Err("relay_funding");
    }
    if job.existing_relay_sweep_transfer.is_some() {
        return Err("relay_sweep");
    }
    if job.blackhole_update_attempted || job.blackhole_confirmed {
        return Err("controller_handoff");
    }
    Ok(())
}

pub(crate) fn validate_retired_relay_factory_state(config: &Config) {
    let abandoned_job_key = with_retired_relay_setup_jobs_map(|map| {
        assert!(
            map.len() <= 1,
            "retired Relay setup-job memory contains unexpected pre-launch state"
        );
        map.iter().next().map(|entry| {
            let (key, job) = entry.into_pair();
            if let Err(invariant) = validate_authorized_abandoned_prelaunch_relay_job(&key, &job) {
                panic!("authorized abandoned Relay setup mismatch: {invariant}");
            }
            key
        })
    });
    with_retired_relay_registry_map(|map| {
        if !map.is_empty() {
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
        }
        assert!(
            map.get(&PrincipalKey::from(authorized_abandoned_relay_target()))
                .is_none(),
            "retired Relay registry contains unexpected self-service state"
        );
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
