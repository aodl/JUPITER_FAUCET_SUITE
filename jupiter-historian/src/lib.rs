mod clients;
mod logic;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::state::{
    CanisterMeta, CanisterSource, Config, ContributionSample, CyclesSample, InvalidContribution,
    RecentBurn, RecentContribution, State,
};

pub(crate) const MAX_PUBLIC_QUERY_LIMIT: u32 = 100;
pub(crate) const MAX_RECENT_QUALIFYING_CONTRIBUTIONS: usize = 500;
pub(crate) const MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS: usize = 100;
pub(crate) const MAX_RECENT_INVALID_CONTRIBUTIONS: usize = 100;
pub(crate) const MAX_RECENT_BURNS: usize = 500;
pub(crate) const MAX_CONTRIBUTION_ENTRIES_PER_CANISTER_HARD_CAP: u32 = 250;
pub(crate) const MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP: u32 = 250;
pub(crate) const MAX_INDEX_PAGES_PER_TICK_HARD_CAP: u32 = 100;
pub(crate) const MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP: u32 = 500;


pub(crate) const MIN_MIN_TX_E8S: u64 = 10_000_000;

fn assert_non_anonymous_principal(name: &str, principal: Principal) {
    assert!(principal != Principal::anonymous(), "{name} must not be the anonymous principal");
}

fn validate_config(cfg: &Config) {
    assert_non_anonymous_principal("staking_account.owner", cfg.staking_account.owner);
    assert_non_anonymous_principal("ledger_canister_id", cfg.ledger_canister_id);
    assert_non_anonymous_principal("index_canister_id", cfg.index_canister_id);
    assert_non_anonymous_principal("blackhole_canister_id", cfg.blackhole_canister_id);
    assert_non_anonymous_principal("sns_wasm_canister_id", cfg.sns_wasm_canister_id);
    if let Some(cmc_canister_id) = cfg.cmc_canister_id {
        assert_non_anonymous_principal("cmc_canister_id", cmc_canister_id);
    }
    if let Some(faucet_canister_id) = cfg.faucet_canister_id {
        assert_non_anonymous_principal("faucet_canister_id", faucet_canister_id);
    }
    assert!(cfg.scan_interval_seconds > 0, "scan_interval_seconds must be greater than 0");
    assert!(cfg.cycles_interval_seconds > 0, "cycles_interval_seconds must be greater than 0");
    assert!(cfg.min_tx_e8s >= MIN_MIN_TX_E8S, "min_tx_e8s must be at least {MIN_MIN_TX_E8S} e8s (0.1 ICP)");
}

fn contribution_sort_key(item: &RecentContribution) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn invalid_contribution_sort_key(item: &InvalidContribution) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn burn_sort_key(item: &RecentBurn) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn clamp_public_limit(limit: Option<u32>, default: u32) -> usize {
    limit.unwrap_or(default).clamp(1, MAX_PUBLIC_QUERY_LIMIT) as usize
}

fn clamp_cycles_entries_per_canister(value: u32) -> u32 {
    value.clamp(1, MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP)
}

fn clamp_contribution_entries_per_canister(value: u32) -> u32 {
    value.clamp(1, MAX_CONTRIBUTION_ENTRIES_PER_CANISTER_HARD_CAP)
}

fn clamp_index_pages_per_tick(value: u32) -> u32 {
    value.clamp(1, MAX_INDEX_PAGES_PER_TICK_HARD_CAP)
}

fn clamp_canisters_per_cycles_tick(value: u32) -> u32 {
    value.clamp(1, MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP)
}

#[cfg(target_arch = "wasm32")]
fn allocated_heap_memory_bytes() -> u64 {
    (core::arch::wasm32::memory_size(0) as u64) * 65_536
}

#[cfg(not(target_arch = "wasm32"))]
fn allocated_heap_memory_bytes() -> u64 {
    0
}

#[cfg(target_arch = "wasm32")]
fn allocated_stable_memory_bytes() -> u64 {
    ic_cdk::stable::stable_size().saturating_mul(65_536)
}

#[cfg(not(target_arch = "wasm32"))]
fn allocated_stable_memory_bytes() -> u64 {
    0
}

fn normalize_recent_contribution_bucket(items: &mut Vec<RecentContribution>, counts_toward_faucet: bool, max_entries: usize) {
    items.retain(|item| item.counts_toward_faucet == counts_toward_faucet);
    items.sort_by(|a, b| contribution_sort_key(b).cmp(&contribution_sort_key(a)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(max_entries);
}

fn normalize_recent_invalid_contributions(items: &mut Vec<InvalidContribution>) {
    items.sort_by(|a, b| invalid_contribution_sort_key(b).cmp(&invalid_contribution_sort_key(a)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(MAX_RECENT_INVALID_CONTRIBUTIONS);
}

fn normalize_recent_burns(items: &mut Vec<RecentBurn>) {
    items.sort_by(|a, b| burn_sort_key(b).cmp(&burn_sort_key(a)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(MAX_RECENT_BURNS);
}

fn memo_source_is_registered(st: &State, canister_id: &Principal, sources: &BTreeSet<CanisterSource>) -> bool {
    sources.contains(&CanisterSource::MemoContribution)
        && st
            .contribution_history
            .get(canister_id)
            .map(|history| history.iter().any(|item| item.counts_toward_faucet))
            .unwrap_or(false)
}

fn visible_sources_for_canister(st: &State, canister_id: &Principal) -> Option<BTreeSet<CanisterSource>> {
    let mut sources = st.canister_sources.get(canister_id)?.clone();
    if !memo_source_is_registered(st, canister_id, &sources) {
        sources.remove(&CanisterSource::MemoContribution);
    }
    if sources.is_empty() {
        return None;
    }
    Some(sources)
}



fn clamp_config(st: &mut State) {
    st.config.max_cycles_entries_per_canister =
        clamp_cycles_entries_per_canister(st.config.max_cycles_entries_per_canister);
    st.config.max_contribution_entries_per_canister =
        clamp_contribution_entries_per_canister(st.config.max_contribution_entries_per_canister);
    st.config.max_index_pages_per_tick = clamp_index_pages_per_tick(st.config.max_index_pages_per_tick);
    st.config.max_canisters_per_cycles_tick =
        clamp_canisters_per_cycles_tick(st.config.max_canisters_per_cycles_tick);
}

fn normalize_runtime_state(st: &mut State) {
    clamp_config(st);

    let mut recent_contributions = st.recent_contributions.take().unwrap_or_default();
    recent_contributions.extend(fallback_recent_qualifying_contributions_state(st));
    let mut recent_under_threshold = st.recent_under_threshold_contributions.take().unwrap_or_default();
    recent_under_threshold.extend(fallback_recent_under_threshold_contributions_state(st));

    for item in recent_contributions.iter().filter(|item| !item.counts_toward_faucet).cloned() {
        recent_under_threshold.push(item);
    }
    recent_contributions.retain(|item| item.counts_toward_faucet);

    let mut empty_histories = Vec::new();
    for (canister_id, history) in st.contribution_history.iter_mut() {
        let mut removed = Vec::new();
        history.retain(|item| {
            if item.counts_toward_faucet {
                true
            } else {
                removed.push(RecentContribution {
                    canister_id: *canister_id,
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                });
                false
            }
        });
        recent_under_threshold.extend(removed);
        if history.len() > st.config.max_contribution_entries_per_canister as usize {
            let excess = history.len() - st.config.max_contribution_entries_per_canister as usize;
            history.drain(0..excess);
        }
        if history.is_empty() {
            empty_histories.push(*canister_id);
        }
    }
    for canister_id in empty_histories {
        st.contribution_history.remove(&canister_id);
    }

    let stale_memo_only_canisters: Vec<_> = st
        .canister_sources
        .iter()
        .filter_map(|(canister_id, sources)| {
            if sources.contains(&CanisterSource::MemoContribution)
                && !memo_source_is_registered(st, canister_id, sources)
            {
                Some(*canister_id)
            } else {
                None
            }
        })
        .collect();
    for canister_id in stale_memo_only_canisters {
        let remove_entry = if let Some(sources) = st.canister_sources.get_mut(&canister_id) {
            sources.remove(&CanisterSource::MemoContribution);
            sources.is_empty()
        } else {
            false
        };
        if remove_entry {
            st.canister_sources.remove(&canister_id);
            st.cycles_history.remove(&canister_id);
            st.per_canister_meta.remove(&canister_id);
        }
    }

    normalize_recent_contribution_bucket(&mut recent_contributions, true, MAX_RECENT_QUALIFYING_CONTRIBUTIONS);
    normalize_recent_contribution_bucket(&mut recent_under_threshold, false, MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS);
    st.recent_contributions = Some(recent_contributions);
    st.recent_under_threshold_contributions = Some(recent_under_threshold);

    let mut recent_invalid = st.recent_invalid_contributions.take().unwrap_or_default();
    normalize_recent_invalid_contributions(&mut recent_invalid);
    st.recent_invalid_contributions = Some(recent_invalid);

    let mut recent_burns = st.recent_burns.take().unwrap_or_default();
    recent_burns.extend(fallback_recent_burns_state(st));
    normalize_recent_burns(&mut recent_burns);
    st.recent_burns = Some(recent_burns);

    st.qualifying_contribution_count = Some(fallback_qualifying_contribution_count(st));

    let contribution_last_ts: BTreeMap<_, _> = st
        .contribution_history
        .iter()
        .map(|(canister_id, history)| {
            (
                *canister_id,
                history
                    .iter()
                    .filter_map(|item| item.timestamp_nanos.map(|ts| ts / 1_000_000_000))
                    .max(),
            )
        })
        .collect();
    for (canister_id, meta) in st.per_canister_meta.iter_mut() {
        meta.last_contribution_ts = contribution_last_ts.get(canister_id).copied().flatten();
    }

    let distinct_canisters: BTreeSet<_> = st
        .canister_sources
        .keys()
        .copied()
        .chain(st.contribution_history.keys().copied())
        .chain(st.cycles_history.keys().copied())
        .collect();
    st.distinct_canisters = distinct_canisters;
    rebuild_registered_canister_summaries_cache(st);
}

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub staking_account: Account,
    pub ledger_canister_id: Option<Principal>,
    pub index_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub faucet_canister_id: Option<Principal>,
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
    pub cmc_canister_id: Option<Principal>,
    pub faucet_canister_id: Option<Principal>,
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

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicCounts {
    pub registered_canister_count: u64,
    pub qualifying_contribution_count: u64,
    pub icp_burned_e8s: u64,
    pub sns_discovered_canister_count: u64,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicStatus {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub last_index_run_ts: Option<u64>,
    pub index_interval_seconds: u64,
    pub last_completed_cycles_sweep_ts: Option<u64>,
    pub cycles_interval_seconds: u64,
    pub heap_memory_bytes: Option<u64>,
    pub stable_memory_bytes: Option<u64>,
    pub total_memory_bytes: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone)]
pub enum RegisteredCanisterSummarySort {
    CanisterIdAsc,
    LastContributionDesc,
    QualifyingContributionCountDesc,
    TotalQualifyingContributedDesc,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRegisteredCanisterSummariesArgs {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub sort: Option<RegisteredCanisterSummarySort>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RegisteredCanisterSummary {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
    pub qualifying_contribution_count: u64,
    pub total_qualifying_contributed_e8s: u64,
    pub last_contribution_ts: Option<u64>,
    pub latest_cycles: Option<u128>,
    pub last_cycles_probe_ts: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRegisteredCanisterSummariesResponse {
    pub items: Vec<RegisteredCanisterSummary>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRecentContributionsArgs {
    pub limit: Option<u32>,
    pub qualifying_only: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone, Serialize, Debug, PartialEq, Eq)]
pub enum RecentContributionOutcomeCategory {
    QualifyingContribution,
    UnderThresholdContribution,
    InvalidTargetMemo,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RecentContributionListItem {
    pub canister_id: Option<Principal>,
    pub memo_text: Option<String>,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
    pub outcome_category: RecentContributionOutcomeCategory,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRecentContributionsResponse {
    pub items: Vec<RecentContributionListItem>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRecentBurnsArgs {
    pub limit: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RecentBurnListItem {
    pub canister_id: Principal,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRecentBurnsResponse {
    pub items: Vec<RecentBurnListItem>,
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

pub(crate) fn mainnet_cmc_id() -> Principal {
    Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").expect("invalid hardcoded cmc principal")
}

pub(crate) fn mainnet_faucet_id() -> Principal {
    Principal::from_text("acjuz-liaaa-aaaar-qb4qq-cai").expect("invalid hardcoded faucet principal")
}

fn mainnet_sns_wasm_id() -> Principal {
    Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("invalid hardcoded sns-wasm principal")
}

#[cfg(any(test, feature = "debug_api"))]
fn production_canister_id() -> Principal {
    Principal::from_text(env!("JUPITER_HISTORIAN_PROD_CANISTER_ID")).expect("invalid embedded production canister principal")
}

#[cfg(any(test, feature = "debug_api"))]
fn is_production_canister(principal: Principal) -> bool {
    principal == production_canister_id()
}

#[cfg(feature = "debug_api")]
fn guard_debug_api_not_production() {
    if is_production_canister(ic_cdk::api::canister_self()) {
        ic_cdk::trap("debug_api is disabled for the production canister");
    }
}

fn config_from_init_args(args: InitArgs) -> Config {
    let cfg = Config {
        staking_account: args.staking_account,
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        index_canister_id: args.index_canister_id.unwrap_or_else(mainnet_index_id),
        cmc_canister_id: Some(args.cmc_canister_id.unwrap_or_else(mainnet_cmc_id)),
        faucet_canister_id: Some(args.faucet_canister_id.unwrap_or_else(mainnet_faucet_id)),
        blackhole_canister_id: args.blackhole_canister_id.unwrap_or_else(mainnet_blackhole_id),
        sns_wasm_canister_id: args.sns_wasm_canister_id.unwrap_or_else(mainnet_sns_wasm_id),
        enable_sns_tracking: args.enable_sns_tracking.unwrap_or(false),
        scan_interval_seconds: args.scan_interval_seconds.unwrap_or(10 * 60),
        cycles_interval_seconds: args.cycles_interval_seconds.unwrap_or(7 * 24 * 60 * 60),
        min_tx_e8s: args.min_tx_e8s.unwrap_or(100_000_000),
        max_cycles_entries_per_canister: clamp_cycles_entries_per_canister(args.max_cycles_entries_per_canister.unwrap_or(100)),
        max_contribution_entries_per_canister: clamp_contribution_entries_per_canister(args.max_contribution_entries_per_canister.unwrap_or(100)),
        max_index_pages_per_tick: clamp_index_pages_per_tick(args.max_index_pages_per_tick.unwrap_or(10)),
        max_canisters_per_cycles_tick: clamp_canisters_per_cycles_tick(args.max_canisters_per_cycles_tick.unwrap_or(25)),
    };
    validate_config(&cfg);
    cfg
}

fn count_registered_canisters(st: &State) -> u64 {
    st.canister_sources
        .iter()
        .filter(|(canister_id, sources)| memo_source_is_registered(st, canister_id, sources))
        .count() as u64
}

fn count_sns_discovered_canisters(st: &State) -> u64 {
    st.canister_sources
        .values()
        .filter(|sources| sources.contains(&CanisterSource::SnsDiscovery))
        .count() as u64
}


fn effective_faucet_canister_id(st: &State) -> Principal {
    st.config.faucet_canister_id.clone().unwrap_or_else(mainnet_faucet_id)
}

pub(crate) fn burn_target_canisters(st: &State) -> BTreeSet<Principal> {
    let mut out: BTreeSet<Principal> = st
        .canister_sources
        .iter()
        .filter(|(canister_id, sources)| memo_source_is_registered(st, canister_id, sources))
        .map(|(canister_id, _)| *canister_id)
        .collect();
    for (canister_id, meta) in st.per_canister_meta.iter() {
        if meta.last_burn_tx_id.is_some() || meta.burned_e8s > 0 {
            out.insert(*canister_id);
        }
    }
    out.insert(effective_faucet_canister_id(st));
    out
}

fn qualifying_rollup(history: &[ContributionSample]) -> (u64, u64, Option<u64>) {
    let mut count = 0u64;
    let mut total = 0u64;
    let mut last_ts = None;
    for item in history.iter().filter(|item| item.counts_toward_faucet) {
        count = count.saturating_add(1);
        total = total.saturating_add(item.amount_e8s);
        last_ts = last_ts.max(item.timestamp_nanos.map(|ts| ts / 1_000_000_000));
    }
    (count, total, last_ts)
}

fn latest_cycles(history: &[CyclesSample]) -> Option<u128> {
    history.iter().max_by_key(|item| item.timestamp_nanos).map(|item| item.cycles)
}

fn fallback_qualifying_contribution_count(st: &State) -> u64 {
    st.contribution_history
        .values()
        .flat_map(|history| history.iter())
        .filter(|item| item.counts_toward_faucet)
        .count() as u64
}

fn fallback_icp_burned_e8s(st: &State) -> u64 {
    st.per_canister_meta
        .values()
        .fold(0u64, |acc, meta| acc.saturating_add(meta.burned_e8s))
}

fn fallback_recent_qualifying_contributions_state(st: &State) -> Vec<RecentContribution> {
    let mut items: Vec<_> = st
        .contribution_history
        .iter()
        .flat_map(|(canister_id, history)| {
            history
                .iter()
                .filter(|item| item.counts_toward_faucet)
                .cloned()
                .map(|item| RecentContribution {
                    canister_id: *canister_id,
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                })
        })
        .collect();
    normalize_recent_contribution_bucket(&mut items, true, MAX_RECENT_QUALIFYING_CONTRIBUTIONS);
    items
}

fn fallback_recent_under_threshold_contributions_state(st: &State) -> Vec<RecentContribution> {
    let mut items: Vec<_> = st
        .contribution_history
        .iter()
        .flat_map(|(canister_id, history)| {
            history
                .iter()
                .filter(|item| !item.counts_toward_faucet)
                .cloned()
                .map(|item| RecentContribution {
                    canister_id: *canister_id,
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                })
        })
        .collect();
    normalize_recent_contribution_bucket(&mut items, false, MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS);
    items
}

fn fallback_recent_burns_state(st: &State) -> Vec<RecentBurn> {
    let mut items: Vec<_> = st
        .per_canister_meta
        .iter()
        .filter_map(|(canister_id, meta)| {
            let tx_id = meta.last_burn_tx_id?;
            if meta.burned_e8s == 0 {
                return None;
            }
            Some(RecentBurn {
                canister_id: *canister_id,
                tx_id,
                timestamp_nanos: None,
                amount_e8s: meta.burned_e8s,
            })
        })
        .collect();
    normalize_recent_burns(&mut items);
    items
}

fn fallback_recent_contributions(st: &State) -> Vec<RecentContributionListItem> {
    let mut items: Vec<_> = fallback_recent_qualifying_contributions_state(st)
        .into_iter()
        .map(|item| RecentContributionListItem {
            canister_id: Some(item.canister_id),
            memo_text: Some(item.canister_id.to_text()),
            tx_id: item.tx_id,
            timestamp_nanos: item.timestamp_nanos,
            amount_e8s: item.amount_e8s,
            counts_toward_faucet: true,
            outcome_category: RecentContributionOutcomeCategory::QualifyingContribution,
        })
        .collect();
    items.extend(
        fallback_recent_under_threshold_contributions_state(st)
            .into_iter()
            .map(|item| RecentContributionListItem {
                canister_id: Some(item.canister_id),
                memo_text: Some(item.canister_id.to_text()),
                tx_id: item.tx_id,
                timestamp_nanos: item.timestamp_nanos,
                amount_e8s: item.amount_e8s,
                counts_toward_faucet: false,
                outcome_category: RecentContributionOutcomeCategory::UnderThresholdContribution,
            }),
    );
    if let Some(invalid) = &st.recent_invalid_contributions {
        items.extend(invalid.iter().cloned().map(|item| RecentContributionListItem {
            canister_id: None,
            memo_text: Some(item.memo_text),
            tx_id: item.tx_id,
            timestamp_nanos: item.timestamp_nanos,
            amount_e8s: item.amount_e8s,
            counts_toward_faucet: false,
            outcome_category: RecentContributionOutcomeCategory::InvalidTargetMemo,
        }));
    }
    items.sort_by(|a, b| {
        let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
        let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
        b_key.cmp(&a_key)
    });
    items.truncate(MAX_RECENT_QUALIFYING_CONTRIBUTIONS + MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS + MAX_RECENT_INVALID_CONTRIBUTIONS);
    items
}

fn initialize_config_defaults_if_missing(st: &mut State) {
    if st.config.cmc_canister_id.is_none() {
        st.config.cmc_canister_id = Some(mainnet_cmc_id());
    }
    if st.config.faucet_canister_id.is_none() {
        st.config.faucet_canister_id = Some(mainnet_faucet_id());
    }
}

fn initialize_derived_state_if_missing(st: &mut State) {
    if st.qualifying_contribution_count.is_none() {
        st.qualifying_contribution_count = Some(fallback_qualifying_contribution_count(st));
    }
    if st.icp_burned_e8s.is_none() {
        st.icp_burned_e8s = Some(fallback_icp_burned_e8s(st));
    }
    if st.recent_contributions.is_none() {
        st.recent_contributions = Some(fallback_recent_qualifying_contributions_state(st));
    }
    if st.recent_under_threshold_contributions.is_none() {
        st.recent_under_threshold_contributions = Some(fallback_recent_under_threshold_contributions_state(st));
    }
    if st.recent_invalid_contributions.is_none() {
        st.recent_invalid_contributions = Some(Vec::new());
    }
    if st.recent_burns.is_none() {
        st.recent_burns = Some(fallback_recent_burns_state(st));
    }
    if st.last_index_run_ts.is_none() {
        st.last_index_run_ts = Some(st.last_main_run_ts);
    }
    if st.registered_canister_summaries_cache.is_none() {
        rebuild_registered_canister_summaries_cache(st);
    }
}

fn registered_canister_summary_for(st: &State, canister_id: Principal) -> Option<RegisteredCanisterSummary> {
    let sources = visible_sources_for_canister(st, &canister_id)?;
    let history = st.contribution_history.get(&canister_id)?;
    let (qualifying_contribution_count, total_qualifying_contributed_e8s, rollup_last_ts) = qualifying_rollup(history);
    let meta = st.per_canister_meta.get(&canister_id).cloned().unwrap_or_default();
    Some(RegisteredCanisterSummary {
        canister_id,
        sources: sources.into_iter().collect(),
        qualifying_contribution_count,
        total_qualifying_contributed_e8s,
        last_contribution_ts: meta.last_contribution_ts.or(rollup_last_ts),
        latest_cycles: st.cycles_history.get(&canister_id).and_then(|history| latest_cycles(history)),
        last_cycles_probe_ts: meta.last_cycles_probe_ts,
    })
}

pub(crate) fn refresh_registered_canister_summary(st: &mut State, canister_id: Principal) {
    let summary = registered_canister_summary_for(st, canister_id);
    let cache = st.registered_canister_summaries_cache.get_or_insert_with(BTreeMap::new);
    if let Some(summary) = summary {
        cache.insert(canister_id, summary);
    } else {
        cache.remove(&canister_id);
    }
}

pub(crate) fn rebuild_registered_canister_summaries_cache(st: &mut State) {
    let canister_ids: Vec<_> = st.canister_sources.keys().copied().collect();
    st.registered_canister_summaries_cache = Some(BTreeMap::new());
    for canister_id in canister_ids {
        refresh_registered_canister_summary(st, canister_id);
    }
}

fn registered_canister_summaries(st: &State) -> Vec<RegisteredCanisterSummary> {
    if let Some(cache) = &st.registered_canister_summaries_cache {
        return cache.values().cloned().collect();
    }

    st.canister_sources
        .keys()
        .copied()
        .filter_map(|canister_id| registered_canister_summary_for(st, canister_id))
        .collect()
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let cfg = config_from_init_args(args);
    state::init_stable_storage();
    let mut st = State::new(cfg, now_secs);
    initialize_config_defaults_if_missing(&mut st);
    normalize_runtime_state(&mut st);
    state::set_state(st);
    scheduler::install_timers();
}

fn apply_upgrade_args(st: &mut State, args: Option<UpgradeArgs>) {
    if let Some(args) = args {
        if let Some(v) = args.enable_sns_tracking {
            st.config.enable_sns_tracking = v;
        }
        if let Some(v) = args.scan_interval_seconds {
            st.config.scan_interval_seconds = v;
        }
        if let Some(v) = args.cycles_interval_seconds {
            st.config.cycles_interval_seconds = v;
        }
        if let Some(v) = args.min_tx_e8s {
            st.config.min_tx_e8s = v;
        }
        if let Some(v) = args.max_cycles_entries_per_canister {
            st.config.max_cycles_entries_per_canister = clamp_cycles_entries_per_canister(v);
        }
        if let Some(v) = args.max_contribution_entries_per_canister {
            st.config.max_contribution_entries_per_canister = clamp_contribution_entries_per_canister(v);
        }
        if let Some(v) = args.max_index_pages_per_tick {
            st.config.max_index_pages_per_tick = clamp_index_pages_per_tick(v);
        }
        if let Some(v) = args.max_canisters_per_cycles_tick {
            st.config.max_canisters_per_cycles_tick = clamp_canisters_per_cycles_tick(v);
        }
        if let Some(v) = args.blackhole_canister_id {
            st.config.blackhole_canister_id = v;
        }
        if let Some(v) = args.sns_wasm_canister_id {
            st.config.sns_wasm_canister_id = v;
        }
        if let Some(v) = args.cmc_canister_id {
            st.config.cmc_canister_id = Some(v);
        }
        if let Some(v) = args.faucet_canister_id {
            st.config.faucet_canister_id = Some(v);
        }
    }
    initialize_derived_state_if_missing(st);
    normalize_runtime_state(st);
    validate_config(&st.config);
    st.main_lock_state_ts = Some(0);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(args: Option<UpgradeArgs>) {
    state::init_stable_storage();
    let mut st: State = state::restore_state_from_stable().expect("stable state missing during historian post_upgrade");
    initialize_config_defaults_if_missing(&mut st);
    apply_upgrade_args(&mut st, args);
    state::set_state(st);
    scheduler::install_timers();
}

#[ic_cdk::query]
fn list_canisters(args: ListCanistersArgs) -> ListCanistersResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 50);
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
            let Some(sources) = visible_sources_for_canister(st, &canister_id) else {
                continue;
            };
            if let Some(filter) = &args.source_filter {
                if !sources.contains(filter) {
                    continue;
                }
            }
            if items.len() >= limit {
                next = items.last().map(|item: &CanisterListItem| item.canister_id);
                break;
            }
            items.push(CanisterListItem {
                canister_id,
                sources: sources.into_iter().collect(),
            });
        }
        ListCanistersResponse {
            items,
            next_start_after: next,
        }
    })
}

#[ic_cdk::query]
fn get_cycles_history(args: GetCyclesHistoryArgs) -> CyclesHistoryPage {
    state::with_state(|st| {
        let descending = args.descending.unwrap_or(false);
        let limit = clamp_public_limit(args.limit, 100);
        let mut items = Vec::new();
        let mut next = None;
        if let Some(history) = st.cycles_history.get(&args.canister_id) {
            let iter: Box<dyn Iterator<Item = &CyclesSample>> = if descending {
                Box::new(history.iter().rev())
            } else {
                Box::new(history.iter())
            };
            for item in iter {
                let include = match args.start_after_ts {
                    Some(ts) if descending => item.timestamp_nanos < ts,
                    Some(ts) => item.timestamp_nanos > ts,
                    None => true,
                };
                if !include {
                    continue;
                }
                if items.len() >= limit {
                    next = items.last().map(|sample: &CyclesSample| sample.timestamp_nanos);
                    break;
                }
                items.push(item.clone());
            }
        }
        CyclesHistoryPage {
            items,
            next_start_after_ts: next,
        }
    })
}

#[ic_cdk::query]
fn get_contribution_history(args: GetContributionHistoryArgs) -> ContributionHistoryPage {
    state::with_state(|st| {
        let descending = args.descending.unwrap_or(false);
        let limit = clamp_public_limit(args.limit, 100);
        let mut items = Vec::new();
        let mut next = None;
        if let Some(history) = st.contribution_history.get(&args.canister_id) {
            let iter: Box<dyn Iterator<Item = &ContributionSample>> = if descending {
                Box::new(history.iter().rev())
            } else {
                Box::new(history.iter())
            };
            for item in iter {
                let include = match args.start_after_tx_id {
                    Some(tx_id) if descending => item.tx_id < tx_id,
                    Some(tx_id) => item.tx_id > tx_id,
                    None => true,
                };
                if !include {
                    continue;
                }
                if items.len() >= limit {
                    next = items.last().map(|sample: &ContributionSample| sample.tx_id);
                    break;
                }
                items.push(item.clone());
            }
        }
        ContributionHistoryPage {
            items,
            next_start_after_tx_id: next,
        }
    })
}

#[ic_cdk::query]
fn get_canister_overview(canister_id: Principal) -> Option<CanisterOverview> {
    state::with_state(|st| {
        let sources = visible_sources_for_canister(st, &canister_id)?
            .into_iter()
            .collect();
        let meta = st.per_canister_meta.get(&canister_id).cloned().unwrap_or_default();
        let cycles_points = st
            .cycles_history
            .get(&canister_id)
            .map(|v| v.len() as u32)
            .unwrap_or(0);
        let contribution_points = st
            .contribution_history
            .get(&canister_id)
            .map(|v| v.len() as u32)
            .unwrap_or(0);
        Some(CanisterOverview {
            canister_id,
            sources,
            meta,
            cycles_points,
            contribution_points,
        })
    })
}

#[ic_cdk::query]
fn get_public_counts() -> PublicCounts {
    state::with_state(|st| PublicCounts {
        registered_canister_count: count_registered_canisters(st),
        qualifying_contribution_count: st
            .qualifying_contribution_count
            .unwrap_or_else(|| fallback_qualifying_contribution_count(st)),
        icp_burned_e8s: st.icp_burned_e8s.unwrap_or_else(|| fallback_icp_burned_e8s(st)),
        sns_discovered_canister_count: count_sns_discovered_canisters(st),
    })
}

#[ic_cdk::query]
fn get_public_status() -> PublicStatus {
    let heap_memory_bytes = allocated_heap_memory_bytes();
    let stable_memory_bytes = allocated_stable_memory_bytes();
    state::with_state(|st| PublicStatus {
        staking_account: st.config.staking_account.clone(),
        ledger_canister_id: st.config.ledger_canister_id,
        last_index_run_ts: st.last_index_run_ts.or(Some(st.last_main_run_ts)),
        index_interval_seconds: st.config.scan_interval_seconds,
        last_completed_cycles_sweep_ts: if st.last_completed_cycles_sweep_ts == 0 {
            None
        } else {
            Some(st.last_completed_cycles_sweep_ts)
        },
        cycles_interval_seconds: st.config.cycles_interval_seconds,
        heap_memory_bytes: Some(heap_memory_bytes),
        stable_memory_bytes: Some(stable_memory_bytes),
        total_memory_bytes: Some(heap_memory_bytes.saturating_add(stable_memory_bytes)),
    })
}

#[ic_cdk::query]
fn list_registered_canister_summaries(
    args: ListRegisteredCanisterSummariesArgs,
) -> ListRegisteredCanisterSummariesResponse {
    state::with_state(|st| {
        let page = args.page.unwrap_or(0);
        let page_size = args.page_size.unwrap_or(25).clamp(1, 100);
        let mut items = registered_canister_summaries(st);
        match args.sort.unwrap_or(RegisteredCanisterSummarySort::TotalQualifyingContributedDesc) {
            RegisteredCanisterSummarySort::CanisterIdAsc => {
                items.sort_by_key(|item| item.canister_id);
            }
            RegisteredCanisterSummarySort::LastContributionDesc => {
                items.sort_by_key(|item| (Reverse(item.last_contribution_ts.unwrap_or(0)), item.canister_id));
            }
            RegisteredCanisterSummarySort::QualifyingContributionCountDesc => {
                items.sort_by_key(|item| (Reverse(item.qualifying_contribution_count), item.canister_id));
            }
            RegisteredCanisterSummarySort::TotalQualifyingContributedDesc => {
                items.sort_by_key(|item| (Reverse(item.total_qualifying_contributed_e8s), item.canister_id));
            }
        }
        let total = items.len() as u64;
        let start = page.saturating_mul(page_size) as usize;
        let end = start.saturating_add(page_size as usize).min(items.len());
        let page_items = if start >= items.len() {
            Vec::new()
        } else {
            items[start..end].to_vec()
        };
        ListRegisteredCanisterSummariesResponse {
            items: page_items,
            page,
            page_size,
            total,
        }
    })
}

#[ic_cdk::query]
fn list_recent_contributions(args: ListRecentContributionsArgs) -> ListRecentContributionsResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 20);
        let qualifying_only = args.qualifying_only.unwrap_or(false);
        let mut items: Vec<RecentContributionListItem> = if let Some(recent) = &st.recent_contributions {
            let mut merged: Vec<RecentContributionListItem> = recent
                .iter()
                .cloned()
                .map(|item| RecentContributionListItem {
                    canister_id: Some(item.canister_id),
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                    outcome_category: RecentContributionOutcomeCategory::QualifyingContribution,
                })
                .collect();
            if let Some(low_value) = &st.recent_under_threshold_contributions {
                merged.extend(low_value.iter().cloned().map(|item| RecentContributionListItem {
                    canister_id: Some(item.canister_id),
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentContributionOutcomeCategory::UnderThresholdContribution,
                }));
            }
            if let Some(invalid) = &st.recent_invalid_contributions {
                merged.extend(invalid.iter().cloned().map(|item| RecentContributionListItem {
                    canister_id: None,
                    memo_text: Some(item.memo_text),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentContributionOutcomeCategory::InvalidTargetMemo,
                }));
            }
            merged.sort_by(|a, b| {
                let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
                let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
                b_key.cmp(&a_key)
            });
            merged
        } else {
            fallback_recent_contributions(st)
        };
        if qualifying_only {
            items.retain(|item| item.counts_toward_faucet);
        }
        items.truncate(limit);
        ListRecentContributionsResponse { items }
    })
}

#[ic_cdk::query]
fn list_recent_burns(args: ListRecentBurnsArgs) -> ListRecentBurnsResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 20);
        let mut items = st.recent_burns.clone().unwrap_or_default();
        items.truncate(limit);
        ListRecentBurnsResponse {
            items: items
                .into_iter()
                .map(|item| RecentBurnListItem {
                    canister_id: item.canister_id,
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                })
                .collect(),
        }
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
    pub last_index_run_ts: Option<u64>,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugConfig {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub cmc_canister_id: Option<Principal>,
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

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    guard_debug_api_not_production();
    state::with_state(|st| DebugState {
        distinct_canister_count: st.distinct_canisters.len() as u32,
        last_indexed_staking_tx_id: st.last_indexed_staking_tx_id,
        last_sns_discovery_ts: st.last_sns_discovery_ts,
        last_completed_cycles_sweep_ts: st.last_completed_cycles_sweep_ts,
        active_cycles_sweep_present: st.active_cycles_sweep.is_some(),
        active_cycles_sweep_next_index: st.active_cycles_sweep.as_ref().map(|s| s.next_index),
        last_index_run_ts: st.last_index_run_ts,
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_config() -> DebugConfig {
    guard_debug_api_not_production();
    state::with_state(|st| DebugConfig {
        staking_account: st.config.staking_account.clone(),
        ledger_canister_id: st.config.ledger_canister_id,
        index_canister_id: st.config.index_canister_id,
        cmc_canister_id: st.config.cmc_canister_id,
        faucet_canister_id: st.config.faucet_canister_id,
        blackhole_canister_id: st.config.blackhole_canister_id,
        sns_wasm_canister_id: st.config.sns_wasm_canister_id,
        enable_sns_tracking: st.config.enable_sns_tracking,
        scan_interval_seconds: st.config.scan_interval_seconds,
        cycles_interval_seconds: st.config.cycles_interval_seconds,
        min_tx_e8s: st.config.min_tx_e8s,
        max_cycles_entries_per_canister: st.config.max_cycles_entries_per_canister,
        max_contribution_entries_per_canister: st.config.max_contribution_entries_per_canister,
        max_index_pages_per_tick: st.config.max_index_pages_per_tick,
        max_canisters_per_cycles_tick: st.config.max_canisters_per_cycles_tick,
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_driver_tick() {
    guard_debug_api_not_production();
    scheduler::main_tick(true).await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_completed_cycles_sweep_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.last_completed_cycles_sweep_ts = ts.unwrap_or(0));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_sns_discovery_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.last_sns_discovery_ts = ts.unwrap_or(0));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_indexed_staking_tx_id(tx_id: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.last_indexed_staking_tx_id = tx_id);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_reset_runtime_state() {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| {
        st.active_cycles_sweep = None;
        st.main_lock_state_ts = Some(0);
        st.last_main_run_ts = 0;
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_main_lock_expires_at_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    state::with_root_state_mut(|st| st.main_lock_state_ts = Some(ts.unwrap_or(0)));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_reset_derived_state() {
    guard_debug_api_not_production();
    state::with_state_mut(|st| {
        st.distinct_canisters.clear();
        st.canister_sources.clear();
        st.contribution_history.clear();
        st.cycles_history.clear();
        st.per_canister_meta.clear();
        st.last_indexed_staking_tx_id = None;
        st.last_sns_discovery_ts = 0;
        st.last_completed_cycles_sweep_ts = 0;
        st.active_cycles_sweep = None;
        st.main_lock_state_ts = Some(0);
        st.last_main_run_ts = 0;
        st.qualifying_contribution_count = Some(0);
        st.icp_burned_e8s = Some(0);
        st.recent_contributions = Some(Vec::new());
        st.recent_under_threshold_contributions = Some(Vec::new());
        st.recent_invalid_contributions = Some(Vec::new());
        st.recent_burns = Some(Vec::new());
        st.last_index_run_ts = Some(0);
        st.registered_canister_summaries_cache = Some(BTreeMap::new());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CanisterMeta, CyclesSampleSource, InvalidContribution};
    use std::collections::{BTreeMap, BTreeSet};

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn sample_account() -> Account {
        Account {
            owner: principal("22255-zqaaa-aaaas-qf6uq-cai"),
            subaccount: None,
        }
    }

    fn alternate_account() -> Account {
        Account {
            owner: principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
            subaccount: Some([7u8; 32]),
        }
    }

    fn base_state() -> State {
        State {
            config: Config {
                staking_account: sample_account(),
                ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
                index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
                cmc_canister_id: Some(principal("rkp4c-7iaaa-aaaaa-aaaca-cai")),
                faucet_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                blackhole_canister_id: principal("e3mmv-5qaaa-aaaah-aadma-cai"),
                sns_wasm_canister_id: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
                enable_sns_tracking: false,
                scan_interval_seconds: 600,
                cycles_interval_seconds: 604800,
                min_tx_e8s: 100_000_000,
                max_cycles_entries_per_canister: 100,
                max_contribution_entries_per_canister: 100,
                max_index_pages_per_tick: 10,
                max_canisters_per_cycles_tick: 25,
            },
            distinct_canisters: BTreeSet::new(),
            canister_sources: BTreeMap::new(),
            contribution_history: BTreeMap::new(),
            cycles_history: BTreeMap::new(),
            per_canister_meta: BTreeMap::new(),
            last_indexed_staking_tx_id: None,
            last_sns_discovery_ts: 0,
            last_completed_cycles_sweep_ts: 0,
            active_cycles_sweep: None,
            active_sns_discovery: None,
            main_lock_state_ts: Some(0),
            last_main_run_ts: 1,
            qualifying_contribution_count: None,
            icp_burned_e8s: None,
            recent_contributions: None,
            recent_under_threshold_contributions: None,
            recent_invalid_contributions: None,
            recent_burns: None,
            last_index_run_ts: None,
            registered_canister_summaries_cache: None,
        }
    }

    #[test]
    fn initialize_derived_state_reconstructs_recent_burns_from_legacy_meta() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                last_burn_tx_id: Some(77),
                burned_e8s: 123_456_789,
                ..CanisterMeta::default()
            },
        );

        initialize_derived_state_if_missing(&mut st);

        assert_eq!(st.icp_burned_e8s, Some(123_456_789));
        let burns = st.recent_burns.clone().expect("recent burns should be reconstructed");
        assert_eq!(burns.len(), 1);
        assert_eq!(burns[0].canister_id, canister);
        assert_eq!(burns[0].tx_id, 77);
        assert_eq!(burns[0].amount_e8s, 123_456_789);
        assert_eq!(burns[0].timestamp_nanos, None);
    }

    #[test]
    fn config_from_init_args_uses_mainnet_defaults_for_optional_canisters() {
        let cfg = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: None,
            cycles_interval_seconds: None,
            min_tx_e8s: None,
            max_cycles_entries_per_canister: None,
            max_contribution_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });

        assert_eq!(cfg.ledger_canister_id, mainnet_ledger_id());
        assert_eq!(cfg.index_canister_id, mainnet_index_id());
        assert_eq!(cfg.blackhole_canister_id, mainnet_blackhole_id());
        assert_eq!(cfg.cmc_canister_id, Some(mainnet_cmc_id()));
        assert_eq!(cfg.faucet_canister_id, Some(mainnet_faucet_id()));
        assert_eq!(cfg.sns_wasm_canister_id, mainnet_sns_wasm_id());
        assert_eq!(cfg.scan_interval_seconds, 600);
        assert_eq!(cfg.cycles_interval_seconds, 604800);
        assert_eq!(cfg.min_tx_e8s, 100_000_000);
    }

    #[test]
    fn config_validation_accepts_minimum_supported_threshold() {
        let mut cfg = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: Some(600),
            cycles_interval_seconds: Some(604800),
            min_tx_e8s: Some(MIN_MIN_TX_E8S),
            max_cycles_entries_per_canister: None,
            max_contribution_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });
        cfg.min_tx_e8s = MIN_MIN_TX_E8S;
        validate_config(&cfg);
    }


    #[test]
    fn production_canister_detection_matches_expected_id() {
        assert!(is_production_canister(production_canister_id()));
        assert!(!is_production_canister(principal("aaaaa-aa")));
    }

    #[test]
    fn refresh_registered_canister_summary_updates_cache_incrementally() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 7,
                timestamp_nanos: Some(9_000_000_000),
                amount_e8s: 123_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                last_contribution_ts: Some(9),
                last_cycles_probe_ts: Some(10),
                ..CanisterMeta::default()
            },
        );
        st.cycles_history.insert(
            canister,
            vec![CyclesSample {
                timestamp_nanos: 10_000_000_000,
                cycles: 777,
                source: CyclesSampleSource::BlackholeStatus,
            }],
        );

        refresh_registered_canister_summary(&mut st, canister);
        let cached = st
            .registered_canister_summaries_cache
            .as_ref()
            .and_then(|cache| cache.get(&canister))
            .cloned()
            .expect("cached summary should exist");

        assert_eq!(cached.canister_id, canister);
        assert_eq!(cached.qualifying_contribution_count, 1);
        assert_eq!(cached.total_qualifying_contributed_e8s, 123_000_000);
        assert_eq!(cached.last_contribution_ts, Some(9));
        assert_eq!(cached.latest_cycles, Some(777));
        assert_eq!(cached.last_cycles_probe_ts, Some(10));
    }

    #[test]
    #[should_panic(expected = "min_tx_e8s must be at least")]
    fn config_from_init_args_rejects_threshold_below_minimum() {
        let _ = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            ledger_canister_id: None,
            index_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
            blackhole_canister_id: None,
            sns_wasm_canister_id: None,
            enable_sns_tracking: None,
            scan_interval_seconds: Some(600),
            cycles_interval_seconds: Some(604800),
            min_tx_e8s: Some(MIN_MIN_TX_E8S - 1),
            max_cycles_entries_per_canister: None,
            max_contribution_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });
    }

    #[test]
    fn apply_upgrade_args_updates_tuning_fields_and_preserves_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 10,
                counts_toward_faucet: true,
            }],
        );
        st.main_lock_state_ts = Some(99);

        let original_account = st.config.staking_account.clone();
        let original_ledger = st.config.ledger_canister_id;
        let original_index = st.config.index_canister_id;

        apply_upgrade_args(
            &mut st,
            Some(UpgradeArgs {
                enable_sns_tracking: Some(true),
                scan_interval_seconds: Some(123),
                cycles_interval_seconds: Some(456),
                min_tx_e8s: Some(MIN_MIN_TX_E8S),
                max_cycles_entries_per_canister: Some(11),
                max_contribution_entries_per_canister: Some(12),
                max_index_pages_per_tick: Some(13),
                max_canisters_per_cycles_tick: Some(14),
                blackhole_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                sns_wasm_canister_id: Some(principal("qaa6y-5yaaa-aaaaa-aaafa-cai")),
                cmc_canister_id: None,
                faucet_canister_id: None,
            }),
        );

        assert_eq!(st.config.staking_account, original_account);
        assert_eq!(st.config.ledger_canister_id, original_ledger);
        assert_eq!(st.config.index_canister_id, original_index);
        assert!(st.config.enable_sns_tracking);
        assert_eq!(st.config.scan_interval_seconds, 123);
        assert_eq!(st.config.cycles_interval_seconds, 456);
        assert_eq!(st.config.min_tx_e8s, MIN_MIN_TX_E8S);
        assert_eq!(st.config.max_cycles_entries_per_canister, 11);
        assert_eq!(st.config.max_contribution_entries_per_canister, 12);
        assert_eq!(st.config.max_index_pages_per_tick, 13);
        assert_eq!(st.config.max_canisters_per_cycles_tick, 14);
        assert_eq!(st.contribution_history.get(&canister).map(|v| v.len()), Some(1));
        assert_eq!(st.main_lock_state_ts, Some(0));
    }

    #[test]
    fn get_public_status_reflects_effective_runtime_config() {
        let mut st = base_state();
        st.config.staking_account = alternate_account();
        st.config.ledger_canister_id = principal("jufzc-caaaa-aaaar-qb5da-cai");
        st.last_index_run_ts = Some(777);
        st.last_completed_cycles_sweep_ts = 888;
        state::set_state(st);

        let status = get_public_status();
        assert_eq!(status.staking_account, alternate_account());
        assert_eq!(status.ledger_canister_id, principal("jufzc-caaaa-aaaar-qb5da-cai"));
        assert_eq!(status.last_index_run_ts, Some(777));
        assert_eq!(status.last_completed_cycles_sweep_ts, Some(888));
        assert!(status.heap_memory_bytes.is_some());
        assert!(status.stable_memory_bytes.is_some());
        assert_eq!(
            status.total_memory_bytes,
            Some(status.heap_memory_bytes.unwrap_or(0).saturating_add(status.stable_memory_bytes.unwrap_or(0))),
        );
    }

    #[test]
    fn registered_canister_count_requires_qualifying_memo_contribution_history() {
        let memo_only = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let sns_only = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let both = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");

        let mut st = base_state();
        st.canister_sources.insert(memo_only, BTreeSet::from([CanisterSource::MemoContribution]));
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        st.canister_sources.insert(
            both,
            BTreeSet::from([CanisterSource::MemoContribution, CanisterSource::SnsDiscovery]),
        );
        st.contribution_history.insert(
            memo_only,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 80_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.contribution_history.insert(
            both,
            vec![ContributionSample {
                tx_id: 2,
                timestamp_nanos: Some(2_000_000_000),
                amount_e8s: 50_000_000,
                counts_toward_faucet: true,
            }],
        );

        assert_eq!(count_registered_canisters(&st), 2);
    }

    #[test]
    fn get_public_counts_surfaces_expected_frontend_metrics() {
        let memo_canister = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let sns_only = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.canister_sources.insert(memo_canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        st.contribution_history.insert(
            memo_canister,
            vec![
                ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 80_000_000,
                    counts_toward_faucet: true,
                },
                ContributionSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 5_000_000,
                    counts_toward_faucet: false,
                },
            ],
        );
        state::set_state(st);

        let counts = get_public_counts();
        assert_eq!(counts.registered_canister_count, 1);
        assert_eq!(counts.qualifying_contribution_count, 1);
        assert_eq!(counts.icp_burned_e8s, 0);
        assert_eq!(counts.sns_discovered_canister_count, 1);
    }

    #[test]
    fn get_public_counts_excludes_non_qualifying_memo_canisters_from_registered_totals() {
        let memo_canister = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let mut st = base_state();
        st.canister_sources.insert(memo_canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            memo_canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        let counts = get_public_counts();
        assert_eq!(counts.registered_canister_count, 0);
        assert_eq!(counts.qualifying_contribution_count, 0);
        assert_eq!(counts.icp_burned_e8s, 0);
        assert_eq!(counts.sns_discovered_canister_count, 0);
    }

    #[test]
    fn list_registered_canister_summaries_excludes_sns_only_canisters() {
        let sns_only = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
            sort: Some(RegisteredCanisterSummarySort::CanisterIdAsc),
        });
        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }


    #[test]
    fn list_registered_canister_summaries_excludes_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
            sort: Some(RegisteredCanisterSummarySort::CanisterIdAsc),
        });
        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }

    #[test]
    fn get_canister_overview_hides_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        assert!(get_canister_overview(canister).is_none());
    }

    #[test]
    fn burn_targets_exclude_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );

        let targets = burn_target_canisters(&st);
        assert!(!targets.contains(&canister));
        assert!(targets.contains(&effective_faucet_canister_id(&st)));
    }

    #[test]
    fn list_registered_canister_summaries_uses_canister_id_as_tie_breaker_for_stable_pagination() {
        let a = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let b = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let mut st = base_state();
        for canister in [a, b] {
            st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
            st.contribution_history.insert(
                canister,
                vec![ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000),
                    amount_e8s: 50_000_000,
                    counts_toward_faucet: true,
                }],
            );
            st.per_canister_meta.insert(
                canister,
                CanisterMeta {
                    last_contribution_ts: Some(1_000),
                    ..CanisterMeta::default()
                },
            );
        }
        state::set_state(st);

        for sort in [
            RegisteredCanisterSummarySort::LastContributionDesc,
            RegisteredCanisterSummarySort::QualifyingContributionCountDesc,
            RegisteredCanisterSummarySort::TotalQualifyingContributedDesc,
        ] {
            let first_page = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
                page: Some(0),
                page_size: Some(1),
                sort: Some(sort.clone()),
            });
            let second_page = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
                page: Some(1),
                page_size: Some(1),
                sort: Some(sort),
            });

            assert_eq!(first_page.total, 2);
            assert_eq!(second_page.total, 2);
            assert_eq!(first_page.items.len(), 1);
            assert_eq!(second_page.items.len(), 1);
            assert_eq!(first_page.items[0].canister_id, b.min(a));
            assert_eq!(second_page.items[0].canister_id, b.max(a));
        }
    }

    #[test]
    fn list_registered_canister_summaries_returns_empty_pages_past_the_end() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000),
                amount_e8s: 50_000_000,
                counts_toward_faucet: true,
            }],
        );
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(5),
            page_size: Some(1),
            sort: Some(RegisteredCanisterSummarySort::CanisterIdAsc),
        });
        assert_eq!(response.total, 1);
        assert!(response.items.is_empty());
    }

    #[test]
    fn list_recent_contributions_returns_qualifying_and_non_qualifying_commitments() {
        let qualifying = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let low_amount = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.recent_contributions = Some(vec![
            RecentContribution {
                canister_id: qualifying,
                tx_id: 11,
                timestamp_nanos: Some(11),
                amount_e8s: 20_000_000,
                counts_toward_faucet: true,
            },
        ]);
        st.recent_under_threshold_contributions = Some(vec![
            RecentContribution {
                canister_id: low_amount,
                tx_id: 10,
                timestamp_nanos: Some(10),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            },
        ]);
        st.recent_invalid_contributions = Some(vec![InvalidContribution {
            tx_id: 12,
            timestamp_nanos: Some(12),
            amount_e8s: 20_000_000,
            memo_text: crate::logic::INVALID_MEMO_PLACEHOLDER.to_string(),
        }]);
        state::set_state(st);

        let all = list_recent_contributions(ListRecentContributionsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        });
        assert_eq!(all.items.len(), 3);
        assert_eq!(all.items[0].tx_id, 12);
        assert_eq!(all.items[0].canister_id, None);
        assert_eq!(all.items[0].memo_text.as_deref(), Some(crate::logic::INVALID_MEMO_PLACEHOLDER));
        assert!(!all.items[0].counts_toward_faucet);
        assert_eq!(all.items[0].outcome_category, RecentContributionOutcomeCategory::InvalidTargetMemo);
        assert_eq!(all.items[1].tx_id, 11);
        assert_eq!(all.items[1].canister_id, Some(qualifying));
        assert!(all.items[1].counts_toward_faucet);
        assert_eq!(all.items[1].outcome_category, RecentContributionOutcomeCategory::QualifyingContribution);
        assert_eq!(all.items[2].tx_id, 10);
        assert_eq!(all.items[2].canister_id, Some(low_amount));
        assert!(!all.items[2].counts_toward_faucet);
        assert_eq!(all.items[2].outcome_category, RecentContributionOutcomeCategory::UnderThresholdContribution);

        let qualifying_only = list_recent_contributions(ListRecentContributionsArgs {
            limit: Some(10),
            qualifying_only: Some(true),
        });
        assert_eq!(qualifying_only.items.len(), 1);
        assert_eq!(qualifying_only.items[0].tx_id, 11);
        assert!(qualifying_only.items[0].counts_toward_faucet);
        assert_eq!(qualifying_only.items[0].outcome_category, RecentContributionOutcomeCategory::QualifyingContribution);
    }

    #[test]
    fn derived_aggregates_fallback_from_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.contribution_history.insert(
            canister,
            vec![
                ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(10),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                },
                ContributionSample {
                    tx_id: 2,
                    timestamp_nanos: Some(20),
                    amount_e8s: 50,
                    counts_toward_faucet: false,
                },
                ContributionSample {
                    tx_id: 3,
                    timestamp_nanos: Some(30),
                    amount_e8s: 200,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                burned_e8s: 300,
                ..Default::default()
            },
        );

        initialize_derived_state_if_missing(&mut st);
        assert_eq!(st.qualifying_contribution_count, Some(2));
        assert_eq!(st.icp_burned_e8s, Some(300));
        assert_eq!(st.recent_contributions.as_ref().unwrap()[0].tx_id, 3);
    }


    #[test]
    fn normalize_runtime_state_moves_non_qualifying_commitments_out_of_registered_history() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.config.max_contribution_entries_per_canister = 1;
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![
                ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 5_000_000,
                    counts_toward_faucet: false,
                },
                ContributionSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 50_000_000,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.recent_contributions = Some(vec![RecentContribution {
            canister_id: canister,
            tx_id: 1,
            timestamp_nanos: Some(1_000_000_000),
            amount_e8s: 5_000_000,
            counts_toward_faucet: false,
        }]);
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                first_seen_ts: Some(1),
                ..CanisterMeta::default()
            },
        );

        normalize_runtime_state(&mut st);

        assert_eq!(st.qualifying_contribution_count, Some(1));
        assert_eq!(st.contribution_history.get(&canister).map(|items| items.len()), Some(1));
        assert_eq!(st.contribution_history.get(&canister).unwrap()[0].tx_id, 2);
        assert_eq!(st.recent_contributions.as_ref().map(|items| items.len()), Some(1));
        assert_eq!(st.recent_contributions.as_ref().unwrap()[0].tx_id, 2);
        assert_eq!(
            st.recent_under_threshold_contributions
                .as_ref()
                .map(|items| items.iter().map(|item| item.tx_id).collect::<Vec<_>>()),
            Some(vec![1]),
        );
        assert_eq!(st.per_canister_meta.get(&canister).and_then(|meta| meta.last_contribution_ts), Some(2));
        assert_eq!(count_registered_canisters(&st), 1);
    }

    #[test]
    fn normalize_runtime_state_prunes_memo_only_registration_when_history_is_non_qualifying() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );

        normalize_runtime_state(&mut st);

        assert_eq!(count_registered_canisters(&st), 0);
        assert!(!st.canister_sources.contains_key(&canister));
        assert!(!st.distinct_canisters.contains(&canister));
        assert!(!st.contribution_history.contains_key(&canister));
        assert!(!st.cycles_history.contains_key(&canister));
        assert!(!st.per_canister_meta.contains_key(&canister));
        assert_eq!(
            st.recent_under_threshold_contributions
                .as_ref()
                .map(|items| items.iter().map(|item| item.tx_id).collect::<Vec<_>>()),
            Some(vec![1]),
        );
    }


    #[test]
    fn normalize_runtime_state_preserves_large_beneficiary_registry() {
        let mut st = base_state();
        for idx in 0..=2_100u32 {
            let canister = Principal::from_slice(&[((idx % 250) + 1) as u8, ((idx / 250) + 1) as u8]);
            st.distinct_canisters.insert(canister);
            st.canister_sources
                .insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
            st.contribution_history.insert(
                canister,
                vec![ContributionSample {
                    tx_id: idx as u64 + 1,
                    timestamp_nanos: Some((idx as u64 + 1) * 1_000_000_000),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                }],
            );
        }

        normalize_runtime_state(&mut st);

        assert_eq!(st.distinct_canisters.len(), 2_101);
        assert_eq!(st.canister_sources.len(), 2_101);
        assert_eq!(st.contribution_history.len(), 2_101);
    }

    #[test]
    fn apply_upgrade_args_clamps_runtime_caps() {
        let mut st = base_state();
        apply_upgrade_args(
            &mut st,
            Some(UpgradeArgs {
                max_cycles_entries_per_canister: Some(u32::MAX),
                max_contribution_entries_per_canister: Some(u32::MAX),
                max_index_pages_per_tick: Some(u32::MAX),
                max_canisters_per_cycles_tick: Some(u32::MAX),
                ..UpgradeArgs::default()
            }),
        );

        assert_eq!(st.config.max_cycles_entries_per_canister, MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP);
        assert_eq!(
            st.config.max_contribution_entries_per_canister,
            MAX_CONTRIBUTION_ENTRIES_PER_CANISTER_HARD_CAP,
        );
        assert_eq!(st.config.max_index_pages_per_tick, MAX_INDEX_PAGES_PER_TICK_HARD_CAP);
        assert_eq!(
            st.config.max_canisters_per_cycles_tick,
            MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP,
        );
    }

    #[test]
    fn public_query_limits_are_clamped() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            (1..=150)
                .map(|tx_id| ContributionSample {
                    tx_id,
                    timestamp_nanos: Some(tx_id * 1_000_000_000),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                })
                .collect(),
        );
        st.cycles_history.insert(
            canister,
            (1..=150)
                .map(|idx| CyclesSample {
                    timestamp_nanos: idx,
                    cycles: idx as u128,
                    source: CyclesSampleSource::BlackholeStatus,
                })
                .collect(),
        );
        state::set_state(st);

        let canisters = list_canisters(ListCanistersArgs {
            start_after: None,
            limit: Some(5_000),
            source_filter: None,
        });
        assert_eq!(canisters.items.len(), 1);

        let contributions = get_contribution_history(GetContributionHistoryArgs {
            canister_id: canister,
            start_after_tx_id: None,
            limit: Some(5_000),
            descending: Some(false),
        });
        assert_eq!(contributions.items.len(), MAX_PUBLIC_QUERY_LIMIT as usize);
        assert_eq!(contributions.next_start_after_tx_id, Some(100));

        let cycles = get_cycles_history(GetCyclesHistoryArgs {
            canister_id: canister,
            start_after_ts: None,
            limit: Some(5_000),
            descending: Some(false),
        });
        assert_eq!(cycles.items.len(), MAX_PUBLIC_QUERY_LIMIT as usize);
        assert_eq!(cycles.next_start_after_ts, Some(100));
    }

    #[test]
    fn list_canisters_pagination_round_trips_without_skips() {
        let canisters = [
            principal("22255-zqaaa-aaaas-qf6uq-cai"),
            principal("r7inp-6aaaa-aaaaa-aaabq-cai"),
            principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
        ];
        let mut st = base_state();
        for canister in canisters {
            st.distinct_canisters.insert(canister);
            st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
            st.contribution_history.insert(
                canister,
                vec![ContributionSample {
                    tx_id: canister.as_slice()[0] as u64,
                    timestamp_nanos: Some(canister.as_slice()[0] as u64),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                }],
            );
        }
        state::set_state(st);

        let first = list_canisters(ListCanistersArgs { start_after: None, limit: Some(2), source_filter: None });
        let second = list_canisters(ListCanistersArgs { start_after: first.next_start_after, limit: Some(2), source_filter: None });
        let returned: Vec<_> = first.items.into_iter().chain(second.items.into_iter()).map(|item| item.canister_id).collect();
        let mut expected = canisters.to_vec();
        expected.sort();
        assert_eq!(returned, expected);
    }

    #[test]
    fn cycles_history_pagination_round_trips_without_skips_in_both_directions() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![ContributionSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 100_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.cycles_history.insert(
            canister,
            vec![
                CyclesSample { timestamp_nanos: 10, cycles: 1, source: CyclesSampleSource::BlackholeStatus },
                CyclesSample { timestamp_nanos: 20, cycles: 2, source: CyclesSampleSource::BlackholeStatus },
                CyclesSample { timestamp_nanos: 30, cycles: 3, source: CyclesSampleSource::BlackholeStatus },
            ],
        );
        state::set_state(st);

        let first = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: None, limit: Some(2), descending: Some(false) });
        let second = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: first.next_start_after_ts, limit: Some(2), descending: Some(false) });
        let asc: Vec<_> = first.items.iter().chain(second.items.iter()).map(|item| item.timestamp_nanos).collect();
        assert_eq!(asc, vec![10, 20, 30]);

        let first_desc = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: None, limit: Some(2), descending: Some(true) });
        let second_desc = get_cycles_history(GetCyclesHistoryArgs { canister_id: canister, start_after_ts: first_desc.next_start_after_ts, limit: Some(2), descending: Some(true) });
        let desc: Vec<_> = first_desc.items.iter().chain(second_desc.items.iter()).map(|item| item.timestamp_nanos).collect();
        assert_eq!(desc, vec![30, 20, 10]);
    }

    #[test]
    fn contribution_history_pagination_round_trips_without_skips_in_both_directions() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![
                ContributionSample { tx_id: 10, timestamp_nanos: Some(10), amount_e8s: 1, counts_toward_faucet: true },
                ContributionSample { tx_id: 20, timestamp_nanos: Some(20), amount_e8s: 1, counts_toward_faucet: true },
                ContributionSample { tx_id: 30, timestamp_nanos: Some(30), amount_e8s: 1, counts_toward_faucet: true },
            ],
        );
        state::set_state(st);

        let first = get_contribution_history(GetContributionHistoryArgs { canister_id: canister, start_after_tx_id: None, limit: Some(2), descending: Some(false) });
        let second = get_contribution_history(GetContributionHistoryArgs { canister_id: canister, start_after_tx_id: first.next_start_after_tx_id, limit: Some(2), descending: Some(false) });
        let asc: Vec<_> = first.items.iter().chain(second.items.iter()).map(|item| item.tx_id).collect();
        assert_eq!(asc, vec![10, 20, 30]);

        let first_desc = get_contribution_history(GetContributionHistoryArgs { canister_id: canister, start_after_tx_id: None, limit: Some(2), descending: Some(true) });
        let second_desc = get_contribution_history(GetContributionHistoryArgs { canister_id: canister, start_after_tx_id: first_desc.next_start_after_tx_id, limit: Some(2), descending: Some(true) });
        let desc: Vec<_> = first_desc.items.iter().chain(second_desc.items.iter()).map(|item| item.tx_id).collect();
        assert_eq!(desc, vec![30, 20, 10]);
    }

    #[test]
    fn stable_state_decodes_original_historian_layout() {
        #[derive(CandidType, candid::Deserialize, Clone)]
        struct LegacyConfig {
            staking_account: Account,
            ledger_canister_id: Principal,
            index_canister_id: Principal,
            blackhole_canister_id: Principal,
            sns_wasm_canister_id: Principal,
            enable_sns_tracking: bool,
            scan_interval_seconds: u64,
            cycles_interval_seconds: u64,
            min_tx_e8s: u64,
            max_cycles_entries_per_canister: u32,
            max_contribution_entries_per_canister: u32,
            max_index_pages_per_tick: u32,
            max_canisters_per_cycles_tick: u32,
        }

        #[derive(CandidType, candid::Deserialize, Clone, Default)]
        struct LegacyCanisterMeta {
            first_seen_ts: Option<u64>,
            last_contribution_ts: Option<u64>,
            last_cycles_probe_ts: Option<u64>,
            last_cycles_probe_result: Option<crate::state::CyclesProbeResult>,
        }

        #[derive(CandidType, candid::Deserialize, Clone)]
        struct LegacyState {
            config: LegacyConfig,
            distinct_canisters: BTreeSet<Principal>,
            canister_sources: BTreeMap<Principal, BTreeSet<crate::state::CanisterSource>>,
            contribution_history: BTreeMap<Principal, Vec<crate::state::ContributionSample>>,
            cycles_history: BTreeMap<Principal, Vec<crate::state::CyclesSample>>,
            per_canister_meta: BTreeMap<Principal, LegacyCanisterMeta>,
            last_indexed_staking_tx_id: Option<u64>,
            last_sns_discovery_ts: u64,
            last_completed_cycles_sweep_ts: u64,
            active_cycles_sweep: Option<crate::state::ActiveCyclesSweep>,
            main_lock_state_ts: Option<u64>,
            last_main_run_ts: u64,
        }

        let canister = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let mut canister_sources = BTreeMap::new();
        canister_sources.insert(canister, BTreeSet::from([crate::state::CanisterSource::MemoContribution]));
        let mut contribution_history = BTreeMap::new();
        contribution_history.insert(
            canister,
            vec![crate::state::ContributionSample {
                tx_id: 42,
                timestamp_nanos: Some(7),
                amount_e8s: 123,
                counts_toward_faucet: true,
            }],
        );
        let mut per_canister_meta = BTreeMap::new();
        per_canister_meta.insert(
            canister,
            LegacyCanisterMeta {
                first_seen_ts: Some(1),
                last_contribution_ts: Some(2),
                last_cycles_probe_ts: Some(3),
                last_cycles_probe_result: None,
            },
        );

        let legacy = LegacyState {
            config: LegacyConfig {
                staking_account: sample_account(),
                ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
                index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
                blackhole_canister_id: principal("e3mmv-5qaaa-aaaah-aadma-cai"),
                sns_wasm_canister_id: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
                enable_sns_tracking: false,
                scan_interval_seconds: 600,
                cycles_interval_seconds: 604800,
                min_tx_e8s: 100_000_000,
                max_cycles_entries_per_canister: 100,
                max_contribution_entries_per_canister: 100,
                max_index_pages_per_tick: 10,
                max_canisters_per_cycles_tick: 25,
            },
            distinct_canisters: BTreeSet::from([canister]),
            canister_sources,
            contribution_history,
            cycles_history: BTreeMap::new(),
            per_canister_meta,
            last_indexed_staking_tx_id: Some(42),
            last_sns_discovery_ts: 9,
            last_completed_cycles_sweep_ts: 10,
            active_cycles_sweep: None,
            main_lock_state_ts: Some(0),
            last_main_run_ts: 11,
        };

        let bytes = candid::encode_one((legacy,)).unwrap();
        let (stable,): (crate::state::StableState,) = candid::decode_one(&bytes).unwrap();
        let restored: State = stable.into();

        assert_eq!(restored.config.cmc_canister_id, None);
        assert_eq!(restored.config.faucet_canister_id, None);
        assert_eq!(restored.icp_burned_e8s, None);
        assert_eq!(restored.qualifying_contribution_count, None);
        assert_eq!(restored.per_canister_meta.get(&canister).map(|m| m.burned_e8s), Some(0));
        assert_eq!(restored.per_canister_meta.get(&canister).and_then(|m| m.last_burn_tx_id), None);
    }

    #[test]
    fn registered_canister_summaries_roll_up_qualifying_only() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoContribution]));
        st.contribution_history.insert(
            canister,
            vec![
                ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                },
                ContributionSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 50,
                    counts_toward_faucet: false,
                },
                ContributionSample {
                    tx_id: 3,
                    timestamp_nanos: Some(3_000_000_000),
                    amount_e8s: 250,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.cycles_history.insert(
            canister,
            vec![
                CyclesSample {
                    timestamp_nanos: 100,
                    cycles: 5,
                    source: CyclesSampleSource::BlackholeStatus,
                },
                CyclesSample {
                    timestamp_nanos: 200,
                    cycles: 8,
                    source: CyclesSampleSource::BlackholeStatus,
                },
            ],
        );
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                first_seen_ts: Some(1),
                last_contribution_ts: Some(3),
                last_cycles_probe_ts: Some(9),
                last_cycles_probe_result: None,
                ..Default::default()
            },
        );

        let summaries = registered_canister_summaries(&st);
        assert_eq!(summaries.len(), 1);
        let item = &summaries[0];
        assert_eq!(item.qualifying_contribution_count, 2);
        assert_eq!(item.total_qualifying_contributed_e8s, 350);
        assert_eq!(item.last_contribution_ts, Some(3));
        assert_eq!(item.latest_cycles, Some(8));
    }
}
