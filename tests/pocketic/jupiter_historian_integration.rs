// PocketIC historian scenarios use explicit casts to mirror Candid/interface boundary values.
#![allow(clippy::unnecessary_cast)]

use anyhow::{anyhow, bail, Context, Result};
use candid::{encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use jupiter_ic_clients::account_identifier::account_identifier_text;
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsArgs, GetAccountIdentifierTransactionsResponse,
    GetAccountIdentifierTransactionsResult,
};
use pocket_ic::PocketIc;
use sha2::{Digest, Sha256};

#[path = "real_blackhole.rs"]
mod real_blackhole;
#[path = "support/mod.rs"]
mod support;
use std::process::Command;
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
static RELAY_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_ENABLED_HISTORIAN_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static MOCK_LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static MOCK_CMC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static STATUS_PROXY_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CYCLE_BURNER_WASM: OnceLock<Vec<u8>> = OnceLock::new();

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
fn relay_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&RELAY_WASM, "jupiter-relay", None)
}
fn mock_ledger_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&MOCK_LEDGER_WASM, "mock-icrc-ledger", None)
}
fn mock_cmc_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&MOCK_CMC_WASM, "mock-cmc", None)
}
fn status_proxy_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&STATUS_PROXY_WASM, "mock-status-proxy", None)
}
fn cycle_burner_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&CYCLE_BURNER_WASM, "mock-cycle-burner", None)
}
fn relay_enabled_historian_wasm() -> Result<Vec<u8>> {
    if let Some(bytes) = RELAY_ENABLED_HISTORIAN_WASM.get() {
        return Ok(bytes.clone());
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
    relay_setup_dust_e8s: Option<u64>,
    relay_setup_refund_cooldown_seconds: Option<u64>,
    relay_initial_cycles: Option<u128>,
    relay_cycle_safety_margin_e8s: Option<u64>,
    relay_min_subaccount_one_seed_e8s: Option<u64>,
    self_service_relay_interval_seconds: Option<u64>,
    self_service_relay_max_transfers_per_tick: Option<u32>,
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

#[derive(Clone, Debug, CandidType, Deserialize)]
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

#[derive(Clone, Debug, CandidType, Deserialize)]
enum CyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootStatus,
    SnsSwapStatus,
    SnsRootSummary,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
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
struct DebugState {
    distinct_canister_count: u32,
    last_indexed_staking_tx_id: Option<u64>,
    last_indexed_output_tx_id: Option<u64>,
    last_indexed_rewards_tx_id: Option<u64>,
    last_sns_discovery_ts: u64,
    last_completed_cycles_sweep_ts: u64,
    active_cycles_sweep_present: bool,
    active_cycles_sweep_next_index: Option<u64>,
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
#[derive(Clone, Debug, CandidType, Deserialize)]
struct PublicCounts {
    tracked_canister_count: u64,
    memo_registered_canister_count: u64,
    qualifying_commitment_count: u64,
    sns_discovered_canister_count: u64,
    relay_target_canister_count: u64,
    relay_instance_canister_count: u64,
    total_output_e8s: u64,
    total_rewards_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CommitmentIndexFault {
    observed_at_ts: u64,
    last_cursor_tx_id: Option<u64>,
    offending_tx_id: u64,
    message: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
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

#[derive(Clone, Debug, CandidType, Deserialize)]
enum CyclesProbeResult {
    Ok(CyclesSampleSource),
    NotAvailable,
    Error(String),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterMeta {
    first_seen_ts: Option<u64>,
    last_commitment_ts: Option<u64>,
    last_cycles_probe_ts: Option<u64>,
    last_cycles_probe_result: Option<CyclesProbeResult>,
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

#[derive(Clone, Debug, CandidType, Deserialize)]
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

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayRegistryKind {
    Canonical,
    SelfService,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayRegistration {
    target_canister_id: Principal,
    relay_canister_id: Principal,
    kind: RelayRegistryKind,
    created_at_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListRelayRegistrationsResponse {
    items: Vec<RelayRegistration>,
    next_start_after: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListRelayRegistrationsArgs {
    start_after: Option<Principal>,
    limit: Option<u32>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct TransferRecord {
    from: Account,
    to: Account,
    amount: Nat,
    fee: Nat,
    memo: Option<Vec<u8>>,
    created_at_time: Option<u64>,
    result: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugStatusProxyCall {
    canister_id: Principal,
    caller: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugSnsRootCall {
    method: String,
    canister_id: Option<Principal>,
    caller: Principal,
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

#[derive(Clone, Debug, CandidType, Deserialize)]
struct BurnCyclesArgs {
    sink: Principal,
    amount: u128,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelaySetupPublicStatus {
    NotFunded,
    BelowMinimum,
    PaymentNotAllowed,
    IndexNotReady,
    Pending,
    CreatingRelay,
    Active,
    SweepingToExistingRelay,
    Refunding,
    Refunded,
    FailedRetryable,
    ManualRecoveryRequired,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetRelaySetupViewArgs {
    target_canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelaySetupView {
    target_canister_id: Principal,
    setup_account: Account,
    setup_account_identifier: String,
    minimum_e8s: u64,
    payment_allowed: bool,
    payment_blocked_reason: Option<String>,
    existing_relay: Option<RelayRegistration>,
    status: RelaySetupPublicStatus,
    factory_available: bool,
    warning_text: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RelaySetupNotifyResult {
    BelowMinimum {
        minimum_e8s: u64,
        current_balance_e8s: u64,
    },
    InsufficientForCurrentRate {
        required_e8s: u64,
        current_balance_e8s: u64,
    },
    TargetNotObservable {
        message: String,
    },
    Pending {
        status: RelaySetupPublicStatus,
    },
    Active {
        relay: RelayRegistration,
    },
    SweptToExistingRelay {
        relay: RelayRegistration,
        amount_e8s: u64,
        block_index: u64,
    },
    SweepBelowDust {
        relay: RelayRegistration,
        current_balance_e8s: u64,
    },
    Refunded {
        blocks: Vec<u64>,
    },
    RefundPending {
        reason: String,
    },
    Failed {
        status: RelaySetupPublicStatus,
        message: String,
    },
}

fn real_icp_ledger_principal() -> Principal {
    support::principals::icp_ledger()
}

fn real_icp_index_principal() -> Principal {
    support::principals::icp_index()
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

fn relay_subaccount_one() -> [u8; 32] {
    let mut subaccount = [0u8; 32];
    subaccount[31] = 1;
    subaccount
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
        relay_setup_dust_e8s: Some(10_000),
        relay_setup_refund_cooldown_seconds: Some(0),
        relay_initial_cycles: Some(1_000_000_000_000),
        relay_cycle_safety_margin_e8s: Some(5_000_000),
        relay_min_subaccount_one_seed_e8s: Some(100_020_000),
        self_service_relay_interval_seconds: Some(3600),
        self_service_relay_max_transfers_per_tick: Some(10),
        io_surplus_neuron_id: Some(11614578985374291210),
        canonical_relay_canister_id: None,
        canonical_relay_targets: Some(Vec::new()),
    }
}

fn auto_self_service_historian_init(
    ledger: Principal,
    index: Principal,
    cmc: Principal,
    sns_wasm: Principal,
) -> HistorianInitArg {
    let mut init = self_service_historian_init(
        ledger,
        index,
        cmc,
        jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
    );
    init.sns_wasm_canister_id = Some(sns_wasm);
    init.xrc_canister_id = Some(jupiter_ic_clients::constants::fiduciary_blackhole_canister_id());
    init.faucet_canister_id =
        Some(jupiter_ic_clients::constants::fiduciary_blackhole_canister_id());
    init
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

fn install_sns_wasm_mock(pic: &PocketIc) -> Result<Principal> {
    let sns_wasm = pic.create_canister();
    pic.add_cycles(sns_wasm, 5_000_000_000_000);
    pic.install_canister(sns_wasm, sns_wasm_wasm()?, vec![], None);
    Ok(sns_wasm)
}

fn install_sns_wasm_mock_at(pic: &PocketIc, canister_id: Principal) -> Result<Principal> {
    create_fixed_canister(pic, canister_id)?;
    pic.add_cycles(canister_id, 5_000_000_000_000);
    pic.install_canister(canister_id, sns_wasm_wasm()?, vec![], None);
    Ok(canister_id)
}

fn credit_setup_account(
    pic: &PocketIc,
    ledger: Principal,
    index: Principal,
    view: &RelaySetupView,
    setup_amount: u64,
) -> Result<()> {
    let _: () = update_bytes(
        pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((view.setup_account, setup_amount))?,
    )?;
    let _: u64 = update_bytes(
        pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer_from",
        encode_args((
            "setup-source".to_string(),
            view.setup_account_identifier.clone(),
            setup_amount,
            Option::<Vec<u8>>::None,
        ))?,
    )?;
    Ok(())
}

fn activate_self_service_relay(
    pic: &PocketIc,
    historian: Principal,
    ledger: Principal,
    index: Principal,
    target: Principal,
) -> Result<RelayRegistration> {
    let view: RelaySetupView = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        GetRelaySetupViewArgs {
            target_canister_id: target,
        },
    )?;
    assert!(
        view.payment_allowed,
        "self-service setup should be allowed before activation: {view:?}"
    );
    credit_setup_account(pic, ledger, index, &view, 300_000_000)?;
    let result: RelaySetupNotifyResult = update_one(
        pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        target,
    )?;
    match result {
        RelaySetupNotifyResult::Active { relay } => Ok(relay),
        other => bail!("expected self-service setup to activate relay, got {other:?}"),
    }
}

fn relay_default_account(relay_id: Principal) -> Account {
    Account {
        owner: relay_id,
        subaccount: None,
    }
}

fn fund_relay_default_account(
    pic: &PocketIc,
    ledger: Principal,
    relay_id: Principal,
) -> Result<()> {
    let _: () = update_bytes(
        pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((relay_default_account(relay_id), 200_000_000u64))?,
    )?;
    Ok(())
}

fn wait_for_cmc_notification(
    pic: &PocketIc,
    cmc: Principal,
    target: Principal,
    step_seconds: u64,
) -> Result<Vec<NotifyRecord>> {
    let mut notifications = Vec::<NotifyRecord>::new();
    for _ in 0..5 {
        pic.advance_time(Duration::from_secs(step_seconds));
        tick_n(pic, 30);
        notifications = query_one(pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
        if notifications
            .iter()
            .any(|notification| notification.canister_id == target)
        {
            return Ok(notifications);
        }
    }
    bail!("expected CMC notification for {target}; notifications={notifications:?}")
}

fn assert_target_topup_transfer(
    pic: &PocketIc,
    ledger: Principal,
    cmc: Principal,
    relay_id: Principal,
    target: Principal,
) -> Result<()> {
    let cmc_deposit = Account {
        owner: cmc,
        subaccount: Some(jupiter_ic_clients::account::principal_to_subaccount(target)),
    };
    let transfers: Vec<TransferRecord> =
        query_one(pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    assert!(
        transfers.iter().any(|transfer| {
            transfer.from == relay_default_account(relay_id)
                && transfer.to == cmc_deposit
                && transfer.result == "Ok"
                && transfer.amount > 0u8
        }),
        "spawned relay should create a positive CMC top-up transfer for target; transfers={transfers:?}"
    );
    Ok(())
}

fn assert_historian_tracks_target(
    pic: &PocketIc,
    historian: Principal,
    target: Principal,
) -> Result<()> {
    let registrations: ListRelayRegistrationsResponse = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "list_relay_registrations",
        ListRelayRegistrationsArgs {
            start_after: None,
            limit: Some(100),
        },
    )?;
    let relay_id = registrations
        .items
        .iter()
        .find(|entry| entry.target_canister_id == target && entry.kind == RelayRegistryKind::SelfService)
        .map(|entry| entry.relay_canister_id)
        .with_context(|| {
            format!("historian registry should contain active self-service relay; registrations={registrations:?}")
        })?;

    let canisters: ListCanistersResponse = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        ListCanistersArgs {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: None,
        },
    )?;
    assert!(
        canisters.items.iter().any(|item| {
            item.canister_id == target
                && item
                    .tracking_reasons
                    .contains(&CanisterTrackingReason::RelayTarget)
        }),
        "self-service target should be publicly tracked as RelayTarget: {canisters:?}"
    );
    assert!(
        canisters.items.iter().any(|item| {
            item.canister_id == relay_id
                && item
                    .tracking_reasons
                    .contains(&CanisterTrackingReason::RelayInstance)
        }),
        "self-service relay should be publicly tracked as RelayInstance: {canisters:?}"
    );
    let counts: PublicCounts = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert!(
        counts.relay_target_canister_count >= 1,
        "public counts should include the self-service target as RelayTarget; counts={counts:?}"
    );
    assert!(
        counts.relay_instance_canister_count >= 1,
        "public counts should include the self-service relay as RelayInstance; counts={counts:?}"
    );
    assert!(
        counts.tracked_canister_count >= 2,
        "public counts should include target and relay as unique tracked principals; counts={counts:?}"
    );
    Ok(())
}

fn run_historian_cycles_tick(pic: &PocketIc, historian: Principal) -> Result<()> {
    pic.advance_time(Duration::from_secs(61));
    let _ = historian;
    tick_n(pic, 30);
    Ok(())
}

fn assert_historian_cycles_sample(
    pic: &PocketIc,
    historian: Principal,
    target: Principal,
) -> Result<()> {
    let cycles: CyclesHistoryPage = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "get_cycles_history",
        GetCyclesHistoryArgs {
            canister_id: target,
            start_after_ts: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert!(
        cycles.items.iter().any(|sample| sample.cycles > 0),
        "historian should record a positive cycles sample for target; cycles={cycles:?}"
    );
    Ok(())
}

fn assert_historian_cycles_samples_for_target_and_relay(
    pic: &PocketIc,
    historian: Principal,
    target: Principal,
) -> Result<()> {
    let registrations: ListRelayRegistrationsResponse = query_one(
        pic,
        historian,
        Principal::anonymous(),
        "list_relay_registrations",
        ListRelayRegistrationsArgs {
            start_after: None,
            limit: Some(100),
        },
    )?;
    let relay_id = registrations
        .items
        .iter()
        .find(|entry| entry.target_canister_id == target && entry.kind == RelayRegistryKind::SelfService)
        .map(|entry| entry.relay_canister_id)
        .with_context(|| {
            format!("historian registry should contain active self-service relay; registrations={registrations:?}")
        })?;
    assert_historian_cycles_sample(pic, historian, target)?;
    assert_historian_cycles_sample(pic, historian, relay_id)?;
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
            relay_setup_dust_e8s: None,
            relay_setup_refund_cooldown_seconds: None,
            relay_initial_cycles: None,
            relay_cycle_safety_margin_e8s: None,
            relay_min_subaccount_one_seed_e8s: None,
            self_service_relay_interval_seconds: None,
            self_service_relay_max_transfers_per_tick: None,
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
fn self_service_relay_notify_creates_installs_funds_blackholes_relay() -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = real_icp_ledger_principal();
    let index = real_icp_index_principal();
    let cmc = support::principals::cycles_minting_canister();
    let thirteen = jupiter_ic_clients::constants::thirteen_node_blackhole_canister_id();
    let fiduciary = jupiter_ic_clients::constants::fiduciary_blackhole_canister_id();
    install_status_proxy(&pic, thirteen)?;
    install_status_proxy(&pic, fiduciary)?;
    let historian = pic.create_canister();
    let target = pic.create_canister();
    for canister in [historian, target] {
        pic.add_cycles(canister, 10_000_000_000_000);
    }
    set_controllers_exact(&pic, target, vec![thirteen])?;
    pic.install_canister(
        historian,
        relay_enabled_historian_wasm()?,
        encode_one(self_service_historian_init(ledger, index, cmc, fiduciary))?,
        None,
    );
    pic.add_cycles(historian, 20_000_000_000_000);

    let view: RelaySetupView = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        GetRelaySetupViewArgs {
            target_canister_id: target,
        },
    )?;
    assert!(
        view.factory_available,
        "embedded relay wasm should enable factory in setup view: {view:?}"
    );
    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let setup_amount = 300_000_000u64;
    let setup_block = icrc1_transfer(
        &pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: view.setup_account,
            fee: Some(Nat::from(fee_e8s)),
            created_at_time: None,
            memo: Some(Memo::from(b"self-service-relay-pocketic".to_vec())),
            amount: Nat::from(setup_amount),
        },
    )?;
    let setup_page = wait_for_index_transactions(&pic, index, &view.setup_account_identifier, 1)?;
    assert!(
        setup_page
            .transactions
            .iter()
            .any(|tx| tx.id == setup_block),
        "setup account index page should include funding block {setup_block}: {:?}",
        setup_page.transactions
    );

    let result: RelaySetupNotifyResult = update_one(
        &pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        target,
    )?;
    let relay = match result {
        RelaySetupNotifyResult::Active { relay } => relay,
        other => bail!("expected Active relay setup result, got {other:?}"),
    };
    assert_eq!(relay.target_canister_id, target);
    assert_eq!(relay.kind, RelayRegistryKind::SelfService);
    assert_ne!(relay.relay_canister_id, historian);
    assert_ne!(relay.relay_canister_id, target);

    let registered: ListRelayRegistrationsResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_relay_registrations",
        ListRelayRegistrationsArgs {
            start_after: None,
            limit: Some(100),
        },
    )?;
    let registered = registered
        .items
        .into_iter()
        .find(|entry| entry.target_canister_id == target)
        .context("expected active relay registry entry")?;
    assert_eq!(registered.relay_canister_id, relay.relay_canister_id);

    let relay_status = pic
        .canister_status(relay.relay_canister_id, Some(fiduciary))
        .map_err(|err| anyhow!("relay canister_status failed: {err:?}"))?;
    assert!(
        relay_status.module_hash.is_some(),
        "relay wasm should be installed"
    );
    assert_eq!(
        pic.get_controllers(relay.relay_canister_id),
        vec![fiduciary]
    );

    let post_setup_page =
        wait_for_index_transactions(&pic, index, &view.setup_account_identifier, 3)?;
    let relay_subaccount_one_id =
        account_identifier_text(relay.relay_canister_id, Some(relay_subaccount_one()));
    let historian_cmc_deposit_id = account_identifier_text(
        cmc,
        Some(jupiter_ic_clients::account::principal_to_subaccount(
            historian,
        )),
    );
    let mut saw_cmc_conversion_transfer = false;
    let mut saw_relay_funding_transfer = false;
    for tx in &post_setup_page.transactions {
        let jupiter_ic_clients::index::IndexOperation::Transfer { from, to, .. } =
            &tx.transaction.operation
        else {
            continue;
        };
        if from == &view.setup_account_identifier && to == &historian_cmc_deposit_id {
            saw_cmc_conversion_transfer = true;
        }
        if from == &view.setup_account_identifier && to == &relay_subaccount_one_id {
            saw_relay_funding_transfer = true;
        }
    }
    assert!(
        saw_cmc_conversion_transfer,
        "mock/real ledger should record setup subaccount CMC conversion transfer; page={post_setup_page:?}"
    );
    assert!(
        saw_relay_funding_transfer,
        "ledger should record setup subaccount transfer to relay subaccount 1; page={post_setup_page:?}"
    );

    let second_amount = 50_000_000u64;
    let second_block = icrc1_transfer(
        &pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: view.setup_account,
            fee: Some(Nat::from(fee_e8s)),
            created_at_time: None,
            memo: Some(Memo::from(b"second-self-service-sweep".to_vec())),
            amount: Nat::from(second_amount),
        },
    )?;
    let _ = wait_for_index_transactions(&pic, index, &view.setup_account_identifier, 4)?;
    let duplicate: RelaySetupNotifyResult = update_one(
        &pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        target,
    )?;
    let (swept_relay, swept_amount) = match duplicate {
        RelaySetupNotifyResult::SweptToExistingRelay {
            relay, amount_e8s, ..
        } => (relay, amount_e8s),
        other => bail!("expected duplicate notify to sweep to existing relay, got {other:?}"),
    };
    assert_eq!(swept_relay.relay_canister_id, relay.relay_canister_id);
    assert_eq!(swept_amount, second_amount.saturating_sub(fee_e8s));
    let after_duplicate: ListRelayRegistrationsResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_relay_registrations",
        ListRelayRegistrationsArgs {
            start_after: None,
            limit: Some(100),
        },
    )?;
    assert_eq!(
        after_duplicate
            .items
            .into_iter()
            .find(|entry| entry.target_canister_id == target)
            .context("expected relay registry after duplicate notify")?
            .relay_canister_id,
        relay.relay_canister_id
    );
    let final_setup_page =
        wait_for_index_transactions(&pic, index, &view.setup_account_identifier, 5)?;
    assert!(
        final_setup_page
            .transactions
            .iter()
            .any(|tx| tx.id == second_block),
        "setup account index should include second payment block {second_block}"
    );
    Ok(())
}

#[test]
#[ignore]
fn retained_gzip_relay_payload_module_hash_matches_exact_supplied_bytes() -> Result<()> {
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
fn self_service_spawned_relay_runs_after_time_advance() -> Result<()> {
    require_ignored_flag()?;
    let historian_wasm = relay_enabled_historian_wasm()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
    let thirteen = jupiter_ic_clients::constants::thirteen_node_blackhole_canister_id();
    let fiduciary = jupiter_ic_clients::constants::fiduciary_blackhole_canister_id();
    install_status_proxy(&pic, thirteen)?;
    install_status_proxy(&pic, fiduciary)?;
    let sns_wasm = install_sns_wasm_mock(&pic)?;

    let ledger = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let historian = pic.create_canister();
    let target = pic.create_canister();
    let sink = pic.create_canister();
    for canister in [ledger, index, cmc, historian, target, sink] {
        pic.add_cycles(canister, 10_000_000_000_000);
    }
    pic.install_canister(ledger, mock_ledger_wasm()?, vec![], None);
    pic.install_canister(index, index_wasm()?, vec![], None);
    pic.install_canister(cmc, mock_cmc_wasm()?, vec![], None);
    pic.install_canister(target, cycle_burner_wasm()?, vec![], None);
    pic.install_canister(sink, cycle_burner_wasm()?, vec![], None);
    set_controllers_exact(&pic, target, vec![thirteen])?;

    pic.install_canister(
        historian,
        historian_wasm,
        encode_one(auto_self_service_historian_init(
            ledger, index, cmc, sns_wasm,
        ))?,
        None,
    );
    pic.add_cycles(historian, 20_000_000_000_000);

    let view: RelaySetupView = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_relay_setup_view",
        GetRelaySetupViewArgs {
            target_canister_id: target,
        },
    )?;
    assert!(
        view.payment_allowed,
        "funded self-service setup should be allowed before notify: {view:?}"
    );
    let setup_amount = 300_000_000u64;
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((view.setup_account, setup_amount))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer_from",
        encode_args((
            "setup-source".to_string(),
            view.setup_account_identifier.clone(),
            setup_amount,
            Option::<Vec<u8>>::None,
        ))?,
    )?;

    let result: RelaySetupNotifyResult = update_one(
        &pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        target,
    )?;
    let relay = match result {
        RelaySetupNotifyResult::Active { relay } => relay,
        other => bail!("expected self-service setup to activate relay, got {other:?}"),
    };
    let relay_id = relay.relay_canister_id;
    assert_ne!(relay_id, historian);
    assert_ne!(relay_id, target);
    let setup_calls: Vec<DebugStatusProxyCall> =
        query_one(&pic, thirteen, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        setup_calls
            .iter()
            .any(|call| call.caller == historian && call.canister_id == target),
        "historian setup should probe target through 13-node before spending; calls={setup_calls:?}"
    );
    let setup_fiduciary_calls: Vec<DebugStatusProxyCall> =
        query_one(&pic, fiduciary, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        setup_fiduciary_calls.is_empty(),
        "Fiduciary should not be called after 13-node setup success; calls={setup_fiduciary_calls:?}"
    );
    let relay_status = pic
        .canister_status(relay_id, Some(fiduciary))
        .map_err(|err| anyhow!("spawned relay canister_status failed: {err:?}"))?;
    assert!(
        relay_status.module_hash.is_some(),
        "spawned relay should have installed code"
    );
    assert_eq!(pic.get_controllers(relay_id), vec![fiduciary]);
    let relay_default = Account {
        owner: relay_id,
        subaccount: None,
    };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((relay_default, 200_000_000u64))?,
    )?;

    let _: () = update_noargs(&pic, thirteen, Principal::anonymous(), "debug_reset")?;
    let _: () = update_noargs(&pic, fiduciary, Principal::anonymous(), "debug_reset")?;
    pic.advance_time(Duration::from_secs(3_605));
    tick_n(&pic, 20);
    let baseline_calls: Vec<DebugStatusProxyCall> =
        query_one(&pic, thirteen, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        baseline_calls
            .iter()
            .any(|call| call.caller == relay_id && call.canister_id == target),
        "spawned relay should establish target baseline through 13-node; calls={baseline_calls:?}"
    );

    let before = pic.cycle_balance(target);
    let _: () = update_one(
        &pic,
        target,
        Principal::anonymous(),
        "burn_cycles",
        BurnCyclesArgs {
            sink,
            amount: 5_000_000_000_000,
        },
    )?;
    let after = pic.cycle_balance(target);
    assert!(
        after < before,
        "target should have a real high-to-low cycles transition; before={before} after={after}"
    );

    let mut notifications = Vec::<NotifyRecord>::new();
    for _ in 0..3 {
        pic.advance_time(Duration::from_secs(3_605));
        tick_n(&pic, 30);
        notifications = query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
        if notifications
            .iter()
            .any(|notification| notification.canister_id == target)
        {
            break;
        }
    }
    assert!(
        notifications
            .iter()
            .any(|notification| notification.canister_id == target),
        "spawned relay should notify CMC for the low-cycle target; notifications={notifications:?}"
    );

    let calls: Vec<DebugStatusProxyCall> =
        query_one(&pic, thirteen, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        calls
            .iter()
            .any(|call| call.caller == relay_id && call.canister_id == target),
        "spawned relay should probe target cycles through 13-node; calls={calls:?}"
    );

    let cmc_deposit = Account {
        owner: cmc,
        subaccount: Some(jupiter_ic_clients::account::principal_to_subaccount(target)),
    };
    let transfers: Vec<TransferRecord> =
        query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    assert!(
        transfers.iter().any(|transfer| {
            transfer.from == relay_default
                && transfer.to == cmc_deposit
                && transfer.result == "Ok"
                && transfer.amount > 0u8
        }),
        "spawned relay should create a CMC top-up transfer for target; transfers={transfers:?}"
    );

    let registrations: ListRelayRegistrationsResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_relay_registrations",
        ListRelayRegistrationsArgs {
            start_after: None,
            limit: Some(100),
        },
    )?;
    let target_registrations = registrations
        .items
        .iter()
        .filter(|entry| entry.target_canister_id == target)
        .collect::<Vec<_>>();
    assert_eq!(target_registrations.len(), 1);
    assert_eq!(target_registrations[0].relay_canister_id, relay_id);
    assert_eq!(target_registrations[0].kind, RelayRegistryKind::SelfService);

    run_historian_cycles_tick(&pic, historian)?;
    assert_historian_tracks_target(&pic, historian, target)?;
    assert_historian_cycles_samples_for_target_and_relay(&pic, historian, target)?;
    upgrade_historian_without_config_changes(&pic, historian)?;
    assert_historian_tracks_target(&pic, historian, target)?;
    run_historian_cycles_tick(&pic, historian)?;
    assert_historian_cycles_samples_for_target_and_relay(&pic, historian, target)?;

    let repeated: RelaySetupNotifyResult = update_one(
        &pic,
        historian,
        Principal::anonymous(),
        "notify_relay_setup",
        target,
    )?;
    match repeated {
        RelaySetupNotifyResult::Active { relay } => {
            assert_eq!(relay.relay_canister_id, relay_id);
        }
        RelaySetupNotifyResult::SweepBelowDust { relay, .. } => {
            assert_eq!(relay.relay_canister_id, relay_id);
        }
        other => bail!("repeated notify should not create a second relay, got {other:?}"),
    }
    let after_repeated: ListRelayRegistrationsResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_relay_registrations",
        ListRelayRegistrationsArgs {
            start_after: None,
            limit: Some(100),
        },
    )?;
    let after_target_registrations = after_repeated
        .items
        .iter()
        .filter(|entry| entry.target_canister_id == target)
        .collect::<Vec<_>>();
    assert_eq!(after_target_registrations.len(), 1);
    assert_eq!(after_target_registrations[0].relay_canister_id, relay_id);
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
fn self_service_auto_discovers_sns_dapp_route_without_blackhole_controller() -> Result<()> {
    require_ignored_flag()?;
    let historian_wasm = relay_enabled_historian_wasm()?;
    let pic = support::pocketic::sns_topology_builder().build();
    let topology = pic.topology();
    let nns_subnet = topology.get_nns().expect("NNS subnet");
    let sns_subnet = topology.get_sns().expect("SNS subnet");
    let app_subnet = topology
        .get_app_subnets()
        .into_iter()
        .next()
        .expect("application subnet");
    let thirteen = jupiter_ic_clients::constants::thirteen_node_blackhole_canister_id();
    let fiduciary = jupiter_ic_clients::constants::fiduciary_blackhole_canister_id();
    install_status_proxy(&pic, thirteen)?;
    install_status_proxy(&pic, fiduciary)?;
    let sns_wasm = install_sns_wasm_mock_at(&pic, jupiter_ic_clients::constants::sns_wasm_id())?;
    assert_eq!(pic.get_subnet(sns_wasm), Some(nns_subnet));

    let sns_root = pic.create_canister_on_subnet(None, None, sns_subnet);
    let target = pic.create_canister_on_subnet(None, None, app_subnet);
    let ledger = pic.create_canister_on_subnet(None, None, app_subnet);
    let index = pic.create_canister_on_subnet(None, None, app_subnet);
    let cmc = pic.create_canister_on_subnet(None, None, app_subnet);
    let historian = pic.create_canister_on_subnet(None, None, app_subnet);
    let sink = pic.create_canister_on_subnet(None, None, app_subnet);
    for canister in [ledger, index, cmc, historian, sns_root, target, sink] {
        pic.add_cycles(canister, 10_000_000_000_000);
    }
    pic.install_canister(ledger, mock_ledger_wasm()?, vec![], None);
    pic.install_canister(index, index_wasm()?, vec![], None);
    pic.install_canister(cmc, mock_cmc_wasm()?, vec![], None);
    pic.install_canister(sns_root, sns_root_wasm()?, vec![], None);
    pic.install_canister(target, cycle_burner_wasm()?, vec![], None);
    pic.install_canister(sink, cycle_burner_wasm()?, vec![], None);
    set_controllers_exact(&pic, target, vec![sns_root])?;
    assert_eq!(pic.get_subnet(sns_root), Some(sns_subnet));
    assert_eq!(pic.get_subnet(target), Some(app_subnet));
    assert_eq!(pic.get_subnet(historian), Some(app_subnet));
    assert_eq!(pic.get_controllers(target), vec![sns_root]);

    let _: () = update_one(
        &pic,
        sns_wasm,
        Principal::anonymous(),
        "debug_set_roots",
        vec![sns_root],
    )?;
    let _: () = update_one(
        &pic,
        sns_root,
        Principal::anonymous(),
        "debug_set_canisters",
        ListSnsCanistersResponse {
            root: Some(sns_root),
            dapps: vec![target],
            ..Default::default()
        },
    )?;

    pic.install_canister(
        historian,
        historian_wasm,
        encode_one(auto_self_service_historian_init(
            ledger, index, cmc, sns_wasm,
        ))?,
        None,
    );
    pic.add_cycles(historian, 20_000_000_000_000);

    let relay = activate_self_service_relay(&pic, historian, ledger, index, target)?;
    let relay_id = relay.relay_canister_id;
    assert_eq!(pic.get_controllers(relay_id), vec![fiduciary]);

    for probe in [thirteen, fiduciary] {
        let calls: Vec<DebugStatusProxyCall> =
            query_one(&pic, probe, Principal::anonymous(), "debug_calls", ())?;
        assert!(
            calls
                .iter()
                .any(|call| call.caller == historian && call.canister_id == target),
            "historian should try blackhole probe {probe} before SNS route; calls={calls:?}"
        );
    }
    let sns_wasm_calls: Vec<Principal> =
        query_one(&pic, sns_wasm, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        sns_wasm_calls.contains(&historian),
        "historian should query SNS-W during route discovery; calls={sns_wasm_calls:?}"
    );
    let root_calls: Vec<DebugSnsRootCall> =
        query_one(&pic, sns_root, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        root_calls
            .iter()
            .any(|call| call.method == "list_sns_canisters" && call.caller == historian),
        "historian should verify SNS root membership; calls={root_calls:?}"
    );
    assert!(
        root_calls
            .iter()
            .any(|call| call.method == "canister_status"
                && call.caller == historian
                && call.canister_id == Some(target)),
        "historian should read target status through SNS root; calls={root_calls:?}"
    );

    fund_relay_default_account(&pic, ledger, relay_id)?;
    let _: () = update_noargs(&pic, thirteen, Principal::anonymous(), "debug_reset")?;
    let _: () = update_noargs(&pic, fiduciary, Principal::anonymous(), "debug_reset")?;
    let _: () = update_noargs(&pic, sns_root, Principal::anonymous(), "debug_reset_calls")?;

    pic.advance_time(Duration::from_secs(3_605));
    tick_n(&pic, 30);
    let relay_blackhole_calls: Vec<DebugStatusProxyCall> =
        query_one(&pic, thirteen, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        relay_blackhole_calls
            .iter()
            .any(|call| call.caller == relay_id && call.canister_id == target),
        "relay should independently try 13-node before SNS root route; calls={relay_blackhole_calls:?}"
    );
    let relay_fiduciary_calls: Vec<DebugStatusProxyCall> =
        query_one(&pic, fiduciary, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        relay_fiduciary_calls
            .iter()
            .any(|call| call.caller == relay_id && call.canister_id == target),
        "relay should independently try Fiduciary before SNS root route; calls={relay_fiduciary_calls:?}"
    );
    let relay_root_calls: Vec<DebugSnsRootCall> =
        query_one(&pic, sns_root, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        relay_root_calls
            .iter()
            .any(|call| call.method == "canister_status"
                && call.caller == relay_id
                && call.canister_id == Some(target)),
        "relay should independently discover and use SNS root; calls={relay_root_calls:?}"
    );

    let before = pic.cycle_balance(target);
    let _: () = update_one(
        &pic,
        target,
        Principal::anonymous(),
        "burn_cycles",
        BurnCyclesArgs {
            sink,
            amount: 5_000_000_000_000,
        },
    )?;
    let after = pic.cycle_balance(target);
    assert!(
        after < before,
        "SNS dapp target should have a real high-to-low cycles transition; before={before} after={after}"
    );

    let notifications = wait_for_cmc_notification(&pic, cmc, target, 3_605)?;
    assert!(
        notifications
            .iter()
            .any(|notification| notification.canister_id == target),
        "relay should notify CMC for low SNS dapp target; notifications={notifications:?}"
    );
    let post_burn_root_calls: Vec<DebugSnsRootCall> =
        query_one(&pic, sns_root, Principal::anonymous(), "debug_calls", ())?;
    assert!(
        post_burn_root_calls
            .iter()
            .filter(|call| call.method == "canister_status"
                && call.caller == relay_id
                && call.canister_id == Some(target))
            .count()
            >= 2,
        "timer-driven relay operation should observe the lower target balance through SNS root; calls={post_burn_root_calls:?}"
    );
    assert_target_topup_transfer(&pic, ledger, cmc, relay_id, target)?;

    run_historian_cycles_tick(&pic, historian)?;
    assert_historian_tracks_target(&pic, historian, target)?;
    assert_historian_cycles_samples_for_target_and_relay(&pic, historian, target)?;
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
        relay_setup_dust_e8s: None,
        relay_setup_refund_cooldown_seconds: None,
        relay_initial_cycles: None,
        relay_cycle_safety_margin_e8s: None,
        relay_min_subaccount_one_seed_e8s: None,
        self_service_relay_interval_seconds: None,
        self_service_relay_max_transfers_per_tick: None,
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
        relay_setup_dust_e8s: None,
        relay_setup_refund_cooldown_seconds: None,
        relay_initial_cycles: None,
        relay_cycle_safety_margin_e8s: None,
        relay_min_subaccount_one_seed_e8s: None,
        self_service_relay_interval_seconds: None,
        self_service_relay_max_transfers_per_tick: None,
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
