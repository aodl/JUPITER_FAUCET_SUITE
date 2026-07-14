use super::*;
use jupiter_canister_logging::{
    format_event_line, FIELD_EVENT, FIELD_MAIN_INTERVAL_SECONDS, FIELD_TIMERS_INSTALLED,
};
pub(super) fn mainnet_ledger_id() -> Principal {
    jupiter_ic_clients::constants::icp_ledger_id()
}

pub(super) fn mainnet_index_id() -> Principal {
    jupiter_ic_clients::constants::icp_index_id()
}

pub(super) fn mainnet_blackhole_id() -> Principal {
    jupiter_ic_clients::constants::blackhole_canister_id()
}

pub(crate) fn mainnet_disburser_id() -> Principal {
    Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai")
        .expect("invalid hardcoded disburser principal")
}

pub(crate) fn mainnet_rewards_id() -> Principal {
    Principal::from_text("alk7f-5aaaa-aaaar-qb4ra-cai")
        .expect("invalid hardcoded rewards principal")
}

pub(crate) fn mainnet_disburser_staging_account() -> Account {
    Account {
        owner: mainnet_disburser_id(),
        subaccount: None,
    }
}

pub(crate) fn mainnet_output_account() -> Account {
    Account {
        owner: mainnet_faucet_id(),
        subaccount: None,
    }
}

pub(crate) fn mainnet_rewards_account() -> Account {
    Account {
        owner: mainnet_rewards_id(),
        subaccount: None,
    }
}

pub(crate) fn mainnet_cmc_id() -> Principal {
    jupiter_ic_clients::constants::cycles_minting_canister_id()
}

pub(crate) fn mainnet_faucet_id() -> Principal {
    Principal::from_text("acjuz-liaaa-aaaar-qb4qq-cai").expect("invalid hardcoded faucet principal")
}

pub(crate) const DEFAULT_IO_SURPLUS_NEURON_ID: u64 = 10_292_412_127_977_304_661;

pub(crate) fn mainnet_relay_id() -> Principal {
    Principal::from_text("u2qkp-aqaaa-aaaar-qb7ea-cai").expect("invalid hardcoded relay principal")
}

pub(crate) fn mainnet_canonical_relay_targets() -> Vec<Principal> {
    [
        "uccpi-cqaaa-aaaar-qby3q-cai",
        "afisn-gqaaa-aaaar-qb4qa-cai",
        "acjuz-liaaa-aaaar-qb4qq-cai",
        "alk7f-5aaaa-aaaar-qb4ra-cai",
        "jufzc-caaaa-aaaar-qb5da-cai",
        "j5gs6-uiaaa-aaaar-qb5cq-cai",
        "77deu-baaaa-aaaar-qb6za-cai",
        "e3mmv-5qaaa-aaaah-aadma-cai",
    ]
    .into_iter()
    .map(|id| Principal::from_text(id).expect("invalid hardcoded canonical relay target"))
    .collect()
}

pub(super) fn mainnet_sns_wasm_id() -> Principal {
    jupiter_ic_clients::constants::sns_wasm_id()
}

pub(crate) fn mainnet_xrc_id() -> Principal {
    jupiter_ic_clients::xrc::mainnet_xrc_canister_id()
}

#[cfg(any(test, feature = "debug_api"))]
pub(super) fn production_canister_id() -> Principal {
    Principal::from_text(env!("JUPITER_HISTORIAN_PROD_CANISTER_ID"))
        .expect("invalid embedded production canister principal")
}

#[cfg(any(test, feature = "debug_api"))]
pub(super) fn is_production_canister(principal: Principal) -> bool {
    principal == production_canister_id()
}

#[cfg(feature = "debug_api")]
pub(super) fn guard_debug_api_not_production() {
    if is_production_canister(ic_cdk::api::canister_self()) {
        ic_cdk::trap("debug_api is disabled for the production canister");
    }
}

pub(super) fn config_from_init_args(args: InitArgs) -> Config {
    let init_blackhole_canister_id = args.blackhole_canister_id;
    let blackhole_canister_id = init_blackhole_canister_id.unwrap_or_else(mainnet_blackhole_id);
    let cycles_probe_policy = Some(match init_blackhole_canister_id {
        Some(canister_id) => CyclesProbePolicy::FixedBlackhole { canister_id },
        None => CyclesProbePolicy::Auto,
    });
    let cfg = Config {
        staking_account: args.staking_account,
        output_source_account: args
            .output_source_account
            .unwrap_or_else(mainnet_disburser_staging_account),
        output_account: args.output_account.unwrap_or_else(mainnet_output_account),
        rewards_account: args.rewards_account.unwrap_or_else(mainnet_rewards_account),
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        index_canister_id: args.index_canister_id.unwrap_or_else(mainnet_index_id),
        cmc_canister_id: Some(args.cmc_canister_id.unwrap_or_else(mainnet_cmc_id)),
        faucet_canister_id: Some(args.faucet_canister_id.unwrap_or_else(mainnet_faucet_id)),
        blackhole_canister_id,
        cycles_probe_policy,
        sns_wasm_canister_id: args
            .sns_wasm_canister_id
            .unwrap_or_else(mainnet_sns_wasm_id),
        xrc_canister_id: args.xrc_canister_id.unwrap_or_else(mainnet_xrc_id),
        enable_sns_tracking: args.enable_sns_tracking.unwrap_or(false),
        scan_interval_seconds: args.scan_interval_seconds.unwrap_or(10 * 60),
        cycles_interval_seconds: args.cycles_interval_seconds.unwrap_or(7 * 24 * 60 * 60),
        min_tx_e8s: args.min_tx_e8s.unwrap_or(100_000_000),
        max_cycles_entries_per_canister: clamp_cycles_entries_per_canister(
            args.max_cycles_entries_per_canister.unwrap_or(100),
        ),
        max_commitment_entries_per_canister: clamp_commitment_entries_per_canister(
            args.max_commitment_entries_per_canister.unwrap_or(100),
        ),
        max_index_pages_per_tick: clamp_index_pages_per_tick(
            args.max_index_pages_per_tick.unwrap_or(10),
        ),
        max_canisters_per_cycles_tick: clamp_canisters_per_cycles_tick(
            args.max_canisters_per_cycles_tick.unwrap_or(25),
        ),
        relay_factory_enabled: args
            .relay_factory_enabled
            .unwrap_or_else(|| crate::approved_self_service_relay_wasm().is_some()),
        relay_setup_min_e8s: args.relay_setup_min_e8s.unwrap_or(300_000_000),
        relay_setup_dust_e8s: args.relay_setup_dust_e8s.unwrap_or(10_000),
        relay_setup_refund_cooldown_seconds: args
            .relay_setup_refund_cooldown_seconds
            .unwrap_or(300),
        relay_initial_cycles: args.relay_initial_cycles.unwrap_or(2_000_000_000_000),
        relay_cycle_safety_margin_e8s: args.relay_cycle_safety_margin_e8s.unwrap_or(5_000_000),
        relay_min_subaccount_one_seed_e8s: args
            .relay_min_subaccount_one_seed_e8s
            .unwrap_or(100_020_000),
        self_service_relay_interval_seconds: args
            .self_service_relay_interval_seconds
            .unwrap_or(86400)
            .max(60),
        self_service_relay_max_transfers_per_tick: Some(
            args.self_service_relay_max_transfers_per_tick.unwrap_or(10),
        ),
        io_surplus_neuron_id: args
            .io_surplus_neuron_id
            .unwrap_or(DEFAULT_IO_SURPLUS_NEURON_ID),
        canonical_relay_canister_id: Some(
            args.canonical_relay_canister_id
                .unwrap_or_else(mainnet_relay_id),
        ),
        canonical_relay_targets: args
            .canonical_relay_targets
            .unwrap_or_else(mainnet_canonical_relay_targets),
    };
    validate_config(&cfg);
    cfg
}

pub(super) fn count_registered_canisters(st: &State) -> u64 {
    st.canister_sources
        .iter()
        .filter(|(canister_id, sources)| memo_source_is_registered(st, canister_id, sources))
        .count() as u64
}

pub(super) fn count_raw_icp_declared_canisters(st: &State) -> u64 {
    st.raw_icp_commitment_history
        .keys()
        .copied()
        .chain(state::stable_raw_icp_commitment_history_keys())
        .collect::<BTreeSet<_>>()
        .len() as u64
}

pub(super) fn count_declared_neurons(st: &State) -> u64 {
    st.neuron_commitment_history
        .keys()
        .copied()
        .chain(state::stable_neuron_commitment_history_keys())
        .collect::<BTreeSet<_>>()
        .len() as u64
}

pub(super) fn count_sns_discovered_canisters(st: &State) -> u64 {
    st.canister_sources
        .values()
        .filter(|sources| sources.contains(&CanisterSource::SnsDiscovery))
        .count() as u64
}

pub(super) fn effective_faucet_canister_id(st: &State) -> Principal {
    st.config
        .faucet_canister_id
        .unwrap_or_else(mainnet_faucet_id)
}

pub(super) fn qualifying_rollup(history: &[CommitmentSample]) -> (u64, u64, Option<u64>) {
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

pub(super) fn latest_cycles(history: &[CyclesSample]) -> Option<u128> {
    history
        .iter()
        .max_by_key(|item| item.timestamp_nanos)
        .map(|item| item.cycles)
}

pub(super) fn commitment_history_canister_ids(st: &State) -> BTreeSet<Principal> {
    st.commitment_history
        .keys()
        .copied()
        .chain(state::stable_commitment_history_keys())
        .collect()
}

pub(super) fn cycles_history_canister_ids(st: &State) -> BTreeSet<Principal> {
    st.cycles_history
        .keys()
        .copied()
        .chain(state::stable_cycles_history_keys())
        .collect()
}

pub(super) fn commitment_history_snapshot(
    st: &State,
    canister_id: Principal,
) -> Vec<CommitmentSample> {
    st.commitment_history
        .get(&canister_id)
        .cloned()
        .unwrap_or_else(|| state::stable_commitment_history_for(canister_id))
}

pub(super) fn cycles_history_snapshot(st: &State, canister_id: Principal) -> Vec<CyclesSample> {
    st.cycles_history
        .get(&canister_id)
        .cloned()
        .unwrap_or_else(|| state::stable_cycles_history_for(canister_id))
}

pub(super) fn raw_icp_commitment_history_snapshot(
    st: &State,
    canister_id: Principal,
) -> Vec<CommitmentSample> {
    st.raw_icp_commitment_history
        .get(&canister_id)
        .cloned()
        .unwrap_or_else(|| state::stable_raw_icp_commitment_history_for(canister_id))
}

pub(super) fn neuron_commitment_history_snapshot(
    st: &State,
    neuron_id: u64,
) -> Vec<CommitmentSample> {
    st.neuron_commitment_history
        .get(&neuron_id)
        .cloned()
        .unwrap_or_else(|| state::stable_neuron_commitment_history_for(neuron_id))
}

pub(super) fn fallback_qualifying_commitment_count(st: &State) -> u64 {
    let cycles_top_up_count = commitment_history_canister_ids(st)
        .into_iter()
        .flat_map(|canister_id| commitment_history_snapshot(st, canister_id).into_iter())
        .filter(|item| item.counts_toward_faucet)
        .count() as u64;
    let raw_icp_count = st
        .raw_icp_commitment_history
        .keys()
        .copied()
        .chain(state::stable_raw_icp_commitment_history_keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|canister_id| raw_icp_commitment_history_snapshot(st, canister_id).into_iter())
        .filter(|item| item.counts_toward_faucet)
        .count() as u64;
    let neuron_count = st
        .neuron_commitment_history
        .keys()
        .copied()
        .chain(state::stable_neuron_commitment_history_keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|neuron_id| neuron_commitment_history_snapshot(st, neuron_id).into_iter())
        .filter(|item| item.counts_toward_faucet)
        .count() as u64;
    cycles_top_up_count
        .saturating_add(raw_icp_count)
        .saturating_add(neuron_count)
}

pub(super) fn fallback_recent_qualifying_commitments_state(st: &State) -> Vec<RecentCommitment> {
    let mut items: Vec<_> = commitment_history_canister_ids(st)
        .into_iter()
        .flat_map(|canister_id| {
            commitment_history_snapshot(st, canister_id)
                .into_iter()
                .filter(|item| item.counts_toward_faucet)
                .map(move |item| RecentCommitment {
                    canister_id,
                    raw_icp_memo_text: None,
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

pub(super) fn fallback_recent_under_threshold_commitments_state(
    st: &State,
) -> Vec<RecentCommitment> {
    let mut items: Vec<_> = commitment_history_canister_ids(st)
        .into_iter()
        .flat_map(|canister_id| {
            commitment_history_snapshot(st, canister_id)
                .into_iter()
                .filter(|item| !item.counts_toward_faucet)
                .map(move |item| RecentCommitment {
                    canister_id,
                    raw_icp_memo_text: None,
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

pub(super) fn fallback_recent_commitments(st: &State) -> Vec<RecentCommitmentListItem> {
    let mut items: Vec<_> = fallback_recent_qualifying_commitments_state(st)
        .into_iter()
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
    items.extend(
        fallback_recent_under_threshold_commitments_state(st)
            .into_iter()
            .map(|item| RecentCommitmentListItem {
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
            }),
    );
    if let Some(invalid) = &st.recent_invalid_commitments {
        items.extend(
            invalid
                .iter()
                .cloned()
                .map(|item| RecentCommitmentListItem {
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
                }),
        );
    }
    items.sort_by(|a, b| {
        let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
        let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
        b_key.cmp(&a_key)
    });
    items.truncate(
        MAX_RECENT_QUALIFYING_COMMITMENTS
            + MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS
            + MAX_RECENT_INVALID_COMMITMENTS,
    );
    items
}

pub(super) fn initialize_config_defaults_if_missing(st: &mut State) {
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
    if st.config.relay_setup_min_e8s == 0 {
        st.config.relay_setup_min_e8s = 300_000_000;
    }
    if st.config.relay_setup_dust_e8s == 0 {
        st.config.relay_setup_dust_e8s = 10_000;
    }
    if st.config.relay_setup_refund_cooldown_seconds == 0 {
        st.config.relay_setup_refund_cooldown_seconds = 300;
    }
    if st.config.relay_initial_cycles == 0 {
        st.config.relay_initial_cycles = 2_000_000_000_000;
    }
    if st.config.relay_cycle_safety_margin_e8s == 0 {
        st.config.relay_cycle_safety_margin_e8s = 5_000_000;
    }
    if st.config.relay_min_subaccount_one_seed_e8s == 0 {
        st.config.relay_min_subaccount_one_seed_e8s = 100_020_000;
    }
    if st.config.self_service_relay_interval_seconds == 0 {
        st.config.self_service_relay_interval_seconds = 86400;
    }
    if st.config.io_surplus_neuron_id == 0 {
        st.config.io_surplus_neuron_id = DEFAULT_IO_SURPLUS_NEURON_ID;
    }
    if st.config.canonical_relay_canister_id.is_none() {
        st.config.canonical_relay_canister_id = Some(mainnet_relay_id());
    }
    if st.config.canonical_relay_targets.is_empty() {
        st.config.canonical_relay_targets = mainnet_canonical_relay_targets();
    }
    ensure_canonical_relay_registry(st);
}

pub(crate) fn ensure_canonical_relay_registry(st: &mut State) {
    let Some(relay_canister_id) = st.config.canonical_relay_canister_id else {
        return;
    };
    let targets = st.config.canonical_relay_targets.clone();
    for target_canister_id in targets {
        let entry = RelayRegistryEntry {
            relay_canister_id,
            target_canister_id,
            kind: RelayRegistryKind::Canonical,
            status: RelayRegistryStatus::Active,
            setup_account: None,
            setup_account_identifier: None,
            setup_amount_e8s: None,
            setup_tx_ids: Vec::new(),
            relay_wasm_hash_hex: None,
            final_controllers: None,
            log_visibility_public: Some(true),
            created_at_ts: None,
            activated_at_ts: None,
        };
        st.relay_registry_by_target
            .entry(target_canister_id)
            .or_insert(entry);
    }
}

pub(crate) fn ensure_active_self_service_relay_targets_tracked(st: &mut State, now_secs: u64) {
    let stable_cycles_targets = state::stable_cycles_history_keys();
    let active_targets: Vec<_> = st
        .relay_registry_by_target
        .iter()
        .filter_map(|(target, entry)| {
            (entry.kind == RelayRegistryKind::SelfService
                && entry.status == RelayRegistryStatus::Active)
                .then_some(*target)
        })
        .collect();

    for target in active_targets {
        st.distinct_canisters.insert(target);
        let meta = st.per_canister_meta.entry(target).or_default();
        if meta.first_seen_ts.is_none() {
            meta.first_seen_ts = Some(now_secs);
        }
        let has_cycles_history = st
            .cycles_history
            .get(&target)
            .map(|history| !history.is_empty())
            .unwrap_or(false)
            || stable_cycles_targets.contains(&target);
        if !has_cycles_history
            && meta.last_cycles_probe_ts.is_none()
            && !st.initial_cycles_probe_queue.contains(&target)
        {
            scheduler::enqueue_initial_cycles_probe(st, target);
        }
    }
}

pub(super) fn initialize_derived_state_if_missing(st: &mut State) {
    if st.qualifying_commitment_count.is_none() {
        st.qualifying_commitment_count = Some(fallback_qualifying_commitment_count(st));
    }
    if st.recent_commitments.is_none() {
        st.recent_commitments = Some(fallback_recent_qualifying_commitments_state(st));
    }
    if st.recent_under_threshold_commitments.is_none() {
        st.recent_under_threshold_commitments =
            Some(fallback_recent_under_threshold_commitments_state(st));
    }
    if st.recent_neuron_commitments.is_none() {
        st.recent_neuron_commitments = Some(Vec::new());
    }
    if st.recent_under_threshold_neuron_commitments.is_none() {
        st.recent_under_threshold_neuron_commitments = Some(Vec::new());
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

pub(super) fn registered_canister_summary_for(
    st: &State,
    canister_id: Principal,
) -> Option<RegisteredCanisterSummary> {
    let sources = visible_sources_for_canister(st, &canister_id)?;
    let history = commitment_history_snapshot(st, canister_id);
    if history.is_empty() {
        return None;
    }
    let (qualifying_commitment_count, total_qualifying_committed_e8s, rollup_last_ts) =
        qualifying_rollup(&history);
    let meta = st
        .per_canister_meta
        .get(&canister_id)
        .cloned()
        .unwrap_or_default();
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

pub(super) fn registered_canister_summary_total_desc_key(
    item: &RegisteredCanisterSummary,
) -> (Reverse<u64>, Principal) {
    (
        Reverse(item.total_qualifying_committed_e8s),
        item.canister_id,
    )
}

pub(super) fn remove_registered_canister_from_total_desc_index(
    index: &mut Vec<Principal>,
    canister_id: Principal,
) {
    index.retain(|existing| *existing != canister_id);
}

pub(super) fn insert_registered_canister_into_total_desc_index(
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

pub(super) fn registered_canister_summaries_total_desc_page(
    st: &State,
    page: u32,
    page_size: u32,
) -> Option<ListRegisteredCanisterSummariesResponse> {
    let cache = st.registered_canister_summaries_cache.as_ref()?;
    let index = st.registered_canister_summaries_total_desc_index.as_ref()?;
    if index.len() != cache.len()
        || index
            .iter()
            .any(|canister_id| !cache.contains_key(canister_id))
    {
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
    let total_desc_index =
        registered_canister_summaries_total_desc_index.get_or_insert_with(Vec::new);
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

pub(super) fn registered_canister_summaries(st: &State) -> Vec<RegisteredCanisterSummary> {
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
pub(super) fn init(args: InitArgs) {
    let now_secs = ic_cdk::api::time() / 1_000_000_000;
    let cfg = config_from_init_args(args);
    state::init_stable_storage();
    let mut st = State::new(cfg, now_secs);
    initialize_config_defaults_if_missing(&mut st);
    normalize_runtime_state(&mut st);
    state::set_state(st);
    scheduler::install_timers();
    log_lifecycle("init_complete");
}

pub(super) fn apply_upgrade_args(st: &mut State, args: Option<UpgradeArgs>) {
    let previous_policy = st.config.effective_cycles_probe_policy();
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
            st.config.max_commitment_entries_per_canister =
                clamp_commitment_entries_per_canister(v);
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
        if let Some(v) = args.cycles_probe_policy {
            st.config.cycles_probe_policy = Some(v);
        } else if let Some(v) = args.blackhole_canister_id {
            st.config.cycles_probe_policy =
                Some(CyclesProbePolicy::FixedBlackhole { canister_id: v });
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
        if let Some(v) = args.xrc_canister_id {
            st.config.xrc_canister_id = v;
        }
        if let Some(v) = args.relay_factory_enabled {
            st.config.relay_factory_enabled = v;
        }
        if let Some(v) = args.relay_setup_min_e8s {
            st.config.relay_setup_min_e8s = v;
        }
        if let Some(v) = args.relay_setup_dust_e8s {
            st.config.relay_setup_dust_e8s = v;
        }
        if let Some(v) = args.relay_setup_refund_cooldown_seconds {
            st.config.relay_setup_refund_cooldown_seconds = v;
        }
        if let Some(v) = args.relay_initial_cycles {
            st.config.relay_initial_cycles = v;
        }
        if let Some(v) = args.relay_cycle_safety_margin_e8s {
            st.config.relay_cycle_safety_margin_e8s = v;
        }
        if let Some(v) = args.relay_min_subaccount_one_seed_e8s {
            st.config.relay_min_subaccount_one_seed_e8s = v;
        }
        if let Some(v) = args.self_service_relay_interval_seconds {
            st.config.self_service_relay_interval_seconds = v.max(60);
        }
        if let Some(v) = args.self_service_relay_max_transfers_per_tick {
            st.config.self_service_relay_max_transfers_per_tick = v;
        }
        if let Some(v) = args.io_surplus_neuron_id {
            st.config.io_surplus_neuron_id = v;
        }
        if let Some(v) = args.canonical_relay_canister_id {
            st.config.canonical_relay_canister_id = v;
        }
        if let Some(v) = args.canonical_relay_targets {
            st.config.canonical_relay_targets = v;
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
    ensure_canonical_relay_registry(st);
    validate_config(&st.config);
    if st.config.effective_cycles_probe_policy() != previous_policy {
        st.cached_cycles_probe_routes.clear();
    }
    st.main_lock_state_ts = Some(0);
}

pub(super) fn decode_post_upgrade_args_from_bytes(
    raw: &[u8],
) -> Result<Option<UpgradeArgs>, String> {
    jupiter_ic_clients::lifecycle::decode_post_upgrade_args::<InitArgs, UpgradeArgs>(
        "historian",
        raw,
    )
}

pub(super) fn decode_post_upgrade_args(raw: Vec<u8>) -> Option<UpgradeArgs> {
    decode_post_upgrade_args_from_bytes(&raw).unwrap_or_else(|err| ic_cdk::trap(&err))
}

#[ic_cdk::post_upgrade(decode_with = "decode_post_upgrade_args")]
pub(super) fn post_upgrade(args: Option<UpgradeArgs>) {
    post_upgrade_with_backfill_timestamp(args, ic_cdk::api::time() / 1_000_000_000);
}

pub(crate) fn post_upgrade_with_backfill_timestamp(args: Option<UpgradeArgs>, now_secs: u64) {
    restore_post_upgrade_state_with_backfill_timestamp(args, now_secs);
    scheduler::install_timers();
    log_lifecycle("post_upgrade_complete");
}

pub(crate) fn restore_post_upgrade_state_with_backfill_timestamp(
    args: Option<UpgradeArgs>,
    now_secs: u64,
) {
    state::init_stable_storage();
    let mut st: State = state::restore_state_from_stable()
        .expect("stable state missing during historian post_upgrade");
    initialize_config_defaults_if_missing(&mut st);
    apply_upgrade_args(&mut st, args);
    ensure_active_self_service_relay_targets_tracked(&mut st, now_secs);
    let tracked_self_service_targets: std::collections::BTreeSet<_> = st
        .relay_registry_by_target
        .iter()
        .filter_map(|(target, entry)| {
            (entry.kind == RelayRegistryKind::SelfService
                && entry.status == RelayRegistryStatus::Active
                && st.per_canister_meta.contains_key(target))
            .then_some(*target)
        })
        .collect();
    // Persist only the historian root on upgrade. Commitment/cycles histories are
    // restored lazily from stable entry/index maps, so rewriting all durable sections
    // here would clobber those bulk histories with an intentionally sparse heap view.
    state::set_state_root_and_registry_principals(st, &tracked_self_service_targets);
}

fn log_lifecycle(event: &str) {
    let main_interval_seconds = state::with_state(|st| st.config.scan_interval_seconds);
    ic_cdk::println!(
        "{}",
        format_event_line(
            "historian",
            "LIFECYCLE",
            &[
                (FIELD_EVENT, event.to_string()),
                (FIELD_TIMERS_INSTALLED, true.to_string()),
                (
                    FIELD_MAIN_INTERVAL_SECONDS,
                    main_interval_seconds.to_string()
                ),
            ],
        )
    );
}
