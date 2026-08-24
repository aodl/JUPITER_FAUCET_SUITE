#![allow(dead_code)]

use super::*;

pub(crate) const LEGACY_HISTORIAN_V1_REVISION: &str = "98c871a85af91320a5dfc59b5b040727e21aa094";

// Frozen stable-schema subset copied from the Historian at
// LEGACY_HISTORIAN_V1_REVISION. These types are private compatibility decoders
// for stable memory and tests; they are not public API or runtime source of truth.

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LegacyCanisterSourceV1 {
    MemoCommitment,
    SnsDiscovery,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyCyclesSampleSourceV1 {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyCyclesProbeResultV1 {
    Ok(LegacyCyclesSampleSourceV1),
    NotAvailable,
    Error(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyCyclesSampleV1 {
    pub timestamp_nanos: u64,
    pub cycles: u128,
    pub source: LegacyCyclesSampleSourceV1,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct LegacyStableSourceSetV1(pub BTreeSet<LegacyCanisterSourceV1>);

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LegacyStableCanisterMetaV1 {
    pub first_seen_ts: Option<u64>,
    pub last_commitment_ts: Option<u64>,
    pub last_cycles_probe_ts: Option<u64>,
    pub last_cycles_probe_result: Option<LegacyCyclesProbeResultV1>,
    #[serde(default)]
    pub last_burn_tx_id: Option<u64>,
    #[serde(default)]
    pub last_burn_scan_tx_id: Option<u64>,
    #[serde(default)]
    pub burned_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct LegacyStableConfigV1 {
    pub staking_account: Account,
    #[serde(default)]
    pub output_source_account: Option<Account>,
    #[serde(default)]
    pub output_account: Option<Account>,
    #[serde(default)]
    pub rewards_account: Option<Account>,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    #[serde(default)]
    pub cmc_canister_id: Option<Principal>,
    #[serde(default)]
    pub faucet_canister_id: Option<Principal>,
    pub blackhole_canister_id: Principal,
    pub sns_wasm_canister_id: Principal,
    #[serde(default)]
    pub xrc_canister_id: Option<Principal>,
    pub enable_sns_tracking: bool,
    pub scan_interval_seconds: u64,
    pub cycles_interval_seconds: u64,
    pub min_tx_e8s: u64,
    pub max_cycles_entries_per_canister: u32,
    pub max_commitment_entries_per_canister: u32,
    pub max_index_pages_per_tick: u32,
    pub max_canisters_per_cycles_tick: u32,
    #[serde(default)]
    pub relay_factory_enabled: Option<bool>,
    #[serde(default)]
    pub relay_setup_min_e8s: Option<u64>,
    #[serde(default)]
    pub relay_initial_cycles: Option<u128>,
    #[serde(default)]
    pub relay_cycle_safety_margin_e8s: Option<u64>,
    #[serde(default)]
    pub relay_min_subaccount_one_seed_e8s: Option<u64>,
    #[serde(default)]
    pub self_service_relay_interval_seconds: Option<u64>,
    #[serde(default)]
    pub canonical_relay_canister_id: Option<Option<Principal>>,
    #[serde(default)]
    pub canonical_relay_targets: Option<Vec<Principal>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct LegacyStableRootStateV1 {
    pub config: LegacyStableConfigV1,
    pub last_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub staking_index_descending: Option<bool>,
    #[serde(default)]
    pub staking_backfill_complete: Option<bool>,
    #[serde(default)]
    pub last_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    pub output_route_index_descending: Option<bool>,
    #[serde(default)]
    pub output_route_backfill_complete: Option<bool>,
    #[serde(default)]
    pub last_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    pub rewards_route_index_descending: Option<bool>,
    #[serde(default)]
    pub rewards_route_backfill_complete: Option<bool>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    #[serde(default)]
    pub last_completed_route_sweep_ts: Option<u64>,
    pub active_cycles_sweep: Option<ActiveCyclesSweep>,
    #[serde(default)]
    pub initial_cycles_probe_queue: Vec<Principal>,
    #[serde(default)]
    pub active_route_sweep: Option<ActiveRouteSweep>,
    #[serde(default)]
    pub active_sns_discovery: Option<ActiveSnsDiscovery>,
    pub main_lock_state_ts: Option<u64>,
    pub last_main_run_ts: u64,
    #[serde(default)]
    pub qualifying_commitment_count: Option<u64>,
    #[serde(default)]
    pub total_output_e8s: Option<u64>,
    #[serde(default)]
    pub total_rewards_e8s: Option<u64>,
    #[serde(default)]
    pub icp_burned_e8s: Option<u64>,
    #[serde(default)]
    pub recent_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    pub recent_under_threshold_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    pub recent_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    pub recent_under_threshold_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    pub recent_invalid_commitments: Option<Vec<InvalidCommitment>>,
    #[serde(default)]
    pub recent_burns: Option<Vec<RecentBurn>>,
    #[serde(default)]
    pub last_index_run_ts: Option<u64>,
    #[serde(default)]
    pub commitment_index_fault: Option<CommitmentIndexFault>,
    #[serde(default)]
    pub icp_xdr_rate: Option<IcpXdrRateSnapshot>,
    #[serde(default)]
    pub last_icp_xdr_rate_attempt_ts: Option<u64>,
    #[serde(default)]
    pub last_icp_xdr_rate_error: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum LegacyVersionedStableStateV1 {
    Uninitialized,
    Current(LegacyStableRootStateV1),
}

impl From<LegacyCanisterSourceV1> for CanisterTrackingReason {
    fn from(value: LegacyCanisterSourceV1) -> Self {
        match value {
            LegacyCanisterSourceV1::MemoCommitment => Self::MemoCommitment,
            LegacyCanisterSourceV1::SnsDiscovery => Self::SnsDiscovery,
        }
    }
}

impl From<LegacyCyclesSampleSourceV1> for CyclesSampleSource {
    fn from(value: LegacyCyclesSampleSourceV1) -> Self {
        match value {
            LegacyCyclesSampleSourceV1::BlackholeStatus => Self::BlackholeStatus,
            LegacyCyclesSampleSourceV1::SelfCanister => Self::SelfCanister,
            LegacyCyclesSampleSourceV1::SnsRootSummary => Self::SnsRootSummary,
        }
    }
}

impl From<LegacyCyclesProbeResultV1> for CyclesProbeResult {
    fn from(value: LegacyCyclesProbeResultV1) -> Self {
        match value {
            LegacyCyclesProbeResultV1::Ok(source) => Self::Ok(source.into()),
            LegacyCyclesProbeResultV1::NotAvailable => Self::NotAvailable,
            LegacyCyclesProbeResultV1::Error(message) => Self::Error(message),
        }
    }
}

impl From<LegacyCyclesSampleV1> for CyclesSample {
    fn from(value: LegacyCyclesSampleV1) -> Self {
        Self {
            timestamp_nanos: value.timestamp_nanos,
            cycles: value.cycles,
            source: value.source.into(),
        }
    }
}

impl From<LegacyStableCanisterMetaV1> for StableCanisterMeta {
    fn from(value: LegacyStableCanisterMetaV1) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_commitment_ts: value.last_commitment_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result.map(Into::into),
            last_burn_tx_id: value.last_burn_tx_id,
            last_burn_scan_tx_id: value.last_burn_scan_tx_id,
            burned_e8s: value.burned_e8s,
        }
    }
}

impl From<LegacyStableConfigV1> for StableConfig {
    fn from(value: LegacyStableConfigV1) -> Self {
        Self {
            staking_account: value.staking_account,
            output_source_account: value.output_source_account,
            output_account: value.output_account,
            rewards_account: value.rewards_account,
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: value.cmc_canister_id,
            faucet_canister_id: value.faucet_canister_id,
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            xrc_canister_id: value.xrc_canister_id,
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_commitment_entries_per_canister: value.max_commitment_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
            relay_factory_enabled: value.relay_factory_enabled,
            relay_setup_min_e8s: value.relay_setup_min_e8s,
            relay_initial_cycles: value.relay_initial_cycles,
            relay_cycle_safety_margin_e8s: value.relay_cycle_safety_margin_e8s,
            relay_min_subaccount_one_seed_e8s: value.relay_min_subaccount_one_seed_e8s,
            self_service_relay_interval_seconds: value.self_service_relay_interval_seconds,
            canonical_relay_canister_id: value.canonical_relay_canister_id,
            canonical_relay_targets: value.canonical_relay_targets,
        }
    }
}

impl From<LegacyStableRootStateV1> for StableRootState {
    fn from(value: LegacyStableRootStateV1) -> Self {
        Self {
            config: value.config.into(),
            last_indexed_staking_tx_id: value.last_indexed_staking_tx_id,
            oldest_indexed_staking_tx_id: value.oldest_indexed_staking_tx_id,
            staking_index_descending: value.staking_index_descending,
            staking_backfill_complete: value.staking_backfill_complete,
            last_indexed_output_tx_id: value.last_indexed_output_tx_id,
            oldest_indexed_output_tx_id: value.oldest_indexed_output_tx_id,
            output_route_index_descending: value.output_route_index_descending,
            output_route_backfill_complete: value.output_route_backfill_complete,
            last_indexed_rewards_tx_id: value.last_indexed_rewards_tx_id,
            oldest_indexed_rewards_tx_id: value.oldest_indexed_rewards_tx_id,
            rewards_route_index_descending: value.rewards_route_index_descending,
            rewards_route_backfill_complete: value.rewards_route_backfill_complete,
            last_sns_discovery_ts: value.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: value.last_completed_cycles_sweep_ts,
            last_completed_route_sweep_ts: value.last_completed_route_sweep_ts,
            active_cycles_sweep: value.active_cycles_sweep,
            initial_cycles_probe_queue: value.initial_cycles_probe_queue,
            active_route_sweep: value.active_route_sweep,
            active_sns_discovery: value.active_sns_discovery,
            main_lock_state_ts: value.main_lock_state_ts,
            last_main_run_ts: value.last_main_run_ts,
            qualifying_commitment_count: value.qualifying_commitment_count,
            total_output_e8s: value.total_output_e8s,
            total_rewards_e8s: value.total_rewards_e8s,
            icp_burned_e8s: value.icp_burned_e8s,
            recent_commitments: value.recent_commitments,
            recent_under_threshold_commitments: value.recent_under_threshold_commitments,
            recent_neuron_commitments: value.recent_neuron_commitments,
            recent_under_threshold_neuron_commitments: value
                .recent_under_threshold_neuron_commitments,
            recent_invalid_commitments: value.recent_invalid_commitments,
            recent_burns: value.recent_burns,
            last_index_run_ts: value.last_index_run_ts,
            commitment_index_fault: value.commitment_index_fault,
            icp_xdr_rate: value.icp_xdr_rate,
            last_icp_xdr_rate_attempt_ts: value.last_icp_xdr_rate_attempt_ts,
            last_icp_xdr_rate_error: value.last_icp_xdr_rate_error,
        }
    }
}

impl From<LegacyVersionedStableStateV1> for VersionedStableState {
    fn from(value: LegacyVersionedStableStateV1) -> Self {
        match value {
            LegacyVersionedStableStateV1::Uninitialized => Self::Uninitialized,
            LegacyVersionedStableStateV1::Current(root) => Self::Current(root.into()),
        }
    }
}

pub(crate) fn decode_legacy_root(bytes: &[u8]) -> Result<VersionedStableState, candid::Error> {
    candid::decode_one::<LegacyVersionedStableStateV1>(bytes).map(Into::into)
}
