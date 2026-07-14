use std::collections::BTreeSet;

use candid::{Nat, Principal};

use crate::management::{self, CanisterInfoArgs};
use crate::sns::{
    DeployedSns, ListDeployedSnsesResponse, ListSnsCanistersResponse, SnsRootCanister,
    SnsSwapCanister, SnsWasmCanister,
};
use crate::{constants, ClientError};

#[derive(
    candid::CandidType,
    serde::Deserialize,
    serde::Serialize,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub enum CyclesProbeRoute {
    Blackhole {
        canister_id: Principal,
    },
    SnsRoot {
        root_canister_id: Principal,
    },
    SnsSwap {
        root_canister_id: Principal,
        swap_canister_id: Principal,
    },
}

#[derive(candid::CandidType, serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq, Eq)]
pub enum CyclesProbePolicy {
    FixedBlackhole { canister_id: Principal },
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CyclesProbeSuccess {
    pub cycles: u128,
    pub route: Option<CyclesProbeRoute>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CyclesProbeFailure {
    pub message: String,
    pub attempted_routes: Vec<CyclesProbeRoute>,
}

pub type CyclesProbeResult = Result<CyclesProbeSuccess, CyclesProbeFailure>;

#[allow(async_fn_in_trait)]
pub trait CyclesProbeClient {
    async fn self_cycles(&self, target: Principal) -> Option<u128>;
    async fn blackhole_cycles(
        &self,
        probe_canister_id: Principal,
        target_canister_id: Principal,
    ) -> Result<u128, ClientError>;
    async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError>;
    async fn canister_info_controllers(
        &self,
        target: Principal,
    ) -> Result<Vec<Principal>, ClientError>;
    async fn list_sns_canisters(
        &self,
        root_canister_id: Principal,
    ) -> Result<ListSnsCanistersResponse, ClientError>;
    async fn sns_root_cycles(
        &self,
        root_canister_id: Principal,
        target_canister_id: Principal,
    ) -> Result<u128, ClientError>;
    async fn sns_swap_cycles(&self, swap_canister_id: Principal) -> Result<u128, ClientError>;
}

pub struct IcCyclesProbeClient {
    sns_wasm: SnsWasmCanister,
    sns_root: SnsRootCanister,
    sns_swap: SnsSwapCanister,
}

impl IcCyclesProbeClient {
    pub fn new(sns_wasm_canister_id: Principal) -> Self {
        Self {
            sns_wasm: SnsWasmCanister::new(sns_wasm_canister_id),
            sns_root: SnsRootCanister,
            sns_swap: SnsSwapCanister,
        }
    }
}

#[allow(async_fn_in_trait)]
impl CyclesProbeClient for IcCyclesProbeClient {
    async fn self_cycles(&self, target: Principal) -> Option<u128> {
        (target == ic_cdk::api::canister_self()).then(ic_cdk::api::canister_cycle_balance)
    }

    async fn blackhole_cycles(
        &self,
        probe_canister_id: Principal,
        target_canister_id: Principal,
    ) -> Result<u128, ClientError> {
        let resp = ic_cdk::call::Call::bounded_wait(probe_canister_id, "canister_status")
            .with_arg(BlackholeCanisterStatusArgs {
                canister_id: target_canister_id,
            })
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("blackhole canister_status failed: {e:?}")))?;
        let status: BlackholeCanisterStatus = resp.candid().map_err(|e| {
            ClientError::Call(format!("decode blackhole canister_status failed: {e:?}"))
        })?;
        nat_to_u128(&status.cycles)
    }

    async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError> {
        self.sns_wasm.list_deployed_snses().await
    }

    async fn canister_info_controllers(
        &self,
        target: Principal,
    ) -> Result<Vec<Principal>, ClientError> {
        let info = management::canister_info(&CanisterInfoArgs {
            canister_id: target,
            num_requested_changes: None,
        })
        .await
        .map_err(|e| ClientError::Call(format!("canister_info failed: {e:?}")))?;
        Ok(info.controllers)
    }

    async fn list_sns_canisters(
        &self,
        root_canister_id: Principal,
    ) -> Result<ListSnsCanistersResponse, ClientError> {
        self.sns_root.list_sns_canisters(root_canister_id).await
    }

    async fn sns_root_cycles(
        &self,
        root_canister_id: Principal,
        target_canister_id: Principal,
    ) -> Result<u128, ClientError> {
        let status = self
            .sns_root
            .canister_status(root_canister_id, target_canister_id)
            .await?;
        nat_to_u128(&status.cycles)
    }

    async fn sns_swap_cycles(&self, swap_canister_id: Principal) -> Result<u128, ClientError> {
        let status = self.sns_swap.get_canister_status(swap_canister_id).await?;
        nat_to_u128(&status.cycles)
    }
}

#[derive(candid::CandidType, serde::Deserialize)]
struct BlackholeCanisterStatusArgs {
    canister_id: Principal,
}

#[derive(candid::CandidType, serde::Deserialize)]
struct BlackholeCanisterStatus {
    cycles: Nat,
}

fn nat_to_u128(n: &Nat) -> Result<u128, ClientError> {
    u128::try_from(n.0.clone())
        .map_err(|_| ClientError::Convert(format!("Nat does not fit u128: {n}")))
}

pub async fn probe_cycles<C: CyclesProbeClient>(
    policy: &CyclesProbePolicy,
    target: Principal,
    cached_route: Option<CyclesProbeRoute>,
    client: &C,
) -> CyclesProbeResult {
    let mut state = ProbeState::default();

    if let Some(cycles) = client.self_cycles(target).await {
        return Ok(state.success(cycles, None));
    }

    if constants::is_production_blackhole_canister_id(target) {
        match execute_route(
            &mut state,
            client,
            target,
            CyclesProbeRoute::Blackhole {
                canister_id: target,
            },
        )
        .await
        {
            Ok(Some(success)) => return Ok(success),
            Ok(None) => {}
            Err(err) => match policy {
                CyclesProbePolicy::FixedBlackhole { .. } => {
                    return state.failure(format!("canonical blackhole self-status failed: {err}"));
                }
                CyclesProbePolicy::Auto => state
                    .errors
                    .push(format!("canonical blackhole self-status failed: {err}")),
            },
        }
    }

    match policy {
        CyclesProbePolicy::FixedBlackhole { canister_id } => {
            return match execute_route(
                &mut state,
                client,
                target,
                CyclesProbeRoute::Blackhole {
                    canister_id: *canister_id,
                },
            )
            .await
            {
                Ok(Some(success)) => Ok(success),
                Ok(None) => {
                    state.failure("fixed blackhole route was already attempted".to_string())
                }
                Err(err) => state.failure(format!("fixed blackhole probe failed: {err}")),
            };
        }
        CyclesProbePolicy::Auto => {}
    }

    if let Some(route) = cached_route {
        match execute_route(&mut state, client, target, route).await {
            Ok(Some(success)) => return Ok(success),
            Ok(None) => {}
            Err(err) => {
                state.errors.push(format!("cached route failed: {err}"));
            }
        }
    }

    for canister_id in constants::ordered_production_blackhole_canister_ids() {
        match execute_route(
            &mut state,
            client,
            target,
            CyclesProbeRoute::Blackhole { canister_id },
        )
        .await
        {
            Ok(Some(success)) => return Ok(success),
            Ok(None) => continue,
            Err(err) => state
                .errors
                .push(format!("blackhole {} failed: {err}", canister_id.to_text())),
        }
    }

    match discover_sns_route(target, client).await {
        Ok(Some(route)) => match execute_route(&mut state, client, target, route).await {
            Ok(Some(success)) => Ok(success),
            Ok(None) => state.failure("no cycles probe route could observe target".to_string()),
            Err(err) => state.failure(format!("SNS route probe failed after discovery: {err}")),
        },
        Ok(None) => state.failure("no cycles probe route could observe target".to_string()),
        Err(err) => state.failure(format!("SNS route discovery failed: {err}")),
    }
}

async fn execute_route<C: CyclesProbeClient>(
    state: &mut ProbeState,
    client: &C,
    target: Principal,
    route: CyclesProbeRoute,
) -> Result<Option<CyclesProbeSuccess>, ClientError> {
    if !state.attempted_set.insert(route.clone()) {
        return Ok(None);
    }
    state.attempted_routes.push(route.clone());
    let cycles = match route {
        CyclesProbeRoute::Blackhole { canister_id } => {
            client.blackhole_cycles(canister_id, target).await?
        }
        CyclesProbeRoute::SnsRoot { root_canister_id } => {
            client.sns_root_cycles(root_canister_id, target).await?
        }
        CyclesProbeRoute::SnsSwap {
            root_canister_id: _,
            swap_canister_id,
        } => client.sns_swap_cycles(swap_canister_id).await?,
    };
    Ok(Some(state.success(cycles, Some(route))))
}

async fn discover_sns_route<C: CyclesProbeClient>(
    target: Principal,
    client: &C,
) -> Result<Option<CyclesProbeRoute>, ClientError> {
    let deployed = client.list_deployed_snses().await?;
    let mut roots = BTreeSet::new();
    for sns in &deployed.instances {
        let Some(root) = sns.root_canister_id else {
            continue;
        };
        roots.insert(root);
        if let Some(route) = framework_route_from_deployed_sns(target, sns) {
            return Ok(Some(route));
        }
    }

    let controllers = client.canister_info_controllers(target).await?;
    let candidate_roots = controllers
        .into_iter()
        .filter(|controller| roots.contains(controller))
        .collect::<BTreeSet<_>>();
    if candidate_roots.is_empty() {
        return Ok(None);
    }
    let mut candidate_errors = Vec::new();
    for candidate_root in candidate_roots {
        let list = match client.list_sns_canisters(candidate_root).await {
            Ok(list) => list,
            Err(err) => {
                candidate_errors.push(format!("{}: {err}", candidate_root.to_text()));
                continue;
            }
        };
        if let Some(route) = route_from_list_sns_canisters(candidate_root, target, &list) {
            return Ok(Some(route));
        }
    }

    if !candidate_errors.is_empty() {
        return Err(ClientError::Call(format!(
            "failed to query candidate SNS roots: {}",
            candidate_errors.join("; ")
        )));
    }

    Ok(None)
}

fn framework_route_from_deployed_sns(
    target: Principal,
    sns: &DeployedSns,
) -> Option<CyclesProbeRoute> {
    let root = sns.root_canister_id?;
    if [
        Some(root),
        sns.governance_canister_id,
        sns.ledger_canister_id,
        sns.index_canister_id,
    ]
    .contains(&Some(target))
    {
        return Some(CyclesProbeRoute::SnsRoot {
            root_canister_id: root,
        });
    }
    if sns.swap_canister_id == Some(target) {
        return Some(CyclesProbeRoute::SnsSwap {
            root_canister_id: root,
            swap_canister_id: target,
        });
    }
    None
}

fn route_from_list_sns_canisters(
    candidate_root: Principal,
    target: Principal,
    list: &ListSnsCanistersResponse,
) -> Option<CyclesProbeRoute> {
    if list.root != Some(candidate_root) {
        return None;
    }
    if [list.root, list.governance, list.ledger, list.index].contains(&Some(target)) {
        return Some(CyclesProbeRoute::SnsRoot {
            root_canister_id: candidate_root,
        });
    }
    if list.swap == Some(target) {
        return Some(CyclesProbeRoute::SnsSwap {
            root_canister_id: candidate_root,
            swap_canister_id: target,
        });
    }
    if list.dapps.contains(&target)
        || list.archives.contains(&target)
        || list
            .extensions
            .as_ref()
            .map(|extensions| extensions.extension_canister_ids.contains(&target))
            .unwrap_or(false)
    {
        return Some(CyclesProbeRoute::SnsRoot {
            root_canister_id: candidate_root,
        });
    }
    None
}

#[derive(Default)]
struct ProbeState {
    attempted_routes: Vec<CyclesProbeRoute>,
    attempted_set: BTreeSet<CyclesProbeRoute>,
    errors: Vec<String>,
}

impl ProbeState {
    fn success(&self, cycles: u128, route: Option<CyclesProbeRoute>) -> CyclesProbeSuccess {
        CyclesProbeSuccess { cycles, route }
    }

    fn failure(&self, message: String) -> CyclesProbeResult {
        let detail = if self.errors.is_empty() {
            message
        } else {
            format!("{message}; previous errors: {}", self.errors.join("; "))
        };
        Err(CyclesProbeFailure {
            message: detail,
            attempted_routes: self.attempted_routes.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sns::SnsExtensions;
    use std::collections::BTreeMap;
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
        SelfCycles(Principal),
        Blackhole { probe: Principal, target: Principal },
        ListDeployedSnses,
        CanisterInfo(Principal),
        ListSnsCanisters(Principal),
        SnsRootStatus { root: Principal, target: Principal },
        SnsSwapStatus(Principal),
    }

    struct RecordingClient {
        self_cycles: Option<(Principal, u128)>,
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
                self_cycles: None,
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
        async fn self_cycles(&self, target: Principal) -> Option<u128> {
            self.calls
                .lock()
                .unwrap()
                .push(TestCall::SelfCycles(target));
            self.self_cycles
                .filter(|(self_target, _)| *self_target == target)
                .map(|(_, cycles)| cycles)
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

    fn probe_auto(
        target: Principal,
        cached_route: Option<CyclesProbeRoute>,
        client: &RecordingClient,
    ) -> CyclesProbeResult {
        block_on(probe_cycles(
            &CyclesProbePolicy::Auto,
            target,
            cached_route,
            client,
        ))
    }

    fn probe_fixed(
        target: Principal,
        fixed: Principal,
        client: &RecordingClient,
    ) -> CyclesProbeResult {
        block_on(probe_cycles(
            &CyclesProbePolicy::FixedBlackhole { canister_id: fixed },
            target,
            None,
            client,
        ))
    }

    fn deployed(root: Principal) -> crate::sns::DeployedSns {
        crate::sns::DeployedSns {
            root_canister_id: Some(root),
            ..Default::default()
        }
    }

    fn blackhole_call(probe: Principal, target: Principal) -> TestCall {
        TestCall::Blackhole { probe, target }
    }

    fn root_status(root: Principal, target: Principal) -> TestCall {
        TestCall::SnsRootStatus { root, target }
    }

    fn auto_discovery_client(
        _target: Principal,
        root: Principal,
        list: ListSnsCanistersResponse,
    ) -> RecordingClient {
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(root)],
            }),
            controllers: Ok(vec![root]),
            root_lists: BTreeMap::from([(root, Ok(list))]),
            sns_root: BTreeMap::from([(root, TestResponse::Ok(123))]),
            ..Default::default()
        }
    }

    #[test]
    fn direct_self_cycles_makes_no_external_calls() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let client = RecordingClient {
            self_cycles: Some((target, 55)),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(
            outcome,
            CyclesProbeSuccess {
                cycles: 55,
                route: None,
            }
        );
        assert_eq!(client.calls(), vec![TestCall::SelfCycles(target)]);
    }

    #[test]
    fn fixed_success_calls_only_supplied_blackhole() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let fixed = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let client = RecordingClient {
            blackhole: BTreeMap::from([(fixed, TestResponse::Ok(42))]),
            ..Default::default()
        };

        let outcome = block_on(probe_cycles(
            &CyclesProbePolicy::FixedBlackhole { canister_id: fixed },
            target,
            None,
            &client,
        ));

        assert_eq!(
            outcome,
            Ok(CyclesProbeSuccess {
                cycles: 42,
                route: Some(CyclesProbeRoute::Blackhole { canister_id: fixed }),
            })
        );
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(fixed, target),]
        );
    }

    #[test]
    fn fixed_failure_performs_no_fallback_or_sns_discovery() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let fixed = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let client = RecordingClient {
            blackhole: BTreeMap::from([(fixed, TestResponse::Err("not controller"))]),
            ..Default::default()
        };

        let outcome = block_on(probe_cycles(
            &CyclesProbePolicy::FixedBlackhole { canister_id: fixed },
            target,
            None,
            &client,
        ));

        assert!(outcome.is_err());
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(fixed, target),]
        );
    }

    #[test]
    fn auto_13_node_success_stops_immediately() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([(thirteen, TestResponse::Ok(77))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 77);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
            ]
        );
    }

    #[test]
    fn auto_fiduciary_fallback_calls_13_node_then_fiduciary_and_no_sns() {
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

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 88);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
            ]
        );
    }

    #[test]
    fn canonical_blackhole_target_uses_self_status_before_cached_route() {
        let target = constants::fiduciary_blackhole_canister_id();
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([(target, TestResponse::Ok(77))]),
            ..Default::default()
        };

        let outcome = probe_auto(
            target,
            Some(CyclesProbeRoute::Blackhole {
                canister_id: thirteen,
            }),
            &client,
        )
        .unwrap();

        assert_eq!(outcome.cycles, 77);
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(target, target),]
        );
    }

    #[test]
    fn fixed_fiduciary_targeting_13_node_uses_13_node_self_status_only() {
        let target = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([(target, TestResponse::Ok(1313))]),
            ..Default::default()
        };

        let outcome = probe_fixed(target, fiduciary, &client).unwrap();

        assert_eq!(outcome.cycles, 1313);
        assert_eq!(
            outcome.route,
            Some(CyclesProbeRoute::Blackhole {
                canister_id: target
            })
        );
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(target, target),]
        );
    }

    #[test]
    fn fixed_13_node_targeting_fiduciary_uses_fiduciary_self_status_only() {
        let target = constants::fiduciary_blackhole_canister_id();
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([(target, TestResponse::Ok(777))]),
            ..Default::default()
        };

        let outcome = probe_fixed(target, thirteen, &client).unwrap();

        assert_eq!(outcome.cycles, 777);
        assert_eq!(
            outcome.route,
            Some(CyclesProbeRoute::Blackhole {
                canister_id: target
            })
        );
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(target, target),]
        );
    }

    #[test]
    fn fixed_custom_blackhole_ordinary_target_uses_only_custom_route() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let custom = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let client = RecordingClient {
            blackhole: BTreeMap::from([(custom, TestResponse::Ok(42))]),
            ..Default::default()
        };

        let outcome = probe_fixed(target, custom, &client).unwrap();

        assert_eq!(outcome.cycles, 42);
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(custom, target),]
        );
    }

    #[test]
    fn fixed_canonical_self_status_failure_does_not_fall_back_to_configured_blackhole() {
        let target = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (target, TestResponse::Err("self-status unavailable")),
                (fiduciary, TestResponse::Ok(777)),
            ]),
            ..Default::default()
        };

        let outcome = probe_fixed(target, fiduciary, &client).unwrap_err();

        assert!(outcome
            .message
            .contains("canonical blackhole self-status failed"));
        assert_eq!(
            outcome.attempted_routes,
            vec![CyclesProbeRoute::Blackhole {
                canister_id: target
            }]
        );
        assert_eq!(
            client.calls(),
            vec![TestCall::SelfCycles(target), blackhole_call(target, target),]
        );
    }

    #[test]
    fn auto_canonical_self_status_failure_continues_without_duplicate_target_call() {
        let target = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (target, TestResponse::Err("not readable through itself")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(root)],
            }),
            controllers: Ok(vec![root]),
            root_lists: BTreeMap::from([(
                root,
                Ok(ListSnsCanistersResponse {
                    root: Some(root),
                    dapps: vec![target],
                    ..Default::default()
                }),
            )]),
            sns_root: BTreeMap::from([(root, TestResponse::Ok(313))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 313);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(target, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                TestCall::CanisterInfo(target),
                TestCall::ListSnsCanisters(root),
                root_status(root, target),
            ]
        );
        assert_eq!(
            client
                .calls()
                .into_iter()
                .filter(|call| *call == blackhole_call(target, target))
                .count(),
            1
        );
    }

    #[test]
    fn cached_route_success_stops_immediately() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let cached_root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let cached = CyclesProbeRoute::SnsRoot {
            root_canister_id: cached_root,
        };
        let client = RecordingClient {
            sns_root: BTreeMap::from([(cached_root, TestResponse::Ok(99))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, Some(cached.clone()), &client).unwrap();

        assert_eq!(
            outcome,
            CyclesProbeSuccess {
                cycles: 99,
                route: Some(cached),
            }
        );
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                root_status(cached_root, target),
            ]
        );
    }

    #[test]
    fn failed_cached_blackhole_route_is_not_called_twice() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let cached = CyclesProbeRoute::Blackhole {
            canister_id: thirteen,
        };
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("stale")),
                (fiduciary, TestResponse::Ok(88)),
            ]),
            ..Default::default()
        };

        let outcome = block_on(probe_cycles(
            &CyclesProbePolicy::Auto,
            target,
            Some(cached.clone()),
            &client,
        ));

        assert!(outcome.is_ok());
        assert_eq!(
            client
                .calls()
                .into_iter()
                .filter(|call| *call == blackhole_call(thirteen, target))
                .count(),
            1
        );
    }

    #[test]
    fn framework_root_governance_ledger_index_resolve_from_sns_w_without_canister_info() {
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
                instances: vec![crate::sns::DeployedSns {
                    root_canister_id: Some(root),
                    governance_canister_id: Some(target),
                    ledger_canister_id: Some(principal("77deu-baaaa-aaaar-qb6za-cai")),
                    index_canister_id: Some(principal("e3mmv-5qaaa-aaaah-aadma-cai")),
                    ..Default::default()
                }],
            }),
            sns_root: BTreeMap::from([(root, TestResponse::Ok(101))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 101);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                root_status(root, target),
            ]
        );
    }

    #[test]
    fn framework_swap_resolves_from_sns_w_and_uses_swap_get_canister_status() {
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
                instances: vec![crate::sns::DeployedSns {
                    root_canister_id: Some(root),
                    swap_canister_id: Some(target),
                    ..Default::default()
                }],
            }),
            sns_swap: BTreeMap::from([(target, TestResponse::Ok(202))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 202);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                TestCall::SnsSwapStatus(target),
            ]
        );
    }

    #[test]
    fn dapp_discovery_follows_blackholes_sns_w_canister_info_root_list_root_status() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = auto_discovery_client(
            target,
            root,
            ListSnsCanistersResponse {
                root: Some(root),
                dapps: vec![target],
                ..Default::default()
            },
        );

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 123);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                TestCall::CanisterInfo(target),
                TestCall::ListSnsCanisters(root),
                root_status(root, target),
            ]
        );
    }

    #[test]
    fn fake_controller_absent_from_sns_w_is_never_called() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let official_root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let fake_root = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(official_root)],
            }),
            controllers: Ok(vec![fake_root]),
            root_lists: BTreeMap::from([(fake_root, Err("must not call fake"))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client);

        assert!(outcome.is_err());
        assert!(!client
            .calls()
            .contains(&TestCall::ListSnsCanisters(fake_root)));
    }

    #[test]
    fn candidate_root_response_with_root_not_candidate_is_rejected() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let candidate_root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let other_root = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let client = auto_discovery_client(
            target,
            candidate_root,
            ListSnsCanistersResponse {
                root: Some(other_root),
                dapps: vec![target],
                ..Default::default()
            },
        );

        let outcome = probe_auto(target, None, &client);

        assert!(outcome.is_err());
        assert!(!client
            .calls()
            .contains(&root_status(candidate_root, target)));
    }

    #[test]
    fn archive_membership_resolves() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let client = auto_discovery_client(
            target,
            root,
            ListSnsCanistersResponse {
                root: Some(root),
                archives: vec![target],
                ..Default::default()
            },
        );

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(
            outcome.route,
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root
            })
        );
    }

    #[test]
    fn extension_membership_resolves() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let client = auto_discovery_client(
            target,
            root,
            ListSnsCanistersResponse {
                root: Some(root),
                extensions: Some(SnsExtensions {
                    extension_canister_ids: vec![target],
                }),
                ..Default::default()
            },
        );

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(
            outcome.route,
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root
            })
        );
    }

    #[test]
    fn first_official_candidate_failure_followed_by_second_candidate_success() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root_a = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let root_b = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(root_a), deployed(root_b)],
            }),
            controllers: Ok(vec![root_a, root_b]),
            root_lists: BTreeMap::from([
                (root_a, Err("temporarily unavailable")),
                (
                    root_b,
                    Ok(ListSnsCanistersResponse {
                        root: Some(root_b),
                        dapps: vec![target],
                        ..Default::default()
                    }),
                ),
            ]),
            sns_root: BTreeMap::from([(root_b, TestResponse::Ok(456))]),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client).unwrap();

        assert_eq!(outcome.cycles, 456);
        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                TestCall::CanisterInfo(target),
                TestCall::ListSnsCanisters(root_a),
                TestCall::ListSnsCanisters(root_b),
                root_status(root_b, target),
            ]
        );
    }

    #[test]
    fn failed_cached_sns_route_is_not_executed_twice_during_rediscovery() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let cached = CyclesProbeRoute::SnsRoot {
            root_canister_id: root,
        };
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            sns_root: BTreeMap::from([(root, TestResponse::Err("stale root status"))]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![crate::sns::DeployedSns {
                    root_canister_id: Some(root),
                    governance_canister_id: Some(target),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let outcome = probe_auto(target, Some(cached), &client);

        assert!(outcome.is_err());
        assert_eq!(
            client
                .calls()
                .into_iter()
                .filter(|call| *call == root_status(root, target))
                .count(),
            1
        );
        let message = outcome.unwrap_err().message;
        assert!(message.contains("cached route failed"));
        assert!(message.contains("stale root status"));
        assert!(!message.contains("route already attempted"));
    }

    #[test]
    fn sns_w_response_with_only_root_canister_id_decodes() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let client = RecordingClient {
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(root)],
            }),
            controllers: Ok(Vec::new()),
            ..Default::default()
        };

        let outcome = probe_auto(target, None, &client);

        assert!(outcome.is_err());
        assert!(client.calls().contains(&TestCall::CanisterInfo(target)));
    }

    #[test]
    fn list_sns_canisters_with_missing_extensions_decodes() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let list = ListSnsCanistersResponse {
            root: Some(root),
            dapps: vec![target],
            extensions: None,
            ..Default::default()
        };

        assert_eq!(
            route_from_list_sns_canisters(root, target, &list),
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root,
            })
        );
    }

    #[test]
    fn list_sns_canisters_with_populated_optional_extensions_decodes() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let list = ListSnsCanistersResponse {
            root: Some(root),
            extensions: Some(SnsExtensions {
                extension_canister_ids: vec![target],
            }),
            ..Default::default()
        };

        assert_eq!(
            route_from_list_sns_canisters(root, target, &list),
            Some(CyclesProbeRoute::SnsRoot {
                root_canister_id: root,
            })
        );
    }

    #[test]
    fn all_candidate_root_failures_are_preserved() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root_a = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let root_b = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(root_a), deployed(root_b)],
            }),
            controllers: Ok(vec![root_a, root_b]),
            root_lists: BTreeMap::from([
                (root_a, Err("root a rejected")),
                (root_b, Err("root b rejected")),
            ]),
            ..Default::default()
        };

        let err = probe_auto(target, None, &client).unwrap_err();

        assert!(err.message.contains(&root_a.to_text()));
        assert!(err.message.contains("root a rejected"));
        assert!(err.message.contains(&root_b.to_text()));
        assert!(err.message.contains("root b rejected"));
    }

    #[test]
    fn one_official_candidate_nonmatching_and_another_failing_is_not_definitively_absent() {
        let target = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let root_a = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let root_b = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let unrelated = principal("77deu-baaaa-aaaar-qb6za-cai");
        let thirteen = constants::thirteen_node_blackhole_canister_id();
        let fiduciary = constants::fiduciary_blackhole_canister_id();
        let client = RecordingClient {
            blackhole: BTreeMap::from([
                (thirteen, TestResponse::Err("not controller")),
                (fiduciary, TestResponse::Err("not controller")),
            ]),
            deployed: Ok(ListDeployedSnsesResponse {
                instances: vec![deployed(root_a), deployed(root_b)],
            }),
            controllers: Ok(vec![root_a, root_b]),
            root_lists: BTreeMap::from([
                (
                    root_a,
                    Ok(ListSnsCanistersResponse {
                        root: Some(root_a),
                        dapps: vec![unrelated],
                        ..Default::default()
                    }),
                ),
                (root_b, Err("root b rejected membership query")),
            ]),
            ..Default::default()
        };

        let err = probe_auto(target, None, &client).unwrap_err();

        assert_eq!(
            client.calls(),
            vec![
                TestCall::SelfCycles(target),
                blackhole_call(thirteen, target),
                blackhole_call(fiduciary, target),
                TestCall::ListDeployedSnses,
                TestCall::CanisterInfo(target),
                TestCall::ListSnsCanisters(root_a),
                TestCall::ListSnsCanisters(root_b),
            ]
        );
        assert!(err.message.contains(&root_b.to_text()));
        assert!(err.message.contains("root b rejected membership query"));
    }
}
