mod clients;
mod logic;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;
use std::collections::BTreeSet;

use crate::state::{CanisterMeta, CanisterSource, Config, ContributionSample, CyclesSample, State};

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub staking_account: Account,
    pub ledger_canister_id: Option<Principal>,
    pub index_canister_id: Option<Principal>,
    pub blackhole_canister_id: Option<Principal>,
    pub sns_wasm_canister_id: Option<Principal>,
    pub enable_sns_tracking: Option<bool>,
    pub scan_interval_seconds: Option<u64>,
    pub cycles_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
    pub max_cycles_entries_per_canister: Option<u32>,
    pub max_contribution_entries_per_canister: Option<u32>,
    pub max_index_pages_per_tick: Option<u32>,
    pub max_canisters_per_cycles_tick: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct UpgradeArgs {
    pub enable_sns_tracking: Option<bool>,
    pub scan_interval_seconds: Option<u64>,
    pub cycles_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
    pub max_cycles_entries_per_canister: Option<u32>,
    pub max_contribution_entries_per_canister: Option<u32>,
    pub max_index_pages_per_tick: Option<u32>,
    pub max_canisters_per_cycles_tick: Option<u32>,
    pub blackhole_canister_id: Option<Principal>,
    pub sns_wasm_canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListCanistersArgs {
    pub start_after: Option<Principal>,
    pub limit: Option<u32>,
    pub source_filter: Option<CanisterSource>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterListItem {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListCanistersResponse {
    pub items: Vec<CanisterListItem>,
    pub next_start_after: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct GetCyclesHistoryArgs {
    pub canister_id: Principal,
    pub start_after_ts: Option<u64>,
    pub limit: Option<u32>,
    pub descending: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct GetContributionHistoryArgs {
    pub canister_id: Principal,
    pub start_after_tx_id: Option<u64>,
    pub limit: Option<u32>,
    pub descending: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CyclesHistoryPage {
    pub items: Vec<CyclesSample>,
    pub next_start_after_ts: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ContributionHistoryPage {
    pub items: Vec<ContributionSample>,
    pub next_start_after_tx_id: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterOverview {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
    pub meta: CanisterMeta,
    pub cycles_points: u32,
    pub contribution_points: u32,
}

fn mainnet_ledger_id() -> Principal {
    Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid hardcoded ledger principal")
}

fn mainnet_index_id() -> Principal {
    Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").expect("invalid hardcoded index principal")
}

fn mainnet_blackhole_id() -> Principal {
    Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").expect("invalid hardcoded blackhole principal")
}

fn mainnet_sns_wasm_id() -> Principal {
    Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("invalid hardcoded sns-wasm principal")
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let cfg = Config {
        staking_account: args.staking_account,
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        index_canister_id: args.index_canister_id.unwrap_or_else(mainnet_index_id),
        blackhole_canister_id: args.blackhole_canister_id.unwrap_or_else(mainnet_blackhole_id),
        sns_wasm_canister_id: args.sns_wasm_canister_id.unwrap_or_else(mainnet_sns_wasm_id),
        enable_sns_tracking: args.enable_sns_tracking.unwrap_or(false),
        scan_interval_seconds: args.scan_interval_seconds.unwrap_or(10 * 60),
        cycles_interval_seconds: args.cycles_interval_seconds.unwrap_or(7 * 24 * 60 * 60),
        min_tx_e8s: args.min_tx_e8s.unwrap_or(10_000_000),
        max_cycles_entries_per_canister: args.max_cycles_entries_per_canister.unwrap_or(100),
        max_contribution_entries_per_canister: args.max_contribution_entries_per_canister.unwrap_or(100),
        max_index_pages_per_tick: args.max_index_pages_per_tick.unwrap_or(10),
        max_canisters_per_cycles_tick: args.max_canisters_per_cycles_tick.unwrap_or(25),
    };
    state::set_state(State::new(cfg, now_secs));
    scheduler::install_timers();
}

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    let st = state::get_state();
    ic_cdk::storage::stable_save((st,)).expect("stable_save failed");
}

fn apply_upgrade_args(st: &mut State, args: Option<UpgradeArgs>) {
    if let Some(args) = args {
        if let Some(v) = args.enable_sns_tracking { st.config.enable_sns_tracking = v; }
        if let Some(v) = args.scan_interval_seconds { st.config.scan_interval_seconds = v; }
        if let Some(v) = args.cycles_interval_seconds { st.config.cycles_interval_seconds = v; }
        if let Some(v) = args.min_tx_e8s { st.config.min_tx_e8s = v; }
        if let Some(v) = args.max_cycles_entries_per_canister { st.config.max_cycles_entries_per_canister = v; }
        if let Some(v) = args.max_contribution_entries_per_canister { st.config.max_contribution_entries_per_canister = v; }
        if let Some(v) = args.max_index_pages_per_tick { st.config.max_index_pages_per_tick = v; }
        if let Some(v) = args.max_canisters_per_cycles_tick { st.config.max_canisters_per_cycles_tick = v; }
        if let Some(v) = args.blackhole_canister_id { st.config.blackhole_canister_id = v; }
        if let Some(v) = args.sns_wasm_canister_id { st.config.sns_wasm_canister_id = v; }
    }
    st.main_lock_expires_at_ts = Some(0);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(args: Option<UpgradeArgs>) {
    let (mut st,): (State,) = ic_cdk::storage::stable_restore().expect("stable_restore failed");
    apply_upgrade_args(&mut st, args);
    state::set_state(st);
    scheduler::install_timers();
}

#[ic_cdk::query]
fn list_canisters(args: ListCanistersArgs) -> ListCanistersResponse {
    state::with_state(|st| {
        let limit = args.limit.unwrap_or(50).max(1) as usize;
        let mut items = Vec::new();
        let mut next = None;
        let mut started = args.start_after.is_none();
        for canister_id in st.distinct_canisters.iter().copied() {
            if !started {
                if Some(canister_id) == args.start_after {
                    started = true;
                }
                continue;
            }
            let sources: BTreeSet<CanisterSource> = st.canister_sources.get(&canister_id).cloned().unwrap_or_default();
            if let Some(filter) = &args.source_filter {
                if !sources.contains(filter) {
                    continue;
                }
            }
            if items.len() >= limit {
                next = Some(canister_id);
                break;
            }
            items.push(CanisterListItem { canister_id, sources: sources.into_iter().collect() });
        }
        ListCanistersResponse { items, next_start_after: next }
    })
}

#[ic_cdk::query]
fn get_cycles_history(args: GetCyclesHistoryArgs) -> CyclesHistoryPage {
    state::with_state(|st| {
        let history = st.cycles_history.get(&args.canister_id).cloned().unwrap_or_default();
        let descending = args.descending.unwrap_or(false);
        let limit = args.limit.unwrap_or(100).max(1) as usize;
        let mut filtered: Vec<_> = history.into_iter().filter(|item| match args.start_after_ts {
            Some(ts) if descending => item.timestamp_nanos < ts,
            Some(ts) => item.timestamp_nanos > ts,
            None => true,
        }).collect();
        if descending { filtered.reverse(); }
        let next = filtered.get(limit).map(|item| item.timestamp_nanos);
        filtered.truncate(limit);
        CyclesHistoryPage { items: filtered, next_start_after_ts: next }
    })
}

#[ic_cdk::query]
fn get_contribution_history(args: GetContributionHistoryArgs) -> ContributionHistoryPage {
    state::with_state(|st| {
        let history = st.contribution_history.get(&args.canister_id).cloned().unwrap_or_default();
        let descending = args.descending.unwrap_or(false);
        let limit = args.limit.unwrap_or(100).max(1) as usize;
        let mut filtered: Vec<_> = history.into_iter().filter(|item| match args.start_after_tx_id {
            Some(tx_id) if descending => item.tx_id < tx_id,
            Some(tx_id) => item.tx_id > tx_id,
            None => true,
        }).collect();
        if descending { filtered.reverse(); }
        let next = filtered.get(limit).map(|item| item.tx_id);
        filtered.truncate(limit);
        ContributionHistoryPage { items: filtered, next_start_after_tx_id: next }
    })
}

#[ic_cdk::query]
fn get_canister_overview(canister_id: Principal) -> Option<CanisterOverview> {
    state::with_state(|st| {
        let sources = st.canister_sources.get(&canister_id)?.clone().into_iter().collect();
        let meta = st.per_canister_meta.get(&canister_id).cloned().unwrap_or_default();
        let cycles_points = st.cycles_history.get(&canister_id).map(|v| v.len() as u32).unwrap_or(0);
        let contribution_points = st.contribution_history.get(&canister_id).map(|v| v.len() as u32).unwrap_or(0);
        Some(CanisterOverview { canister_id, sources, meta, cycles_points, contribution_points })
    })
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub distinct_canister_count: u32,
    pub last_indexed_staking_tx_id: Option<u64>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    pub active_cycles_sweep_present: bool,
    pub active_cycles_sweep_next_index: Option<u64>,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    state::with_state(|st| DebugState {
        distinct_canister_count: st.distinct_canisters.len() as u32,
        last_indexed_staking_tx_id: st.last_indexed_staking_tx_id,
        last_sns_discovery_ts: st.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: st.last_completed_cycles_sweep_ts,
        active_cycles_sweep_present: st.active_cycles_sweep.is_some(),
        active_cycles_sweep_next_index: st.active_cycles_sweep.as_ref().map(|s| s.next_index),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_driver_tick() {
    scheduler::main_tick(true).await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_completed_cycles_sweep_ts(ts: Option<u64>) {
    state::with_state_mut(|st| st.last_completed_cycles_sweep_ts = ts.unwrap_or(0));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_sns_discovery_ts(ts: Option<u64>) {
    state::with_state_mut(|st| st.last_sns_discovery_ts = ts.unwrap_or(0));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_indexed_staking_tx_id(tx_id: Option<u64>) {
    state::with_state_mut(|st| st.last_indexed_staking_tx_id = tx_id);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_reset_runtime_state() {
    state::with_state_mut(|st| {
        st.active_cycles_sweep = None;
        st.main_lock_expires_at_ts = Some(0);
        st.last_main_run_ts = 0;
    });
}
