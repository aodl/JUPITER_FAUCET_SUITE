use candid::{CandidType, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Config {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    #[serde(default)]
    pub cmc_canister_id: Option<Principal>,
    #[serde(default)]
    pub faucet_canister_id: Option<Principal>,
    pub blackhole_canister_id: Principal,
    pub sns_wasm_canister_id: Principal,
    pub enable_sns_tracking: bool,
    pub scan_interval_seconds: u64,
    pub cycles_interval_seconds: u64,
    pub min_tx_e8s: u64,
    pub max_cycles_entries_per_canister: u32,
    pub max_contribution_entries_per_canister: u32,
    pub max_index_pages_per_tick: u32,
    pub max_canisters_per_cycles_tick: u32,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanisterSource {
    MemoContribution,
    SnsDiscovery,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum CyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum CyclesProbeResult {
    Ok(CyclesSampleSource),
    NotAvailable,
    Error(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ContributionSample {
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct InvalidContribution {
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub memo_text: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RecentContribution {
    pub canister_id: Principal,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RecentBurn {
    pub canister_id: Principal,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
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
    pub last_contribution_ts: Option<u64>,
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
pub struct ActiveSnsDiscovery {
    pub started_at_ts_nanos: u64,
    pub root_canister_ids: Vec<Principal>,
    pub next_index: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ActiveCyclesSweep {
    pub started_at_ts_nanos: u64,
    pub canisters: Vec<Principal>,
    pub next_index: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct StableConfig {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    #[serde(default)]
    pub cmc_canister_id: Option<Principal>,
    #[serde(default)]
    pub faucet_canister_id: Option<Principal>,
    pub blackhole_canister_id: Principal,
    pub sns_wasm_canister_id: Principal,
    pub enable_sns_tracking: bool,
    pub scan_interval_seconds: u64,
    pub cycles_interval_seconds: u64,
    pub min_tx_e8s: u64,
    pub max_cycles_entries_per_canister: u32,
    pub max_contribution_entries_per_canister: u32,
    pub max_index_pages_per_tick: u32,
    pub max_canisters_per_cycles_tick: u32,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct StableCanisterMeta {
    pub first_seen_ts: Option<u64>,
    pub last_contribution_ts: Option<u64>,
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
pub struct StableRootState {
    pub config: StableConfig,
    pub last_indexed_staking_tx_id: Option<u64>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    pub active_cycles_sweep: Option<ActiveCyclesSweep>,
    #[serde(default)]
    pub active_sns_discovery: Option<ActiveSnsDiscovery>,
    pub main_lock_state_ts: Option<u64>,
    pub last_main_run_ts: u64,
    #[serde(default)]
    pub qualifying_contribution_count: Option<u64>,
    #[serde(default)]
    pub icp_burned_e8s: Option<u64>,
    #[serde(default)]
    pub recent_contributions: Option<Vec<RecentContribution>>,
    #[serde(default)]
    pub recent_under_threshold_contributions: Option<Vec<RecentContribution>>,
    #[serde(default)]
    pub recent_invalid_contributions: Option<Vec<InvalidContribution>>,
    #[serde(default)]
    pub recent_burns: Option<Vec<RecentBurn>>,
    #[serde(default)]
    pub last_index_run_ts: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PrincipalKey(Vec<u8>);

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
    fn to_principal(&self) -> Principal {
        Principal::from_slice(&self.0)
    }
}

impl Storable for PrincipalKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.clone())
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 29,
        is_fixed_size: false,
    };
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
struct StableSourceSet(pub BTreeSet<CanisterSource>);

impl Storable for StableSourceSet {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian stable source set"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian stable source set")
    }

    const BOUND: Bound = Bound::Unbounded;
}


#[derive(CandidType, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
struct StableU64List(pub Vec<u64>);

impl Storable for StableU64List {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian stable u64 list"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian stable u64 list")
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ContributionEntryKey {
    canister: PrincipalKey,
    tx_id: u64,
}

impl ContributionEntryKey {
    fn new(canister: impl Into<PrincipalKey>, tx_id: u64) -> Self {
        Self {
            canister: canister.into(),
            tx_id,
        }
    }
}

impl Storable for ContributionEntryKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut out = Vec::with_capacity(1 + self.canister.0.len() + 8);
        out.push(self.canister.0.len() as u8);
        out.extend_from_slice(&self.canister.0);
        out.extend_from_slice(&self.tx_id.to_be_bytes());
        Cow::Owned(out)
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let len = bytes.first().copied().unwrap_or(0) as usize;
        assert!(bytes.len() == 1 + len + 8, "invalid historian contribution entry key length");
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
struct CyclesEntryKey {
    canister: PrincipalKey,
    timestamp_nanos: u64,
}

impl CyclesEntryKey {
    fn new(canister: impl Into<PrincipalKey>, timestamp_nanos: u64) -> Self {
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

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let bytes = bytes.as_ref();
        let len = bytes.first().copied().unwrap_or(0) as usize;
        assert!(bytes.len() == 1 + len + 8, "invalid historian cycles entry key length");
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

impl Storable for ContributionSample {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian contribution sample"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian contribution sample")
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for CyclesSample {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian cycles sample"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian cycles sample")
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for StableCanisterMeta {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian stable canister meta"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian stable canister meta")
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct State {
    pub config: Config,
    pub distinct_canisters: BTreeSet<Principal>,
    pub canister_sources: BTreeMap<Principal, BTreeSet<CanisterSource>>,
    pub contribution_history: BTreeMap<Principal, Vec<ContributionSample>>,
    pub cycles_history: BTreeMap<Principal, Vec<CyclesSample>>,
    pub per_canister_meta: BTreeMap<Principal, CanisterMeta>,
    #[serde(default)]
    pub registered_canister_summaries_cache: Option<BTreeMap<Principal, crate::RegisteredCanisterSummary>>,
    #[serde(default)]
    pub registered_canister_summaries_total_desc_index: Option<Vec<Principal>>,
    pub last_indexed_staking_tx_id: Option<u64>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    pub active_cycles_sweep: Option<ActiveCyclesSweep>,
    #[serde(default)]
    pub active_sns_discovery: Option<ActiveSnsDiscovery>,
    pub main_lock_state_ts: Option<u64>,
    pub last_main_run_ts: u64,
    pub qualifying_contribution_count: Option<u64>,
    pub icp_burned_e8s: Option<u64>,
    pub recent_contributions: Option<Vec<RecentContribution>>,
    pub recent_under_threshold_contributions: Option<Vec<RecentContribution>>,
    pub recent_invalid_contributions: Option<Vec<InvalidContribution>>,
    pub recent_burns: Option<Vec<RecentBurn>>,
    pub last_index_run_ts: Option<u64>,
}

impl State {
    pub fn new(config: Config, now_secs: u64) -> Self {
        Self {
            config,
            distinct_canisters: BTreeSet::new(),
            canister_sources: BTreeMap::new(),
            contribution_history: BTreeMap::new(),
            cycles_history: BTreeMap::new(),
            per_canister_meta: BTreeMap::new(),
            registered_canister_summaries_cache: Some(BTreeMap::new()),
            registered_canister_summaries_total_desc_index: Some(Vec::new()),
            last_indexed_staking_tx_id: None,
            last_sns_discovery_ts: 0,
            last_completed_cycles_sweep_ts: 0,
            active_cycles_sweep: None,
            active_sns_discovery: None,
            main_lock_state_ts: Some(0),
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
            qualifying_contribution_count: Some(0),
            icp_burned_e8s: Some(0),
            recent_contributions: Some(Vec::new()),
            recent_under_threshold_contributions: Some(Vec::new()),
            recent_invalid_contributions: Some(Vec::new()),
            recent_burns: Some(Vec::new()),
            last_index_run_ts: Some(0),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub enum VersionedStableState {
    Uninitialized,
    Current(StableRootState),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian root stable state"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian root stable state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_ROOT_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CANISTER_SOURCES_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableSourceSet, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CANISTER_META_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableCanisterMeta, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CONTRIBUTION_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CYCLES_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CONTRIBUTION_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<ContributionEntryKey, ContributionSample, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CYCLES_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CyclesEntryKey, CyclesSample, Memory>>> =
        std::cell::RefCell::new(None);
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
    static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static PERSISTENCE_DIRTY_SECTIONS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static DIRTY_REGISTRY_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
    static DIRTY_CONTRIBUTION_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
    static DIRTY_CYCLES_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
}

pub const DIRTY_ROOT: u8 = 1 << 0;
pub const DIRTY_REGISTRY: u8 = 1 << 1;
pub const DIRTY_CONTRIBUTIONS: u8 = 1 << 2;
pub const DIRTY_CYCLES: u8 = 1 << 3;
pub const DIRTY_ALL: u8 = DIRTY_ROOT | DIRTY_REGISTRY | DIRTY_CONTRIBUTIONS | DIRTY_CYCLES;

fn with_root_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_ROOT_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized)
                    .expect("failed to initialize historian root stable cell");
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("historian root stable cell not initialized"))
    })
}





fn with_canister_sources_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableSourceSet, Memory>) -> R,
) -> R {
    STABLE_CANISTER_SOURCES_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(10));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("historian canister-sources stable map not initialized"))
    })
}

fn with_canister_meta_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableCanisterMeta, Memory>) -> R,
) -> R {
    STABLE_CANISTER_META_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(11));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("historian canister-meta stable map not initialized"))
    })
}



fn with_contribution_history_index_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableU64List, Memory>) -> R,
) -> R {
    STABLE_CONTRIBUTION_HISTORY_INDEX_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(14));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("historian contribution-history index map not initialized"))
    })
}

fn with_cycles_history_index_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableU64List, Memory>) -> R,
) -> R {
    STABLE_CYCLES_HISTORY_INDEX_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(15));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("historian cycles-history index map not initialized"))
    })
}

fn with_contribution_entry_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<ContributionEntryKey, ContributionSample, Memory>) -> R,
) -> R {
    STABLE_CONTRIBUTION_ENTRY_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(16));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("historian contribution entry map not initialized"))
    })
}

fn with_cycles_entry_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<CyclesEntryKey, CyclesSample, Memory>) -> R,
) -> R {
    STABLE_CYCLES_ENTRY_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(17));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("historian cycles entry map not initialized"))
    })
}

fn mark_registry_principal_dirty(canister_id: Principal) {
    DIRTY_REGISTRY_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

fn mark_contribution_principal_dirty(canister_id: Principal) {
    DIRTY_CONTRIBUTION_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

fn mark_cycles_principal_dirty(canister_id: Principal) {
    DIRTY_CYCLES_PRINCIPALS.with(|dirty| {
        dirty.borrow_mut().insert(canister_id);
    });
}

fn dirty_registry_principals() -> BTreeSet<Principal> {
    DIRTY_REGISTRY_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

fn dirty_contribution_principals() -> BTreeSet<Principal> {
    DIRTY_CONTRIBUTION_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

fn dirty_cycles_principals() -> BTreeSet<Principal> {
    DIRTY_CYCLES_PRINCIPALS.with(|dirty| dirty.borrow().clone())
}

fn stable_contribution_history_keys_internal() -> BTreeSet<Principal> {
    with_contribution_history_index_map(|map| map.iter().map(|(key, _)| key.to_principal()).collect())
}

fn stable_cycles_history_keys_internal() -> BTreeSet<Principal> {
    with_cycles_history_index_map(|map| map.iter().map(|(key, _)| key.to_principal()).collect())
}

fn load_stable_contribution_history_internal(canister_id: Principal) -> Vec<ContributionSample> {
    with_contribution_history_index_map(|index_map| {
        index_map
            .get(&PrincipalKey::from(canister_id))
            .map(|ids| {
                ids.0
                    .into_iter()
                    .filter_map(|tx_id| {
                        with_contribution_entry_map(|entry_map| {
                            entry_map.get(&ContributionEntryKey::new(canister_id, tx_id))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn load_stable_cycles_history_internal(canister_id: Principal) -> Vec<CyclesSample> {
    with_cycles_history_index_map(|index_map| {
        index_map
            .get(&PrincipalKey::from(canister_id))
            .map(|ids| {
                ids.0
                    .into_iter()
                    .filter_map(|timestamp_nanos| {
                        with_cycles_entry_map(|entry_map| {
                            entry_map.get(&CyclesEntryKey::new(canister_id, timestamp_nanos))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn rebuild_distinct_canisters(st: &mut State) {
    st.distinct_canisters = st
        .canister_sources
        .keys()
        .copied()
        .chain(st.contribution_history.keys().copied())
        .chain(stable_contribution_history_keys_internal())
        .chain(st.cycles_history.keys().copied())
        .chain(stable_cycles_history_keys_internal())
        .chain(st.per_canister_meta.keys().copied())
        .collect();
}

fn sync_canister_sources_map(
    current: &BTreeMap<Principal, BTreeSet<CanisterSource>>,
    scope: Option<&BTreeSet<Principal>>,
) {
    with_canister_sources_map(|map| {
        match scope {
            Some(principals) => {
                for principal in principals {
                    let key = PrincipalKey::from(principal);
                    match current.get(principal) {
                        Some(sources) => {
                            let desired = StableSourceSet(sources.clone());
                            let needs_update = map.get(&key).map(|existing| existing != desired).unwrap_or(true);
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
                    let needs_update = map.get(&key).map(|existing| existing != desired).unwrap_or(true);
                    if needs_update {
                        map.insert(key, desired);
                    }
                }
            }
        }
    });
}

fn sync_canister_meta_map(current: &BTreeMap<Principal, CanisterMeta>, scope: Option<&BTreeSet<Principal>>) {
    with_canister_meta_map(|map| {
        match scope {
            Some(principals) => {
                for principal in principals {
                    let key = PrincipalKey::from(principal);
                    match current.get(principal) {
                        Some(meta) => {
                            let desired: StableCanisterMeta = meta.clone().into();
                            let needs_update = map.get(&key).map(|existing| existing != desired).unwrap_or(true);
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
                    let needs_update = map.get(&key).map(|existing| existing != desired).unwrap_or(true);
                    if needs_update {
                        map.insert(key, desired);
                    }
                }
            }
        }
    });
}

fn sync_all_contribution_history_maps(current: &BTreeMap<Principal, Vec<ContributionSample>>) {
    with_contribution_history_index_map(|map| map.clear_new());
    with_contribution_entry_map(|map| map.clear_new());
    for (principal, samples) in current {
        let ids: Vec<u64> = samples.iter().map(|sample| sample.tx_id).collect();
        if !ids.is_empty() {
            with_contribution_history_index_map(|map| {
                map.insert(PrincipalKey::from(principal), StableU64List(ids));
            });
            with_contribution_entry_map(|map| {
                for sample in samples {
                    map.insert(ContributionEntryKey::new(principal, sample.tx_id), sample.clone());
                }
            });
        }
    }
}

fn sync_contribution_history_principals(
    current: &BTreeMap<Principal, Vec<ContributionSample>>,
    principals: &BTreeSet<Principal>,
) {
    for principal in principals {
        let principal_key = PrincipalKey::from(principal);
        let existing_ids = with_contribution_history_index_map(|map| {
            map.get(&principal_key)
                .map(|ids| ids.0.clone())
                .unwrap_or_default()
        });
        let current_samples = current.get(principal).cloned().unwrap_or_default();
        let current_ids: Vec<u64> = current_samples.iter().map(|sample| sample.tx_id).collect();
        let current_id_set: BTreeSet<u64> = current_ids.iter().copied().collect();

        with_contribution_entry_map(|map| {
            for tx_id in &existing_ids {
                if !current_id_set.contains(tx_id) {
                    map.remove(&ContributionEntryKey::new(principal, *tx_id));
                }
            }
            for sample in &current_samples {
                let key = ContributionEntryKey::new(principal, sample.tx_id);
                let needs_update = map.get(&key).map(|existing| existing != *sample).unwrap_or(true);
                if needs_update {
                    map.insert(key, sample.clone());
                }
            }
        });

        with_contribution_history_index_map(|map| {
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


fn build_root_snapshot(st: &State) -> StableRootState {
    StableRootState {
        config: st.config.clone().into(),
        last_indexed_staking_tx_id: st.last_indexed_staking_tx_id,
        last_sns_discovery_ts: st.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: st.last_completed_cycles_sweep_ts,
        active_cycles_sweep: st.active_cycles_sweep.clone(),
        active_sns_discovery: st.active_sns_discovery.clone(),
        main_lock_state_ts: st.main_lock_state_ts,
        last_main_run_ts: st.last_main_run_ts,
        qualifying_contribution_count: st.qualifying_contribution_count,
        icp_burned_e8s: st.icp_burned_e8s,
        recent_contributions: st.recent_contributions.clone(),
        recent_under_threshold_contributions: st.recent_under_threshold_contributions.clone(),
        recent_invalid_contributions: st.recent_invalid_contributions.clone(),
        recent_burns: st.recent_burns.clone(),
        last_index_run_ts: st.last_index_run_ts,
    }
}

fn persist_snapshot_sections_scoped(
    st: &State,
    dirty_sections: u8,
    registry_scope: Option<&BTreeSet<Principal>>,
    contribution_scope: Option<&BTreeSet<Principal>>,
    cycles_scope: Option<&BTreeSet<Principal>>,
) {
    if dirty_sections & DIRTY_REGISTRY != 0 {
        sync_canister_sources_map(&st.canister_sources, registry_scope);
        sync_canister_meta_map(&st.per_canister_meta, registry_scope);
    }
    if dirty_sections & DIRTY_CONTRIBUTIONS != 0 {
        if let Some(scope) = contribution_scope {
            sync_contribution_history_principals(&st.contribution_history, scope);
        } else {
            sync_all_contribution_history_maps(&st.contribution_history);
        }
    }
    if dirty_sections & DIRTY_CYCLES != 0 {
        if let Some(scope) = cycles_scope {
            sync_cycles_history_principals(&st.cycles_history, scope);
        } else {
            sync_all_cycles_history_maps(&st.cycles_history);
        }
    }
    if dirty_sections & DIRTY_ROOT != 0 {
        // Commit the root section last so the durable root always points at fully written
        // bulk sections. This keeps the root as the final commit marker if a trap occurs before
        // the map-backed writes complete.
        with_root_stable_cell(|cell| {
            cell.set(VersionedStableState::Current(build_root_snapshot(st)))
                .expect("failed to persist historian root stable state");
        });
    }
}

fn persist_snapshot_sections(st: &State, dirty_sections: u8) {
    persist_snapshot_sections_scoped(st, dirty_sections, None, None, None);
}

fn persist_snapshot(st: &State) {
    persist_snapshot_sections(st, DIRTY_ALL);
}

pub fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

fn restore_state_current(root: StableRootState) -> State {
    let canister_sources = with_canister_sources_map(|map| {
        let mut out = BTreeMap::new();
        for (key, value) in map.iter() {
            out.insert(key.to_principal(), value.0.clone());
        }
        out
    });
    let contribution_history = BTreeMap::new();
    let cycles_history = BTreeMap::new();
    let per_canister_meta = with_canister_meta_map(|map| {
        let mut out = BTreeMap::new();
        for (key, value) in map.iter() {
            out.insert(key.to_principal(), value.clone().into());
        }
        out
    });

    let mut st = State {
        config: root.config.into(),
        distinct_canisters: BTreeSet::new(),
        canister_sources,
        contribution_history,
        cycles_history,
        per_canister_meta,
        registered_canister_summaries_cache: None,
        registered_canister_summaries_total_desc_index: None,
        last_indexed_staking_tx_id: root.last_indexed_staking_tx_id,
        last_sns_discovery_ts: root.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: root.last_completed_cycles_sweep_ts,
        active_cycles_sweep: root.active_cycles_sweep,
        active_sns_discovery: root.active_sns_discovery,
        main_lock_state_ts: root.main_lock_state_ts,
        last_main_run_ts: root.last_main_run_ts,
        qualifying_contribution_count: root.qualifying_contribution_count,
        icp_burned_e8s: root.icp_burned_e8s,
        recent_contributions: root.recent_contributions,
        recent_under_threshold_contributions: root.recent_under_threshold_contributions,
        recent_invalid_contributions: root.recent_invalid_contributions,
        recent_burns: root.recent_burns,
        last_index_run_ts: root.last_index_run_ts,
    };
    rebuild_distinct_canisters(&mut st);
    st
}

pub fn restore_state_from_stable() -> Option<State> {
    let snapshot = with_root_stable_cell(|cell| cell.get().clone());
    match snapshot {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::Current(root) => Some(restore_state_current(root)),
    }
}

pub fn set_state(st: State) {
    persist_snapshot(&st);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub fn set_state_root_only(st: State) {
    persist_snapshot_sections(&st, DIRTY_ROOT);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub fn get_state() -> State {
    STATE.with(|s| s.borrow().clone()).expect("state not initialized")
}

pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.get() > 0)
}

fn mark_persistence_dirty(dirty_sections: u8) {
    PERSISTENCE_DIRTY_SECTIONS.with(|dirty| dirty.set(dirty.get() | dirty_sections));
}

fn clear_persistence_dirty() {
    PERSISTENCE_DIRTY_SECTIONS.with(|dirty| dirty.set(0));
    DIRTY_REGISTRY_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_CONTRIBUTION_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_CYCLES_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
}

pub fn persist_dirty_state() {
    let dirty_sections = PERSISTENCE_DIRTY_SECTIONS.with(|flag| flag.get());
    if dirty_sections == 0 {
        return;
    }
    let registry_scope = dirty_registry_principals();
    let contribution_scope = dirty_contribution_principals();
    let cycles_scope = dirty_cycles_principals();
    let snapshot = get_state();
    persist_snapshot_sections_scoped(
        &snapshot,
        dirty_sections,
        (!registry_scope.is_empty()).then_some(&registry_scope),
        (!contribution_scope.is_empty()).then_some(&contribution_scope),
        (!cycles_scope.is_empty()).then_some(&cycles_scope),
    );
    clear_persistence_dirty();
}

/// A synchronous persistence-batch guard.
///
/// Do not hold this guard across an `await` point. While it is live, mutations are
/// only marked dirty and are not durably flushed until the batch ends or an
/// explicit `persist_dirty_state()` call occurs.
pub struct PersistenceBatch {
    active: bool,
}

impl Drop for PersistenceBatch {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let should_flush = PERSISTENCE_BATCH_DEPTH.with(|depth| {
            let current = depth.get();
            assert!(current > 0, "persistence batch depth underflow");
            depth.set(current - 1);
            current == 1
        });
        if should_flush {
            persist_dirty_state();
        }
        self.active = false;
    }
}

#[must_use]
pub fn begin_persistence_batch() -> PersistenceBatch {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    PersistenceBatch { active: true }
}

fn with_state_mut_sections_scoped<R>(
    dirty_sections: u8,
    registry_principal: Option<Principal>,
    contribution_principal: Option<Principal>,
    cycles_principal: Option<Principal>,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        let immediate_persist = !persistence_batch_active();
        let out = f(st);
        if immediate_persist {
            let snapshot = st.clone();
            drop(borrow);
            let registry_scope = registry_principal.into_iter().collect::<BTreeSet<_>>();
            let contribution_scope = contribution_principal.into_iter().collect::<BTreeSet<_>>();
            let cycles_scope = cycles_principal.into_iter().collect::<BTreeSet<_>>();
            persist_snapshot_sections_scoped(
                &snapshot,
                dirty_sections,
                (!registry_scope.is_empty()).then_some(&registry_scope),
                (!contribution_scope.is_empty()).then_some(&contribution_scope),
                (!cycles_scope.is_empty()).then_some(&cycles_scope),
            );
            return out;
        }
        if let Some(canister_id) = registry_principal {
            mark_registry_principal_dirty(canister_id);
        }
        if let Some(canister_id) = contribution_principal {
            mark_contribution_principal_dirty(canister_id);
        }
        if let Some(canister_id) = cycles_principal {
            mark_cycles_principal_dirty(canister_id);
        }
        mark_persistence_dirty(dirty_sections);
        drop(borrow);
        out
    })
}

pub fn with_state_mut_sections<R>(dirty_sections: u8, f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections_scoped(dirty_sections, None, None, None, f)
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections(DIRTY_ALL, f)
}

pub fn with_root_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections(DIRTY_ROOT, f)
}

pub fn stable_contribution_history_keys() -> BTreeSet<Principal> {
    stable_contribution_history_keys_internal()
}

pub fn stable_cycles_history_keys() -> BTreeSet<Principal> {
    stable_cycles_history_keys_internal()
}

pub fn stable_contribution_history_for(canister_id: Principal) -> Vec<ContributionSample> {
    load_stable_contribution_history_internal(canister_id)
}

pub fn stable_cycles_history_for(canister_id: Principal) -> Vec<CyclesSample> {
    load_stable_cycles_history_internal(canister_id)
}

pub fn ensure_contribution_history_loaded(st: &mut State, canister_id: Principal) {
    if st.contribution_history.contains_key(&canister_id) {
        return;
    }
    let history = load_stable_contribution_history_internal(canister_id);
    if !history.is_empty() {
        st.contribution_history.insert(canister_id, history);
    }
}

pub fn ensure_cycles_history_loaded(st: &mut State, canister_id: Principal) {
    if st.cycles_history.contains_key(&canister_id) {
        return;
    }
    let history = load_stable_cycles_history_internal(canister_id);
    if !history.is_empty() {
        st.cycles_history.insert(canister_id, history);
    }
}

pub fn with_root_and_registry_canister_state_mut<R>(canister_id: Principal, f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections_scoped(DIRTY_ROOT | DIRTY_REGISTRY, Some(canister_id), None, None, f)
}

pub fn with_root_registry_and_contributions_canister_state_mut<R>(
    canister_id: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_REGISTRY | DIRTY_CONTRIBUTIONS,
        Some(canister_id),
        Some(canister_id),
        None,
        f,
    )
}

pub fn with_root_registry_and_cycles_canister_state_mut<R>(
    canister_id: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_REGISTRY | DIRTY_CYCLES,
        Some(canister_id),
        None,
        Some(canister_id),
        f,
    )
}

impl From<Config> for StableConfig {
    fn from(value: Config) -> Self {
        Self {
            staking_account: value.staking_account,
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: value.cmc_canister_id,
            faucet_canister_id: value.faucet_canister_id,
            blackhole_canister_id: value.blackhole_canister_id,
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_contribution_entries_per_canister: value.max_contribution_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
        }
    }
}

impl From<StableConfig> for Config {
    fn from(value: StableConfig) -> Self {
        Self {
            staking_account: value.staking_account,
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: value.cmc_canister_id,
            faucet_canister_id: value.faucet_canister_id,
            blackhole_canister_id: value.blackhole_canister_id,
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_contribution_entries_per_canister: value.max_contribution_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
        }
    }
}

impl From<CanisterMeta> for StableCanisterMeta {
    fn from(value: CanisterMeta) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_contribution_ts: value.last_contribution_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result,
            last_burn_tx_id: value.last_burn_tx_id,
            last_burn_scan_tx_id: value.last_burn_scan_tx_id,
            burned_e8s: Some(value.burned_e8s),
        }
    }
}

impl From<StableCanisterMeta> for CanisterMeta {
    fn from(value: StableCanisterMeta) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_contribution_ts: value.last_contribution_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result,
            last_burn_tx_id: value.last_burn_tx_id,
            last_burn_scan_tx_id: value.last_burn_scan_tx_id.or(value.last_burn_tx_id),
            burned_e8s: value.burned_e8s.unwrap_or(0),
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegisteredCanisterSummary;
    use std::collections::{BTreeMap, BTreeSet};

    fn reset_test_storage() {
        with_root_stable_cell(|cell| {
            cell.set(VersionedStableState::Uninitialized)
                .expect("failed to reset historian root stable state for test");
        });
        with_canister_sources_map(|map| map.clear_new());
        with_canister_meta_map(|map| map.clear_new());
        with_contribution_history_index_map(|map| map.clear_new());
        with_contribution_entry_map(|map| map.clear_new());
        with_cycles_history_index_map(|map| map.clear_new());
        with_cycles_entry_map(|map| map.clear_new());
        PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(0));
        clear_persistence_dirty();
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn principal(bytes: &[u8]) -> Principal {
        Principal::from_slice(bytes)
    }

    fn sample_config() -> Config {
        Config {
            staking_account: Account { owner: principal(&[1]), subaccount: None },
            ledger_canister_id: principal(&[2]),
            index_canister_id: principal(&[3]),
            cmc_canister_id: Some(principal(&[4])),
            faucet_canister_id: Some(principal(&[5])),
            blackhole_canister_id: principal(&[6]),
            sns_wasm_canister_id: principal(&[7]),
            enable_sns_tracking: true,
            scan_interval_seconds: 60,
            cycles_interval_seconds: 120,
            min_tx_e8s: 100_000_000,
            max_cycles_entries_per_canister: 100,
            max_contribution_entries_per_canister: 100,
            max_index_pages_per_tick: 10,
            max_canisters_per_cycles_tick: 10,
        }
    }

    fn snapshot_sources_map() -> BTreeMap<Principal, BTreeSet<CanisterSource>> {
        with_canister_sources_map(|map| {
            let mut out = BTreeMap::new();
            for (key, value) in map.iter() {
                out.insert(key.to_principal(), value.0.clone());
            }
            out
        })
    }

    fn snapshot_meta_map() -> BTreeMap<Principal, StableCanisterMeta> {
        with_canister_meta_map(|map| {
            let mut out = BTreeMap::new();
            for (key, value) in map.iter() {
                out.insert(key.to_principal(), value.clone());
            }
            out
        })
    }

    fn snapshot_contribution_history_map() -> BTreeMap<Principal, Vec<ContributionSample>> {
        with_contribution_history_index_map(|index_map| {
            let mut out = BTreeMap::new();
            for (key, ids) in index_map.iter() {
                let canister_id = key.to_principal();
                let mut samples = Vec::new();
                for tx_id in ids.0 {
                    if let Some(sample) = with_contribution_entry_map(|entry_map| {
                        entry_map.get(&ContributionEntryKey::new(canister_id, tx_id))
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
            for (key, timestamps) in index_map.iter() {
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
        sources.insert(CanisterSource::MemoContribution);
        st.canister_sources.insert(canister_id, sources);
        st.contribution_history.insert(canister_id, vec![ContributionSample {
            tx_id: 7,
            timestamp_nanos: Some(77),
            amount_e8s: 100_000_000,
            counts_toward_faucet: true,
        }]);
        st.cycles_history.insert(canister_id, vec![CyclesSample {
            timestamp_nanos: 88,
            cycles: 123_456,
            source: CyclesSampleSource::BlackholeStatus,
        }]);
        st.per_canister_meta.insert(canister_id, CanisterMeta {
            first_seen_ts: Some(1),
            last_contribution_ts: Some(77),
            last_cycles_probe_ts: Some(88),
            last_cycles_probe_result: Some(CyclesProbeResult::Ok(CyclesSampleSource::BlackholeStatus)),
            last_burn_tx_id: Some(11),
            last_burn_scan_tx_id: Some(12),
            burned_e8s: 42,
        });
        let mut cache = BTreeMap::new();
        cache.insert(
            canister_id,
            RegisteredCanisterSummary {
                canister_id,
                sources: vec![CanisterSource::MemoContribution],
                qualifying_contribution_count: 1,
                total_qualifying_contributed_e8s: 100_000_000,
                last_contribution_ts: Some(77),
                latest_cycles: Some(123_456),
                last_cycles_probe_ts: Some(88),
            },
        );
        st.registered_canister_summaries_cache = Some(cache);
        st.registered_canister_summaries_total_desc_index = Some(vec![canister_id]);
        set_state(st);

        let restored = restore_state_from_stable().expect("expected persisted historian state");
        assert_eq!(restored.distinct_canisters.len(), 1);
        assert!(restored.contribution_history.get(&canister_id).is_none());
        assert!(restored.cycles_history.get(&canister_id).is_none());
        assert_eq!(stable_contribution_history_for(canister_id)[0].tx_id, 7);
        assert_eq!(stable_cycles_history_for(canister_id)[0].cycles, 123_456);
        assert_eq!(restored.per_canister_meta.get(&canister_id).expect("missing canister meta").burned_e8s, 42);
        assert!(restored.registered_canister_summaries_cache.is_none());
        assert!(restored.registered_canister_summaries_total_desc_index.is_none());
    }





    #[test]
    fn with_state_mut_persists_recent_feeds_to_stable_storage() {
        reset_test_storage();
        let canister_id = principal(&[10]);
        set_state(State::new(sample_config(), 6_000));

        with_state_mut(|st| {
            st.recent_invalid_contributions = Some(vec![InvalidContribution {
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

        let restored = restore_state_from_stable().expect("expected persisted historian state after mutation");
        assert_eq!(restored.main_lock_state_ts, Some(66));
        assert_eq!(restored.recent_invalid_contributions.as_ref().expect("missing invalid contributions")[0].tx_id, 12);
        assert_eq!(restored.recent_burns.as_ref().expect("missing recent burns")[0].canister_id, canister_id);
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
            let restored_mid = restore_state_from_stable().expect("expected persisted state before batch mutation");
            assert_ne!(restored_mid.last_indexed_staking_tx_id, Some(88));
            assert_ne!(restored_mid.main_lock_state_ts, Some(77));
            persist_dirty_state();
        }

        let restored = restore_state_from_stable().expect("expected persisted state after batch flush");
        assert_eq!(restored.last_indexed_staking_tx_id, Some(88));
        assert_eq!(restored.main_lock_state_ts, Some(77));
    }

    #[test]
    fn section_scoped_mutation_only_flushes_target_sections() {
        reset_test_storage();
        let canister_id = principal(&[12]);
        let mut st = State::new(sample_config(), 9_000);
        st.canister_sources.insert(canister_id, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(canister_id, vec![ContributionSample {
            tx_id: 31,
            timestamp_nanos: Some(310),
            amount_e8s: 500,
            counts_toward_faucet: true,
        }]);
        st.cycles_history.insert(canister_id, vec![CyclesSample {
            timestamp_nanos: 320,
            cycles: 600,
            source: CyclesSampleSource::SelfCanister,
        }]);
        st.per_canister_meta.insert(canister_id, CanisterMeta {
            first_seen_ts: Some(1),
            last_contribution_ts: Some(2),
            last_cycles_probe_ts: Some(3),
            last_cycles_probe_result: Some(CyclesProbeResult::Ok(CyclesSampleSource::SelfCanister)),
            last_burn_tx_id: Some(4),
            last_burn_scan_tx_id: Some(5),
            burned_e8s: 6,
        });
        set_state(st);

        let sources_before = snapshot_sources_map();
        let meta_before = snapshot_meta_map();
        let contributions_before = snapshot_contribution_history_map();
        let cycles_before = snapshot_cycles_history_map();

        with_root_state_mut(|st| {
            st.main_lock_state_ts = Some(1234);
        });

        let restored = restore_state_from_stable().expect("expected restored historian state after root-only mutation");
        assert_eq!(restored.main_lock_state_ts, Some(1234));
        assert_eq!(snapshot_sources_map(), sources_before);
        assert_eq!(snapshot_meta_map(), meta_before);
        assert_eq!(snapshot_contribution_history_map(), contributions_before);
        assert_eq!(snapshot_cycles_history_map(), cycles_before);
    }

    #[test]
    fn restore_keeps_bulk_histories_in_stable_storage_until_requested() {
        reset_test_storage();
        let canister_id = principal(&[31]);
        let mut st = State::new(sample_config(), 10_000);
        st.canister_sources.insert(canister_id, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(canister_id, vec![ContributionSample {
            tx_id: 91,
            timestamp_nanos: Some(910),
            amount_e8s: 111,
            counts_toward_faucet: true,
        }]);
        st.cycles_history.insert(canister_id, vec![CyclesSample {
            timestamp_nanos: 920,
            cycles: 222,
            source: CyclesSampleSource::SelfCanister,
        }]);
        set_state(st);

        let restored = restore_state_from_stable().expect("expected restored historian state");
        assert!(restored.contribution_history.is_empty());
        assert!(restored.cycles_history.is_empty());
        assert_eq!(stable_contribution_history_for(canister_id)[0].tx_id, 91);
        assert_eq!(stable_cycles_history_for(canister_id)[0].cycles, 222);
    }

    #[test]
    fn canister_scoped_contribution_flush_only_rewrites_target_canister_history() {
        reset_test_storage();
        let canister_a = principal(&[21]);
        let canister_b = principal(&[22]);
        let mut st = State::new(sample_config(), 9_500);
        for canister_id in [canister_a, canister_b] {
            st.canister_sources.insert(canister_id, BTreeSet::from([CanisterSource::MemoContribution]));
            st.per_canister_meta.insert(canister_id, CanisterMeta::default());
        }
        st.contribution_history.insert(
            canister_a,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(10),
                amount_e8s: 100,
                counts_toward_faucet: true,
            }],
        );
        st.contribution_history.insert(
            canister_b,
            vec![ContributionSample {
                tx_id: 2,
                timestamp_nanos: Some(20),
                amount_e8s: 200,
                counts_toward_faucet: true,
            }],
        );
        set_state(st);

        let contributions_before = snapshot_contribution_history_map();
        assert_eq!(contributions_before.get(&canister_b).unwrap()[0].tx_id, 2);

        with_root_registry_and_contributions_canister_state_mut(canister_a, |st| {
            st.contribution_history.get_mut(&canister_a).unwrap().push(ContributionSample {
                tx_id: 3,
                timestamp_nanos: Some(30),
                amount_e8s: 300,
                counts_toward_faucet: true,
            });
            st.per_canister_meta.entry(canister_a).or_default().last_contribution_ts = Some(30);
        });

        let contributions_after = snapshot_contribution_history_map();
        assert_eq!(contributions_after.get(&canister_a).unwrap().len(), 2);
        assert_eq!(contributions_after.get(&canister_a).unwrap()[1].tx_id, 3);
        assert_eq!(contributions_after.get(&canister_b), contributions_before.get(&canister_b));
    }

}
