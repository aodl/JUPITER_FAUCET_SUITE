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
        icp_xdr_rate: st.icp_xdr_rate.clone(),
        last_icp_xdr_rate_error: st.last_icp_xdr_rate_error.clone(),
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
                merged.extend(low_value.iter().cloned().map(|item| RecentCommitmentListItem {
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
                merged.extend(neurons.iter().cloned().map(|item| RecentCommitmentListItem {
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
                merged.extend(neurons.iter().cloned().map(|item| RecentCommitmentListItem {
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
