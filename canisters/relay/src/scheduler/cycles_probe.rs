use std::collections::BTreeMap;

use candid::Principal;

use crate::clients::BlackholeClient;
use crate::logic;
use crate::state::{CyclesSnapshot, ProbeFailure};

pub(super) async fn probe_cycles<B: BlackholeClient>(
    canisters: &[Principal],
    self_id: Principal,
    self_cycles: u128,
    configured_blackhole_canister_id: Principal,
    now_nanos: u64,
    blackhole: &B,
) -> (BTreeMap<Principal, CyclesSnapshot>, Vec<ProbeFailure>) {
    let mut snapshots = BTreeMap::new();
    let mut failures = Vec::new();
    for canister_id in canisters {
        let source = logic::sample_source_for(*canister_id, self_id);
        let result = if *canister_id == self_id {
            Ok(self_cycles)
        } else {
            let probe_canister_id =
                logic::probe_canister_for(*canister_id, self_id, configured_blackhole_canister_id);
            blackhole
                .cycles_balance_via(probe_canister_id, *canister_id)
                .await
        };
        match result {
            Ok(cycles) => {
                snapshots.insert(
                    *canister_id,
                    CyclesSnapshot {
                        cycles,
                        timestamp_nanos: now_nanos,
                        source,
                    },
                );
            }
            Err(err) => failures.push(ProbeFailure {
                canister_id: *canister_id,
                error: err.to_string(),
                consecutive_failures: 0,
            }),
        }
    }
    (snapshots, failures)
}
