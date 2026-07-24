use candid::{CandidType, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeSet;

const LEGACY_HISTORIAN_V1_REVISION: &str = "98c871a85af91320a5dfc59b5b040727e21aa094";

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
}

fn memory(id: u8) -> Memory {
    MEMORY_MANAGER.with(|manager| manager.borrow().get(MemoryId::new(id)))
}

#[derive(CandidType, Deserialize)]
struct LegacyFixtureInit {
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    cmc_canister_id: Principal,
    sns_wasm_canister_id: Principal,
    xrc_canister_id: Principal,
    memo_target: Principal,
    sns_target: Principal,
    canonical_target: Principal,
    canonical_relay: Principal,
    self_service_target: Principal,
    self_service_relay: Principal,
    normal_setup_target: Principal,
    refundable_setup_target: Principal,
    unsafe_setup_target: Principal,
}

#[derive(CandidType, Serialize)]
struct LegacySeedSummary {
    revision: String,
    memo_target: Principal,
    sns_target: Principal,
    canonical_target: Principal,
    canonical_relay: Principal,
    self_service_target: Principal,
    self_service_relay: Principal,
    normal_setup_target: Principal,
    refundable_setup_target: Principal,
    unsafe_setup_target: Principal,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PrincipalKey(Vec<u8>);

impl From<Principal> for PrincipalKey {
    fn from(value: Principal) -> Self {
        Self(value.as_slice().to_vec())
    }
}

impl Storable for PrincipalKey {
    const BOUND: Bound = Bound::Bounded {
        max_size: 29,
        is_fixed_size: false,
    };

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.clone())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CommitmentEntryKey {
    canister: PrincipalKey,
    tx_id: u64,
}

impl CommitmentEntryKey {
    fn new(canister: Principal, tx_id: u64) -> Self {
        Self {
            canister: canister.into(),
            tx_id,
        }
    }
}

impl Storable for CommitmentEntryKey {
    const BOUND: Bound = Bound::Bounded {
        max_size: 38,
        is_fixed_size: false,
    };

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let len = bytes[0] as usize;
        let mut tx_id = [0; 8];
        tx_id.copy_from_slice(&bytes[1 + len..]);
        Self {
            canister: PrincipalKey(bytes[1..1 + len].to_vec()),
            tx_id: u64::from_be_bytes(tx_id),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CyclesEntryKey {
    canister: PrincipalKey,
    timestamp_nanos: u64,
}

impl CyclesEntryKey {
    fn new(canister: Principal, timestamp_nanos: u64) -> Self {
        Self {
            canister: canister.into(),
            timestamp_nanos,
        }
    }
}

impl Storable for CyclesEntryKey {
    const BOUND: Bound = Bound::Bounded {
        max_size: 38,
        is_fixed_size: false,
    };

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.timestamp_nanos.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let len = bytes[0] as usize;
        let mut timestamp = [0; 8];
        timestamp.copy_from_slice(&bytes[1 + len..]);
        Self {
            canister: PrincipalKey(bytes[1..1 + len].to_vec()),
            timestamp_nanos: u64::from_be_bytes(timestamp),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NeuronCommitmentEntryKey {
    neuron_id: u64,
    tx_id: u64,
}

impl Storable for NeuronCommitmentEntryKey {
    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&self.neuron_id.to_be_bytes());
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        Cow::Owned(out)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let mut neuron_id = [0; 8];
        let mut tx_id = [0; 8];
        neuron_id.copy_from_slice(&bytes[..8]);
        tx_id.copy_from_slice(&bytes[8..]);
        Self {
            neuron_id: u64::from_be_bytes(neuron_id),
            tx_id: u64::from_be_bytes(tx_id),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CanisterSource {
    MemoCommitment,
    SnsDiscovery,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
struct StableSourceSet(BTreeSet<CanisterSource>);

impl Storable for StableSourceSet {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
struct StableU64List(Vec<u64>);

impl Storable for StableU64List {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum CyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum CyclesProbeResult {
    Ok(CyclesSampleSource),
    NotAvailable,
    Error(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct CommitmentSample {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

impl Storable for CommitmentSample {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct CyclesSample {
    timestamp_nanos: u64,
    cycles: u128,
    source: CyclesSampleSource,
}

impl Storable for CyclesSample {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
struct StableCanisterMeta {
    first_seen_ts: Option<u64>,
    last_commitment_ts: Option<u64>,
    last_cycles_probe_ts: Option<u64>,
    last_cycles_probe_result: Option<CyclesProbeResult>,
    #[serde(default)]
    last_burn_tx_id: Option<u64>,
    #[serde(default)]
    last_burn_scan_tx_id: Option<u64>,
    #[serde(default)]
    burned_e8s: Option<u64>,
}

impl Storable for StableCanisterMeta {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum RelayRegistryKind {
    Canonical,
    SelfService,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum RelayRegistryStatus {
    Pending,
    Active,
    Failed,
    Superseded,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RelayRegistryEntry {
    relay_canister_id: Principal,
    target_canister_id: Principal,
    kind: RelayRegistryKind,
    status: RelayRegistryStatus,
    setup_account: Option<Account>,
    setup_account_identifier: Option<String>,
    setup_amount_e8s: Option<u64>,
    setup_tx_ids: Vec<u64>,
    relay_wasm_hash_hex: Option<String>,
    final_controllers: Option<Vec<Principal>>,
    log_visibility_public: Option<bool>,
    created_at_ts: Option<u64>,
    activated_at_ts: Option<u64>,
}

impl Storable for RelayRegistryEntry {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum RelaySetupStatus {
    NotFunded,
    Pending,
    CycleTransferAccepted,
    CanisterCreated,
    Active,
    Refunded,
    FailedRetryable,
    Ambiguous,
    ManualRecoveryRequired,
    TargetNotObservable,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RelaySetupPayment {
    target_canister_id: Principal,
    tx_id: u64,
    from_account_identifier: String,
    amount_e8s: u64,
    timestamp_nanos: Option<u64>,
    processed: bool,
    refunded: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum RelaySetupTransferKind {
    CmcConversion,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum RelaySetupPhase {
    PreSpend,
    RelayCanisterCreated,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RelaySetupTransferRecord {
    kind: RelaySetupTransferKind,
    from_subaccount: Option<[u8; 32]>,
    from_account_identifier: String,
    to: Account,
    to_account_identifier: String,
    amount_e8s: u64,
    fee_e8s: u64,
    memo: Option<Vec<u8>>,
    created_at_time_nanos: u64,
    block_index: Option<u64>,
    completed: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RelayCreateAttempt {
    target_canister_id: Principal,
    created_at_ts: u64,
    initial_cycles: u128,
    #[serde(default)]
    raw_relay_wasm_hash_hex: Option<String>,
    #[serde(default)]
    install_payload_hash_hex: Option<String>,
    #[serde(default)]
    relay_wasm_hash_hex: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RelaySetupJob {
    target_canister_id: Principal,
    setup_account: Account,
    setup_account_identifier: String,
    status: RelaySetupStatus,
    relay_canister_id: Option<Principal>,
    last_indexed_setup_tx_id: Option<u64>,
    setup_tx_ids: Vec<u64>,
    setup_amount_seen_e8s: u64,
    setup_amount_processed_e8s: u64,
    payments: Vec<RelaySetupPayment>,
    cycle_conversion_e8s: Option<u64>,
    cycle_transfer_block_index: Option<u64>,
    cycles_minted: Option<u128>,
    relay_initial_cycles: Option<u128>,
    relay_funding_e8s: Option<u64>,
    relay_funding_block_index: Option<u64>,
    #[serde(default)]
    phase: Option<RelaySetupPhase>,
    #[serde(default)]
    cycle_transfer: Option<RelaySetupTransferRecord>,
    #[serde(default)]
    relay_funding_transfer: Option<RelaySetupTransferRecord>,
    #[serde(default)]
    existing_relay_sweep_transfer: Option<RelaySetupTransferRecord>,
    #[serde(default)]
    refund_transfers: Vec<RelaySetupTransferRecord>,
    #[serde(default)]
    relay_create_attempt: Option<RelayCreateAttempt>,
    #[serde(default)]
    code_installed: bool,
    #[serde(default)]
    relay_funding_accepted: bool,
    #[serde(default)]
    blackhole_update_attempted: bool,
    #[serde(default)]
    blackhole_confirmed: bool,
    refund_attempt_count: u32,
    last_refund_attempt_ts: Option<u64>,
    refund_blocks: Vec<u64>,
    created_at_ts: u64,
    updated_at_ts: u64,
    last_error: Option<String>,
}

impl Storable for RelaySetupJob {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
struct StableConfig {
    staking_account: Account,
    #[serde(default)]
    output_source_account: Option<Account>,
    #[serde(default)]
    output_account: Option<Account>,
    #[serde(default)]
    rewards_account: Option<Account>,
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    #[serde(default)]
    cmc_canister_id: Option<Principal>,
    #[serde(default)]
    faucet_canister_id: Option<Principal>,
    blackhole_canister_id: Principal,
    sns_wasm_canister_id: Principal,
    #[serde(default)]
    xrc_canister_id: Option<Principal>,
    enable_sns_tracking: bool,
    scan_interval_seconds: u64,
    cycles_interval_seconds: u64,
    min_tx_e8s: u64,
    max_cycles_entries_per_canister: u32,
    max_commitment_entries_per_canister: u32,
    max_index_pages_per_tick: u32,
    max_canisters_per_cycles_tick: u32,
    #[serde(default)]
    relay_factory_enabled: Option<bool>,
    #[serde(default)]
    relay_setup_min_e8s: Option<u64>,
    #[serde(default)]
    relay_setup_dust_e8s: Option<u64>,
    #[serde(default)]
    relay_setup_refund_cooldown_seconds: Option<u64>,
    #[serde(default)]
    relay_initial_cycles: Option<u128>,
    #[serde(default)]
    relay_cycle_safety_margin_e8s: Option<u64>,
    #[serde(default)]
    relay_min_subaccount_one_seed_e8s: Option<u64>,
    #[serde(default)]
    self_service_relay_interval_seconds: Option<u64>,
    #[serde(default)]
    self_service_relay_max_transfers_per_tick: Option<Option<u32>>,
    #[serde(default)]
    io_surplus_neuron_id: Option<u64>,
    #[serde(default)]
    canonical_relay_canister_id: Option<Option<Principal>>,
    #[serde(default)]
    canonical_relay_targets: Option<Vec<Principal>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct ActiveCyclesSweep {
    started_at_ts_nanos: u64,
    canisters: Vec<Principal>,
    next_index: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
struct StableRootState {
    config: StableConfig,
    last_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    oldest_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    staking_index_descending: Option<bool>,
    #[serde(default)]
    staking_backfill_complete: Option<bool>,
    #[serde(default)]
    last_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    oldest_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    output_route_index_descending: Option<bool>,
    #[serde(default)]
    output_route_backfill_complete: Option<bool>,
    #[serde(default)]
    last_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    oldest_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    rewards_route_index_descending: Option<bool>,
    #[serde(default)]
    rewards_route_backfill_complete: Option<bool>,
    last_sns_discovery_ts: u64,
    last_completed_cycles_sweep_ts: u64,
    #[serde(default)]
    last_completed_route_sweep_ts: Option<u64>,
    active_cycles_sweep: Option<ActiveCyclesSweep>,
    #[serde(default)]
    initial_cycles_probe_queue: Vec<Principal>,
    #[serde(default)]
    active_route_sweep: Option<()>,
    #[serde(default)]
    active_sns_discovery: Option<()>,
    main_lock_state_ts: Option<u64>,
    last_main_run_ts: u64,
    #[serde(default)]
    qualifying_commitment_count: Option<u64>,
    #[serde(default)]
    total_output_e8s: Option<u64>,
    #[serde(default)]
    total_rewards_e8s: Option<u64>,
    #[serde(default)]
    icp_burned_e8s: Option<u64>,
    #[serde(default)]
    recent_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    recent_under_threshold_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    recent_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    recent_under_threshold_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    recent_invalid_commitments: Option<Vec<InvalidCommitment>>,
    #[serde(default)]
    recent_burns: Option<Vec<RecentBurn>>,
    #[serde(default)]
    last_index_run_ts: Option<u64>,
    #[serde(default)]
    commitment_index_fault: Option<()>,
    #[serde(default)]
    icp_xdr_rate: Option<()>,
    #[serde(default)]
    last_icp_xdr_rate_attempt_ts: Option<u64>,
    #[serde(default)]
    last_icp_xdr_rate_error: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RecentCommitment {
    canister_id: Principal,
    #[serde(default)]
    raw_icp_memo_text: Option<String>,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RecentNeuronCommitment {
    neuron_id: u64,
    #[serde(default)]
    memo_text: Option<String>,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct InvalidCommitment {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    memo_text: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct RecentBurn {
    canister_id: Principal,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
}

#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
enum VersionedStableState {
    Uninitialized,
    Current(StableRootState),
}

impl Storable for VersionedStableState {
    const BOUND: Bound = Bound::Unbounded;

    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).unwrap())
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).unwrap()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).unwrap()
    }
}

fn insert_commitment(
    index_map: &mut StableBTreeMap<PrincipalKey, StableU64List, Memory>,
    entry_map: &mut StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>,
    canister: Principal,
    sample: CommitmentSample,
) {
    let key = PrincipalKey::from(canister);
    let mut ids = index_map.get(&key).unwrap_or_default().0;
    if !ids.contains(&sample.tx_id) {
        ids.push(sample.tx_id);
        ids.sort_unstable();
    }
    entry_map.insert(CommitmentEntryKey::new(canister, sample.tx_id), sample);
    index_map.insert(key, StableU64List(ids));
}

#[ic_cdk::init]
fn init(args: LegacyFixtureInit) {
    let staking_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some([42; 32]),
    };
    let root = VersionedStableState::Current(StableRootState {
        config: StableConfig {
            staking_account,
            output_source_account: Some(Account {
                owner: Principal::management_canister(),
                subaccount: Some([43; 32]),
            }),
            output_account: Some(Account {
                owner: Principal::management_canister(),
                subaccount: Some([44; 32]),
            }),
            rewards_account: Some(Account {
                owner: Principal::management_canister(),
                subaccount: Some([45; 32]),
            }),
            ledger_canister_id: args.ledger_canister_id,
            index_canister_id: args.index_canister_id,
            cmc_canister_id: Some(args.cmc_canister_id),
            faucet_canister_id: Some(args.cmc_canister_id),
            blackhole_canister_id: args.cmc_canister_id,
            sns_wasm_canister_id: args.sns_wasm_canister_id,
            xrc_canister_id: Some(args.xrc_canister_id),
            enable_sns_tracking: false,
            scan_interval_seconds: 3_600,
            cycles_interval_seconds: 86_400,
            min_tx_e8s: 10_000_000,
            max_cycles_entries_per_canister: 100,
            max_commitment_entries_per_canister: 100,
            max_index_pages_per_tick: 10,
            max_canisters_per_cycles_tick: 10,
            relay_factory_enabled: Some(false),
            relay_setup_min_e8s: Some(200_000_000),
            relay_setup_dust_e8s: Some(10_000),
            relay_setup_refund_cooldown_seconds: Some(300),
            relay_initial_cycles: Some(1_000_000_000_000),
            relay_cycle_safety_margin_e8s: Some(5_000_000),
            relay_min_subaccount_one_seed_e8s: Some(100_020_000),
            self_service_relay_interval_seconds: Some(3_600),
            self_service_relay_max_transfers_per_tick: Some(Some(10)),
            io_surplus_neuron_id: Some(11_614_578_985_374_291_210),
            canonical_relay_canister_id: Some(Some(args.canonical_relay)),
            canonical_relay_targets: Some(vec![args.canonical_target]),
        },
        last_indexed_staking_tx_id: Some(700),
        oldest_indexed_staking_tx_id: Some(650),
        staking_index_descending: Some(true),
        staking_backfill_complete: Some(false),
        last_indexed_output_tx_id: Some(800),
        oldest_indexed_output_tx_id: Some(750),
        output_route_index_descending: Some(true),
        output_route_backfill_complete: Some(false),
        last_indexed_rewards_tx_id: Some(900),
        oldest_indexed_rewards_tx_id: Some(850),
        rewards_route_index_descending: Some(true),
        rewards_route_backfill_complete: Some(false),
        last_sns_discovery_ts: 1_000,
        last_completed_cycles_sweep_ts: 2_000,
        last_completed_route_sweep_ts: Some(3_000),
        active_cycles_sweep: Some(ActiveCyclesSweep {
            started_at_ts_nanos: 4_000,
            canisters: vec![args.memo_target, args.sns_target],
            next_index: 1,
        }),
        initial_cycles_probe_queue: vec![args.memo_target],
        active_route_sweep: None,
        active_sns_discovery: None,
        main_lock_state_ts: Some(999_999),
        last_main_run_ts: 4_000,
        qualifying_commitment_count: Some(2),
        total_output_e8s: Some(12_345),
        total_rewards_e8s: Some(67_890),
        icp_burned_e8s: Some(55),
        recent_commitments: Some(vec![RecentCommitment {
            canister_id: args.memo_target,
            raw_icp_memo_text: None,
            tx_id: 20,
            timestamp_nanos: Some(20_000),
            amount_e8s: 200_000_000,
            counts_toward_faucet: true,
        }]),
        recent_under_threshold_commitments: Some(Vec::new()),
        recent_neuron_commitments: Some(vec![RecentNeuronCommitment {
            neuron_id: 42,
            memo_text: Some("42".to_string()),
            tx_id: 501,
            timestamp_nanos: Some(501_000),
            amount_e8s: 111,
            counts_toward_faucet: false,
        }]),
        recent_under_threshold_neuron_commitments: Some(Vec::new()),
        recent_invalid_commitments: Some(vec![InvalidCommitment {
            tx_id: 600,
            timestamp_nanos: Some(600_000),
            amount_e8s: 1,
            memo_text: "bad".to_string(),
        }]),
        recent_burns: Some(vec![RecentBurn {
            canister_id: args.memo_target,
            tx_id: 99,
            timestamp_nanos: Some(99_000),
            amount_e8s: 55,
        }]),
        last_index_run_ts: Some(4_001),
        commitment_index_fault: None,
        icp_xdr_rate: None,
        last_icp_xdr_rate_attempt_ts: Some(4_002),
        last_icp_xdr_rate_error: None,
    });
    let mut root_cell = StableCell::init(memory(0), VersionedStableState::Uninitialized);
    root_cell.set(root);

    let mut reasons = StableBTreeMap::<PrincipalKey, StableSourceSet, Memory>::init(memory(10));
    reasons.insert(
        PrincipalKey::from(args.memo_target),
        StableSourceSet(BTreeSet::from([CanisterSource::MemoCommitment])),
    );
    reasons.insert(
        PrincipalKey::from(args.sns_target),
        StableSourceSet(BTreeSet::from([CanisterSource::SnsDiscovery])),
    );

    let mut meta = StableBTreeMap::<PrincipalKey, StableCanisterMeta, Memory>::init(memory(11));
    for (principal, first_seen) in [
        (args.memo_target, 111),
        (args.sns_target, 222),
        (args.self_service_target, 333),
        (args.self_service_relay, 444),
    ] {
        meta.insert(
            PrincipalKey::from(principal),
            StableCanisterMeta {
                first_seen_ts: Some(first_seen),
                last_commitment_ts: Some(first_seen + 10),
                last_cycles_probe_ts: Some(first_seen + 20),
                last_cycles_probe_result: Some(CyclesProbeResult::Ok(
                    CyclesSampleSource::BlackholeStatus,
                )),
                last_burn_tx_id: Some(first_seen + 30),
                last_burn_scan_tx_id: Some(first_seen + 31),
                burned_e8s: Some(first_seen + 32),
            },
        );
    }

    let mut commitment_index =
        StableBTreeMap::<PrincipalKey, StableU64List, Memory>::init(memory(14));
    let mut commitment_entries =
        StableBTreeMap::<CommitmentEntryKey, CommitmentSample, Memory>::init(memory(16));
    insert_commitment(
        &mut commitment_index,
        &mut commitment_entries,
        args.memo_target,
        CommitmentSample {
            tx_id: 10,
            timestamp_nanos: Some(10_000),
            amount_e8s: 100_000_000,
            counts_toward_faucet: true,
        },
    );
    insert_commitment(
        &mut commitment_index,
        &mut commitment_entries,
        args.memo_target,
        CommitmentSample {
            tx_id: 20,
            timestamp_nanos: Some(20_000),
            amount_e8s: 200_000_000,
            counts_toward_faucet: true,
        },
    );

    let mut cycles_index = StableBTreeMap::<PrincipalKey, StableU64List, Memory>::init(memory(15));
    let mut cycles_entries =
        StableBTreeMap::<CyclesEntryKey, CyclesSample, Memory>::init(memory(17));
    let cycles_samples = [
        (31_000, 31, CyclesSampleSource::BlackholeStatus),
        (32_000, 32, CyclesSampleSource::SelfCanister),
        (33_000, 33, CyclesSampleSource::SnsRootSummary),
    ];
    cycles_index.insert(
        PrincipalKey::from(args.memo_target),
        StableU64List(cycles_samples.iter().map(|(ts, _, _)| *ts).collect()),
    );
    for (timestamp_nanos, cycles, source) in cycles_samples {
        cycles_entries.insert(
            CyclesEntryKey::new(args.memo_target, timestamp_nanos),
            CyclesSample {
                timestamp_nanos,
                cycles,
                source,
            },
        );
    }

    let mut raw_index = StableBTreeMap::<PrincipalKey, StableU64List, Memory>::init(memory(18));
    let mut raw_entries =
        StableBTreeMap::<CommitmentEntryKey, CommitmentSample, Memory>::init(memory(19));
    insert_commitment(
        &mut raw_index,
        &mut raw_entries,
        args.sns_target,
        CommitmentSample {
            tx_id: 401,
            timestamp_nanos: Some(401_000),
            amount_e8s: 401,
            counts_toward_faucet: false,
        },
    );

    let mut neuron_index = StableBTreeMap::<u64, StableU64List, Memory>::init(memory(20));
    let mut neuron_entries =
        StableBTreeMap::<NeuronCommitmentEntryKey, CommitmentSample, Memory>::init(memory(21));
    neuron_index.insert(42, StableU64List(vec![501]));
    neuron_entries.insert(
        NeuronCommitmentEntryKey {
            neuron_id: 42,
            tx_id: 501,
        },
        CommitmentSample {
            tx_id: 501,
            timestamp_nanos: Some(501_000),
            amount_e8s: 501,
            counts_toward_faucet: false,
        },
    );

    let mut registry = StableBTreeMap::<PrincipalKey, RelayRegistryEntry, Memory>::init(memory(22));
    registry.insert(
        PrincipalKey::from(args.canonical_target),
        RelayRegistryEntry {
            relay_canister_id: args.canonical_relay,
            target_canister_id: args.canonical_target,
            kind: RelayRegistryKind::Canonical,
            status: RelayRegistryStatus::Active,
            setup_account: None,
            setup_account_identifier: None,
            setup_amount_e8s: None,
            setup_tx_ids: vec![],
            relay_wasm_hash_hex: Some("discarded-canonical-hash".to_string()),
            final_controllers: None,
            log_visibility_public: Some(true),
            created_at_ts: Some(5_000),
            activated_at_ts: Some(5_001),
        },
    );
    registry.insert(
        PrincipalKey::from(args.self_service_target),
        RelayRegistryEntry {
            relay_canister_id: args.self_service_relay,
            target_canister_id: args.self_service_target,
            kind: RelayRegistryKind::SelfService,
            status: RelayRegistryStatus::Active,
            setup_account: Some(Account {
                owner: ic_cdk::api::canister_self(),
                subaccount: Some([7; 32]),
            }),
            setup_account_identifier: Some("self-service-setup".to_string()),
            setup_amount_e8s: Some(300_000_000),
            setup_tx_ids: vec![701],
            relay_wasm_hash_hex: Some("discarded-self-service-hash".to_string()),
            final_controllers: Some(vec![args.cmc_canister_id]),
            log_visibility_public: Some(true),
            created_at_ts: Some(6_000),
            activated_at_ts: Some(6_001),
        },
    );

    let mut jobs = StableBTreeMap::<PrincipalKey, RelaySetupJob, Memory>::init(memory(24));
    jobs.insert(
        PrincipalKey::from(args.normal_setup_target),
        setup_job(args.normal_setup_target, RelaySetupStatus::Pending, false),
    );
    jobs.insert(
        PrincipalKey::from(args.refundable_setup_target),
        setup_job(
            args.refundable_setup_target,
            RelaySetupStatus::TargetNotObservable,
            false,
        ),
    );
    jobs.insert(
        PrincipalKey::from(args.unsafe_setup_target),
        setup_job(
            args.unsafe_setup_target,
            RelaySetupStatus::TargetNotObservable,
            true,
        ),
    );
}

fn setup_job(target: Principal, status: RelaySetupStatus, unsafe_evidence: bool) -> RelaySetupJob {
    RelaySetupJob {
        target_canister_id: target,
        setup_account: Account {
            owner: ic_cdk::api::canister_self(),
            subaccount: Some([8; 32]),
        },
        setup_account_identifier: format!("setup-{target}"),
        status,
        relay_canister_id: unsafe_evidence.then_some(Principal::from_slice(&[99])),
        last_indexed_setup_tx_id: Some(700),
        setup_tx_ids: vec![700],
        setup_amount_seen_e8s: 300_000_000,
        setup_amount_processed_e8s: if unsafe_evidence { 100_000_000 } else { 0 },
        payments: vec![RelaySetupPayment {
            target_canister_id: target,
            tx_id: 700,
            from_account_identifier: "payer".to_string(),
            amount_e8s: 300_000_000,
            timestamp_nanos: Some(700_000),
            processed: unsafe_evidence,
            refunded: false,
        }],
        cycle_conversion_e8s: unsafe_evidence.then_some(100_000_000),
        cycle_transfer_block_index: unsafe_evidence.then_some(701),
        cycles_minted: unsafe_evidence.then_some(1_000_000_000_000),
        relay_initial_cycles: Some(1_000_000_000_000),
        relay_funding_e8s: None,
        relay_funding_block_index: None,
        phase: Some(if unsafe_evidence {
            RelaySetupPhase::RelayCanisterCreated
        } else {
            RelaySetupPhase::PreSpend
        }),
        cycle_transfer: unsafe_evidence.then_some(RelaySetupTransferRecord {
            kind: RelaySetupTransferKind::CmcConversion,
            from_subaccount: Some([8; 32]),
            from_account_identifier: "setup".to_string(),
            to: Account {
                owner: Principal::management_canister(),
                subaccount: None,
            },
            to_account_identifier: "cmc".to_string(),
            amount_e8s: 100_000_000,
            fee_e8s: 10_000,
            memo: None,
            created_at_time_nanos: 701_000,
            block_index: Some(701),
            completed: true,
        }),
        relay_funding_transfer: None,
        existing_relay_sweep_transfer: None,
        refund_transfers: Vec::new(),
        relay_create_attempt: unsafe_evidence.then_some(RelayCreateAttempt {
            target_canister_id: target,
            created_at_ts: 702,
            initial_cycles: 1_000_000_000_000,
            raw_relay_wasm_hash_hex: Some("discarded-raw".to_string()),
            install_payload_hash_hex: Some("discarded-install".to_string()),
            relay_wasm_hash_hex: Some("discarded-gz".to_string()),
        }),
        code_installed: false,
        relay_funding_accepted: false,
        blackhole_update_attempted: false,
        blackhole_confirmed: false,
        refund_attempt_count: 0,
        last_refund_attempt_ts: None,
        refund_blocks: Vec::new(),
        created_at_ts: 700,
        updated_at_ts: 701,
        last_error: None,
    }
}

#[ic_cdk::query]
fn debug_seed_summary() -> LegacySeedSummary {
    LegacySeedSummary {
        revision: LEGACY_HISTORIAN_V1_REVISION.to_string(),
        memo_target: Principal::from_slice(&[1, 1]),
        sns_target: Principal::from_slice(&[1, 2]),
        canonical_target: Principal::from_slice(&[1, 3]),
        canonical_relay: Principal::from_slice(&[1, 4]),
        self_service_target: Principal::from_slice(&[1, 5]),
        self_service_relay: Principal::from_slice(&[1, 6]),
        normal_setup_target: Principal::from_slice(&[1, 7]),
        refundable_setup_target: Principal::from_slice(&[1, 8]),
        unsafe_setup_target: Principal::from_slice(&[1, 9]),
    }
}

ic_cdk::export_candid!();
