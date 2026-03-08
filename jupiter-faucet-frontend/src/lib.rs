// Placeholder canister used to reserve the frontend principal and deployment slot until the asset-serving frontend canister is implemented.

#[ic_cdk::init]
fn init() {}

#[ic_cdk::post_upgrade]
fn post_upgrade() {}

ic_cdk::export_candid!();
