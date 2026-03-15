use candid::{CandidType, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Config {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
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
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ActiveCyclesSweep {
    pub started_at_ts_nanos: u64,
    pub canisters: Vec<Principal>,
    pub next_index: u64,
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
