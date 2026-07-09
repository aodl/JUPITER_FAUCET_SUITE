//! Minimal Jupiter-owned subset of IC management-canister bindings.
//!
//! This module intentionally models only the management methods Jupiter needs
//! in production canister code, keeping production wasm dependency graphs
//! narrow instead of pulling in broad generated management bindings.
//!
//! Preserve the existing management-call policy unless making an explicit
//! scheduler-policy decision: trusted/mutating management-canister calls use
//! unbounded wait and include `sender_canister_version`, while read-style
//! metadata calls use bounded wait.

use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::{Call, CallResult};

const UPDATE_SETTINGS_METHOD: &str = "update_settings";
const CANISTER_INFO_METHOD: &str = "canister_info";
const CANISTER_STATUS_METHOD: &str = "canister_status";
const CREATE_CANISTER_METHOD: &str = "create_canister";
const INSTALL_CODE_METHOD: &str = "install_code";

#[derive(CandidType, Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct CanisterSettings {
    pub controllers: Option<Vec<Principal>>,
    pub log_visibility: Option<LogVisibility>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum LogVisibility {
    #[serde(rename = "controllers")]
    Controllers,
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "allowed_viewers")]
    AllowedViewers(Vec<Principal>),
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

#[derive(CandidType, Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct CreateCanisterArgs {
    pub settings: Option<CanisterSettings>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
struct CompleteCreateCanisterArgs {
    settings: Option<CanisterSettings>,
    sender_canister_version: Option<u64>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CreateCanisterResult {
    pub canister_id: Principal,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum InstallMode {
    #[serde(rename = "install")]
    Install,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct InstallCodeArgs {
    pub mode: InstallMode,
    pub canister_id: Principal,
    pub wasm_module: Vec<u8>,
    pub arg: Vec<u8>,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
struct CompleteInstallCodeArgs {
    mode: InstallMode,
    canister_id: Principal,
    wasm_module: Vec<u8>,
    arg: Vec<u8>,
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

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CanisterStatusArgs {
    pub canister_id: Principal,
}

#[derive(CandidType, Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CanisterStatusResult {
    pub module_hash: Option<Vec<u8>>,
    pub settings: CanisterSettings,
}

pub async fn update_settings(arg: &UpdateSettingsArgs) -> CallResult<()> {
    let complete_arg = CompleteUpdateSettingsArgs {
        canister_id: arg.canister_id,
        settings: arg.settings.clone(),
        sender_canister_version: Some(ic_cdk::api::canister_version()),
    };

    Ok(
        // Intentional exception: trusted IC management-canister mutations follow
        // the SDK management-call model and include sender_canister_version.
        // External canister calls remain bounded to avoid untrusted callee stalls.
        Call::unbounded_wait(Principal::management_canister(), UPDATE_SETTINGS_METHOD)
            .with_arg(&complete_arg)
            .await?
            .candid()?,
    )
}

pub async fn create_canister(
    arg: &CreateCanisterArgs,
    cycles_to_attach: u128,
) -> CallResult<CreateCanisterResult> {
    let complete_arg = CompleteCreateCanisterArgs {
        settings: arg.settings.clone(),
        sender_canister_version: Some(ic_cdk::api::canister_version()),
    };

    Ok(
        Call::unbounded_wait(Principal::management_canister(), CREATE_CANISTER_METHOD)
            .with_arg(&complete_arg)
            .with_cycles(cycles_to_attach)
            .await?
            .candid()?,
    )
}

pub async fn install_code(arg: &InstallCodeArgs) -> CallResult<()> {
    let complete_arg = CompleteInstallCodeArgs {
        mode: arg.mode.clone(),
        canister_id: arg.canister_id,
        wasm_module: arg.wasm_module.clone(),
        arg: arg.arg.clone(),
        sender_canister_version: Some(ic_cdk::api::canister_version()),
    };

    Ok(
        Call::unbounded_wait(Principal::management_canister(), INSTALL_CODE_METHOD)
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

pub async fn canister_status(arg: &CanisterStatusArgs) -> CallResult<CanisterStatusResult> {
    Ok(
        Call::bounded_wait(Principal::management_canister(), CANISTER_STATUS_METHOD)
            .with_arg(arg)
            .await?
            .candid()?,
    )
}
