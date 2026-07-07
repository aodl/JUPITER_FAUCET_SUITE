#![allow(
    clippy::cmp_owned,
    clippy::collapsible_if,
    clippy::len_zero,
    clippy::manual_split_once,
    clippy::unnecessary_cast,
    clippy::while_let_on_iterator
)]

use anyhow::{bail, Context, Result};
use candid::{decode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::account::relay_setup_subaccount;
use jupiter_ic_clients::account_identifier::account_identifier_text;
use jupiter_ic_clients::constants as ic_constants;
use num_traits::ToPrimitive;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::Command;
use std::time::Instant;

mod cli;
mod constants;
mod process;
mod test_runner;
mod workspace;

use cli::{parse_scoped_command, TestComponent, TestScope};
use constants::*;
use process::{
    local_replica_host, pocketic_test_env, principal_of_identity, run_icp, run_icp_with_identity,
    stop_local_network_best_effort,
};
use test_runner::run_cargo_test_suite;
use workspace::repo_root;

#[derive(Debug)]
struct ScenarioOutcome {
    name: String,
    ms: u128,
    passed: bool,
    error: Option<String>,
}

fn run_scenario<F, N>(outcomes: &mut Vec<ScenarioOutcome>, name: N, f: F)
where
    F: FnOnce() -> anyhow::Result<()>,
    N: Into<String>,
{
    let name = name.into();
    eprintln!("\n{BOLD}=== Scenario: {name} ==={RESET}");
    let t0 = Instant::now();

    match f() {
        Ok(()) => {
            let ms = t0.elapsed().as_millis();
            outcomes.push(ScenarioOutcome {
                name: name.clone(),
                ms,
                passed: true,
                error: None,
            });
            eprintln!("{GREEN}✓{RESET} {name} {DIM}({ms}ms){RESET}");
        }
        Err(e) => {
            let ms = t0.elapsed().as_millis();
            let msg = format!("{e:#}");
            outcomes.push(ScenarioOutcome {
                name: name.clone(),
                ms,
                passed: false,
                error: Some(msg.clone()),
            });
            eprintln!("{RED}✗{RESET} {name} {DIM}({ms}ms){RESET}");
            eprintln!("{DIM}  {msg}{RESET}");
        }
    }
}

fn label(layer: &str, component: &str, name: &str) -> String {
    if component.is_empty() {
        format!("[{layer}] {name}")
    } else {
        format!("[{layer}/{component}] {name}")
    }
}

fn short_test_principal() -> Principal {
    Principal::from_slice(&[1])
}

fn print_summary(outcomes: &[ScenarioOutcome]) -> bool {
    let passed = outcomes.iter().filter(|o| o.passed).count();
    let failed = outcomes.len().saturating_sub(passed);

    if failed == 0 {
        eprintln!(
            "\n{GREEN}{BOLD}✅ xtask:test PASSED{RESET} {DIM}({} checks){RESET}",
            outcomes.len()
        );
    } else {
        eprintln!(
            "\n{RED}{BOLD}❌ xtask:test FAILED{RESET} {DIM}({} checks; {passed} passed, {failed} failed){RESET}",
            outcomes.len()
        );
    }

    for o in outcomes {
        if o.passed {
            eprintln!("  {GREEN}✓{RESET} {} {DIM}({}ms){RESET}", o.name, o.ms);
        } else {
            eprintln!("  {RED}✗{RESET} {} {DIM}({}ms){RESET}", o.name, o.ms);
        }
    }

    if failed != 0 {
        eprintln!("\n{BOLD}Failures:{RESET}");
        for o in outcomes.iter().filter(|o| !o.passed) {
            if let Some(err) = &o.error {
                eprintln!("  {RED}✗{RESET} {BOLD}{}{RESET}", o.name);
                eprintln!("    {DIM}{}{RESET}", err.replace('\n', "\n    "));
            }
        }
        eprintln!();
    } else {
        eprintln!();
    }

    failed == 0
}

fn run_cargo_build(package: &str, features: &[&str]) -> Result<()> {
    let root = repo_root();
    let mut args = vec![
        "build".to_string(),
        "--target".to_string(),
        "wasm32-unknown-unknown".to_string(),
        "--release".to_string(),
        "-p".to_string(),
        package.to_string(),
        "--locked".to_string(),
    ];
    if !features.is_empty() {
        args.push("--features".to_string());
        args.push(features.join(","));
    }

    eprintln!("▶ cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(&args)
        .current_dir(&root)
        .status()
        .with_context(|| format!("failed to build {package}"))?;
    if !status.success() {
        bail!("cargo build failed for {package}");
    }
    Ok(())
}

fn candid_path_for_canister(canister: &str) -> Option<String> {
    let repo = repo_root();
    let relative = match canister {
        "jupiter_disburser_dbg" | "jupiter_disburser_args_dbg" => {
            "canisters/disburser/jupiter_disburser_debug.did"
        }
        "jupiter_faucet_dbg" | "jupiter_faucet_args_dbg" => {
            "canisters/faucet/jupiter_faucet_debug.did"
        }
        "jupiter_historian_dbg" | "jupiter_historian_args_dbg" => {
            "canisters/historian/jupiter_historian_debug.did"
        }
        "jupiter_relay_dbg" | "jupiter_relay_args_dbg" => "canisters/relay/jupiter_relay_debug.did",
        "mock_icrc_ledger" => "tests/mocks/mock-icrc-ledger/mock_icrc_ledger.did",
        "mock_nns_governance" => "tests/mocks/mock-nns-governance/mock_nns_governance.did",
        "mock_icp_index" => "tests/mocks/mock-icp-index/mock_icp_index.did",
        "mock_cmc" => "tests/mocks/mock-cmc/mock_cmc.did",
        "mock_xrc" => "tests/mocks/mock-xrc/mock_xrc.did",
        "mock_blackhole" => "tests/mocks/mock-blackhole/mock_blackhole.did",
        "mock_sns_wasm" => "tests/mocks/mock-sns-wasm/mock_sns_wasm.did",
        "mock_sns_root" => "tests/mocks/mock-sns-root/mock_sns_root.did",
        _ => return None,
    };
    let path = std::path::Path::new(&repo)
        .join(relative)
        .canonicalize()
        .ok()?;
    Some(path.to_string_lossy().into_owned())
}

fn embed_candid_metadata(canister: &str) -> Result<()> {
    let Some(candid) = candid_path_for_canister(canister) else {
        return Ok(());
    };
    let wasm = wasm_path_for_canister(canister)?;
    let output = format!("{wasm}.metadata");
    let status = Command::new("ic-wasm")
        .args([
            wasm.as_str(),
            "-o",
            output.as_str(),
            "metadata",
            "candid:service",
            "--file",
            candid.as_str(),
            "--visibility",
            "public",
        ])
        .status()
        .with_context(|| format!("failed to embed Candid metadata for {canister}"))?;
    if !status.success() {
        bail!("ic-wasm metadata failed for {canister}");
    }
    fs::rename(&output, &wasm).with_context(|| {
        format!("failed to replace Wasm with metadata-enhanced output for {canister}")
    })?;
    Ok(())
}

fn build_canister_wasm(canister: &str) -> Result<()> {
    match canister {
        "mock_icrc_ledger" => run_cargo_build("mock-icrc-ledger", &[]),
        "mock_nns_governance" => run_cargo_build("mock-nns-governance", &[]),
        "mock_icp_index" => run_cargo_build("mock-icp-index", &[]),
        "mock_cmc" => run_cargo_build("mock-cmc", &[]),
        "mock_xrc" => run_cargo_build("mock-xrc", &[]),
        "mock_blackhole" => run_cargo_build("mock-blackhole", &[]),
        "mock_sns_wasm" => run_cargo_build("mock-sns-wasm", &[]),
        "mock_sns_root" => run_cargo_build("mock-sns-root", &[]),
        "jupiter_disburser_dbg" | "jupiter_disburser_args_dbg" => {
            run_cargo_build("jupiter-disburser", &["debug_api"])
        }
        "jupiter_faucet_dbg" | "jupiter_faucet_args_dbg" => {
            run_cargo_build("jupiter-faucet", &["debug_api"])
        }
        "jupiter_historian_dbg" | "jupiter_historian_args_dbg" => {
            run_cargo_build("jupiter-historian", &["debug_api"])
        }
        "jupiter_relay_dbg" | "jupiter_relay_args_dbg" => {
            run_cargo_build("jupiter-relay", &["debug_api"])
        }
        _ => bail!("no local build mapping configured for {canister}"),
    }?;
    embed_candid_metadata(canister)
}

fn call_raw<T>(canister: &str, method: &str, args: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de> + CandidType,
{
    let encoded_args = encode_call_args(canister, method, args)?;
    let cmd: Vec<String> = vec![
        "canister".into(),
        "call".into(),
        "--environment".into(),
        LOCAL_ENVIRONMENT.into(),
        canister.into(),
        method.into(),
        encoded_args,
        "--args-format".into(),
        "hex".into(),
        "--output".into(),
        "hex".into(),
    ];
    let refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
    let out = run_icp_with_identity(&refs)?;
    let hex_str = out.trim().trim_start_matches("0x");
    let bytes = hex::decode(hex_str)?;
    Ok(decode_one(&bytes)?)
}

fn encode_call_args(canister: &str, method: &str, args: &str) -> Result<String> {
    let candid = candid_path_for_canister(canister)
        .with_context(|| format!("no Candid file configured for {canister}"))?;
    let output = Command::new("didc")
        .args([
            "encode",
            "--defs",
            candid.as_str(),
            "--method",
            method,
            args,
        ])
        .output()
        .with_context(|| format!("failed to spawn didc for {canister}.{method}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "didc encode failed for {canister}.{method}: {}",
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn call_raw_noargs<T>(canister: &str, method: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de> + CandidType,
{
    call_raw(canister, method, "()")
}

fn canister_id(name: &str) -> Result<String> {
    let out = run_icp_with_identity(&[
        "canister",
        "status",
        name,
        "--id-only",
        "--environment",
        LOCAL_ENVIRONMENT,
    ])?;
    Ok(out.trim().to_string())
}

fn try_canister_id(name: &str) -> Result<Option<String>> {
    let root = repo_root();
    let output = Command::new("icp")
        .args([
            "--project-root-override",
            &root,
            "canister",
            "status",
            name,
            "--id-only",
            "--environment",
            LOCAL_ENVIRONMENT,
            "--identity",
            LOCAL_IDENTITY,
        ])
        .output()
        .with_context(|| format!("failed to lookup canister ID for {name}"))?;

    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not find ID for canister")
        || stderr.contains("failed to lookup canister ID")
    {
        return Ok(None);
    }

    bail!("failed to lookup canister ID for {name}: {}", stderr.trim());
}

fn ensure_canister_exists(canister: &str) -> Result<()> {
    if try_canister_id(canister)?.is_none() {
        create_canister(canister)?;
    }
    Ok(())
}

#[derive(Debug, CandidType, Deserialize)]
struct TransferRecord {
    from: Account,
    to: Account,
    amount: Nat,
    fee: Nat,
    memo: Option<Vec<u8>>,
    created_at_time: Option<u64>,
    result: String,
}

#[derive(Debug, CandidType, Deserialize)]
struct Tokens {
    e8s: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct LegacyTransferRecord {
    from: Account,
    to_account_identifier_hex: String,
    amount: Tokens,
    fee: Tokens,
    memo: u64,
    created_at_time: Option<u64>,
    result: String,
}

#[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
enum ForcedRescueReason {
    BootstrapNoSuccess,
    IndexAnchorMissing,
    IndexLatestInvariantBroken,
    IndexLatestUnreadable,
    CmcZeroSuccessRuns,
    AccountingInvariantBroken,
    FundingTrancheBalanceMismatch,
    FundingDiscoveryUnreadable,
}

#[derive(Debug, CandidType, Deserialize)]
struct DebugState {
    prev_age_seconds: u64,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    payout_plan_present: bool,
    payout_plan_transfer_count: u64,
    last_main_run_ts: u64,
    main_lock_state_ts: Option<u64>,
    blackhole_armed_since_ts: Option<u64>,
    forced_rescue_reason: Option<ForcedRescueReason>,
}

#[derive(Debug, CandidType, Deserialize)]
struct DisburserDebugConfig {
    neuron_id: u64,
    normal_recipient: Account,
    age_bonus_recipient_1: Account,
    age_bonus_recipient_2: Account,
    ledger_canister_id: Principal,
    governance_canister_id: Principal,
    rescue_controller: Principal,
    blackhole_controller: Option<Principal>,
    blackhole_armed: Option<bool>,
    main_interval_seconds: u64,
    rescue_interval_seconds: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct FaucetDebugAccounts {
    payout: Account,
    staking: Account,
}

#[derive(Debug, CandidType, Deserialize)]
struct FaucetDebugConfig {
    staking_account: Account,
    payout_subaccount: Option<Vec<u8>>,
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    cmc_canister_id: Principal,
    governance_canister_id: Principal,
    funding_source_account: Account,
    rescue_controller: Principal,
    blackhole_controller: Option<Principal>,
    blackhole_armed: Option<bool>,
    expected_first_staking_tx_id: Option<u64>,
    main_interval_seconds: u64,
    rescue_interval_seconds: u64,
    min_tx_e8s: u64,
    stake_recognition_delay_seconds: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct FaucetDebugState {
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    active_payout_job_present: bool,
    last_summary_present: bool,
    blackhole_armed_since_ts: Option<u64>,
    forced_rescue_reason: Option<ForcedRescueReason>,
    consecutive_index_anchor_failures: u8,
    consecutive_index_latest_invariant_failures: u8,
    consecutive_cmc_zero_success_runs: u8,
    last_observed_staking_balance_e8s: Option<u64>,
    last_observed_latest_tx_id: Option<u64>,
    expected_first_staking_tx_id: Option<u64>,
}

#[derive(Debug, CandidType, Deserialize)]
struct FaucetSummary {
    pot_start_e8s: u64,
    pot_remaining_e8s: u64,
    denom_staking_balance_e8s: u64,
    topped_up_count: u64,
    topped_up_sum_e8s: u64,
    topped_up_min_e8s: Option<u64>,
    topped_up_max_e8s: Option<u64>,
    failed_topups: u64,
    #[serde(default)]
    ambiguous_topups: u64,
    ignored_under_threshold: u64,
    ignored_bad_memo: u64,
    remainder_to_self_e8s: u64,
}

#[derive(Debug, CandidType, Deserialize)]
enum HistorianCanisterSource {
    MemoCommitment,
    SnsDiscovery,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianCanisterListItem {
    canister_id: Principal,
    sources: Vec<HistorianCanisterSource>,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianListCanistersResponse {
    items: Vec<HistorianCanisterListItem>,
    next_start_after: Option<Principal>,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianCommitmentSample {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianCommitmentHistoryPage {
    items: Vec<HistorianCommitmentSample>,
    next_start_after_tx_id: Option<u64>,
}

#[derive(Debug, CandidType, Deserialize)]
enum HistorianCyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianCyclesSample {
    timestamp_nanos: u64,
    cycles: Nat,
    source: HistorianCyclesSampleSource,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianCyclesHistoryPage {
    items: Vec<HistorianCyclesSample>,
    next_start_after_ts: Option<u64>,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianPublicCounts {
    registered_canister_count: u64,
    qualifying_commitment_count: u64,
    sns_discovered_canister_count: u64,
    total_output_e8s: u64,
    total_rewards_e8s: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianIcpXdrRateSnapshot {
    rate: u64,
    decimals: u32,
    timestamp: u64,
    fetched_at_ts: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianPublicStatus {
    staking_account: Account,
    ledger_canister_id: Principal,
    last_index_run_ts: Option<u64>,
    index_interval_seconds: u64,
    last_completed_cycles_sweep_ts: Option<u64>,
    cycles_interval_seconds: u64,
    icp_xdr_rate: Option<HistorianIcpXdrRateSnapshot>,
    last_icp_xdr_rate_error: Option<String>,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianDebugConfig {
    staking_account: Account,
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    cmc_canister_id: Option<Principal>,
    faucet_canister_id: Option<Principal>,
    blackhole_canister_id: Principal,
    sns_wasm_canister_id: Principal,
    xrc_canister_id: Principal,
    enable_sns_tracking: bool,
    scan_interval_seconds: u64,
    cycles_interval_seconds: u64,
    min_tx_e8s: u64,
    max_cycles_entries_per_canister: u32,
    max_commitment_entries_per_canister: u32,
    max_index_pages_per_tick: u32,
    max_canisters_per_cycles_tick: u32,
}

#[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayRegistryKind {
    Canonical,
    SelfService,
}

#[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayRegistryStatus {
    Pending,
    Active,
    Failed,
    Superseded,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelayRegistryEntry {
    relay_canister_id: Principal,
    target_canister_id: Principal,
    kind: RelayRegistryKind,
    status: RelayRegistryStatus,
    setup_account: Option<Account>,
    setup_account_identifier: Option<String>,
    setup_amount_e8s: Option<u64>,
    setup_tx_ids: Vec<u64>,
    relay_wasm_hash_hex: Option<String>,
    final_controllers: Option<Vec<Principal>>,
    log_visibility_public: Option<bool>,
    created_at_ts: Option<u64>,
    activated_at_ts: Option<u64>,
}

#[derive(Debug, CandidType, Deserialize, PartialEq, Eq, Clone)]
enum RelaySetupStatus {
    NotFunded,
    BelowMinimum,
    InsufficientForCurrentRate,
    TargetNotObservable,
    Pending,
    ConvertingCycles,
    CycleTransferAccepted,
    CycleNotifySucceeded,
    CreatingCanister,
    CanisterCreated,
    InstallingCode,
    CodeInstalled,
    SettingPublicLogs,
    FundingRelaySubaccountOne,
    Blackholing,
    Active,
    SweepingToExistingRelay,
    SweptToExistingRelay,
    SweepBelowDust,
    RefundAvailable,
    Refunding,
    Refunded,
    FailedRetryable,
    FailedTerminal,
    Ambiguous,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelaySetupJobView {
    target_canister_id: Principal,
    status: RelaySetupStatus,
    relay_canister_id: Option<Principal>,
    setup_amount_seen_e8s: u64,
    setup_amount_processed_e8s: u64,
    cycle_conversion_e8s: Option<u64>,
    relay_funding_e8s: Option<u64>,
    last_error: Option<String>,
    updated_at_ts: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelaySetupView {
    target_canister_id: Principal,
    setup_account: Account,
    setup_account_identifier: String,
    minimum_e8s: u64,
    dust_e8s: u64,
    current_status: Option<RelaySetupStatus>,
    existing_relay: Option<RelayRegistryEntry>,
    setup_job: Option<RelaySetupJobView>,
    factory_enabled: bool,
    relay_wasm_hash_hex: Option<String>,
    warning_text: Option<String>,
}

#[derive(Debug, CandidType, Deserialize)]
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
        job: RelaySetupJobView,
    },
    Active {
        relay: RelayRegistryEntry,
    },
    SweptToExistingRelay {
        relay: RelayRegistryEntry,
        amount_e8s: u64,
        block_index: u64,
    },
    SweepBelowDust {
        relay: RelayRegistryEntry,
        current_balance_e8s: u64,
    },
    Failed {
        status: RelaySetupStatus,
        message: String,
    },
}

#[derive(Debug, CandidType, Deserialize)]
enum RelaySetupRefundResult {
    NotEligible { status: Option<RelaySetupStatus> },
    Cooldown { retry_after_seconds: u64 },
    Refunded { blocks: Vec<u64> },
    NoRefundableAmount,
    Failed { message: String },
}

#[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayMode {
    BaselineOnly,
    TopUpThenSurplus,
    Degraded,
    NoFunds,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelayCanisterBurnSample {
    canister_id: Principal,
    previous_cycles: Option<Nat>,
    current_cycles: Nat,
    relay_minted_cycles: Nat,
    burn_cycles: Nat,
    carried_deficit_cycles: Nat,
    target_topup_cycles: Nat,
    gross_share_e8s: u64,
    amount_e8s: u64,
    sent_topup_e8s: u64,
    actual_minted_cycles: Nat,
    remaining_deficit_cycles: Nat,
    skipped_reason: Option<String>,
}

#[derive(Debug, CandidType, Deserialize)]
enum RelaySurplusTarget {
    Canister(Principal),
    Neuron(u64),
}

#[derive(Debug, CandidType, Deserialize)]
struct RelaySurplusTransferSample {
    target: RelaySurplusTarget,
    account: Account,
    gross_share_e8s: u64,
    amount_e8s: u64,
    memo_len: Option<u32>,
    skipped_reason: Option<String>,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelayConversionEstimate {
    cycles_per_e8: Nat,
    timestamp_nanos: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelayProbeFailure {
    canister_id: Principal,
    error: String,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelaySummary {
    mode: RelayMode,
    started_at_ts_nanos: u64,
    completed_at_ts_nanos: Option<u64>,
    default_account_balance_start_e8s: u64,
    fee_e8s: u64,
    managed_canister_count: u32,
    min_cycles_balance: Option<Nat>,
    total_burn_cycles: Nat,
    total_target_topup_cycles: Nat,
    total_actual_minted_cycles: Nat,
    total_carried_deficit_cycles: Nat,
    total_remaining_deficit_cycles: Nat,
    deficit_canister_count: u32,
    transfer_count: u32,
    ledger_transfer_count: u32,
    ledger_sent_e8s: u64,
    ledger_fees_e8s: u64,
    cmc_notify_success_count: u32,
    cmc_notify_failed_count: u32,
    cmc_notify_ambiguous_count: u32,
    planned_retained_e8s: u64,
    known_unspent_e8s: u64,
    ambiguous_e8s: u64,
    failed_transfers: u32,
    ambiguous_transfers: u32,
    partial_tick_count: u32,
    probe_failures: Vec<RelayProbeFailure>,
    canisters: Vec<RelayCanisterBurnSample>,
    conversion_estimate_used: Option<RelayConversionEstimate>,
    surplus_e8s_before_fees: u64,
    surplus_transfers: Vec<RelaySurplusTransferSample>,
    skipped_surplus_reason: Option<String>,
}

#[derive(Debug, CandidType, Deserialize)]
struct RelayDebugConfig {
    managed_canisters: Vec<Principal>,
    effective_managed_canisters: Vec<Principal>,
    ledger_canister_id: Principal,
    cmc_canister_id: Principal,
    governance_canister_id: Principal,
    blackhole_canister_id: Principal,
    main_interval_seconds: u64,
    max_transfers_per_tick: Option<u32>,
}

#[derive(Debug, CandidType, Deserialize)]
enum DebugRefreshIcpXdrRateResult {
    Ok,
    Err(String),
}

#[derive(Debug, CandidType, Deserialize)]
struct MockXrcCall {
    base_symbol: String,
    quote_symbol: String,
    requested_timestamp: Option<u64>,
    attached_cycles: Nat,
    accepted_cycles: Nat,
}

#[derive(Debug, CandidType, Deserialize)]
struct HistorianRegisteredCanisterSummary {
    canister_id: Principal,
    qualifying_commitment_count: u64,
    total_qualifying_committed_e8s: u64,
    last_commitment_ts: Option<u64>,
    latest_cycles: Option<Nat>,
    last_cycles_probe_ts: Option<u64>,
}

#[derive(Debug, CandidType, Deserialize)]
struct ListRegisteredCanisterSummariesResponse {
    items: Vec<HistorianRegisteredCanisterSummary>,
    page: u32,
    page_size: u32,
    total: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct RecentCommitmentListItem {
    canister_id: Option<Principal>,
    neuron_id: Option<u64>,
    raw_icp_memo_text: Option<String>,
    neuron_memo_text: Option<String>,
    memo_text: Option<String>,
    tx_id: u64,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Debug, CandidType, Deserialize)]
struct ListRecentCommitmentsResponse {
    items: Vec<RecentCommitmentListItem>,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpected {
    stakeE8s: String,
    counts: FrontendDashboardExpectedCounts,
    status: FrontendDashboardExpectedStatus,
    registered: FrontendDashboardExpectedRegistered,
    recent: FrontendDashboardExpectedRecent,
    errors: FrontendDashboardExpectedErrors,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedCounts {
    registeredCanisterCount: String,
    qualifyingCommitmentCount: String,
    totalOutputE8s: String,
    totalRewardsE8s: String,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedStatus {
    ledgerCanisterId: String,
    indexIntervalSeconds: String,
    cyclesIntervalSeconds: String,
    stakingAccountIdentifier: String,
    lastIndexRunTsPresent: bool,
    lastCyclesSweepTsPresent: bool,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedRegistered {
    total: String,
    items: Vec<FrontendDashboardExpectedRegisteredItem>,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedRegisteredItem {
    canisterId: String,
    qualifyingCommitmentCount: String,
    totalQualifyingCommittedE8s: String,
    lastCommitmentTsPresent: bool,
    latestCycles: Option<String>,
    lastCyclesProbeTsPresent: bool,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedRecent {
    items: Vec<FrontendDashboardExpectedRecentItem>,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedRecentItem {
    canisterId: String,
    txId: String,
    amountE8s: String,
    countsTowardFaucet: bool,
}

#[allow(non_snake_case)]
#[derive(Debug, serde::Serialize)]
struct FrontendDashboardExpectedErrors {
    stake: Option<String>,
}

#[derive(Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Debug, CandidType, Deserialize)]
struct IndexGetCall {
    account_identifier: String,
    start: Option<u64>,
    max_results: u64,
    returned_count: u64,
}

fn nat_plain_string(value: &Nat) -> String {
    value.to_string().replace('_', "")
}

fn nat_to_u64(value: &Nat) -> u64 {
    value.0.to_u64_digits().first().copied().unwrap_or(0)
}

fn bytes_to_candid_blob(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!(r"\{:02x}", b)).collect()
}

fn account_to_candid(account: &Account) -> String {
    let subaccount = match account.subaccount {
        Some(bytes) => format!(
            "opt vec {{ {} }}",
            bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ),
        None => "null".to_string(),
    };
    format!(
        "record {{ owner = principal \"{}\"; subaccount = {} }}",
        account.owner.to_text(),
        subaccount
    )
}

fn relay_setup_view(target: Principal) -> Result<RelaySetupView> {
    call_raw(
        "jupiter_historian_dbg",
        "get_relay_setup_view",
        &format!(
            r#"(record {{ target_canister_id = principal "{}" }})"#,
            target.to_text()
        ),
    )
}

fn append_setup_payment(
    source: Account,
    setup_account_identifier: &str,
    amount_e8s: u64,
) -> Result<u64> {
    let source_id = account_identifier_text(source.owner, source.subaccount);
    call_raw(
        "mock_icp_index",
        "debug_append_transfer_from",
        &format!(
            r#"("{}", "{}", {}:nat64, null)"#,
            source_id, setup_account_identifier, amount_e8s
        ),
    )
}

fn local_faucet_funding_source_account() -> Result<Account> {
    Ok(Account {
        owner: Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai")?,
        subaccount: None,
    })
}

fn local_faucet_funding_source_candid() -> Result<String> {
    Ok(account_to_candid(&local_faucet_funding_source_account()?))
}

fn append_local_faucet_funding_tranche(payout: &Account, amount_e8s: u64) -> Result<u64> {
    let cfg: FaucetDebugConfig = call_raw_noargs("jupiter_faucet_dbg", "debug_config")?;
    let funding_source = cfg.funding_source_account;
    let funding_source_id =
        account_identifier_text(funding_source.owner, funding_source.subaccount);
    let payout_id = account_identifier_text(payout.owner, payout.subaccount);
    let timestamp_nanos = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock before UNIX epoch")?
        .as_nanos() as u64)
        .saturating_add(
            cfg.stake_recognition_delay_seconds
                .saturating_add(1)
                .saturating_mul(1_000_000_000),
        );
    call_raw(
        "mock_icp_index",
        "debug_append_transfer_from_with_timestamp",
        &format!(
            "(\"{}\", \"{}\", {}:nat64, null, {}:nat64)",
            funding_source_id, payout_id, amount_e8s, timestamp_nanos
        ),
    )
}

fn relay_local_logs(canister_name: &str) -> Result<String> {
    run_icp_with_identity(&[
        "canister",
        "logs",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister_name,
    ])
}

fn opt_blob_to_candid(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => format!(
            "opt vec {{ {} }}",
            bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ),
        None => "null".to_string(),
    }
}

fn opt_nat64_to_candid(v: u64) -> String {
    format!("(opt ({}:nat64))", v)
}

fn ensure_identity() -> Result<()> {
    // If identity already exists, this returns OK.
    let list = Command::new("icp")
        .args(["identity", "list", "--quiet"])
        .output()
        .context("failed to run icp identity list")?;

    if !list.status.success() {
        bail!("icp identity list failed");
    }

    let stdout = String::from_utf8_lossy(&list.stdout);
    let exists = stdout
        .lines()
        .any(|l| l.trim().trim_start_matches('*').trim() == LOCAL_IDENTITY);

    if exists {
        return Ok(());
    }

    eprintln!("▶ creating non-interactive icp identity: {LOCAL_IDENTITY}");

    // Create plaintext storage identity (no passphrase prompts).
    let output = Command::new("icp")
        .args(["identity", "new", LOCAL_IDENTITY, "--storage", "plaintext"])
        .output()
        .context("failed to run icp identity new")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim_end());
        }
        bail!("icp identity new {LOCAL_IDENTITY} failed");
    }

    Ok(())
}

fn cmd_setup_common() -> Result<()> {
    ensure_identity()?;

    // Stop any running local replica, treating an already-stopped network as clean.
    let project_root = repo_root();
    stop_local_network_best_effort(&project_root)?;
    let cache_dir = std::path::Path::new(&project_root)
        .join(".icp")
        .join("cache");
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir)
            .with_context(|| format!("failed to clear {}", cache_dir.display()))?;
    }
    run_icp(&["network", "start", "-d", LOCAL_ENVIRONMENT]).context("icp network start failed")?;
    run_icp(&["network", "ping", "--wait-healthy", LOCAL_ENVIRONMENT])?;

    Ok(())
}

fn deploy_local_canister(canister: &str, args: Option<&str>) -> Result<()> {
    build_canister_wasm(canister)?;
    let wasm = wasm_path_for_canister(canister)?;
    run_icp_with_identity(&[
        "canister",
        "create",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister,
        "--quiet",
    ])?;
    let mut install = vec![
        "canister",
        "install",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister,
        "--wasm",
        wasm.as_str(),
        "--mode",
        "reinstall",
        "--yes",
    ];
    if let Some(args) = args {
        install.push("--args");
        install.push(args);
    }
    run_icp_with_identity(&install)?;
    Ok(())
}

fn add_self_controller(canister: &str) -> Result<()> {
    let canister_id = canister_id(canister)?;
    run_icp_with_identity(&[
        "canister",
        "settings",
        "update",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister,
        "--add-controller",
        canister_id.trim(),
        "--force",
    ])?;
    Ok(())
}

fn faucet_staking_account() -> Account {
    Account {
        owner: short_test_principal(),
        subaccount: Some([9u8; 32]),
    }
}

fn mainnet_governance_principal() -> Principal {
    ic_constants::nns_governance_id()
}

fn mainnet_ledger_principal() -> Principal {
    ic_constants::icp_ledger_id()
}

fn mainnet_index_principal() -> Principal {
    ic_constants::icp_index_id()
}

fn mainnet_cmc_principal() -> Principal {
    ic_constants::cycles_minting_canister_id()
}

fn mainnet_blackhole_principal() -> Principal {
    ic_constants::blackhole_canister_id()
}

fn mainnet_sns_wasm_principal() -> Principal {
    ic_constants::sns_wasm_id()
}

fn mainnet_xrc_principal() -> Principal {
    Principal::from_text("uf6dk-hyaaa-aaaaq-qaaaq-cai").expect("valid XRC principal")
}

fn prod_lifeline_principal() -> Principal {
    Principal::from_text("afisn-gqaaa-aaaar-qb4qa-cai").expect("valid lifeline principal")
}

fn prod_faucet_principal() -> Principal {
    Principal::from_text("acjuz-liaaa-aaaar-qb4qq-cai").expect("valid faucet principal")
}

fn prod_sns_rewards_principal() -> Principal {
    Principal::from_text("alk7f-5aaaa-aaaar-qb4ra-cai").expect("valid sns rewards principal")
}

fn expected_mainnet_staking_subaccount() -> [u8; 32] {
    [
        255, 12, 11, 54, 175, 239, 255, 208, 199, 164, 216, 92, 11, 206, 163, 102, 172, 214, 215,
        79, 69, 247, 112, 61, 7, 131, 204, 100, 72, 137, 156, 104,
    ]
}

fn expected_dquorum_subaccount() -> [u8; 32] {
    [
        119, 230, 61, 231, 43, 94, 51, 57, 234, 32, 244, 186, 243, 236, 43, 217, 33, 56, 221, 222,
        13, 174, 182, 157, 181, 10, 204, 235, 56, 75, 223, 15,
    ]
}

fn expected_mainnet_staking_account() -> Account {
    Account {
        owner: mainnet_governance_principal(),
        subaccount: Some(expected_mainnet_staking_subaccount()),
    }
}

fn cmd_setup_disburser_local() -> Result<()> {
    cmd_setup_common()?;

    deploy_local_canister("mock_icrc_ledger", None)?;
    deploy_local_canister("mock_nns_governance", None)?;
    deploy_local_canister("mock_blackhole", None)?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let gov_id = canister_id("mock_nns_governance")?;
    let blackhole_id = canister_id("mock_blackhole")?;
    let rescue = principal_of_identity()?;

    let r1 = Principal::management_canister();
    let r2 = short_test_principal();
    let r3 = rescue;

    let args = format!(
        r#"(record {{
            neuron_id = 1:nat64;
            normal_recipient = record {{ owner = principal "{r1}"; subaccount = null }};
            age_bonus_recipient_1 = record {{ owner = principal "{r2}"; subaccount = null }};
            age_bonus_recipient_2 = record {{ owner = principal "{r3}"; subaccount = null }};

            ledger_canister_id = opt principal "{ledger_id}";
            governance_canister_id = opt principal "{gov_id}";
            rescue_controller = principal "{r3}";
            blackhole_controller = opt principal "{blackhole_id}";
            blackhole_armed = opt true;

            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);
        }},)"#,
        r1 = r1.to_text(),
        r2 = r2.to_text(),
        r3 = r3.to_text(),
    );

    deploy_local_canister("jupiter_disburser_dbg", Some(&args))?;
    add_self_controller("jupiter_disburser_dbg")?;

    Ok(())
}

fn cmd_setup_faucet_local() -> Result<()> {
    cmd_setup_common()?;

    deploy_local_canister("mock_icrc_ledger", None)?;
    deploy_local_canister("mock_icp_index", None)?;
    deploy_local_canister("mock_cmc", None)?;
    deploy_local_canister("mock_nns_governance", None)?;
    deploy_local_canister("mock_blackhole", None)?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let index_id = canister_id("mock_icp_index")?;
    let cmc_id = canister_id("mock_cmc")?;
    let gov_id = canister_id("mock_nns_governance")?;
    let blackhole_id = canister_id("mock_blackhole")?;
    let faucet_staking_account = faucet_staking_account();
    let faucet_rescue = Principal::from_text(cmc_id.trim())?;
    let funding_source = local_faucet_funding_source_candid()?;
    let faucet_args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt blob "{staking_subaccount}" }};
            payout_subaccount = null;
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            governance_canister_id = opt principal "{gov_id}";
            funding_source_account = {funding_source};
            rescue_controller = principal "{faucet_rescue}";
            blackhole_controller = opt principal "{blackhole_id}";
            blackhole_armed = opt false;
            expected_first_staking_tx_id = null;
            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);
            min_tx_e8s = opt (100000000:nat64);
        }},)"#,
        staking_owner = faucet_staking_account.owner.to_text(),
        staking_subaccount = bytes_to_candid_blob(&faucet_staking_account.subaccount.unwrap()),
        ledger_id = ledger_id.trim(),
        index_id = index_id.trim(),
        cmc_id = cmc_id.trim(),
        gov_id = gov_id.trim(),
        funding_source = funding_source,
        faucet_rescue = faucet_rescue.to_text(),
        blackhole_id = blackhole_id.trim(),
    );

    deploy_local_canister("jupiter_faucet_dbg", Some(&faucet_args))?;
    add_self_controller("jupiter_faucet_dbg")?;

    Ok(())
}

fn cmd_setup_historian_local() -> Result<()> {
    cmd_setup_common()?;

    deploy_local_canister("mock_icrc_ledger", None)?;
    deploy_local_canister("mock_nns_governance", None)?;
    deploy_local_canister("mock_icp_index", None)?;
    deploy_local_canister("mock_cmc", None)?;
    deploy_local_canister("mock_xrc", None)?;
    deploy_local_canister("mock_blackhole", None)?;
    deploy_local_canister("mock_sns_wasm", None)?;
    deploy_local_canister("mock_sns_root", None)?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let gov_id = canister_id("mock_nns_governance")?;
    let index_id = canister_id("mock_icp_index")?;
    let cmc_id = canister_id("mock_cmc")?;
    let xrc_id = canister_id("mock_xrc")?;
    let blackhole_id = canister_id("mock_blackhole")?;
    let sns_wasm_id = canister_id("mock_sns_wasm")?;
    let rescue = principal_of_identity()?;

    let r1 = Principal::management_canister();
    let r2 = short_test_principal();
    let r3 = rescue;

    let disburser_args = format!(
        r#"(record {{
            neuron_id = 1:nat64;
            normal_recipient = record {{ owner = principal "{r1}"; subaccount = null }};
            age_bonus_recipient_1 = record {{ owner = principal "{r2}"; subaccount = null }};
            age_bonus_recipient_2 = record {{ owner = principal "{r3}"; subaccount = null }};

            ledger_canister_id = opt principal "{ledger_id}";
            governance_canister_id = opt principal "{gov_id}";
            rescue_controller = principal "{r3}";
            blackhole_controller = opt principal "{blackhole_id}";
            blackhole_armed = opt true;

            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);
        }},)"#,
        r1 = r1.to_text(),
        r2 = r2.to_text(),
        r3 = r3.to_text(),
        ledger_id = ledger_id.trim(),
        gov_id = gov_id.trim(),
        blackhole_id = blackhole_id.trim(),
    );

    deploy_local_canister("jupiter_disburser_dbg", Some(&disburser_args))?;
    add_self_controller("jupiter_disburser_dbg")?;
    let disb_id = canister_id("jupiter_disburser_dbg")?;

    let faucet_staking_account = faucet_staking_account();
    let faucet_rescue = Principal::from_text(cmc_id.trim())?;
    let funding_source = account_to_candid(&Account {
        owner: Principal::from_text(disb_id.trim())?,
        subaccount: None,
    });
    let faucet_args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt blob "{staking_subaccount}" }};
            payout_subaccount = null;
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            governance_canister_id = opt principal "{gov_id}";
            funding_source_account = {funding_source};
            rescue_controller = principal "{faucet_rescue}";
            blackhole_controller = opt principal "{blackhole_id}";
            blackhole_armed = opt false;
            expected_first_staking_tx_id = null;
            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);
            min_tx_e8s = opt (100000000:nat64);
        }},)"#,
        staking_owner = faucet_staking_account.owner.to_text(),
        staking_subaccount = bytes_to_candid_blob(&faucet_staking_account.subaccount.unwrap()),
        ledger_id = ledger_id.trim(),
        index_id = index_id.trim(),
        cmc_id = cmc_id.trim(),
        gov_id = gov_id.trim(),
        funding_source = funding_source,
        faucet_rescue = faucet_rescue.to_text(),
        blackhole_id = blackhole_id.trim(),
    );
    deploy_local_canister("jupiter_faucet_dbg", Some(&faucet_args))?;
    add_self_controller("jupiter_faucet_dbg")?;
    let faucet_id = canister_id("jupiter_faucet_dbg")?;

    let faucet_accounts: FaucetDebugAccounts =
        call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
    let output_source_owner = Principal::from_text(disb_id.trim())?;
    let output_owner = faucet_accounts.payout.owner;
    let output_subaccount = faucet_accounts.payout.subaccount;
    let rewards_owner = short_test_principal();

    let historian_args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt blob "{staking_subaccount}" }};
            output_source_account = opt record {{ owner = principal "{output_source_owner}"; subaccount = null }};
            output_account = opt record {{ owner = principal "{output_owner}"; subaccount = {output_subaccount} }};
            rewards_account = opt record {{ owner = principal "{rewards_owner}"; subaccount = null }};
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            faucet_canister_id = opt principal "{faucet_id}";
            blackhole_canister_id = opt principal "{blackhole_id}";
            sns_wasm_canister_id = opt principal "{sns_wasm_id}";
            xrc_canister_id = opt principal "{xrc_id}";
            enable_sns_tracking = opt true;
            scan_interval_seconds = opt (31536000:nat64);
            cycles_interval_seconds = opt (1:nat64);
            min_tx_e8s = opt (100000000:nat64);
            max_cycles_entries_per_canister = opt (100:nat32);
            max_commitment_entries_per_canister = opt (100:nat32);
            max_index_pages_per_tick = opt (10:nat32);
            max_canisters_per_cycles_tick = opt (10:nat32);
        }},)"#,
        staking_owner = faucet_staking_account.owner.to_text(),
        staking_subaccount = bytes_to_candid_blob(&faucet_staking_account.subaccount.unwrap()),
        output_source_owner = output_source_owner.to_text(),
        output_owner = output_owner.to_text(),
        output_subaccount = match output_subaccount {
            Some(bytes) => format!("opt blob \"{}\"", bytes_to_candid_blob(&bytes)),
            None => "null".to_string(),
        },
        rewards_owner = rewards_owner.to_text(),
        ledger_id = ledger_id.trim(),
        index_id = index_id.trim(),
        cmc_id = cmc_id.trim(),
        faucet_id = faucet_id.trim(),
        blackhole_id = blackhole_id.trim(),
        sns_wasm_id = sns_wasm_id.trim(),
        xrc_id = xrc_id.trim(),
    );
    deploy_local_canister("jupiter_historian_dbg", Some(&historian_args))?;
    add_self_controller("jupiter_historian_dbg")?;

    let relay_args = format!(
        r#"(record {{
            managed_canisters = vec {{ principal "{managed_id}" }};
            ledger_canister_id = opt principal "{ledger_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            governance_canister_id = opt principal "{cmc_id}";
            blackhole_canister_id = opt principal "{blackhole_id}";
            main_interval_seconds = opt (31536000:nat64);
            max_transfers_per_tick = opt (10:nat32);
            surplus_canister_recipients = null;
            surplus_neuron_recipients = vec {{}};
        }},)"#,
        managed_id = cmc_id.trim(),
        ledger_id = ledger_id.trim(),
        cmc_id = cmc_id.trim(),
        blackhole_id = blackhole_id.trim(),
    );
    deploy_local_canister("jupiter_relay_dbg", Some(&relay_args))?;
    add_self_controller("jupiter_relay_dbg")?;

    Ok(())
}

fn cmd_setup_relay_local() -> Result<()> {
    cmd_setup_common()?;

    deploy_local_canister("mock_icrc_ledger", None)?;
    deploy_local_canister("mock_cmc", None)?;
    deploy_local_canister("mock_blackhole", None)?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let cmc_id = canister_id("mock_cmc")?;
    let blackhole_id = canister_id("mock_blackhole")?;
    let relay_args = format!(
        r#"(record {{
            managed_canisters = vec {{ principal "{managed_id}" }};
            ledger_canister_id = opt principal "{ledger_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            governance_canister_id = opt principal "{cmc_id}";
            blackhole_canister_id = opt principal "{blackhole_id}";
            main_interval_seconds = opt (31536000:nat64);
            max_transfers_per_tick = opt (10:nat32);
            surplus_canister_recipients = null;
            surplus_neuron_recipients = vec {{}};
        }},)"#,
        managed_id = cmc_id.trim(),
        ledger_id = ledger_id.trim(),
        cmc_id = cmc_id.trim(),
        blackhole_id = blackhole_id.trim(),
    );

    deploy_local_canister("jupiter_relay_dbg", Some(&relay_args))?;
    add_self_controller("jupiter_relay_dbg")?;

    Ok(())
}

fn cmd_setup() -> Result<()> {
    cmd_setup_common()?;

    deploy_local_canister("mock_icrc_ledger", None)?;
    deploy_local_canister("mock_nns_governance", None)?;
    deploy_local_canister("mock_icp_index", None)?;
    deploy_local_canister("mock_cmc", None)?;
    deploy_local_canister("mock_xrc", None)?;
    deploy_local_canister("mock_blackhole", None)?;
    deploy_local_canister("mock_sns_wasm", None)?;
    deploy_local_canister("mock_sns_root", None)?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let gov_id = canister_id("mock_nns_governance")?;
    let index_id = canister_id("mock_icp_index")?;
    let cmc_id = canister_id("mock_cmc")?;
    let xrc_id = canister_id("mock_xrc")?;
    let blackhole_id = canister_id("mock_blackhole")?;
    let sns_wasm_id = canister_id("mock_sns_wasm")?;
    let rescue = principal_of_identity()?;

    let r1 = Principal::management_canister();
    let r2 = short_test_principal();
    let r3 = rescue;

    let args = format!(
        r#"(record {{
            neuron_id = 1:nat64;
            normal_recipient = record {{ owner = principal "{r1}"; subaccount = null }};
            age_bonus_recipient_1 = record {{ owner = principal "{r2}"; subaccount = null }};
            age_bonus_recipient_2 = record {{ owner = principal "{r3}"; subaccount = null }};
    
            ledger_canister_id = opt principal "{ledger_id}";
            governance_canister_id = opt principal "{gov_id}";
            rescue_controller = principal "{r3}";
            blackhole_controller = opt principal "{blackhole_id}";
            blackhole_armed = opt true;

            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);    
        }},)"#,
        r1 = r1.to_text(),
        r2 = r2.to_text(),
        r3 = r3.to_text(),
        ledger_id = ledger_id.trim(),
        gov_id = gov_id.trim(),
        blackhole_id = blackhole_id.trim(),
    );

    deploy_local_canister("jupiter_disburser_dbg", Some(&args))?;
    add_self_controller("jupiter_disburser_dbg")?;
    let disb_id = canister_id("jupiter_disburser_dbg")?;

    let faucet_staking_account = faucet_staking_account();
    let faucet_rescue = Principal::from_text(cmc_id.trim())?;
    let funding_source = account_to_candid(&Account {
        owner: Principal::from_text(disb_id.trim())?,
        subaccount: None,
    });
    let faucet_args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt blob "{staking_subaccount}" }};
            payout_subaccount = null;
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            governance_canister_id = opt principal "{gov_id}";
            funding_source_account = {funding_source};
            rescue_controller = principal "{faucet_rescue}";
            blackhole_controller = opt principal "{blackhole_id}";
            blackhole_armed = opt false;
            expected_first_staking_tx_id = null;
            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);
            min_tx_e8s = opt (100000000:nat64);
        }},)"#,
        staking_owner = faucet_staking_account.owner.to_text(),
        staking_subaccount = bytes_to_candid_blob(&faucet_staking_account.subaccount.unwrap()),
        ledger_id = ledger_id.trim(),
        index_id = index_id.trim(),
        cmc_id = cmc_id.trim(),
        gov_id = gov_id.trim(),
        funding_source = funding_source,
        faucet_rescue = faucet_rescue.to_text(),
        blackhole_id = blackhole_id.trim(),
    );

    deploy_local_canister("jupiter_faucet_dbg", Some(&faucet_args))?;
    add_self_controller("jupiter_faucet_dbg")?;
    let faucet_id = canister_id("jupiter_faucet_dbg")?;

    let faucet_accounts: FaucetDebugAccounts =
        call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
    let output_source_owner = Principal::from_text(disb_id.trim())?;
    let output_owner = faucet_accounts.payout.owner;
    let output_subaccount = faucet_accounts.payout.subaccount;
    let rewards_owner = short_test_principal();

    let historian_args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt blob "{staking_subaccount}" }};
            output_source_account = opt record {{ owner = principal "{output_source_owner}"; subaccount = null }};
            output_account = opt record {{ owner = principal "{output_owner}"; subaccount = {output_subaccount} }};
            rewards_account = opt record {{ owner = principal "{rewards_owner}"; subaccount = null }};
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            faucet_canister_id = opt principal "{faucet_id}";
            blackhole_canister_id = opt principal "{blackhole_id}";
            sns_wasm_canister_id = opt principal "{sns_wasm_id}";
            xrc_canister_id = opt principal "{xrc_id}";
            enable_sns_tracking = opt true;
            scan_interval_seconds = opt (31536000:nat64);
            cycles_interval_seconds = opt (1:nat64);
            min_tx_e8s = opt (100000000:nat64);
            max_cycles_entries_per_canister = opt (100:nat32);
            max_commitment_entries_per_canister = opt (100:nat32);
            max_index_pages_per_tick = opt (10:nat32);
            max_canisters_per_cycles_tick = opt (10:nat32);
        }},)"#,
        staking_owner = faucet_staking_account.owner.to_text(),
        staking_subaccount = bytes_to_candid_blob(&faucet_staking_account.subaccount.unwrap()),
        output_source_owner = output_source_owner.to_text(),
        output_owner = output_owner.to_text(),
        output_subaccount = match output_subaccount {
            Some(bytes) => format!("opt blob \"{}\"", bytes_to_candid_blob(&bytes)),
            None => "null".to_string(),
        },
        rewards_owner = rewards_owner.to_text(),
        ledger_id = ledger_id.trim(),
        index_id = index_id.trim(),
        cmc_id = cmc_id.trim(),
        faucet_id = faucet_id.trim(),
        blackhole_id = blackhole_id.trim(),
        sns_wasm_id = sns_wasm_id.trim(),
        xrc_id = xrc_id.trim(),
    );
    deploy_local_canister("jupiter_historian_dbg", Some(&historian_args))?;
    add_self_controller("jupiter_historian_dbg")?;

    let relay_args = format!(
        r#"(record {{
            managed_canisters = vec {{ principal "{managed_id}" }};
            ledger_canister_id = opt principal "{ledger_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            governance_canister_id = opt principal "{cmc_id}";
            blackhole_canister_id = opt principal "{blackhole_id}";
            main_interval_seconds = opt (31536000:nat64);
            max_transfers_per_tick = opt (10:nat32);
            surplus_canister_recipients = null;
            surplus_neuron_recipients = vec {{}};
        }},)"#,
        managed_id = cmc_id.trim(),
        ledger_id = ledger_id.trim(),
        cmc_id = cmc_id.trim(),
        blackhole_id = blackhole_id.trim(),
    );
    deploy_local_canister("jupiter_relay_dbg", Some(&relay_args))?;
    add_self_controller("jupiter_relay_dbg")?;

    Ok(())
}

fn cmd_teardown() -> Result<()> {
    let _ = run_icp(&["network", "stop", LOCAL_ENVIRONMENT])?;
    Ok(())
}

fn cmd_faucet_production_reinstall_cutover() -> Result<()> {
    println!(
        "Faucet reinstall is only for fresh deployments where no faucet payout has completed.\n\
         The current production faucet has paid out, so use upgrade with appropriate UpgradeArgs instead.\n\
         Do not pass canisters/faucet/mainnet-install-args.did to upgrade.\n\n\
         Fresh-only command:\n\
         icp canister install jupiter_faucet \\\n\
           --environment ic \\\n\
           --mode reinstall \\\n\
           --args-file canisters/faucet/mainnet-install-args.did \\\n\
           --yes"
    );
    Ok(())
}

fn create_canister(canister: &str) -> Result<()> {
    build_canister_wasm(canister)?;
    run_icp_with_identity(&[
        "canister",
        "create",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister,
        "--quiet",
    ])?;
    Ok(())
}

fn wasm_path_for_canister(canister: &str) -> Result<String> {
    let repo = repo_root();
    let relative = match canister {
        "mock_icrc_ledger" => "target/wasm32-unknown-unknown/release/mock_icrc_ledger.wasm",
        "mock_nns_governance" => "target/wasm32-unknown-unknown/release/mock_nns_governance.wasm",
        "mock_icp_index" => "target/wasm32-unknown-unknown/release/mock_icp_index.wasm",
        "mock_cmc" => "target/wasm32-unknown-unknown/release/mock_cmc.wasm",
        "mock_xrc" => "target/wasm32-unknown-unknown/release/mock_xrc.wasm",
        "mock_blackhole" => "target/wasm32-unknown-unknown/release/mock_blackhole.wasm",
        "mock_sns_wasm" => "target/wasm32-unknown-unknown/release/mock_sns_wasm.wasm",
        "mock_sns_root" => "target/wasm32-unknown-unknown/release/mock_sns_root.wasm",
        "jupiter_disburser_dbg" | "jupiter_disburser_args_dbg" => {
            "target/wasm32-unknown-unknown/release/jupiter_disburser.wasm"
        }
        "jupiter_faucet_dbg" | "jupiter_faucet_args_dbg" => {
            "target/wasm32-unknown-unknown/release/jupiter_faucet.wasm"
        }
        "jupiter_historian_dbg" | "jupiter_historian_args_dbg" => {
            "target/wasm32-unknown-unknown/release/jupiter_historian.wasm"
        }
        "jupiter_relay_dbg" | "jupiter_relay_args_dbg" => {
            "target/wasm32-unknown-unknown/release/jupiter_relay.wasm"
        }
        _ => bail!("no explicit wasm path configured for {canister}"),
    };
    let wasm = std::path::Path::new(&repo).join(relative);
    let wasm = wasm
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", wasm.display()))?;
    let wasm = wasm.to_str().context("wasm path is not valid UTF-8")?;
    Ok(wasm.to_string())
}

fn install_with_argument_file(canister: &str, relative_path: &str) -> Result<()> {
    let repo = repo_root();
    let arg_file = std::path::Path::new(&repo).join(relative_path);
    let arg_file = arg_file
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", arg_file.display()))?;
    let arg_file = arg_file
        .to_str()
        .context("argument file path is not valid UTF-8")?;
    let wasm = wasm_path_for_canister(canister)?;
    run_icp_with_identity(&[
        "canister",
        "install",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister,
        "--wasm",
        &wasm,
        "--args-format",
        "candid",
        "--args-file",
        arg_file,
        "--yes",
    ])?;
    Ok(())
}

fn get_canister_controllers(canister: &str) -> Result<BTreeSet<String>> {
    // Example output typically contains a line like:
    //   Controllers: <principal1> <principal2>
    // We parse that line and return a deterministic set of principal text values.
    let out = run_icp_with_identity(&[
        "canister",
        "status",
        "--environment",
        LOCAL_ENVIRONMENT,
        canister,
    ])?;

    for line in out.lines() {
        let l = line.trim();
        if l.to_ascii_lowercase().starts_with("controllers:") {
            let rest = l.splitn(2, ':').nth(1).unwrap_or("").trim();

            let mut set = BTreeSet::new();
            for raw in rest.split_whitespace() {
                // strip common punctuation that sometimes appears in output
                let tok = raw.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-'));
                if tok.is_empty() {
                    continue;
                }
                if let Ok(p) = Principal::from_text(tok) {
                    set.insert(p.to_text());
                }
            }

            if set.is_empty() {
                bail!("parsed Controllers line but found no principals: '{l}'");
            }
            return Ok(set);
        }
    }

    bail!("could not find Controllers line in `icp canister status {canister}` output");
}

fn assert_controllers_eq(
    canister: &str,
    actual: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> Result<()> {
    if actual == expected {
        return Ok(());
    }
    bail!(
        "controllers mismatch for {canister}\n  expected: {:?}\n  actual:   {:?}",
        expected,
        actual
    );
}

fn run_local_disburser_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    // Shared time base for scenarios that need it.
    let now_secs = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()) as u64;

    // This is reused across multiple scenarios.
    let four_years = 4u64 * 365 * 86_400;

    // Resolve disburser principal once (it exists by now).
    let disb_principal = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
    let staging_arg = format!(
        "(record {{ owner = principal \"{}\"; subaccount = null }}, 500:nat64)",
        disb_principal.to_text()
    );

    // Always start from a known governance age.
    run_scenario(
        outcomes,
        label("icp", "disburser", "Setup: reset mocks + set aging_since"),
        || {
            let _: () = call_raw_noargs("mock_icrc_ledger", "debug_reset")?;
            let _: () = call_raw_noargs("mock_nns_governance", "debug_reset")?;
            let _: () = call_raw(
                "mock_nns_governance",
                "debug_set_aging_since",
                &format!("({}:nat64)", now_secs.saturating_sub(100)),
            )?;
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "In-flight skip skips payout and disbursement",
        ),
        || {
            let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(true)")?;
            let _: () = call_raw("mock_icrc_ledger", "debug_credit", &staging_arg)?;
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers.is_empty() {
                bail!("expected 0 transfers, got {}", transfers.len());
            }

            let calls: u64 = call_raw_noargs("mock_nns_governance", "debug_get_manage_calls")?;
            if calls != 2 {
                bail!("expected 2 manage_neuron calls (best-effort ClaimOrRefresh + RefreshVotingPower), got {}", calls);
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Happy path: bonus split (3 transfers, 399/94/4 net)",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(false)")?;

            let _: () = call_raw("mock_icrc_ledger", "debug_set_fee", "(1:nat64)")?;
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_prev_age_seconds",
                &format!("({}:nat64)", four_years),
            )?;
            let _: () = call_raw("mock_icrc_ledger", "debug_credit", &staging_arg)?;

            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers.len() != 3 {
                bail!("expected 3 transfers, got {}", transfers.len());
            }

            let mut amts: Vec<u64> = transfers
                .iter()
                .map(|t| t.amount.0.to_u64().unwrap_or(0))
                .collect();
            amts.sort_unstable();

            if amts != vec![4, 94, 399] {
                bail!("unexpected transfer amounts: {:?}", amts);
            }

            let calls: u64 = call_raw_noargs("mock_nns_governance", "debug_get_manage_calls")?;
            if calls != 5 {
                bail!("expected 5 manage_neuron calls, got {}", calls);
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Retry: TemporarilyUnavailable preserves plan and later succeeds",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(false)")?;
            let _: () = call_raw("mock_icrc_ledger", "debug_set_fee", "(1:nat64)")?;
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_prev_age_seconds",
                &format!("({}:nat64)", four_years),
            )?;
            let _: () = call_raw("mock_icrc_ledger", "debug_credit", &staging_arg)?;

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_set_next_error",
                "(opt variant { TemporarilyUnavailable })",
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers.is_empty() {
                bail!(
                    "expected 0 transfers on first attempt, got {}",
                    transfers.len()
                );
            }

            // retry
            let _: () = call_raw("mock_icrc_ledger", "debug_set_next_error", "(null)")?;
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers.len() != 3 {
                bail!("expected 3 transfers after retry, got {}", transfers.len());
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "BadFee: clears plan then rebuilds with new fee",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(false)")?;
            let _: () = call_raw("mock_icrc_ledger", "debug_set_fee", "(1:nat64)")?;
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_prev_age_seconds",
                &format!("({}:nat64)", four_years),
            )?;
            let _: () = call_raw("mock_icrc_ledger", "debug_credit", &staging_arg)?;

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_set_next_error",
                "(opt variant { BadFee = record { expected_fee_e8s = 2:nat64 } })",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            let st: DebugState = call_raw_noargs("jupiter_disburser_dbg", "debug_state")?;
            if st.payout_plan_present {
                bail!("expected payout_plan to be cleared after BadFee");
            }

            // now set fee=2 and succeed
            let _: () = call_raw("mock_icrc_ledger", "debug_set_fee", "(2:nat64)")?;
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers.len() != 3 {
                bail!(
                    "expected 3 transfers after rebuild, got {}",
                    transfers.len()
                );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "checked-in mainnet install args round-trip into config",
        ),
        || {
            create_canister("jupiter_disburser_args_dbg")?;
            install_with_argument_file(
                "jupiter_disburser_args_dbg",
                "canisters/disburser/mainnet-install-args.did",
            )?;
            let cfg: DisburserDebugConfig =
                call_raw_noargs("jupiter_disburser_args_dbg", "debug_config")?;
            let expected_normal = Account {
                owner: prod_faucet_principal(),
                subaccount: None,
            };
            let expected_bonus_1 = Account {
                owner: prod_sns_rewards_principal(),
                subaccount: None,
            };
            let expected_bonus_2 = Account {
                owner: mainnet_governance_principal(),
                subaccount: Some(expected_dquorum_subaccount()),
            };
            let ok = cfg.neuron_id == 11_614_578_985_374_291_210
                && cfg.normal_recipient == expected_normal
                && cfg.age_bonus_recipient_1 == expected_bonus_1
                && cfg.age_bonus_recipient_2 == expected_bonus_2
                && cfg.ledger_canister_id == mainnet_ledger_principal()
                && cfg.governance_canister_id == mainnet_governance_principal()
                && cfg.rescue_controller == prod_lifeline_principal()
                && cfg.blackhole_controller == Some(mainnet_blackhole_principal())
                && cfg.blackhole_armed == Some(false)
                && cfg.main_interval_seconds == 86_400
                && cfg.rescue_interval_seconds == 86_400;
            if !ok {
                let failure = format!("unexpected disburser debug_config: {:?}", cfg);
                bail!("{failure}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Rescue controllers invariants (broken→blackhole+rescue+self, healthy→blackhole+self)",
        ),
        || {
            // Determine expected principals from reality (not mocks).
            let self_id = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
            let rescue = principal_of_identity()?; // same identity used to deploy/configure

            let self_txt = self_id.to_text();
            let rescue_txt = rescue.to_text();
            let blackhole_txt = canister_id("mock_blackhole")?.trim().to_string();

            // 1) Force "broken" state.
            let old = now_secs.saturating_sub(30 * 86_400);
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_last_successful_transfer_ts",
                &format!("(opt ({}:nat64))", old),
            )?;

            // Run rescue tick: should set controllers to {blackhole, rescue, self}
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let actual = get_canister_controllers("jupiter_disburser_dbg")?;
            let expected_broken: BTreeSet<String> =
                [blackhole_txt.clone(), rescue_txt.clone(), self_txt.clone()]
                    .into_iter()
                    .collect();
            assert_controllers_eq("jupiter_disburser_dbg", &actual, &expected_broken)?;

            // 2) Recovery: mark as healthy, then rescue tick should reconcile to {blackhole, self}.
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_last_successful_transfer_ts",
                &opt_nat64_to_candid(now_secs),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let actual2 = get_canister_controllers("jupiter_disburser_dbg")?;
            let expected_healthy: BTreeSet<String> =
                [blackhole_txt, self_txt].into_iter().collect();
            assert_controllers_eq("jupiter_disburser_dbg", &actual2, &expected_healthy)?;

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Rescue healthy no-op (controllers remain blackhole+self)",
        ),
        || {
            let self_id = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
            let self_txt = self_id.to_text();
            let blackhole_txt = canister_id("mock_blackhole")?.trim().to_string();

            // Ensure we are in healthy window and actively reconcile once into blackhole+self.
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_last_successful_transfer_ts",
                &opt_nat64_to_candid(now_secs),
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let expected: BTreeSet<String> =
                [blackhole_txt, self_txt.clone()].into_iter().collect();
            let before = get_canister_controllers("jupiter_disburser_dbg")?;
            assert_controllers_eq("jupiter_disburser_dbg", &before, &expected)?;

            // Run rescue tick again; should remain unchanged.
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;
            let after = get_canister_controllers("jupiter_disburser_dbg")?;
            assert_controllers_eq("jupiter_disburser_dbg", &after, &expected)?;

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Rescue is not armed before first successful payout",
        ),
        || {
            let before = get_canister_controllers("jupiter_disburser_dbg")?;

            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_last_successful_transfer_ts",
                "(null)",
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let after = get_canister_controllers("jupiter_disburser_dbg")?;
            if before != after {
                bail!(
                "expected controllers to remain unchanged before first successful payout, before={before:?} after={after:?}"
            );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Plan persistence: present after failure, cleared after retry success",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(false)")?;
            let _: () = call_raw("mock_icrc_ledger", "debug_set_fee", "(1:nat64)")?;
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_prev_age_seconds",
                &format!("({}:nat64)", four_years),
            )?;
            let _: () = call_raw("mock_icrc_ledger", "debug_credit", &staging_arg)?;

            // Inject transient error
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_set_next_error",
                "(opt variant { TemporarilyUnavailable })",
            )?;

            // First tick: should fail payout, plan should exist
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;
            let st1: DebugState = call_raw_noargs("jupiter_disburser_dbg", "debug_state")?;
            if !st1.payout_plan_present {
                bail!("expected payout_plan_present=true after TemporarilyUnavailable");
            }

            // Retry: clear error
            let _: () = call_raw("mock_icrc_ledger", "debug_set_next_error", "(null)")?;
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            // After success: plan should be cleared and transfers present
            let st2: DebugState = call_raw_noargs("jupiter_disburser_dbg", "debug_state")?;
            if st2.payout_plan_present {
                bail!("expected payout_plan_present=false after successful retry");
            }

            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers.len() != 3 {
                bail!("expected 3 transfers after retry, got {}", transfers.len());
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "disburser",
            "Dust stays in staging when below fee (no transfers, no plan)",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_reset", "()")?;
            let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(false)")?;

            // Fee is larger than the entire staging balance we will credit.
            let _: () = call_raw("mock_icrc_ledger", "debug_set_fee", "(10000:nat64)")?;

            // Credit a tiny amount (e.g., 9000 e8s) to staging.
            let tiny_credit = format!(
                "(record {{ owner = principal \"{}\"; subaccount = null }}, 9000:nat64)",
                Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?.to_text()
            );
            let _: () = call_raw("mock_icrc_ledger", "debug_credit", &tiny_credit)?;

            // Run tick: should plan zero transfers and not wedge
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers.is_empty() {
                bail!("expected 0 transfers, got {}", transfers.len());
            }

            let st: DebugState = call_raw_noargs("jupiter_disburser_dbg", "debug_state")?;
            if st.payout_plan_present {
                bail!("expected payout plan to be cleared/absent when all shares <= fee");
            }

            // Balance should still be there (unchanged) in staging.
            let bal: Nat = call_raw(
                "mock_icrc_ledger",
                "icrc1_balance_of",
                &format!(
                    "(record {{ owner = principal \"{}\"; subaccount = null }})",
                    Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?.to_text()
                ),
            )?;
            let bal_u64 = bal.0.to_u64().unwrap_or(0);
            if bal_u64 != 9000 {
                bail!("expected staging balance to remain 9000, got {}", bal_u64);
            }

            Ok(())
        },
    );

    Ok(())
}

fn run_local_faucet_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "same beneficiary commitments stay separate (no aggregation)",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 300000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 90000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_repeated_transfer",
                &format!("(\"{}\", 3:nat64, 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 90_000_000)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;

            let summary: Option<FaucetSummary> =
                call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary")?;
            if summary.topped_up_count != 3 {
                bail!(
                    "expected three independent top-ups for the same beneficiary, got {}",
                    summary.topped_up_count
                );
            }

            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            let beneficiary_notes = notes.iter().filter(|n| n.canister_id == target).count();
            if beneficiary_notes != 3 {
                bail!("expected three beneficiary notifications for the same canister, got {beneficiary_notes}");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "numeric neuron id memo routes payout to resolved neuron staking account",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let neuron_id = 42_u64;
            let memo_text = format!("{neuron_id}.local.memo");
            let memo = opt_blob_to_candid(Some(memo_text.as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;

            let mut expected_subaccount = [0u8; 32];
            expected_subaccount[24..].copy_from_slice(&neuron_id.to_be_bytes());
            let gov_id = Principal::from_text(canister_id("mock_nns_governance")?.trim())?;
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers.len() != 1 {
                bail!("expected exactly one neuron stake transfer, got {transfers:?}");
            }
            if transfers[0].to
                != (Account {
                    owner: gov_id,
                    subaccount: Some(expected_subaccount),
                })
                || transfers[0].memo.as_deref() != Some(b"local.memo".as_slice())
                || nat_to_u64(&transfers[0].amount) != 99_990_000
            {
                bail!("unexpected neuron stake transfer: {:?}", transfers[0]);
            }
            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if !notes.is_empty() {
                bail!("neuron stake payout should not call CMC notify_top_up, got {notes:?}");
            }
            let summary: Option<FaucetSummary> =
                call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary")?;
            if summary.topped_up_count != 1
                || summary.failed_topups != 0
                || summary.topped_up_sum_e8s != 99_990_000
            {
                bail!("unexpected neuron stake summary: {summary:?}");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "every new payout job rescans full history from the beginning",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 300000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_repeated_transfer",
                &format!("(\"{}\", 3:nat64, 100000000:nat64, {})", staking_id, memo),
            )?;

            for pot in [90_000_000u64, 60_000_000u64] {
                let _: () = call_raw(
                    "mock_icrc_ledger",
                    "debug_credit",
                    &format!("({}, {}:nat64)", account_to_candid(&accounts.payout), pot),
                )?;
                let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, pot)?;
                let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
                let summary: Option<FaucetSummary> =
                    call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
                let summary = summary.context("expected faucet summary after payout job")?;
                if summary.topped_up_count != 3 {
                    bail!(
                        "expected replayed history to produce three top-ups per run, got {}",
                        summary.topped_up_count
                    );
                }
            }

            let calls: Vec<IndexGetCall> = call_raw_noargs("mock_icp_index", "debug_get_calls")?;
            let starts: Vec<Option<u64>> = calls
                .iter()
                .filter(|c| c.account_identifier == staking_id)
                .map(|c| c.start)
                .collect();
            if starts.len() != 3 || starts.iter().any(|start| start.is_some()) {
                bail!(
                "expected first-tranche denominator pre-scan and both payout scans to start from the beginning, got starts {starts:?} from calls {calls:?}"
            );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "page-boundary scan skips bad/small entries and still finds late eligible tx",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let good_memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 1500000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 120000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_repeated_transfer",
                &format!(
                    "(\"{}\", 499:nat64, 1000000:nat64, {})",
                    staking_id, good_memo
                ),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(
                    "(\"{}\", 200000000:nat64, opt vec {{ 98; 97; 100 }})",
                    staking_id
                ),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_repeated_transfer",
                &format!("(\"{}\", 500:nat64, 1000000:nat64, null)", staking_id),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 300000000:nat64, {})", staking_id, good_memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 120_000_000)?;

            let mut summary: Option<FaucetSummary> = None;
            for _ in 0..5 {
                let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
                let state: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
                summary = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
                if state.last_summary_present {
                    break;
                }
            }
            let state: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            let summary = summary
                .context("expected faucet summary after advancing across page boundaries")?;
            if summary.topped_up_count != 1
                || summary.ignored_bad_memo != 1
                || summary.ignored_under_threshold != 999
            {
                bail!(
                "unexpected page-boundary summary: topped_up_count={} ignored_bad_memo={} ignored_under_threshold={} state={state:?}",
                summary.topped_up_count,
                summary.ignored_bad_memo,
                summary.ignored_under_threshold
            );
            }

            let calls: Vec<IndexGetCall> = call_raw_noargs("mock_icp_index", "debug_get_calls")?;
            if calls.len() < 3 {
                bail!(
                    "expected multi-page history scan, got {} index calls: {calls:?}",
                    calls.len()
                );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "notify retry completes inline without duplicate transfer",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;

            let _: () = call_raw(
                "mock_cmc",
                "debug_set_script",
                "(vec { variant { Processing }; variant { Ok } })",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;

            let st: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st.active_payout_job_present || !st.last_summary_present {
                bail!("expected inline notify retry to complete within one tick");
            }

            let transfers_after: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_after.len() != 1 {
                bail!("expected exactly one beneficiary transfer after inline notify retry, got {} total transfers", transfers_after.len());
            }

            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            let beneficiary_notes = notes.iter().filter(|n| n.canister_id == target).count();
            if beneficiary_notes != 1 {
                bail!("expected exactly one successful beneficiary notification after inline retry, got {beneficiary_notes}");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "multiple beneficiaries are processed independently",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let beneficiary_a = Principal::from_text(canister_id("mock_cmc")?.trim())?;
            let beneficiary_b = short_test_principal();
            let memo_a = opt_blob_to_candid(Some(beneficiary_a.to_text().as_bytes()));
            let memo_b = opt_blob_to_candid(Some(beneficiary_b.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 200000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo_a),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo_b),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let summary: Option<FaucetSummary> =
                call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary")?;
            if summary.topped_up_count != 2 {
                bail!(
                    "expected two independent beneficiary top-ups, got {}",
                    summary.topped_up_count
                );
            }

            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            let count_a = notes
                .iter()
                .filter(|n| n.canister_id == beneficiary_a)
                .count();
            let count_b = notes
                .iter()
                .filter(|n| n.canister_id == beneficiary_b)
                .count();
            if count_a != 1 || count_b != 1 {
                bail!(
                    "expected one notification per beneficiary, got count_a={} count_b={}",
                    count_a,
                    count_b
                );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "empty history returns payout remainder to self",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let accounts: FaucetDebugAccounts =
                call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
            let faucet_id = Principal::from_text(canister_id("jupiter_faucet_dbg")?.trim())?;

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let summary: Option<FaucetSummary> =
                call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary after empty-history payout")?;
            if summary.topped_up_count != 0 || summary.remainder_to_self_e8s != 99_990_000 {
                bail!(
                "expected empty history to send whole payout remainder to self, got topped_up_count={} remainder_to_self_e8s={}",
                summary.topped_up_count,
                summary.remainder_to_self_e8s
            );
            }

            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes.len() != 1 || notes[0].canister_id != faucet_id {
                bail!("expected exactly one remainder notification to faucet self, got {notes:?}");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label("icp", "faucet", "zero payout pot produces no work"),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st.active_payout_job_present || st.last_summary_present {
                bail!("expected zero payout pot to avoid creating any payout job or summary");
            }
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if !transfers.is_empty() || !notes.is_empty() {
                bail!("expected zero payout pot to produce no transfers or notifications");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "payout pot at or below fee produces no work",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 50000000:nat64)", account_to_candid(&accounts.staking)),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 10000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 50000000:nat64, {})", staking_id, memo),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st.active_payout_job_present || st.last_summary_present {
                bail!("expected payout pot <= fee to avoid creating any payout job or summary");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "first strict tranche ignores live staking balance when computing share",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 1000000000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(
                    "(\"{}\", 100000000:nat64, {})",
                    staking_id,
                    opt_blob_to_candid(Some(short_test_principal().to_text().as_bytes()))
                ),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let summary: Option<FaucetSummary> =
                call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary")?;
            if summary.topped_up_count != 1
                || summary.ignored_under_threshold != 0
                || summary.ignored_bad_memo != 0
            {
                bail!(
                "expected strict first-tranche denominator to pay the indexed commitment despite large live staking balance, got topped_up_count={} ignored_under_threshold={} ignored_bad_memo={}",
                summary.topped_up_count,
                summary.ignored_under_threshold,
                summary.ignored_bad_memo
            );
            }
            if summary.remainder_to_self_e8s != 0 {
                bail!(
                    "expected no fallback remainder after full first-tranche allocation, got {}",
                    summary.remainder_to_self_e8s
                );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "temporary pre-transfer ledger failure is retried inline without blocking the job",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_set_next_error",
                "(opt variant { TemporarilyUnavailable })",
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st1.active_payout_job_present || !st1.last_summary_present {
                bail!("expected temporary pre-transfer failure to be retried inline and finish within one tick");
            }
            let transfers_after: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_after.len() != 1 {
                bail!(
                    "expected exactly one beneficiary transfer after inline recovery, got {}",
                    transfers_after.len()
                );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "duplicate ledger result reuses prior block index and still notifies",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_set_next_error",
                "(opt variant { Duplicate = record { duplicate_of = 77:nat64 } })",
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers.is_empty() {
                bail!("expected injected duplicate path not to create a fresh ledger transfer in the mock ledger");
            }
            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes.len() != 1 || notes[0].block_index != 77 {
                bail!("expected duplicate result to drive notify_top_up with duplicate_of block index 77, got {notes:?}");
            }
            let summary: Option<FaucetSummary> =
                call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary =
                summary.context("expected faucet summary after duplicate-handling path")?;
            if summary.topped_up_count != 1 {
                bail!(
                    "expected duplicate ledger path to count as one completed top-up, got {}",
                    summary.topped_up_count
                );
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "CMC Processing response is retried without duplicate transfer",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, 100000000:nat64)",
                    account_to_candid(&accounts.staking)
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;
            let _: () = call_raw(
                "mock_cmc",
                "debug_set_script",
                "(vec { variant { Processing }; variant { Ok } })",
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st1.active_payout_job_present || !st1.last_summary_present {
                bail!("expected Processing response to be retried inline and complete within one tick");
            }
            let transfers_after: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_after.len() != 1 {
                bail!("expected Processing retry path to avoid duplicate beneficiary transfer, got {}", transfers_after.len());
            }
            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes.len() != 1 || notes[0].canister_id != target {
                bail!("expected one eventual beneficiary notification after Processing retry, got {notes:?}");
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "terminal CMC responses are retried safely without duplicate transfer",
        ),
        || {
            for (label, script) in [
            (
                "Refunded",
                "(vec { variant { Refunded = record { reason = \"refunded\"; block_index = opt (7:nat64) } }; variant { Ok } })",
            ),
            (
                "TransactionTooOld",
                "(vec { variant { TransactionTooOld = 99:nat64 }; variant { Ok } })",
            ),
            (
                "InvalidTransaction",
                "(vec { variant { InvalidTransaction = \"bad block\" }; variant { Ok } })",
            ),
        ] {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
            let staking_id = account_identifier_text(accounts.staking.owner, accounts.staking.subaccount);
            let target = short_test_principal();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.staking)),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
            )?;
            let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;
            let _: () = call_raw("mock_cmc", "debug_set_script", script)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st1.active_payout_job_present || !st1.last_summary_present {
                bail!("expected {label} response to be retried safely inline and finish within one tick");
            }
            let transfers_after: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_after.len() != 1 {
                bail!("expected {label} path to avoid duplicate beneficiary transfer, got {}", transfers_after.len());
            }
            let notes_after: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes_after.len() != 1 || notes_after[0].canister_id != target {
                bail!("expected {label} retry path to end with one completed beneficiary notification after the safe inline retry, got {notes_after:?}");
            }
        }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "deterministic pre-transfer ledger errors are skipped without blocking the job",
        ),
        || {
            for (label, err_arg) in [
                ("TooOld", "(opt variant { TooOld })"),
                (
                    "CreatedInFuture",
                    "(opt variant { CreatedInFuture = record { ledger_time = 123:nat64 } })",
                ),
                (
                    "BadFee",
                    "(opt variant { BadFee = record { expected_fee_e8s = 20000:nat64 } })",
                ),
            ] {
                let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
                let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
                let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
                let _: () =
                    call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

                let accounts: FaucetDebugAccounts =
                    call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
                let staking_id =
                    account_identifier_text(accounts.staking.owner, accounts.staking.subaccount);
                let memo = opt_blob_to_candid(Some(short_test_principal().to_text().as_bytes()));

                let _: () = call_raw(
                    "mock_icrc_ledger",
                    "debug_credit",
                    &format!(
                        "({}, 100000000:nat64)",
                        account_to_candid(&accounts.staking)
                    ),
                )?;
                let _: () = call_raw(
                    "mock_icrc_ledger",
                    "debug_credit",
                    &format!("({}, 100000000:nat64)", account_to_candid(&accounts.payout)),
                )?;
                let _: u64 = call_raw(
                    "mock_icp_index",
                    "debug_append_transfer",
                    &format!("(\"{}\", 100000000:nat64, {})", staking_id, memo),
                )?;
                let _: u64 = append_local_faucet_funding_tranche(&accounts.payout, 100_000_000)?;
                let _: () = call_raw("mock_icrc_ledger", "debug_set_next_error", err_arg)?;

                let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
                let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
                if st1.active_payout_job_present {
                    bail!("expected {label} ledger rejection to be skipped immediately without leaving active job behind");
                }
                let summary: Option<FaucetSummary> =
                    call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
                let summary = summary
                    .context("expected faucet summary after deterministic ledger rejection")?;
                if summary.failed_topups != 1 || summary.topped_up_count != 0 {
                    bail!("expected {label} path to count exactly one failed top-up and zero successful beneficiary top-ups, got failed_topups={} topped_up_count={}", summary.failed_topups, summary.topped_up_count);
                }
                if summary.remainder_to_self_e8s != 99_990_000 {
                    bail!("expected {label} path to leave the failed beneficiary share in the faucet and send the full remainder to self, got {}", summary.remainder_to_self_e8s);
                }
                let transfers_after: Vec<TransferRecord> =
                    call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
                if transfers_after.len() != 1 {
                    bail!("expected {label} path to produce only the fallback remainder transfer, got {} transfers", transfers_after.len());
                }
                let notes_after: Vec<NotifyRecord> =
                    call_raw_noargs("mock_cmc", "debug_notifications")?;
                if notes_after.len() != 1 || notes_after[0].canister_id != accounts.payout.owner {
                    bail!("expected {label} path to finish with exactly one self notification for the fallback remainder, got {notes_after:?}");
                }
            }

            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "checked-in mainnet install args round-trip into config",
        ),
        || {
            create_canister("jupiter_faucet_args_dbg")?;
            install_with_argument_file(
                "jupiter_faucet_args_dbg",
                "canisters/faucet/mainnet-install-args.did",
            )?;
            let cfg: FaucetDebugConfig =
                call_raw_noargs("jupiter_faucet_args_dbg", "debug_config")?;
            let ok = cfg.staking_account == expected_mainnet_staking_account()
                && cfg.payout_subaccount.is_none()
                && cfg.ledger_canister_id == mainnet_ledger_principal()
                && cfg.index_canister_id == mainnet_index_principal()
                && cfg.cmc_canister_id == mainnet_cmc_principal()
                && cfg.governance_canister_id == mainnet_governance_principal()
                && cfg.funding_source_account
                    == Account {
                        owner: Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai")?,
                        subaccount: None,
                    }
                && cfg.rescue_controller == prod_lifeline_principal()
                && cfg.blackhole_controller == Some(mainnet_blackhole_principal())
                && cfg.blackhole_armed.is_none()
                && cfg.expected_first_staking_tx_id == Some(31_118_741)
                && cfg.main_interval_seconds == 86_400
                && cfg.rescue_interval_seconds == 86_400
                && cfg.min_tx_e8s == 100_000_000;
            if !ok {
                let failure = format!("unexpected faucet debug_config: {:?}", cfg);
                bail!("{failure}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "faucet",
            "rescue: before first successful top-up it stays on current controllers",
        ),
        || {
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;
            let _: () = call_raw(
                "jupiter_faucet_dbg",
                "debug_set_blackhole_armed",
                "(opt true)",
            )?;
            let _: () = call_raw(
                "jupiter_faucet_dbg",
                "debug_set_last_successful_transfer_ts",
                "(null)",
            )?;

            let before = get_canister_controllers("jupiter_faucet_dbg")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_rescue_tick")?;
            let after = get_canister_controllers("jupiter_faucet_dbg")?;
            if before != after {
                bail!("expected rescue to remain inactive before any successful top-up, before={before:?} after={after:?}");
            }

            Ok(())
        },
    );

    run_scenario(outcomes, label("icp", "faucet", "rescue: broken path adds lifeline alongside blackhole+self and healthy path recovers to blackhole+self"), || {
        let faucet_id = Principal::from_text(canister_id("jupiter_faucet_dbg")?.trim())?;
        let rescue = Principal::from_text(canister_id("mock_cmc")?.trim())?;
        let blackhole = canister_id("mock_blackhole")?.trim().to_string();
        let expected_broken: BTreeSet<String> = [blackhole.clone(), faucet_id.to_text(), rescue.to_text()].into_iter().collect();
        let expected_healthy: BTreeSet<String> = [blackhole, faucet_id.to_text()].into_iter().collect();

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;
        let _: () = call_raw("jupiter_faucet_dbg", "debug_set_blackhole_armed", "(opt true)")?;
        let _: () = call_raw(
            "jupiter_faucet_dbg",
            "debug_set_last_successful_transfer_ts",
            &opt_nat64_to_candid(0),
        )?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_rescue_tick")?;
        let broken = get_canister_controllers("jupiter_faucet_dbg")?;
        assert_controllers_eq("jupiter_faucet_dbg", &broken, &expected_broken)?;

        let now_secs = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock before unix epoch")?
            .as_secs())
            .saturating_add(1);
        let _: () = call_raw(
            "jupiter_faucet_dbg",
            "debug_set_last_successful_transfer_ts",
            &opt_nat64_to_candid(now_secs),
        )?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_rescue_tick")?;
        let healthy = get_canister_controllers("jupiter_faucet_dbg")?;
        assert_controllers_eq("jupiter_faucet_dbg", &healthy, &expected_healthy)?;

        Ok(())
    });

    Ok(())
}

fn run_frontend_dashboard_local_fixture(expected: &FrontendDashboardExpected) -> Result<()> {
    ensure_frontend_node_modules()?;
    let root = repo_root();
    let historian_id = canister_id("jupiter_historian_dbg")?;
    let output = Command::new("node")
        .args([
            "--test",
            "canisters/frontend/web/test/dashboard-data.local-replica.test.mjs",
        ])
        .env("FRONTEND_DASHBOARD_TEST_HOST", local_replica_host())
        .env(
            "FRONTEND_DASHBOARD_TEST_HISTORIAN_CANISTER_ID",
            historian_id.trim(),
        )
        .env(
            "FRONTEND_DASHBOARD_EXPECTED_JSON",
            serde_json::to_string(expected).context("serialize frontend dashboard expectations")?,
        )
        .current_dir(&root)
        .output()
        .context("failed to run local frontend dashboard fixture test")?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            eprintln!("{}", stdout.trim_end());
        }
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim_end());
        }
        bail!("local frontend dashboard fixture test failed");
    }
    Ok(())
}

fn reset_historian_local_replica_state() -> Result<()> {
    let _: () = call_raw_noargs("mock_icp_index", "debug_reset")?;
    let _: () = call_raw_noargs("mock_blackhole", "debug_reset")?;
    let _: () = call_raw_noargs("mock_icrc_ledger", "debug_reset")?;
    let _: () = call_raw_noargs("mock_xrc", "debug_reset")?;
    let _: () = call_raw_noargs("jupiter_historian_dbg", "debug_reset_derived_state")?;
    Ok(())
}

fn run_local_frontend_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    run_scenario(
        outcomes,
        label(
            "icp",
            "frontend",
            "dashboard loader matches local replica fixture",
        ),
        || {
            reset_historian_local_replica_state()?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let sub_vec = staking
                .subaccount
                .unwrap()
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            let target = Principal::from_text(canister_id("mock_blackhole")?.trim())?;

            let blackhole_id = canister_id("mock_blackhole")?;
            let ledger_id = canister_id("mock_icrc_ledger")?;

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    r#"(record {{ owner = principal "{}"; subaccount = opt vec {{ {} }} }}, 123000000:nat64)"#,
                    staking.owner.to_text(),
                    sub_vec
                ),
            )?;

            let memo = format!(
                "opt vec {{ {} }}",
                target
                    .to_text()
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 100000000:nat64, {})"#, staking_id, memo),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 5000000:nat64, {})"#, staking_id, memo),
            )?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (1234:nat), vec {{ principal "{}" }})"#,
                    target.to_text(),
                    blackhole_id.trim()
                ),
            )?;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_last_completed_cycles_sweep_ts",
                "(null)",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

            let counts: HistorianPublicCounts =
                call_raw("jupiter_historian_dbg", "get_public_counts", "()")?;
            if counts.registered_canister_count != 1
                || counts.qualifying_commitment_count != 1
                || counts.total_output_e8s != 0
                || counts.total_rewards_e8s != 0
            {
                bail!(
                "unexpected historian public counts fixture: registered={} qualifying={} output={} rewards={}",
                counts.registered_canister_count,
                counts.qualifying_commitment_count,
                counts.total_output_e8s,
                counts.total_rewards_e8s
            );
            }

            let status: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            let registered: ListRegisteredCanisterSummariesResponse = call_raw(
                "jupiter_historian_dbg",
                "list_registered_canister_summaries",
                "(record { page = opt (0:nat32); page_size = opt (10:nat32) })",
            )?;
            let recent: ListRecentCommitmentsResponse = call_raw(
                "jupiter_historian_dbg",
                "list_recent_commitments",
                "(record { limit = opt (10:nat32); qualifying_only = opt false })",
            )?;

            if registered.items.len() != 1 || recent.items.len() != 2 {
                bail!(
                    "unexpected fixture table sizes: registered={} recent={}",
                    registered.items.len(),
                    recent.items.len()
                );
            }

            let expected = FrontendDashboardExpected {
                stakeE8s: "123000000".to_string(),
                counts: FrontendDashboardExpectedCounts {
                    registeredCanisterCount: counts.registered_canister_count.to_string(),
                    qualifyingCommitmentCount: counts.qualifying_commitment_count.to_string(),
                    totalOutputE8s: counts.total_output_e8s.to_string(),
                    totalRewardsE8s: counts.total_rewards_e8s.to_string(),
                },
                status: FrontendDashboardExpectedStatus {
                    ledgerCanisterId: ledger_id.trim().to_string(),
                    indexIntervalSeconds: status.index_interval_seconds.to_string(),
                    cyclesIntervalSeconds: status.cycles_interval_seconds.to_string(),
                    stakingAccountIdentifier: account_identifier_text(
                        status.staking_account.owner,
                        status.staking_account.subaccount,
                    ),
                    lastIndexRunTsPresent: status.last_index_run_ts.is_some(),
                    lastCyclesSweepTsPresent: status.last_completed_cycles_sweep_ts.is_some(),
                },
                registered: FrontendDashboardExpectedRegistered {
                    total: registered.total.to_string(),
                    items: registered
                        .items
                        .iter()
                        .map(|item| FrontendDashboardExpectedRegisteredItem {
                            canisterId: item.canister_id.to_text(),
                            qualifyingCommitmentCount: item.qualifying_commitment_count.to_string(),
                            totalQualifyingCommittedE8s: item
                                .total_qualifying_committed_e8s
                                .to_string(),
                            lastCommitmentTsPresent: item.last_commitment_ts.is_some(),
                            latestCycles: item.latest_cycles.as_ref().map(nat_plain_string),
                            lastCyclesProbeTsPresent: item.last_cycles_probe_ts.is_some(),
                        })
                        .collect(),
                },
                recent: FrontendDashboardExpectedRecent {
                    items: recent
                        .items
                        .iter()
                        .map(|item| FrontendDashboardExpectedRecentItem {
                            canisterId: item
                                .canister_id
                                .as_ref()
                                .map(|principal| principal.to_text())
                                .or_else(|| item.memo_text.clone())
                                .unwrap_or_default(),
                            txId: item.tx_id.to_string(),
                            amountE8s: item.amount_e8s.to_string(),
                            countsTowardFaucet: item.counts_toward_faucet,
                        })
                        .collect(),
                },
                errors: FrontendDashboardExpectedErrors { stake: None },
            };

            run_frontend_dashboard_local_fixture(&expected)?;
            Ok(())
        },
    );

    Ok(())
}

fn run_local_historian_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    run_scenario(
        outcomes,
        label("icp", "historian", "relay setup account is deterministic"),
        || {
            reset_historian_local_replica_state()?;
            let historian = Principal::from_text(canister_id("jupiter_historian_dbg")?.trim())?;
            let view = relay_setup_view(target)?;
            let expected_subaccount = relay_setup_subaccount(target);
            let expected_account = Account {
                owner: historian,
                subaccount: Some(expected_subaccount),
            };
            let expected_account_identifier =
                account_identifier_text(expected_account.owner, expected_account.subaccount);

            if view.target_canister_id != target
                || view.setup_account != expected_account
                || view.setup_account_identifier != expected_account_identifier
                || view.existing_relay.is_some()
            {
                bail!("unexpected relay setup view: {view:?}");
            }
            if view.minimum_e8s == 0 || view.dust_e8s == 0 {
                bail!("relay setup view should expose positive economics: {view:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label("icp", "historian", "notify below minimum does not spend"),
        || {
            reset_historian_local_replica_state()?;
            let view = relay_setup_view(target)?;
            let amount = view.minimum_e8s.saturating_sub(1);
            let source = Account {
                owner: short_test_principal(),
                subaccount: Some([11; 32]),
            };
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, {}:nat64)",
                    account_to_candid(&view.setup_account),
                    amount
                ),
            )?;
            append_setup_payment(source, &view.setup_account_identifier, amount)?;

            let result: RelaySetupNotifyResult = call_raw(
                "jupiter_historian_dbg",
                "notify_relay_setup",
                &format!(r#"(principal "{}")"#, target.to_text()),
            )?;
            match result {
                RelaySetupNotifyResult::BelowMinimum {
                    minimum_e8s,
                    current_balance_e8s,
                } if minimum_e8s == view.minimum_e8s && current_balance_e8s == amount => {}
                other => bail!("expected BelowMinimum notify result, got {other:?}"),
            }

            let relay: Option<RelayRegistryEntry> = call_raw(
                "jupiter_historian_dbg",
                "get_relay_for_canister",
                &format!(r#"(principal "{}")"#, target.to_text()),
            )?;
            if relay.is_some() {
                bail!("below-minimum notify should not register a relay: {relay:?}");
            }
            let balance: Nat = call_raw(
                "mock_icrc_ledger",
                "icrc1_balance_of",
                &format!("({})", account_to_candid(&view.setup_account)),
            )?;
            if nat_to_u64(&balance) != amount {
                bail!("below-minimum notify spent funds: balance={balance}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "target not observable exposes refund to source",
        ),
        || {
            reset_historian_local_replica_state()?;
            let view = relay_setup_view(target)?;
            let amount = view.minimum_e8s;
            let source = Account {
                owner: short_test_principal(),
                subaccount: Some([12; 32]),
            };
            let source_id = account_identifier_text(source.owner, source.subaccount);
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, {}:nat64)",
                    account_to_candid(&view.setup_account),
                    amount
                ),
            )?;
            append_setup_payment(source, &view.setup_account_identifier, amount)?;

            let result: RelaySetupNotifyResult = call_raw(
                "jupiter_historian_dbg",
                "notify_relay_setup",
                &format!(r#"(principal "{}")"#, target.to_text()),
            )?;
            if !matches!(result, RelaySetupNotifyResult::TargetNotObservable { .. }) {
                bail!("expected TargetNotObservable, got {result:?}");
            }

            let refund: RelaySetupRefundResult = call_raw(
                "jupiter_historian_dbg",
                "request_relay_setup_refund",
                &format!(r#"(principal "{}")"#, target.to_text()),
            )?;
            if !matches!(refund, RelaySetupRefundResult::Refunded { .. }) {
                bail!("expected Refunded result, got {refund:?}");
            }
            let legacy: Vec<LegacyTransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_legacy_transfers")?;
            if legacy.len() != 1
                || legacy[0].to_account_identifier_hex != source_id
                || legacy[0].from != view.setup_account
                || legacy[0].amount.e8s == 0
            {
                bail!("unexpected legacy refund transfers: {legacy:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "existing relay setup balance sweeps to relay subaccount one",
        ),
        || {
            reset_historian_local_replica_state()?;
            let view = relay_setup_view(target)?;
            let relay = Principal::from_text(canister_id("mock_blackhole")?.trim())?;
            let amount = view.minimum_e8s;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_insert_relay_registry_entry",
                &format!(
                    r#"(record {{
                        relay_canister_id = principal "{}";
                        target_canister_id = principal "{}";
                        kind = variant {{ SelfService }};
                        status = variant {{ Active }};
                        setup_account = null;
                        setup_account_identifier = null;
                        setup_amount_e8s = null;
                        setup_tx_ids = vec {{}};
                        relay_wasm_hash_hex = null;
                        final_controllers = null;
                        log_visibility_public = null;
                        created_at_ts = null;
                        activated_at_ts = null;
                    }})"#,
                    relay.to_text(),
                    target.to_text()
                ),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    "({}, {}:nat64)",
                    account_to_candid(&view.setup_account),
                    amount
                ),
            )?;

            let result: RelaySetupNotifyResult = call_raw(
                "jupiter_historian_dbg",
                "notify_relay_setup",
                &format!(r#"(principal "{}")"#, target.to_text()),
            )?;
            let swept_amount = match result {
                RelaySetupNotifyResult::SweptToExistingRelay { amount_e8s, .. } => amount_e8s,
                other => bail!("expected sweep result for existing relay, got {other:?}"),
            };
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            let transfer = transfers
                .last()
                .context("expected setup sweep ledger transfer")?;
            let mut relay_subaccount_one = [0u8; 32];
            relay_subaccount_one[31] = 1;
            if transfer.from != view.setup_account
                || transfer.to
                    != (Account {
                        owner: relay,
                        subaccount: Some(relay_subaccount_one),
                    })
                || nat_to_u64(&transfer.amount) != swept_amount
            {
                bail!("unexpected sweep transfer: {transfer:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "caches ICP/XDR rate and protects XRC cycles",
        ),
        || {
            reset_historian_local_replica_state()?;
            let _: () = call_raw(
                "mock_xrc",
                "debug_set_rate",
                "(123456:nat64, 4:nat32, 1700000001:nat64)",
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let status1: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            let snapshot1 = status1
                .icp_xdr_rate
                .as_ref()
                .context("expected ICP/XDR rate after first tick")?;
            if snapshot1.rate != 123456
                || snapshot1.decimals != 4
                || snapshot1.timestamp != 1700000001
                || status1.last_icp_xdr_rate_error.is_some()
            {
                bail!("unexpected initial ICP/XDR status: {:?}", status1);
            }
            let calls1: Vec<MockXrcCall> = call_raw_noargs("mock_xrc", "debug_get_calls")?;
            if calls1.len() != 1 {
                bail!(
                    "expected exactly one XRC call after first tick, got {}",
                    calls1.len()
                );
            }
            let attached = calls1[0].attached_cycles.0.to_u128().unwrap_or(0);
            let accepted = calls1[0].accepted_cycles.0.to_u128().unwrap_or(0);
            if calls1[0].base_symbol != "ICP"
                || calls1[0].quote_symbol != "XDR"
                || calls1[0].requested_timestamp.is_some()
                || attached < 1_000_000_000
                || accepted != 260_000_000
            {
                bail!("unexpected XRC call details: {:?}", calls1[0]);
            }

            let _: () = call_raw(
                "mock_xrc",
                "debug_set_rate",
                "(999999:nat64, 4:nat32, 1700000999:nat64)",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let calls2: Vec<MockXrcCall> = call_raw_noargs("mock_xrc", "debug_get_calls")?;
            let status2: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            if calls2.len() != 1 {
                bail!(
                    "cache bypass: expected still one XRC call inside one-day TTL, got {}",
                    calls2.len()
                );
            }
            if status2.icp_xdr_rate.as_ref().map(|s| s.rate) != Some(123456) {
                bail!(
                    "cached rate should remain unchanged inside TTL: {:?}",
                    status2.icp_xdr_rate
                );
            }

            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_icp_xdr_rate_fetched_at_ts",
                "(opt (1:nat64))",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let calls3: Vec<MockXrcCall> = call_raw_noargs("mock_xrc", "debug_get_calls")?;
            let status3: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            if calls3.len() != 2 || status3.icp_xdr_rate.as_ref().map(|s| s.rate) != Some(999999) {
                bail!("expected stale cache refresh to call XRC once and update rate; calls={} status={:?}", calls3.len(), status3.icp_xdr_rate);
            }

            let _: () = call_raw(
                "mock_xrc",
                "debug_set_error",
                "(opt variant { RateLimited })",
            )?;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_icp_xdr_rate_fetched_at_ts",
                "(opt (1:nat64))",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let calls4: Vec<MockXrcCall> = call_raw_noargs("mock_xrc", "debug_get_calls")?;
            let status4: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            if calls4.len() != 3 {
                bail!(
                    "expected one failed stale refresh call, got {}",
                    calls4.len()
                );
            }
            if status4.icp_xdr_rate.as_ref().map(|s| s.rate) != Some(999999)
                || status4.last_icp_xdr_rate_error.is_none()
            {
                bail!(
                    "failed refresh should preserve last good rate and expose error: {:?}",
                    status4
                );
            }

            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let calls5: Vec<MockXrcCall> = call_raw_noargs("mock_xrc", "debug_get_calls")?;
            if calls5.len() != 3 {
                bail!("failed XRC refresh should be cached for one day to prevent a cycle drain, got {} calls", calls5.len());
            }

            let _: () = call_raw("mock_xrc", "debug_set_error", "(null)")?;
            let _: () = call_raw(
                "mock_xrc",
                "debug_set_rate",
                "(777777:nat64, 4:nat32, 1700007777:nat64)",
            )?;
            let refresh_result: DebugRefreshIcpXdrRateResult =
                call_raw_noargs("jupiter_historian_dbg", "debug_refresh_icp_xdr_rate_cache")?;
            if !matches!(refresh_result, DebugRefreshIcpXdrRateResult::Ok) {
                bail!(
                    "debug ICP/XDR refresh should succeed after clearing mock error: {:?}",
                    refresh_result
                );
            }
            let calls6: Vec<MockXrcCall> = call_raw_noargs("mock_xrc", "debug_get_calls")?;
            let status6: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            if calls6.len() != 4 || status6.icp_xdr_rate.as_ref().map(|s| s.rate) != Some(777777) {
                bail!(
                    "debug ICP/XDR refresh should bypass the normal TTL once; calls={} status={:?}",
                    calls6.len(),
                    status6.icp_xdr_rate
                );
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "indexes memo-derived commitment exactly once",
        ),
        || {
            reset_historian_local_replica_state()?;
            let listed_before: HistorianListCanistersResponse = call_raw(
                "jupiter_historian_dbg",
                "list_canisters",
                "(record { start_after = null; limit = opt (10:nat32); source_filter = null })",
            )?;
            if !listed_before.items.is_empty() {
                bail!("expected empty historian canister list at scenario start");
            }

            let staking = Account {
                owner: short_test_principal(),
                subaccount: Some([9u8; 32]),
            };
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let memo = format!(
                "opt vec {{ {} }}",
                target
                    .to_text()
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let append_args = format!(r#"("{}", 100000000:nat64, {})"#, staking_id, memo);
            let _: u64 = call_raw("mock_icp_index", "debug_append_transfer", &append_args)?;

            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

            let listed: HistorianListCanistersResponse = call_raw(
                "jupiter_historian_dbg",
                "list_canisters",
                "(record { start_after = null; limit = opt (10:nat32); source_filter = null })",
            )?;
            if listed.items.len() != 1 || listed.items[0].canister_id != target {
                bail!(
                    "unexpected historian list response: {:?}",
                    listed
                        .items
                        .iter()
                        .map(|i| i.canister_id.to_text())
                        .collect::<Vec<_>>()
                );
            }

            let history: HistorianCommitmentHistoryPage = call_raw(
                "jupiter_historian_dbg",
                "get_commitment_history",
                &format!(
                    r#"(record {{ canister_id = principal "{}"; start_after_tx_id = null; limit = opt (10:nat32); descending = opt false }})"#,
                    target.to_text()
                ),
            )?;
            if history.items.len() != 1 || history.items[0].tx_id != 1 {
                bail!(
                    "expected one indexed commitment, got {}",
                    history.items.len()
                );
            }

            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let history2: HistorianCommitmentHistoryPage = call_raw(
                "jupiter_historian_dbg",
                "get_commitment_history",
                &format!(
                    r#"(record {{ canister_id = principal "{}"; start_after_tx_id = null; limit = opt (10:nat32); descending = opt false }})"#,
                    target.to_text()
                ),
            )?;
            if history2.items.len() != 1 {
                bail!(
                    "expected historian not to duplicate commitments, got {}",
                    history2.items.len()
                );
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "indexes raw ICP and neuron declarations in recent commitments",
        ),
        || {
            reset_historian_local_replica_state()?;
            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let raw_memo = format!("{}.vault42", target.to_text().replace('-', ""));
            let neuron_id = 42_u64;
            let raw_blob = opt_blob_to_candid(Some(raw_memo.as_bytes()));
            let neuron_memo = format!("{neuron_id}.local.memo");
            let neuron_blob = opt_blob_to_candid(Some(neuron_memo.as_bytes()));

            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 100000000:nat64, {})"#, staking_id, raw_blob),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 100000000:nat64, {})"#, staking_id, neuron_blob),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

            let counts: HistorianPublicCounts =
                call_raw("jupiter_historian_dbg", "get_public_counts", "()")?;
            if counts.registered_canister_count != 0 || counts.qualifying_commitment_count != 2 {
                bail!(
                    "unexpected raw/neuron commitment counts: registered={} qualifying={}",
                    counts.registered_canister_count,
                    counts.qualifying_commitment_count,
                );
            }

            let recent: ListRecentCommitmentsResponse = call_raw(
                "jupiter_historian_dbg",
                "list_recent_commitments",
                "(record { limit = opt (10:nat32); qualifying_only = opt false })",
            )?;
            let raw = recent
                .items
                .iter()
                .find(|item| item.raw_icp_memo_text.as_deref() == Some("vault42"))
                .context("expected raw ICP recent commitment")?;
            if raw.canister_id != Some(target)
                || raw.neuron_id.is_some()
                || !raw.counts_toward_faucet
            {
                bail!("unexpected raw ICP recent commitment: {:?}", raw);
            }
            let neuron = recent
                .items
                .iter()
                .find(|item| item.neuron_id == Some(neuron_id))
                .context("expected neuron recent commitment")?;
            if neuron.canister_id.is_some()
                || neuron.raw_icp_memo_text.is_some()
                || neuron.neuron_memo_text.as_deref() != Some("local.memo")
                || neuron.memo_text.as_deref() != Some("42")
                || !neuron.counts_toward_faucet
            {
                bail!("unexpected neuron recent commitment: {:?}", neuron);
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label("icp", "historian", "weekly sweep records blackhole cycles"),
        || {
            reset_historian_local_replica_state()?;
            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let memo = format!(
                "opt vec {{ {} }}",
                target
                    .to_text()
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 100000000:nat64, {})"#, staking_id, memo),
            )?;
            let blackhole_id = canister_id("mock_blackhole")?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (1234:nat), vec {{ principal "{}" }})"#,
                    target.to_text(),
                    blackhole_id.trim()
                ),
            )?;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_last_completed_cycles_sweep_ts",
                "(null)",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let cycles: HistorianCyclesHistoryPage = call_raw(
                "jupiter_historian_dbg",
                "get_cycles_history",
                &format!(
                    r#"(record {{ canister_id = principal "{}"; start_after_ts = null; limit = opt (10:nat32); descending = opt false }})"#,
                    target.to_text()
                ),
            )?;
            if cycles.items.is_empty() {
                bail!("expected historian cycles history entry");
            }
            if cycles.items[0].cycles != Nat::from(1234u64) {
                bail!("unexpected cycles value: {:?}", cycles.items[0].cycles);
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "SNS discovery adds summary-tracked canisters",
        ),
        || {
            reset_historian_local_replica_state()?;
            let sns_root = Principal::from_text(canister_id("mock_sns_root")?.trim())?;
            let governance = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
            let _: () = call_raw(
                "mock_sns_wasm",
                "debug_set_roots",
                &format!(r#"(vec {{ principal "{}" }})"#, sns_root.to_text()),
            )?;
            let summary_args = format!(
                r#"(record {{ root = opt record {{ canister_id = opt principal "{}"; status = opt record {{ cycles = opt (1000:nat) }} }}; governance = opt record {{ canister_id = opt principal "{}"; status = opt record {{ cycles = opt (2000:nat) }} }}; ledger = null; swap = null; index = null; dapps = vec {{}}; archives = vec {{}} }})"#,
                sns_root.to_text(),
                governance.to_text()
            );
            let _: () = call_raw("mock_sns_root", "debug_set_summary", &summary_args)?;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_last_sns_discovery_ts",
                "(null)",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;
            let listed: HistorianListCanistersResponse = call_raw(
            "jupiter_historian_dbg",
            "list_canisters",
            "(record { start_after = null; limit = opt (20:nat32); source_filter = opt variant { SnsDiscovery } })",
        )?;
            if listed
                .items
                .iter()
                .all(|item| item.canister_id != governance)
            {
                bail!("expected SNS-discovered governance canister in historian results");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "frontend dashboard loader matches local replica fixture",
        ),
        || {
            reset_historian_local_replica_state()?;

            let staking = faucet_staking_account();
            let staking_id = account_identifier_text(staking.owner, staking.subaccount);
            let sub_vec = staking
                .subaccount
                .unwrap()
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            let target = Principal::from_text(canister_id("mock_blackhole")?.trim())?;

            let blackhole_id = canister_id("mock_blackhole")?;
            let ledger_id = canister_id("mock_icrc_ledger")?;

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!(
                    r#"(record {{ owner = principal "{}"; subaccount = opt vec {{ {} }} }}, 123000000:nat64)"#,
                    staking.owner.to_text(),
                    sub_vec
                ),
            )?;

            let memo = format!(
                "opt vec {{ {} }}",
                target
                    .to_text()
                    .as_bytes()
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 100000000:nat64, {})"#, staking_id, memo),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!(r#"("{}", 5000000:nat64, {})"#, staking_id, memo),
            )?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (1234:nat), vec {{ principal "{}" }})"#,
                    target.to_text(),
                    blackhole_id.trim()
                ),
            )?;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_last_completed_cycles_sweep_ts",
                "(null)",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

            let counts: HistorianPublicCounts =
                call_raw("jupiter_historian_dbg", "get_public_counts", "()")?;
            if counts.registered_canister_count != 1
                || counts.qualifying_commitment_count != 1
                || counts.total_output_e8s != 0
                || counts.total_rewards_e8s != 0
            {
                bail!(
                "unexpected historian public counts fixture: registered={} qualifying={} output={} rewards={}",
                counts.registered_canister_count,
                counts.qualifying_commitment_count,
                counts.total_output_e8s,
                counts.total_rewards_e8s
            );
            }

            let status: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            let registered: ListRegisteredCanisterSummariesResponse = call_raw(
                "jupiter_historian_dbg",
                "list_registered_canister_summaries",
                "(record { page = opt (0:nat32); page_size = opt (10:nat32) })",
            )?;
            let recent: ListRecentCommitmentsResponse = call_raw(
                "jupiter_historian_dbg",
                "list_recent_commitments",
                "(record { limit = opt (10:nat32); qualifying_only = opt false })",
            )?;

            if registered.items.len() != 1 || recent.items.len() != 2 {
                bail!(
                    "unexpected fixture table sizes: registered={} recent={}",
                    registered.items.len(),
                    recent.items.len()
                );
            }

            let expected = FrontendDashboardExpected {
                stakeE8s: "123000000".to_string(),
                counts: FrontendDashboardExpectedCounts {
                    registeredCanisterCount: counts.registered_canister_count.to_string(),
                    qualifyingCommitmentCount: counts.qualifying_commitment_count.to_string(),
                    totalOutputE8s: counts.total_output_e8s.to_string(),
                    totalRewardsE8s: counts.total_rewards_e8s.to_string(),
                },
                status: FrontendDashboardExpectedStatus {
                    ledgerCanisterId: ledger_id.trim().to_string(),
                    indexIntervalSeconds: status.index_interval_seconds.to_string(),
                    cyclesIntervalSeconds: status.cycles_interval_seconds.to_string(),
                    stakingAccountIdentifier: account_identifier_text(
                        status.staking_account.owner,
                        status.staking_account.subaccount,
                    ),
                    lastIndexRunTsPresent: status.last_index_run_ts.is_some(),
                    lastCyclesSweepTsPresent: status.last_completed_cycles_sweep_ts.is_some(),
                },
                registered: FrontendDashboardExpectedRegistered {
                    total: registered.total.to_string(),
                    items: registered
                        .items
                        .iter()
                        .map(|item| FrontendDashboardExpectedRegisteredItem {
                            canisterId: item.canister_id.to_text(),
                            qualifyingCommitmentCount: item.qualifying_commitment_count.to_string(),
                            totalQualifyingCommittedE8s: item
                                .total_qualifying_committed_e8s
                                .to_string(),
                            lastCommitmentTsPresent: item.last_commitment_ts.is_some(),
                            latestCycles: item.latest_cycles.as_ref().map(nat_plain_string),
                            lastCyclesProbeTsPresent: item.last_cycles_probe_ts.is_some(),
                        })
                        .collect(),
                },
                recent: FrontendDashboardExpectedRecent {
                    items: recent
                        .items
                        .iter()
                        .map(|item| FrontendDashboardExpectedRecentItem {
                            canisterId: item
                                .canister_id
                                .as_ref()
                                .map(|principal| principal.to_text())
                                .or_else(|| item.memo_text.clone())
                                .unwrap_or_default(),
                            txId: item.tx_id.to_string(),
                            amountE8s: item.amount_e8s.to_string(),
                            countsTowardFaucet: item.counts_toward_faucet,
                        })
                        .collect(),
                },
                errors: FrontendDashboardExpectedErrors { stake: None },
            };

            run_frontend_dashboard_local_fixture(&expected)?;
            Ok(())
        },
    );

    run_scenario(outcomes, label("icp", "historian", "frontend dashboard loader keeps output and rewards at zero for commitment-only fixtures"), || {
        reset_historian_local_replica_state()?;

        let staking = faucet_staking_account();
        let staking_id = account_identifier_text(staking.owner, staking.subaccount);
        let sub_vec = staking
            .subaccount
            .unwrap()
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        let target = Principal::from_text(canister_id("jupiter_faucet_dbg")?.trim())?;

        let ledger_id = canister_id("mock_icrc_ledger")?;

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!(
                r#"(record {{ owner = principal "{}"; subaccount = opt vec {{ {} }} }}, 123000000:nat64)"#,
                staking.owner.to_text(),
                sub_vec
            ),
        )?;

        let memo = format!(
            "opt vec {{ {} }}",
            target
                .to_text()
                .as_bytes()
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!(r#"("{}", 100000000:nat64, {})"#, staking_id, memo),
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

        let counts: HistorianPublicCounts = call_raw("jupiter_historian_dbg", "get_public_counts", "()")?;
        if counts.registered_canister_count != 1
            || counts.qualifying_commitment_count != 1
            || counts.total_output_e8s != 0
            || counts.total_rewards_e8s != 0
        {
            bail!(
                "unexpected commitment-only fixture public counts: registered={} qualifying={} output={} rewards={}",
                counts.registered_canister_count,
                counts.qualifying_commitment_count,
                counts.total_output_e8s,
                counts.total_rewards_e8s
            );
        }

        let status: HistorianPublicStatus = call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
        let registered: ListRegisteredCanisterSummariesResponse = call_raw(
            "jupiter_historian_dbg",
            "list_registered_canister_summaries",
            "(record { page = opt (0:nat32); page_size = opt (10:nat32) })",
        )?;
        let recent: ListRecentCommitmentsResponse = call_raw(
            "jupiter_historian_dbg",
            "list_recent_commitments",
            "(record { limit = opt (10:nat32); qualifying_only = opt false })",
        )?;

        let expected = FrontendDashboardExpected {
            stakeE8s: "123000000".to_string(),
            counts: FrontendDashboardExpectedCounts {
                registeredCanisterCount: counts.registered_canister_count.to_string(),
                qualifyingCommitmentCount: counts.qualifying_commitment_count.to_string(),
                totalOutputE8s: counts.total_output_e8s.to_string(),
                totalRewardsE8s: counts.total_rewards_e8s.to_string(),
            },
            status: FrontendDashboardExpectedStatus {
                ledgerCanisterId: ledger_id.trim().to_string(),
                indexIntervalSeconds: status.index_interval_seconds.to_string(),
                cyclesIntervalSeconds: status.cycles_interval_seconds.to_string(),
                stakingAccountIdentifier: account_identifier_text(status.staking_account.owner, status.staking_account.subaccount),
                lastIndexRunTsPresent: status.last_index_run_ts.is_some(),
                lastCyclesSweepTsPresent: status.last_completed_cycles_sweep_ts.is_some(),
            },
            registered: FrontendDashboardExpectedRegistered {
                total: registered.total.to_string(),
                items: registered
                    .items
                    .iter()
                    .map(|item| FrontendDashboardExpectedRegisteredItem {
                        canisterId: item.canister_id.to_text(),
                        qualifyingCommitmentCount: item.qualifying_commitment_count.to_string(),
                        totalQualifyingCommittedE8s: item.total_qualifying_committed_e8s.to_string(),
                        lastCommitmentTsPresent: item.last_commitment_ts.is_some(),
                        latestCycles: item.latest_cycles.as_ref().map(nat_plain_string),
                        lastCyclesProbeTsPresent: item.last_cycles_probe_ts.is_some(),
                    })
                    .collect(),
            },
            recent: FrontendDashboardExpectedRecent {
                items: recent
                    .items
                    .iter()
                    .map(|item| FrontendDashboardExpectedRecentItem {
                        canisterId: item
                            .canister_id
                            .as_ref()
                            .map(|principal| principal.to_text())
                            .or_else(|| item.memo_text.clone())
                            .unwrap_or_default(),
                        txId: item.tx_id.to_string(),
                        amountE8s: item.amount_e8s.to_string(),
                        countsTowardFaucet: item.counts_toward_faucet,
                    })
                    .collect(),
            },
            errors: FrontendDashboardExpectedErrors { stake: None },
        };

        run_frontend_dashboard_local_fixture(&expected)?;
        Ok(())
    });

    run_scenario(outcomes, label("icp", "historian", "frontend dashboard loader preserves zero output, rewards, and qualifying counts for non-qualifying memo fixture"), || {
        reset_historian_local_replica_state()?;

        let staking = faucet_staking_account();
        let staking_id = account_identifier_text(staking.owner, staking.subaccount);
        let sub_vec = staking
            .subaccount
            .unwrap()
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        let target = Principal::from_text(canister_id("mock_blackhole")?.trim())?;

        let ledger_id = canister_id("mock_icrc_ledger")?;

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!(
                r#"(record {{ owner = principal "{}"; subaccount = opt vec {{ {} }} }}, 5000000:nat64)"#,
                staking.owner.to_text(),
                sub_vec
            ),
        )?;

        let memo = format!(
            "opt vec {{ {} }}",
            target
                .to_text()
                .as_bytes()
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!(r#"("{}", 5000000:nat64, {})"#, staking_id, memo),
        )?;
        let blackhole_id = canister_id("mock_blackhole")?;
        let _: () = call_raw(
            "mock_blackhole",
            "debug_set_status",
            &format!(
                r#"(principal "{}", opt (777:nat), vec {{ principal "{}" }})"#,
                target.to_text(),
                blackhole_id.trim()
            ),
        )?;
        let _: () = call_raw("jupiter_historian_dbg", "debug_set_last_completed_cycles_sweep_ts", "(null)")?;
        let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

        let counts: HistorianPublicCounts = call_raw("jupiter_historian_dbg", "get_public_counts", "()")?;
        if counts.registered_canister_count != 0
            || counts.qualifying_commitment_count != 0
            || counts.total_output_e8s != 0
            || counts.total_rewards_e8s != 0
        {
            bail!(
                "unexpected non-qualifying fixture public counts: registered={} qualifying={} output={} rewards={}",
                counts.registered_canister_count,
                counts.qualifying_commitment_count,
                counts.total_output_e8s,
                counts.total_rewards_e8s
            );
        }

        let status: HistorianPublicStatus = call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
        let registered: ListRegisteredCanisterSummariesResponse = call_raw(
            "jupiter_historian_dbg",
            "list_registered_canister_summaries",
            "(record { page = opt (0:nat32); page_size = opt (10:nat32) })",
        )?;
        let recent: ListRecentCommitmentsResponse = call_raw(
            "jupiter_historian_dbg",
            "list_recent_commitments",
            "(record { limit = opt (10:nat32); qualifying_only = opt false })",
        )?;

        if registered.items.len() != 0 || recent.items.len() != 1 {
            bail!(
                "unexpected non-qualifying fixture table sizes: registered={} recent={}",
                registered.items.len(),
                recent.items.len()
            );
        }

        let expected = FrontendDashboardExpected {
            stakeE8s: "5000000".to_string(),
            counts: FrontendDashboardExpectedCounts {
                registeredCanisterCount: counts.registered_canister_count.to_string(),
                qualifyingCommitmentCount: counts.qualifying_commitment_count.to_string(),
                totalOutputE8s: counts.total_output_e8s.to_string(),
                totalRewardsE8s: counts.total_rewards_e8s.to_string(),
            },
            status: FrontendDashboardExpectedStatus {
                ledgerCanisterId: ledger_id.trim().to_string(),
                indexIntervalSeconds: status.index_interval_seconds.to_string(),
                cyclesIntervalSeconds: status.cycles_interval_seconds.to_string(),
                stakingAccountIdentifier: account_identifier_text(status.staking_account.owner, status.staking_account.subaccount),
                lastIndexRunTsPresent: status.last_index_run_ts.is_some(),
                lastCyclesSweepTsPresent: status.last_completed_cycles_sweep_ts.is_some(),
            },
            registered: FrontendDashboardExpectedRegistered {
                total: registered.total.to_string(),
                items: registered
                    .items
                    .iter()
                    .map(|item| FrontendDashboardExpectedRegisteredItem {
                        canisterId: item.canister_id.to_text(),
                        qualifyingCommitmentCount: item.qualifying_commitment_count.to_string(),
                        totalQualifyingCommittedE8s: item.total_qualifying_committed_e8s.to_string(),
                        lastCommitmentTsPresent: item.last_commitment_ts.is_some(),
                        latestCycles: item.latest_cycles.as_ref().map(nat_plain_string),
                        lastCyclesProbeTsPresent: item.last_cycles_probe_ts.is_some(),
                    })
                    .collect(),
            },
            recent: FrontendDashboardExpectedRecent {
                items: recent
                    .items
                    .iter()
                    .map(|item| FrontendDashboardExpectedRecentItem {
                        canisterId: item
                            .canister_id
                            .as_ref()
                            .map(|principal| principal.to_text())
                            .or_else(|| item.memo_text.clone())
                            .unwrap_or_default(),
                        txId: item.tx_id.to_string(),
                        amountE8s: item.amount_e8s.to_string(),
                        countsTowardFaucet: item.counts_toward_faucet,
                    })
                    .collect(),
            },
            errors: FrontendDashboardExpectedErrors { stake: None },
        };

        run_frontend_dashboard_local_fixture(&expected)?;
        Ok(())
    });

    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "frontend dashboard loader excludes SNS-only canisters from registered totals",
        ),
        || {
            reset_historian_local_replica_state()?;

            let sns_root = Principal::from_text(canister_id("mock_sns_root")?.trim())?;
            let governance = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
            let ledger_id = canister_id("mock_icrc_ledger")?;
            let _: () = call_raw(
                "mock_sns_wasm",
                "debug_set_roots",
                &format!(r#"(vec {{ principal "{}" }})"#, sns_root.to_text()),
            )?;
            let summary_args = format!(
                r#"(record {{ root = opt record {{ canister_id = opt principal "{}"; status = opt record {{ cycles = opt (1000:nat) }} }}; governance = opt record {{ canister_id = opt principal "{}"; status = opt record {{ cycles = opt (2000:nat) }} }}; ledger = null; swap = null; index = null; dapps = vec {{}}; archives = vec {{}} }})"#,
                sns_root.to_text(),
                governance.to_text()
            );
            let _: () = call_raw("mock_sns_root", "debug_set_summary", &summary_args)?;
            let _: () = call_raw(
                "jupiter_historian_dbg",
                "debug_set_last_sns_discovery_ts",
                "(null)",
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_historian_dbg", "debug_driver_tick")?;

            let counts: HistorianPublicCounts =
                call_raw("jupiter_historian_dbg", "get_public_counts", "()")?;
            if counts.registered_canister_count != 0
                || counts.qualifying_commitment_count != 0
                || counts.total_output_e8s != 0
                || counts.total_rewards_e8s != 0
            {
                bail!(
                "unexpected SNS-only fixture public counts: registered={} qualifying={} output={} rewards={}",
                counts.registered_canister_count,
                counts.qualifying_commitment_count,
                counts.total_output_e8s,
                counts.total_rewards_e8s
            );
            }
            if counts.sns_discovered_canister_count < 2 {
                bail!(
                    "expected SNS-only fixture to expose discovered canisters, got {}",
                    counts.sns_discovered_canister_count
                );
            }

            let status: HistorianPublicStatus =
                call_raw("jupiter_historian_dbg", "get_public_status", "()")?;
            let registered: ListRegisteredCanisterSummariesResponse = call_raw(
                "jupiter_historian_dbg",
                "list_registered_canister_summaries",
                "(record { page = opt (0:nat32); page_size = opt (10:nat32) })",
            )?;
            let recent: ListRecentCommitmentsResponse = call_raw(
                "jupiter_historian_dbg",
                "list_recent_commitments",
                "(record { limit = opt (10:nat32); qualifying_only = opt false })",
            )?;

            let expected = FrontendDashboardExpected {
                stakeE8s: "0".to_string(),
                counts: FrontendDashboardExpectedCounts {
                    registeredCanisterCount: counts.registered_canister_count.to_string(),
                    qualifyingCommitmentCount: counts.qualifying_commitment_count.to_string(),
                    totalOutputE8s: counts.total_output_e8s.to_string(),
                    totalRewardsE8s: counts.total_rewards_e8s.to_string(),
                },
                status: FrontendDashboardExpectedStatus {
                    ledgerCanisterId: ledger_id.trim().to_string(),
                    indexIntervalSeconds: status.index_interval_seconds.to_string(),
                    cyclesIntervalSeconds: status.cycles_interval_seconds.to_string(),
                    stakingAccountIdentifier: account_identifier_text(
                        status.staking_account.owner,
                        status.staking_account.subaccount,
                    ),
                    lastIndexRunTsPresent: status.last_index_run_ts.is_some(),
                    lastCyclesSweepTsPresent: status.last_completed_cycles_sweep_ts.is_some(),
                },
                registered: FrontendDashboardExpectedRegistered {
                    total: registered.total.to_string(),
                    items: registered
                        .items
                        .iter()
                        .map(|item| FrontendDashboardExpectedRegisteredItem {
                            canisterId: item.canister_id.to_text(),
                            qualifyingCommitmentCount: item.qualifying_commitment_count.to_string(),
                            totalQualifyingCommittedE8s: item
                                .total_qualifying_committed_e8s
                                .to_string(),
                            lastCommitmentTsPresent: item.last_commitment_ts.is_some(),
                            latestCycles: item.latest_cycles.as_ref().map(nat_plain_string),
                            lastCyclesProbeTsPresent: item.last_cycles_probe_ts.is_some(),
                        })
                        .collect(),
                },
                recent: FrontendDashboardExpectedRecent {
                    items: recent
                        .items
                        .iter()
                        .map(|item| FrontendDashboardExpectedRecentItem {
                            canisterId: item
                                .canister_id
                                .as_ref()
                                .map(|principal| principal.to_text())
                                .or_else(|| item.memo_text.clone())
                                .unwrap_or_default(),
                            txId: item.tx_id.to_string(),
                            amountE8s: item.amount_e8s.to_string(),
                            countsTowardFaucet: item.counts_toward_faucet,
                        })
                        .collect(),
                },
                errors: FrontendDashboardExpectedErrors { stake: None },
            };

            run_frontend_dashboard_local_fixture(&expected)?;
            Ok(())
        },
    );

    Ok(())
}
fn run_local_historian_config_roundtrip_scenario(
    outcomes: &mut Vec<ScenarioOutcome>,
) -> Result<()> {
    run_scenario(
        outcomes,
        label(
            "icp",
            "historian",
            "checked-in mainnet install args round-trip into config",
        ),
        || {
            create_canister("jupiter_historian_args_dbg")?;
            install_with_argument_file(
                "jupiter_historian_args_dbg",
                "canisters/historian/mainnet-install-args.did",
            )?;
            let cfg: HistorianDebugConfig =
                call_raw_noargs("jupiter_historian_args_dbg", "debug_config")?;
            if cfg.staking_account != expected_mainnet_staking_account()
                || cfg.ledger_canister_id != mainnet_ledger_principal()
                || cfg.index_canister_id != mainnet_index_principal()
                || cfg.cmc_canister_id != Some(mainnet_cmc_principal())
                || cfg.faucet_canister_id != Some(prod_faucet_principal())
                || cfg.blackhole_canister_id != mainnet_blackhole_principal()
                || cfg.sns_wasm_canister_id != mainnet_sns_wasm_principal()
                || cfg.xrc_canister_id != mainnet_xrc_principal()
                || cfg.enable_sns_tracking
                || cfg.scan_interval_seconds != 600
                || cfg.cycles_interval_seconds != 604_800
                || cfg.min_tx_e8s != 100_000_000
                || cfg.max_cycles_entries_per_canister != 100
                || cfg.max_commitment_entries_per_canister != 100
                || cfg.max_index_pages_per_tick != 10
                || cfg.max_canisters_per_cycles_tick != 25
            {
                bail!("unexpected historian debug_config: {:?}", cfg);
            }
            Ok(())
        },
    );

    Ok(())
}

fn run_local_relay_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    run_scenario(
        outcomes,
        label(
            "icp",
            "relay",
            "config roundtrip includes effective self canister",
        ),
        || {
            let cfg: RelayDebugConfig = call_raw_noargs("jupiter_relay_dbg", "debug_config")?;
            let relay_id = Principal::from_text(canister_id("jupiter_relay_dbg")?.trim())?;
            let ledger_id = Principal::from_text(canister_id("mock_icrc_ledger")?.trim())?;
            let cmc_id = Principal::from_text(canister_id("mock_cmc")?.trim())?;
            let blackhole_id = Principal::from_text(canister_id("mock_blackhole")?.trim())?;
            if cfg.ledger_canister_id != ledger_id
                || cfg.cmc_canister_id != cmc_id
                || cfg.blackhole_canister_id != blackhole_id
            {
                bail!("unexpected relay config principals: {cfg:?}");
            }
            if !cfg.effective_managed_canisters.contains(&relay_id)
                || !cfg.effective_managed_canisters.contains(&cmc_id)
            {
                bail!("expected effective managed set to contain relay and CMC canister: {cfg:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "relay",
            "first complete probe stores baseline without spending",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_relay_dbg", "debug_force_clear_active_job")?;
            let cmc_id = Principal::from_text(canister_id("mock_cmc")?.trim())?;
            let blackhole_id = canister_id("mock_blackhole")?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (10000000000000:nat), vec {{ principal "{}" }})"#,
                    cmc_id.to_text(),
                    blackhole_id.trim()
                ),
            )?;
            let relay_account = Account {
                owner: Principal::from_text(canister_id("jupiter_relay_dbg")?.trim())?,
                subaccount: None,
            };
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 5000000000:nat64)", account_to_candid(&relay_account)),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_relay_dbg", "debug_main_tick")?;
            let summary: Option<RelaySummary> =
                call_raw_noargs("jupiter_relay_dbg", "debug_last_summary")?;
            let summary = summary.context("expected relay summary")?;
            if summary.mode != RelayMode::BaselineOnly || summary.transfer_count != 0 {
                bail!("expected baseline-only summary with no transfers, got {summary:?}");
            }
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers.is_empty() {
                bail!("expected no relay transfers during baseline, got {transfers:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label("icp", "relay", "public logs include cycles and config"),
        || {
            let logs = relay_local_logs("jupiter_relay_dbg")?;
            if !logs.contains("Cycles:") || !logs.contains("CONFIG ") {
                bail!("expected relay logs to include Cycles and CONFIG lines, got {logs}");
            }
            if !logs.contains("managed_canisters")
                || !logs.contains("effective_managed_canisters")
                || !logs.contains("max_transfers_per_tick")
            {
                bail!("expected relay config log to include managed/effective sets and transfer limit, got {logs}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label("icp", "relay", "second probe allocates weighted CMC top-up"),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let cmc_id = Principal::from_text(canister_id("mock_cmc")?.trim())?;
            let blackhole_id = canister_id("mock_blackhole")?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (5000000000000:nat), vec {{ principal "{}" }})"#,
                    cmc_id.to_text(),
                    blackhole_id.trim()
                ),
            )?;
            let relay_account = Account {
                owner: Principal::from_text(canister_id("jupiter_relay_dbg")?.trim())?,
                subaccount: None,
            };
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 5000000000:nat64)", account_to_candid(&relay_account)),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_relay_dbg", "debug_main_tick")?;
            let summary: Option<RelaySummary> =
                call_raw_noargs("jupiter_relay_dbg", "debug_last_summary")?;
            let summary = summary.context("expected relay top-up summary")?;
            if summary.mode != RelayMode::TopUpThenSurplus || summary.transfer_count == 0 {
                bail!("expected cycles top-up summary with transfers, got {summary:?}");
            }
            if summary
                .canisters
                .iter()
                .all(|sample| sample.canister_id != relay_account.owner)
            {
                bail!("expected relay self canister in summary: {summary:?}");
            }
            let logs = relay_local_logs("jupiter_relay_dbg")?;
            if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
                || !logs.contains("RELAY_CANISTER ")
                || !logs.contains("burn_cycles=")
                || !logs.contains("planned_topup_e8s=")
                || !logs.contains("sent_topup_e8s=")
            {
                bail!("expected cycles top-up public logs with summary and canister allocation fields, got {logs}");
            }
            let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes.iter().all(|note| note.canister_id != cmc_id) {
                bail!("expected CMC notification for managed canister, got {notes:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "relay",
            "missing blackhole status fails closed without transfers",
        ),
        || {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw("mock_blackhole", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_relay_dbg", "debug_force_clear_active_job")?;
            let relay_account = Account {
                owner: Principal::from_text(canister_id("jupiter_relay_dbg")?.trim())?,
                subaccount: None,
            };
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 100000000:nat64)", account_to_candid(&relay_account)),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_relay_dbg", "debug_main_tick")?;
            let summary: Option<RelaySummary> =
                call_raw_noargs("jupiter_relay_dbg", "debug_last_summary")?;
            let summary = summary.context("expected relay degraded summary")?;
            if summary.mode != RelayMode::Degraded || summary.probe_failures.is_empty() {
                bail!("expected degraded summary with probe failure, got {summary:?}");
            }
            let logs = relay_local_logs("jupiter_relay_dbg")?;
            if !logs.contains("RELAY_SUMMARY mode=Degraded")
                || !logs.contains("RELAY_PROBE_FAILURE ")
            {
                bail!("expected degraded public logs with probe failure, got {logs}");
            }
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers.is_empty() {
                bail!("expected fail-closed relay tick to avoid transfers, got {transfers:?}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "relay",
            "surplus canister recipient uses configured memo",
        ),
        || {
            ensure_canister_exists("jupiter_relay_args_dbg")?;
            let ledger_id = Principal::from_text(canister_id("mock_icrc_ledger")?.trim())?;
            let cmc_id = Principal::from_text(canister_id("mock_cmc")?.trim())?;
            let blackhole_id = Principal::from_text(canister_id("mock_blackhole")?.trim())?;
            let external = blackhole_id;
            let raw_args = format!(
                r#"(record {{
                managed_canisters = vec {{ principal "{managed_id}" }};
                ledger_canister_id = opt principal "{ledger_id}";
                cmc_canister_id = opt principal "{cmc_id}";
                governance_canister_id = opt principal "{cmc_id}";
                blackhole_canister_id = opt principal "{blackhole_id}";
                main_interval_seconds = opt (31536000:nat64);
                max_transfers_per_tick = opt (10:nat32);
                surplus_canister_recipients = opt vec {{
                    record {{
                        canister_id = principal "{external}";
                        memo = blob "\01";
                    }};
                }};
                surplus_neuron_recipients = vec {{}};
            }},)"#,
                managed_id = cmc_id.to_text(),
                ledger_id = ledger_id.to_text(),
                cmc_id = cmc_id.to_text(),
                blackhole_id = blackhole_id.to_text(),
                external = external.to_text(),
            );
            let wasm = wasm_path_for_canister("jupiter_relay_args_dbg")?;
            run_icp_with_identity(&[
                "canister",
                "install",
                "--environment",
                LOCAL_ENVIRONMENT,
                "jupiter_relay_args_dbg",
                "--wasm",
                &wasm,
                "--mode",
                "reinstall",
                "--yes",
                "--args",
                &raw_args,
            ])?;
            add_self_controller("jupiter_relay_args_dbg")?;

            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw(
                "mock_cmc",
                "debug_set_conversion_rate",
                "(record { timestamp_seconds = 4000000000:nat64; xdr_permyriad_per_icp = 10000000:nat64 })",
            )?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (10000000000000:nat), vec {{ principal "{}" }})"#,
                    cmc_id.to_text(),
                    blackhole_id.to_text()
                ),
            )?;
            let relay_account = Account {
                owner: Principal::from_text(canister_id("jupiter_relay_args_dbg")?.trim())?,
                subaccount: None,
            };
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 300000000:nat64)", account_to_candid(&relay_account)),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_relay_args_dbg", "debug_main_tick")?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (5000000000000:nat), vec {{ principal "{}" }})"#,
                    cmc_id.to_text(),
                    blackhole_id.to_text()
                ),
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_relay_args_dbg", "debug_main_tick")?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 300000000:nat64)", account_to_candid(&relay_account)),
            )?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (10000000000000:nat), vec {{ principal "{}" }})"#,
                    cmc_id.to_text(),
                    blackhole_id.to_text()
                ),
            )?;
            let _: () = call_raw_noargs::<()>("jupiter_relay_args_dbg", "debug_main_tick")?;
            let summary: Option<RelaySummary> =
                call_raw_noargs("jupiter_relay_args_dbg", "debug_last_summary")?;
            let summary = summary.context("expected relay surplus summary")?;
            if summary.mode != RelayMode::TopUpThenSurplus || summary.ledger_transfer_count == 0 {
                bail!("expected top-up/surplus summary with ledger transfers, got {summary:?}");
            }
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if !transfers
                .iter()
                .any(|t| t.to.owner == external && t.memo == Some(vec![1]))
            {
                bail!("expected surplus canister transfer with memo 1, got {transfers:?}");
            }
            let logs = relay_local_logs("jupiter_relay_args_dbg")?;
            if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
                || !logs.contains("RELAY_SURPLUS_TRANSFER ")
                || !logs.contains("memo_len=1")
            {
                bail!("expected surplus public logs with recipient and memo length, got {logs}");
            }
            Ok(())
        },
    );

    run_scenario(
        outcomes,
        label(
            "icp",
            "relay",
            "empty surplus recipients route all spendable ICP as cycles",
        ),
        || {
            ensure_canister_exists("jupiter_relay_args_dbg")?;
            let relay_id = Principal::from_text(canister_id("jupiter_relay_args_dbg")?.trim())?;
            let ledger_id = Principal::from_text(canister_id("mock_icrc_ledger")?.trim())?;
            let cmc_id = Principal::from_text(canister_id("mock_cmc")?.trim())?;
            let blackhole_id = Principal::from_text(canister_id("mock_blackhole")?.trim())?;
            let raw_args = format!(
                r#"(record {{
                managed_canisters = vec {{ principal "{managed_id}" }};
                ledger_canister_id = opt principal "{ledger_id}";
                cmc_canister_id = opt principal "{cmc_id}";
                governance_canister_id = opt principal "{cmc_id}";
                blackhole_canister_id = opt principal "{blackhole_id}";
                main_interval_seconds = opt (31536000:nat64);
                max_transfers_per_tick = opt (10:nat32);
                surplus_canister_recipients = null;
                surplus_neuron_recipients = vec {{}};
            }},)"#,
                managed_id = cmc_id.to_text(),
                ledger_id = ledger_id.to_text(),
                cmc_id = cmc_id.to_text(),
                blackhole_id = blackhole_id.to_text(),
            );
            let wasm = wasm_path_for_canister("jupiter_relay_args_dbg")?;
            run_icp_with_identity(&[
                "canister",
                "install",
                "--environment",
                LOCAL_ENVIRONMENT,
                "jupiter_relay_args_dbg",
                "--wasm",
                &wasm,
                "--mode",
                "reinstall",
                "--yes",
                "--args",
                &raw_args,
            ])?;
            add_self_controller("jupiter_relay_args_dbg")?;

            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw(
                "mock_blackhole",
                "debug_set_status",
                &format!(
                    r#"(principal "{}", opt (5000000000000:nat), vec {{ principal "{}" }})"#,
                    cmc_id.to_text(),
                    blackhole_id.to_text()
                ),
            )?;
            let relay_account = Account {
                owner: relay_id,
                subaccount: None,
            };
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 99000000:nat64)", account_to_candid(&relay_account)),
            )?;

            let mut summary = None;
            for _ in 0..3 {
                let _: () = call_raw_noargs::<()>("jupiter_relay_args_dbg", "debug_main_tick")?;
                summary = call_raw_noargs("jupiter_relay_args_dbg", "debug_last_summary")?;
                if matches!(
                    summary.as_ref().map(|summary: &RelaySummary| &summary.mode),
                    Some(RelayMode::TopUpThenSurplus)
                ) {
                    break;
                }
            }
            let summary = summary.context("expected relay no-surplus-recipient summary")?;
            if summary.mode != RelayMode::TopUpThenSurplus
                || summary.skipped_surplus_reason.as_deref() != Some("no_raw_icp_recipients")
            {
                bail!("expected empty surplus config to route spendable ICP as cycles, got {summary:?}");
            }
            if summary.cmc_notify_success_count == 0 || summary.surplus_transfers.len() != 0 {
                bail!("expected CMC top-up and no raw surplus transfers without recipients, got {summary:?}");
            }
            let transfers: Vec<TransferRecord> =
                call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers
                .iter()
                .any(|t| t.to.owner == blackhole_id && t.memo == Some(vec![1]))
            {
                bail!("expected no surplus transfer without recipients, got {transfers:?}");
            }
            Ok(())
        },
    );

    Ok(())
}

fn run_local_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    run_local_disburser_scenarios(outcomes)?;
    run_local_faucet_scenarios(outcomes)?;
    run_local_historian_scenarios(outcomes)?;
    run_local_relay_scenarios(outcomes)?;
    run_local_frontend_scenarios(outcomes)?;
    run_local_historian_config_roundtrip_scenario(outcomes)?;
    Ok(())
}

fn finish_outcomes(
    outcomes: Vec<ScenarioOutcome>,
    failure_message: &str,
    success_message: &str,
) -> Result<()> {
    let ok = print_summary(&outcomes);
    if ok {
        eprintln!("{GREEN}{BOLD}✅ {success_message}{RESET}\n");
        Ok(())
    } else {
        bail!("{failure_message}")
    }
}

fn run_unit_disburser_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    run_cargo_test_suite(
        outcomes,
        "unit",
        "disburser",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-disburser",
            "--lib",
            "--",
            "--color",
            "always",
        ],
        &root,
        &[],
    )
}

fn run_unit_faucet_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    run_cargo_test_suite(
        outcomes,
        "unit",
        "faucet",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-faucet",
            "--lib",
            "--",
            "--color",
            "always",
        ],
        &root,
        &[],
    )
}

fn run_unit_historian_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    run_cargo_test_suite(
        outcomes,
        "unit",
        "historian",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-historian",
            "--lib",
            "--",
            "--color",
            "always",
        ],
        &root,
        &[],
    )
}

fn run_unit_relay_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    run_cargo_test_suite(
        outcomes,
        "unit",
        "relay",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-relay",
            "--lib",
            "--",
            "--color",
            "always",
        ],
        &root,
        &[],
    )
}

fn ensure_frontend_node_modules() -> Result<()> {
    let root = repo_root();
    let root_path = std::path::Path::new(&root);
    let marker = root_path.join("node_modules").join("@icp-sdk").join("core");
    let package_json = root_path.join("package.json");
    let package_lock = root_path.join("package-lock.json");
    let stamp = root_path.join("node_modules").join(".frontend-deps-stamp");

    let package_json_contents = std::fs::read_to_string(&package_json)
        .with_context(|| format!("failed to read {}", package_json.display()))?;
    let package_lock_contents = if package_lock.exists() {
        Some(
            std::fs::read_to_string(&package_lock)
                .with_context(|| format!("failed to read {}", package_lock.display()))?,
        )
    } else {
        None
    };
    let expected_stamp = if let Some(package_lock_contents) = &package_lock_contents {
        format!(
            "package.json\n{}\n---\npackage-lock.json\n{}",
            package_json_contents, package_lock_contents,
        )
    } else {
        format!("package.json\n{}", package_json_contents)
    };

    if marker.exists() {
        if let Ok(existing_stamp) = std::fs::read_to_string(&stamp) {
            if existing_stamp == expected_stamp {
                return Ok(());
            }
        }
    }

    let lock_check = Command::new("node")
        .arg("tools/scripts/check-npm-lock-hermetic.mjs")
        .current_dir(&root)
        .output()
        .context("failed to run npm lockfile hermeticity check")?;
    if !lock_check.status.success() {
        let stdout = String::from_utf8_lossy(&lock_check.stdout);
        let stderr = String::from_utf8_lossy(&lock_check.stderr);
        if !stdout.trim().is_empty() {
            eprintln!("{}", stdout.trim_end());
        }
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim_end());
        }
        bail!("npm lockfile hermeticity check failed");
    }

    let npm_args = if package_lock.exists() {
        vec!["ci", "--no-fund", "--no-audit", "--silent"]
    } else {
        vec!["install", "--no-fund", "--no-audit", "--silent"]
    };

    eprintln!(
        "frontend tests: refreshing npm dependencies via npm {}",
        npm_args[0]
    );
    let output = Command::new("npm")
        .args(&npm_args)
        .current_dir(&root)
        .output()
        .with_context(|| format!("failed to run npm {} for frontend tests", npm_args[0]))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            eprintln!("{}", stdout.trim_end());
        }
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim_end());
        }
        bail!("npm {} failed for frontend tests", npm_args[0]);
    }

    if let Some(parent) = stamp.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(&stamp, expected_stamp)
        .with_context(|| format!("failed to write {}", stamp.display()))?;
    Ok(())
}

fn cmd_frontend_setup() -> Result<()> {
    ensure_frontend_node_modules()?;
    eprintln!("frontend_setup complete");
    Ok(())
}

fn run_frontend_unit_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    ensure_frontend_node_modules()?;
    let root = repo_root();
    run_cargo_test_suite(
        outcomes,
        "unit",
        "frontend",
        "npm",
        &["run", "test:frontend-unit"],
        &root,
        &[],
    )
}

fn run_frontend_local_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    ensure_frontend_node_modules()?;
    run_local_frontend_scenarios(outcomes)
}

fn run_pocketic_disburser_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = pocketic_test_env()?;
    run_cargo_test_suite(
        outcomes,
        "pocketic",
        "disburser",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-disburser",
            "--test",
            "jupiter_disburser_integration",
            "--",
            "--ignored",
            "--color",
            "always",
        ],
        &root,
        &common_env,
    )
}

fn run_pocketic_faucet_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = pocketic_test_env()?;
    run_cargo_test_suite(
        outcomes,
        "pocketic",
        "faucet",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-faucet",
            "--test",
            "jupiter_faucet_integration",
            "--",
            "--ignored",
            "--color",
            "always",
        ],
        &root,
        &common_env,
    )
}

fn run_pocketic_historian_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = pocketic_test_env()?;
    run_cargo_test_suite(
        outcomes,
        "pocketic",
        "historian",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-historian",
            "--test",
            "jupiter_historian_integration",
            "--",
            "--ignored",
            "--color",
            "always",
        ],
        &root,
        &common_env,
    )
}

fn run_pocketic_relay_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = pocketic_test_env()?;
    run_cargo_test_suite(
        outcomes,
        "pocketic",
        "relay",
        "cargo",
        &[
            "test",
            "-p",
            "jupiter-relay",
            "--test",
            "jupiter_relay_integration",
            "--",
            "--ignored",
            "--color",
            "always",
        ],
        &root,
        &common_env,
    )
}

fn run_e2e_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = pocketic_test_env()?;
    run_cargo_test_suite(
        outcomes,
        "e2e",
        "",
        "cargo",
        &[
            "test",
            "-p",
            "xtask",
            "--test",
            "e2e",
            "--",
            "--ignored",
            "--color",
            "always",
        ],
        &root,
        &common_env,
    )
}

fn run_repo_validation_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    run_cargo_test_suite(
        outcomes,
        "repo",
        "",
        "python3",
        &["./tools/scripts/validate-mainnet-install-args"],
        &root,
        &[],
    )
}

fn run_unit_component(outcomes: &mut Vec<ScenarioOutcome>, component: TestComponent) -> Result<()> {
    match component {
        TestComponent::Test => {
            run_repo_validation_suite(outcomes)?;
            run_unit_disburser_suite(outcomes)?;
            run_unit_faucet_suite(outcomes)?;
            run_unit_historian_suite(outcomes)?;
            run_unit_relay_suite(outcomes)?;
            run_frontend_unit_suite(outcomes)?;
        }
        TestComponent::Disburser => run_unit_disburser_suite(outcomes)?,
        TestComponent::Faucet => run_unit_faucet_suite(outcomes)?,
        TestComponent::Historian => run_unit_historian_suite(outcomes)?,
        TestComponent::Relay => run_unit_relay_suite(outcomes)?,
        TestComponent::Frontend => run_frontend_unit_suite(outcomes)?,
        TestComponent::E2e => bail!("e2e_unit is not supported; use e2e_all"),
    }
    Ok(())
}

fn run_local_component(
    outcomes: &mut Vec<ScenarioOutcome>,
    component: TestComponent,
) -> Result<()> {
    match component {
        TestComponent::Test => run_local_scenarios(outcomes)?,
        TestComponent::Disburser => run_local_disburser_scenarios(outcomes)?,
        TestComponent::Faucet => run_local_faucet_scenarios(outcomes)?,
        TestComponent::Historian => {
            run_local_historian_scenarios(outcomes)?;
            run_local_historian_config_roundtrip_scenario(outcomes)?;
        }
        TestComponent::Relay => run_local_relay_scenarios(outcomes)?,
        TestComponent::Frontend => run_frontend_local_suite(outcomes)?,
        TestComponent::E2e => bail!("e2e_local_integration is not supported; use e2e_all"),
    }
    Ok(())
}

fn run_pocketic_component(
    outcomes: &mut Vec<ScenarioOutcome>,
    component: TestComponent,
) -> Result<()> {
    match component {
        TestComponent::Test => {
            run_pocketic_disburser_suite(outcomes)?;
            run_pocketic_faucet_suite(outcomes)?;
            run_pocketic_historian_suite(outcomes)?;
            run_pocketic_relay_suite(outcomes)?;
            run_e2e_suite(outcomes)?;
        }
        TestComponent::Disburser => run_pocketic_disburser_suite(outcomes)?,
        TestComponent::Faucet => run_pocketic_faucet_suite(outcomes)?,
        TestComponent::Historian => run_pocketic_historian_suite(outcomes)?,
        TestComponent::Relay => run_pocketic_relay_suite(outcomes)?,
        TestComponent::Frontend => bail!("frontend_pocketic_integration is not supported; use frontend_all or frontend_local_integration"),
        TestComponent::E2e => run_e2e_suite(outcomes)?,
    }
    Ok(())
}

fn scoped_command_needs_local_env(component: TestComponent, scope: TestScope) -> bool {
    match scope {
        TestScope::LocalIntegration => true,
        TestScope::All => component != TestComponent::E2e,
        TestScope::Unit | TestScope::PocketicIntegration => false,
    }
}

fn run_scoped_command(component: TestComponent, scope: TestScope) -> Result<()> {
    let mut outcomes: Vec<ScenarioOutcome> = Vec::new();

    match scope {
        TestScope::Unit => run_unit_component(&mut outcomes, component)?,
        TestScope::LocalIntegration => run_local_component(&mut outcomes, component)?,
        TestScope::PocketicIntegration => run_pocketic_component(&mut outcomes, component)?,
        TestScope::All => match component {
            TestComponent::Test => {
                run_local_component(&mut outcomes, component)?;
                run_unit_component(&mut outcomes, component)?;
                run_pocketic_component(&mut outcomes, component)?;
            }
            TestComponent::Disburser
            | TestComponent::Faucet
            | TestComponent::Historian
            | TestComponent::Relay => {
                run_unit_component(&mut outcomes, component)?;
                run_local_component(&mut outcomes, component)?;
                run_pocketic_component(&mut outcomes, component)?;
            }
            TestComponent::Frontend => {
                run_unit_component(&mut outcomes, component)?;
                run_local_component(&mut outcomes, component)?;
            }
            TestComponent::E2e => run_e2e_suite(&mut outcomes)?,
        },
    }

    let failure_message = match (component, scope) {
        (TestComponent::Test, TestScope::Unit) => "one or more unit test suites failed",
        (TestComponent::Test, TestScope::LocalIntegration) => {
            "one or more local icp integration scenario suites failed"
        }
        (TestComponent::Test, TestScope::PocketicIntegration) => {
            "one or more pocketic integration or e2e suites failed"
        }
        (TestComponent::Test, TestScope::All) => {
            "one or more tests failed across icp, unit, pocketic, or e2e layers"
        }
        (TestComponent::Disburser, TestScope::Unit) => "the disburser unit test suite failed",
        (TestComponent::Disburser, TestScope::LocalIntegration) => {
            "one or more disburser local icp integration scenarios failed"
        }
        (TestComponent::Disburser, TestScope::PocketicIntegration) => {
            "the disburser pocketic integration suite failed"
        }
        (TestComponent::Disburser, TestScope::All) => "one or more disburser test suites failed",
        (TestComponent::Faucet, TestScope::Unit) => "the faucet unit test suite failed",
        (TestComponent::Faucet, TestScope::LocalIntegration) => {
            "one or more faucet local icp integration scenarios failed"
        }
        (TestComponent::Faucet, TestScope::PocketicIntegration) => {
            "the faucet pocketic integration suite failed"
        }
        (TestComponent::Faucet, TestScope::All) => "one or more faucet test suites failed",
        (TestComponent::Historian, TestScope::Unit) => "the historian unit test suite failed",
        (TestComponent::Historian, TestScope::LocalIntegration) => {
            "one or more historian local icp integration scenarios failed"
        }
        (TestComponent::Historian, TestScope::PocketicIntegration) => {
            "the historian pocketic integration suite failed"
        }
        (TestComponent::Historian, TestScope::All) => "one or more historian test suites failed",
        (TestComponent::Relay, TestScope::Unit) => "the relay unit test suite failed",
        (TestComponent::Relay, TestScope::LocalIntegration) => {
            "one or more relay local icp integration scenarios failed"
        }
        (TestComponent::Relay, TestScope::PocketicIntegration) => {
            "the relay pocketic integration suite failed"
        }
        (TestComponent::Relay, TestScope::All) => "one or more relay test suites failed",
        (TestComponent::Frontend, TestScope::Unit) => "the frontend unit test suite failed",
        (TestComponent::Frontend, TestScope::LocalIntegration) => {
            "one or more frontend local icp integration scenarios failed"
        }
        (TestComponent::Frontend, TestScope::All) => "one or more frontend test suites failed",
        (TestComponent::E2e, TestScope::PocketicIntegration)
        | (TestComponent::E2e, TestScope::All) => "the e2e suite failed",
        _ => "the selected xtask command failed",
    };

    let success_message = match (component, scope) {
        (TestComponent::Test, TestScope::Unit) => "test_unit complete",
        (TestComponent::Test, TestScope::LocalIntegration) => "test_local_integration complete",
        (TestComponent::Test, TestScope::PocketicIntegration) => {
            "test_pocketic_integration complete"
        }
        (TestComponent::Test, TestScope::All) => "test_all complete",
        (TestComponent::Disburser, TestScope::Unit) => "disburser_unit complete",
        (TestComponent::Disburser, TestScope::LocalIntegration) => {
            "disburser_local_integration complete"
        }
        (TestComponent::Disburser, TestScope::PocketicIntegration) => {
            "disburser_pocketic_integration complete"
        }
        (TestComponent::Disburser, TestScope::All) => "disburser_all complete",
        (TestComponent::Faucet, TestScope::Unit) => "faucet_unit complete",
        (TestComponent::Faucet, TestScope::LocalIntegration) => "faucet_local_integration complete",
        (TestComponent::Faucet, TestScope::PocketicIntegration) => {
            "faucet_pocketic_integration complete"
        }
        (TestComponent::Faucet, TestScope::All) => "faucet_all complete",
        (TestComponent::Historian, TestScope::Unit) => "historian_unit complete",
        (TestComponent::Historian, TestScope::LocalIntegration) => {
            "historian_local_integration complete"
        }
        (TestComponent::Historian, TestScope::PocketicIntegration) => {
            "historian_pocketic_integration complete"
        }
        (TestComponent::Historian, TestScope::All) => "historian_all complete",
        (TestComponent::Relay, TestScope::Unit) => "relay_unit complete",
        (TestComponent::Relay, TestScope::LocalIntegration) => "relay_local_integration complete",
        (TestComponent::Relay, TestScope::PocketicIntegration) => {
            "relay_pocketic_integration complete"
        }
        (TestComponent::Relay, TestScope::All) => "relay_all complete",
        (TestComponent::Frontend, TestScope::Unit) => "frontend_unit complete",
        (TestComponent::Frontend, TestScope::LocalIntegration) => {
            "frontend_local_integration complete"
        }
        (TestComponent::Frontend, TestScope::All) => "frontend_all complete",
        (TestComponent::E2e, TestScope::PocketicIntegration) => "e2e_pocketic_integration complete",
        (TestComponent::E2e, TestScope::All) => "e2e_all complete",
        _ => "xtask command complete",
    };

    finish_outcomes(outcomes, failure_message, success_message)
}

fn cmd_scoped(component: TestComponent, scope: TestScope) -> Result<()> {
    if !scoped_command_needs_local_env(component, scope) {
        return run_scoped_command(component, scope);
    }

    let setup_res = match (component, scope) {
        (TestComponent::Disburser, TestScope::LocalIntegration | TestScope::All) => {
            cmd_setup_disburser_local()
        }
        (TestComponent::Faucet, TestScope::LocalIntegration | TestScope::All) => {
            cmd_setup_faucet_local()
        }
        (TestComponent::Historian, TestScope::LocalIntegration | TestScope::All) => {
            cmd_setup_historian_local()
        }
        (TestComponent::Relay, TestScope::LocalIntegration | TestScope::All) => {
            cmd_setup_relay_local()
        }
        (TestComponent::Frontend, TestScope::LocalIntegration | TestScope::All) => {
            cmd_setup_historian_local()
        }
        _ => cmd_setup(),
    };
    setup_res?;
    let run_res = run_scoped_command(component, scope);
    let teardown_res = cmd_teardown();

    match (run_res, teardown_res) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(run_err), Ok(())) => Err(run_err),
        (Ok(()), Err(teardown_err)) => Err(teardown_err),
        (Err(run_err), Err(teardown_err)) => {
            eprintln!("⚠️ teardown also failed after scoped local icp run: {teardown_err:#}");
            Err(run_err)
        }
    }
}

fn main() -> Result<()> {
    let cmd = env::args().nth(1).unwrap_or_else(|| "help".to_string());
    if let Some((component, scope)) = parse_scoped_command(&cmd) {
        return cmd_scoped(component, scope);
    }

    match cmd.as_str() {
        "setup" => cmd_setup(),
        "teardown" => cmd_teardown(),
        "frontend_setup" => cmd_frontend_setup(),
        "faucet_production_reinstall_cutover" => cmd_faucet_production_reinstall_cutover(),
        _ => {
            eprintln!(
                "Usage: cargo run -p xtask -- <command>

                 Utility commands:
                 - setup
                 - teardown
                 - frontend_setup
                 - faucet_production_reinstall_cutover

                 Scoped commands:
                 - disburser_unit
                 - disburser_local_integration
                 - disburser_pocketic_integration
                 - disburser_all
                 - faucet_unit
                 - faucet_local_integration
                 - faucet_pocketic_integration
                 - faucet_all
                 - historian_unit
                 - historian_local_integration
                 - historian_pocketic_integration
                 - historian_all
                 - relay_unit
                 - relay_local_integration
                 - relay_pocketic_integration
                 - relay_all
                 - frontend_unit
                 - frontend_local_integration
                 - frontend_all
                 - e2e_all
                 - e2e_pocketic_integration
                 - test_unit
                 - test_local_integration
                 - test_pocketic_integration
                 - test_all
"
            );
            Ok(())
        }
    }
}
