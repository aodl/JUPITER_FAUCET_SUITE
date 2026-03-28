use candid::{CandidType, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub tx_hash: Option<String>,
    pub memo_text: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RecentContribution {
    pub canister_id: Principal,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
    #[serde(default)]
    pub tx_hash: Option<String>,
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
    pub burned_e8s: u64,
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
    pub last_indexed_staking_tx_id: Option<u64>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    pub active_cycles_sweep: Option<ActiveCyclesSweep>,
    pub main_lock_expires_at_ts: Option<u64>,
    pub last_main_run_ts: u64,
    #[serde(default)]
    pub qualifying_contribution_count: Option<u64>,
    #[serde(default)]
    pub icp_burned_e8s: Option<u64>,
    #[serde(default)]
    pub recent_contributions: Option<Vec<RecentContribution>>,
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
    pub last_indexed_staking_tx_id: Option<u64>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    pub active_cycles_sweep: Option<ActiveCyclesSweep>,
    pub main_lock_expires_at_ts: Option<u64>,
    pub last_main_run_ts: u64,
    pub qualifying_contribution_count: Option<u64>,
    pub icp_burned_e8s: Option<u64>,
    pub recent_contributions: Option<Vec<RecentContribution>>,
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
            last_indexed_staking_tx_id: None,
            last_sns_discovery_ts: 0,
            last_completed_cycles_sweep_ts: 0,
            active_cycles_sweep: None,
            main_lock_expires_at_ts: Some(0),
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
            qualifying_contribution_count: Some(0),
            icp_burned_e8s: Some(0),
            recent_contributions: Some(Vec::new()),
            recent_invalid_contributions: Some(Vec::new()),
            recent_burns: Some(Vec::new()),
            last_index_run_ts: Some(0),
        }
    }
}

thread_local! {
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
}

pub fn set_state(st: State) { STATE.with(|s| *s.borrow_mut() = Some(st)); }
pub fn get_state() -> State { STATE.with(|s| s.borrow().clone()).expect("state not initialized") }
pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R { STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized"))) }
pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R { STATE.with(|s| f(s.borrow_mut().as_mut().expect("state not initialized"))) }


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
            last_indexed_staking_tx_id: value.last_indexed_staking_tx_id,
            last_sns_discovery_ts: value.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: value.last_completed_cycles_sweep_ts,
            active_cycles_sweep: value.active_cycles_sweep,
            main_lock_expires_at_ts: value.main_lock_expires_at_ts,
            last_main_run_ts: value.last_main_run_ts,
            qualifying_contribution_count: value.qualifying_contribution_count,
            icp_burned_e8s: value.icp_burned_e8s,
            recent_contributions: value.recent_contributions,
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
            last_indexed_staking_tx_id: value.last_indexed_staking_tx_id,
            last_sns_discovery_ts: value.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: value.last_completed_cycles_sweep_ts,
            active_cycles_sweep: value.active_cycles_sweep,
            main_lock_expires_at_ts: value.main_lock_expires_at_ts,
            last_main_run_ts: value.last_main_run_ts,
            qualifying_contribution_count: value.qualifying_contribution_count,
            icp_burned_e8s: value.icp_burned_e8s,
            recent_contributions: value.recent_contributions,
            recent_invalid_contributions: value.recent_invalid_contributions,
            recent_burns: value.recent_burns,
            last_index_run_ts: value.last_index_run_ts,
        }
    }
}
