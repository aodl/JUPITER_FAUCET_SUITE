use super::*;
pub(super) fn apply_sns_canister_summary(
    timestamp_nanos: u64,
    now_secs: u64,
    max_cycles_entries: u32,
    summary: SnsCanisterSummary,
) {
    let Some(canister_id) = summary.canister_id else {
        return;
    };
    let cycles = summary
        .status
        .and_then(|status| status.cycles)
        .and_then(|cycles| nat_to_u128(&cycles));
    let dirty_canister_id = canister_id;
    state::with_root_registry_and_cycles_canister_state_mut(dirty_canister_id, |st| {
        st.distinct_canisters.insert(canister_id);
        st.canister_tracking_reasons.insert(
            canister_id,
            logic::merge_tracking_reasons(
                st.canister_tracking_reasons.get(&canister_id),
                CanisterTrackingReason::SnsDiscovery,
            ),
        );
        if let Some(cycles) = cycles {
            crate::state::ensure_cycles_history_loaded(st, canister_id);
            let history = st.cycles_history.entry(canister_id).or_default();
            let inserted = logic::push_cycles_sample(
                history,
                logic::make_cycles_sample(
                    timestamp_nanos,
                    cycles,
                    CyclesSampleSource::SnsRootSummary,
                ),
                max_cycles_entries,
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
                    CyclesProbeResult::Ok(CyclesSampleSource::SnsRootSummary),
                );
            }
        } else {
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
                CyclesProbeResult::NotAvailable,
            );
        }
        crate::refresh_registered_canister_summary(st, canister_id);
    });
}

pub(super) async fn process_sns_discovery<W: SnsWasmClient, R: SnsRootClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    sns_wasm: &W,
    sns_root: &R,
) -> Result<(), String> {
    let (snapshot, max_per_tick, max_cycles_entries) = state::with_root_state_mut(|st| {
        if st.active_sns_discovery.is_none() {
            st.active_sns_discovery = Some(ActiveSnsDiscovery {
                started_at_ts_nanos: timestamp_nanos,
                root_canister_ids: Vec::new(),
                next_index: 0,
            });
        }
        (
            st.active_sns_discovery
                .clone()
                .expect("active sns discovery"),
            st.config.max_canisters_per_cycles_tick.max(1),
            st.config.max_cycles_entries_per_canister,
        )
    });

    let snapshot = if snapshot.root_canister_ids.is_empty() && snapshot.next_index == 0 {
        let deployed = sns_wasm
            .list_deployed_snses()
            .await
            .map_err(|e| format!("list_deployed_snses failed: {e}"))?;
        let mut root_canister_ids: Vec<_> = deployed
            .instances
            .into_iter()
            .filter_map(|sns| sns.root_canister_id)
            .collect();
        root_canister_ids.sort();
        root_canister_ids.dedup();
        state::with_root_state_mut(|st| {
            if let Some(active) = st.active_sns_discovery.as_mut() {
                active.root_canister_ids = root_canister_ids.clone();
            }
        });
        ActiveSnsDiscovery {
            started_at_ts_nanos: snapshot.started_at_ts_nanos,
            root_canister_ids,
            next_index: 0,
        }
    } else {
        snapshot
    };

    let discovery_timestamp_nanos = snapshot.started_at_ts_nanos;
    let start = snapshot.next_index as usize;
    let end = (snapshot.next_index + max_per_tick as u64)
        .min(snapshot.root_canister_ids.len() as u64) as usize;
    for root_id in snapshot.root_canister_ids[start..end].iter().copied() {
        let summary = match sns_root.get_sns_canisters_summary(root_id).await {
            Ok(summary) => summary,
            Err(err) => {
                log_error(&format!(
                    "historian SNS discovery skipped root {} after get_sns_canisters_summary failed: {err}",
                    root_id.to_text()
                ));
                continue;
            }
        };
        if let Some(summary) = summary.root {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
        if let Some(summary) = summary.governance {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
        if let Some(summary) = summary.ledger {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
        if let Some(summary) = summary.swap {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
        if let Some(summary) = summary.index {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
        for summary in summary.dapps {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
        for summary in summary.archives {
            apply_sns_canister_summary(
                discovery_timestamp_nanos,
                now_secs,
                max_cycles_entries,
                summary,
            );
        }
    }

    state::with_root_state_mut(|st| {
        if let Some(active) = st.active_sns_discovery.as_mut() {
            active.next_index = end as u64;
            if active.next_index >= active.root_canister_ids.len() as u64 {
                st.active_sns_discovery = None;
                st.last_sns_discovery_ts = now_secs;
            }
        }
    });
    Ok(())
}
