use candid::{CandidType, Deserialize, Nat, Principal};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct BlackholeSettings {
    controllers: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct BlackholeCanisterStatus {
    cycles: Nat,
    settings: BlackholeSettings,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugStatus {
    canister_id: Principal,
    cycles: Nat,
    controllers: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct Args {
    canister_id: Principal,
}

thread_local! {
    static STATUSES: RefCell<BTreeMap<Principal, BlackholeCanisterStatus>> = RefCell::new(BTreeMap::new());
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn canister_status(args: Args) -> BlackholeCanisterStatus {
    STATUSES.with(|s| {
        s.borrow().get(&args.canister_id).cloned().unwrap_or_else(|| ic_cdk::trap("status not found"))
    })
}

#[ic_cdk::update]
fn debug_reset() {
    STATUSES.with(|s| s.borrow_mut().clear());
}

#[ic_cdk::update]
fn debug_set_status(canister_id: Principal, cycles: Option<Nat>, controllers: Vec<Principal>) {
    STATUSES.with(|s| {
        let mut st = s.borrow_mut();
        if let Some(cycles) = cycles {
            st.insert(canister_id, BlackholeCanisterStatus { cycles, settings: BlackholeSettings { controllers } });
        } else {
            st.remove(&canister_id);
        }
    });
}

#[ic_cdk::query]
fn debug_statuses() -> Vec<DebugStatus> {
    STATUSES.with(|s| {
        s.borrow().iter().map(|(canister_id, status)| DebugStatus {
            canister_id: *canister_id,
            cycles: status.cycles.clone(),
            controllers: status.settings.controllers.clone(),
        }).collect()
    })
}
