use super::*;
use jupiter_ic_clients::sns::ListSnsCanistersResponse;
use std::collections::BTreeSet;

fn register_sns_membership(canister_ids: BTreeSet<candid::Principal>, now_secs: u64) {
    for canister_id in canister_ids {
        state::with_root_and_registry_canister_state_mut(canister_id, |st| {
            st.distinct_canisters.insert(canister_id);
            st.canister_tracking_reasons.insert(
                canister_id,
                logic::merge_tracking_reasons(
                    st.canister_tracking_reasons.get(&canister_id),
                    CanisterTrackingReason::SnsDiscovery,
                ),
            );
            let meta = st
                .per_canister_meta
                .entry(canister_id)
                .or_insert_with(CanisterMeta::default);
            if meta.first_seen_ts.is_none() {
                meta.first_seen_ts = Some(now_secs);
            }
            if meta.last_cycles_probe_ts.is_none() {
                enqueue_initial_cycles_probe(st, canister_id);
            }
            crate::refresh_memo_registered_canister_summary(st, canister_id);
        });
    }
}

fn authoritative_membership(
    requested_root: candid::Principal,
    response: ListSnsCanistersResponse,
) -> Result<BTreeSet<candid::Principal>, String> {
    if response
        .root
        .is_some_and(|returned| returned != requested_root)
    {
        return Err(format!(
            "SNS Root {} returned a conflicting root principal",
            requested_root.to_text()
        ));
    }

    let mut canister_ids = BTreeSet::from([requested_root]);
    canister_ids.extend(
        [
            response.governance,
            response.ledger,
            response.swap,
            response.index,
        ]
        .into_iter()
        .flatten(),
    );
    canister_ids.extend(response.dapps);
    canister_ids.extend(response.archives);
    if let Some(extensions) = response.extensions {
        canister_ids.extend(extensions.extension_canister_ids);
    }
    Ok(canister_ids)
}

pub(super) async fn process_sns_discovery<W: SnsWasmClient, R: SnsRootClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    sns_wasm: &W,
    sns_root: &R,
) -> Result<(), String> {
    let (snapshot, max_per_tick) = state::with_root_state_mut(|st| {
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

    let start = snapshot.next_index as usize;
    let end = (snapshot.next_index + max_per_tick as u64)
        .min(snapshot.root_canister_ids.len() as u64) as usize;
    for root_id in snapshot.root_canister_ids[start..end].iter().copied() {
        register_sns_membership(BTreeSet::from([root_id]), now_secs);
        let response = match sns_root.list_sns_canisters(root_id).await {
            Ok(response) => response,
            Err(err) => {
                log_error(&format!(
                    "historian SNS discovery skipped membership for {} after list_sns_canisters failed: {err}",
                    root_id.to_text()
                ));
                continue;
            }
        };
        match authoritative_membership(root_id, response) {
            Ok(canister_ids) => register_sns_membership(canister_ids, now_secs),
            Err(err) => log_error(&format!(
                "historian SNS discovery rejected membership: {err}"
            )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_ic_clients::sns::SnsExtensions;

    fn principal(byte: u8) -> candid::Principal {
        candid::Principal::from_slice(&[byte])
    }

    #[test]
    fn membership_includes_every_supported_field_and_deduplicates() {
        let root = principal(1);
        let result = authoritative_membership(
            root,
            ListSnsCanistersResponse {
                root: Some(root),
                governance: Some(principal(2)),
                ledger: Some(principal(3)),
                swap: Some(principal(4)),
                index: Some(principal(5)),
                dapps: vec![principal(6), principal(2)],
                archives: vec![principal(7), principal(6)],
                extensions: Some(SnsExtensions {
                    extension_canister_ids: vec![principal(8), principal(7)],
                }),
            },
        )
        .unwrap();
        assert_eq!(result, (1..=8).map(principal).collect());
    }

    #[test]
    fn missing_extensions_is_supported_and_conflicting_root_is_rejected() {
        let root = principal(1);
        assert_eq!(
            authoritative_membership(
                root,
                ListSnsCanistersResponse {
                    root: None,
                    extensions: None,
                    ..Default::default()
                }
            )
            .unwrap(),
            BTreeSet::from([root])
        );
        assert!(authoritative_membership(
            root,
            ListSnsCanistersResponse {
                root: Some(principal(9)),
                dapps: vec![principal(10)],
                ..Default::default()
            }
        )
        .is_err());
    }
}
