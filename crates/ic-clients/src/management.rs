//! Minimal Jupiter-owned subset of IC management-canister bindings.
//!
//! This module intentionally models only the management methods Jupiter needs
//! in production canister code, keeping production wasm dependency graphs
//! narrow instead of pulling in broad generated management bindings.
//!
//! Preserve the existing management-call policy unless making an explicit
//! scheduler-policy decision: mutating or trusted management calls use
//! unbounded wait and include `sender_canister_version`, while read-style
//! metadata calls use bounded wait.

use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::{Call, CallResult};

const UPDATE_SETTINGS_METHOD: &str = "update_settings";
const CANISTER_INFO_METHOD: &str = "canister_info";

#[derive(CandidType, Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct CanisterSettings {
    pub controllers: Option<Vec<Principal>>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct UpdateSettingsArgs {
    pub canister_id: Principal,
    pub settings: CanisterSettings,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
struct CompleteUpdateSettingsArgs {
    canister_id: Principal,
    settings: CanisterSettings,
    sender_canister_version: Option<u64>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CanisterInfoArgs {
    pub canister_id: Principal,
    pub num_requested_changes: Option<u64>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CanisterInfoResult {
    pub module_hash: Option<Vec<u8>>,
    pub controllers: Vec<Principal>,
}

pub async fn update_settings(arg: &UpdateSettingsArgs) -> CallResult<()> {
    let complete_arg = CompleteUpdateSettingsArgs {
        canister_id: arg.canister_id,
        settings: arg.settings.clone(),
        sender_canister_version: Some(ic_cdk::api::canister_version()),
    };

    Ok(
        Call::unbounded_wait(Principal::management_canister(), UPDATE_SETTINGS_METHOD)
            .with_arg(&complete_arg)
            .await?
            .candid()?,
    )
}

pub async fn canister_info(arg: &CanisterInfoArgs) -> CallResult<CanisterInfoResult> {
    Ok(
        Call::bounded_wait(Principal::management_canister(), CANISTER_INFO_METHOD)
            .with_arg(arg)
            .await?
            .candid()?,
    )
}
