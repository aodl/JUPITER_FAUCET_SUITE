use std::collections::BTreeMap;

use candid::Principal;
use jupiter_ic_clients::cycles_probe::{
    probe_cycles as shared_probe_cycles, CyclesProbeClient, CyclesProbePolicy, CyclesProbeRoute,
};
use jupiter_ic_clients::ClientError;

use crate::state::{CyclesSampleSource, CyclesSnapshot, ProbeFailure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RelayProbeBatch {
    pub(super) snapshots: BTreeMap<Principal, CyclesSnapshot>,
    pub(super) failures: Vec<ProbeFailure>,
    pub(super) route_updates: BTreeMap<Principal, Option<CyclesProbeRoute>>,
}

pub(super) struct RelayCyclesProbeClient<'a, C> {
    inner: &'a C,
    self_canister_id: Principal,
    self_cycles: u128,
}

impl<'a, C> RelayCyclesProbeClient<'a, C> {
    pub(super) fn new(inner: &'a C, self_canister_id: Principal, self_cycles: u128) -> Self {
        Self {
            inner,
            self_canister_id,
            self_cycles,
        }
    }
}

impl<C: CyclesProbeClient> CyclesProbeClient for RelayCyclesProbeClient<'_, C> {
    async fn self_cycles(&self, target: Principal) -> Option<u128> {
        if target == self.self_canister_id {
            Some(self.self_cycles)
        } else {
            None
        }
    }

    async fn blackhole_cycles(
        &self,
        probe_canister_id: Principal,
        target_canister_id: Principal,
    ) -> Result<u128, ClientError> {
        self.inner
            .blackhole_cycles(probe_canister_id, target_canister_id)
            .await
    }

    async fn list_deployed_snses(
        &self,
    ) -> Result<jupiter_ic_clients::sns::ListDeployedSnsesResponse, ClientError> {
        self.inner.list_deployed_snses().await
    }

    async fn canister_info_controllers(
        &self,
        target: Principal,
    ) -> Result<Vec<Principal>, ClientError> {
        self.inner.canister_info_controllers(target).await
    }

    async fn list_sns_canisters(
        &self,
        root_canister_id: Principal,
    ) -> Result<jupiter_ic_clients::sns::ListSnsCanistersResponse, ClientError> {
        self.inner.list_sns_canisters(root_canister_id).await
    }

    async fn sns_root_cycles(
        &self,
        root_canister_id: Principal,
        target_canister_id: Principal,
    ) -> Result<u128, ClientError> {
        self.inner
            .sns_root_cycles(root_canister_id, target_canister_id)
            .await
    }

    async fn sns_swap_cycles(&self, swap_canister_id: Principal) -> Result<u128, ClientError> {
        self.inner.sns_swap_cycles(swap_canister_id).await
    }
}

pub(super) async fn probe_cycles_batch<C: CyclesProbeClient>(
    canisters: &[Principal],
    policy: &CyclesProbePolicy,
    cached_routes: BTreeMap<Principal, CyclesProbeRoute>,
    now_nanos: u64,
    client: &C,
) -> RelayProbeBatch {
    let mut snapshots = BTreeMap::new();
    let mut failures = Vec::new();
    let mut route_updates = BTreeMap::new();

    for canister_id in canisters {
        let cached_route = match policy {
            CyclesProbePolicy::Auto => cached_routes.get(canister_id).cloned(),
            CyclesProbePolicy::FixedBlackhole { .. } => None,
        };
        let result = shared_probe_cycles(policy, *canister_id, cached_route, client).await;
        match result {
            Ok(success) => {
                snapshots.insert(
                    *canister_id,
                    CyclesSnapshot {
                        cycles: success.cycles,
                        timestamp_nanos: now_nanos,
                        source: sample_source_from_route(success.route.as_ref()),
                    },
                );
                route_updates.insert(*canister_id, success.route);
            }
            Err(err) => {
                failures.push(ProbeFailure {
                    canister_id: *canister_id,
                    error: err.message,
                    consecutive_failures: 0,
                });
                route_updates.insert(*canister_id, None);
            }
        }
    }

    RelayProbeBatch {
        snapshots,
        failures,
        route_updates,
    }
}

pub(crate) fn sample_source_from_route(route: Option<&CyclesProbeRoute>) -> CyclesSampleSource {
    match route {
        None => CyclesSampleSource::SelfCanister,
        Some(CyclesProbeRoute::Blackhole { .. }) => CyclesSampleSource::BlackholeStatus,
        Some(CyclesProbeRoute::SnsRoot { .. }) => CyclesSampleSource::SnsRootStatus,
        Some(CyclesProbeRoute::SnsSwap { .. }) => CyclesSampleSource::SnsSwapStatus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_ic_clients::constants;
    use jupiter_ic_clients::sns::{ListDeployedSnsesResponse, ListSnsCanistersResponse};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[derive(Default)]
    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match Pin::new(&mut future).poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    #[derive(Clone)]
    enum TestResponse {
        Ok(u128),
        Err(&'static str),
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum TestCall {
        Blackhole { probe: Principal, target: Principal },
        ListDeployedSnses,
        CanisterInfo(Principal),
        ListSnsCanisters(Principal),
        SnsRootStatus { root: Principal, target: Principal },
        SnsSwapStatus(Principal),
    }

    struct RecordingClient {
        blackhole: BTreeMap<Principal, TestResponse>,
        sns_root: BTreeMap<Principal, TestResponse>,
        sns_swap: BTreeMap<Principal, TestResponse>,
        deployed: Result<ListDeployedSnsesResponse, &'static str>,
        controllers: Result<Vec<Principal>, &'static str>,
        root_lists: BTreeMap<Principal, Result<ListSnsCanistersResponse, &'static str>>,
        calls: Mutex<Vec<TestCall>>,
    }

    impl Default for RecordingClient {
        fn default() -> Self {
            Self {
                blackhole: BTreeMap::new(),
                sns_root: BTreeMap::new(),
                sns_swap: BTreeMap::new(),
                deployed: Ok(ListDeployedSnsesResponse::default()),
                controllers: Ok(Vec::new()),
                root_lists: BTreeMap::new(),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl RecordingClient {
        fn calls(&self) -> Vec<TestCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Default for TestResponse {
        fn default() -> Self {
            Self::Err("missing")
        }
    }

    impl CyclesProbeClient for RecordingClient {
        async fn self_cycles(&self, _target: Principal) -> Option<u128> {
            None
        }

        async fn blackhole_cycles(
            &self,
            probe_canister_id: Principal,
            target_canister_id: Principal,
        ) -> Result<u128, ClientError> {
            self.calls.lock().unwrap().push(TestCall::Blackhole {
                probe: probe_canister_id,
                target: target_canister_id,
            });
            response_to_result(self.blackhole.get(&probe_canister_id).cloned())
        }

        async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError> {
            self.calls.lock().unwrap().push(TestCall::ListDeployedSnses);
            self.deployed
                .clone()
                .map_err(|err| ClientError::Call(err.to_string()))
        }

        async fn canister_info_controllers(
            &self,
            target: Principal,
        ) -> Result<Vec<Principal>, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(TestCall::CanisterInfo(target));
            self.controllers
                .clone()
                .map_err(|err| ClientError::Call(err.to_string()))
        }

        async fn list_sns_canisters(
            &self,
            root_canister_id: Principal,
        ) -> Result<ListSnsCanistersResponse, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(TestCall::ListSnsCanisters(root_canister_id));
            self.root_lists
                .get(&root_canister_id)
                .cloned()
                .unwrap_or(Err("missing root list"))
                .map_err(|err| ClientError::Call(err.to_string()))
        }

        async fn sns_root_cycles(
            &self,
            root_canister_id: Principal,
            target_canister_id: Principal,
        ) -> Result<u128, ClientError> {
            self.calls.lock().unwrap().push(TestCall::SnsRootStatus {
                root: root_canister_id,
                target: target_canister_id,
            });
            response_to_result(self.sns_root.get(&root_canister_id).cloned())
        }

        async fn sns_swap_cycles(&self, swap_canister_id: Principal) -> Result<u128, ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(TestCall::SnsSwapStatus(swap_canister_id));
            response_to_result(self.sns_swap.get(&swap_canister_id).cloned())
        }
    }

    fn response_to_result(response: Option<TestResponse>) -> Result<u128, ClientError> {
        match response.unwrap_or_default() {
            TestResponse::Ok(cycles) => Ok(cycles),
            TestResponse::Err(message) => Err(ClientError::Call(message.to_string())),
        }
    }

    fn blackhole_call(probe: Principal, target: Principal) -> TestCall {
        TestCall::Blackhole { probe, target }
    }

    fn root_status(root: Principal, target: Principal) -> TestCall {
        TestCall::SnsRootStatus { root, target }
    }

    fn relay_client<'a>(
        inner: &'a RecordingClient,
        self_id: Principal,
        self_cycles: u128,
    ) -> RelayCyclesProbeClient<'a, RecordingClient> {
        RelayCyclesProbeClient::new(inner, self_id, self_cycles)
    }

    fn probe_one(
        target: Principal,
        policy: CyclesProbePolicy,
        cached: Option<CyclesProbeRoute>,
        client: &RecordingClient,
    ) -> RelayProbeBatch {
        let cached_routes = cached
            .map(|route| BTreeMap::from([(target, route)]))
            .unwrap_or_default();
        block_on(probe_cycles_batch(
            &[target],
            &policy,
            cached_routes,
            123,
            &relay_client(client, principal("u2qkp-aqaaa-aaaar-qb7ea-cai"), 9_000),
        ))
    }

    #[test]
    fn direct_relay_self_balance_uses_no_external_routes() {
        let self_id = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        let client = RecordingClient::default();
        let batch = block_on(probe_cycles_batch(
            &[self_id],
            &CyclesProbePolicy::Auto,
            BTreeMap::new(),
            123,
            &relay_client(&client, self_id, 55),
        ));

        assert_eq!(batch.failures, Vec::new());
        assert_eq!(batch.snapshots[&self_id].cycles, 55);
        assert_eq!(
            batch.snapshots[&self_id].source,
            CyclesSampleSource::SelfCanister
        );
        assert_eq!(batch.route_updates[&self_id], None);
        assert_eq!(client.calls(), Vec::new());
    }

    #[test]
    fn auto_13_node_success_caches_13_node_route() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([(thirteen, TestResponse::Ok(77))]),
            ..Default::default()
        };

        let batch = probe_one(target, CyclesProbePolicy::Auto, None, &client);

        assert_eq!(batch.snapshots[&target].cycles, 77);
        assert_eq!(
            batch.snapshots[&target].source,
            CyclesSampleSource::BlackholeStatus
        );
        assert_eq!(
            batch.route_updates[&target],
            Some(CyclesProbeRoute::Blackhole {
                canister_id: thirteen
            })
        );
        assert_eq!(client.calls(), vec![blackhole_call(thirteen, target)]);
    }

    #[test]
    fn auto_fiduciary_fallback_caches_fiduciary_route() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Ok(88)),
            ]),
            ..Default::default()
        };

        let batch = probe_one(target, CyclesProbePolicy::Auto, None, &client);

        assert_eq!(batch.snapshots[&target].cycles, 88);
        assert_eq!(
            batch.route_updates[&target],
            Some(CyclesProbeRoute::Blackhole {
                canister_id: fiduciary
            })
        );
        assert_eq!(
            client.calls(),
            vec![
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
            ]
        );
    }

    #[test]
    fn fixed_custom_blackhole_uses_only_configured_route_and_removes_old_cache() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let fixed = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let stale_root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let client = RecordingClient {
            blackhole: BTreeMap::from([(fixed, TestResponse::Ok(42))]),
            ..Default::default()
        };

        let batch = probe_one(
            target,
            CyclesProbePolicy::FixedBlackhole { canister_id: fixed },
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: stale_root,
            }),
            &client,
        );

        assert_eq!(batch.snapshots[&target].cycles, 42);
        assert_eq!(
            batch.route_updates[&target],
            Some(CyclesProbeRoute::Blackhole { canister_id: fixed })
        );
        assert_eq!(client.calls(), vec![blackhole_call(fixed, target)]);
    }

    #[test]
    fn fixed_fiduciary_canonical_targets_probe_each_blackhole_through_itself() {
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let managed_targets = constants::ordered_production_blackhole_canister_ids();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (fiduciary, TestResponse::Ok(77)),
                (thirteen, TestResponse::Ok(13)),
            ]),
            ..Default::default()
        };

        let batch = block_on(probe_cycles_batch(
            &managed_targets,
            &CyclesProbePolicy::FixedBlackhole {
                canister_id: fiduciary,
            },
            BTreeMap::new(),
            123,
            &relay_client(&client, principal("u2qkp-aqaaa-aaaar-qb7ea-cai"), 9_000),
        ));

        assert!(batch.failures.is_empty());
        assert_eq!(batch.snapshots[&thirteen].cycles, 13);
        assert_eq!(batch.snapshots[&fiduciary].cycles, 77);
        assert_eq!(
            batch.route_updates[&thirteen],
            Some(CyclesProbeRoute::Blackhole {
                canister_id: thirteen
            })
        );
        assert_eq!(
            batch.route_updates[&fiduciary],
            Some(CyclesProbeRoute::Blackhole {
                canister_id: fiduciary
            })
        );
        assert_eq!(
            client.calls(),
            vec![
                blackhole_call(thirteen, thirteen),
                blackhole_call(fiduciary, fiduciary),
            ]
        );
    }

    #[test]
    fn cached_route_success_stops_without_fallback_calls() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let cached = CyclesProbeRoute::SnsRoot {
            root_canister_id: root,
        };
        let client = RecordingClient {
            sns_root: BTreeMap::from([(root, TestResponse::Ok(99))]),
            ..Default::default()
        };

        let batch = probe_one(
            target,
            CyclesProbePolicy::Auto,
            Some(cached.clone()),
            &client,
        );

        assert_eq!(batch.snapshots[&target].cycles, 99);
        assert_eq!(
            batch.snapshots[&target].source,
            CyclesSampleSource::SnsRootStatus
        );
        assert_eq!(batch.route_updates[&target], Some(cached));
        assert_eq!(client.calls(), vec![root_status(root, target)]);
    }

    #[test]
    fn stale_cached_sns_root_then_13_node_success_replaces_cache_without_failure() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([(thirteen, TestResponse::Ok(77))]),
            sns_root: BTreeMap::from([(root, TestResponse::Err("stale root"))]),
            ..Default::default()
        };

        let batch = probe_one(
            target,
            CyclesProbePolicy::Auto,
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root,
            }),
            &client,
        );

        assert!(batch.failures.is_empty());
        assert_eq!(
            batch.route_updates[&target],
            Some(CyclesProbeRoute::Blackhole {
                canister_id: thirteen
            })
        );
        assert_eq!(
            client.calls(),
            vec![root_status(root, target), blackhole_call(thirteen, target)]
        );
    }

    #[test]
    fn both_blackholes_fail_and_sns_root_route_succeeds() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![jupiter_ic_clients::sns::DeployedSns {
                    root_canister_id: Some(root),
                    governance_canister_id: Some(target),
                    ..Default::default()
                }],
            }),
            sns_root: BTreeMap::from([(root, TestResponse::Ok(123))]),
            ..Default::default()
        };

        let batch = probe_one(target, CyclesProbePolicy::Auto, None, &client);

        assert_eq!(
            batch.snapshots[&target].source,
            CyclesSampleSource::SnsRootStatus
        );
        assert_eq!(
            batch.route_updates[&target],
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root
            })
        );
        assert_eq!(
            client.calls(),
            vec![
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                root_status(root, target),
            ]
        );
    }

    #[test]
    fn framework_swap_success_maps_to_swap_source_and_cache() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![jupiter_ic_clients::sns::DeployedSns {
                    root_canister_id: Some(root),
                    swap_canister_id: Some(target),
                    ..Default::default()
                }],
            }),
            sns_swap: BTreeMap::from([(target, TestResponse::Ok(202))]),
            ..Default::default()
        };

        let batch = probe_one(target, CyclesProbePolicy::Auto, None, &client);

        assert_eq!(
            batch.snapshots[&target].source,
            CyclesSampleSource::SnsSwapStatus
        );
        assert_eq!(
            batch.route_updates[&target],
            Some(CyclesProbeRoute::SnsSwap {
                root_canister_id: root,
                swap_canister_id: target,
            })
        );
        assert_eq!(
            client.calls(),
            vec![
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                TestCall::SnsSwapStatus(target),
            ]
        );
    }

    #[test]
    fn final_failure_removes_old_cache_and_records_probe_failure() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            sns_root: BTreeMap::from([(root, TestResponse::Err("stale root"))]),
            ..Default::default()
        };

        let batch = probe_one(
            target,
            CyclesProbePolicy::Auto,
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root,
            }),
            &client,
        );

        assert_eq!(batch.snapshots.len(), 0);
        assert_eq!(batch.route_updates[&target], None);
        assert_eq!(batch.failures.len(), 1);
        assert_eq!(batch.failures[0].canister_id, target);
    }

    #[test]
    fn multiple_targets_keep_successes_when_one_target_fails() {
        let failed = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let healthy = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let client = RecordingClient::default();
        let batch = block_on(probe_cycles_batch(
            &[failed, healthy],
            &CyclesProbePolicy::Auto,
            BTreeMap::from([(
                failed,
                CyclesProbeRoute::SnsRoot {
                    root_canister_id: principal("r7inp-6aaaa-aaaaa-aaabq-cai"),
                },
            )]),
            123,
            &relay_client(&client, healthy, 9_000),
        ));

        assert!(batch.snapshots.contains_key(&healthy));
        assert!(!batch.snapshots.contains_key(&failed));
        assert_eq!(batch.failures.len(), 1);
        assert_eq!(batch.failures[0].canister_id, failed);
    }
}
