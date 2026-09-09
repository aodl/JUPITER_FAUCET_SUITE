use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use jupiter_ic_clients::management::{
    self, CanisterStatusArgs as ManagementCanisterStatusArgs,
    CanisterStatusResult as ManagementCanisterStatusResult, UpdateSettingsArgs,
};
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusArgs {
    canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusResult {
    cycles: Nat,
    settings: CanisterStatusSettings,
    memory_size: Option<Nat>,
    memory_metrics: Option<CanisterStatusMemoryMetrics>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusSettings {
    controllers: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterStatusMemoryMetrics {
    wasm_memory_size: Nat,
    stable_memory_size: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugCall {
    canister_id: Principal,
    caller: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RefreshEndowmentsProxyArgs {
    canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RefreshEndowmentsOnewayBatchArgs {
    canister_id: Principal,
    call_count: u32,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RefreshEndowmentsRawOnewayBatchArgs {
    canister_id: Principal,
    call_count: u32,
    raw_args: Vec<u8>,
    take_raw_args: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CommitmentIndexFault {
    observed_at_ts: u64,
    last_cursor_tx_id: Option<u64>,
    offending_tx_id: u64,
    message: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RefreshEndowmentsOutcome {
    Updated,
    NoQualifyingChange,
    IncompleteProgress,
    Busy,
    RateLimited,
    UpstreamFailure { message: String },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct EndowmentIndexProgress {
    revision: u64,
    newly_indexed_qualifying_endowments: u64,
    complete_from_genesis: bool,
    committed_head_staking_tx_id: Option<u64>,
    oldest_indexed_staking_tx_id: Option<u64>,
    observed_head_staking_tx_id: Option<u64>,
    next_staking_start_tx_id: Option<u64>,
    commitment_index_fault: Option<CommitmentIndexFault>,
    retry_after_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RefreshEndowmentsResponse {
    outcome: RefreshEndowmentsOutcome,
    progress: EndowmentIndexProgress,
}

thread_local! {
    static CALLS: RefCell<Vec<DebugCall>> = const { RefCell::new(Vec::new()) };
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
async fn canister_status(args: CanisterStatusArgs) -> CanisterStatusResult {
    CALLS.with(|calls| {
        calls.borrow_mut().push(DebugCall {
            canister_id: args.canister_id,
            caller: ic_cdk::api::msg_caller(),
        });
    });

    let resp = Call::bounded_wait(Principal::management_canister(), "canister_status")
        .with_arg(&args)
        .await
        .unwrap_or_else(|err| ic_cdk::trap(format!("management canister_status failed: {err:?}")));
    resp.candid()
        .unwrap_or_else(|err| ic_cdk::trap(format!("decode canister_status failed: {err:?}")))
}

#[ic_cdk::update]
async fn debug_management_canister_status(
    args: ManagementCanisterStatusArgs,
) -> Result<ManagementCanisterStatusResult, String> {
    management::canister_status(&args)
        .await
        .map_err(|err| format!("{err:?}"))
}

#[ic_cdk::update]
async fn debug_management_update_settings(args: UpdateSettingsArgs) -> Result<(), String> {
    management::update_settings(&args)
        .await
        .map_err(|err| format!("{err:?}"))
}

#[ic_cdk::update]
async fn debug_refresh_endowments(
    args: RefreshEndowmentsProxyArgs,
) -> Result<RefreshEndowmentsResponse, String> {
    let response = Call::bounded_wait(args.canister_id, "refresh_endowments")
        .with_arg(())
        .await
        .map_err(|err| format!("refresh_endowments call failed: {err:?}"))?;
    response
        .candid()
        .map_err(|err| format!("refresh_endowments decode failed: {err:?}"))
}

#[ic_cdk::update]
fn debug_refresh_endowments_oneway_batch(
    args: RefreshEndowmentsOnewayBatchArgs,
) -> Result<u32, String> {
    for ordinal in 0..args.call_count {
        Call::bounded_wait(args.canister_id, "refresh_endowments")
            .with_arg(())
            .oneway()
            .map_err(|err| format!("refresh_endowments one-way call {ordinal} failed: {err:?}"))?;
    }
    Ok(args.call_count)
}

#[ic_cdk::update]
fn debug_refresh_endowments_raw_oneway_batch(
    args: RefreshEndowmentsRawOnewayBatchArgs,
) -> Result<u32, String> {
    for ordinal in 0..args.call_count {
        let result = if args.take_raw_args {
            Call::bounded_wait(args.canister_id, "refresh_endowments")
                .take_raw_args(args.raw_args.clone())
                .oneway()
        } else {
            Call::bounded_wait(args.canister_id, "refresh_endowments")
                .with_raw_args(&args.raw_args)
                .oneway()
        };
        result.map_err(|err| {
            format!("refresh_endowments raw one-way call {ordinal} failed: {err:?}")
        })?;
    }
    Ok(args.call_count)
}

#[ic_cdk::query]
fn debug_calls() -> Vec<DebugCall> {
    CALLS.with(|calls| calls.borrow().clone())
}

#[ic_cdk::update]
fn debug_reset() {
    CALLS.with(|calls| calls.borrow_mut().clear());
}
