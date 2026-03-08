// Placeholder canister used to reserve the faucet principal and default canister account until the payout and cycles top-up logic is implemented.

#[ic_cdk::init]
fn init() {}

#[ic_cdk::post_upgrade]
fn post_upgrade() {}

ic_cdk::export_candid!();
