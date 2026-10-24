use super::*;
use jupiter_ic_clients::event_horizon::{
    EventHorizonPokeMatch, EventHorizonPokeNotification, EventHorizonPokeTarget,
};
use std::ops::Bound::{Excluded, Unbounded};

#[ic_cdk::query]
pub(super) fn list_canister_aliases(args: ListCanisterAliasesArgs) -> ListCanisterAliasesResponse {
    canister_aliases_page(jupiter_memo_policy::CANISTER_ALIASES, args)
}

fn canister_aliases_page(
    bindings: &[(&str, &str)],
    args: ListCanisterAliasesArgs,
) -> ListCanisterAliasesResponse {
    use sha2::{Digest, Sha256};
    let mut table: Vec<_> = bindings
        .iter()
        .map(|&(alias, principal)| CanisterAlias {
            alias: alias.to_string(),
            canister_id: Principal::from_text(principal)
                .expect("invalid reviewed identity principal"),
        })
        .collect();
    table.sort_by(|a, b| a.alias.cmp(&b.alias));

    // Protocol fingerprint of the complete sorted table, independent of paging.
    let mut hash = Sha256::new();
    hash.update(b"JUPITER_CANISTER_ALIASES_V1\0");
    hash.update((table.len() as u32).to_be_bytes());
    for item in &table {
        hash.update([item.alias.len() as u8]);
        hash.update(item.alias.as_bytes());
        hash.update([item.canister_id.as_slice().len() as u8]);
        hash.update(item.canister_id.as_slice());
    }
    let revision = hash.finalize().to_vec();
    let limit = args.limit.unwrap_or(128).clamp(1, 128) as usize;
    let mut remaining = table.into_iter().filter(|item| {
        args.start_after
            .as_ref()
            .is_none_or(|cursor| item.alias > *cursor)
    });
    let items: Vec<_> = remaining.by_ref().take(limit).collect();
    let next_start_after = remaining
        .next()
        .and_then(|_| items.last().map(|item| item.alias.clone()));
    ListCanisterAliasesResponse {
        revision,
        items,
        next_start_after,
    }
}

#[cfg(test)]
mod alias_tests {
    use super::*;

    #[test]
    fn alias_pages_are_sorted_exclusive_bounded_and_complete() {
        for count in [0, 1, 128, 129, 300] {
            let names: Vec<_> = (0..count).map(|n| format!("S{n:07}")).collect();
            let bindings: Vec<_> = names
                .iter()
                .rev()
                .map(|name| {
                    (
                        name.as_str(),
                        jupiter_memo_policy::EVENT_HORIZON_CANISTER_ID,
                    )
                })
                .collect();
            for limit in [None, Some(0), Some(1), Some(128), Some(129), Some(u32::MAX)] {
                for initial in [
                    None,
                    Some("A"),
                    Some("S0000127"),
                    Some("S0000127Z"),
                    Some("Z"),
                ] {
                    let mut cursor = initial.map(str::to_string);
                    let expected: Vec<_> = names
                        .iter()
                        .filter(|name| initial.is_none_or(|start| name.as_str() > start))
                        .cloned()
                        .collect();
                    let mut assembled = Vec::new();
                    let revision = canister_aliases_page(&bindings, Default::default()).revision;
                    loop {
                        let page = canister_aliases_page(
                            &bindings,
                            ListCanisterAliasesArgs {
                                start_after: cursor.clone(),
                                limit,
                            },
                        );
                        assert_eq!(page.revision, revision);
                        assert_eq!(page.revision.len(), 32);
                        let remaining = expected.len() - assembled.len();
                        assert_eq!(
                            page.items.len(),
                            remaining.min(limit.unwrap_or(128).clamp(1, 128) as usize)
                        );
                        let page_names: Vec<_> =
                            page.items.into_iter().map(|item| item.alias).collect();
                        assembled.extend(page_names.clone());
                        if assembled.len() < expected.len() {
                            assert_eq!(page.next_start_after.as_ref(), page_names.last());
                            assert_ne!(page.next_start_after, cursor);
                            cursor = page.next_start_after;
                        } else {
                            assert_eq!(page.next_start_after, None);
                            break;
                        }
                    }
                    assert_eq!(
                        assembled, expected,
                        "count={count}, limit={limit:?}, cursor={initial:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn alias_revision_has_the_independent_golden_digest_and_covers_the_whole_table() {
        let initial = list_canister_aliases(Default::default());
        assert_eq!(
            hex::encode(&initial.revision),
            "d216eb713b3082a92186c72ce6f7ba31cdcd50f1978cd6d92c4fcd92d4a9e4e3"
        );
        let destination = jupiter_memo_policy::EVENT_HORIZON_CANISTER_ID;
        let bindings = [
            ("F", destination),
            ("E", destination),
            ("D", jupiter_memo_policy::DISBURSER_CANISTER_ID),
        ];
        let added = canister_aliases_page(&bindings, Default::default());
        assert_ne!(added.revision, initial.revision);
        assert_eq!(
            added
                .items
                .iter()
                .map(|item| item.alias.as_str())
                .collect::<Vec<_>>(),
            ["D", "E", "F"]
        );
        let reversed = [
            ("D", jupiter_memo_policy::DISBURSER_CANISTER_ID),
            ("E", destination),
            ("F", destination),
        ];
        assert_eq!(canister_aliases_page(&reversed, Default::default()), added);
        for start_after in [None, Some("E".into()), Some("Z".into())] {
            let page = canister_aliases_page(
                &bindings,
                ListCanisterAliasesArgs {
                    start_after,
                    limit: Some(1),
                },
            );
            assert_eq!(page.revision, added.revision);
        }
    }
}

#[ic_cdk::query]
pub(super) fn list_canisters(args: ListCanistersArgs) -> ListCanistersResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 50);
        let mut items = Vec::new();
        let mut next = None;
        let iter: Box<dyn Iterator<Item = &Principal>> = match args.start_after {
            Some(start_after) => Box::new(
                st.distinct_canisters
                    .range((Excluded(start_after), Unbounded)),
            ),
            None => Box::new(st.distinct_canisters.range(..)),
        };
        for canister_id in iter.copied() {
            let Some(tracking_reasons) = visible_tracking_reasons_for_canister(st, &canister_id)
            else {
                continue;
            };
            if let Some(filter) = &args.tracking_reason_filter {
                if !tracking_reasons.contains(filter) {
                    continue;
                }
            }
            if items.len() >= limit {
                next = items.last().map(|item: &CanisterListItem| item.canister_id);
                break;
            }
            items.push(CanisterListItem {
                canister_id,
                tracking_reasons: tracking_reasons.into_iter().collect(),
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
                next = items
                    .last()
                    .map(|sample: &CyclesSample| sample.timestamp_nanos);
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

fn commitment_history_page_from_snapshot(
    history: Vec<CommitmentSample>,
    start_after_tx_id: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
) -> CommitmentHistoryPage {
    let descending = descending.unwrap_or(false);
    let limit = clamp_public_limit(limit, 100);
    let mut items = Vec::new();
    let mut next = None;
    let iter: Box<dyn Iterator<Item = &CommitmentSample>> = if descending {
        Box::new(history.iter().rev())
    } else {
        Box::new(history.iter())
    };
    for item in iter {
        let include = match start_after_tx_id {
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
}

pub(super) fn commitment_history_page(args: GetCommitmentHistoryArgs) -> CommitmentHistoryPage {
    state::with_state(|st| {
        commitment_history_page_from_snapshot(
            commitment_history_snapshot(st, args.canister_id),
            args.start_after_tx_id,
            args.limit,
            args.descending,
        )
    })
}

#[ic_cdk::query]
pub(super) fn get_commitment_history(args: GetCommitmentHistoryArgs) -> CommitmentHistoryPage {
    commitment_history_page(args)
}

#[ic_cdk::query]
pub(super) fn get_raw_icp_commitment_history(
    args: GetRawIcpCommitmentHistoryArgs,
) -> CommitmentHistoryPage {
    let Ok(destination_subaccount) = normalize_raw_destination(args.destination_subaccount) else {
        return CommitmentHistoryPage {
            items: Vec::new(),
            next_start_after_tx_id: None,
        };
    };
    state::with_state(|st| {
        let history = raw_icp_commitment_history_snapshot(st, args.canister_id)
            .into_iter()
            .filter(|sample| {
                normalize_raw_destination(sample.raw_icp_destination_subaccount.clone())
                    .is_ok_and(|sample_destination| sample_destination == destination_subaccount)
            })
            .collect();
        commitment_history_page_from_snapshot(
            history,
            args.start_after_tx_id,
            args.limit,
            args.descending,
        )
    })
}

fn normalize_raw_destination(
    destination_subaccount: Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>, ()> {
    let Some(destination_subaccount) = destination_subaccount else {
        return Ok(None);
    };
    if destination_subaccount.len() != 32 {
        return Err(());
    }
    Ok((destination_subaccount.iter().any(|byte| *byte != 0)).then_some(destination_subaccount))
}

#[ic_cdk::query]
pub(super) fn get_neuron_commitment_history(
    args: GetNeuronCommitmentHistoryArgs,
) -> CommitmentHistoryPage {
    state::with_state(|st| {
        commitment_history_page_from_snapshot(
            neuron_commitment_history_snapshot(st, args.neuron_id),
            args.start_after_tx_id,
            args.limit,
            args.descending,
        )
    })
}

#[ic_cdk::query]
pub(super) fn get_commitment_route_summaries(
    args: GetCommitmentRouteSummariesArgs,
) -> GetCommitmentRouteSummariesResponse {
    let truncated = args.routes.len() > MAX_PUBLIC_QUERY_LIMIT as usize;
    let items = args
        .routes
        .into_iter()
        .take(MAX_PUBLIC_QUERY_LIMIT as usize)
        .map(|route| {
            let rollup = state::CommitmentRouteKey::from_public(&route)
                .map(|key| state::get_commitment_route_rollup(&key))
                .unwrap_or_default();
            CommitmentRouteSummary {
                route,
                qualifying_commitment_count: rollup.qualifying_commitment_count,
                total_qualifying_committed_e8s: rollup.total_qualifying_committed_e8s,
            }
        })
        .collect();
    state::with_state(|st| GetCommitmentRouteSummariesResponse {
        items,
        truncated,
        complete_from_genesis: state::commitment_index_is_complete(st),
        indexed_through_staking_tx_id: st.last_indexed_staking_tx_id,
        oldest_indexed_staking_tx_id: st.oldest_indexed_staking_tx_id,
        last_index_run_ts: st.last_index_run_ts,
        commitment_index_fault: st.commitment_index_fault.clone(),
        revision: Some(st.commitment_index_revision),
        observed_head_staking_tx_id: st
            .active_staking_catch_up
            .as_ref()
            .map(|progress| progress.observed_head_tx_id)
            .or(st.last_indexed_staking_tx_id),
        next_staking_start_tx_id: st
            .active_staking_catch_up
            .as_ref()
            .and_then(|progress| progress.next_start_tx_id),
    })
}

#[ic_cdk::query]
pub(super) fn get_canister_overview(canister_id: Principal) -> Option<CanisterOverview> {
    state::with_state(|st| {
        let tracking_reasons = visible_tracking_reasons_for_canister(st, &canister_id)?
            .into_iter()
            .collect();
        let meta = st
            .per_canister_meta
            .get(&canister_id)
            .cloned()
            .unwrap_or_default();
        let cycles_points = cycles_history_snapshot(st, canister_id).len() as u32;
        let commitment_points = commitment_history_snapshot(st, canister_id).len() as u32;
        Some(CanisterOverview {
            canister_id,
            tracking_reasons,
            meta,
            cycles_points,
            commitment_points,
        })
    })
}

#[ic_cdk::query]
pub(super) fn get_public_counts() -> PublicCounts {
    state::with_state(|st| PublicCounts {
        tracked_canister_count: count_tracked_canisters(st),
        memo_registered_canister_count: count_registered_canisters(st),
        raw_icp_declared_canister_count: Some(count_raw_icp_declared_canisters(st)),
        declared_neuron_count: Some(count_declared_neurons(st)),
        qualifying_commitment_count: st
            .qualifying_commitment_count
            .expect("historian invariant: missing qualifying_commitment_count"),
        sns_discovered_canister_count: count_sns_discovered_canisters(st),
        relay_target_canister_count: count_relay_target_canisters(st),
        relay_instance_canister_count: count_relay_instance_canisters(st),
        total_output_e8s: st
            .total_output_e8s
            .expect("historian invariant: missing total_output_e8s"),
        total_rewards_e8s: st
            .total_rewards_e8s
            .expect("historian invariant: missing total_rewards_e8s"),
    })
}

pub(crate) fn route_index_fault(st: &state::State) -> Option<String> {
    let output_ascending = st.output_route_index_descending == Some(false);
    let rewards_ascending = st.rewards_route_index_descending == Some(false);
    match (output_ascending, rewards_ascending) {
        (true, true) => {
            Some("unsupported persisted ascending output and rewards pagination state".to_string())
        }
        (true, false) => {
            Some("unsupported persisted ascending output pagination state".to_string())
        }
        (false, true) => {
            Some("unsupported persisted ascending rewards pagination state".to_string())
        }
        (false, false) => {
            let output_incomplete = st.output_route_backfill_complete != Some(true);
            let rewards_incomplete = st.rewards_route_backfill_complete != Some(true);
            match (output_incomplete, rewards_incomplete) {
                (true, true) => {
                    Some("output and rewards historical route backfills are incomplete".to_string())
                }
                (true, false) => Some("output historical route backfill is incomplete".to_string()),
                (false, true) => {
                    Some("rewards historical route backfill is incomplete".to_string())
                }
                (false, false) => None,
            }
        }
    }
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
        last_index_run_ts: st.last_index_run_ts,
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
        route_index_fault: route_index_fault(st),
        icp_xdr_rate: st.icp_xdr_rate.clone(),
        last_icp_xdr_rate_error: st.last_icp_xdr_rate_error.clone(),
        relay_factory_enabled: Some(st.config.relay_factory_enabled),
        relay_setup_min_e8s: Some(st.config.relay_setup_min_e8s),
    })
}

// Payment-authorizing configuration and Active mappings require consensus.
// setup_view is a bounded synchronous read of local state, with no outcalls.
#[ic_cdk::update]
pub(super) fn get_relay_configuration_view(args: RelaySetupArgs) -> RelaySetupViewResult {
    crate::relay_setup::setup_view(args)
}

#[ic_cdk::update]
pub(super) async fn notify_relay_configuration(args: RelaySetupArgs) -> RelaySetupNotifyResult {
    crate::relay_setup::notify_relay_configuration(args).await
}

// Only the canonical Event Horizon ICP instance may accelerate staking reconciliation.
#[cfg(not(feature = "debug_api"))]
const HISTORIAN_POKE_CALLER_WHITELIST: &[&[u8]] = &[&[0, 0, 0, 0, 2, 48, 17, 226, 1, 1]];

// Debug builds use the fixed PocketIC proxy to simulate Event Horizon.
#[cfg(feature = "debug_api")]
const HISTORIAN_POKE_CALLER_WHITELIST: &[&[u8]] = &[&[0, 0, 0, 0, 2, 48, 15, 70, 1, 1]];

fn historian_poke_caller_is_whitelisted(caller: Principal) -> bool {
    HISTORIAN_POKE_CALLER_WHITELIST
        .iter()
        .any(|allowed| caller.as_slice() == *allowed)
}

fn guard_historian_poke_caller() -> Result<(), String> {
    let caller = ic_cdk::api::msg_caller();
    historian_poke_caller_is_whitelisted(caller)
        .then_some(())
        .ok_or_else(|| "caller is not an authorized Historian poke source".to_string())
}

pub(crate) fn staking_max_amount(matches: &[EventHorizonPokeMatch]) -> Option<Nat> {
    let disburser = Principal::from_text(jupiter_memo_policy::DISBURSER_CANISTER_ID).unwrap();
    matches
        .iter()
        .filter(|item| {
            item.subject == disburser
                && matches!(&item.target, EventHorizonPokeTarget::NeuronNonce(0))
        })
        .map(|item| item.max_amount.clone())
        .max()
}

pub(crate) fn staking_poke_is_relevant(max_amount: &Nat) -> bool {
    let min_tx_e8s = state::with_state(|st| st.config.min_tx_e8s);
    max_amount >= &Nat::from(min_tx_e8s)
}

#[ic_cdk::update(guard = "guard_historian_poke_caller")]
pub(super) async fn poke(notification: EventHorizonPokeNotification) {
    // Ingress is rejected; the replicated guard authorizes Event Horizon.
    let Some(max_amount) = staking_max_amount(&notification.matches) else {
        return;
    };
    if staking_poke_is_relevant(&max_amount) {
        crate::scheduler::handle_staking_poke().await;
    }
}

#[ic_cdk::query]
pub(super) fn get_endowment_transaction_status(
    transaction_id: u64,
) -> EndowmentTransactionStatusResponse {
    crate::scheduler::endowment_transaction_status(transaction_id)
}

fn principal_matches_compact_prefix(principal: Principal, prefix: &str) -> bool {
    let text = principal.to_text();
    let first_group = text
        .split_once('-')
        .map(|(head, _)| head)
        .unwrap_or(text.as_str());

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

async fn source_memory_status(
    target: Principal,
    controllers: &[Principal],
    direct: &dyn clients::BlackholeClient,
    compatibility: &[(Principal, &dyn clients::BlackholeClient)],
) -> Option<clients::blackhole::BlackholeCanisterStatus> {
    if let Ok(status) = direct.canister_status(target).await {
        return Some(status);
    }
    for (route, client) in compatibility {
        if controllers.contains(route)
            && jupiter_ic_clients::constants::is_production_blackhole_canister_id(*route)
        {
            if let Ok(status) = client.canister_status(target).await {
                return Some(status);
            }
        }
    }
    None
}

fn source_memory_bytes(
    status: Option<&clients::blackhole::BlackholeCanisterStatus>,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    let Some(status) = status else {
        return (None, None, None);
    };
    (
        status
            .memory_metrics
            .as_ref()
            .and_then(|m| nat_to_u64(&m.wasm_memory_size)),
        status
            .memory_metrics
            .as_ref()
            .and_then(|m| nat_to_u64(&m.stable_memory_size)),
        status.memory_size.as_ref().and_then(nat_to_u64),
    )
}

async fn load_canister_module_hashes() -> Vec<CanisterModuleHash> {
    let canister_ids = source_module_hash_canister_ids();
    let direct = clients::blackhole::BlackholeCanister::new(Principal::management_canister());
    let routes = jupiter_ic_clients::constants::ordered_production_blackhole_canister_ids();
    let compatibility = routes.map(clients::blackhole::BlackholeCanister::new);
    let mut hashes = Vec::with_capacity(canister_ids.len());
    for canister_id in canister_ids {
        let request = jupiter_ic_clients::management::CanisterInfoArgs {
            canister_id,
            num_requested_changes: Some(0),
        };
        let (module_hash_hex, controllers) =
            match jupiter_ic_clients::management::canister_info(&request).await {
                Ok(result) => (
                    result
                        .module_hash
                        .map(|module_hash| format_module_hash_hex(module_hash.as_ref())),
                    Some(result.controllers),
                ),
                Err(err) => {
                    ic_cdk::println!(
                        "get_canister_module_hashes failed for {}: {:?}",
                        canister_id,
                        err
                    );
                    (None, None)
                }
            };
        let status = source_memory_status(
            canister_id,
            controllers.as_deref().unwrap_or_default(),
            &direct,
            &[
                (routes[0], &compatibility[0]),
                (routes[1], &compatibility[1]),
            ],
        )
        .await;
        let (heap_memory_bytes, stable_memory_bytes, total_memory_bytes) =
            source_memory_bytes(status.as_ref());
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

pub(crate) fn finish_canister_module_hash_refresh(
    lock_ts: u64,
    completed_ts: u64,
    hashes: Vec<CanisterModuleHash>,
) {
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
pub(super) fn list_memo_registered_canister_summaries(
    args: ListMemoRegisteredCanisterSummariesArgs,
) -> ListMemoRegisteredCanisterSummariesResponse {
    state::with_state(|st| {
        let page = args.page.unwrap_or(0);
        let page_size = args.page_size.unwrap_or(25).clamp(1, 100);
        if let Some(response) =
            memo_registered_canister_summaries_total_desc_page(st, page, page_size)
        {
            return response;
        }
        ListMemoRegisteredCanisterSummariesResponse {
            items: Vec::new(),
            page,
            page_size,
            total: 0,
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
            return FindCanistersByMemoPrefixResponse {
                items: Vec::new(),
                truncated: false,
            };
        }
        let limit = clamp_public_limit(args.limit, 20).min(50);
        let mut items = Vec::new();
        let mut matched = 0usize;
        for canister_id in st.distinct_canisters.iter().copied() {
            if !principal_matches_compact_prefix(canister_id, &prefix) {
                continue;
            }
            let Some(tracking_reasons) = visible_tracking_reasons_for_canister(st, &canister_id)
            else {
                continue;
            };
            if !tracking_reasons.contains(&CanisterTrackingReason::MemoCommitment) {
                continue;
            }
            let Some(summary) = memo_registered_canister_summary_for(st, canister_id) else {
                continue;
            };
            matched += 1;
            if items.len() < limit {
                items.push(CanisterPrefixMatch {
                    canister_id,
                    tracking_reasons: summary.tracking_reasons,
                    matched_prefix: prefix.clone(),
                    qualifying_commitment_count: summary.qualifying_commitment_count,
                    total_qualifying_committed_e8s: summary.total_qualifying_committed_e8s,
                    last_commitment_ts: summary.last_commitment_ts,
                    latest_cycles: summary.latest_cycles,
                    last_cycles_probe_ts: summary.last_cycles_probe_ts,
                });
            }
        }
        items.sort_by_key(|item| {
            (
                std::cmp::Reverse(item.total_qualifying_committed_e8s),
                item.canister_id,
            )
        });
        FindCanistersByMemoPrefixResponse {
            items,
            truncated: matched > limit,
        }
    })
}

#[ic_cdk::query]
pub(super) fn list_recent_commitments(
    args: ListRecentCommitmentsArgs,
) -> ListRecentCommitmentsResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 20);
        let qualifying_only = args.qualifying_only.unwrap_or(false);
        let mut items: Vec<RecentCommitmentListItem> = {
            let recent = st
                .recent_commitments
                .as_ref()
                .expect("historian invariant: missing recent_commitments");
            let mut merged: Vec<RecentCommitmentListItem> = recent
                .iter()
                .map(|item| RecentCommitmentListItem {
                    canister_id: Some(item.canister_id),
                    neuron_id: None,
                    raw_icp_memo_text: item.raw_icp_memo_text.clone(),
                    raw_icp_destination_subaccount: item.raw_icp_destination_subaccount.clone(),
                    neuron_memo_text: None,
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                    outcome_category: RecentCommitmentOutcomeCategory::QualifyingCommitment,
                })
                .collect();
            {
                let low_value = st
                    .recent_under_threshold_commitments
                    .as_ref()
                    .expect("historian invariant: missing recent_under_threshold_commitments");
                merged.extend(low_value.iter().map(|item| RecentCommitmentListItem {
                    canister_id: Some(item.canister_id),
                    neuron_id: None,
                    raw_icp_memo_text: item.raw_icp_memo_text.clone(),
                    raw_icp_destination_subaccount: item.raw_icp_destination_subaccount.clone(),
                    neuron_memo_text: None,
                    memo_text: Some(item.canister_id.to_text()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentCommitmentOutcomeCategory::UnderThresholdCommitment,
                }));
            }
            {
                let neurons = st
                    .recent_neuron_commitments
                    .as_ref()
                    .expect("historian invariant: missing recent_neuron_commitments");
                merged.extend(neurons.iter().map(|item| RecentCommitmentListItem {
                    canister_id: None,
                    neuron_id: Some(item.neuron_id),
                    raw_icp_memo_text: None,
                    raw_icp_destination_subaccount: None,
                    neuron_memo_text: item.memo_text.clone(),
                    memo_text: Some(item.neuron_id.to_string()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: true,
                    outcome_category: RecentCommitmentOutcomeCategory::QualifyingCommitment,
                }));
            }
            {
                let neurons = st
                    .recent_under_threshold_neuron_commitments
                    .as_ref()
                    .expect(
                        "historian invariant: missing recent_under_threshold_neuron_commitments",
                    );
                merged.extend(neurons.iter().map(|item| RecentCommitmentListItem {
                    canister_id: None,
                    neuron_id: Some(item.neuron_id),
                    raw_icp_memo_text: None,
                    raw_icp_destination_subaccount: None,
                    neuron_memo_text: item.memo_text.clone(),
                    memo_text: Some(item.neuron_id.to_string()),
                    tx_id: item.tx_id,
                    timestamp_nanos: item.timestamp_nanos,
                    amount_e8s: item.amount_e8s,
                    counts_toward_faucet: false,
                    outcome_category: RecentCommitmentOutcomeCategory::UnderThresholdCommitment,
                }));
            }
            {
                let invalid = st
                    .recent_invalid_commitments
                    .as_ref()
                    .expect("historian invariant: missing recent_invalid_commitments");
                merged.extend(
                    invalid
                        .iter()
                        .cloned()
                        .map(|item| RecentCommitmentListItem {
                            canister_id: None,
                            neuron_id: None,
                            raw_icp_memo_text: None,
                            raw_icp_destination_subaccount: None,
                            neuron_memo_text: None,
                            memo_text: Some(item.memo_text),
                            tx_id: item.tx_id,
                            timestamp_nanos: item.timestamp_nanos,
                            amount_e8s: item.amount_e8s,
                            counts_toward_faucet: false,
                            outcome_category: RecentCommitmentOutcomeCategory::InvalidTargetMemo,
                        }),
                );
            }
            merged.sort_by(|a, b| {
                let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
                let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
                b_key.cmp(&a_key)
            });
            merged
        };
        if qualifying_only {
            items.retain(|item| item.counts_toward_faucet);
        }
        items.truncate(limit);
        ListRecentCommitmentsResponse { items }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "debug_api"))]
    #[test]
    fn production_historian_poke_whitelist_accepts_only_event_horizon() {
        assert!(historian_poke_caller_is_whitelisted(
            Principal::from_text("eo6ei-gaaaa-aaaar-qchra-cai").unwrap()
        ));
        assert!(!historian_poke_caller_is_whitelisted(
            Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai").unwrap()
        ));
        assert!(!historian_poke_caller_is_whitelisted(Principal::anonymous()));
        assert!(!historian_poke_caller_is_whitelisted(
            Principal::self_authenticating(b"authenticated ingress")
        ));
        assert!(!historian_poke_caller_is_whitelisted(
            Principal::from_slice(&[0, 0, 0, 0, 2, 48, 17, 226, 1, 2])
        ));
    }

    #[cfg(feature = "debug_api")]
    #[test]
    fn debug_historian_poke_whitelist_is_exact() {
        assert!(historian_poke_caller_is_whitelisted(Principal::from_slice(
            &[0, 0, 0, 0, 2, 48, 15, 70, 1, 1]
        )));
        assert!(!historian_poke_caller_is_whitelisted(
            Principal::from_slice(&[0, 0, 0, 0, 2, 48, 15, 70, 2, 1])
        ));
    }

    #[test]
    fn staking_target_classification_uses_only_neuron_nonce_zero_maximum() {
        let item = |target, amount: u64| EventHorizonPokeMatch {
            subject: Principal::from_text(jupiter_memo_policy::DISBURSER_CANISTER_ID).unwrap(),
            first_block_index: 17,
            target,
            max_amount: Nat::from(amount),
        };
        assert_eq!(staking_max_amount(&[]), None);
        let mut unrelated = item(EventHorizonPokeTarget::NeuronNonce(0), u64::MAX);
        unrelated.subject = Principal::anonymous();
        assert_eq!(staking_max_amount(&[unrelated.clone()]), None);
        assert_eq!(
            staking_max_amount(&[unrelated, item(EventHorizonPokeTarget::NeuronNonce(0), 1)]),
            Some(Nat::from(1_u64))
        );

        assert_eq!(
            staking_max_amount(&[
                item(EventHorizonPokeTarget::Subaccount(0), 900),
                item(EventHorizonPokeTarget::NeuronNonce(1), 800),
            ]),
            None
        );
        assert_eq!(
            staking_max_amount(&[
                item(EventHorizonPokeTarget::NeuronNonce(0), 100),
                item(EventHorizonPokeTarget::NeuronNonce(0), 250),
                item(EventHorizonPokeTarget::NeuronNonce(1), 900),
            ]),
            Some(Nat::from(250u64))
        );
    }
}

#[cfg(test)]
mod source_memory_tests {
    use super::*;
    use crate::clients::blackhole::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct StatusClient {
        allowed: bool,
        calls: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl clients::BlackholeClient for StatusClient {
        async fn canister_status(
            &self,
            _: Principal,
        ) -> Result<BlackholeCanisterStatus, clients::ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.allowed {
                return Err(clients::ClientError::Call("denied".into()));
            }
            Ok(BlackholeCanisterStatus {
                status: BlackholeCanisterStatusKind::Running,
                module_hash: Some(vec![7; 32]),
                cycles: candid::Nat::from(10_u64),
                settings: BlackholeSettings {
                    controllers: vec![],
                },
                memory_size: Some(candid::Nat::from(123_u64)),
                memory_metrics: Some(BlackholeMemoryMetrics {
                    wasm_memory_size: candid::Nat::from(40_u64),
                    stable_memory_size: candid::Nat::from(60_u64),
                }),
            })
        }
    }
    #[test]
    fn public_native_status_without_blackhole_and_authenticated_denial_fallback() {
        futures::executor::block_on(async {
            let direct = StatusClient {
                allowed: true,
                calls: AtomicUsize::new(0),
            };
            let fallback = StatusClient {
                allowed: true,
                calls: AtomicUsize::new(0),
            };
            let blackhole = jupiter_ic_clients::constants::blackhole_canister_id();
            let target = Principal::from_slice(&[1]);
            let mut status = source_memory_status(target, &[], &direct, &[(blackhole, &fallback)])
                .await
                .unwrap();
            assert_eq!(
                source_memory_bytes(Some(&status)),
                (Some(40), Some(60), Some(123))
            );
            status.memory_size = None;
            assert_eq!(
                source_memory_bytes(Some(&status)),
                (Some(40), Some(60), None),
                "component sizes cannot invent native total size"
            );
            assert_eq!(source_memory_bytes(None), (None, None, None));
            assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
            let denied = StatusClient {
                allowed: false,
                calls: AtomicUsize::new(0),
            };
            assert!(
                source_memory_status(target, &[], &denied, &[(blackhole, &fallback)])
                    .await
                    .is_none()
            );
            assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
            assert!(
                source_memory_status(target, &[blackhole], &denied, &[(blackhole, &fallback)])
                    .await
                    .is_some()
            );
            assert_eq!(fallback.calls.load(Ordering::SeqCst), 1);
            assert!(
                source_memory_status(target, &[blackhole], &denied, &[(blackhole, &denied)])
                    .await
                    .is_none()
            );
            assert!(
                source_memory_status(target, &[target], &denied, &[(target, &fallback)])
                    .await
                    .is_none()
            );
        });
    }
}
