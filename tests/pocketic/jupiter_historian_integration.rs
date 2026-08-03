// PocketIC historian scenarios use explicit casts to mirror Candid/interface boundary values.
#![allow(clippy::unnecessary_cast)]

use anyhow::{anyhow, bail, Context, Result};
use candid::{encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    StableBTreeMap, Storable, VectorMemory,
};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use jupiter_ic_clients::account_identifier::account_identifier_text;
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsArgs, GetAccountIdentifierTransactionsResponse,
    GetAccountIdentifierTransactionsResult,
};
use pocket_ic::PocketIc;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[path = "real_blackhole.rs"]
mod real_blackhole;
#[path = "support/mod.rs"]
mod support;
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

fn require_ignored_flag() -> Result<()> {
    // These PocketIC suites are intentionally #[ignore] so a plain cargo test stays fast.
    // The supported repository entry points (for example `cargo run -p xtask -- test_all`)
    // invoke them explicitly with `--ignored`.
    support::assertions::require_ignored_flag()
}
fn build_pic_with_real_icp() -> PocketIc {
    support::ledger::build_pic_with_real_icp()
}

static INDEX_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_WASM_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_ROOT_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static XRC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static HISTORIAN_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static PRE_CUTOVER_HISTORIAN_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_ENABLED_HISTORIAN_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static STATUS_PROXY_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CYCLE_BURNER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static NNS_GOVERNANCE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
fn index_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&INDEX_WASM, "mock-icp-index", None)
}
fn sns_wasm_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&SNS_WASM_WASM, "mock-sns-wasm", None)
}
fn sns_root_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&SNS_ROOT_WASM, "mock-sns-root", None)
}
fn xrc_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&XRC_WASM, "mock-xrc", None)
}
fn historian_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(
        &HISTORIAN_WASM,
        "jupiter-historian",
        Some("debug_api"),
    )
}

fn pre_cutover_historian_wasm() -> Result<Vec<u8>> {
    const BASE_REVISION: &str = "98c871a85af91320a5dfc59b5b040727e21aa094";
    if let Some(bytes) = PRE_CUTOVER_HISTORIAN_WASM.get() {
        return Ok(bytes.clone());
    }
    if let Some(path) = std::env::var_os("JUPITER_HISTORIAN_PRE_CUTOVER_WASM") {
        let bytes = std::fs::read(PathBuf::from(path))?;
        let _ = PRE_CUTOVER_HISTORIAN_WASM.set(bytes.clone());
        return Ok(bytes);
    }

    let workspace_root = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let unique = format!(
        "jupiter-historian-pre-cutover-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let worktree = std::env::temp_dir().join(unique);
    let add_status = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&worktree)
        .arg(BASE_REVISION)
        .current_dir(&workspace_root)
        .status()
        .context("create pre-cutover Historian worktree")?;
    if !add_status.success() {
        bail!("git worktree add failed for pre-cutover Historian");
    }

    let build_result = (|| {
        let status = Command::new("./tools/scripts/build-canister")
            .arg("jupiter-historian")
            .current_dir(&worktree)
            .status()
            .context("build pre-cutover Historian")?;
        if !status.success() {
            bail!("pre-cutover Historian build failed");
        }
        std::fs::read(worktree.join("release-artifacts/jupiter_historian.wasm"))
            .context("read pre-cutover Historian Wasm")
    })();
    let remove_status = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&worktree)
        .current_dir(&workspace_root)
        .status()
        .context("remove pre-cutover Historian worktree")?;
    if !remove_status.success() {
        bail!("failed to remove pre-cutover Historian worktree");
    }
    let bytes = build_result?;
    let _ = PRE_CUTOVER_HISTORIAN_WASM.set(bytes.clone());
    Ok(bytes)
}
fn relay_wasm() -> Result<Vec<u8>> {
    if let Some(path) = std::env::var_os("JUPITER_RELAY_TEST_WASM") {
        return std::fs::read(PathBuf::from(path)).context("read JUPITER_RELAY_TEST_WASM");
    }
    support::wasm::build_wasm_cached_for_test(&RELAY_WASM, "jupiter-relay", None)
}
fn status_proxy_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&STATUS_PROXY_WASM, "mock-status-proxy", None)
}
fn cycle_burner_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&CYCLE_BURNER_WASM, "mock-cycle-burner", None)
}
fn nns_governance_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&NNS_GOVERNANCE_WASM, "mock-nns-governance", None)
}
fn relay_enabled_historian_wasm() -> Result<Vec<u8>> {
    if let Some(bytes) = RELAY_ENABLED_HISTORIAN_WASM.get() {
        return Ok(bytes.clone());
    }
    if let Some(path) = std::env::var_os("JUPITER_HISTORIAN_CANDIDATE_WASM") {
        let bytes =
            std::fs::read(PathBuf::from(path)).context("read JUPITER_HISTORIAN_CANDIDATE_WASM")?;
        let _ = RELAY_ENABLED_HISTORIAN_WASM.set(bytes.clone());
        return Ok(bytes);
    }
    let relay = relay_wasm()?;
    let workspace_root = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let relay_path =
        workspace_root.join("target/wasm32-unknown-unknown/release/jupiter_relay.wasm");
    let relay_gz_path =
        workspace_root.join("target/wasm32-unknown-unknown/release/jupiter_relay.wasm.gz");
    let gzip_status = Command::new("gzip")
        .args(["-n", "-9", "-c"])
        .arg(&relay_path)
        .current_dir(&workspace_root)
        .output()?;
    if !gzip_status.status.success() {
        bail!("gzip failed for relay wasm embedded in historian PocketIC test");
    }
    std::fs::write(&relay_gz_path, gzip_status.stdout)?;
    let raw_hash = hex::encode(Sha256::digest(std::fs::read(&relay_path)?));
    let gz_hash = hex::encode(Sha256::digest(std::fs::read(&relay_gz_path)?));
    let status = Command::new("cargo")
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "-p",
            "jupiter-historian",
            "--locked",
        ])
        .env("JUPITER_RELAY_WASM_PATH", &relay_gz_path)
        .env("JUPITER_RELAY_RAW_WASM_PATH", &relay_path)
        .env("JUPITER_RELAY_RAW_WASM_SHA256", &raw_hash)
        .env("JUPITER_RELAY_GZ_WASM_SHA256", &gz_hash)
        .current_dir(&workspace_root)
        .status()?;
    if !status.success() {
        bail!("cargo build (wasm) failed for jupiter-historian with embedded relay wasm");
    }
    let path = workspace_root.join("target/wasm32-unknown-unknown/release/jupiter_historian.wasm");
    let bytes = std::fs::read(&path)?;
    let _ = RELAY_ENABLED_HISTORIAN_WASM.set(bytes.clone());
    assert!(
        !relay.is_empty(),
        "non-debug relay wasm used for embedding must not be empty"
    );
    Ok(bytes)
}

use support::calls::{query_one, tick_n, update_bytes, update_noargs, update_one};
use support::governance::set_controllers_exact;

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianInitArg {
    staking_account: Account,
    output_source_account: Option<Account>,
    output_account: Option<Account>,
    rewards_account: Option<Account>,
    ledger_canister_id: Option<Principal>,
    index_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    faucet_canister_id: Option<Principal>,
    sns_wasm_canister_id: Option<Principal>,
    xrc_canister_id: Option<Principal>,
    enable_sns_tracking: Option<bool>,
    scan_interval_seconds: Option<u64>,
    cycles_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    max_cycles_entries_per_canister: Option<u32>,
    max_commitment_entries_per_canister: Option<u32>,
    max_index_pages_per_tick: Option<u32>,
    max_canisters_per_cycles_tick: Option<u32>,
    relay_factory_enabled: Option<bool>,
    relay_setup_min_e8s: Option<u64>,
    relay_initial_cycles: Option<u128>,
    relay_cycle_safety_margin_e8s: Option<u64>,
    relay_min_subaccount_one_seed_e8s: Option<u64>,
    self_service_relay_interval_seconds: Option<u64>,
    io_surplus_neuron_id: Option<u64>,
    canonical_relay_canister_id: Option<Principal>,
    canonical_relay_targets: Option<Vec<Principal>>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianUpgradeArg {
    enable_sns_tracking: Option<bool>,
    scan_interval_seconds: Option<u64>,
    cycles_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    max_cycles_entries_per_canister: Option<u32>,
    max_commitment_entries_per_canister: Option<u32>,
    max_index_pages_per_tick: Option<u32>,
    max_canisters_per_cycles_tick: Option<u32>,
    sns_wasm_canister_id: Option<Principal>,
    xrc_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    faucet_canister_id: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum CanisterTrackingReason {
    MemoCommitment,
    SnsDiscovery,
    RelayTarget,
    RelayInstance,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListCanistersArgs {
    start_after: Option<Principal>,
    limit: Option<u32>,
    tracking_reason_filter: Option<CanisterTrackingReason>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct CanisterListItem {
    canister_id: Principal,
    tracking_reasons: Vec<CanisterTrackingReason>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListCanistersResponse {
    items: Vec<CanisterListItem>,
    next_start_after: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetCommitmentHistoryArgs {
    canister_id: Principal,
    start_after_tx_id: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetNeuronCommitmentHistoryArgs {
    neuron_id: u64,
    start_after_tx_id: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct CommitmentSample {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CommitmentHistoryPage {
    items: Vec<CommitmentSample>,
    next_start_after_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum CyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootStatus,
    SnsSwapStatus,
    SnsRootSummary,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct CyclesSample {
    timestamp_nanos: u64,
    cycles: u128,
    source: CyclesSampleSource,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CyclesHistoryPage {
    items: Vec<CyclesSample>,
    next_start_after_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetCyclesHistoryArgs {
    canister_id: Principal,
    start_after_ts: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsCanisterStatus {
    cycles: Option<Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsCanisterSummary {
    canister_id: Option<Principal>,
    status: Option<SnsCanisterStatus>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct GetSnsCanistersSummaryResponse {
    root: Option<SnsCanisterSummary>,
    governance: Option<SnsCanisterSummary>,
    ledger: Option<SnsCanisterSummary>,
    swap: Option<SnsCanisterSummary>,
    index: Option<SnsCanisterSummary>,
    dapps: Vec<SnsCanisterSummary>,
    archives: Vec<SnsCanisterSummary>,
}
#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct PublicCounts {
    tracked_canister_count: u64,
    memo_registered_canister_count: u64,
    raw_icp_declared_canister_count: Option<u64>,
    declared_neuron_count: Option<u64>,
    qualifying_commitment_count: u64,
    sns_discovered_canister_count: u64,
    relay_target_canister_count: u64,
    relay_instance_canister_count: u64,
    total_output_e8s: u64,
    total_rewards_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct CommitmentIndexFault {
    observed_at_ts: u64,
    last_cursor_tx_id: Option<u64>,
    offending_tx_id: u64,
    message: String,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct IcpXdrRateSnapshot {
    rate: u64,
    decimals: u32,
    timestamp: u64,
    fetched_at_ts: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct PublicStatus {
    staking_account: Account,
    ledger_canister_id: Principal,
    last_index_run_ts: Option<u64>,
    index_interval_seconds: u64,
    last_completed_cycles_sweep_ts: Option<u64>,
    cycles_interval_seconds: u64,
    heap_memory_bytes: Option<u64>,
    stable_memory_bytes: Option<u64>,
    total_memory_bytes: Option<u64>,
    commitment_index_fault: Option<CommitmentIndexFault>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct DebugState {
    distinct_canister_count: u32,
    last_indexed_staking_tx_id: Option<u64>,
    last_indexed_output_tx_id: Option<u64>,
    last_indexed_rewards_tx_id: Option<u64>,
    last_sns_discovery_ts: u64,
    last_completed_cycles_sweep_ts: u64,
    last_completed_route_sweep_ts: Option<u64>,
    active_cycles_sweep_present: bool,
    active_cycles_sweep_next_index: Option<u64>,
    active_route_sweep_present: bool,
    active_route_sweep_next_index: Option<u64>,
    active_sns_discovery_present: bool,
    active_sns_discovery_next_index: Option<u64>,
    oldest_indexed_staking_tx_id: Option<u64>,
    staking_index_descending: Option<bool>,
    staking_backfill_complete: Option<bool>,
    oldest_indexed_output_tx_id: Option<u64>,
    output_route_index_descending: Option<bool>,
    output_route_backfill_complete: Option<bool>,
    oldest_indexed_rewards_tx_id: Option<u64>,
    rewards_route_index_descending: Option<bool>,
    rewards_route_backfill_complete: Option<bool>,
    main_lock_state_ts: Option<u64>,
    last_main_run_ts: u64,
    initial_cycles_probe_queue_len: u32,
    initial_cycles_probe_queue: Vec<Principal>,
    active_cycles_sweep_started_at_ts_nanos: Option<u64>,
    active_route_sweep_started_at_ts_nanos: Option<u64>,
    active_sns_discovery_started_at_ts_nanos: Option<u64>,
    active_sns_discovery_root_canister_ids: Vec<Principal>,
    commitment_index_fault: Option<CommitmentIndexFault>,
    icp_xdr_rate: Option<IcpXdrRateSnapshot>,
    last_icp_xdr_rate_attempt_ts: Option<u64>,
    last_icp_xdr_rate_error: Option<String>,
    cached_cycles_probe_route_count: u32,
    last_index_run_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListMemoRegisteredCanisterSummariesArgs {
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct MemoRegisteredCanisterSummary {
    canister_id: Principal,
    tracking_reasons: Vec<CanisterTrackingReason>,
    qualifying_commitment_count: u64,
    total_qualifying_committed_e8s: u64,
    last_commitment_ts: Option<u64>,
    latest_cycles: Option<u128>,
    last_cycles_probe_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListMemoRegisteredCanisterSummariesResponse {
    items: Vec<MemoRegisteredCanisterSummary>,
    page: u32,
    page_size: u32,
    total: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum CyclesProbeResult {
    Ok(CyclesSampleSource),
    NotAvailable,
    Error(String),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct CanisterMeta {
    first_seen_ts: Option<u64>,
    last_commitment_ts: Option<u64>,
    last_cycles_probe_ts: Option<u64>,
    last_cycles_probe_result: Option<CyclesProbeResult>,
    last_burn_tx_id: Option<u64>,
    last_burn_scan_tx_id: Option<u64>,
    burned_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterOverview {
    canister_id: Principal,
    tracking_reasons: Vec<CanisterTrackingReason>,
    meta: CanisterMeta,
    cycles_points: u32,
    commitment_points: u32,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListRecentCommitmentsArgs {
    limit: Option<u32>,
    qualifying_only: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RecentCommitmentListItem {
    canister_id: Option<Principal>,
    neuron_id: Option<u64>,
    raw_icp_memo_text: Option<String>,
    neuron_memo_text: Option<String>,
    memo_text: Option<String>,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListRecentCommitmentsResponse {
    items: Vec<RecentCommitmentListItem>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct SnsExtensions {
    extension_canister_ids: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListSnsCanistersResponse {
    root: Option<Principal>,
    governance: Option<Principal>,
    ledger: Option<Principal>,
    swap: Option<Principal>,
    index: Option<Principal>,
    dapps: Vec<Principal>,
    archives: Vec<Principal>,
    extensions: Option<SnsExtensions>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListDeployedSnsesArgs {}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DeployedSns {
    root_canister_id: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListDeployedSnsesResponse {
    instances: Vec<DeployedSns>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsRootCanisterStatusArgs {
    canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsRootCanisterStatusResult {
    cycles: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayCreationPhase {
    Reserved,
    ProbingTargets,
    CmcTransferPrepared,
    CmcTransferAccepted,
    CmcNotifySucceeded,
    CreateDispatched,
    ChildCreated,
    CodeInstalled,
    RelayFundingPrepared,
    RelayFunded,
    HandoffAttempted,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelaySetupState {
    NotFunded,
    InProgress {
        phase: RelayCreationPhase,
        relay_canister_id: Option<Principal>,
    },
    Active {
        relay_canister_id: Principal,
    },
    ManualRecoveryRequired {
        phase: RelayCreationPhase,
        relay_canister_id: Option<Principal>,
        message: String,
    },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayTargetSetArgs {
    target_canister_ids: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelaySetupView {
    canonical_target_canister_ids: Vec<Principal>,
    setup_key_identifier: String,
    setup_account: Option<Account>,
    setup_account_identifier: Option<String>,
    target_count: u32,
    nominal_minimum_e8s: u64,
    factory_available: bool,
    state: RelaySetupState,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RelaySetupViewResult {
    Ok(RelaySetupView),
    Err(String),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RelaySetupNotifyResult {
    BelowMinimum {
        balance_e8s: u64,
        required_e8s: u64,
        shortfall_e8s: u64,
    },
    BelowCurrentRequirement {
        balance_e8s: u64,
        required_e8s: u64,
        shortfall_e8s: u64,
    },
    Busy,
    InProgress {
        phase: RelayCreationPhase,
        relay_canister_id: Option<Principal>,
    },
    Active {
        relay_canister_id: Principal,
    },
    FailedPreSpend {
        message: String,
    },
    ManualRecoveryRequired {
        phase: RelayCreationPhase,
        relay_canister_id: Option<Principal>,
        message: String,
    },
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
enum RetiredFixtureSetupStatus {
    ManualRecoveryRequired,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
struct RetiredFixturePayment {
    target_canister_id: Principal,
    tx_id: u64,
    from_account_identifier: String,
    amount_e8s: u64,
    timestamp_nanos: Option<u64>,
    processed: bool,
    refunded: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
enum RetiredFixtureTransferKind {
    CmcConversion,
    Refund,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
enum RetiredFixturePhase {
    CycleNotifySucceeded,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
struct RetiredFixtureTransfer {
    kind: RetiredFixtureTransferKind,
    from_subaccount: Option<[u8; 32]>,
    from_account_identifier: String,
    to: Account,
    to_account_identifier: String,
    amount_e8s: u64,
    fee_e8s: u64,
    memo: Option<Vec<u8>>,
    created_at_time_nanos: u64,
    block_index: Option<u64>,
    completed: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
struct RetiredFixtureCreateAttempt {
    target_canister_id: Principal,
    created_at_ts: u64,
    initial_cycles: u128,
    raw_relay_wasm_hash_hex: Option<String>,
    install_payload_hash_hex: Option<String>,
    relay_wasm_hash_hex: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
struct RetiredFixtureJob {
    target_canister_id: Principal,
    setup_account: Account,
    setup_account_identifier: String,
    status: RetiredFixtureSetupStatus,
    relay_canister_id: Option<Principal>,
    last_indexed_setup_tx_id: Option<u64>,
    setup_tx_ids: Vec<u64>,
    setup_amount_seen_e8s: u64,
    setup_amount_processed_e8s: u64,
    payments: Vec<RetiredFixturePayment>,
    cycle_conversion_e8s: Option<u64>,
    cycle_transfer_block_index: Option<u64>,
    cycles_minted: Option<u128>,
    relay_initial_cycles: Option<u128>,
    relay_funding_e8s: Option<u64>,
    relay_funding_block_index: Option<u64>,
    phase: Option<RetiredFixturePhase>,
    cycle_transfer: Option<RetiredFixtureTransfer>,
    relay_funding_transfer: Option<RetiredFixtureTransfer>,
    existing_relay_sweep_transfer: Option<RetiredFixtureTransfer>,
    refund_transfers: Vec<RetiredFixtureTransfer>,
    relay_create_attempt: Option<RetiredFixtureCreateAttempt>,
    code_installed: bool,
    relay_funding_accepted: bool,
    blackhole_update_attempted: bool,
    blackhole_confirmed: bool,
    refund_attempt_count: u32,
    last_refund_attempt_ts: Option<u64>,
    refund_blocks: Vec<u64>,
    created_at_ts: u64,
    updated_at_ts: u64,
    last_error: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
enum RetiredFixtureRegistryKind {
    Canonical,
    SelfService,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
enum RetiredFixtureRegistryStatus {
    Active,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
struct RetiredFixtureRegistryEntry {
    relay_canister_id: Principal,
    target_canister_id: Principal,
    kind: RetiredFixtureRegistryKind,
    status: RetiredFixtureRegistryStatus,
    setup_account: Option<Account>,
    setup_account_identifier: Option<String>,
    setup_amount_e8s: Option<u64>,
    setup_tx_ids: Vec<u64>,
    final_controllers: Option<Vec<Principal>>,
    log_visibility_public: Option<bool>,
    created_at_ts: Option<u64>,
    activated_at_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RetiredRecoveryStatus {
    ManualRecoveryRequired,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RetiredRecoveryView {
    target_canister_id: Principal,
    status: RetiredRecoveryStatus,
    last_error: Option<String>,
    relay_canister_id: Option<Principal>,
    setup_account_identifier: String,
}

#[derive(Clone, Debug, CandidType)]
struct RetiredRecoveryArgs {
    target_canister_id: Principal,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FixturePrincipalKey(Vec<u8>);

impl Storable for FixturePrincipalKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 29,
        is_fixed_size: false,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FixtureBytes(Vec<u8>);

impl Storable for FixtureBytes {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FixtureSetupKey([u8; 32]);

impl Storable for FixtureSetupKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.as_ref().try_into().expect("32-byte setup key"))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: true,
    };
}

type FixtureMemory = VirtualMemory<VectorMemory>;

fn real_icp_ledger_principal() -> Principal {
    support::principals::icp_ledger()
}

fn real_icp_index_principal() -> Principal {
    support::principals::icp_index()
}

fn abandoned_relay_target() -> Principal {
    Principal::from_text("2lo52-kiaaa-aaaar-qaqta-cai").unwrap()
}

fn production_historian() -> Principal {
    Principal::from_text("j5gs6-uiaaa-aaaar-qb5cq-cai").unwrap()
}

fn retired_fixture_job() -> RetiredFixtureJob {
    let target = abandoned_relay_target();
    let historian = production_historian();
    let setup_subaccount = jupiter_ic_clients::account::relay_setup_subaccount(target);
    let setup_identifier = "54467f93c896278cad2d7a6190b021c7f69e393389d96e8a2b47a1ee5e2ad5ab";
    let payer = "production-payer-account".to_string();
    RetiredFixtureJob {
        target_canister_id: target,
        setup_account: Account {
            owner: historian,
            subaccount: Some(setup_subaccount),
        },
        setup_account_identifier: setup_identifier.to_string(),
        status: RetiredFixtureSetupStatus::ManualRecoveryRequired,
        relay_canister_id: None,
        last_indexed_setup_tx_id: Some(37_414_358),
        setup_tx_ids: vec![37_414_222, 37_414_358],
        setup_amount_seen_e8s: 300_000_000,
        setup_amount_processed_e8s: 0,
        payments: vec![
            RetiredFixturePayment {
                target_canister_id: target,
                tx_id: 37_414_222,
                from_account_identifier: payer.clone(),
                amount_e8s: 100_000_000,
                timestamp_nanos: Some(1_783_774_380_958_013_045),
                processed: false,
                refunded: true,
            },
            RetiredFixturePayment {
                target_canister_id: target,
                tx_id: 37_414_358,
                from_account_identifier: payer.clone(),
                amount_e8s: 200_000_000,
                timestamp_nanos: Some(1_783_775_049_246_080_846),
                processed: false,
                refunded: false,
            },
        ],
        cycle_conversion_e8s: Some(94_950_000),
        cycle_transfer_block_index: Some(37_414_364),
        cycles_minted: Some(1_590_792_300_000),
        relay_initial_cycles: None,
        relay_funding_e8s: None,
        relay_funding_block_index: None,
        phase: Some(RetiredFixturePhase::CycleNotifySucceeded),
        cycle_transfer: Some(RetiredFixtureTransfer {
            kind: RetiredFixtureTransferKind::CmcConversion,
            from_subaccount: Some(setup_subaccount),
            from_account_identifier: setup_identifier.to_string(),
            to: Account {
                owner: jupiter_ic_clients::constants::cycles_minting_canister_id(),
                subaccount: Some(jupiter_ic_clients::account::principal_to_subaccount(
                    historian,
                )),
            },
            to_account_identifier:
                "7a09fb19b536cb547f8e345b46413b213da91c48c58f93565bb474615fc52e21"
                    .to_string(),
            amount_e8s: 94_950_000,
            fee_e8s: 10_000,
            memo: Some(1_347_768_404u64.to_le_bytes().to_vec()),
            created_at_time_nanos: 1_783_775_072_224_720_476,
            block_index: Some(37_414_364),
            completed: true,
        }),
        relay_funding_transfer: None,
        existing_relay_sweep_transfer: None,
        refund_transfers: vec![RetiredFixtureTransfer {
            kind: RetiredFixtureTransferKind::Refund,
            from_subaccount: Some(setup_subaccount),
            from_account_identifier: setup_identifier.to_string(),
            to: Account {
                owner: Principal::anonymous(),
                subaccount: None,
            },
            to_account_identifier: payer,
            amount_e8s: 99_990_000,
            fee_e8s: 10_000,
            memo: Some(0x4a52_5246u64.to_le_bytes().to_vec()),
            created_at_time_nanos: 1_783_774_405_254_587_251,
            block_index: Some(37_414_223),
            completed: true,
        }],
        relay_create_attempt: Some(RetiredFixtureCreateAttempt {
            target_canister_id: target,
            created_at_ts: 1_783_784_987,
            initial_cycles: 1_000_000_000_000,
            raw_relay_wasm_hash_hex: None,
            install_payload_hash_hex: None,
            relay_wasm_hash_hex: None,
        }),
        code_installed: false,
        relay_funding_accepted: false,
        blackhole_update_attempted: false,
        blackhole_confirmed: false,
        refund_attempt_count: 1,
        last_refund_attempt_ts: Some(1_783_774_405),
        refund_blocks: vec![37_414_223],
        created_at_ts: 1_783_774_396,
        updated_at_ts: 1_783_851_665,
        last_error: Some("CMC notify minted 1590792300000 cycles, below configured relay_initial_cycles 2000000000000; refusing create_canister to avoid historian subsidy after conversion".to_string()),
    }
}

fn write_retired_fixture(
    pic: &PocketIc,
    historian: Principal,
    canonical_target: Principal,
    canonical_relay: Principal,
    invalid_registry: bool,
) -> Result<()> {
    let stable_memory = Rc::new(RefCell::new(pic.get_stable_memory(historian)));
    let manager = MemoryManager::init(stable_memory.clone());
    let mut setup_jobs = StableBTreeMap::<FixturePrincipalKey, FixtureBytes, FixtureMemory>::init(
        manager.get(MemoryId::new(24)),
    );
    setup_jobs.insert(
        FixturePrincipalKey(abandoned_relay_target().as_slice().to_vec()),
        FixtureBytes(encode_one(retired_fixture_job())?),
    );
    let mut registry = StableBTreeMap::<FixturePrincipalKey, FixtureBytes, FixtureMemory>::init(
        manager.get(MemoryId::new(22)),
    );
    let (key, entry) = if invalid_registry {
        (
            abandoned_relay_target(),
            RetiredFixtureRegistryEntry {
                relay_canister_id: canonical_relay,
                target_canister_id: abandoned_relay_target(),
                kind: RetiredFixtureRegistryKind::SelfService,
                status: RetiredFixtureRegistryStatus::Active,
                setup_account: None,
                setup_account_identifier: None,
                setup_amount_e8s: None,
                setup_tx_ids: Vec::new(),
                final_controllers: None,
                log_visibility_public: None,
                created_at_ts: None,
                activated_at_ts: None,
            },
        )
    } else {
        (
            canonical_target,
            RetiredFixtureRegistryEntry {
                relay_canister_id: canonical_relay,
                target_canister_id: canonical_target,
                kind: RetiredFixtureRegistryKind::Canonical,
                status: RetiredFixtureRegistryStatus::Active,
                setup_account: None,
                setup_account_identifier: None,
                setup_amount_e8s: None,
                setup_tx_ids: Vec::new(),
                final_controllers: None,
                log_visibility_public: None,
                created_at_ts: None,
                activated_at_ts: None,
            },
        )
    };
    registry.insert(
        FixturePrincipalKey(key.as_slice().to_vec()),
        FixtureBytes(encode_one(entry)?),
    );
    drop(registry);
    drop(setup_jobs);
    drop(manager);
    let bytes = stable_memory.borrow().clone();
    pic.set_stable_memory(
        historian,
        bytes,
        pocket_ic::common::rest::BlobCompression::NoCompression,
    );
    Ok(())
}

fn retired_and_current_setup_map_lengths(pic: &PocketIc, historian: Principal) -> (u64, u64) {
    let stable_memory = Rc::new(RefCell::new(pic.get_stable_memory(historian)));
    let manager = MemoryManager::init(stable_memory);
    let retired = StableBTreeMap::<FixturePrincipalKey, FixtureBytes, FixtureMemory>::init(
        manager.get(MemoryId::new(24)),
    );
    let current = StableBTreeMap::<FixtureSetupKey, FixtureBytes, FixtureMemory>::init(
        manager.get(MemoryId::new(25)),
    );
    (retired.len(), current.len())
}

fn icrc1_fee(pic: &PocketIc, ledger: Principal) -> Result<u64> {
    support::ledger::icrc1_fee(pic, ledger)
}

fn icrc1_transfer(
    pic: &PocketIc,
    ledger: Principal,
    from: Principal,
    arg: TransferArg,
) -> Result<u64> {
    support::ledger::icrc1_transfer(pic, ledger, from, arg)
}

fn index_account_transactions(
    pic: &PocketIc,
    index: Principal,
    account_identifier: String,
    start: Option<u64>,
    max_results: u64,
) -> Result<GetAccountIdentifierTransactionsResponse> {
    let result: GetAccountIdentifierTransactionsResult = update_one(
        pic,
        index,
        Principal::anonymous(),
        "get_account_identifier_transactions",
        GetAccountIdentifierTransactionsArgs {
            max_results,
            start,
            account_identifier,
        },
    )?;
    match result {
        GetAccountIdentifierTransactionsResult::Ok(resp) => Ok(resp),
        GetAccountIdentifierTransactionsResult::Err(err) => {
            bail!("real ICP index returned error: {}", err.message)
        }
    }
}

fn wait_for_index_transactions(
    pic: &PocketIc,
    index: Principal,
    account_identifier: &str,
    expected_min: usize,
) -> Result<GetAccountIdentifierTransactionsResponse> {
    let mut last = None;
    for _ in 0..40 {
        let page = index_account_transactions(
            pic,
            index,
            account_identifier.to_string(),
            None,
            expected_min as u64,
        )?;
        if page.transactions.len() >= expected_min {
            return Ok(page);
        }
        last = Some(page);
        pic.advance_time(Duration::from_secs(1));
        tick_n(pic, 5);
    }
    bail!("real ICP index did not expose {expected_min} transactions for account {} after waiting; last page: {:?}", account_identifier, last.map(|page| page.transactions.iter().map(|tx| tx.id).collect::<Vec<_>>()));
}

fn self_service_historian_init(
    ledger: Principal,
    index: Principal,
    cmc: Principal,
    blackhole: Principal,
) -> HistorianInitArg {
    HistorianInitArg {
        staking_account: Account {
            owner: Principal::management_canister(),
            subaccount: Some([33u8; 32]),
        },
        output_source_account: None,
        output_account: None,
        rewards_account: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        faucet_canister_id: Some(blackhole),
        sns_wasm_canister_id: Some(blackhole),
        xrc_canister_id: Some(blackhole),
        enable_sns_tracking: Some(false),
        scan_interval_seconds: Some(60),
        cycles_interval_seconds: Some(60),
        min_tx_e8s: Some(10_000_000),
        max_cycles_entries_per_canister: Some(100),
        max_commitment_entries_per_canister: Some(100),
        max_index_pages_per_tick: Some(10),
        max_canisters_per_cycles_tick: Some(10),
        relay_factory_enabled: Some(true),
        relay_setup_min_e8s: Some(200_000_000),
        relay_initial_cycles: Some(1_000_000_000_000),
        relay_cycle_safety_margin_e8s: Some(5_000_000),
        relay_min_subaccount_one_seed_e8s: Some(100_020_000),
        self_service_relay_interval_seconds: Some(3600),
        io_surplus_neuron_id: Some(11614578985374291210),
        canonical_relay_canister_id: None,
        canonical_relay_targets: None,
    }
}

fn create_fixed_canister(pic: &PocketIc, canister_id: Principal) -> Result<()> {
    pic.create_canister_with_id(None, None, canister_id)
        .map(|_| ())
        .map_err(anyhow::Error::msg)
}

fn install_status_proxy(pic: &PocketIc, canister_id: Principal) -> Result<()> {
    create_fixed_canister(pic, canister_id)?;
    pic.add_cycles(canister_id, 5_000_000_000_000);
    pic.install_canister(canister_id, status_proxy_wasm()?, vec![], None);
    Ok(())
}

fn activate_target_set(
    pic: &PocketIc,
    historian: Principal,
    ledger: Principal,
    targets: Vec<Principal>,
) -> Result<Principal> {
    let view_result: RelaySetupViewResult = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        RelayTargetSetArgs {
            target_canister_ids: targets.clone(),
        },
    )?;
    let view = match view_result {
        RelaySetupViewResult::Ok(view) => view,
        other => bail!("target-set view was rejected: {other:?}"),
    };
    assert_eq!(view.target_count as usize, targets.len());
    assert!(view.factory_available);
    let setup_account = view
        .setup_account
        .context("new target set should expose setup account")?;
    let fee = icrc1_fee(pic, ledger)?;
    let split_one = (view.nominal_minimum_e8s - 1) / 2;
    let deposits = [1, split_one, view.nominal_minimum_e8s - 1 - split_one];
    assert!(deposits
        .iter()
        .all(|amount| *amount < view.nominal_minimum_e8s));
    for amount in deposits {
        let _ = icrc1_transfer(
            pic,
            ledger,
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: setup_account,
                fee: Some(Nat::from(fee)),
                created_at_time: None,
                memo: None,
                amount: Nat::from(amount),
            },
        )?;
    }
    assert_eq!(
        support::ledger::icrc1_balance(pic, ledger, &setup_account)?,
        view.nominal_minimum_e8s
    );
    let result: RelaySetupNotifyResult = update_one(
        pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        RelayTargetSetArgs {
            target_canister_ids: targets,
        },
    )?;
    match result {
        RelaySetupNotifyResult::Active { relay_canister_id } => Ok(relay_canister_id),
        other => bail!("expected Active target-set setup, got {other:?}"),
    }
}

fn assert_spawned_relay_faucet_identity(
    pic: &PocketIc,
    index: Principal,
    relay: Principal,
) -> Result<()> {
    let mut subaccount_one = [0u8; 32];
    subaccount_one[31] = 1;
    let account_identifier = account_identifier_text(relay, Some(subaccount_one));
    let page = wait_for_index_transactions(pic, index, &account_identifier, 2)?;
    let expected_memo = format!("{}.Relay", relay.to_text().replace('-', "")).into_bytes();
    assert!(
        page.transactions
            .iter()
            .any(|item| item.transaction.icrc1_memo.as_deref() == Some(expected_memo.as_slice())),
        "spawned Relay {relay} must use its own principal in its Faucet memo; transactions={:?}",
        page.transactions
    );
    Ok(())
}

fn prepare_pre_cutover_fixture(
    base_wasm: &[u8],
    invalid_registry: bool,
) -> Result<(PocketIc, Principal)> {
    let pic = build_pic_with_real_icp();
    let historian = production_historian();
    let canonical_target = pic.create_canister();
    let canonical_relay = pic.create_canister();
    for canister in [canonical_target, canonical_relay] {
        pic.add_cycles(canister, 5_000_000_000_000);
    }
    create_fixed_canister(&pic, historian)?;
    pic.add_cycles(historian, 40_000_000_000_000);
    let mut init = self_service_historian_init(
        real_icp_ledger_principal(),
        real_icp_index_principal(),
        support::principals::cycles_minting_canister(),
        jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
    );
    init.canonical_relay_canister_id = Some(canonical_relay);
    init.canonical_relay_targets = Some(vec![canonical_target]);
    pic.install_canister(historian, base_wasm.to_vec(), encode_one(init)?, None);
    write_retired_fixture(
        &pic,
        historian,
        canonical_target,
        canonical_relay,
        invalid_registry,
    )?;
    pic.advance_time(Duration::from_secs(2));
    tick_n(&pic, 5);
    pic.upgrade_canister(
        historian,
        base_wasm.to_vec(),
        encode_one(HistorianUpgradeArg {
            enable_sns_tracking: None,
            scan_interval_seconds: None,
            cycles_interval_seconds: None,
            min_tx_e8s: None,
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
        })?,
        None,
    )
    .map_err(|error| anyhow!("load pre-cutover stable fixture: {error:?}"))?;
    tick_n(&pic, 5);
    Ok((pic, historian))
}

fn query_retired_recovery_view(
    pic: &PocketIc,
    historian: Principal,
) -> Result<RetiredRecoveryView> {
    query_one(
        pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_recovery_view",
        RetiredRecoveryArgs {
            target_canister_id: abandoned_relay_target(),
        },
    )
}

#[test]
#[ignore]
fn exact_base_abandoned_relay_cutover_is_atomic_and_one_time() -> Result<()> {
    require_ignored_flag()?;
    let base_wasm = pre_cutover_historian_wasm()?;
    let candidate_wasm = relay_enabled_historian_wasm()?;
    let base_hash = Sha256::digest(&base_wasm).to_vec();

    let (negative_pic, negative_historian) = prepare_pre_cutover_fixture(&base_wasm, true)?;
    let recovery_before = query_retired_recovery_view(&negative_pic, negative_historian)?;
    assert_eq!(recovery_before.target_canister_id, abandoned_relay_target());
    assert_eq!(
        recovery_before.status,
        RetiredRecoveryStatus::ManualRecoveryRequired
    );
    assert!(recovery_before.relay_canister_id.is_none());
    let failed_upgrade = negative_pic.upgrade_canister(
        negative_historian,
        candidate_wasm.clone(),
        encode_one(HistorianUpgradeArg {
            enable_sns_tracking: None,
            scan_interval_seconds: None,
            cycles_interval_seconds: None,
            min_tx_e8s: None,
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            cmc_canister_id: None,
            faucet_canister_id: None,
        })?,
        None,
    );
    assert!(
        failed_upgrade.is_err(),
        "invalid retired registry must reject upgrade"
    );
    let status_after_failure = negative_pic
        .canister_status(negative_historian, Some(Principal::anonymous()))
        .map_err(|error| anyhow!("status after rejected cutover: {error:?}"))?;
    assert_eq!(
        status_after_failure.module_hash.as_deref(),
        Some(base_hash.as_slice())
    );
    let recovery_after = query_retired_recovery_view(&negative_pic, negative_historian)?;
    assert_eq!(recovery_after.target_canister_id, abandoned_relay_target());
    assert_eq!(
        retired_and_current_setup_map_lengths(&negative_pic, negative_historian),
        (1, 0)
    );

    let (positive_pic, positive_historian) = prepare_pre_cutover_fixture(&base_wasm, false)?;
    let base_stable_snapshot = positive_pic.get_stable_memory(positive_historian);
    positive_pic.advance_time(Duration::from_secs(2));
    tick_n(&positive_pic, 5);
    positive_pic
        .upgrade_canister(
            positive_historian,
            candidate_wasm.clone(),
            encode_one(HistorianUpgradeArg {
                enable_sns_tracking: None,
                scan_interval_seconds: None,
                cycles_interval_seconds: None,
                min_tx_e8s: None,
                max_cycles_entries_per_canister: None,
                max_commitment_entries_per_canister: None,
                max_index_pages_per_tick: None,
                max_canisters_per_cycles_tick: None,
                sns_wasm_canister_id: None,
                xrc_canister_id: None,
                cmc_canister_id: None,
                faucet_canister_id: None,
            })?,
            None,
        )
        .map_err(|error| anyhow!("exact abandoned-job cutover failed: {error:?}"))?;
    tick_n(&positive_pic, 5);
    assert_eq!(
        retired_and_current_setup_map_lengths(&positive_pic, positive_historian),
        (0, 0)
    );
    let view: RelaySetupViewResult = query_one(
        &positive_pic,
        positive_historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        RelayTargetSetArgs {
            target_canister_ids: vec![abandoned_relay_target()],
        },
    )?;
    let RelaySetupViewResult::Ok(view) = view else {
        bail!("abandoned singleton target view was rejected after cutover")
    };
    assert_eq!(view.state, RelaySetupState::NotFunded);
    let new_setup_account = view.setup_account.context("fresh setup account missing")?;
    let legacy_setup_account = Account {
        owner: positive_historian,
        subaccount: Some(jupiter_ic_clients::account::relay_setup_subaccount(
            abandoned_relay_target(),
        )),
    };
    assert_ne!(new_setup_account, legacy_setup_account);
    let relay_targets: ListCanistersResponse = query_one(
        &positive_pic,
        positive_historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayTarget),
        },
    )?;
    assert!(relay_targets
        .items
        .iter()
        .all(|item| item.canister_id != abandoned_relay_target()));
    let public_counts_before: PublicCounts = query_one(
        &positive_pic,
        positive_historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;

    positive_pic.advance_time(Duration::from_secs(2));
    tick_n(&positive_pic, 5);
    positive_pic
        .upgrade_canister(
            positive_historian,
            candidate_wasm,
            encode_one(HistorianUpgradeArg {
                enable_sns_tracking: None,
                scan_interval_seconds: None,
                cycles_interval_seconds: None,
                min_tx_e8s: None,
                max_cycles_entries_per_canister: None,
                max_commitment_entries_per_canister: None,
                max_index_pages_per_tick: None,
                max_canisters_per_cycles_tick: None,
                sns_wasm_canister_id: None,
                xrc_canister_id: None,
                cmc_canister_id: None,
                faucet_canister_id: None,
            })?,
            None,
        )
        .map_err(|error| anyhow!("candidate-to-candidate upgrade failed: {error:?}"))?;
    assert_eq!(
        retired_and_current_setup_map_lengths(&positive_pic, positive_historian),
        (0, 0)
    );
    let public_counts_after: PublicCounts = query_one(
        &positive_pic,
        positive_historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(public_counts_after, public_counts_before);

    positive_pic.advance_time(Duration::from_secs(2));
    tick_n(&positive_pic, 5);
    positive_pic.set_stable_memory(
        positive_historian,
        base_stable_snapshot,
        pocket_ic::common::rest::BlobCompression::NoCompression,
    );
    positive_pic
        .upgrade_canister(
            positive_historian,
            base_wasm,
            encode_one(HistorianUpgradeArg {
                enable_sns_tracking: None,
                scan_interval_seconds: None,
                cycles_interval_seconds: None,
                min_tx_e8s: None,
                max_cycles_entries_per_canister: None,
                max_commitment_entries_per_canister: None,
                max_index_pages_per_tick: None,
                max_canisters_per_cycles_tick: None,
                sns_wasm_canister_id: None,
                xrc_canister_id: None,
                cmc_canister_id: None,
                faucet_canister_id: None,
            })?,
            None,
        )
        .map_err(|error| anyhow!("snapshot rollback to pre-cutover Historian failed: {error:?}"))?;
    let restored = query_retired_recovery_view(&positive_pic, positive_historian)?;
    assert_eq!(restored.target_canister_id, abandoned_relay_target());
    assert_eq!(
        retired_and_current_setup_map_lengths(&positive_pic, positive_historian),
        (1, 0)
    );
    Ok(())
}

#[test]
#[ignore]
fn multi_target_setup_blackholes_one_and_twenty_target_relays_and_survives_upgrade() -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let cmc = support::principals::cycles_minting_canister();
    let governance = support::principals::nns_governance();
    let thirteen = jupiter_ic_clients::constants::thirteen_node_blackhole_canister_id();
    let fiduciary = jupiter_ic_clients::constants::fiduciary_blackhole_canister_id();
    install_status_proxy(&pic, thirteen)?;
    create_fixed_canister(&pic, fiduciary)?;
    pic.add_cycles(fiduciary, 5_000_000_000_000);
    pic.install_canister(
        fiduciary,
        real_blackhole::real_blackhole_wasm()?,
        vec![],
        None,
    );
    set_controllers_exact(&pic, fiduciary, vec![fiduciary])?;
    create_fixed_canister(&pic, governance)?;
    pic.add_cycles(governance, 5_000_000_000_000);
    pic.install_canister(governance, nns_governance_wasm()?, vec![], None);

    let historian = pic.create_canister();
    pic.add_cycles(historian, 40_000_000_000_000);
    let targets = (0..20)
        .map(|_| {
            let target = pic.create_canister();
            pic.add_cycles(target, 5_000_000_000_000);
            set_controllers_exact(&pic, target, vec![thirteen])?;
            Ok(target)
        })
        .collect::<Result<Vec<_>>>()?;
    pic.install_canister(
        historian,
        relay_enabled_historian_wasm()?,
        encode_one(self_service_historian_init(ledger, index, cmc, fiduciary))?,
        None,
    );

    let counts_before: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;

    let singleton_relay = activate_target_set(&pic, historian, ledger, vec![targets[0]])?;
    let multi_relay = activate_target_set(&pic, historian, ledger, targets.clone())?;
    assert_ne!(singleton_relay, multi_relay);
    pic.advance_time(Duration::from_secs(2));
    tick_n(&pic, 30);
    assert_spawned_relay_faucet_identity(&pic, index, singleton_relay)?;
    assert_spawned_relay_faucet_identity(&pic, index, multi_relay)?;
    let multi_logs = pic
        .fetch_canister_logs(multi_relay, Principal::anonymous())
        .map_err(|error| anyhow!("fetch multi-target Relay logs failed: {error:?}"))?
        .iter()
        .map(|record| String::from_utf8_lossy(&record.content).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let canonical_targets = targets
        .iter()
        .map(Principal::to_text)
        .collect::<Vec<_>>()
        .join("|");
    let mut effective_targets = targets.clone();
    effective_targets.push(multi_relay);
    effective_targets.sort();
    effective_targets.dedup();
    let effective_targets = effective_targets
        .iter()
        .map(Principal::to_text)
        .collect::<Vec<_>>()
        .join("|");
    assert!(multi_logs.contains(&format!("managed_canisters={canonical_targets}")));
    assert!(multi_logs.contains(&format!("effective_managed_canisters={effective_targets}")));
    assert!(multi_logs.contains("max_transfers_per_tick=22"));
    for relay in [singleton_relay, multi_relay] {
        assert_eq!(pic.get_controllers(relay), vec![fiduciary]);
        let status = pic
            .canister_status(relay, Some(fiduciary))
            .map_err(|error| anyhow!("relay status failed: {error:?}"))?;
        assert!(status.module_hash.is_some());
    }

    let reversed: RelaySetupViewResult = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        RelayTargetSetArgs {
            target_canister_ids: targets.iter().rev().copied().collect(),
        },
    )?;
    let RelaySetupViewResult::Ok(reversed) = reversed else {
        bail!("reversed set rejected")
    };
    assert_eq!(reversed.setup_account, None);
    assert_eq!(
        reversed.state,
        RelaySetupState::Active {
            relay_canister_id: multi_relay
        }
    );

    let relays: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayInstance),
        },
    )?;
    assert!(relays
        .items
        .iter()
        .any(|item| item.canister_id == singleton_relay));
    assert!(relays
        .items
        .iter()
        .any(|item| item.canister_id == multi_relay));
    let counts_after_setup: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(
        counts_after_setup.relay_target_canister_count,
        counts_before.relay_target_canister_count + 20
    );
    assert_eq!(
        counts_after_setup.relay_instance_canister_count,
        counts_before.relay_instance_canister_count + 2
    );

    upgrade_historian_without_config_changes(&pic, historian)?;
    let after_upgrade: RelaySetupViewResult = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        RelayTargetSetArgs {
            target_canister_ids: targets.clone(),
        },
    )?;
    let RelaySetupViewResult::Ok(after_upgrade) = after_upgrade else {
        bail!("upgrade lost setup")
    };
    assert_eq!(
        after_upgrade.state,
        RelaySetupState::Active {
            relay_canister_id: multi_relay
        }
    );
    let counts_after: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(
        counts_after.relay_target_canister_count,
        counts_after_setup.relay_target_canister_count
    );
    assert_eq!(
        counts_after.relay_instance_canister_count,
        counts_after_setup.relay_instance_canister_count
    );
    let relay_instances_after_upgrade: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayInstance),
        },
    )?;
    assert!(relay_instances_after_upgrade
        .items
        .iter()
        .any(|item| item.canister_id == singleton_relay));
    assert!(relay_instances_after_upgrade
        .items
        .iter()
        .any(|item| item.canister_id == multi_relay));
    let relay_targets_after_upgrade: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayTarget),
        },
    )?;
    for target in targets {
        assert!(relay_targets_after_upgrade
            .items
            .iter()
            .any(|item| item.canister_id == target));
    }
    Ok(())
}

#[test]
#[ignore]
fn active_setup_survives_target_becoming_configured_cmc_without_external_work() -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let cmc = support::principals::cycles_minting_canister();
    let governance = support::principals::nns_governance();
    let thirteen = jupiter_ic_clients::constants::thirteen_node_blackhole_canister_id();
    let fiduciary = jupiter_ic_clients::constants::fiduciary_blackhole_canister_id();
    install_status_proxy(&pic, thirteen)?;
    create_fixed_canister(&pic, fiduciary)?;
    pic.add_cycles(fiduciary, 5_000_000_000_000);
    pic.install_canister(
        fiduciary,
        real_blackhole::real_blackhole_wasm()?,
        vec![],
        None,
    );
    set_controllers_exact(&pic, fiduciary, vec![fiduciary])?;
    create_fixed_canister(&pic, governance)?;
    pic.add_cycles(governance, 5_000_000_000_000);
    pic.install_canister(governance, nns_governance_wasm()?, vec![], None);

    let target = pic.create_canister();
    pic.add_cycles(target, 5_000_000_000_000);
    set_controllers_exact(&pic, target, vec![thirteen])?;
    let historian = pic.create_canister();
    pic.add_cycles(historian, 40_000_000_000_000);
    pic.install_canister(
        historian,
        relay_enabled_historian_wasm()?,
        encode_one(self_service_historian_init(ledger, index, cmc, fiduciary))?,
        None,
    );

    let relay = activate_target_set(&pic, historian, ledger, vec![target])?;
    let counts_before: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    let relay_targets_before: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayTarget),
        },
    )?;
    let relay_instances_before: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayInstance),
        },
    )?;

    pic.upgrade_canister(
        historian,
        relay_enabled_historian_wasm()?,
        encode_one(HistorianUpgradeArg {
            enable_sns_tracking: None,
            scan_interval_seconds: None,
            cycles_interval_seconds: None,
            min_tx_e8s: None,
            max_cycles_entries_per_canister: None,
            max_commitment_entries_per_canister: None,
            max_index_pages_per_tick: None,
            max_canisters_per_cycles_tick: None,
            sns_wasm_canister_id: None,
            xrc_canister_id: None,
            cmc_canister_id: Some(target),
            faucet_canister_id: None,
        })?,
        None,
    )
    .map_err(|error| anyhow!("Historian policy-change upgrade failed: {error:?}"))?;
    tick_n(&pic, 10);

    let view: RelaySetupViewResult = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        RelayTargetSetArgs {
            target_canister_ids: vec![target],
        },
    )?;
    assert!(matches!(
        view,
        RelaySetupViewResult::Ok(RelaySetupView {
            state: RelaySetupState::Active { relay_canister_id },
            ..
        }) if relay_canister_id == relay
    ));
    let notify: RelaySetupNotifyResult = update_one(
        &pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        RelayTargetSetArgs {
            target_canister_ids: vec![target],
        },
    )?;
    assert!(matches!(
        notify,
        RelaySetupNotifyResult::Active { relay_canister_id } if relay_canister_id == relay
    ));

    let counts_after: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts_after, counts_before);
    let relay_targets_after: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayTarget),
        },
    )?;
    let relay_instances_after: ListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(CanisterTrackingReason::RelayInstance),
        },
    )?;
    assert_eq!(relay_targets_after.items, relay_targets_before.items);
    assert_eq!(
        relay_targets_after.next_start_after,
        relay_targets_before.next_start_after
    );
    assert_eq!(relay_instances_after.items, relay_instances_before.items);
    assert_eq!(
        relay_instances_after.next_start_after,
        relay_instances_before.next_start_after
    );
    assert!(relay_instances_after
        .items
        .iter()
        .any(|item| item.canister_id == relay));
    Ok(())
}

fn upgrade_historian_without_config_changes(pic: &PocketIc, historian: Principal) -> Result<()> {
    let args = HistorianUpgradeArg {
        enable_sns_tracking: None,
        scan_interval_seconds: None,
        cycles_interval_seconds: None,
        min_tx_e8s: None,
        max_cycles_entries_per_canister: None,
        max_commitment_entries_per_canister: None,
        max_index_pages_per_tick: None,
        max_canisters_per_cycles_tick: None,
        sns_wasm_canister_id: None,
        xrc_canister_id: None,
        cmc_canister_id: None,
        faucet_canister_id: None,
    };
    pic.upgrade_canister(
        historian,
        relay_enabled_historian_wasm()?,
        encode_one(args)?,
        None,
    )
    .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;
    tick_n(pic, 10);
    Ok(())
}

struct Harness {
    pic: PocketIc,
    index: Principal,
    blackhole: Principal,
    sns_wasm: Principal,
    historian: Principal,
}

impl Harness {
    fn new(enable_sns_tracking: bool) -> Result<Self> {
        let pic = support::pocketic::builder()
            .with_application_subnet()
            .build();
        let index = pic.create_canister();
        let blackhole = pic.create_canister();
        let sns_wasm = pic.create_canister();
        let cmc = pic.create_canister();
        let xrc = pic.create_canister();
        let historian = pic.create_canister();
        for canister in [index, blackhole, sns_wasm, cmc, xrc, historian] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(index, index_wasm()?, vec![], None);
        pic.install_canister(
            blackhole,
            real_blackhole::real_blackhole_wasm()?,
            vec![],
            None,
        );
        set_controllers_exact(&pic, blackhole, vec![blackhole])?;
        pic.install_canister(sns_wasm, sns_wasm_wasm()?, vec![], None);
        pic.install_canister(xrc, xrc_wasm()?, vec![], None);

        let staking_account = Account {
            owner: Principal::management_canister(),
            subaccount: Some([9u8; 32]),
        };
        let init = HistorianInitArg {
            staking_account,
            output_source_account: None,
            output_account: None,
            rewards_account: None,
            ledger_canister_id: Some(index),
            index_canister_id: Some(index),
            cmc_canister_id: Some(cmc),
            faucet_canister_id: Some(blackhole),
            sns_wasm_canister_id: Some(sns_wasm),
            xrc_canister_id: Some(xrc),
            enable_sns_tracking: Some(enable_sns_tracking),
            scan_interval_seconds: Some(60),
            cycles_interval_seconds: Some(1),
            min_tx_e8s: Some(10_000_000),
            max_cycles_entries_per_canister: Some(100),
            max_commitment_entries_per_canister: Some(100),
            max_index_pages_per_tick: Some(10),
            max_canisters_per_cycles_tick: Some(10),
            relay_factory_enabled: Some(false),
            relay_setup_min_e8s: None,
            relay_initial_cycles: None,
            relay_cycle_safety_margin_e8s: None,
            relay_min_subaccount_one_seed_e8s: None,
            self_service_relay_interval_seconds: None,
            io_surplus_neuron_id: None,
            canonical_relay_canister_id: None,
            canonical_relay_targets: Some(Vec::new()),
        };
        pic.install_canister(historian, historian_wasm()?, encode_one(init)?, None);
        Ok(Self {
            pic,
            index,
            blackhole,
            sns_wasm,
            historian,
        })
    }

    fn staking_identifier(&self) -> Result<String> {
        let account = Account {
            owner: Principal::management_canister(),
            subaccount: Some([9u8; 32]),
        };
        Ok(account_identifier_text(account.owner, account.subaccount))
    }

    fn tick(&self) {
        self.pic.advance_time(Duration::from_secs(2));
        tick_n(&self.pic, 5);
    }
}

#[test]
#[ignore]
fn real_icp_index_returns_newest_first_for_account_history() -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let staking_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some([9u8; 32]),
    };
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
    let fee_e8s = icrc1_fee(&pic, ledger)?;

    for ordinal in 0..3u64 {
        let memo_text = format!("real-index-ordering-{ordinal}");
        let _block_index = icrc1_transfer(
            &pic,
            ledger,
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: staking_account,
                fee: Some(Nat::from(fee_e8s)),
                created_at_time: None,
                memo: Some(Memo::from(memo_text.into_bytes())),
                amount: Nat::from(100_000_000u64 + ordinal),
            },
        )?;
        pic.advance_time(Duration::from_secs(1));
        tick_n(&pic, 3);
    }

    let page = wait_for_index_transactions(&pic, index, &staking_id, 3)?;
    let ids: Vec<u64> = page.transactions.iter().map(|tx| tx.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(
        ids.windows(2).all(|window| window[0] > window[1]),
        "expected real ICP index account history to be newest-first, got ids {ids:?}"
    );
    Ok(())
}

#[test]
#[ignore]
fn real_icp_index_pagination_excludes_start_boundary_when_walking_older_history() -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let staking_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some([7u8; 32]),
    };
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
    let fee_e8s = icrc1_fee(&pic, ledger)?;

    for ordinal in 0..4u64 {
        let memo_text = format!("real-index-pagination-{ordinal}");
        let _block_index = icrc1_transfer(
            &pic,
            ledger,
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: staking_account,
                fee: Some(Nat::from(fee_e8s)),
                created_at_time: None,
                memo: Some(Memo::from(memo_text.into_bytes())),
                amount: Nat::from(100_000_100u64 + ordinal),
            },
        )?;
        pic.advance_time(Duration::from_secs(1));
        tick_n(&pic, 3);
    }

    let first_page = wait_for_index_transactions(&pic, index, &staking_id, 4)?;
    let first_ids: Vec<u64> = first_page.transactions.iter().map(|tx| tx.id).collect();
    assert!(
        first_ids.len() >= 3,
        "expected at least three transactions to characterize pagination, got {first_ids:?}"
    );
    let boundary = *first_ids.get(1).expect("at least two ids");

    let second_page =
        index_account_transactions(&pic, index, staking_id.clone(), Some(boundary), 3)?;
    let second_ids: Vec<u64> = second_page.transactions.iter().map(|tx| tx.id).collect();
    assert!(
        !second_ids.is_empty(),
        "expected second page when querying real ICP index from boundary {boundary}"
    );
    assert!(second_ids[0] < boundary, "expected real ICP index pagination to exclude the start boundary and continue with older tx ids, first page ids={first_ids:?}, second page ids={second_ids:?}");
    assert!(
        second_ids.windows(2).all(|window| window[0] > window[1]),
        "expected second page to stay newest-first, got ids {second_ids:?}"
    );
    Ok(())
}

#[test]
#[ignore]
fn gzip_install_payload_module_hash_matches_exact_supplied_bytes() -> Result<()> {
    require_ignored_flag()?;
    let relay = relay_wasm()?;
    let unique = format!(
        "jupiter-relay-module-hash-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let raw_path = std::env::temp_dir().join(format!("{unique}.wasm"));
    let gzip_path = std::env::temp_dir().join(format!("{unique}.wasm.gz"));
    std::fs::write(&raw_path, relay)?;
    let gzip_status = Command::new("gzip")
        .args(["-n", "-9", "-c"])
        .arg(&raw_path)
        .output()
        .context("spawn deterministic gzip for relay semantic module-hash test")?;
    if !gzip_status.status.success() {
        let _ = std::fs::remove_file(&raw_path);
        bail!(
            "gzip failed for relay semantic module-hash test: {}",
            String::from_utf8_lossy(&gzip_status.stderr)
        );
    }
    std::fs::write(&gzip_path, gzip_status.stdout)?;
    let payload =
        std::fs::read(&gzip_path).with_context(|| format!("read {}", gzip_path.display()))?;
    let expected_hash = Sha256::digest(&payload).to_vec();

    let result = (|| {
        let pic = support::pocketic::builder()
            .with_application_subnet()
            .build();
        let relay = pic.create_canister();
        pic.add_cycles(relay, 10_000_000_000_000);
        #[derive(CandidType)]
        struct MinimalRelaySurplusNeuronRecipient {
            neuron_id: u64,
            memo: Option<Vec<u8>>,
        }
        #[derive(CandidType)]
        struct MinimalRelayInitArg {
            managed_canisters: Vec<Principal>,
            ledger_canister_id: Option<Principal>,
            cmc_canister_id: Option<Principal>,
            governance_canister_id: Option<Principal>,
            blackhole_canister_id: Option<Principal>,
            surplus_neuron_recipients: Vec<MinimalRelaySurplusNeuronRecipient>,
        }
        let relay_args = MinimalRelayInitArg {
            managed_canisters: Vec::new(),
            ledger_canister_id: None,
            cmc_canister_id: None,
            governance_canister_id: None,
            blackhole_canister_id: None,
            surplus_neuron_recipients: Vec::new(),
        };
        pic.install_canister(relay, payload, encode_one(relay_args)?, None);

        let status = pic
            .canister_status(relay, Some(Principal::anonymous()))
            .map_err(|err| anyhow!("relay canister_status failed: {err:?}"))?;
        if status.module_hash.as_deref() != Some(expected_hash.as_slice()) {
            bail!(
                "PocketIC should report the module hash for the exact gzip bytes supplied to install_canister: expected {}, got {:?}",
                hex::encode(&expected_hash),
                status.module_hash.map(hex::encode)
            );
        }
        Ok(())
    })();
    let remove_raw =
        std::fs::remove_file(&raw_path).with_context(|| format!("remove {}", raw_path.display()));
    let remove_gzip =
        std::fs::remove_file(&gzip_path).with_context(|| format!("remove {}", gzip_path.display()));
    result?;
    remove_raw?;
    remove_gzip?;
    Ok(())
}

#[test]
#[ignore]
fn canonical_sns_wasm_mock_is_installed_on_nns_subnet() -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::sns_topology_builder().build();
    let topology = pic.topology();
    let nns_subnet = topology.get_nns().context("NNS subnet missing")?;
    let sns_subnet = topology.get_sns().context("SNS subnet missing")?;
    let app_subnet = topology
        .get_app_subnets()
        .into_iter()
        .next()
        .context("application subnet missing")?;
    assert_ne!(nns_subnet, sns_subnet);
    assert_ne!(nns_subnet, app_subnet);
    assert_ne!(sns_subnet, app_subnet);

    let sns_wasm_id = jupiter_ic_clients::constants::sns_wasm_id();
    assert_eq!(sns_wasm_id.to_text(), "qaa6y-5yaaa-aaaaa-aaafa-cai");
    let created = pic
        .create_canister_with_id(None, None, sns_wasm_id)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(created, sns_wasm_id);
    assert_eq!(pic.get_subnet(sns_wasm_id), Some(nns_subnet));
    pic.add_cycles(sns_wasm_id, 5_000_000_000_000);
    pic.install_canister(sns_wasm_id, sns_wasm_wasm()?, vec![], None);

    let root = pic.create_canister_on_subnet(None, None, sns_subnet);
    let _: () = update_one(
        &pic,
        sns_wasm_id,
        Principal::anonymous(),
        "debug_set_roots",
        vec![root],
    )?;
    let response: ListDeployedSnsesResponse = update_one(
        &pic,
        sns_wasm_id,
        Principal::anonymous(),
        "list_deployed_snses",
        ListDeployedSnsesArgs::default(),
    )?;
    assert_eq!(response.instances.len(), 1);
    assert_eq!(response.instances[0].root_canister_id, Some(root));
    Ok(())
}

#[test]
#[ignore]
fn sns_root_proxy_reads_real_application_dapp_status_cross_subnet() -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::sns_topology_builder().build();
    let topology = pic.topology();
    let sns_subnet = topology.get_sns().context("SNS subnet missing")?;
    let app_subnet = topology
        .get_app_subnets()
        .into_iter()
        .next()
        .context("application subnet missing")?;

    let root = pic.create_canister_on_subnet(None, None, sns_subnet);
    let other_root = pic.create_canister_on_subnet(None, None, sns_subnet);
    let target = pic.create_canister_on_subnet(None, None, app_subnet);
    for canister in [root, other_root, target] {
        pic.add_cycles(canister, 5_000_000_000_000);
    }
    pic.install_canister(root, sns_root_wasm()?, vec![], None);
    pic.install_canister(other_root, sns_root_wasm()?, vec![], None);
    pic.install_canister(target, cycle_burner_wasm()?, vec![], None);
    set_controllers_exact(&pic, target, vec![root])?;
    let _: () = update_one(
        &pic,
        root,
        Principal::anonymous(),
        "debug_set_canisters",
        ListSnsCanistersResponse {
            root: Some(root),
            dapps: vec![target],
            ..Default::default()
        },
    )?;

    assert_eq!(pic.get_subnet(root), Some(sns_subnet));
    assert_eq!(pic.get_subnet(target), Some(app_subnet));
    assert_eq!(pic.get_controllers(target), vec![root]);
    let observed: SnsRootCanisterStatusResult = update_one(
        &pic,
        root,
        Principal::anonymous(),
        "canister_status",
        SnsRootCanisterStatusArgs {
            canister_id: target,
        },
    )?;
    assert_eq!(observed.cycles, Nat::from(pic.cycle_balance(target)));

    let denied = update_one::<_, SnsRootCanisterStatusResult>(
        &pic,
        other_root,
        Principal::anonymous(),
        "canister_status",
        SnsRootCanisterStatusArgs {
            canister_id: target,
        },
    );
    assert!(
        denied.is_err(),
        "unrelated SNS Root must not read target canister_status"
    );
    Ok(())
}

#[test]
#[ignore]
fn historian_with_real_icp_index_resumes_from_cursor_without_latching_non_monotonic_fault(
) -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let blackhole = pic.create_canister();
    let sns_wasm = pic.create_canister();
    let cmc = pic.create_canister();
    let xrc = pic.create_canister();
    let historian = pic.create_canister();
    for canister in [blackhole, sns_wasm, cmc, xrc, historian] {
        pic.add_cycles(canister, 5_000_000_000_000);
    }
    pic.install_canister(
        blackhole,
        real_blackhole::real_blackhole_wasm()?,
        vec![],
        None,
    );
    set_controllers_exact(&pic, blackhole, vec![blackhole])?;
    pic.install_canister(sns_wasm, sns_wasm_wasm()?, vec![], None);
    pic.install_canister(xrc, xrc_wasm()?, vec![], None);

    let staking_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some([6u8; 32]),
    };
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
    let init = HistorianInitArg {
        staking_account,
        output_source_account: None,
        output_account: None,
        rewards_account: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        faucet_canister_id: Some(blackhole),
        sns_wasm_canister_id: Some(sns_wasm),
        xrc_canister_id: Some(xrc),
        enable_sns_tracking: Some(false),
        scan_interval_seconds: Some(60),
        cycles_interval_seconds: Some(1),
        min_tx_e8s: Some(10_000_000),
        max_cycles_entries_per_canister: Some(100),
        max_commitment_entries_per_canister: Some(100),
        max_index_pages_per_tick: Some(10),
        max_canisters_per_cycles_tick: Some(10),
        relay_factory_enabled: None,
        relay_setup_min_e8s: None,
        relay_initial_cycles: None,
        relay_cycle_safety_margin_e8s: None,
        relay_min_subaccount_one_seed_e8s: None,
        self_service_relay_interval_seconds: None,
        io_surplus_neuron_id: None,
        canonical_relay_canister_id: None,
        canonical_relay_targets: Some(Vec::new()),
    };
    pic.install_canister(historian, historian_wasm()?, encode_one(init)?, None);

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    for ordinal in 0..3u64 {
        let memo_text = blackhole.to_text();
        let _block_index = icrc1_transfer(
            &pic,
            ledger,
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: staking_account,
                fee: Some(Nat::from(fee_e8s)),
                created_at_time: None,
                memo: Some(Memo::from(memo_text.clone().into_bytes())),
                amount: Nat::from(100_001_000u64 + ordinal),
            },
        )?;
        pic.advance_time(Duration::from_secs(1));
        tick_n(&pic, 3);
    }

    let page = wait_for_index_transactions(&pic, index, &staking_id, 3)?;
    let ids: Vec<u64> = page.transactions.iter().map(|tx| tx.id).collect();
    assert_eq!(ids.len(), 3, "expected three real ICP index transactions for the dedicated staking account, got ids {ids:?}");
    let resume_cursor = ids[1];
    let expected_older_tx_id = *ids
        .last()
        .expect("dedicated staking account should have an oldest tx id");

    let _: () = update_one(
        &pic,
        historian,
        Principal::anonymous(),
        "debug_set_last_indexed_staking_tx_id",
        Some(resume_cursor),
    )?;
    let _: () = update_noargs(&pic, historian, Principal::anonymous(), "debug_driver_tick")?;

    let status: PublicStatus = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_status",
        (),
    )?;
    assert!(status.commitment_index_fault.is_none(), "historian should continue indexing older real-ICP-index pages from cursor {resume_cursor} without latching a non-monotonic fault; fault={:?}", status.commitment_index_fault);

    let history: CommitmentHistoryPage = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_commitment_history",
        GetCommitmentHistoryArgs {
            canister_id: blackhole,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let recorded_ids: Vec<u64> = history.items.iter().map(|item| item.tx_id).collect();
    assert!(recorded_ids.contains(&expected_older_tx_id), "historian should record the older tx from the real ICP index page reached via cursor {resume_cursor}; expected tx {expected_older_tx_id}, recorded ids {recorded_ids:?}");
    Ok(())
}

#[test]
#[ignore]
fn historian_route_indexing_with_real_icp_index_counts_descending_route_pages_without_stalling(
) -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let blackhole = pic.create_canister();
    let sns_wasm = pic.create_canister();
    let cmc = pic.create_canister();
    let xrc = pic.create_canister();
    let historian = pic.create_canister();
    for canister in [blackhole, sns_wasm, cmc, xrc, historian] {
        pic.add_cycles(canister, 5_000_000_000_000);
    }
    pic.install_canister(
        blackhole,
        real_blackhole::real_blackhole_wasm()?,
        vec![],
        None,
    );
    set_controllers_exact(&pic, blackhole, vec![blackhole])?;
    pic.install_canister(sns_wasm, sns_wasm_wasm()?, vec![], None);
    pic.install_canister(xrc, xrc_wasm()?, vec![], None);

    let source_subaccount = [31u8; 32];
    let output_subaccount = [32u8; 32];
    let rewards_subaccount = [33u8; 32];
    let source_owner = blackhole;
    let output_source_account = Account {
        owner: source_owner,
        subaccount: Some(source_subaccount),
    };
    let output_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some(output_subaccount),
    };
    let rewards_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some(rewards_subaccount),
    };
    let output_id = account_identifier_text(output_account.owner, output_account.subaccount);
    let rewards_id = account_identifier_text(rewards_account.owner, rewards_account.subaccount);
    let fee_e8s = icrc1_fee(&pic, ledger)?;

    icrc1_transfer(
        &pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: output_source_account,
            fee: Some(Nat::from(fee_e8s)),
            created_at_time: None,
            memo: Some(Memo::from(b"fund-output-source".to_vec())),
            amount: Nat::from(1_000_000_000u64),
        },
    )?;
    pic.advance_time(Duration::from_secs(1));
    tick_n(&pic, 5);

    let mut expected_output = 0u64;
    let mut expected_rewards = 0u64;
    for ordinal in 0..3u64 {
        let amount = 100_000_000u64 + ordinal;
        expected_output = expected_output.saturating_add(amount);
        icrc1_transfer(
            &pic,
            ledger,
            source_owner,
            TransferArg {
                from_subaccount: Some(source_subaccount),
                to: output_account,
                fee: Some(Nat::from(fee_e8s)),
                created_at_time: None,
                memo: Some(Memo::from(
                    format!("real-route-output-{ordinal}").into_bytes(),
                )),
                amount: Nat::from(amount),
            },
        )?;
        pic.advance_time(Duration::from_secs(1));
        tick_n(&pic, 3);
    }
    for ordinal in 0..3u64 {
        let amount = 50_000_000u64 + ordinal;
        expected_rewards = expected_rewards.saturating_add(amount);
        icrc1_transfer(
            &pic,
            ledger,
            source_owner,
            TransferArg {
                from_subaccount: Some(source_subaccount),
                to: rewards_account,
                fee: Some(Nat::from(fee_e8s)),
                created_at_time: None,
                memo: Some(Memo::from(
                    format!("real-route-rewards-{ordinal}").into_bytes(),
                )),
                amount: Nat::from(amount),
            },
        )?;
        pic.advance_time(Duration::from_secs(1));
        tick_n(&pic, 3);
    }

    wait_for_index_transactions(&pic, index, &output_id, 3)?;
    wait_for_index_transactions(&pic, index, &rewards_id, 3)?;

    let staking_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some([34u8; 32]),
    };
    let init = HistorianInitArg {
        staking_account,
        output_source_account: Some(output_source_account),
        output_account: Some(output_account),
        rewards_account: Some(rewards_account),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        faucet_canister_id: Some(blackhole),
        sns_wasm_canister_id: Some(sns_wasm),
        xrc_canister_id: Some(xrc),
        enable_sns_tracking: Some(false),
        scan_interval_seconds: Some(60),
        cycles_interval_seconds: Some(1),
        min_tx_e8s: Some(10_000_000),
        max_cycles_entries_per_canister: Some(100),
        max_commitment_entries_per_canister: Some(100),
        max_index_pages_per_tick: Some(10),
        max_canisters_per_cycles_tick: Some(10),
        relay_factory_enabled: None,
        relay_setup_min_e8s: None,
        relay_initial_cycles: None,
        relay_cycle_safety_margin_e8s: None,
        relay_min_subaccount_one_seed_e8s: None,
        self_service_relay_interval_seconds: None,
        io_surplus_neuron_id: None,
        canonical_relay_canister_id: None,
        canonical_relay_targets: Some(Vec::new()),
    };
    pic.install_canister(historian, historian_wasm()?, encode_one(init)?, None);

    let _: () = update_noargs(&pic, historian, Principal::anonymous(), "debug_driver_tick")?;
    let counts_after_output: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts_after_output.total_output_e8s, expected_output, "historian should finish the output route's real-index newest-first page instead of stalling after the first descending tx");

    let _: () = update_noargs(&pic, historian, Principal::anonymous(), "debug_driver_tick")?;
    let counts: PublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.total_output_e8s, expected_output, "output route totals should include every real-index descending-page transfer from the source account");
    assert_eq!(counts.total_rewards_e8s, expected_rewards, "rewards route totals should include every real-index descending-page transfer from the source account");
    Ok(())
}

#[test]
#[ignore]
fn historian_keeps_under_threshold_commitments_out_of_durable_tracking() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = Principal::from_slice(&[1]);
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            5_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let st: DebugState = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_state",
        (),
    )?;
    assert_eq!(st.distinct_canister_count, 0);
    assert_eq!(st.last_indexed_staking_tx_id, Some(1));

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 0);

    let canisters: ListCanistersResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(10),
            tracking_reason_filter: None,
        },
    )?;
    assert!(canisters.items.is_empty());

    let registered: ListMemoRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        ListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    assert_eq!(registered.total, 0);
    assert!(registered.items.is_empty());

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, Some(target));
    assert!(!recent.items[0].counts_toward_faucet);

    let cycles: CyclesHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_cycles_history",
        GetCyclesHistoryArgs {
            canister_id: target,
            start_after_ts: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert!(cycles.items.is_empty());
    Ok(())
}

#[test]
#[ignore]
fn historian_ignores_missing_icrc1_memo_even_when_legacy_numeric_memo_exists() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer_with_numeric_memo",
        encode_args((staking_id, 100_000_000u64, 0x61616161612d6161u64))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 0);

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert!(recent.items.is_empty());
    Ok(())
}

#[test]
#[ignore]
fn historian_accepts_short_valid_principal_text_without_hardcoded_suffix() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    let target = Principal::from_slice(&[1]);
    let target_text = target.to_text();
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(target_text.clone().into_bytes()),
        ))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 1);
    assert_eq!(counts.qualifying_commitment_count, 1);

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, Some(target));
    assert_eq!(
        recent.items[0].memo_text.as_deref(),
        Some(target_text.as_str())
    );
    assert!(recent.items[0].counts_toward_faucet);
    Ok(())
}

#[test]
#[ignore]
fn historian_indexes_raw_icp_directive_with_empty_transfer_memo() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    let target = Principal::from_slice(&[1]);
    let raw_directive = format!("{}.", target.to_text().replace('-', ""));
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(raw_directive.clone().into_bytes()),
        ))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 1);

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, Some(target));
    assert_eq!(recent.items[0].raw_icp_memo_text.as_deref(), Some(""));
    assert_eq!(
        recent.items[0].memo_text.as_deref(),
        Some(target.to_text().as_str())
    );
    assert!(recent.items[0].counts_toward_faucet);
    Ok(())
}

#[test]
#[ignore]
fn historian_indexes_numeric_neuron_id_commitment_without_registering_canister() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    let neuron_id = 11_614_578_985_374_291_210_u64;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(neuron_id.to_string().into_bytes()),
        ))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 1);

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, None);
    assert_eq!(recent.items[0].neuron_id, Some(neuron_id));
    assert_eq!(recent.items[0].raw_icp_memo_text, None);
    assert_eq!(recent.items[0].neuron_memo_text, None);
    assert_eq!(
        recent.items[0].memo_text.as_deref(),
        Some("11614578985374291210")
    );
    assert!(recent.items[0].counts_toward_faucet);
    Ok(())
}

#[test]
#[ignore]
fn historian_indexes_dotted_neuron_id_commitment_with_right_memo_segment() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    let neuron_id = 42_u64;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id, 100_000_000u64, Some(b"42.vault.memo".to_vec())))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 1);

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, None);
    assert_eq!(recent.items[0].neuron_id, Some(neuron_id));
    assert_eq!(recent.items[0].raw_icp_memo_text, None);
    assert_eq!(
        recent.items[0].neuron_memo_text.as_deref(),
        Some("vault.memo")
    );
    assert_eq!(recent.items[0].memo_text.as_deref(), Some("42"));
    assert!(recent.items[0].counts_toward_faucet);
    Ok(())
}

#[test]
#[ignore]
fn historian_rejects_reserved_principal_memos_from_durable_tracking() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    for reserved in [Principal::anonymous(), Principal::management_canister()] {
        let _: u64 = update_bytes(
            &h.pic,
            h.index,
            Principal::anonymous(),
            "debug_append_transfer",
            encode_args((
                staking_id.clone(),
                100_000_000u64,
                Some(reserved.to_text().into_bytes()),
            ))?,
        )?;
    }

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let st: DebugState = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_state",
        (),
    )?;
    assert_eq!(st.distinct_canister_count, 0);
    assert_eq!(st.last_indexed_staking_tx_id, Some(2));

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 0);

    let canisters: ListCanistersResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(10),
            tracking_reason_filter: None,
        },
    )?;
    assert!(canisters.items.is_empty());

    let registered: ListMemoRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        ListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    assert_eq!(registered.total, 0);
    assert!(registered.items.is_empty());

    for reserved in [Principal::anonymous(), Principal::management_canister()] {
        let overview: Option<CanisterOverview> = query_one(
            &h.pic,
            h.historian,
            Principal::anonymous(),
            "get_canister_overview",
            reserved,
        )?;
        assert!(
            overview.is_none(),
            "reserved principal {reserved} must not surface a public overview"
        );

        let commitments: CommitmentHistoryPage = query_one(
            &h.pic,
            h.historian,
            Principal::anonymous(),
            "get_commitment_history",
            GetCommitmentHistoryArgs {
                canister_id: reserved,
                start_after_tx_id: None,
                limit: Some(10),
                descending: Some(false),
            },
        )?;
        assert!(
            commitments.items.is_empty(),
            "reserved principal {reserved} must not gain commitment history"
        );

        let cycles: CyclesHistoryPage = query_one(
            &h.pic,
            h.historian,
            Principal::anonymous(),
            "get_cycles_history",
            GetCyclesHistoryArgs {
                canister_id: reserved,
                start_after_ts: None,
                limit: Some(10),
                descending: Some(false),
            },
        )?;
        assert!(
            cycles.items.is_empty(),
            "reserved principal {reserved} must not gain cycles history"
        );
    }

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 2);
    assert!(recent.items.iter().all(|item| item.canister_id.is_none()));
    assert!(recent.items.iter().all(|item| !item.counts_toward_faucet));
    assert!(recent
        .items
        .iter()
        .all(|item| item.memo_text.as_deref() == Some("invalid declared memo")));

    Ok(())
}

#[test]
#[ignore]
fn historian_indexes_commitments_and_blackhole_cycles() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.historian;
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            42_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let st: DebugState = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_state",
        (),
    )?;
    assert_eq!(st.distinct_canister_count, 1);
    assert_eq!(st.last_indexed_staking_tx_id, Some(1));

    let canisters: ListCanistersResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(10),
            tracking_reason_filter: None,
        },
    )?;
    assert_eq!(canisters.items.len(), 1);
    assert_eq!(canisters.items[0].canister_id, target);
    assert_eq!(
        canisters.items[0].tracking_reasons,
        vec![CanisterTrackingReason::MemoCommitment]
    );

    let commitments: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_commitment_history",
        GetCommitmentHistoryArgs {
            canister_id: target,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert_eq!(commitments.items.len(), 1);
    assert_eq!(commitments.items[0].tx_id, 1);
    assert!(commitments.items[0].counts_toward_faucet);

    let cycles: CyclesHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_cycles_history",
        GetCyclesHistoryArgs {
            canister_id: target,
            start_after_ts: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert_eq!(cycles.items.len(), 1);
    assert!(cycles.items[0].cycles > 0);
    assert!(matches!(
        cycles.items[0].source,
        CyclesSampleSource::SelfCanister
    ));
    Ok(())
}

#[test]
#[ignore]
fn historian_discovers_sns_canisters_and_records_summary_cycles() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(true)?;
    let sns_root = h.pic.create_canister();
    h.pic.add_cycles(sns_root, 5_000_000_000_000);
    h.pic
        .install_canister(sns_root, sns_root_wasm()?, vec![], None);

    let governance = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
    let dapp = Principal::from_text("qjdve-lqaaa-aaaaa-aaaeq-cai")?;
    let archive = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai")?;

    let summary = GetSnsCanistersSummaryResponse {
        root: Some(SnsCanisterSummary {
            canister_id: Some(sns_root),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(1000u64)),
            }),
        }),
        governance: Some(SnsCanisterSummary {
            canister_id: Some(governance),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(2000u64)),
            }),
        }),
        ledger: None,
        swap: None,
        index: None,
        dapps: vec![SnsCanisterSummary {
            canister_id: Some(dapp),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(3000u64)),
            }),
        }],
        archives: vec![SnsCanisterSummary {
            canister_id: Some(archive),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(4000u64)),
            }),
        }],
    };
    let _: () = update_one(
        &h.pic,
        sns_root,
        Principal::anonymous(),
        "debug_set_summary",
        summary,
    )?;
    let _: () = update_one(
        &h.pic,
        h.sns_wasm,
        Principal::anonymous(),
        "debug_set_roots",
        vec![sns_root],
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let canisters: ListCanistersResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(10),
            tracking_reason_filter: Some(CanisterTrackingReason::SnsDiscovery),
        },
    )?;
    let ids: Vec<_> = canisters.items.iter().map(|i| i.canister_id).collect();
    assert!(ids.contains(&sns_root));
    assert!(ids.contains(&governance));
    assert!(ids.contains(&dapp));
    assert!(ids.contains(&archive));

    let dapp_cycles: CyclesHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_cycles_history",
        GetCyclesHistoryArgs {
            canister_id: dapp,
            start_after_ts: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert_eq!(dapp_cycles.items.len(), 1);
    assert_eq!(dapp_cycles.items[0].cycles, 3000u128);
    assert!(matches!(
        dapp_cycles.items[0].source,
        CyclesSampleSource::SnsRootSummary
    ));
    Ok(())
}

#[test]
#[ignore]
fn historian_upgrade_preserves_histories() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.historian;
    let raw_target = h.blackhole;
    let neuron_id = 42_u64;
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id.clone(),
            100_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;
    let raw_memo = format!("{}.vault42", raw_target.to_text().replace('-', ""));
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id.clone(),
            100_000_000u64,
            Some(raw_memo.into_bytes()),
        ))?,
    )?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(format!("{neuron_id}.vault.memo").into_bytes()),
        ))?,
    )?;
    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let commitments_before: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_commitment_history",
        GetCommitmentHistoryArgs {
            canister_id: target,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let raw_commitments_before: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_raw_icp_commitment_history",
        GetCommitmentHistoryArgs {
            canister_id: raw_target,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let neuron_commitments_before: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_neuron_commitment_history",
        GetNeuronCommitmentHistoryArgs {
            neuron_id,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let cycles_before: CyclesHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_cycles_history",
        GetCyclesHistoryArgs {
            canister_id: target,
            start_after_ts: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert_eq!(commitments_before.items.len(), 1);
    assert_eq!(raw_commitments_before.items.len(), 1);
    assert_eq!(neuron_commitments_before.items.len(), 1);
    assert_eq!(cycles_before.items.len(), 1);
    assert!(cycles_before.items[0].cycles > 0);

    let upgrade_sender = h
        .pic
        .get_controllers(h.historian)
        .first()
        .copied()
        .unwrap_or(h.historian);
    h.pic
        .upgrade_canister(
            h.historian,
            historian_wasm()?,
            encode_one(Option::<HistorianUpgradeArg>::None)?,
            Some(upgrade_sender),
        )
        .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;

    let commitments_after: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_commitment_history",
        GetCommitmentHistoryArgs {
            canister_id: target,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let raw_commitments_after: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_raw_icp_commitment_history",
        GetCommitmentHistoryArgs {
            canister_id: raw_target,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let neuron_commitments_after: CommitmentHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_neuron_commitment_history",
        GetNeuronCommitmentHistoryArgs {
            neuron_id,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    let cycles_after: CyclesHistoryPage = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_cycles_history",
        GetCyclesHistoryArgs {
            canister_id: target,
            start_after_ts: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert_eq!(commitments_after.items, commitments_before.items);
    assert_eq!(
        commitments_after.next_start_after_tx_id,
        commitments_before.next_start_after_tx_id
    );
    assert_eq!(raw_commitments_after.items, raw_commitments_before.items);
    assert_eq!(
        raw_commitments_after.next_start_after_tx_id,
        raw_commitments_before.next_start_after_tx_id
    );
    assert_eq!(
        neuron_commitments_after.items,
        neuron_commitments_before.items
    );
    assert_eq!(
        neuron_commitments_after.next_start_after_tx_id,
        neuron_commitments_before.next_start_after_tx_id
    );
    assert_eq!(cycles_after.items, cycles_before.items);
    assert_eq!(
        cycles_after.next_start_after_ts,
        cycles_before.next_start_after_ts
    );
    Ok(())
}

#[test]
#[ignore]
fn historian_upgrade_preserves_paginated_listing_without_skips() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let staking_id = h.staking_identifier()?;
    let targets = vec![h.blackhole, h.index, h.historian];

    for (i, target) in targets.iter().enumerate() {
        let _: u64 = update_bytes(
            &h.pic,
            h.index,
            Principal::anonymous(),
            "debug_append_transfer",
            encode_args((
                staking_id.clone(),
                20_000_000u64 + i as u64,
                Some(target.to_text().into_bytes()),
            ))?,
        )?;
    }

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let mut before_ids = Vec::new();
    let mut cursor = None;
    for _ in 0..8 {
        let page: ListCanistersResponse = query_one(
            &h.pic,
            h.historian,
            Principal::anonymous(),
            "list_canisters",
            ListCanistersArgs {
                start_after: cursor,
                limit: Some(2),
                tracking_reason_filter: None,
            },
        )?;
        before_ids.extend(page.items.iter().map(|item| item.canister_id));
        cursor = page.next_start_after;
        if cursor.is_none() {
            break;
        }
    }
    let mut expected_ids = targets.clone();
    expected_ids.sort();
    if before_ids != expected_ids {
        bail!("expected paginated pre-upgrade list to return all tracked canisters without skips, got {:?}", before_ids);
    }

    let registered_before: ListMemoRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        ListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    if registered_before.total != targets.len() as u64 {
        bail!(
            "expected {} registered summaries before upgrade, got {}",
            targets.len(),
            registered_before.total
        );
    }

    let upgrade_sender = h
        .pic
        .get_controllers(h.historian)
        .first()
        .copied()
        .unwrap_or(h.historian);
    h.pic
        .upgrade_canister(
            h.historian,
            historian_wasm()?,
            encode_one(Option::<HistorianUpgradeArg>::None)?,
            Some(upgrade_sender),
        )
        .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;

    let mut after_ids = Vec::new();
    let mut cursor = None;
    for _ in 0..8 {
        let page: ListCanistersResponse = query_one(
            &h.pic,
            h.historian,
            Principal::anonymous(),
            "list_canisters",
            ListCanistersArgs {
                start_after: cursor,
                limit: Some(2),
                tracking_reason_filter: None,
            },
        )?;
        after_ids.extend(page.items.iter().map(|item| item.canister_id));
        cursor = page.next_start_after;
        if cursor.is_none() {
            break;
        }
    }
    if after_ids != expected_ids {
        bail!("expected paginated post-upgrade list to preserve all tracked canisters without skips, got {:?}", after_ids);
    }

    let registered_after: ListMemoRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        ListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    if registered_after.total != targets.len() as u64 {
        bail!(
            "expected {} registered summaries after upgrade, got {}",
            targets.len(),
            registered_after.total
        );
    }

    for target in targets {
        let commitments: CommitmentHistoryPage = query_one(
            &h.pic,
            h.historian,
            Principal::anonymous(),
            "get_commitment_history",
            GetCommitmentHistoryArgs {
                canister_id: target,
                start_after_tx_id: None,
                limit: Some(10),
                descending: Some(false),
            },
        )?;
        if commitments.items.len() != 1 {
            bail!(
                "expected one preserved commitment for {target}, got {:?}",
                commitments.items
            );
        }
    }

    Ok(())
}

#[test]
#[ignore]
fn historian_reclaims_stale_main_lease_after_time_fast_forward() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.historian;
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            42_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;

    let now_secs = (h.pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    let _: () = update_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_set_main_lock_expires_at_ts",
        Some(now_secs + 30),
    )?;
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts_before: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts_before.tracked_canister_count, 0);
    assert_eq!(counts_before.qualifying_commitment_count, 0);

    h.pic.advance_time(Duration::from_secs(31));
    tick_n(&h.pic, 5);
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts_after: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts_after.tracked_canister_count, 1);
    assert_eq!(counts_after.qualifying_commitment_count, 1);
    Ok(())
}

#[test]
#[ignore]
fn historian_public_queries_surface_expected_counts_and_recent_items() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.historian;
    let staking_id = h.staking_identifier()?;

    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id.clone(),
            42_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            5_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(counts.tracked_canister_count, 1);
    assert_eq!(counts.qualifying_commitment_count, 1);
    assert_eq!(counts.total_output_e8s, 0);
    assert_eq!(counts.total_rewards_e8s, 0);
    assert_eq!(counts.sns_discovered_canister_count, 0);

    let status: PublicStatus = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_status",
        (),
    )?;
    assert_eq!(
        status.staking_account.owner,
        Principal::management_canister()
    );
    assert_eq!(status.staking_account.subaccount, Some([9u8; 32]));
    assert_eq!(status.ledger_canister_id, h.index);
    assert_eq!(status.index_interval_seconds, 60);
    assert_eq!(status.cycles_interval_seconds, 1);
    assert!(status.last_index_run_ts.is_some());
    assert!(status.heap_memory_bytes.is_some());
    assert!(status.stable_memory_bytes.is_some());
    assert!(status.total_memory_bytes.is_some());

    let registered: ListMemoRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        ListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    assert_eq!(registered.total, 1);
    assert_eq!(registered.items.len(), 1);
    assert_eq!(registered.items[0].canister_id, target);
    assert_eq!(
        registered.items[0].tracking_reasons,
        vec![CanisterTrackingReason::MemoCommitment]
    );
    assert_eq!(registered.items[0].qualifying_commitment_count, 1);
    assert_eq!(
        registered.items[0].total_qualifying_committed_e8s,
        42_000_000
    );
    assert!(registered.items[0].last_commitment_ts.is_some());
    assert!(registered.items[0].latest_cycles.unwrap_or_default() > 0);
    assert!(registered.items[0].last_cycles_probe_ts.is_some());

    let recent_all: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent_all.items.len(), 2);
    assert_eq!(recent_all.items[0].tx_id, 2);
    assert_eq!(recent_all.items[0].amount_e8s, 5_000_000);
    assert!(!recent_all.items[0].counts_toward_faucet);
    assert_eq!(recent_all.items[1].tx_id, 1);
    assert_eq!(recent_all.items[1].amount_e8s, 42_000_000);
    assert!(recent_all.items[1].counts_toward_faucet);

    let recent_qualifying: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(true),
        },
    )?;
    assert_eq!(recent_qualifying.items.len(), 1);
    assert_eq!(recent_qualifying.items[0].tx_id, 1);
    assert_eq!(recent_qualifying.items[0].canister_id, Some(target));
    Ok(())
}

#[test]
#[ignore]
fn historian_public_counts_exclude_sns_only_canisters_from_registered_totals() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(true)?;
    let sns_root = h.pic.create_canister();
    h.pic.add_cycles(sns_root, 5_000_000_000_000);
    h.pic
        .install_canister(sns_root, sns_root_wasm()?, vec![], None);

    let governance = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
    let dapp = Principal::from_text("qjdve-lqaaa-aaaaa-aaaeq-cai")?;

    let summary = GetSnsCanistersSummaryResponse {
        root: Some(SnsCanisterSummary {
            canister_id: Some(sns_root),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(1000u64)),
            }),
        }),
        governance: Some(SnsCanisterSummary {
            canister_id: Some(governance),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(2000u64)),
            }),
        }),
        ledger: None,
        swap: None,
        index: None,
        dapps: vec![SnsCanisterSummary {
            canister_id: Some(dapp),
            status: Some(SnsCanisterStatus {
                cycles: Some(Nat::from(3000u64)),
            }),
        }],
        archives: vec![],
    };
    let _: () = update_one(
        &h.pic,
        sns_root,
        Principal::anonymous(),
        "debug_set_summary",
        summary,
    )?;
    let _: () = update_one(
        &h.pic,
        h.sns_wasm,
        Principal::anonymous(),
        "debug_set_roots",
        vec![sns_root],
    )?;

    h.tick();
    let _: () = update_noargs(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "debug_driver_tick",
    )?;

    let counts: PublicCounts = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert_eq!(
        counts.tracked_canister_count,
        counts.sns_discovered_canister_count
    );
    assert_eq!(counts.memo_registered_canister_count, 0);
    assert_eq!(counts.qualifying_commitment_count, 0);
    assert_eq!(counts.total_output_e8s, 0);
    assert_eq!(counts.total_rewards_e8s, 0);
    assert!(counts.sns_discovered_canister_count >= 3);

    let registered: ListMemoRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        ListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    assert_eq!(registered.total, 0);
    assert!(registered.items.is_empty());

    let recent: ListRecentCommitmentsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_commitments",
        ListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert!(recent.items.is_empty());
    Ok(())
}
