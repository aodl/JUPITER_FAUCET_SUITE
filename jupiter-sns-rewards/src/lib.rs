// Placeholder canister used to reserve the SNS rewards principal and default canister account until the staking distribution logic is implemented.

#[ic_cdk::init]
fn init() {}

#[ic_cdk::post_upgrade]
fn post_upgrade() {}

ic_cdk::export_candid!();
