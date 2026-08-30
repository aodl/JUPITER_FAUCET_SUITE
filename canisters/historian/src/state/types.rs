pub(super) use candid::{CandidType, Principal};
pub(super) use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
pub(super) use icrc_ledger_types::icrc1::account::Account;
pub(super) use jupiter_ic_clients::account::account_text;
pub(crate) use jupiter_ic_clients::cycles_probe::CyclesProbeRoute;
pub(super) use serde::{Deserialize, Serialize};
pub(super) use std::borrow::Cow;
pub(super) use std::collections::{BTreeMap, BTreeSet};

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct Config {
    pub staking_account: Account,
    pub output_source_account: Account,
    pub output_account: Account,
    pub rewards_account: Account,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    #[serde(default)]
    pub cmc_canister_id: Option<Principal>,
    #[serde(default)]
    pub faucet_canister_id: Option<Principal>,
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
    #[serde(default)]
    pub relay_factory_enabled: bool,
    #[serde(default)]
    pub relay_setup_min_e8s: u64,
    #[serde(default)]
    pub relay_initial_cycles: u128,
    #[serde(default)]
    pub relay_cycle_safety_margin_e8s: u64,
    #[serde(default)]
    pub relay_min_subaccount_one_seed_e8s: u64,
    #[serde(default)]
    pub self_service_relay_interval_seconds: u64,
    #[serde(default)]
    pub canonical_relay_canister_id: Option<Principal>,
    #[serde(default)]
    pub canonical_relay_targets: Vec<Principal>,
}

fn opt_principal_text(principal: Option<Principal>) -> String {
    principal
        .map(|p| p.to_text())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn runtime_config_log_line(cfg: &Config) -> String {
    format!(
        "CONFIG staking_account={}, output_source_account={}, output_account={}, rewards_account={}, ledger_canister_id={}, index_canister_id={}, cmc_canister_id={}, faucet_canister_id={}, sns_wasm_canister_id={}, xrc_canister_id={}, enable_sns_tracking={}, scan_interval_seconds={}, cycles_interval_seconds={}, min_tx_e8s={}, max_cycles_entries_per_canister={}, max_commitment_entries_per_canister={}, max_index_pages_per_tick={}, max_canisters_per_cycles_tick={}, relay_factory_enabled={}, relay_setup_min_e8s={}, relay_initial_cycles={}, relay_cycle_safety_margin_e8s={}, relay_min_subaccount_one_seed_e8s={}, self_service_relay_interval_seconds={}, canonical_relay_canister_id={}, canonical_relay_targets={}",
        account_text(&cfg.staking_account),
        account_text(&cfg.output_source_account),
        account_text(&cfg.output_account),
        account_text(&cfg.rewards_account),
        cfg.ledger_canister_id.to_text(),
        cfg.index_canister_id.to_text(),
        opt_principal_text(cfg.cmc_canister_id),
        opt_principal_text(cfg.faucet_canister_id),
        cfg.sns_wasm_canister_id.to_text(),
        cfg.xrc_canister_id.to_text(),
        cfg.enable_sns_tracking,
        cfg.scan_interval_seconds,
        cfg.cycles_interval_seconds,
        cfg.min_tx_e8s,
        cfg.max_cycles_entries_per_canister,
        cfg.max_commitment_entries_per_canister,
        cfg.max_index_pages_per_tick,
        cfg.max_canisters_per_cycles_tick,
        cfg.relay_factory_enabled,
        cfg.relay_setup_min_e8s,
        cfg.relay_initial_cycles,
        cfg.relay_cycle_safety_margin_e8s,
        cfg.relay_min_subaccount_one_seed_e8s,
        cfg.self_service_relay_interval_seconds,
        opt_principal_text(cfg.canonical_relay_canister_id),
        cfg.canonical_relay_targets
            .iter()
            .map(|p| p.to_text())
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanisterTrackingReason {
    MemoCommitment,
    SnsDiscovery,
    RelayTarget,
    RelayInstance,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum CyclesSampleSource {
    SelfCanister,
    DirectCanisterStatus,
    BlackholeStatus,
    SnsRootStatus,
    SnsSwapStatus,
    SnsRootSummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum CyclesProbeResult {
    Ok(CyclesSampleSource),
    NotAvailable,
    Error(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CommitmentSample {
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct InvalidCommitment {
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub memo_text: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentCommitment {
    pub canister_id: Principal,
    #[serde(default)]
    pub raw_icp_memo_text: Option<String>,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentNeuronCommitment {
    pub neuron_id: u64,
    #[serde(default)]
    pub memo_text: Option<String>,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentBurn {
    pub canister_id: Principal,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CommitmentIndexFault {
    pub observed_at_ts: u64,
    pub last_cursor_tx_id: Option<u64>,
    pub offending_tx_id: u64,
    pub message: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct IcpXdrRateSnapshot {
    pub rate: u64,
    pub decimals: u32,
    pub timestamp: u64,
    pub fetched_at_ts: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CyclesSample {
    pub timestamp_nanos: u64,
    pub cycles: u128,
    pub source: CyclesSampleSource,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CanisterMeta {
    pub first_seen_ts: Option<u64>,
    pub last_commitment_ts: Option<u64>,
    pub last_cycles_probe_ts: Option<u64>,
    pub last_cycles_probe_result: Option<CyclesProbeResult>,
    #[serde(default)]
    pub last_burn_tx_id: Option<u64>,
    #[serde(default)]
    pub last_burn_scan_tx_id: Option<u64>,
    #[serde(default)]
    pub burned_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveSnsDiscovery {
    pub started_at_ts_nanos: u64,
    pub root_canister_ids: Vec<Principal>,
    pub next_index: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveCyclesSweep {
    pub started_at_ts_nanos: u64,
    pub canisters: Vec<Principal>,
    pub next_index: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndexedRouteKind {
    Output,
    Rewards,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveRouteSweep {
    pub started_at_ts_nanos: u64,
    pub next_index: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveCommitmentRouteRollupBackfill {
    pub boundary_tx_id: u64,
    pub cursor_tx_id: Option<u64>,
    pub descending: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct StableConfig {
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

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StableCanisterMeta {
    pub first_seen_ts: Option<u64>,
    pub last_commitment_ts: Option<u64>,
    pub last_cycles_probe_ts: Option<u64>,
    pub last_cycles_probe_result: Option<CyclesProbeResult>,
    #[serde(default)]
    pub last_burn_tx_id: Option<u64>,
    #[serde(default)]
    pub last_burn_scan_tx_id: Option<u64>,
    #[serde(default)]
    pub burned_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct StableRootState {
    pub config: StableConfig,
    pub last_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub staking_index_descending: Option<bool>,
    #[serde(default)]
    pub staking_backfill_complete: Option<bool>,
    #[serde(default)]
    pub commitment_route_rollups_complete_from_genesis: Option<bool>,
    #[serde(default)]
    pub active_commitment_route_rollup_backfill: Option<ActiveCommitmentRouteRollupBackfill>,
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PrincipalKey(Vec<u8>);

impl From<&Principal> for PrincipalKey {
    fn from(value: &Principal) -> Self {
        Self(value.as_slice().to_vec())
    }
}

impl From<Principal> for PrincipalKey {
    fn from(value: Principal) -> Self {
        Self(value.as_slice().to_vec())
    }
}

impl PrincipalKey {
    pub(super) fn to_principal(&self) -> Principal {
        Principal::from_slice(&self.0)
    }
}

impl Storable for PrincipalKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.clone())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 29,
        is_fixed_size: false,
    };
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CommitmentRouteKey {
    CyclesTopUp {
        canister: PrincipalKey,
    },
    RawIcp {
        destination_canister: PrincipalKey,
        memo: Vec<u8>,
    },
    NeuronStake {
        neuron_id: u64,
        memo: Option<Vec<u8>>,
    },
}

impl CommitmentRouteKey {
    pub(crate) fn from_public(route: &crate::CommitmentRoute) -> Option<Self> {
        match route {
            crate::CommitmentRoute::CyclesTopUp { canister_id } => {
                valid_route_principal(canister_id).then(|| Self::CyclesTopUp {
                    canister: PrincipalKey::from(canister_id),
                })
            }
            crate::CommitmentRoute::RawIcp {
                destination_canister_id,
                memo,
            } => (valid_route_principal(destination_canister_id)
                && memo.len() <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES)
                .then(|| Self::RawIcp {
                    destination_canister: PrincipalKey::from(destination_canister_id),
                    memo: memo.clone(),
                }),
            crate::CommitmentRoute::NeuronStake { neuron_id, memo } => (*neuron_id != 0
                && memo.as_ref().map(Vec::len).unwrap_or(0)
                    <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES)
                .then(|| Self::NeuronStake {
                    neuron_id: *neuron_id,
                    memo: memo.clone(),
                }),
        }
    }

    pub(crate) fn from_indexed(target: &crate::logic::IndexedCommitmentTarget) -> Option<Self> {
        match target {
            crate::logic::IndexedCommitmentTarget::CyclesTopUp { canister_id } => {
                valid_route_principal(canister_id).then(|| Self::CyclesTopUp {
                    canister: PrincipalKey::from(canister_id),
                })
            }
            crate::logic::IndexedCommitmentTarget::RawIcp {
                canister_id,
                memo_text,
            } => (valid_route_principal(canister_id)
                && memo_text.len() <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES)
                .then(|| Self::RawIcp {
                    destination_canister: PrincipalKey::from(canister_id),
                    memo: memo_text.as_bytes().to_vec(),
                }),
            crate::logic::IndexedCommitmentTarget::NeuronStake {
                neuron_id,
                memo_text,
            } => (*neuron_id != 0
                && memo_text.as_ref().map(String::len).unwrap_or(0)
                    <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES)
                .then(|| Self::NeuronStake {
                    neuron_id: *neuron_id,
                    memo: memo_text.as_ref().map(|memo| memo.as_bytes().to_vec()),
                }),
        }
    }
}

fn valid_route_principal(principal: &Principal) -> bool {
    *principal != Principal::anonymous()
        && *principal != Principal::management_canister()
        && principal.as_slice().len() <= 29
}

impl Storable for CommitmentRouteKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.clone().into_bytes())
    }

    fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::CyclesTopUp { canister } => {
                assert!(
                    canister.0.len() <= 29,
                    "historian route principal exceeds 29 bytes"
                );
                let mut out = Vec::with_capacity(2 + canister.0.len());
                out.push(0);
                out.push(canister.0.len() as u8);
                out.extend_from_slice(&canister.0);
                out
            }
            Self::RawIcp {
                destination_canister,
                memo,
            } => {
                assert!(
                    destination_canister.0.len() <= 29,
                    "historian route principal exceeds 29 bytes"
                );
                assert!(
                    memo.len() <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES,
                    "historian route memo exceeds policy bound"
                );
                let mut out = Vec::with_capacity(3 + destination_canister.0.len() + memo.len());
                out.push(1);
                out.push(destination_canister.0.len() as u8);
                out.extend_from_slice(&destination_canister.0);
                out.push(memo.len() as u8);
                out.extend_from_slice(&memo);
                out
            }
            Self::NeuronStake { neuron_id, memo } => {
                assert!(neuron_id != 0, "historian route neuron id must be non-zero");
                let mut out = Vec::with_capacity(11 + memo.as_ref().map(Vec::len).unwrap_or(0));
                out.push(2);
                out.extend_from_slice(&neuron_id.to_be_bytes());
                match memo {
                    None => out.push(0),
                    Some(memo) => {
                        assert!(
                            memo.len() <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES,
                            "historian route memo exceeds policy bound"
                        );
                        out.push(1);
                        out.push(memo.len() as u8);
                        out.extend_from_slice(&memo);
                    }
                }
                out
            }
        }
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let tag = *bytes
            .first()
            .expect("missing historian commitment route tag");
        match tag {
            0 | 1 => {
                let principal_len = *bytes
                    .get(1)
                    .expect("missing historian route principal length")
                    as usize;
                assert!(
                    principal_len <= 29,
                    "invalid historian route principal length"
                );
                let principal_end = 2 + principal_len;
                assert!(
                    bytes.len() >= principal_end,
                    "truncated historian route principal"
                );
                let principal = PrincipalKey(bytes[2..principal_end].to_vec());
                assert!(
                    valid_route_principal(&principal.to_principal()),
                    "invalid historian route principal"
                );
                if tag == 0 {
                    assert_eq!(
                        bytes.len(),
                        principal_end,
                        "trailing bytes in historian cycles route key"
                    );
                    Self::CyclesTopUp {
                        canister: principal,
                    }
                } else {
                    let memo_len = *bytes
                        .get(principal_end)
                        .expect("missing historian raw route memo length")
                        as usize;
                    assert!(
                        memo_len <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES,
                        "invalid historian raw route memo length"
                    );
                    let memo_start = principal_end + 1;
                    assert_eq!(
                        bytes.len(),
                        memo_start + memo_len,
                        "invalid historian raw route key length"
                    );
                    Self::RawIcp {
                        destination_canister: principal,
                        memo: bytes[memo_start..].to_vec(),
                    }
                }
            }
            2 => {
                assert!(bytes.len() >= 10, "truncated historian neuron route key");
                let mut neuron_id = [0; 8];
                neuron_id.copy_from_slice(&bytes[1..9]);
                let neuron_id = u64::from_be_bytes(neuron_id);
                assert!(neuron_id != 0, "invalid historian route neuron id");
                let memo = match bytes[9] {
                    0 => {
                        assert_eq!(
                            bytes.len(),
                            10,
                            "trailing bytes in historian neuron route key"
                        );
                        None
                    }
                    1 => {
                        let memo_len = *bytes
                            .get(10)
                            .expect("missing historian neuron route memo length")
                            as usize;
                        assert!(
                            memo_len <= jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES,
                            "invalid historian neuron route memo length"
                        );
                        assert_eq!(
                            bytes.len(),
                            11 + memo_len,
                            "invalid historian neuron route key length"
                        );
                        Some(bytes[11..].to_vec())
                    }
                    _ => panic!("invalid historian neuron route memo presence tag"),
                };
                Self::NeuronStake { neuron_id, memo }
            }
            _ => panic!("unknown historian commitment route tag"),
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 64,
        is_fixed_size: false,
    };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CommitmentRouteRollup {
    pub qualifying_commitment_count: u64,
    pub total_qualifying_committed_e8s: u64,
}

impl Storable for CommitmentRouteRollup {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&self.qualifying_commitment_count.to_be_bytes());
        out.extend_from_slice(&self.total_qualifying_committed_e8s.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            16,
            "invalid historian commitment route rollup length"
        );
        let mut count = [0; 8];
        count.copy_from_slice(&bytes[..8]);
        let mut total = [0; 8];
        total.copy_from_slice(&bytes[8..]);
        Self {
            qualifying_commitment_count: u64::from_be_bytes(count),
            total_qualifying_committed_e8s: u64::from_be_bytes(total),
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub(super) struct StableTrackingReasonSet(pub BTreeSet<CanisterTrackingReason>);

impl Storable for StableTrackingReasonSet {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(
            candid::encode_one(self)
                .expect("failed to encode historian stable tracking reason set"),
        )
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode historian stable tracking reason set")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref())
            .expect("failed to decode historian stable tracking reason set")
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub(super) struct StableU64List(pub Vec<u64>);

impl Storable for StableU64List {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian stable u64 list"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode historian stable u64 list")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian stable u64 list")
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CommitmentEntryKey {
    canister: PrincipalKey,
    tx_id: u64,
}

impl CommitmentEntryKey {
    pub(super) fn new(canister: impl Into<PrincipalKey>, tx_id: u64) -> Self {
        Self {
            canister: canister.into(),
            tx_id,
        }
    }
}

impl Storable for CommitmentEntryKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        out
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let len = bytes.first().copied().unwrap_or(0) as usize;
        assert!(
            bytes.len() == 1 + len + 8,
            "invalid historian commitment entry key length"
        );
        let canister = PrincipalKey(bytes[1..1 + len].to_vec());
        let mut tx_id = [0u8; 8];
        tx_id.copy_from_slice(&bytes[1 + len..]);
        Self {
            canister,
            tx_id: u64::from_be_bytes(tx_id),
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 38,
        is_fixed_size: false,
    };
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NeuronCommitmentEntryKey {
    neuron_id: u64,
    tx_id: u64,
}

impl NeuronCommitmentEntryKey {
    pub(super) fn new(neuron_id: u64, tx_id: u64) -> Self {
        Self { neuron_id, tx_id }
    }
}

impl Storable for NeuronCommitmentEntryKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&self.neuron_id.to_be_bytes());
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&self.neuron_id.to_be_bytes());
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        out
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        assert!(
            bytes.len() == 16,
            "invalid historian neuron commitment entry key length"
        );
        let mut neuron_id = [0u8; 8];
        neuron_id.copy_from_slice(&bytes[..8]);
        let mut tx_id = [0u8; 8];
        tx_id.copy_from_slice(&bytes[8..]);
        Self {
            neuron_id: u64::from_be_bytes(neuron_id),
            tx_id: u64::from_be_bytes(tx_id),
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CyclesEntryKey {
    canister: PrincipalKey,
    timestamp_nanos: u64,
}

impl CyclesEntryKey {
    pub(super) fn new(canister: impl Into<PrincipalKey>, timestamp_nanos: u64) -> Self {
        Self {
            canister: canister.into(),
            timestamp_nanos,
        }
    }
}

impl Storable for CyclesEntryKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.timestamp_nanos.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.timestamp_nanos.to_be_bytes());
        out
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let len = bytes.first().copied().unwrap_or(0) as usize;
        assert!(
            bytes.len() == 1 + len + 8,
            "invalid historian cycles entry key length"
        );
        let canister = PrincipalKey(bytes[1..1 + len].to_vec());
        let mut timestamp_nanos = [0u8; 8];
        timestamp_nanos.copy_from_slice(&bytes[1 + len..]);
        Self {
            canister,
            timestamp_nanos: u64::from_be_bytes(timestamp_nanos),
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 38,
        is_fixed_size: false,
    };
}

impl Storable for CommitmentSample {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian commitment sample"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode historian commitment sample")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian commitment sample")
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for CyclesSample {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian cycles sample"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode historian cycles sample")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian cycles sample")
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for StableCanisterMeta {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(
            candid::encode_one(self).expect("failed to encode historian stable canister meta"),
        )
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode historian stable canister meta")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian stable canister meta")
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct State {
    pub config: Config,
    pub distinct_canisters: BTreeSet<Principal>,
    pub canister_tracking_reasons: BTreeMap<Principal, BTreeSet<CanisterTrackingReason>>,
    pub commitment_history: BTreeMap<Principal, Vec<CommitmentSample>>,
    pub cycles_history: BTreeMap<Principal, Vec<CyclesSample>>,
    pub per_canister_meta: BTreeMap<Principal, CanisterMeta>,
    #[serde(default)]
    pub cached_cycles_probe_routes: BTreeMap<Principal, CyclesProbeRoute>,
    #[serde(default)]
    pub memo_registered_canister_summaries_cache:
        Option<BTreeMap<Principal, crate::MemoRegisteredCanisterSummary>>,
    #[serde(default)]
    pub memo_registered_canister_summaries_total_desc_index: Option<Vec<Principal>>,
    pub last_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub staking_index_descending: Option<bool>,
    #[serde(default)]
    pub staking_backfill_complete: Option<bool>,
    #[serde(default)]
    pub commitment_route_rollups_complete_from_genesis: Option<bool>,
    #[serde(default)]
    pub active_commitment_route_rollup_backfill: Option<ActiveCommitmentRouteRollupBackfill>,
    pub last_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    pub output_route_index_descending: Option<bool>,
    #[serde(default)]
    pub output_route_backfill_complete: Option<bool>,
    pub last_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    pub rewards_route_index_descending: Option<bool>,
    #[serde(default)]
    pub rewards_route_backfill_complete: Option<bool>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
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
    pub qualifying_commitment_count: Option<u64>,
    #[serde(default)]
    pub raw_icp_commitment_history: BTreeMap<Principal, Vec<CommitmentSample>>,
    #[serde(default)]
    pub neuron_commitment_history: BTreeMap<u64, Vec<CommitmentSample>>,
    pub total_output_e8s: Option<u64>,
    pub total_rewards_e8s: Option<u64>,
    pub icp_burned_e8s: Option<u64>,
    pub recent_commitments: Option<Vec<RecentCommitment>>,
    pub recent_under_threshold_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    pub recent_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    pub recent_under_threshold_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    pub recent_invalid_commitments: Option<Vec<InvalidCommitment>>,
    pub recent_burns: Option<Vec<RecentBurn>>,
    pub last_index_run_ts: Option<u64>,
    pub commitment_index_fault: Option<CommitmentIndexFault>,
    pub icp_xdr_rate: Option<IcpXdrRateSnapshot>,
    pub last_icp_xdr_rate_attempt_ts: Option<u64>,
    pub last_icp_xdr_rate_error: Option<String>,
    #[serde(default)]
    pub canister_module_hash_cache: Vec<crate::CanisterModuleHash>,
    #[serde(default)]
    pub canister_module_hash_cache_updated_ts: Option<u64>,
    #[serde(default)]
    pub canister_module_hash_refresh_lock_ts: Option<u64>,
}

impl State {
    pub(crate) fn new(config: Config, now_secs: u64) -> Self {
        Self {
            config,
            distinct_canisters: BTreeSet::new(),
            canister_tracking_reasons: BTreeMap::new(),
            commitment_history: BTreeMap::new(),
            cycles_history: BTreeMap::new(),
            per_canister_meta: BTreeMap::new(),
            cached_cycles_probe_routes: BTreeMap::new(),
            memo_registered_canister_summaries_cache: Some(BTreeMap::new()),
            memo_registered_canister_summaries_total_desc_index: Some(Vec::new()),
            last_indexed_staking_tx_id: None,
            oldest_indexed_staking_tx_id: None,
            staking_index_descending: None,
            staking_backfill_complete: Some(false),
            commitment_route_rollups_complete_from_genesis: Some(false),
            active_commitment_route_rollup_backfill: None,
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
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
            qualifying_commitment_count: Some(0),
            raw_icp_commitment_history: BTreeMap::new(),
            neuron_commitment_history: BTreeMap::new(),
            total_output_e8s: Some(0),
            total_rewards_e8s: Some(0),
            icp_burned_e8s: Some(0),
            recent_commitments: Some(Vec::new()),
            recent_under_threshold_commitments: Some(Vec::new()),
            recent_neuron_commitments: Some(Vec::new()),
            recent_under_threshold_neuron_commitments: Some(Vec::new()),
            recent_invalid_commitments: Some(Vec::new()),
            recent_burns: Some(Vec::new()),
            last_index_run_ts: Some(0),
            commitment_index_fault: None,
            icp_xdr_rate: None,
            last_icp_xdr_rate_attempt_ts: None,
            last_icp_xdr_rate_error: None,
            canister_module_hash_cache: Vec::new(),
            canister_module_hash_cache_updated_ts: None,
            canister_module_hash_refresh_lock_ts: None,
        }
    }
}

// Stable-state enum shape is part of the upgrade contract; boxing Current would change Candid.
#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum VersionedStableState {
    Uninitialized,
    Current(StableRootState),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian root stable state"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode historian root stable state")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let raw = bytes.as_ref();
        candid::decode_one(raw).unwrap_or_else(|current_err| {
            crate::state::legacy_v1::decode_legacy_root(raw).unwrap_or_else(|legacy_err| {
                panic!(
                    "failed to decode historian root stable state as current schema ({current_err}) or legacy V1 schema from {} ({legacy_err})",
                    crate::state::legacy_v1::LEGACY_HISTORIAN_V1_REVISION
                )
            })
        })
    }

    const BOUND: Bound = Bound::Unbounded;
}
