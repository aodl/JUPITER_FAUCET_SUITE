use super::*;

fn cycles_sample_source_for_route(route: Option<&CyclesProbeRoute>) -> CyclesSampleSource {
    match route {
        None => CyclesSampleSource::SelfCanister,
        Some(CyclesProbeRoute::Blackhole { .. }) => CyclesSampleSource::BlackholeStatus,
        Some(CyclesProbeRoute::SnsRoot { .. }) => CyclesSampleSource::SnsRootStatus,
        Some(CyclesProbeRoute::SnsSwap { .. }) => CyclesSampleSource::SnsSwapStatus,
    }
}

pub(super) async fn probe_and_record_cycles<C: CyclesProbeClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    canister_id: candid::Principal,
    max_entries: u32,
    cycles_probe_client: &C,
) -> Result<(), String> {
    let policy = CyclesProbePolicy::Auto;
    let cached_route =
        state::with_state(|st| st.cached_cycles_probe_routes.get(&canister_id).cloned());

    let outcome =
        shared_probe_cycles(&policy, canister_id, cached_route, cycles_probe_client).await;
    match outcome {
        Ok(CyclesProbeSuccess { cycles, route }) => {
            if route.is_none() {
                log_cycles_once_per_week(cycles);
                log_current_config();
            }
            let source = cycles_sample_source_for_route(route.as_ref());
            state::with_root_registry_and_cycles_canister_state_mut(canister_id, |st| {
                if let Some(route) = route.clone() {
                    st.cached_cycles_probe_routes.insert(canister_id, route);
                } else {
                    st.cached_cycles_probe_routes.remove(&canister_id);
                }
                crate::state::ensure_cycles_history_loaded(st, canister_id);
                let history = st.cycles_history.entry(canister_id).or_default();
                let inserted = logic::push_cycles_sample(
                    history,
                    logic::make_cycles_sample(timestamp_nanos, cycles, source.clone()),
                    max_entries,
                );
                let meta = st
                    .per_canister_meta
                    .entry(canister_id)
                    .or_insert_with(CanisterMeta::default);
                if meta.first_seen_ts.is_none() {
                    meta.first_seen_ts = Some(now_secs);
                }
                if inserted {
                    logic::apply_cycles_probe_result(
                        meta,
                        timestamp_nanos,
                        CyclesProbeResult::Ok(source),
                    );
                }
                crate::refresh_memo_registered_canister_summary(st, canister_id);
            });
        }
        Err(err) => {
            let message = err.message;
            state::with_root_and_registry_canister_state_mut(canister_id, |st| {
                st.cached_cycles_probe_routes.remove(&canister_id);
                let meta = st
                    .per_canister_meta
                    .entry(canister_id)
                    .or_insert_with(CanisterMeta::default);
                if meta.first_seen_ts.is_none() {
                    meta.first_seen_ts = Some(now_secs);
                }
                logic::apply_cycles_probe_result(
                    meta,
                    timestamp_nanos,
                    CyclesProbeResult::Error(message),
                );
                crate::refresh_memo_registered_canister_summary(st, canister_id);
            });
        }
    }
    Ok(())
}

pub(super) async fn process_initial_cycles_probe_queue<
    C: CyclesProbeClient,
    G: GovernanceClient,
>(
    timestamp_nanos: u64,
    now_secs: u64,
    cycles_probe_client: &C,
    governance: &G,
) -> Result<(), String> {
    let (targets, max_entries) = state::with_root_state_mut(|st| {
        let max_per_tick = st.config.max_canisters_per_cycles_tick.max(1) as usize;
        let mut pending = std::mem::take(&mut st.initial_cycles_probe_queue);
        let mut selected = Vec::new();

        while selected.len() < max_per_tick && !pending.is_empty() {
            let canister_id = pending.remove(0);
            let already_probed = st
                .per_canister_meta
                .get(&canister_id)
                .and_then(|meta| meta.last_cycles_probe_ts)
                .is_some();
            if already_probed {
                continue;
            }
            if should_probe_tracked_canister(st, canister_id) {
                selected.push(canister_id);
            }
        }

        st.initial_cycles_probe_queue = pending;
        (selected, st.config.max_cycles_entries_per_canister)
    });

    let should_refresh_stake = !targets.is_empty();

    for canister_id in targets {
        if let Err(err) = probe_and_record_cycles(
            timestamp_nanos,
            now_secs,
            canister_id,
            max_entries,
            cycles_probe_client,
        )
        .await
        {
            log_error(&format!(
                "historian initial cycles probe degraded for {}: {err}",
                canister_id.to_text()
            ));
        }
    }

    if should_refresh_stake {
        refresh_staking_neuron_after_registration(governance).await;
    }

    Ok(())
}

pub(super) async fn refresh_staking_neuron_after_registration<G: GovernanceClient>(governance: &G) {
    let subaccount = state::with_state(|st| st.config.staking_account.subaccount);
    let Some(subaccount) = subaccount else {
        log_error("historian staking neuron refresh skipped: staking account has no subaccount");
        return;
    };

    // A qualifying registration is a staking-account top-up, so ask NNS governance
    // to refresh the neuron stake cache immediately rather than waiting for the
    // disburser's next periodic maintenance tick. This is best-effort: historian
    // cycles history should not fail just because the NNS refresh is temporarily
    // unavailable.
    if let Err(err) = governance
        .claim_or_refresh_neuron_by_subaccount(subaccount)
        .await
    {
        log_error(&format!("historian staking neuron refresh failed: {err}"));
    }
}

pub(super) async fn process_cycles_sweep<C: CyclesProbeClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    cycles_probe_client: &C,
) -> Result<(), String> {
    let (snapshot, max_per_tick, max_entries) = state::with_root_state_mut(|st| {
        if st.active_cycles_sweep.is_none() {
            let self_id = ic_cdk::api::canister_self();
            let canisters = build_cycles_sweep_canisters(st, self_id);
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: timestamp_nanos,
                canisters,
                next_index: 0,
            });
        }
        (
            st.active_cycles_sweep.clone().expect("active sweep"),
            st.config.max_canisters_per_cycles_tick.max(1),
            st.config.max_cycles_entries_per_canister,
        )
    });

    let started_at_ts_nanos = snapshot.started_at_ts_nanos;
    let start = snapshot.next_index as usize;
    let end =
        (snapshot.next_index + max_per_tick as u64).min(snapshot.canisters.len() as u64) as usize;
    for canister_id in snapshot.canisters[start..end].iter().copied() {
        if let Err(err) = probe_and_record_cycles(
            started_at_ts_nanos,
            now_secs,
            canister_id,
            max_entries,
            cycles_probe_client,
        )
        .await
        {
            log_error(&format!(
                "historian cycles sweep degraded for {}: {err}",
                canister_id.to_text()
            ));
        }
    }

    state::with_root_state_mut(|st| {
        if let Some(active) = st.active_cycles_sweep.as_mut() {
            active.next_index = end as u64;
            if active.next_index >= active.canisters.len() as u64 {
                st.active_cycles_sweep = None;
                st.last_completed_cycles_sweep_ts = now_secs;
            }
        }
    });
    Ok(())
}
