use candid::{CandidType, Deserialize, Principal};
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsCanisterStatus {
    cycles: Option<candid::Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsCanisterSummary {
    canister_id: Option<Principal>,
    status: Option<SnsCanisterStatus>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct GetSnsCanistersSummaryResponse {
    root: Option<SnsCanisterSummary>,
    governance: Option<SnsCanisterSummary>,
    ledger: Option<SnsCanisterSummary>,
    swap: Option<SnsCanisterSummary>,
    index: Option<SnsCanisterSummary>,
    dapps: Vec<SnsCanisterSummary>,
    archives: Vec<SnsCanisterSummary>,
}

thread_local! {
    static SUMMARY: RefCell<GetSnsCanistersSummaryResponse> = RefCell::new(GetSnsCanistersSummaryResponse::default());
}

#[ic_cdk::init]
fn init() {}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct Args { update_canister_list: Option<bool> }

#[ic_cdk::update]
fn get_sns_canisters_summary(_: Args) -> GetSnsCanistersSummaryResponse {
    SUMMARY.with(|s| s.borrow().clone())
}

#[ic_cdk::update]
fn debug_set_summary(summary: GetSnsCanistersSummaryResponse) {
    SUMMARY.with(|s| *s.borrow_mut() = summary);
}
