use std::collections::BTreeMap;

use candid::Principal;

use crate::clients::BlackholeClient;
use crate::logic;
use crate::state::{CyclesSnapshot, ProbeFailure};

pub(super) async fn probe_cycles<B: BlackholeClient>(
    canisters: &[Principal],
    self_id: Principal,
    now_nanos: u64,
    blackhole: &B,
) -> (BTreeMap<Principal, CyclesSnapshot>, Vec<ProbeFailure>) {
    let mut snapshots = BTreeMap::new();
    let mut failures = Vec::new();
    for canister_id in canisters {
        let source = logic::sample_source_for(*canister_id, self_id);
        let result = if *canister_id == self_id {
            Ok(ic_cdk::api::canister_cycle_balance())
        } else {
            blackhole.cycles_balance(*canister_id).await
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
            }),
        }
    }
    (snapshots, failures)
}
