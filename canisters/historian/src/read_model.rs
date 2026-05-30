use super::*;
use std::ops::Bound::{Excluded, Unbounded};

#[ic_cdk::query]
pub(super) fn list_canisters(args: ListCanistersArgs) -> ListCanistersResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 50);
        let mut items = Vec::new();
        let mut next = None;
        let iter: Box<dyn Iterator<Item = &Principal>> = match args.start_after {
            Some(start_after) => {
                Box::new(st.distinct_canisters.range((Excluded(start_after), Unbounded)))
            }
            None => Box::new(st.distinct_canisters.range(..)),
        };
        for canister_id in iter.copied() {
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
pub(super) fn get_cycles_history(args: GetCyclesHistoryArgs) -> CyclesHistoryPage {
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

pub(super) fn commitment_history_page(args: GetCommitmentHistoryArgs) -> CommitmentHistoryPage {
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
pub(super) fn get_commitment_history(args: GetCommitmentHistoryArgs) -> CommitmentHistoryPage {
    commitment_history_page(args)
}


#[ic_cdk::query]
pub(super) fn get_canister_overview(canister_id: Principal) -> Option<CanisterOverview> {
    state::with_state(|st| {
        let sources = visible_sources_for_canister(st, &canister_id)?
            .into_iter()
            .collect();
        let meta = st.per_canister_meta.get(&canister_id).cloned().unwrap_or_default();
        let cycles_points = cycles_history_snapshot(st, canister_id).len() as u32;
        let commitment_points = commitment_history_snapshot(st, canister_id).len() as u32;
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
pub(super) fn get_public_counts() -> PublicCounts {
    state::with_state(|st| PublicCounts {
        registered_canister_count: count_registered_canisters(st),
        raw_icp_declared_canister_count: Some(count_raw_icp_declared_canisters(st)),
        declared_neuron_count: Some(count_declared_neurons(st)),
        qualifying_commitment_count: st
            .qualifying_commitment_count
            .unwrap_or_else(|| fallback_qualifying_commitment_count(st)),
        sns_discovered_canister_count: count_sns_discovered_canisters(st),
        total_output_e8s: st.total_output_e8s.unwrap_or(0),
        total_rewards_e8s: st.total_rewards_e8s.unwrap_or(0),
    })
}

#[ic_cdk::query]
pub(super) fn get_public_status() -> PublicStatus {
    let heap_memory_bytes = allocated_heap_memory_bytes();
    let stable_memory_bytes = allocated_stable_memory_bytes();
    state::with_state(|st| PublicStatus {
        staking_account: st.config.staking_account,
        ledger_canister_id: st.config.ledger_canister_id,
        faucet_canister_id: effective_faucet_canister_id(st),
        cmc_canister_id: st.config.cmc_canister_id,
        output_source_account: Some(st.config.output_source_account),
        output_account: Some(st.config.output_account),
        rewards_account: Some(st.config.rewards_account),
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
        icp_xdr_rate: st.icp_xdr_rate.clone(),
        last_icp_xdr_rate_error: st.last_icp_xdr_rate_error.clone(),
    })
}

fn principal_matches_compact_prefix(principal: Principal, prefix: &str) -> bool {
    let text = principal.to_text();
    let first_group = text.split_once('-').map(|(head, _)| head).unwrap_or(text.as_str());

    if prefix.len() <= first_group.len() {
        return first_group.starts_with(prefix);
    }

    if !prefix.starts_with(first_group) {
        return false;
    }

    let mut expected_chars = prefix.chars();
    for ch in text.chars().filter(|ch| *ch != '-') {
        let Some(expected) = expected_chars.next() else {
            return true;
        };
        if ch.to_ascii_lowercase() != expected {
            return false;
        }
    }

    expected_chars.next().is_none()
}

pub(crate) const MODULE_HASH_CACHE_TTL_SECONDS: u64 = 60 * 60;
pub(crate) const MODULE_HASH_REFRESH_INTERVAL_SECONDS: u64 = 10 * 60;
pub(crate) const MODULE_HASH_REFRESH_LEASE_SECONDS: u64 = 5 * 60;

pub(crate) fn source_module_hash_canister_ids() -> Vec<Principal> {
    [
        "uccpi-cqaaa-aaaar-qby3q-cai",
        "acjuz-liaaa-aaaar-qb4qq-cai",
        "j5gs6-uiaaa-aaaar-qb5cq-cai",
        "afisn-gqaaa-aaaar-qb4qa-cai",
        "alk7f-5aaaa-aaaar-qb4ra-cai",
        "u2qkp-aqaaa-aaaar-qb7ea-cai",
        "jufzc-caaaa-aaaar-qb5da-cai",
    ]
    .into_iter()
    .map(|canister_id| Principal::from_text(canister_id).expect("invalid hardcoded canister id"))
    .collect()
}

async fn load_canister_module_hashes() -> Vec<CanisterModuleHash> {
    let canister_ids = source_module_hash_canister_ids();
    let blackhole = clients::blackhole::BlackholeCanister::new(mainnet_blackhole_id());
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
        let (heap_memory_bytes, stable_memory_bytes, total_memory_bytes) =
            match clients::BlackholeClient::canister_status(&blackhole, canister_id).await {
                Ok(status) => {
                    let heap = status.memory_metrics.as_ref().and_then(|metrics| nat_to_u64(&metrics.wasm_memory_size));
                    let stable = status.memory_metrics.as_ref().and_then(|metrics| nat_to_u64(&metrics.stable_memory_size));
                    let total = status
                        .memory_size
                        .as_ref()
                        .and_then(nat_to_u64)
                        .or_else(|| heap.zip(stable).map(|(heap, stable)| heap.saturating_add(stable)));
                    (heap, stable, total)
                }
                Err(err) => {
                    ic_cdk::println!("get_canister_module_hashes status failed for {}: {}", canister_id, err);
                    (None, None, None)
                }
            };
        hashes.push(CanisterModuleHash {
            canister_id,
            module_hash_hex,
            controllers,
            heap_memory_bytes,
            stable_memory_bytes,
            total_memory_bytes,
        });
    }
    hashes
}

pub(crate) fn canister_module_hash_has_any_success(hash: &CanisterModuleHash) -> bool {
    hash.module_hash_hex.is_some()
        || hash.controllers.is_some()
        || hash.heap_memory_bytes.is_some()
        || hash.stable_memory_bytes.is_some()
        || hash.total_memory_bytes.is_some()
}

fn acquire_canister_module_hash_refresh(now_secs: u64) -> bool {
    state::with_root_state_mut(|st| {
        let stale = st
            .canister_module_hash_cache_updated_ts
            .map(|updated_ts| now_secs.saturating_sub(updated_ts) >= MODULE_HASH_CACHE_TTL_SECONDS)
            .unwrap_or(true);
        if !stale {
            return false;
        }
        if st
            .canister_module_hash_refresh_lock_ts
            .map(|lock_ts| now_secs.saturating_sub(lock_ts) < MODULE_HASH_REFRESH_LEASE_SECONDS)
            .unwrap_or(false)
        {
            return false;
        }
        st.canister_module_hash_refresh_lock_ts = Some(now_secs);
        true
    })
}

pub(crate) fn finish_canister_module_hash_refresh(lock_ts: u64, completed_ts: u64, hashes: Vec<CanisterModuleHash>) {
    state::with_root_state_mut(|st| {
        if st.canister_module_hash_refresh_lock_ts != Some(lock_ts) {
            return;
        }
        st.canister_module_hash_cache = hashes;
        st.canister_module_hash_cache_updated_ts = Some(completed_ts);
        st.canister_module_hash_refresh_lock_ts = None;
    });
}

fn release_canister_module_hash_refresh(now_secs: u64) {
    state::with_root_state_mut(|st| {
        if st.canister_module_hash_refresh_lock_ts == Some(now_secs) {
            st.canister_module_hash_refresh_lock_ts = None;
        }
    });
}

pub(crate) async fn refresh_canister_module_hash_cache_if_due(now_secs: u64) {
    if !acquire_canister_module_hash_refresh(now_secs) {
        return;
    }
    let hashes = load_canister_module_hashes().await;
    if !hashes.iter().any(canister_module_hash_has_any_success) {
        release_canister_module_hash_refresh(now_secs);
        return;
    }
    let completed_ts = ic_cdk::api::time() / 1_000_000_000;
    finish_canister_module_hash_refresh(now_secs, completed_ts, hashes);
}

#[ic_cdk::query]
pub(super) fn get_canister_module_hashes() -> Vec<CanisterModuleHash> {
    state::with_state(|st| st.canister_module_hash_cache.clone())
}

#[ic_cdk::query]
pub(super) fn list_registered_canister_summaries(
    args: ListRegisteredCanisterSummariesArgs,
) -> ListRegisteredCanisterSummariesResponse {
    state::with_state(|st| {
        let page = args.page.unwrap_or(0);
        let page_size = args.page_size.unwrap_or(25).clamp(1, 100);
        if let Some(response) = registered_canister_summaries_total_desc_page(st, page, page_size) {
            return response;
        }
        // Slow compatibility fallback for states whose derived ranking cache is
        // missing or drifted. Normal init/upgrade/timer paths rebuild and refresh
        // this cache; a future maintenance pass should repair drift outside public
        // queries before removing this full sort.
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
pub(super) fn find_canisters_by_memo_prefix(
    args: FindCanistersByMemoPrefixArgs,
) -> FindCanistersByMemoPrefixResponse {
    state::with_state(|st| {
        let prefix = args.prefix.replace('-', "").to_ascii_lowercase();
        if prefix.len() < 4 || !prefix.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return FindCanistersByMemoPrefixResponse { items: Vec::new(), truncated: false };
        }
        let limit = clamp_public_limit(args.limit, 20).min(50);
        let source_filter = args.source_filter.unwrap_or(CanisterSource::MemoCommitment);
        let mut items = Vec::new();
        let mut matched = 0usize;
        for canister_id in st.distinct_canisters.iter().copied() {
            if !principal_matches_compact_prefix(canister_id, &prefix) {
                continue;
            }
            let Some(sources) = visible_sources_for_canister(st, &canister_id) else {
                continue;
            };
            if !sources.contains(&source_filter) {
                continue;
            }
            let Some(summary) = registered_canister_summary_for(st, canister_id) else {
                continue;
            };
            matched += 1;
            if items.len() < limit {
                items.push(CanisterPrefixMatch {
                    canister_id,
                    sources: summary.sources,
                    matched_prefix: prefix.clone(),
                    qualifying_commitment_count: summary.qualifying_commitment_count,
                    total_qualifying_committed_e8s: summary.total_qualifying_committed_e8s,
                    last_commitment_ts: summary.last_commitment_ts,
                    latest_cycles: summary.latest_cycles,
                    last_cycles_probe_ts: summary.last_cycles_probe_ts,
                });
            }
        }
        items.sort_by_key(|item| (std::cmp::Reverse(item.total_qualifying_committed_e8s), item.canister_id));
        FindCanistersByMemoPrefixResponse { items, truncated: matched > limit }
    })
}

#[ic_cdk::query]
pub(super) fn list_recent_commitments(args: ListRecentCommitmentsArgs) -> ListRecentCommitmentsResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 20);
        let qualifying_only = args.qualifying_only.unwrap_or(false);
        let mut items: Vec<RecentCommitmentListItem> = if let Some(recent) = &st.recent_commitments {
            let mut merged: Vec<RecentCommitmentListItem> = recent
                .iter()
                .map(|item| RecentCommitmentListItem {
                    canister_id: Some(item.canister_id),
                    neuron_id: None,
                    raw_icp_memo_text: item.raw_icp_memo_text.clone(),
                    neuron_memo_text: None,
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                    outcome_category: RecentCommitmentOutcomeCategory::QualifyingCommitment,
                })
                .collect();
            if let Some(low_value) = &st.recent_under_threshold_commitments {
                merged.extend(low_value.iter().map(|item| RecentCommitmentListItem {
                    canister_id: Some(item.canister_id),
                    neuron_id: None,
                    raw_icp_memo_text: item.raw_icp_memo_text.clone(),
                    neuron_memo_text: None,
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentCommitmentOutcomeCategory::UnderThresholdCommitment,
                }));
            }
            if let Some(neurons) = &st.recent_neuron_commitments {
                merged.extend(neurons.iter().map(|item| RecentCommitmentListItem {
                    canister_id: None,
                    neuron_id: Some(item.neuron_id),
                    raw_icp_memo_text: None,
                    neuron_memo_text: item.memo_text.clone(),
                    memo_text: Some(item.neuron_id.to_string()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                    outcome_category: RecentCommitmentOutcomeCategory::QualifyingCommitment,
                }));
            }
            if let Some(neurons) = &st.recent_under_threshold_neuron_commitments {
                merged.extend(neurons.iter().map(|item| RecentCommitmentListItem {
                    canister_id: None,
                    neuron_id: Some(item.neuron_id),
                    raw_icp_memo_text: None,
                    neuron_memo_text: item.memo_text.clone(),
                    memo_text: Some(item.neuron_id.to_string()),
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
                    neuron_id: None,
                    raw_icp_memo_text: None,
                    neuron_memo_text: None,
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
