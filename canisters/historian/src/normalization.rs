use super::*;
pub(crate) use candid::{CandidType, Deserialize, Nat, Principal};
pub(crate) use icrc_ledger_types::icrc1::account::Account;
pub(crate) use num_traits::ToPrimitive;
pub(crate) use serde::Serialize;
pub(crate) use std::cmp::Reverse;
pub(crate) use std::collections::{BTreeMap, BTreeSet};

#[allow(unused_imports)]
pub(crate) use crate::state::{
    CanisterMeta, CanisterTrackingReason, CommitmentIndexFault, CommitmentSample, Config,
    CyclesSample, IcpXdrRateSnapshot, InvalidCommitment, RecentCommitment, State,
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

pub(crate) fn assert_non_anonymous_principal(name: &str, principal: Principal) {
    assert!(
        principal != Principal::anonymous(),
        "{name} must not be the anonymous principal"
    );
}

pub(crate) fn assert_non_anonymous_account(name: &str, account: &Account) {
    assert_non_anonymous_principal(&format!("{name}.owner"), account.owner);
}

pub(crate) fn validate_config(cfg: &Config) {
    assert_non_anonymous_account("staking_account", &cfg.staking_account);
    assert_non_anonymous_account("output_source_account", &cfg.output_source_account);
    assert_non_anonymous_account("output_account", &cfg.output_account);
    assert_non_anonymous_account("rewards_account", &cfg.rewards_account);
    assert_non_anonymous_principal("ledger_canister_id", cfg.ledger_canister_id);
    assert_non_anonymous_principal("index_canister_id", cfg.index_canister_id);
    assert_non_anonymous_principal("sns_wasm_canister_id", cfg.sns_wasm_canister_id);
    assert_non_anonymous_principal("xrc_canister_id", cfg.xrc_canister_id);
    if let Some(cmc_canister_id) = cfg.cmc_canister_id {
        assert_non_anonymous_principal("cmc_canister_id", cmc_canister_id);
    }
    if let Some(faucet_canister_id) = cfg.faucet_canister_id {
        assert_non_anonymous_principal("faucet_canister_id", faucet_canister_id);
    }
    assert!(
        cfg.output_source_account != cfg.output_account,
        "output_source_account and output_account must be distinct"
    );
    assert!(
        cfg.output_source_account != cfg.rewards_account,
        "output_source_account and rewards_account must be distinct"
    );
    assert!(
        cfg.output_account != cfg.rewards_account,
        "output_account and rewards_account must be distinct"
    );
    assert!(
        cfg.scan_interval_seconds > 0,
        "scan_interval_seconds must be greater than 0"
    );
    assert!(
        cfg.cycles_interval_seconds > 0,
        "cycles_interval_seconds must be greater than 0"
    );
    assert!(
        cfg.min_tx_e8s >= MIN_MIN_TX_E8S,
        "min_tx_e8s must be at least {MIN_MIN_TX_E8S} e8s (0.1 ICP)"
    );
    if cfg.relay_factory_enabled {
        assert!(
            crate::approved_self_service_relay_wasm().is_some()
                && crate::approved_relay_raw_wasm_hash_hex().is_some()
                && crate::approved_relay_install_payload_hash_hex().is_some(),
            "relay_factory_enabled requires a nonempty approved relay wasm artifact and raw/install payload hashes"
        );
    }
}

pub(crate) fn commitment_sort_key(item: &RecentCommitment) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

pub(crate) fn invalid_commitment_sort_key(item: &InvalidCommitment) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

pub(crate) fn clamp_public_limit(limit: Option<u32>, default: u32) -> usize {
    limit.unwrap_or(default).clamp(1, MAX_PUBLIC_QUERY_LIMIT) as usize
}

pub(crate) fn clamp_cycles_entries_per_canister(value: u32) -> u32 {
    value.clamp(1, MAX_CYCLES_ENTRIES_PER_CANISTER_HARD_CAP)
}

pub(crate) fn clamp_commitment_entries_per_canister(value: u32) -> u32 {
    value.clamp(1, MAX_COMMITMENT_ENTRIES_PER_CANISTER_HARD_CAP)
}

pub(crate) fn clamp_index_pages_per_tick(value: u32) -> u32 {
    value.clamp(1, MAX_INDEX_PAGES_PER_TICK_HARD_CAP)
}

pub(crate) fn clamp_canisters_per_cycles_tick(value: u32) -> u32 {
    value.clamp(1, MAX_CANISTERS_PER_CYCLES_TICK_HARD_CAP)
}

pub(crate) fn format_module_hash_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

pub(crate) fn nat_to_u64(n: &Nat) -> Option<u64> {
    n.0.to_u64()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn allocated_heap_memory_bytes() -> u64 {
    (core::arch::wasm32::memory_size(0) as u64) * 65_536
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn allocated_heap_memory_bytes() -> u64 {
    0
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn allocated_stable_memory_bytes() -> u64 {
    ic_cdk::stable::stable_size().saturating_mul(65_536)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn allocated_stable_memory_bytes() -> u64 {
    0
}

pub(crate) fn normalize_recent_commitment_bucket(
    items: &mut Vec<RecentCommitment>,
    counts_toward_faucet: bool,
    max_entries: usize,
) {
    items.retain(|item| item.counts_toward_faucet == counts_toward_faucet);
    items.sort_by_key(|item| std::cmp::Reverse(commitment_sort_key(item)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(max_entries);
}

pub(crate) fn normalize_recent_invalid_commitments(items: &mut Vec<InvalidCommitment>) {
    items.sort_by_key(|item| std::cmp::Reverse(invalid_commitment_sort_key(item)));
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.tx_id));
    items.truncate(MAX_RECENT_INVALID_COMMITMENTS);
}

pub(crate) fn memo_source_is_registered(
    st: &State,
    canister_id: &Principal,
    sources: &BTreeSet<CanisterTrackingReason>,
) -> bool {
    sources.contains(&CanisterTrackingReason::MemoCommitment)
        && commitment_history_snapshot(st, *canister_id)
            .into_iter()
            .any(|item| item.counts_toward_faucet)
}

pub(crate) fn visible_tracking_reasons_for_canister(
    st: &State,
    canister_id: &Principal,
) -> Option<BTreeSet<CanisterTrackingReason>> {
    let mut sources = st.canister_tracking_reasons.get(canister_id)?.clone();
    if !memo_source_is_registered(st, canister_id, &sources) {
        sources.remove(&CanisterTrackingReason::MemoCommitment);
    }
    if sources.is_empty() {
        return None;
    }
    Some(sources)
}

pub(crate) fn clamp_config(st: &mut State) {
    st.config.max_cycles_entries_per_canister =
        clamp_cycles_entries_per_canister(st.config.max_cycles_entries_per_canister);
    st.config.max_commitment_entries_per_canister =
        clamp_commitment_entries_per_canister(st.config.max_commitment_entries_per_canister);
    st.config.max_index_pages_per_tick =
        clamp_index_pages_per_tick(st.config.max_index_pages_per_tick);
    st.config.max_canisters_per_cycles_tick =
        clamp_canisters_per_cycles_tick(st.config.max_canisters_per_cycles_tick);
}

pub(crate) fn normalize_runtime_state(st: &mut State) {
    clamp_config(st);

    let mut recent_commitments = st.recent_commitments.take().unwrap_or_default();
    recent_commitments.extend(fallback_recent_qualifying_commitments_state(st));
    let mut recent_under_threshold = st
        .recent_under_threshold_commitments
        .take()
        .unwrap_or_default();
    recent_under_threshold.extend(fallback_recent_under_threshold_commitments_state(st));

    for item in recent_commitments
        .iter()
        .filter(|item| !item.counts_toward_faucet)
        .cloned()
    {
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
                    raw_icp_memo_text: None,
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
    for history in st.raw_icp_commitment_history.values_mut() {
        history.retain(|item| item.counts_toward_faucet);
        if history.len() > st.config.max_commitment_entries_per_canister as usize {
            let excess = history.len() - st.config.max_commitment_entries_per_canister as usize;
            history.drain(0..excess);
        }
    }
    st.raw_icp_commitment_history
        .retain(|_, history| !history.is_empty());
    for history in st.neuron_commitment_history.values_mut() {
        history.retain(|item| item.counts_toward_faucet);
        if history.len() > st.config.max_commitment_entries_per_canister as usize {
            let excess = history.len() - st.config.max_commitment_entries_per_canister as usize;
            history.drain(0..excess);
        }
    }
    st.neuron_commitment_history
        .retain(|_, history| !history.is_empty());

    let stale_memo_only_canisters: Vec<_> = st
        .canister_tracking_reasons
        .iter()
        .filter_map(|(canister_id, sources)| {
            if sources.contains(&CanisterTrackingReason::MemoCommitment)
                && !memo_source_is_registered(st, canister_id, sources)
            {
                Some(*canister_id)
            } else {
                None
            }
        })
        .collect();
    for canister_id in stale_memo_only_canisters {
        let remove_entry = if let Some(sources) = st.canister_tracking_reasons.get_mut(&canister_id)
        {
            sources.remove(&CanisterTrackingReason::MemoCommitment);
            sources.is_empty()
        } else {
            false
        };
        if remove_entry {
            st.canister_tracking_reasons.remove(&canister_id);
            st.cycles_history.remove(&canister_id);
            st.per_canister_meta.remove(&canister_id);
        }
    }

    normalize_recent_commitment_bucket(
        &mut recent_commitments,
        true,
        MAX_RECENT_QUALIFYING_COMMITMENTS,
    );
    normalize_recent_commitment_bucket(
        &mut recent_under_threshold,
        false,
        MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS,
    );
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
        .canister_tracking_reasons
        .keys()
        .copied()
        .chain(commitment_history_canister_ids(st))
        .chain(cycles_history_canister_ids(st))
        .collect();
    st.distinct_canisters = distinct_canisters;
    rebuild_registered_canister_summaries_cache(st);
}
