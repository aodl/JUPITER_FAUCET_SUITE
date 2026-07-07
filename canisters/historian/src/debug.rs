use super::*;
#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub distinct_canister_count: u32,
    pub last_indexed_staking_tx_id: Option<u64>,
    pub last_indexed_output_tx_id: Option<u64>,
    pub last_indexed_rewards_tx_id: Option<u64>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    pub last_completed_route_sweep_ts: Option<u64>,
    pub active_cycles_sweep_present: bool,
    pub active_cycles_sweep_next_index: Option<u64>,
    pub active_route_sweep_present: bool,
    pub active_route_sweep_next_index: Option<u64>,
    pub last_index_run_ts: Option<u64>,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugConfig {
    pub staking_account: Account,
    pub output_source_account: Account,
    pub output_account: Account,
    pub rewards_account: Account,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub cmc_canister_id: Option<Principal>,
    pub faucet_canister_id: Option<Principal>,
    pub blackhole_canister_id: Principal,
    pub sns_wasm_canister_id: Principal,
    pub xrc_canister_id: Principal,
    pub enable_sns_tracking: bool,
    pub scan_interval_seconds: u64,
    pub cycles_interval_seconds: u64,
    pub min_tx_e8s: u64,
    pub max_cycles_entries_per_canister: u32,
    pub max_commitment_entries_per_canister: u32,
    pub max_index_pages_per_tick: u32,
    pub max_canisters_per_cycles_tick: u32,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub enum DebugRefreshIcpXdrRateResult {
    Ok,
    Err(String),
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
pub(super) fn debug_state() -> DebugState {
    guard_debug_api_not_production();
    state::with_state(|st| DebugState {
        distinct_canister_count: st.distinct_canisters.len() as u32,
        last_indexed_staking_tx_id: st.last_indexed_staking_tx_id,
        last_indexed_output_tx_id: st.last_indexed_output_tx_id,
        last_indexed_rewards_tx_id: st.last_indexed_rewards_tx_id,
        last_sns_discovery_ts: st.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: st.last_completed_cycles_sweep_ts,
        last_completed_route_sweep_ts: st.last_completed_route_sweep_ts,
        active_cycles_sweep_present: st.active_cycles_sweep.is_some(),
        active_cycles_sweep_next_index: st.active_cycles_sweep.as_ref().map(|s| s.next_index),
        active_route_sweep_present: st.active_route_sweep.is_some(),
        active_route_sweep_next_index: st.active_route_sweep.as_ref().map(|s| s.next_index),
        last_index_run_ts: st.last_index_run_ts,
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
pub(super) fn debug_config() -> DebugConfig {
    guard_debug_api_not_production();
    state::with_state(|st| DebugConfig {
        staking_account: st.config.staking_account.clone(),
        output_source_account: st.config.output_source_account.clone(),
        output_account: st.config.output_account.clone(),
        rewards_account: st.config.rewards_account.clone(),
        ledger_canister_id: st.config.ledger_canister_id,
        index_canister_id: st.config.index_canister_id,
        cmc_canister_id: st.config.cmc_canister_id,
        faucet_canister_id: st.config.faucet_canister_id,
        blackhole_canister_id: st.config.blackhole_canister_id,
        sns_wasm_canister_id: st.config.sns_wasm_canister_id,
        xrc_canister_id: st.config.xrc_canister_id,
        enable_sns_tracking: st.config.enable_sns_tracking,
        scan_interval_seconds: st.config.scan_interval_seconds,
        cycles_interval_seconds: st.config.cycles_interval_seconds,
        min_tx_e8s: st.config.min_tx_e8s,
        max_cycles_entries_per_canister: st.config.max_cycles_entries_per_canister,
        max_commitment_entries_per_canister: st.config.max_commitment_entries_per_canister,
        max_index_pages_per_tick: st.config.max_index_pages_per_tick,
        max_canisters_per_cycles_tick: st.config.max_canisters_per_cycles_tick,
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) async fn debug_driver_tick() {
    guard_debug_api_not_production();
    scheduler::main_tick(true).await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) async fn debug_refresh_icp_xdr_rate_cache() -> DebugRefreshIcpXdrRateResult {
    guard_debug_api_not_production();
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let xrc_canister_id = state::with_state(|st| st.config.xrc_canister_id);
    match scheduler::debug_refresh_icp_xdr_rate_now(now_secs, xrc_canister_id).await {
        Ok(()) => DebugRefreshIcpXdrRateResult::Ok,
        Err(err) => DebugRefreshIcpXdrRateResult::Err(err),
    }
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_set_last_completed_cycles_sweep_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.last_completed_cycles_sweep_ts = ts.unwrap_or(0));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_set_last_sns_discovery_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.last_sns_discovery_ts = ts.unwrap_or(0));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_set_last_indexed_staking_tx_id(tx_id: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| {
        st.last_indexed_staking_tx_id = tx_id;
        // This debug hook seeds only the public/latest staking cursor. Reset the
        // derived ordering/backfill metadata so the next driver tick redetects
        // the real index ordering and, for newest-first indexes, resumes older
        // backfill from the seeded cursor instead of staying in legacy ascending
        // mode.
        st.oldest_indexed_staking_tx_id = tx_id;
        st.staking_index_descending = None;
        st.staking_backfill_complete = Some(false);
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_reset_runtime_state() {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| {
        st.active_cycles_sweep = None;
        st.main_lock_state_ts = Some(0);
        st.last_main_run_ts = 0;
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_set_main_lock_expires_at_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.main_lock_state_ts = Some(ts.unwrap_or(0)));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_reset_derived_state() {
    guard_debug_api_not_production();
    state::with_state_mut(|st| {
        st.distinct_canisters.clear();
        st.canister_sources.clear();
        st.commitment_history.clear();
        st.cycles_history.clear();
        st.per_canister_meta.clear();
        st.last_indexed_staking_tx_id = None;
        st.oldest_indexed_staking_tx_id = None;
        st.staking_index_descending = None;
        st.staking_backfill_complete = Some(false);
        st.last_indexed_output_tx_id = None;
        st.oldest_indexed_output_tx_id = None;
        st.output_route_index_descending = None;
        st.output_route_backfill_complete = Some(false);
        st.last_indexed_rewards_tx_id = None;
        st.oldest_indexed_rewards_tx_id = None;
        st.rewards_route_index_descending = None;
        st.rewards_route_backfill_complete = Some(false);
        st.last_sns_discovery_ts = 0;
        st.last_completed_cycles_sweep_ts = 0;
        st.last_completed_route_sweep_ts = Some(0);
        st.active_cycles_sweep = None;
        st.active_route_sweep = None;
        st.main_lock_state_ts = Some(0);
        st.last_main_run_ts = 0;
        st.qualifying_commitment_count = Some(0);
        st.raw_icp_commitment_history.clear();
        st.neuron_commitment_history.clear();
        st.total_output_e8s = Some(0);
        st.total_rewards_e8s = Some(0);
        st.recent_commitments = Some(Vec::new());
        st.recent_under_threshold_commitments = Some(Vec::new());
        st.recent_neuron_commitments = Some(Vec::new());
        st.recent_under_threshold_neuron_commitments = Some(Vec::new());
        st.recent_invalid_commitments = Some(Vec::new());
        st.last_index_run_ts = Some(0);
        st.commitment_index_fault = None;
        st.icp_xdr_rate = None;
        st.last_icp_xdr_rate_attempt_ts = None;
        st.last_icp_xdr_rate_error = None;
        st.relay_registry_by_target.clear();
        st.relay_targets_by_relay.clear();
        st.relay_setup_jobs.clear();
        st.registered_canister_summaries_cache = Some(BTreeMap::new());
        st.registered_canister_summaries_total_desc_index = Some(Vec::new());
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
pub(super) fn debug_get_relay_setup_job(target: Principal) -> Option<RelaySetupJob> {
    guard_debug_api_not_production();
    state::with_state(|st| st.relay_setup_jobs.get(&target).cloned())
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_insert_relay_registry_entry(entry: RelayRegistryEntry) {
    guard_debug_api_not_production();
    state::with_root_and_relay_factory_state_mut(entry.target_canister_id, |st| {
        st.relay_registry_by_target
            .insert(entry.target_canister_id, entry);
        crate::rebuild_relay_targets_by_relay(st);
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_clear_relay_registry() {
    guard_debug_api_not_production();
    state::with_state_mut(|st| {
        st.relay_registry_by_target.clear();
        st.relay_targets_by_relay.clear();
        st.relay_setup_jobs.clear();
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
pub(super) fn debug_set_icp_xdr_rate_fetched_at_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| {
        st.last_icp_xdr_rate_attempt_ts = ts;
        if let Some(snapshot) = st.icp_xdr_rate.as_mut() {
            snapshot.fetched_at_ts = ts.unwrap_or(0);
        }
    });
}
