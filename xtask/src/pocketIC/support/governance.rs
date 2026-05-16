use anyhow::{anyhow, Result};
use candid::Principal;
use pocket_ic::PocketIc;

pub fn set_controllers_exact(pic: &PocketIc, canister: Principal, controllers: Vec<Principal>) -> Result<()> {
    let sender = pic
        .get_controllers(canister)
        .first()
        .copied()
        .unwrap_or(Principal::anonymous());
    pic.set_controllers(canister, Some(sender), controllers)
        .map_err(|e| anyhow!("set_controllers reject: {e:?}"))?;
    Ok(())
}
