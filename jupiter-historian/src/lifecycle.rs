use super::*;
pub(super) fn mainnet_ledger_id() -> Principal {
    Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid hardcoded ledger principal")
}

pub(super) fn mainnet_index_id() -> Principal {
    Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").expect("invalid hardcoded index principal")
}

pub(super) fn mainnet_blackhole_id() -> Principal {
    Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").expect("invalid hardcoded blackhole principal")
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

pub(super) fn mainnet_sns_wasm_id() -> Principal {
    Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("invalid hardcoded sns-wasm principal")
}

pub(crate) fn mainnet_xrc_id() -> Principal {
    jupiter_ic_clients::xrc::mainnet_xrc_canister_id()
}

#[cfg(any(test, feature = "debug_api"))]
pub(super) fn production_canister_id() -> Principal {
    Principal::from_text(env!("JUPITER_HISTORIAN_PROD_CANISTER_ID")).expect("invalid embedded production canister principal")
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
        xrc_canister_id: args.xrc_canister_id.unwrap_or_else(mainnet_xrc_id),
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

pub(super) fn count_registered_canisters(st: &State) -> u64 {
    st.canister_sources
        .iter()
        .filter(|(canister_id, sources)| memo_source_is_registered(st, canister_id, sources))
        .count() as u64
}

pub(super) fn count_sns_discovered_canisters(st: &State) -> u64 {
    st.canister_sources
        .values()
        .filter(|sources| sources.contains(&CanisterSource::SnsDiscovery))
        .count() as u64
}


pub(super) fn effective_faucet_canister_id(st: &State) -> Principal {
    st.config.faucet_canister_id.clone().unwrap_or_else(mainnet_faucet_id)
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
    history.iter().max_by_key(|item| item.timestamp_nanos).map(|item| item.cycles)
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

pub(super) fn commitment_history_snapshot(st: &State, canister_id: Principal) -> Vec<CommitmentSample> {
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

pub(super) fn raw_icp_commitment_history_snapshot(st: &State, canister_id: Principal) -> Vec<CommitmentSample> {
    st.raw_icp_commitment_history
        .get(&canister_id)
        .cloned()
        .unwrap_or_else(|| state::stable_raw_icp_commitment_history_for(canister_id))
}

pub(super) fn neuron_commitment_history_snapshot(st: &State, neuron_id: u64) -> Vec<CommitmentSample> {
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

pub(super) fn fallback_recent_under_threshold_commitments_state(st: &State) -> Vec<RecentCommitment> {
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
        items.extend(invalid.iter().cloned().map(|item| RecentCommitmentListItem {
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
    items.sort_by(|a, b| {
        let a_key = (a.timestamp_nanos.unwrap_or(0), a.tx_id);
        let b_key = (b.timestamp_nanos.unwrap_or(0), b.tx_id);
        b_key.cmp(&a_key)
    });
    items.truncate(MAX_RECENT_QUALIFYING_COMMITMENTS + MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS + MAX_RECENT_INVALID_COMMITMENTS);
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
}

pub(super) fn initialize_derived_state_if_missing(st: &mut State) {
    if st.qualifying_commitment_count.is_none() {
        st.qualifying_commitment_count = Some(fallback_qualifying_commitment_count(st));
    }
    if st.recent_commitments.is_none() {
        st.recent_commitments = Some(fallback_recent_qualifying_commitments_state(st));
    }
    if st.recent_under_threshold_commitments.is_none() {
        st.recent_under_threshold_commitments = Some(fallback_recent_under_threshold_commitments_state(st));
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

pub(super) fn registered_canister_summary_for(st: &State, canister_id: Principal) -> Option<RegisteredCanisterSummary> {
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

pub(super) fn registered_canister_summary_total_desc_key(item: &RegisteredCanisterSummary) -> (Reverse<u64>, Principal) {
    (Reverse(item.total_qualifying_committed_e8s), item.canister_id)
}

pub(super) fn remove_registered_canister_from_total_desc_index(index: &mut Vec<Principal>, canister_id: Principal) {
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
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let cfg = config_from_init_args(args);
    state::init_stable_storage();
    let mut st = State::new(cfg, now_secs);
    initialize_config_defaults_if_missing(&mut st);
    normalize_runtime_state(&mut st);
    state::set_state(st);
    scheduler::install_timers();
}

pub(super) fn apply_upgrade_args(st: &mut State, args: Option<UpgradeArgs>) {
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
        if let Some(v) = args.xrc_canister_id {
            st.config.xrc_canister_id = v;
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
pub(super) fn post_upgrade(args: Option<UpgradeArgs>) {
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

