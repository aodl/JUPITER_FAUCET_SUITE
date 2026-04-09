use candid::{CandidType, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableCell, Storable,
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
pub struct StableState {
    pub config: StableConfig,
    pub distinct_canisters: BTreeSet<Principal>,
    pub canister_sources: BTreeMap<Principal, BTreeSet<CanisterSource>>,
    pub contribution_history: BTreeMap<Principal, Vec<ContributionSample>>,
    pub cycles_history: BTreeMap<Principal, Vec<CyclesSample>>,
    pub per_canister_meta: BTreeMap<Principal, StableCanisterMeta>,
    #[serde(default)]
    pub registered_canister_summaries_cache: Option<BTreeMap<Principal, crate::RegisteredCanisterSummary>>,
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
    V1(StableState),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode historian stable state"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode historian stable state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        std::cell::RefCell::new(None);
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
    #[cfg(test)]
    static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    #[cfg(test)]
    static PERSISTENCE_DIRTY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn with_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized)
                    .expect("failed to initialize historian stable cell");
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("historian stable cell not initialized"))
    })
}

fn persist_snapshot(st: &State) {
    with_stable_cell(|cell| {
        cell.set(VersionedStableState::V1(st.clone().into()))
            .expect("failed to persist historian stable state");
    });
}

pub fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub fn restore_state_from_stable() -> Option<State> {
    with_stable_cell(|cell| match cell.get().clone() {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::V1(st) => Some(st.into()),
    })
}

pub fn set_state(st: State) {
    persist_snapshot(&st);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub fn get_state() -> State {
    STATE.with(|s| s.borrow().clone()).expect("state not initialized")
}

pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

#[cfg(test)]
fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.get() > 0)
}

#[cfg(not(test))]
fn persistence_batch_active() -> bool {
    false
}

#[cfg(test)]
fn mark_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(true));
}

#[cfg(not(test))]
fn mark_persistence_dirty() {}

#[cfg(test)]
fn clear_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
}

#[cfg(not(test))]
fn clear_persistence_dirty() {}

#[cfg(test)]
pub fn persist_dirty_state() {
    let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
    if !dirty {
        return;
    }
    let snapshot = get_state();
    persist_snapshot(&snapshot);
    clear_persistence_dirty();
}

#[cfg(test)]
/// A synchronous persistence-batch guard.
///
/// Do not hold this guard across an `await` point. While it is live, mutations are
/// only marked dirty and are not durably flushed until the batch ends or an
/// explicit `persist_dirty_state()` call occurs.
pub struct PersistenceBatch {
    active: bool,
}

#[cfg(test)]
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

#[cfg(test)]
#[must_use]
pub fn begin_persistence_batch() -> PersistenceBatch {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    PersistenceBatch { active: true }
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        let immediate_persist = !persistence_batch_active();
        let out = f(st);
        if immediate_persist {
            let snapshot = st.clone();
            drop(borrow);
            persist_snapshot(&snapshot);
            return out;
        }
        mark_persistence_dirty();
        drop(borrow);
        out
    })
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

impl From<State> for StableState {
    fn from(value: State) -> Self {
        Self {
            config: value.config.into(),
            distinct_canisters: value.distinct_canisters,
            canister_sources: value.canister_sources,
            contribution_history: value.contribution_history,
            cycles_history: value.cycles_history,
            per_canister_meta: value
                .per_canister_meta
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            registered_canister_summaries_cache: value.registered_canister_summaries_cache,
            last_indexed_staking_tx_id: value.last_indexed_staking_tx_id,
            last_sns_discovery_ts: value.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: value.last_completed_cycles_sweep_ts,
            active_cycles_sweep: value.active_cycles_sweep,
            active_sns_discovery: value.active_sns_discovery,
            main_lock_state_ts: value.main_lock_state_ts,
            last_main_run_ts: value.last_main_run_ts,
            qualifying_contribution_count: value.qualifying_contribution_count,
            icp_burned_e8s: value.icp_burned_e8s,
            recent_contributions: value.recent_contributions,
            recent_under_threshold_contributions: value.recent_under_threshold_contributions,
            recent_invalid_contributions: value.recent_invalid_contributions,
            recent_burns: value.recent_burns,
            last_index_run_ts: value.last_index_run_ts,
        }
    }
}

impl From<StableState> for State {
    fn from(value: StableState) -> Self {
        Self {
            config: value.config.into(),
            distinct_canisters: value.distinct_canisters,
            canister_sources: value.canister_sources,
            contribution_history: value.contribution_history,
            cycles_history: value.cycles_history,
            per_canister_meta: value
                .per_canister_meta
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            registered_canister_summaries_cache: value.registered_canister_summaries_cache,
            last_indexed_staking_tx_id: value.last_indexed_staking_tx_id,
            last_sns_discovery_ts: value.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: value.last_completed_cycles_sweep_ts,
            active_cycles_sweep: value.active_cycles_sweep,
            active_sns_discovery: value.active_sns_discovery,
            main_lock_state_ts: value.main_lock_state_ts,
            last_main_run_ts: value.last_main_run_ts,
            qualifying_contribution_count: value.qualifying_contribution_count,
            icp_burned_e8s: value.icp_burned_e8s,
            recent_contributions: value.recent_contributions,
            recent_under_threshold_contributions: value.recent_under_threshold_contributions,
            recent_invalid_contributions: value.recent_invalid_contributions,
            recent_burns: value.recent_burns,
            last_index_run_ts: value.last_index_run_ts,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegisteredCanisterSummary;
    use std::collections::{BTreeMap, BTreeSet};

    fn reset_test_storage() {
        with_stable_cell(|cell| {
            cell.set(VersionedStableState::Uninitialized)
                .expect("failed to reset historian stable state for test");
        });
        PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(0));
        PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
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

    #[test]
    fn stable_restore_is_none_before_first_persist() {
        reset_test_storage();
        assert!(restore_state_from_stable().is_none());
    }

    #[test]
    fn set_state_round_trips_histories_and_cache_through_stable_storage() {
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
        set_state(st);

        let restored = restore_state_from_stable().expect("expected persisted historian state");
        assert_eq!(restored.distinct_canisters.len(), 1);
        assert_eq!(restored.contribution_history.get(&canister_id).expect("missing contribution history")[0].tx_id, 7);
        assert_eq!(restored.cycles_history.get(&canister_id).expect("missing cycles history")[0].cycles, 123_456);
        assert_eq!(restored.per_canister_meta.get(&canister_id).expect("missing canister meta").burned_e8s, 42);
        assert_eq!(restored.registered_canister_summaries_cache.as_ref().and_then(|m| m.get(&canister_id)).expect("missing registered canister summary").latest_cycles, Some(123_456));
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
}
