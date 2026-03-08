// Placeholder canister reserved for the faucet principal and default account until payout and cycles top-up logic lands.

#[ic_cdk::init]
fn init() {}

#[ic_cdk::post_upgrade]
fn post_upgrade() {}

ic_cdk::export_candid!();
