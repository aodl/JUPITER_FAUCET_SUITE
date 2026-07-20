use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct BurnCyclesArgs {
    sink: Principal,
    amount: u128,
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
async fn burn_cycles(args: BurnCyclesArgs) {
    let _ = Call::bounded_wait(args.sink, "accept_cycles")
        .with_arg(())
        .with_cycles(args.amount)
        .await
        .unwrap_or_else(|err| ic_cdk::trap(format!("cycle sink call failed: {err:?}")));
}

#[ic_cdk::update]
fn accept_cycles() -> u128 {
    let available = ic_cdk::api::msg_cycles_available();
    ic_cdk::api::msg_cycles_accept(available)
}

#[ic_cdk::query]
fn cycle_balance() -> u128 {
    ic_cdk::api::canister_cycle_balance()
}
