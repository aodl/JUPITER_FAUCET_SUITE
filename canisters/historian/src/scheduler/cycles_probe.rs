use super::*;
pub(super) async fn probe_and_record_cycles<B: BlackholeClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    canister_id: candid::Principal,
    max_entries: u32,
    self_canister_id: Option<candid::Principal>,
    blackhole: &B,
) -> Result<(), String> {
    if self_canister_id
        .map(|self_id| canister_id == self_id)
        .unwrap_or(false)
    {
        let cycles = ic_cdk::api::canister_cycle_balance();
        log_cycles_once_per_week(cycles);
        log_current_config();
        state::with_root_registry_and_cycles_canister_state_mut(canister_id, |st| {
            crate::state::ensure_cycles_history_loaded(st, canister_id);
            let history = st.cycles_history.entry(canister_id).or_default();
            let inserted = logic::push_cycles_sample(
                history,
                logic::make_cycles_sample(
                    timestamp_nanos,
                    cycles,
                    CyclesSampleSource::SelfCanister,
                ),
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
                    CyclesProbeResult::Ok(CyclesSampleSource::SelfCanister),
                );
            }
            crate::refresh_registered_canister_summary(st, canister_id);
        });
        return Ok(());
    }

    match blackhole.canister_status(canister_id).await {
        Ok(status) => {
            let cycles = nat_to_u128(&status.cycles)
                .ok_or_else(|| "cycles overflow converting nat to u128".to_string())?;
            state::with_root_registry_and_cycles_canister_state_mut(canister_id, |st| {
                crate::state::ensure_cycles_history_loaded(st, canister_id);
                let history = st.cycles_history.entry(canister_id).or_default();
                let inserted = logic::push_cycles_sample(
                    history,
                    logic::make_cycles_sample(
                        timestamp_nanos,
                        cycles,
                        CyclesSampleSource::BlackholeStatus,
                    ),
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
                        CyclesProbeResult::Ok(CyclesSampleSource::BlackholeStatus),
                    );
                }
                crate::refresh_registered_canister_summary(st, canister_id);
            });
        }
        Err(err) => {
            state::with_root_and_registry_canister_state_mut(canister_id, |st| {
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
                    CyclesProbeResult::Error(err.to_string()),
                );
                crate::refresh_registered_canister_summary(st, canister_id);
            });
        }
    }
    Ok(())
}

pub(super) async fn process_initial_cycles_probe_queue<B: BlackholeClient, G: GovernanceClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    blackhole: &B,
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
            if should_probe_memo_registered_canister(st, canister_id) {
                selected.push(canister_id);
            }
        }

        st.initial_cycles_probe_queue = pending;
        (selected, st.config.max_cycles_entries_per_canister)
    });

    let should_refresh_stake = !targets.is_empty();
    let mut first_probe_error = None;

    for canister_id in targets {
        if let Err(err) = probe_and_record_cycles(
            timestamp_nanos,
            now_secs,
            canister_id,
            max_entries,
            None,
            blackhole,
        )
        .await
        {
            if first_probe_error.is_none() {
                first_probe_error = Some(err);
            }
        }
    }

    if should_refresh_stake {
        refresh_staking_neuron_after_registration(governance).await;
    }

    if let Some(err) = first_probe_error {
        return Err(err);
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

pub(super) async fn process_cycles_sweep<B: BlackholeClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    blackhole: &B,
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

    let self_id = ic_cdk::api::canister_self();
    let started_at_ts_nanos = snapshot.started_at_ts_nanos;
    let start = snapshot.next_index as usize;
    let end =
        (snapshot.next_index + max_per_tick as u64).min(snapshot.canisters.len() as u64) as usize;
    for canister_id in snapshot.canisters[start..end].iter().copied() {
        probe_and_record_cycles(
            started_at_ts_nanos,
            now_secs,
            canister_id,
            max_entries,
            Some(self_id),
            blackhole,
        )
        .await?;
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
