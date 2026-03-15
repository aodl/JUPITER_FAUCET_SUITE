use candid::{CandidType, Deserialize, Principal};
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DeployedSns {
    root_canister_id: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListDeployedSnsesResponse {
    instances: Vec<DeployedSns>,
}

thread_local! {
    static ROOTS: RefCell<Vec<Principal>> = RefCell::new(Vec::new());
}

#[ic_cdk::init]
fn init() {}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct Args {}

#[ic_cdk::update]
fn list_deployed_snses(_: Args) -> ListDeployedSnsesResponse {
    ROOTS.with(|r| ListDeployedSnsesResponse {
        instances: r.borrow().iter().copied().map(|root_canister_id| DeployedSns { root_canister_id: Some(root_canister_id) }).collect(),
    })
}

#[ic_cdk::update]
fn debug_reset() { ROOTS.with(|r| r.borrow_mut().clear()); }

#[ic_cdk::update]
fn debug_set_roots(roots: Vec<Principal>) { ROOTS.with(|r| *r.borrow_mut() = roots); }
