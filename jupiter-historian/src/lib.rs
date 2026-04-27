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
    CanisterMeta, CanisterSource, Config, CommitmentIndexFault, CommitmentSample, CyclesSample,
    InvalidCommitment, RecentCommitment, State,
};

pub(crate) const MAX_PUBLIC_QUERY_LIMIT: u32 = 100;
pub(crate) const MAX_RECENT_QUALIFYING_COMMITMENTS: usize = 500;
pub(crate) const MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS: usize = 100;
pub(crate) const MAX_RECENT_INVALID_COMMITMENTS: usize = 100;
pub(crate) const MAX_COMMITMENT_ENTRIES_PER_CANISTER_HARD_CAP: u32 = 250;
pub(crate) const MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP: u32 = 250;
pub(crate) const MAX_INDEX_PAGES_PER_TICK_HARD_CAP: u32 = 100;
pub(crate) const MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP: u32 = 500;


pub(crate) const MIN_MIN_TX_E8S: u64 = 10_000_000;

fn assert_non_anonymous_principal(name: &str, principal: Principal) {
    assert!(principal != Principal::anonymous(), "{name} must not be the anonymous principal");
}

fn assert_non_anonymous_account(name: &str, account: &Account) {
    assert_non_anonymous_principal(&format!("{name}.owner"), account.owner);
}

fn validate_config(cfg: &Config) {
    assert_non_anonymous_account("staking_account", &cfg.staking_account);
    assert_non_anonymous_account("output_source_account", &cfg.output_source_account);
    assert_non_anonymous_account("output_account", &cfg.output_account);
    assert_non_anonymous_account("rewards_account", &cfg.rewards_account);
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
    assert!(cfg.output_source_account != cfg.output_account, "output_source_account and output_account must be distinct");
    assert!(cfg.output_source_account != cfg.rewards_account, "output_source_account and rewards_account must be distinct");
    assert!(cfg.output_account != cfg.rewards_account, "output_account and rewards_account must be distinct");
    assert!(cfg.scan_interval_seconds > 0, "scan_interval_seconds must be greater than 0");
    assert!(cfg.cycles_interval_seconds > 0, "cycles_interval_seconds must be greater than 0");
    assert!(cfg.min_tx_e8s >= MIN_MIN_TX_E8S, "min_tx_e8s must be at least {MIN_MIN_TX_E8S} e8s (0.1 ICP)");
}

fn commitment_sort_key(item: &RecentCommitment) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn invalid_commitment_sort_key(item: &InvalidCommitment) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn clamp_public_limit(limit: Option<u32>, default: u32) -> usize {
    limit.unwrap_or(default).clamp(1, MAX_PUBLIC_QUERY_LIMIT) as usize
}

fn clamp_cycles_entries_per_canister(value: u32) -> u32 {
    value.clamp(1, MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP)
}

fn clamp_commitment_entries_per_canister(value: u32) -> u32 {
    value.clamp(1, MAX_COMMITMENT_ENTRIES_PER_CANISTER_HARD_CAP)
}

fn clamp_index_pages_per_tick(value: u32) -> u32 {
    value.clamp(1, MAX_INDEX_PAGES_PER_TICK_HARD_CAP)
}

fn clamp_canisters_per_cycles_tick(value: u32) -> u32 {
    value.clamp(1, MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP)
}


fn format_module_hash_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
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

fn normalize_recent_commitment_bucket(items: &mut Vec<RecentCommitment>, counts_toward_faucet: bool, max_entries: usize) {
    items.retain(|item| item.counts_toward_faucet == counts_toward_faucet);
    items.sort_by(|a, b| commitment_sort_key(b).cmp(&commitment_sort_key(a)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(max_entries);
}

fn normalize_recent_invalid_commitments(items: &mut Vec<InvalidCommitment>) {
    items.sort_by(|a, b| invalid_commitment_sort_key(b).cmp(&invalid_commitment_sort_key(a)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(MAX_RECENT_INVALID_COMMITMENTS);
}

fn memo_source_is_registered(st: &State, canister_id: &Principal, sources: &BTreeSet<CanisterSource>) -> bool {
    sources.contains(&CanisterSource::MemoCommitment)
        && commitment_history_snapshot(st, *canister_id)
            .into_iter()
            .any(|item| item.counts_toward_faucet)
}

fn visible_sources_for_canister(st: &State, canister_id: &Principal) -> Option<BTreeSet<CanisterSource>> {
    let mut sources = st.canister_sources.get(canister_id)?.clone();
    if !memo_source_is_registered(st, canister_id, &sources) {
        sources.remove(&CanisterSource::MemoCommitment);
    }
    if sources.is_empty() {
        return None;
    }
    Some(sources)
}



fn clamp_config(st: &mut State) {
    st.config.max_cycles_entries_per_canister =
        clamp_cycles_entries_per_canister(st.config.max_cycles_entries_per_canister);
    st.config.max_commitment_entries_per_canister =
        clamp_commitment_entries_per_canister(st.config.max_commitment_entries_per_canister);
    st.config.max_index_pages_per_tick = clamp_index_pages_per_tick(st.config.max_index_pages_per_tick);
    st.config.max_canisters_per_cycles_tick =
        clamp_canisters_per_cycles_tick(st.config.max_canisters_per_cycles_tick);
}

fn normalize_runtime_state(st: &mut State) {
    clamp_config(st);

    let mut recent_commitments = st.recent_commitments.take().unwrap_or_default();
    recent_commitments.extend(fallback_recent_qualifying_commitments_state(st));
    let mut recent_under_threshold = st.recent_under_threshold_commitments.take().unwrap_or_default();
    recent_under_threshold.extend(fallback_recent_under_threshold_commitments_state(st));

    for item in recent_commitments.iter().filter(|item| !item.counts_toward_faucet).cloned() {
        recent_under_threshold.push(item);
    }
    recent_commitments.retain(|item| item.counts_toward_faucet);

    let mut empty_histories = Vec::new();
    for (canister_id, history) in st.commitment_history.iter_mut() {
        let mut removed = Vec::new();
        history.retain(|item| {
            if item.counts_toward_faucet {
                true
            } else {
                removed.push(RecentCommitment {
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
        if history.len() > st.config.max_commitment_entries_per_canister as usize {
            let excess = history.len() - st.config.max_commitment_entries_per_canister as usize;
            history.drain(0..excess);
        }
        if history.is_empty() {
            empty_histories.push(*canister_id);
        }
    }
    for canister_id in empty_histories {
        st.commitment_history.remove(&canister_id);
    }

    let stale_memo_only_canisters: Vec<_> = st
        .canister_sources
        .iter()
        .filter_map(|(canister_id, sources)| {
            if sources.contains(&CanisterSource::MemoCommitment)
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
            sources.remove(&CanisterSource::MemoCommitment);
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

    normalize_recent_commitment_bucket(&mut recent_commitments, true, MAX_RECENT_QUALIFYING_COMMITMENTS);
    normalize_recent_commitment_bucket(&mut recent_under_threshold, false, MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS);
    st.recent_commitments = Some(recent_commitments);
    st.recent_under_threshold_commitments = Some(recent_under_threshold);

    let mut recent_invalid = st.recent_invalid_commitments.take().unwrap_or_default();
    normalize_recent_invalid_commitments(&mut recent_invalid);
    st.recent_invalid_commitments = Some(recent_invalid);

    st.qualifying_commitment_count = Some(fallback_qualifying_commitment_count(st));

    let commitment_last_ts: BTreeMap<_, _> = commitment_history_canister_ids(st)
        .into_iter()
        .map(|canister_id| {
            let history = commitment_history_snapshot(st, canister_id);
            (
                canister_id,
                history
                    .iter()
                    .filter_map(|item| item.timestamp_nanos.map(|ts| ts / 1_000_000_000))
                    .max(),
            )
        })
        .collect();
    for (canister_id, meta) in st.per_canister_meta.iter_mut() {
        meta.last_commitment_ts = commitment_last_ts.get(canister_id).copied().flatten();
    }

    let distinct_canisters: BTreeSet<_> = st
        .canister_sources
        .keys()
        .copied()
        .chain(commitment_history_canister_ids(st))
        .chain(cycles_history_canister_ids(st))
        .collect();
    st.distinct_canisters = distinct_canisters;
    rebuild_registered_canister_summaries_cache(st);
}

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub staking_account: Account,
    pub output_source_account: Option<Account>,
    pub output_account: Option<Account>,
    pub rewards_account: Option<Account>,
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
    pub max_commitment_entries_per_canister: Option<u32>,
    pub max_index_pages_per_tick: Option<u32>,
    pub max_canisters_per_cycles_tick: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct UpgradeArgs {
    pub staking_account: Option<Account>,
    pub ledger_canister_id: Option<Principal>,
    pub index_canister_id: Option<Principal>,
    pub enable_sns_tracking: Option<bool>,
    pub clear_commitment_index_fault: Option<bool>,
    pub output_source_account: Option<Account>,
    pub output_account: Option<Account>,
    pub rewards_account: Option<Account>,
    pub scan_interval_seconds: Option<u64>,
    pub cycles_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
    pub max_cycles_entries_per_canister: Option<u32>,
    pub max_commitment_entries_per_canister: Option<u32>,
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
pub struct GetCommitmentHistoryArgs {
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
pub struct CommitmentHistoryPage {
    pub items: Vec<CommitmentSample>,
    pub next_start_after_tx_id: Option<u64>,
}


#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterOverview {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
    pub meta: CanisterMeta,
    pub cycles_points: u32,
    pub commitment_points: u32,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicCounts {
    pub registered_canister_count: u64,
    pub qualifying_commitment_count: u64,
    pub sns_discovered_canister_count: u64,
    pub total_output_e8s: u64,
    pub total_rewards_e8s: u64,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicStatus {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub faucet_canister_id: Principal,
    pub cmc_canister_id: Option<Principal>,
    pub output_source_account: Option<Account>,
    pub output_account: Option<Account>,
    pub rewards_account: Option<Account>,
    pub index_canister_id: Option<Principal>,
    pub last_index_run_ts: Option<u64>,
    pub index_interval_seconds: u64,
    pub last_completed_cycles_sweep_ts: Option<u64>,
    pub cycles_interval_seconds: u64,
    pub heap_memory_bytes: Option<u64>,
    pub stable_memory_bytes: Option<u64>,
    pub total_memory_bytes: Option<u64>,
    pub commitment_index_fault: Option<CommitmentIndexFault>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRegisteredCanisterSummariesArgs {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RegisteredCanisterSummary {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
    pub qualifying_commitment_count: u64,
    pub total_qualifying_committed_e8s: u64,
    pub last_commitment_ts: Option<u64>,
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
pub struct ListRecentCommitmentsArgs {
    pub limit: Option<u32>,
    pub qualifying_only: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone, Serialize, Debug, PartialEq, Eq)]
pub enum RecentCommitmentOutcomeCategory {
    QualifyingCommitment,
    UnderThresholdCommitment,
    InvalidTargetMemo,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RecentCommitmentListItem {
    pub canister_id: Option<Principal>,
    pub memo_text: Option<String>,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
    pub outcome_category: RecentCommitmentOutcomeCategory,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRecentCommitmentsResponse {
    pub items: Vec<RecentCommitmentListItem>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterModuleHash {
    pub canister_id: Principal,
    pub module_hash_hex: Option<String>,
    pub controllers: Option<Vec<Principal>>,
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

pub(crate) fn mainnet_disburser_id() -> Principal {
    Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai").expect("invalid hardcoded disburser principal")
}

pub(crate) fn mainnet_rewards_id() -> Principal {
    Principal::from_text("alk7f-5aaaa-aaaar-qb4ra-cai").expect("invalid hardcoded rewards principal")
}

pub(crate) fn mainnet_disburser_staging_account() -> Account {
    Account { owner: mainnet_disburser_id(), subaccount: None }
}

pub(crate) fn mainnet_output_account() -> Account {
    Account { owner: mainnet_faucet_id(), subaccount: None }
}

pub(crate) fn mainnet_rewards_account() -> Account {
    Account { owner: mainnet_rewards_id(), subaccount: None }
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
        output_source_account: args.output_source_account.unwrap_or_else(mainnet_disburser_staging_account),
        output_account: args.output_account.unwrap_or_else(mainnet_output_account),
        rewards_account: args.rewards_account.unwrap_or_else(mainnet_rewards_account),
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
        max_commitment_entries_per_canister: clamp_commitment_entries_per_canister(args.max_commitment_entries_per_canister.unwrap_or(100)),
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

fn qualifying_rollup(history: &[CommitmentSample]) -> (u64, u64, Option<u64>) {
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

fn commitment_history_canister_ids(st: &State) -> BTreeSet<Principal> {
    st.commitment_history
        .keys()
        .copied()
        .chain(state::stable_commitment_history_keys())
        .collect()
}

fn cycles_history_canister_ids(st: &State) -> BTreeSet<Principal> {
    st.cycles_history
        .keys()
        .copied()
        .chain(state::stable_cycles_history_keys())
        .collect()
}

fn commitment_history_snapshot(st: &State, canister_id: Principal) -> Vec<CommitmentSample> {
    st.commitment_history
        .get(&canister_id)
        .cloned()
        .unwrap_or_else(|| state::stable_commitment_history_for(canister_id))
}

fn cycles_history_snapshot(st: &State, canister_id: Principal) -> Vec<CyclesSample> {
    st.cycles_history
        .get(&canister_id)
        .cloned()
        .unwrap_or_else(|| state::stable_cycles_history_for(canister_id))
}

fn fallback_qualifying_commitment_count(st: &State) -> u64 {
    commitment_history_canister_ids(st)
        .into_iter()
        .flat_map(|canister_id| commitment_history_snapshot(st, canister_id).into_iter())
        .filter(|item| item.counts_toward_faucet)
        .count() as u64
}

fn fallback_recent_qualifying_commitments_state(st: &State) -> Vec<RecentCommitment> {
    let mut items: Vec<_> = commitment_history_canister_ids(st)
        .into_iter()
        .flat_map(|canister_id| {
            commitment_history_snapshot(st, canister_id)
                .into_iter()
                .filter(|item| item.counts_toward_faucet)
                .map(move |item| RecentCommitment {
                    canister_id,
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                })
        })
        .collect();
    normalize_recent_commitment_bucket(&mut items, true, MAX_RECENT_QUALIFYING_COMMITMENTS);
    items
}

fn fallback_recent_under_threshold_commitments_state(st: &State) -> Vec<RecentCommitment> {
    let mut items: Vec<_> = commitment_history_canister_ids(st)
        .into_iter()
        .flat_map(|canister_id| {
            commitment_history_snapshot(st, canister_id)
                .into_iter()
                .filter(|item| !item.counts_toward_faucet)
                .map(move |item| RecentCommitment {
                    canister_id,
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                })
        })
        .collect();
    normalize_recent_commitment_bucket(&mut items, false, MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS);
    items
}

fn fallback_recent_commitments(st: &State) -> Vec<RecentCommitmentListItem> {
    let mut items: Vec<_> = fallback_recent_qualifying_commitments_state(st)
        .into_iter()
        .map(|item| RecentCommitmentListItem {
            canister_id: Some(item.canister_id),
            memo_text: Some(item.canister_id.to_text()),
            tx_id: item.tx_id,
            timestamp_nanos: item.timestamp_nanos,
            amount_e8s: item.amount_e8s,
            counts_toward_faucet: true,
            outcome_category: RecentCommitmentOutcomeCategory::QualifyingCommitment,
        })
        .collect();
    items.extend(
        fallback_recent_under_threshold_commitments_state(st)
            .into_iter()
            .map(|item| RecentCommitmentListItem {
                canister_id: Some(item.canister_id),
                memo_text: Some(item.canister_id.to_text()),
                tx_id: item.tx_id,
                timestamp_nanos: item.timestamp_nanos,
                amount_e8s: item.amount_e8s,
                counts_toward_faucet: false,
                outcome_category: RecentCommitmentOutcomeCategory::UnderThresholdCommitment,
            }),
    );
    if let Some(invalid) = &st.recent_invalid_commitments {
        items.extend(invalid.iter().cloned().map(|item| RecentCommitmentListItem {
            canister_id: None,
            memo_text: Some(item.memo_text),
            tx_id: item.tx_id,
            timestamp_nanos: item.timestamp_nanos,
            amount_e8s: item.amount_e8s,
            counts_toward_faucet: false,
            outcome_category: RecentCommitmentOutcomeCategory::InvalidTargetMemo,
        }));
    }
    items.sort_by(|a, b| {
        let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
        let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
        b_key.cmp(&a_key)
    });
    items.truncate(MAX_RECENT_QUALIFYING_COMMITMENTS + MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS + MAX_RECENT_INVALID_COMMITMENTS);
    items
}

fn initialize_config_defaults_if_missing(st: &mut State) {
    if st.config.cmc_canister_id.is_none() {
        st.config.cmc_canister_id = Some(mainnet_cmc_id());
    }
    if st.config.faucet_canister_id.is_none() {
        st.config.faucet_canister_id = Some(mainnet_faucet_id());
    }
    if st.total_output_e8s.is_none() {
        st.total_output_e8s = Some(0);
    }
    if st.total_rewards_e8s.is_none() {
        st.total_rewards_e8s = Some(0);
    }
    if st.last_completed_route_sweep_ts.is_none() {
        st.last_completed_route_sweep_ts = Some(0);
    }
}

fn initialize_derived_state_if_missing(st: &mut State) {
    if st.qualifying_commitment_count.is_none() {
        st.qualifying_commitment_count = Some(fallback_qualifying_commitment_count(st));
    }
    if st.recent_commitments.is_none() {
        st.recent_commitments = Some(fallback_recent_qualifying_commitments_state(st));
    }
    if st.recent_under_threshold_commitments.is_none() {
        st.recent_under_threshold_commitments = Some(fallback_recent_under_threshold_commitments_state(st));
    }
    if st.recent_invalid_commitments.is_none() {
        st.recent_invalid_commitments = Some(Vec::new());
    }
    if st.last_index_run_ts.is_none() {
        st.last_index_run_ts = Some(st.last_main_run_ts);
    }
    if st.registered_canister_summaries_cache.is_none()
        || st.registered_canister_summaries_total_desc_index.is_none()
    {
        rebuild_registered_canister_summaries_cache(st);
    }
}

fn registered_canister_summary_for(st: &State, canister_id: Principal) -> Option<RegisteredCanisterSummary> {
    let sources = visible_sources_for_canister(st, &canister_id)?;
    let history = commitment_history_snapshot(st, canister_id);
    if history.is_empty() {
        return None;
    }
    let (qualifying_commitment_count, total_qualifying_committed_e8s, rollup_last_ts) = qualifying_rollup(&history);
    let meta = st.per_canister_meta.get(&canister_id).cloned().unwrap_or_default();
    let cycles_history = cycles_history_snapshot(st, canister_id);
    Some(RegisteredCanisterSummary {
        canister_id,
        sources: sources.into_iter().collect(),
        qualifying_commitment_count,
        total_qualifying_committed_e8s,
        last_commitment_ts: meta.last_commitment_ts.or(rollup_last_ts),
        latest_cycles: latest_cycles(&cycles_history),
        last_cycles_probe_ts: meta.last_cycles_probe_ts,
    })
}

fn registered_canister_summary_total_desc_key(item: &RegisteredCanisterSummary) -> (Reverse<u64>, Principal) {
    (Reverse(item.total_qualifying_committed_e8s), item.canister_id)
}

fn remove_registered_canister_from_total_desc_index(index: &mut Vec<Principal>, canister_id: Principal) {
    index.retain(|existing| *existing != canister_id);
}

fn insert_registered_canister_into_total_desc_index(
    cache: &BTreeMap<Principal, RegisteredCanisterSummary>,
    index: &mut Vec<Principal>,
    canister_id: Principal,
) {
    let Some(summary) = cache.get(&canister_id) else {
        return;
    };
    index.retain(|existing| *existing != canister_id && cache.contains_key(existing));
    let summary_key = registered_canister_summary_total_desc_key(summary);
    let insert_at = index
        .binary_search_by(|existing_canister_id| {
            let existing_summary = cache
                .get(existing_canister_id)
                .expect("ranked canister missing from summary cache");
            registered_canister_summary_total_desc_key(existing_summary).cmp(&summary_key)
        })
        .unwrap_or_else(|position| position);
    index.insert(insert_at, canister_id);
}

fn registered_canister_summaries_total_desc_page(
    st: &State,
    page: u32,
    page_size: u32,
) -> Option<ListRegisteredCanisterSummariesResponse> {
    let cache = st.registered_canister_summaries_cache.as_ref()?;
    let index = st.registered_canister_summaries_total_desc_index.as_ref()?;
    if index.len() != cache.len() || index.iter().any(|canister_id| !cache.contains_key(canister_id)) {
        return None;
    }
    let total = index.len() as u64;
    let start = page.saturating_mul(page_size) as usize;
    let end = start.saturating_add(page_size as usize).min(index.len());
    let items = if start >= index.len() {
        Vec::new()
    } else {
        index[start..end]
            .iter()
            .filter_map(|canister_id| cache.get(canister_id).cloned())
            .collect()
    };
    Some(ListRegisteredCanisterSummariesResponse {
        items,
        page,
        page_size,
        total,
    })
}

pub(crate) fn refresh_registered_canister_summary(st: &mut State, canister_id: Principal) {
    let summary = registered_canister_summary_for(st, canister_id);
    let State {
        registered_canister_summaries_cache,
        registered_canister_summaries_total_desc_index,
        ..
    } = st;
    let cache = registered_canister_summaries_cache.get_or_insert_with(BTreeMap::new);
    let total_desc_index = registered_canister_summaries_total_desc_index.get_or_insert_with(Vec::new);
    remove_registered_canister_from_total_desc_index(total_desc_index, canister_id);
    if let Some(summary) = summary {
        cache.insert(canister_id, summary);
        insert_registered_canister_into_total_desc_index(cache, total_desc_index, canister_id);
    } else {
        cache.remove(&canister_id);
    }
}

pub(crate) fn rebuild_registered_canister_summaries_cache(st: &mut State) {
    let canister_ids: Vec<_> = st.canister_sources.keys().copied().collect();
    st.registered_canister_summaries_cache = Some(BTreeMap::new());
    st.registered_canister_summaries_total_desc_index = Some(Vec::new());
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
        if let Some(v) = args.staking_account {
            st.config.staking_account = v;
        }
        if let Some(v) = args.ledger_canister_id {
            st.config.ledger_canister_id = v;
        }
        if let Some(v) = args.index_canister_id {
            st.config.index_canister_id = v;
        }
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
        if let Some(v) = args.max_commitment_entries_per_canister {
            st.config.max_commitment_entries_per_canister = clamp_commitment_entries_per_canister(v);
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
        if let Some(v) = args.output_source_account {
            st.config.output_source_account = v;
        }
        if let Some(v) = args.output_account {
            st.config.output_account = v;
        }
        if let Some(v) = args.rewards_account {
            st.config.rewards_account = v;
        }
        if args.clear_commitment_index_fault.unwrap_or(false) {
            st.commitment_index_fault = None;
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
    // Persist only the historian root on upgrade. Commitment/cycles histories are
    // restored lazily from stable entry/index maps, so rewriting all durable sections
    // here would clobber those bulk histories with an intentionally sparse heap view.
    state::set_state_root_only(st);
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
        let history = cycles_history_snapshot(st, args.canister_id);
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
        CyclesHistoryPage {
            items,
            next_start_after_ts: next,
        }
    })
}

fn commitment_history_page(args: GetCommitmentHistoryArgs) -> CommitmentHistoryPage {
    state::with_state(|st| {
        let descending = args.descending.unwrap_or(false);
        let limit = clamp_public_limit(args.limit, 100);
        let mut items = Vec::new();
        let mut next = None;
        let history = commitment_history_snapshot(st, args.canister_id);
        let iter: Box<dyn Iterator<Item = &CommitmentSample>> = if descending {
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
                next = items.last().map(|sample: &CommitmentSample| sample.tx_id);
                break;
            }
            items.push(item.clone());
        }
        CommitmentHistoryPage {
            items,
            next_start_after_tx_id: next,
        }
    })
}

#[ic_cdk::query]
fn get_commitment_history(args: GetCommitmentHistoryArgs) -> CommitmentHistoryPage {
    commitment_history_page(args)
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
        let commitment_points = st
            .commitment_history
            .get(&canister_id)
            .map(|v| v.len() as u32)
            .unwrap_or(0);
        Some(CanisterOverview {
            canister_id,
            sources,
            meta,
            cycles_points,
            commitment_points,
        })
    })
}

#[ic_cdk::query]
fn get_public_counts() -> PublicCounts {
    state::with_state(|st| PublicCounts {
        registered_canister_count: count_registered_canisters(st),
        qualifying_commitment_count: st
            .qualifying_commitment_count
            .unwrap_or_else(|| fallback_qualifying_commitment_count(st)),
        sns_discovered_canister_count: count_sns_discovered_canisters(st),
        total_output_e8s: st.total_output_e8s.unwrap_or(0),
        total_rewards_e8s: st.total_rewards_e8s.unwrap_or(0),
    })
}

#[ic_cdk::query]
fn get_public_status() -> PublicStatus {
    let heap_memory_bytes = allocated_heap_memory_bytes();
    let stable_memory_bytes = allocated_stable_memory_bytes();
    state::with_state(|st| PublicStatus {
        staking_account: st.config.staking_account.clone(),
        ledger_canister_id: st.config.ledger_canister_id,
        faucet_canister_id: effective_faucet_canister_id(st),
        cmc_canister_id: st.config.cmc_canister_id,
        output_source_account: Some(st.config.output_source_account.clone()),
        output_account: Some(st.config.output_account.clone()),
        rewards_account: Some(st.config.rewards_account.clone()),
        index_canister_id: Some(st.config.index_canister_id),
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
        commitment_index_fault: st.commitment_index_fault.clone(),
    })
}

fn source_module_hash_canister_ids() -> Vec<Principal> {
    [
        "uccpi-cqaaa-aaaar-qby3q-cai",
        "acjuz-liaaa-aaaar-qb4qq-cai",
        "j5gs6-uiaaa-aaaar-qb5cq-cai",
        "afisn-gqaaa-aaaar-qb4qa-cai",
        "alk7f-5aaaa-aaaar-qb4ra-cai",
        "jufzc-caaaa-aaaar-qb5da-cai",
    ]
    .into_iter()
    .map(|canister_id| Principal::from_text(canister_id).expect("invalid hardcoded canister id"))
    .collect()
}

#[ic_cdk::update]
async fn get_canister_module_hashes() -> Vec<CanisterModuleHash> {
    let canister_ids = source_module_hash_canister_ids();
    let mut hashes = Vec::with_capacity(canister_ids.len());
    for canister_id in canister_ids {
        let request = ic_cdk::management_canister::CanisterInfoArgs {
            canister_id,
            num_requested_changes: Some(0),
        };
        let (module_hash_hex, controllers) = match ic_cdk::management_canister::canister_info(&request).await {
            Ok(result) => (
                result
                    .module_hash
                    .map(|module_hash| format_module_hash_hex(module_hash.as_ref())),
                Some(result.controllers),
            ),
            Err(err) => {
                ic_cdk::println!("get_canister_module_hashes failed for {}: {:?}", canister_id, err);
                (None, None)
            }
        };
        hashes.push(CanisterModuleHash {
            canister_id,
            module_hash_hex,
            controllers,
        });
    }
    hashes
}

#[ic_cdk::query]
fn list_registered_canister_summaries(
    args: ListRegisteredCanisterSummariesArgs,
) -> ListRegisteredCanisterSummariesResponse {
    state::with_state(|st| {
        let page = args.page.unwrap_or(0);
        let page_size = args.page_size.unwrap_or(25).clamp(1, 100);
        if let Some(response) = registered_canister_summaries_total_desc_page(st, page, page_size) {
            return response;
        }
        let mut items = registered_canister_summaries(st);
        items.sort_by_key(registered_canister_summary_total_desc_key);
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
fn list_recent_commitments(args: ListRecentCommitmentsArgs) -> ListRecentCommitmentsResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 20);
        let qualifying_only = args.qualifying_only.unwrap_or(false);
        let mut items: Vec<RecentCommitmentListItem> = if let Some(recent) = &st.recent_commitments {
            let mut merged: Vec<RecentCommitmentListItem> = recent
                .iter()
                .cloned()
                .map(|item| RecentCommitmentListItem {
                    canister_id: Some(item.canister_id),
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                    outcome_category: RecentCommitmentOutcomeCategory::QualifyingCommitment,
                })
                .collect();
            if let Some(low_value) = &st.recent_under_threshold_commitments {
                merged.extend(low_value.iter().cloned().map(|item| RecentCommitmentListItem {
                    canister_id: Some(item.canister_id),
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentCommitmentOutcomeCategory::UnderThresholdCommitment,
                }));
            }
            if let Some(invalid) = &st.recent_invalid_commitments {
                merged.extend(invalid.iter().cloned().map(|item| RecentCommitmentListItem {
                    canister_id: None,
                    memo_text: Some(item.memo_text),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentCommitmentOutcomeCategory::InvalidTargetMemo,
                }));
            }
            merged.sort_by(|a, b| {
                let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
                let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
                b_key.cmp(&a_key)
            });
            merged
        } else {
            fallback_recent_commitments(st)
        };
        if qualifying_only {
            items.retain(|item| item.counts_toward_faucet);
        }
        items.truncate(limit);
        ListRecentCommitmentsResponse { items }
    })
}


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
#[ic_cdk::query]
fn debug_state() -> DebugState {
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
fn debug_config() -> DebugConfig {
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
        st.total_output_e8s = Some(0);
        st.total_rewards_e8s = Some(0);
        st.recent_commitments = Some(Vec::new());
        st.recent_under_threshold_commitments = Some(Vec::new());
        st.recent_invalid_commitments = Some(Vec::new());
        st.last_index_run_ts = Some(0);
        st.commitment_index_fault = None;
        st.registered_canister_summaries_cache = Some(BTreeMap::new());
        st.registered_canister_summaries_total_desc_index = Some(Vec::new());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CanisterMeta, CyclesSampleSource, InvalidCommitment};
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
                output_source_account: Account { owner: principal("uccpi-cqaaa-aaaar-qby3q-cai"), subaccount: None },
                output_account: Account { owner: principal("acjuz-liaaa-aaaar-qb4qq-cai"), subaccount: None },
                rewards_account: Account { owner: principal("alk7f-5aaaa-aaaar-qb4ra-cai"), subaccount: None },
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
                max_commitment_entries_per_canister: 100,
                max_index_pages_per_tick: 10,
                max_canisters_per_cycles_tick: 25,
            },
            distinct_canisters: BTreeSet::new(),
            canister_sources: BTreeMap::new(),
            commitment_history: BTreeMap::new(),
            cycles_history: BTreeMap::new(),
            per_canister_meta: BTreeMap::new(),
            registered_canister_summaries_cache: None,
            registered_canister_summaries_total_desc_index: None,
            last_indexed_staking_tx_id: None,
            oldest_indexed_staking_tx_id: None,
            staking_index_descending: None,
            staking_backfill_complete: Some(false),
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
            active_route_sweep: None,
            active_sns_discovery: None,
            main_lock_state_ts: Some(0),
            last_main_run_ts: 1,
            qualifying_commitment_count: None,
            total_output_e8s: None,
            total_rewards_e8s: None,
            icp_burned_e8s: None,
            recent_commitments: None,
            recent_under_threshold_commitments: None,
            recent_invalid_commitments: None,
            recent_burns: None,
            last_index_run_ts: None,
            commitment_index_fault: None,
        }
    }


    #[test]
    fn config_from_init_args_uses_mainnet_defaults_for_optional_canisters() {
        let cfg = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
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
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });

        assert_eq!(cfg.ledger_canister_id, mainnet_ledger_id());
        assert_eq!(cfg.index_canister_id, mainnet_index_id());
        assert_eq!(cfg.blackhole_canister_id, mainnet_blackhole_id());
        assert_eq!(cfg.output_source_account, mainnet_disburser_staging_account());
        assert_eq!(cfg.output_account, mainnet_output_account());
        assert_eq!(cfg.rewards_account, mainnet_rewards_account());
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
            output_source_account: None,
            output_account: None,
            rewards_account: None,
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
            max_commitment_entries_per_canister: None,
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
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 7,
                timestamp_nanos: Some(9_000_000_000),
                amount_e8s: 123_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.per_canister_meta.insert(
            canister,
            CanisterMeta {
                last_commitment_ts: Some(9),
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
        assert_eq!(cached.qualifying_commitment_count, 1);
        assert_eq!(cached.total_qualifying_committed_e8s, 123_000_000);
        assert_eq!(cached.last_commitment_ts, Some(9));
        assert_eq!(cached.latest_cycles, Some(777));
        assert_eq!(cached.last_cycles_probe_ts, Some(10));
        assert_eq!(
            st.registered_canister_summaries_total_desc_index,
            Some(vec![canister]),
        );
    }

    #[test]
    fn refresh_registered_canister_summary_keeps_total_desc_index_in_dashboard_order() {
        let first = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let second = principal("uxrrr-q7777-77774-qaaaq-cai");
        let mut st = base_state();

        for (canister, amount_e8s) in [(first, 123_000_000), (second, 456_000_000)] {
            st.canister_sources
                .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
                    tx_id: amount_e8s / 1_000_000,
                    timestamp_nanos: Some(9_000_000_000),
                    amount_e8s,
                    counts_toward_faucet: true,
                }],
            );
            refresh_registered_canister_summary(&mut st, canister);
        }

        assert_eq!(
            st.registered_canister_summaries_total_desc_index,
            Some(vec![second, first]),
        );

        st.commitment_history.insert(
            first,
            vec![CommitmentSample {
                tx_id: 999,
                timestamp_nanos: Some(10_000_000_000),
                amount_e8s: 789_000_000,
                counts_toward_faucet: true,
            }],
        );
        refresh_registered_canister_summary(&mut st, first);

        assert_eq!(
            st.registered_canister_summaries_total_desc_index,
            Some(vec![first, second]),
        );
    }


    #[test]
    fn list_registered_canister_summaries_falls_back_to_slow_path_when_total_desc_index_drifts() {
        let first = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let second = principal("uxrrr-q7777-77774-qaaaq-cai");
        let mut cache = BTreeMap::new();
        cache.insert(
            first,
            RegisteredCanisterSummary {
                canister_id: first,
                sources: vec![CanisterSource::MemoCommitment],
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 123_000_000,
                last_commitment_ts: Some(1),
                latest_cycles: None,
                last_cycles_probe_ts: None,
            },
        );
        cache.insert(
            second,
            RegisteredCanisterSummary {
                canister_id: second,
                sources: vec![CanisterSource::MemoCommitment],
                qualifying_commitment_count: 2,
                total_qualifying_committed_e8s: 456_000_000,
                last_commitment_ts: Some(2),
                latest_cycles: None,
                last_cycles_probe_ts: None,
            },
        );

        let mut st = base_state();
        st.registered_canister_summaries_cache = Some(cache);
        st.registered_canister_summaries_total_desc_index = Some(vec![first]);
        state::set_state(st);

        let response = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        });

        assert_eq!(response.total, 2);
        assert_eq!(response.items.iter().map(|item| item.canister_id).collect::<Vec<_>>(), vec![second, first]);
    }

    #[test]
    #[should_panic(expected = "min_tx_e8s must be at least")]
    fn config_from_init_args_rejects_threshold_below_minimum() {
        let _ = config_from_init_args(InitArgs {
            staking_account: sample_account(),
            output_source_account: None,
            output_account: None,
            rewards_account: None,
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
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
        });
    }

    #[test]
    fn apply_upgrade_args_updates_tuning_fields_and_preserves_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 10,
                counts_toward_faucet: true,
            }],
        );
        st.main_lock_state_ts = Some(99);
        st.commitment_index_fault = Some(CommitmentIndexFault {
            observed_at_ts: 77,
            last_cursor_tx_id: Some(66),
            offending_tx_id: 77,
            message: "latched".to_string(),
        });

        let original_account = st.config.staking_account.clone();
        let original_ledger = st.config.ledger_canister_id;
        let original_index = st.config.index_canister_id;

        apply_upgrade_args(
            &mut st,
            Some(UpgradeArgs {
                enable_sns_tracking: Some(true),
                clear_commitment_index_fault: Some(true),
                scan_interval_seconds: Some(123),
                cycles_interval_seconds: Some(456),
                min_tx_e8s: Some(MIN_MIN_TX_E8S),
                max_cycles_entries_per_canister: Some(11),
                max_commitment_entries_per_canister: Some(12),
                max_index_pages_per_tick: Some(13),
                max_canisters_per_cycles_tick: Some(14),
                blackhole_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                sns_wasm_canister_id: Some(principal("qaa6y-5yaaa-aaaaa-aaafa-cai")),
                ..UpgradeArgs::default()
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
        assert_eq!(st.config.max_commitment_entries_per_canister, 12);
        assert_eq!(st.config.max_index_pages_per_tick, 13);
        assert_eq!(st.config.max_canisters_per_cycles_tick, 14);
        assert_eq!(st.commitment_history.get(&canister).map(|v| v.len()), Some(1));
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
    fn registered_canister_count_requires_qualifying_memo_commitment_history() {
        let memo_only = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let sns_only = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let both = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");

        let mut st = base_state();
        st.canister_sources.insert(memo_only, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        st.canister_sources.insert(
            both,
            BTreeSet::from([CanisterSource::MemoCommitment, CanisterSource::SnsDiscovery]),
        );
        st.commitment_history.insert(
            memo_only,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 80_000_000,
                counts_toward_faucet: true,
            }],
        );
        st.commitment_history.insert(
            both,
            vec![CommitmentSample {
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
        st.canister_sources.insert(memo_canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.canister_sources.insert(sns_only, BTreeSet::from([CanisterSource::SnsDiscovery]));
        st.commitment_history.insert(
            memo_canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 80_000_000,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
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
        assert_eq!(counts.qualifying_commitment_count, 1);
        assert_eq!(counts.sns_discovered_canister_count, 1);
        assert_eq!(counts.total_output_e8s, 0);
        assert_eq!(counts.total_rewards_e8s, 0);
    }

    #[test]
    fn get_public_counts_excludes_non_qualifying_memo_canisters_from_registered_totals() {
        let memo_canister = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let mut st = base_state();
        st.canister_sources.insert(memo_canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            memo_canister,
            vec![CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1_000_000_000),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            }],
        );
        state::set_state(st);

        let counts = get_public_counts();
        assert_eq!(counts.registered_canister_count, 0);
        assert_eq!(counts.qualifying_commitment_count, 0);
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
        });
        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }


    #[test]
    fn list_registered_canister_summaries_excludes_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
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
        });
        assert_eq!(response.total, 0);
        assert!(response.items.is_empty());
    }

    #[test]
    fn get_canister_overview_hides_non_qualifying_memo_only_canisters() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
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
    fn list_registered_canister_summaries_uses_canister_id_as_tie_breaker_for_stable_pagination() {
        let a = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let b = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let mut st = base_state();
        for canister in [a, b] {
            st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000),
                    amount_e8s: 50_000_000,
                    counts_toward_faucet: true,
                }],
            );
            st.per_canister_meta.insert(
                canister,
                CanisterMeta {
                    last_commitment_ts: Some(1_000),
                    ..CanisterMeta::default()
                },
            );
        }
        state::set_state(st);

        let first_page = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(1),
        });
        let second_page = list_registered_canister_summaries(ListRegisteredCanisterSummariesArgs {
            page: Some(1),
            page_size: Some(1),
        });

        assert_eq!(first_page.total, 2);
        assert_eq!(second_page.total, 2);
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(first_page.items[0].canister_id, b.min(a));
        assert_eq!(second_page.items[0].canister_id, b.max(a));
    }

    #[test]
    fn list_registered_canister_summaries_returns_empty_pages_past_the_end() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
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
        });
        assert_eq!(response.total, 1);
        assert!(response.items.is_empty());
    }

    #[test]
    fn list_recent_commitments_returns_qualifying_and_non_qualifying_commitments() {
        let qualifying = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let low_amount = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        let mut st = base_state();
        st.recent_commitments = Some(vec![
            RecentCommitment {
                canister_id: qualifying,
                tx_id: 11,
                timestamp_nanos: Some(11),
                amount_e8s: 20_000_000,
                counts_toward_faucet: true,
            },
        ]);
        st.recent_under_threshold_commitments = Some(vec![
            RecentCommitment {
                canister_id: low_amount,
                tx_id: 10,
                timestamp_nanos: Some(10),
                amount_e8s: 5_000_000,
                counts_toward_faucet: false,
            },
        ]);
        st.recent_invalid_commitments = Some(vec![InvalidCommitment {
            tx_id: 12,
            timestamp_nanos: Some(12),
            amount_e8s: 20_000_000,
            memo_text: crate::logic::INVALID_MEMO_PLACEHOLDER.to_string(),
        }]);
        state::set_state(st);

        let all = list_recent_commitments(ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        });
        assert_eq!(all.items.len(), 3);
        assert_eq!(all.items[0].tx_id, 12);
        assert_eq!(all.items[0].canister_id, None);
        assert_eq!(all.items[0].memo_text.as_deref(), Some(crate::logic::INVALID_MEMO_PLACEHOLDER));
        assert!(!all.items[0].counts_toward_faucet);
        assert_eq!(all.items[0].outcome_category, RecentCommitmentOutcomeCategory::InvalidTargetMemo);
        assert_eq!(all.items[1].tx_id, 11);
        assert_eq!(all.items[1].canister_id, Some(qualifying));
        assert!(all.items[1].counts_toward_faucet);
        assert_eq!(all.items[1].outcome_category, RecentCommitmentOutcomeCategory::QualifyingCommitment);
        assert_eq!(all.items[2].tx_id, 10);
        assert_eq!(all.items[2].canister_id, Some(low_amount));
        assert!(!all.items[2].counts_toward_faucet);
        assert_eq!(all.items[2].outcome_category, RecentCommitmentOutcomeCategory::UnderThresholdCommitment);

        let qualifying_only = list_recent_commitments(ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(true),
        });
        assert_eq!(qualifying_only.items.len(), 1);
        assert_eq!(qualifying_only.items[0].tx_id, 11);
        assert!(qualifying_only.items[0].counts_toward_faucet);
        assert_eq!(qualifying_only.items[0].outcome_category, RecentCommitmentOutcomeCategory::QualifyingCommitment);
    }

    #[test]
    fn derived_aggregates_fallback_from_histories() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(10),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(20),
                    amount_e8s: 50,
                    counts_toward_faucet: false,
                },
                CommitmentSample {
                    tx_id: 3,
                    timestamp_nanos: Some(30),
                    amount_e8s: 200,
                    counts_toward_faucet: true,
                },
            ],
        );
        initialize_derived_state_if_missing(&mut st);
        assert_eq!(st.qualifying_commitment_count, Some(2));
        assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 3);
    }


    #[test]
    fn normalize_runtime_state_moves_non_qualifying_commitments_out_of_registered_history() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.config.max_commitment_entries_per_canister = 1;
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 5_000_000,
                    counts_toward_faucet: false,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 50_000_000,
                    counts_toward_faucet: true,
                },
            ],
        );
        st.recent_commitments = Some(vec![RecentCommitment {
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

        assert_eq!(st.qualifying_commitment_count, Some(1));
        assert_eq!(st.commitment_history.get(&canister).map(|items| items.len()), Some(1));
        assert_eq!(st.commitment_history.get(&canister).unwrap()[0].tx_id, 2);
        assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(1));
        assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 2);
        assert_eq!(
            st.recent_under_threshold_commitments
                .as_ref()
                .map(|items| items.iter().map(|item| item.tx_id).collect::<Vec<_>>()),
            Some(vec![1]),
        );
        assert_eq!(st.per_canister_meta.get(&canister).and_then(|meta| meta.last_commitment_ts), Some(2));
        assert_eq!(count_registered_canisters(&st), 1);
    }

    #[test]
    fn normalize_runtime_state_prunes_memo_only_registration_when_history_is_non_qualifying() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
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
        assert!(!st.commitment_history.contains_key(&canister));
        assert!(!st.cycles_history.contains_key(&canister));
        assert!(!st.per_canister_meta.contains_key(&canister));
        assert_eq!(
            st.recent_under_threshold_commitments
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
                .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
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
        assert_eq!(st.commitment_history.len(), 2_101);
    }

    #[test]
    fn apply_upgrade_args_clamps_runtime_caps() {
        let mut st = base_state();
        apply_upgrade_args(
            &mut st,
            Some(UpgradeArgs {
                max_cycles_entries_per_canister: Some(u32::MAX),
                max_commitment_entries_per_canister: Some(u32::MAX),
                max_index_pages_per_tick: Some(u32::MAX),
                max_canisters_per_cycles_tick: Some(u32::MAX),
                ..UpgradeArgs::default()
            }),
        );

        assert_eq!(st.config.max_cycles_entries_per_canister, MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP);
        assert_eq!(
            st.config.max_commitment_entries_per_canister,
            MAX_COMMITMENT_ENTRIES_PER_CANISTER_HARD_CAP,
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
            .insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            (1..=150)
                .map(|tx_id| CommitmentSample {
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

        let commitments = get_commitment_history(GetCommitmentHistoryArgs {
            canister_id: canister,
            start_after_tx_id: None,
            limit: Some(5_000),
            descending: Some(false),
        });
        assert_eq!(commitments.items.len(), MAX_PUBLIC_QUERY_LIMIT as usize);
        assert_eq!(commitments.next_start_after_tx_id, Some(100));

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
            st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
            st.commitment_history.insert(
                canister,
                vec![CommitmentSample {
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
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![CommitmentSample {
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
    fn commitment_history_pagination_round_trips_without_skips_in_both_directions() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.distinct_canisters.insert(canister);
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample { tx_id: 10, timestamp_nanos: Some(10), amount_e8s: 1, counts_toward_faucet: true },
                CommitmentSample { tx_id: 20, timestamp_nanos: Some(20), amount_e8s: 1, counts_toward_faucet: true },
                CommitmentSample { tx_id: 30, timestamp_nanos: Some(30), amount_e8s: 1, counts_toward_faucet: true },
            ],
        );
        state::set_state(st);

        let first = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: None, limit: Some(2), descending: Some(false) });
        let second = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: first.next_start_after_tx_id, limit: Some(2), descending: Some(false) });
        let asc: Vec<_> = first.items.iter().chain(second.items.iter()).map(|item| item.tx_id).collect();
        assert_eq!(asc, vec![10, 20, 30]);

        let first_desc = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: None, limit: Some(2), descending: Some(true) });
        let second_desc = get_commitment_history(GetCommitmentHistoryArgs { canister_id: canister, start_after_tx_id: first_desc.next_start_after_tx_id, limit: Some(2), descending: Some(true) });
        let desc: Vec<_> = first_desc.items.iter().chain(second_desc.items.iter()).map(|item| item.tx_id).collect();
        assert_eq!(desc, vec![30, 20, 10]);
    }


    #[test]
    fn registered_canister_summaries_roll_up_qualifying_only() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = base_state();
        st.canister_sources.insert(canister, BTreeSet::from([CanisterSource::MemoCommitment]));
        st.commitment_history.insert(
            canister,
            vec![
                CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                },
                CommitmentSample {
                    tx_id: 2,
                    timestamp_nanos: Some(2_000_000_000),
                    amount_e8s: 50,
                    counts_toward_faucet: false,
                },
                CommitmentSample {
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
                last_commitment_ts: Some(3),
                last_cycles_probe_ts: Some(9),
                last_cycles_probe_result: None,
                ..Default::default()
            },
        );

        let summaries = registered_canister_summaries(&st);
        assert_eq!(summaries.len(), 1);
        let item = &summaries[0];
        assert_eq!(item.qualifying_commitment_count, 2);
        assert_eq!(item.total_qualifying_committed_e8s, 350);
        assert_eq!(item.last_commitment_ts, Some(3));
        assert_eq!(item.latest_cycles, Some(8));
    }
}
