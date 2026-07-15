use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusArgs {
    canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusResult {
    cycles: Nat,
    settings: CanisterStatusSettings,
    memory_size: Option<Nat>,
    memory_metrics: Option<CanisterStatusMemoryMetrics>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusSettings {
    controllers: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusMemoryMetrics {
    wasm_memory_size: Nat,
    stable_memory_size: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugCall {
    canister_id: Principal,
    caller: Principal,
}

thread_local! {
    static CALLS: RefCell<Vec<DebugCall>> = const { RefCell::new(Vec::new()) };
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
async fn canister_status(args: CanisterStatusArgs) -> CanisterStatusResult {
    CALLS.with(|calls| {
        calls.borrow_mut().push(DebugCall {
            canister_id: args.canister_id,
            caller: ic_cdk::api::msg_caller(),
        });
    });

    let resp = Call::bounded_wait(Principal::management_canister(), "canister_status")
        .with_arg(&args)
        .await
        .unwrap_or_else(|err| ic_cdk::trap(format!("management canister_status failed: {err:?}")));
    resp.candid()
        .unwrap_or_else(|err| ic_cdk::trap(format!("decode canister_status failed: {err:?}")))
}

#[ic_cdk::query]
fn debug_calls() -> Vec<DebugCall> {
    CALLS.with(|calls| calls.borrow().clone())
}

#[ic_cdk::update]
fn debug_reset() {
    CALLS.with(|calls| calls.borrow_mut().clear());
}
