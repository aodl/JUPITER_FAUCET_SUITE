use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListSnsCanistersResponse {
    root: Option<Principal>,
    governance: Option<Principal>,
    ledger: Option<Principal>,
    swap: Option<Principal>,
    index: Option<Principal>,
    dapps: Vec<Principal>,
    archives: Vec<Principal>,
    extensions: Option<SnsExtensions>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct SnsExtensions {
    extension_canister_ids: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListSnsCanistersRequest {}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusArgs {
    canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusResult {
    cycles: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugCall {
    method: String,
    canister_id: Option<Principal>,
    caller: Principal,
}

thread_local! {
    static CANISTERS: RefCell<ListSnsCanistersResponse> = RefCell::new(ListSnsCanistersResponse::default());
    static CALLS: RefCell<Vec<DebugCall>> = const { RefCell::new(Vec::new()) };
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn list_sns_canisters(_: ListSnsCanistersRequest) -> ListSnsCanistersResponse {
    CALLS.with(|calls| {
        calls.borrow_mut().push(DebugCall {
            method: "list_sns_canisters".to_string(),
            canister_id: None,
            caller: ic_cdk::api::msg_caller(),
        });
    });
    CANISTERS.with(|s| s.borrow().clone())
}

#[ic_cdk::update]
async fn canister_status(args: CanisterStatusArgs) -> CanisterStatusResult {
    CALLS.with(|calls| {
        calls.borrow_mut().push(DebugCall {
            method: "canister_status".to_string(),
            canister_id: Some(args.canister_id),
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

#[ic_cdk::update]
fn debug_set_canisters(canisters: ListSnsCanistersResponse) {
    CANISTERS.with(|s| *s.borrow_mut() = canisters);
}

#[ic_cdk::query]
fn debug_calls() -> Vec<DebugCall> {
    CALLS.with(|calls| calls.borrow().clone())
}

#[ic_cdk::update]
fn debug_reset_calls() {
    CALLS.with(|calls| calls.borrow_mut().clear());
}
