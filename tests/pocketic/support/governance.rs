use anyhow::{anyhow, Result};
use candid::Principal;
use pocket_ic::PocketIc;

pub fn stop_canister_as(pic: &PocketIc, canister: Principal, sender: Principal) -> Result<()> {
    pic.stop_canister(canister, Some(sender))
        .map_err(|r| anyhow!("stop_canister({canister}) reject: {r:?}"))
}

pub fn start_canister_as(pic: &PocketIc, canister: Principal, sender: Principal) -> Result<()> {
    pic.start_canister(canister, Some(sender))
        .map_err(|r| anyhow!("start_canister({canister}) reject: {r:?}"))
}

pub fn set_controllers_exact(
    pic: &PocketIc,
    canister: Principal,
    controllers: Vec<Principal>,
) -> Result<()> {
    let sender = pic
        .get_controllers(canister)
        .first()
        .copied()
        .unwrap_or(Principal::anonymous());
    pic.set_controllers(canister, Some(sender), controllers)
        .map_err(|e| anyhow!("set_controllers reject: {e:?}"))?;
    Ok(())
}
