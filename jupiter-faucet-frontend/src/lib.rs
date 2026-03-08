// Placeholder canister reserved for the frontend principal and deployment slot until the asset canister is implemented.

#[ic_cdk::init]
fn init() {}

#[ic_cdk::post_upgrade]
fn post_upgrade() {}

ic_cdk::export_candid!();
