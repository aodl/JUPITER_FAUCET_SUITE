use anyhow::{bail, Context, Result};
use candid::{decode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use num_traits::ToPrimitive;
use sha2::{Digest, Sha224};
use std::env;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::fs::{self, OpenOptions};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const DFX_IDENTITY: &str = "xtask-dev";

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";

#[derive(Debug)]
struct ScenarioOutcome {
    name: String,
    ms: u128,
    passed: bool,
    error: Option<String>,
}

fn repo_root() -> String {
    // xtask/Cargo.toml dir
    let xtask_dir = env!("CARGO_MANIFEST_DIR");
    // repo root is one directory up from xtask/
    std::path::Path::new(xtask_dir)
        .parent()
        .expect("xtask should live under repo root")
        .to_string_lossy()
        .to_string()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestComponent {
    Test,
    Disburser,
    Faucet,
    E2e,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestScope {
    Unit,
    DfxIntegration,
    PocketicIntegration,
    All,
}

fn parse_scoped_command(cmd: &str) -> Option<(TestComponent, TestScope)> {
    use TestComponent::{Disburser, E2e, Faucet, Test};
    use TestScope::{All, DfxIntegration, PocketicIntegration, Unit};

    match cmd {
        "disburser_unit" => Some((Disburser, Unit)),
        "disburser_dfx_integration" => Some((Disburser, DfxIntegration)),
        "disburser_pocketic_integration" => Some((Disburser, PocketicIntegration)),
        "disburser_all" => Some((Disburser, All)),
        "faucet_unit" => Some((Faucet, Unit)),
        "faucet_dfx_integration" => Some((Faucet, DfxIntegration)),
        "faucet_pocketic_integration" => Some((Faucet, PocketicIntegration)),
        "faucet_all" => Some((Faucet, All)),
        "e2e_all" => Some((E2e, All)),
        "e2e_pocketic_integration" => Some((E2e, PocketicIntegration)),
        "test_unit" => Some((Test, Unit)),
        "test_dfx_integration" => Some((Test, DfxIntegration)),
        "test_pocketic_integration" => Some((Test, PocketicIntegration)),
        "test_all" => Some((Test, All)),
        _ => None,
    }
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


fn is_suppressed_dfx_success_stderr_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty()
        || trimmed.contains("] Cycles: ")
        || (trimmed.contains(" UTC: [Canister ") && trimmed.contains("] "))
}

fn run_dfx(args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("dfx");

    // Always use a dedicated, non-interactive identity
    cmd.args(["--identity", DFX_IDENTITY]);
    cmd.args(args);

    let rendered_cmd = format!(
        "dfx {}",
        cmd.get_args()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let verbose = env::var("VERBOSE_DFX")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if verbose {
        eprintln!("▶ {rendered_cmd}");
    }

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to spawn dfx")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        for line in stderr.lines() {
            if is_suppressed_dfx_success_stderr_line(line) {
                continue;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                eprintln!("{trimmed}");
            }
        }
    } else if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }

    if !output.status.success() {
        eprintln!("▶ {rendered_cmd}");
        bail!("dfx {:?} failed", args);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn call_raw<T>(canister: &str, method: &str, args: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de> + CandidType,
{
    let out = run_dfx(&["canister", "call", canister, method, args, "--output", "raw"])?;
    let hex_str = out.trim().trim_start_matches("0x");
    let bytes = hex::decode(hex_str)?;
    Ok(decode_one(&bytes)?)
}

fn call_raw_noargs<T>(canister: &str, method: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de> + CandidType,
{
    call_raw(canister, method, "()")
}

fn canister_id(name: &str) -> Result<String> {
    run_dfx(&["canister", "id", name])
}

fn principal_of_identity() -> Result<Principal> {
    let p = run_dfx(&["identity", "get-principal"])?;
    Ok(Principal::from_text(p.trim())?)
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
struct DebugState {
    prev_age_seconds: u64,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    payout_plan_present: bool,
}

#[derive(Debug, CandidType, Deserialize)]
struct FaucetDebugAccounts {
    payout: Account,
    staking: Account,
}

#[derive(Debug, CandidType, Deserialize)]
struct FaucetDebugState {
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    active_payout_job_present: bool,
    retry_state_present: bool,
    last_summary_present: bool,
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
    ignored_under_threshold: u64,
    ignored_bad_memo: u64,
    remainder_to_self_e8s: u64,
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

fn account_identifier_text(account: &Account) -> String {
    let subaccount = account.subaccount.unwrap_or([0u8; 32]);
    let mut hasher = Sha224::new();
    hasher.update(b"\x0Aaccount-id");
    hasher.update(account.owner.as_slice());
    hasher.update(subaccount);
    let hash = hasher.finalize();
    let checksum = crc32fast::hash(&hash).to_be_bytes();
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&checksum);
    bytes[4..].copy_from_slice(&hash);
    hex::encode(bytes)
}

fn account_to_candid(account: &Account) -> String {
    let subaccount = match account.subaccount {
        Some(bytes) => format!(
            "opt vec {{ {} }}",
            bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join("; ")
        ),
        None => "null".to_string(),
    };
    format!(
        "record {{ owner = principal \"{}\"; subaccount = {} }}",
        account.owner.to_text(),
        subaccount
    )
}

fn opt_blob_to_candid(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => format!(
            "opt vec {{ {} }}",
            bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join("; ")
        ),
        None => "null".to_string(),
    }
}

fn opt_nat64_to_candid(v: u64) -> String {
    format!("(opt ({}:nat64))", v)
}

fn ensure_identity() -> Result<()> {
    // If identity already exists, this returns OK.
    let list = Command::new("dfx")
        .args(["identity", "list"])
        .output()
        .context("failed to run dfx identity list")?;

    if !list.status.success() {
        bail!("dfx identity list failed");
    }

    let stdout = String::from_utf8_lossy(&list.stdout);
    let exists = stdout
        .lines()
        .any(|l| l.trim().trim_start_matches('*').trim() == DFX_IDENTITY);

    if exists {
        return Ok(());
    }

    eprintln!("▶ creating non-interactive dfx identity: {DFX_IDENTITY}");

    // Create plaintext storage identity (no passphrase prompts).
    let status = Command::new("dfx")
        .args(["identity", "new", DFX_IDENTITY, "--storage-mode", "plaintext"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run dfx identity new")?;

    if !status.success() {
        bail!("dfx identity new {DFX_IDENTITY} failed");
    }

    Ok(())
}

fn cmd_setup() -> Result<()> {
    ensure_identity()?;

    // Stop any running local replica (ignore errors), then start clean.
    let _ = run_dfx(&["stop"]);
    // Start replica quietly and send background replica logs to a file so they
    // do not pollute the scenario-by-scenario test output.
    {
        let repo = repo_root();
        let dfx_dir = std::path::Path::new(&repo).join(".dfx");
        fs::create_dir_all(&dfx_dir).context("failed to create .dfx directory for replica logs")?;
        let replica_log_path = dfx_dir.join("xtask-replica.log");
        let replica_log = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&replica_log_path)
            .with_context(|| format!("failed to open replica log at {}", replica_log_path.display()))?;
        let replica_log_err = replica_log
            .try_clone()
            .context("failed to clone replica log file handle")?;

        let mut start = Command::new("dfx");
        start.args(["start", "--background", "--clean"]);
        start.stdout(Stdio::from(replica_log));
        start.stderr(Stdio::from(replica_log_err));
        let status = start.status().context("dfx start failed")?;
        if !status.success() {
            bail!("dfx start failed");
        }
    }

    run_dfx(&["deploy", "mock_icrc_ledger"])?;
    run_dfx(&["deploy", "mock_nns_governance"])?;
    run_dfx(&["deploy", "mock_icp_index"])?;
    run_dfx(&["deploy", "mock_cmc"])?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let gov_id = canister_id("mock_nns_governance")?;
    let index_id = canister_id("mock_icp_index")?;
    let cmc_id = canister_id("mock_cmc")?;
    let rescue = principal_of_identity()?;

    // recipients: use three stable principals
    let r1 = Principal::management_canister();
    let r2 = Principal::anonymous();
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
            blackhole_armed = opt true;

            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);    
        }},)"#,
        r1 = r1.to_text(),
        r2 = r2.to_text(),
        r3 = r3.to_text(),
    );

    run_dfx(&["deploy", "jupiter_disburser_dbg", "--argument", &args])?;

    // IMPORTANT: allow canister to update its own controllers by making self a controller.
    let disb_id = canister_id("jupiter_disburser_dbg")?;
    run_dfx(&[
        "canister",
        "update-settings",
        "jupiter_disburser_dbg",
        "--add-controller",
        disb_id.trim(),
    ])?;

    let faucet_staking_account = Account {
        owner: Principal::anonymous(),
        subaccount: Some([9u8; 32]),
    };
    let faucet_rescue = Principal::from_text(cmc_id.trim())?;
    let faucet_args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt vec {{ {staking_subaccount} }} }};
            payout_subaccount = null;
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            rescue_controller = principal "{faucet_rescue}";
            blackhole_armed = opt false;
            main_interval_seconds = opt (31536000:nat64);
            rescue_interval_seconds = opt (31536000:nat64);
            min_tx_e8s = opt (10000000:nat64);
        }},)"#,
        staking_owner = faucet_staking_account.owner.to_text(),
        staking_subaccount = faucet_staking_account
            .subaccount
            .unwrap()
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join("; "),
        faucet_rescue = faucet_rescue.to_text(),
    );

    run_dfx(&["deploy", "jupiter_faucet_dbg", "--argument", &faucet_args])?;

    let faucet_id = canister_id("jupiter_faucet_dbg")?;
    run_dfx(&[
        "canister",
        "update-settings",
        "jupiter_faucet_dbg",
        "--add-controller",
        faucet_id.trim(),
    ])?;

    Ok(())
}

fn cmd_teardown() -> Result<()> {
    let _ = run_dfx(&["stop"])?;
    Ok(())
}

fn get_canister_controllers(canister: &str) -> Result<BTreeSet<String>> {
    // Example output typically contains a line like:
    //   Controllers: <principal1> <principal2>
    // We parse that line and return a deterministic set of principal text values.
    let out = run_dfx(&["canister", "info", canister])?;

    for line in out.lines() {
        let l = line.trim();
        if l.to_ascii_lowercase().starts_with("controllers:") {
            let rest = l.splitn(2, ':').nth(1).unwrap_or("").trim();

            let mut set = BTreeSet::new();
            for raw in rest.split_whitespace() {
                // strip common punctuation that sometimes appears in output
                let tok = raw.trim_matches(|c: char| {
                    !(c.is_ascii_alphanumeric() || c == '-' )
                });
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

    bail!("could not find Controllers line in `dfx canister info {canister}` output");
}

fn assert_controllers_eq(canister: &str, actual: &BTreeSet<String>, expected: &BTreeSet<String>) -> Result<()> {
    if actual == expected {
        return Ok(());
    }
    bail!(
        "controllers mismatch for {canister}\n  expected: {:?}\n  actual:   {:?}",
        expected, actual
    );
}

fn run_dfx_disburser_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {

    // Shared time base for scenarios that need it.
    let now_secs = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()) as u64;

    // This is reused across multiple scenarios.
    let four_years = 4u64 * 365 * 86_400;

    // Resolve disburser principal once (it exists by now).
    let disb_principal =
        Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
    let staging_arg = format!(
        "(record {{ owner = principal \"{}\"; subaccount = null }}, 125:nat64)",
        disb_principal.to_text()
    );

    // Always start from a known governance age.
    run_scenario(outcomes, label("dfx", "disburser", "Setup: reset mocks + set aging_since"), || {
        let _: () = call_raw_noargs("mock_icrc_ledger", "debug_reset")?;
        let _: () = call_raw_noargs("mock_nns_governance", "debug_reset")?;
        let _: () = call_raw(
            "mock_nns_governance",
            "debug_set_aging_since",
            &format!("({}:nat64)", now_secs.saturating_sub(100)),
        )?;
        Ok(())
    });

    run_scenario(outcomes, label("dfx", "disburser", "In-flight skip is a true no-op"), || {
        let _: () = call_raw("mock_nns_governance", "debug_set_in_flight", "(true)")?;
        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &staging_arg)?;
        let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if !transfers.is_empty() {
            bail!("expected 0 transfers, got {}", transfers.len());
        }

        let calls: u64 = call_raw_noargs("mock_nns_governance", "debug_get_manage_calls")?;
        if calls != 1 {
            bail!("expected 1 manage_neuron call (best-effort ClaimOrRefresh), got {}", calls);
        }
        Ok(())
    });

    run_scenario(outcomes, label("dfx", "disburser", "Happy path: bonus split (3 transfers, 99/19/4 net)"), || {
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

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 3 {
            bail!("expected 3 transfers, got {}", transfers.len());
        }

        let mut amts: Vec<u64> = transfers
            .iter()
            .map(|t| t.amount.0.to_u64().unwrap_or(0))
            .collect();
        amts.sort_unstable();

        if amts != vec![4, 19, 99] {
            bail!("unexpected transfer amounts: {:?}", amts);
        }

        let calls: u64 = call_raw_noargs("mock_nns_governance", "debug_get_manage_calls")?;
        if calls != 4 {
            bail!("expected 4 manage_neuron calls, got {}", calls);
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "disburser", "Retry: TemporarilyUnavailable preserves plan and later succeeds"), || {
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

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if !transfers.is_empty() {
            bail!("expected 0 transfers on first attempt, got {}", transfers.len());
        }

        // retry
        let _: () = call_raw("mock_icrc_ledger", "debug_set_next_error", "(null)")?;
        let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_main_tick")?;

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 3 {
            bail!("expected 3 transfers after retry, got {}", transfers.len());
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "disburser", "BadFee: clears plan then rebuilds with new fee"), || {
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
        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 3 {
            bail!("expected 3 transfers after rebuild, got {}", transfers.len());
        }

        Ok(())
    });

    run_scenario(
        outcomes,
        label("dfx", "disburser", "Rescue controllers invariants (broken→rescue+self, healthy→self-only)"),
        || {
            // Determine expected principals from reality (not mocks).
            let self_id = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
            let rescue = principal_of_identity()?; // same identity used to deploy/configure

            let self_txt = self_id.to_text();
            let rescue_txt = rescue.to_text();

            // 1) Force "broken" state.
            let old = now_secs.saturating_sub(30 * 86_400);
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_last_successful_transfer_ts",
                &format!("(opt ({}:nat64))", old),
            )?;

            // Run rescue tick: should set controllers to {rescue, self}
            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let actual = get_canister_controllers("jupiter_disburser_dbg")?;
            let expected_broken: BTreeSet<String> =
                [rescue_txt.clone(), self_txt.clone()].into_iter().collect();
            assert_controllers_eq("jupiter_disburser_dbg", &actual, &expected_broken)?;

            // 2) Recovery: mark as healthy, then rescue tick should re-blackhole to {self}.
            let _: () = call_raw(
                "jupiter_disburser_dbg",
                "debug_set_last_successful_transfer_ts",
                &opt_nat64_to_candid(now_secs),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let actual2 = get_canister_controllers("jupiter_disburser_dbg")?;
            let expected_healthy: BTreeSet<String> = [self_txt].into_iter().collect();
            assert_controllers_eq("jupiter_disburser_dbg", &actual2, &expected_healthy)?;

            Ok(())
        },
    );

    run_scenario(outcomes, label("dfx", "disburser", "Rescue healthy no-op (controllers remain self-only)"), || {
        let self_id = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
        let self_txt = self_id.to_text();

        // Ensure we are in healthy window and actively reconcile once into self-only.
        let _: () = call_raw(
            "jupiter_disburser_dbg",
            "debug_set_last_successful_transfer_ts",
            &opt_nat64_to_candid(now_secs),
        )?;
        let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

        let expected: BTreeSet<String> = [self_txt.clone()].into_iter().collect();
        let before = get_canister_controllers("jupiter_disburser_dbg")?;
        assert_controllers_eq("jupiter_disburser_dbg", &before, &expected)?;

        // Run rescue tick again; should remain unchanged.
        let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;
        let after = get_canister_controllers("jupiter_disburser_dbg")?;
        assert_controllers_eq("jupiter_disburser_dbg", &after, &expected)?;

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "disburser", "Rescue is not armed before first successful payout"), || {
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
    });

    run_scenario(outcomes, label("dfx", "disburser", "Plan persistence: present after failure, cleared after retry success"), || {
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
    
        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 3 {
            bail!("expected 3 transfers after retry, got {}", transfers.len());
        }
    
        Ok(())
    });

    run_scenario(outcomes, label("dfx", "disburser", "Dust stays in staging when below fee (no transfers, no plan)"), || {
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
    
        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
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
    });

    Ok(())
}

fn run_dfx_faucet_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    run_scenario(outcomes, label("dfx", "faucet", "same beneficiary contributions stay separate (no aggregation)"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 300000000:nat64)", account_to_candid(&accounts.staking)),
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

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;

        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected faucet summary")?;
        if summary.topped_up_count != 3 {
            bail!("expected three independent top-ups for the same beneficiary, got {}", summary.topped_up_count);
        }

        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        let beneficiary_notes = notes.iter().filter(|n| n.canister_id == target).count();
        if beneficiary_notes != 3 {
            bail!("expected three beneficiary notifications for the same canister, got {beneficiary_notes}");
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "every new payout job rescans full history from the beginning"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 300000000:nat64)", account_to_candid(&accounts.staking)),
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
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary after payout job")?;
            if summary.topped_up_count != 3 {
                bail!("expected replayed history to produce three top-ups per run, got {}", summary.topped_up_count);
            }
        }

        let calls: Vec<IndexGetCall> = call_raw_noargs("mock_icp_index", "debug_get_calls")?;
        let starts: Vec<Option<u64>> = calls
            .iter()
            .filter(|c| c.account_identifier == staking_id)
            .map(|c| c.start)
            .collect();
        if starts != vec![None, None] {
            bail!(
                "expected both payout jobs to start scanning from the beginning, got starts {starts:?} from calls {calls:?}"
            );
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "page-boundary scan skips bad/small entries and still finds late eligible tx"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let good_memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 1500000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 120000000:nat64)", account_to_candid(&accounts.payout)),
        )?;

        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_repeated_transfer",
            &format!("(\"{}\", 499:nat64, 1000000:nat64, {})", staking_id, good_memo),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 200000000:nat64, opt vec {{ 98; 97; 100 }})", staking_id),
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
        let summary = summary.context("expected faucet summary after advancing across page boundaries")?;
        if summary.topped_up_count != 1 || summary.ignored_bad_memo != 1 || summary.ignored_under_threshold != 999 {
            bail!(
                "unexpected page-boundary summary: topped_up_count={} ignored_bad_memo={} ignored_under_threshold={} state={state:?}",
                summary.topped_up_count,
                summary.ignored_bad_memo,
                summary.ignored_under_threshold
            );
        }

        let calls: Vec<IndexGetCall> = call_raw_noargs("mock_icp_index", "debug_get_calls")?;
        if calls.len() < 3 {
            bail!("expected multi-page history scan, got {} index calls: {calls:?}", calls.len());
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "notify retry persists minimal retry state and does not duplicate transfer"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
        )?;

        let _: () = call_raw("mock_cmc", "debug_set_fail", "(true)")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;

        let st: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if !st.active_payout_job_present || !st.retry_state_present {
            bail!("expected active payout job with retry state after CMC retry path");
        }

        let transfers_before: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers_before.len() != 1 {
            bail!("expected only one beneficiary transfer while notify is retrying, got {}", transfers_before.len());
        }

        let _: () = call_raw("mock_cmc", "debug_set_fail", "(false)")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_release_retry_backoff")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;

        let transfers_after: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers_after.len() != 1 {
            bail!("expected retry success to avoid duplicate beneficiary transfers, got {} total transfers", transfers_after.len());
        }

        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        let beneficiary_notes = notes.iter().filter(|n| n.canister_id == target).count();
        if beneficiary_notes != 1 {
            bail!("expected exactly one successful beneficiary notification after recovery, got {beneficiary_notes}");
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "multiple beneficiaries are processed independently"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let beneficiary_a = Principal::from_text(canister_id("mock_cmc")?.trim())?;
        let beneficiary_b = Principal::anonymous();
        let memo_a = opt_blob_to_candid(Some(beneficiary_a.to_text().as_bytes()));
        let memo_b = opt_blob_to_candid(Some(beneficiary_b.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 200000000:nat64)", account_to_candid(&accounts.staking)),
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

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected faucet summary")?;
        if summary.topped_up_count != 2 {
            bail!("expected two independent beneficiary top-ups, got {}", summary.topped_up_count);
        }

        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        let count_a = notes.iter().filter(|n| n.canister_id == beneficiary_a).count();
        let count_b = notes.iter().filter(|n| n.canister_id == beneficiary_b).count();
        if count_a != 1 || count_b != 1 {
            bail!("expected one notification per beneficiary, got count_a={} count_b={}", count_a, count_b);
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "empty history returns payout remainder to self"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let faucet_id = Principal::from_text(canister_id("jupiter_faucet_dbg")?.trim())?;

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 100000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected faucet summary after empty-history payout")?;
        if summary.topped_up_count != 0 || summary.remainder_to_self_e8s != 79_990_000 {
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
    });

    run_scenario(outcomes, label("dfx", "faucet", "zero payout pot produces no work"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let st: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if st.active_payout_job_present || st.retry_state_present || st.last_summary_present {
            bail!("expected zero payout pot to avoid creating any payout job or summary");
        }
        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        if !transfers.is_empty() || !notes.is_empty() {
            bail!("expected zero payout pot to produce no transfers or notifications");
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "payout pot at or below fee produces no work"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
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
        if st.active_payout_job_present || st.retry_state_present || st.last_summary_present {
            bail!("expected payout pot <= fee to avoid creating any payout job or summary");
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "tiny computed shares are skipped and payout falls back to self"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 100000000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 10000000:nat64, {})", staking_id, opt_blob_to_candid(Some(Principal::anonymous().to_text().as_bytes()))),
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected faucet summary")?;
        if summary.topped_up_count != 0 || summary.ignored_under_threshold != 0 || summary.ignored_bad_memo != 0 {
            bail!(
                "expected tiny computed share to be treated as no-transfer, got topped_up_count={} ignored_under_threshold={} ignored_bad_memo={}",
                summary.topped_up_count,
                summary.ignored_under_threshold,
                summary.ignored_bad_memo
            );
        }
        if summary.remainder_to_self_e8s != 79_990_000 {
            bail!("expected fallback remainder to self of 79_990_000 e8s, got {}", summary.remainder_to_self_e8s);
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "temporary pre-transfer ledger failure gets one deferred retry without blocking the job"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_set_next_error",
            "(opt variant { TemporarilyUnavailable })",
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if !st1.active_payout_job_present || !st1.retry_state_present {
            bail!("expected temporary pre-transfer failure to schedule a single deferred retry without blocking the job");
        }
        let transfers_before: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if !transfers_before.is_empty() {
            bail!("expected no ledger transfers while temporary failure is injected before transfer");
        }

        let _: () = call_raw("mock_icrc_ledger", "debug_set_next_error", "(null)")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_release_retry_backoff")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let st2: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if st2.active_payout_job_present || st2.retry_state_present {
            bail!("expected resumable payout job to clear after successful retry");
        }
        let transfers_after: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers_after.len() != 1 {
            bail!("expected exactly one beneficiary transfer after recovery, got {}", transfers_after.len());
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "duplicate ledger result reuses prior block index and still notifies"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_set_next_error",
            "(opt variant { Duplicate = record { duplicate_of = 77:nat64 } })",
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if !transfers.is_empty() {
            bail!("expected injected duplicate path not to create a fresh ledger transfer in the mock ledger");
        }
        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        if notes.len() != 1 || notes[0].block_index != 77 {
            bail!("expected duplicate result to drive notify_top_up with duplicate_of block index 77, got {notes:?}");
        }
        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected faucet summary after duplicate-handling path")?;
        if summary.topped_up_count != 1 {
            bail!("expected duplicate ledger path to count as one completed top-up, got {}", summary.topped_up_count);
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "CMC Processing response is retried without duplicate transfer"), || {
        let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
        let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
        let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

        let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
        let staking_id = account_identifier_text(&accounts.staking);
        let target = Principal::anonymous();
        let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
        )?;
        let _: () = call_raw(
            "mock_icrc_ledger",
            "debug_credit",
            &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
        )?;
        let _: u64 = call_raw(
            "mock_icp_index",
            "debug_append_transfer",
            &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
        )?;
        let _: () = call_raw(
            "mock_cmc",
            "debug_set_script",
            "(vec { variant { Processing }; variant { Ok } })",
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if !st1.active_payout_job_present || !st1.retry_state_present {
            bail!("expected Processing response to persist retry state for retry");
        }
        let transfers_before: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers_before.len() != 1 {
            bail!("expected only one beneficiary transfer before Processing retry, got {}", transfers_before.len());
        }

        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_release_retry_backoff")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
        let st2: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if st2.active_payout_job_present || st2.retry_state_present {
            bail!("expected Processing retry to clear once notify succeeds");
        }
        let transfers_after: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers_after.len() != 1 {
            bail!("expected Processing retry to avoid duplicate ledger transfer, got {}", transfers_after.len());
        }
        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        if notes.len() != 1 || notes[0].canister_id != target {
            bail!("expected one eventual beneficiary notification after Processing retry, got {notes:?}");
        }

        Ok(())
    });


    run_scenario(outcomes, label("dfx", "faucet", "terminal CMC responses are retried safely without duplicate transfer"), || {
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
            let staking_id = account_identifier_text(&accounts.staking);
            let target = Principal::anonymous();
            let memo = opt_blob_to_candid(Some(target.to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
            )?;
            let _: () = call_raw("mock_cmc", "debug_set_script", script)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if !st1.active_payout_job_present || !st1.retry_state_present {
                bail!("expected {label} response to persist retry state for safe retry");
            }
            let transfers_before: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_before.len() != 1 {
                bail!("expected {label} path to perform exactly one beneficiary transfer before retry, got {}", transfers_before.len());
            }
            let notes_before: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if !notes_before.is_empty() {
                bail!("expected no completed notifications before {label} retry succeeds");
            }

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_release_retry_backoff")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st2: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st2.active_payout_job_present || st2.retry_state_present {
                bail!("expected {label} retry to clear once a later notify succeeds");
            }
            let transfers_after: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_after.len() != 1 {
                bail!("expected {label} retry path to avoid duplicate ledger transfer, got {}", transfers_after.len());
            }
            let notes_after: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes_after.len() != 1 || notes_after[0].canister_id != target {
                bail!("expected {label} retry path to end with one completed beneficiary notification, got {notes_after:?}");
            }
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "deterministic pre-transfer ledger errors are skipped without blocking the job"), || {
        for (label, err_arg) in [
            ("TooOld", "(opt variant { TooOld })"),
            ("CreatedInFuture", "(opt variant { CreatedInFuture = record { ledger_time = 123:nat64 } })"),
            ("BadFee", "(opt variant { BadFee = record { expected_fee_e8s = 20000:nat64 } })"),
        ] {
            let _: () = call_raw("mock_icrc_ledger", "debug_reset", "()")?;
            let _: () = call_raw("mock_icp_index", "debug_reset", "()")?;
            let _: () = call_raw("mock_cmc", "debug_reset", "()")?;
            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;

            let accounts: FaucetDebugAccounts = call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")?;
            let staking_id = account_identifier_text(&accounts.staking);
            let memo = opt_blob_to_candid(Some(Principal::anonymous().to_text().as_bytes()));

            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 80000000:nat64)", account_to_candid(&accounts.staking)),
            )?;
            let _: () = call_raw(
                "mock_icrc_ledger",
                "debug_credit",
                &format!("({}, 80000000:nat64)", account_to_candid(&accounts.payout)),
            )?;
            let _: u64 = call_raw(
                "mock_icp_index",
                "debug_append_transfer",
                &format!("(\"{}\", 80000000:nat64, {})", staking_id, memo),
            )?;
            let _: () = call_raw("mock_icrc_ledger", "debug_set_next_error", err_arg)?;

            let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_main_tick")?;
            let st1: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
            if st1.active_payout_job_present || st1.retry_state_present {
                bail!("expected {label} ledger rejection to be skipped immediately without leaving retry state behind");
            }
            let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
            let summary = summary.context("expected faucet summary after deterministic ledger rejection")?;
            if summary.failed_topups != 1 || summary.topped_up_count != 0 {
                bail!("expected {label} path to count exactly one failed top-up and zero successful beneficiary top-ups, got failed_topups={} topped_up_count={}", summary.failed_topups, summary.topped_up_count);
            }
            if summary.remainder_to_self_e8s != 79_990_000 {
                bail!("expected {label} path to leave the failed beneficiary share in the faucet and send the full remainder to self, got {}", summary.remainder_to_self_e8s);
            }
            let transfers_after: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
            if transfers_after.len() != 1 {
                bail!("expected {label} path to produce only the fallback remainder transfer, got {} transfers", transfers_after.len());
            }
            let notes_after: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
            if notes_after.len() != 1 || notes_after[0].canister_id != accounts.payout.owner {
                bail!("expected {label} path to finish with exactly one self notification for the fallback remainder, got {notes_after:?}");
            }
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "rescue: before first successful top-up it stays on current controllers"), || {
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_reset_runtime_state")?;
        let _: () = call_raw("jupiter_faucet_dbg", "debug_set_blackhole_armed", "(opt true)")?;
        let _: () = call_raw("jupiter_faucet_dbg", "debug_set_last_successful_transfer_ts", "(null)")?;

        let before = get_canister_controllers("jupiter_faucet_dbg")?;
        let _: () = call_raw_noargs::<()>("jupiter_faucet_dbg", "debug_rescue_tick")?;
        let after = get_canister_controllers("jupiter_faucet_dbg")?;
        if before != after {
            bail!("expected rescue to remain inactive before any successful top-up, before={before:?} after={after:?}");
        }

        Ok(())
    });

    run_scenario(outcomes, label("dfx", "faucet", "rescue: broken path adds rescue controller and healthy path recovers to self-only"), || {
        let faucet_id = Principal::from_text(canister_id("jupiter_faucet_dbg")?.trim())?;
        let rescue = Principal::from_text(canister_id("mock_cmc")?.trim())?;
        let expected_broken: BTreeSet<String> = [faucet_id.to_text(), rescue.to_text()].into_iter().collect();
        let expected_healthy: BTreeSet<String> = [faucet_id.to_text()].into_iter().collect();

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

fn run_dfx_scenarios(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    run_dfx_disburser_scenarios(outcomes)?;
    run_dfx_faucet_scenarios(outcomes)?;
    Ok(())
}


fn finish_outcomes(outcomes: Vec<ScenarioOutcome>, failure_message: &str, success_message: &str) -> Result<()> {
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
        &["test", "-p", "jupiter-disburser", "--lib", "--", "--color", "always"],
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
        &["test", "-p", "jupiter-faucet", "--lib", "--", "--color", "always"],
        &root,
        &[],
    )
}

fn run_pocketic_disburser_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = [("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")];
    run_cargo_test_suite(
        outcomes,
        "pocketic",
        "disburser",
        "cargo",
        &["test", "-p", "jupiter-disburser", "--test", "jupiter_disburser_integration", "--", "--ignored", "--color", "always"],
        &root,
        &common_env,
    )
}

fn run_pocketic_faucet_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = [("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")];
    run_cargo_test_suite(
        outcomes,
        "pocketic",
        "faucet",
        "cargo",
        &["test", "-p", "jupiter-faucet", "--test", "jupiter_faucet_integration", "--", "--ignored", "--color", "always"],
        &root,
        &common_env,
    )
}

fn run_e2e_suite(outcomes: &mut Vec<ScenarioOutcome>) -> Result<()> {
    let root = repo_root();
    let common_env = [("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")];
    run_cargo_test_suite(
        outcomes,
        "e2e",
        "",
        "cargo",
        &["test", "-p", "jupiter-faucet", "--test", "e2e", "--", "--ignored", "--color", "always"],
        &root,
        &common_env,
    )
}

fn run_unit_component(outcomes: &mut Vec<ScenarioOutcome>, component: TestComponent) -> Result<()> {
    match component {
        TestComponent::Test => {
            run_unit_disburser_suite(outcomes)?;
            run_unit_faucet_suite(outcomes)?;
        }
        TestComponent::Disburser => run_unit_disburser_suite(outcomes)?,
        TestComponent::Faucet => run_unit_faucet_suite(outcomes)?,
        TestComponent::E2e => bail!("e2e_unit is not supported; use e2e_all"),
    }
    Ok(())
}

fn run_dfx_component(outcomes: &mut Vec<ScenarioOutcome>, component: TestComponent) -> Result<()> {
    match component {
        TestComponent::Test => run_dfx_scenarios(outcomes)?,
        TestComponent::Disburser => run_dfx_disburser_scenarios(outcomes)?,
        TestComponent::Faucet => run_dfx_faucet_scenarios(outcomes)?,
        TestComponent::E2e => bail!("e2e_dfx_integration is not supported; use e2e_all"),
    }
    Ok(())
}

fn run_pocketic_component(outcomes: &mut Vec<ScenarioOutcome>, component: TestComponent) -> Result<()> {
    match component {
        TestComponent::Test => {
            run_pocketic_disburser_suite(outcomes)?;
            run_pocketic_faucet_suite(outcomes)?;
            run_e2e_suite(outcomes)?;
        }
        TestComponent::Disburser => run_pocketic_disburser_suite(outcomes)?,
        TestComponent::Faucet => run_pocketic_faucet_suite(outcomes)?,
        TestComponent::E2e => run_e2e_suite(outcomes)?,
    }
    Ok(())
}

fn scoped_command_needs_dfx_env(component: TestComponent, scope: TestScope) -> bool {
    match scope {
        TestScope::DfxIntegration => true,
        TestScope::All => component != TestComponent::E2e,
        TestScope::Unit | TestScope::PocketicIntegration => false,
    }
}

fn run_scoped_command(component: TestComponent, scope: TestScope) -> Result<()> {
    let mut outcomes: Vec<ScenarioOutcome> = Vec::new();

    match scope {
        TestScope::Unit => run_unit_component(&mut outcomes, component)?,
        TestScope::DfxIntegration => run_dfx_component(&mut outcomes, component)?,
        TestScope::PocketicIntegration => run_pocketic_component(&mut outcomes, component)?,
        TestScope::All => match component {
            TestComponent::Test => {
                run_dfx_component(&mut outcomes, component)?;
                run_unit_component(&mut outcomes, component)?;
                run_pocketic_component(&mut outcomes, component)?;
            }
            TestComponent::Disburser | TestComponent::Faucet => {
                run_unit_component(&mut outcomes, component)?;
                run_dfx_component(&mut outcomes, component)?;
                run_pocketic_component(&mut outcomes, component)?;
            }
            TestComponent::E2e => run_e2e_suite(&mut outcomes)?,
        },
    }

    let failure_message = match (component, scope) {
        (TestComponent::Test, TestScope::Unit) => "one or more unit test suites failed",
        (TestComponent::Test, TestScope::DfxIntegration) => "one or more dfx integration scenario suites failed",
        (TestComponent::Test, TestScope::PocketicIntegration) => "one or more pocketic integration or e2e suites failed",
        (TestComponent::Test, TestScope::All) => "one or more tests failed across dfx, unit, pocketic, or e2e layers",
        (TestComponent::Disburser, TestScope::Unit) => "the disburser unit test suite failed",
        (TestComponent::Disburser, TestScope::DfxIntegration) => "one or more disburser dfx integration scenarios failed",
        (TestComponent::Disburser, TestScope::PocketicIntegration) => "the disburser pocketic integration suite failed",
        (TestComponent::Disburser, TestScope::All) => "one or more disburser test suites failed",
        (TestComponent::Faucet, TestScope::Unit) => "the faucet unit test suite failed",
        (TestComponent::Faucet, TestScope::DfxIntegration) => "one or more faucet dfx integration scenarios failed",
        (TestComponent::Faucet, TestScope::PocketicIntegration) => "the faucet pocketic integration suite failed",
        (TestComponent::Faucet, TestScope::All) => "one or more faucet test suites failed",
        (TestComponent::E2e, TestScope::PocketicIntegration) | (TestComponent::E2e, TestScope::All) => "the e2e suite failed",
        _ => "the selected xtask command failed",
    };

    let success_message = match (component, scope) {
        (TestComponent::Test, TestScope::Unit) => "test_unit complete",
        (TestComponent::Test, TestScope::DfxIntegration) => "test_dfx_integration complete",
        (TestComponent::Test, TestScope::PocketicIntegration) => "test_pocketic_integration complete",
        (TestComponent::Test, TestScope::All) => "test_all complete",
        (TestComponent::Disburser, TestScope::Unit) => "disburser_unit complete",
        (TestComponent::Disburser, TestScope::DfxIntegration) => "disburser_dfx_integration complete",
        (TestComponent::Disburser, TestScope::PocketicIntegration) => "disburser_pocketic_integration complete",
        (TestComponent::Disburser, TestScope::All) => "disburser_all complete",
        (TestComponent::Faucet, TestScope::Unit) => "faucet_unit complete",
        (TestComponent::Faucet, TestScope::DfxIntegration) => "faucet_dfx_integration complete",
        (TestComponent::Faucet, TestScope::PocketicIntegration) => "faucet_pocketic_integration complete",
        (TestComponent::Faucet, TestScope::All) => "faucet_all complete",
        (TestComponent::E2e, TestScope::PocketicIntegration) => "e2e_pocketic_integration complete",
        (TestComponent::E2e, TestScope::All) => "e2e_all complete",
        _ => "xtask command complete",
    };

    finish_outcomes(outcomes, failure_message, success_message)
}

fn cmd_scoped(component: TestComponent, scope: TestScope) -> Result<()> {
    if !scoped_command_needs_dfx_env(component, scope) {
        return run_scoped_command(component, scope);
    }

    cmd_setup()?;
    let run_res = run_scoped_command(component, scope);
    let teardown_res = cmd_teardown();

    match (run_res, teardown_res) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(run_err), Ok(())) => Err(run_err),
        (Ok(()), Err(teardown_err)) => Err(teardown_err),
        (Err(run_err), Err(teardown_err)) => {
            eprintln!("⚠️ teardown also failed after scoped dfx run: {teardown_err:#}");
            Err(run_err)
        }
    }
}


fn truncate_error(msg: &str, max_chars: usize) -> String {
    let flat = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        flat
    } else {
        let mut out = flat.chars().take(max_chars).collect::<String>();
        out.push_str("...");
        out
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '' {
            if matches!(chars.peek(), Some('[')) {
                let _ = chars.next();
                while let Some(c) = chars.next() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn should_live_print_rust_test_line(line: &str) -> bool {
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim();
    trimmed.starts_with("running ")
        || trimmed.starts_with("test ")
        || trimmed == "failures:"
        || trimmed.starts_with("test result:")
        || trimmed.starts_with("error[")
        || trimmed.starts_with("error:")
        || trimmed.starts_with("warning:")
}

fn parse_failed_rust_test_details(output: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = output.lines().collect();
    let mut blocks: BTreeMap<String, String> = BTreeMap::new();
    let mut i = 0usize;

    while i < lines.len() {
        let stripped = strip_ansi(lines[i]);
        let trimmed = stripped.trim();
        if let Some(name) = trimmed
            .strip_prefix("---- ")
            .and_then(|s| s.strip_suffix(" stdout ----"))
            .or_else(|| trimmed.strip_prefix("---- ").and_then(|s| s.strip_suffix(" stderr ----")))
        {
            let test_name = name.to_string();
            i += 1;
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() {
                let inner_stripped = strip_ansi(lines[i]);
                let inner = inner_stripped.trim();
                let starts_next = inner.starts_with("---- ")
                    && (inner.ends_with(" stdout ----") || inner.ends_with(" stderr ----"));
                if starts_next || inner == "failures:" || inner.starts_with("test result:") {
                    break;
                }
                body.push(strip_ansi(lines[i]));
                i += 1;
            }
            let body_str = body.join("\n").trim().to_string();
            blocks.entry(test_name).or_insert(body_str);
            continue;
        }
        i += 1;
    }

    let mut ordered_names: BTreeSet<String> = BTreeSet::new();
    for line in output.lines() {
        let stripped = strip_ansi(line);
        let trimmed = stripped.trim();
        if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some(name) = rest.strip_suffix(" ... FAILED") {
                ordered_names.insert(name.to_string());
            }
        }
    }
    for name in blocks.keys() {
        ordered_names.insert(name.clone());
    }

    ordered_names
        .into_iter()
        .map(|name| {
            let detail = blocks
                .get(&name)
                .cloned()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "see cargo output above".to_string());
            (name, detail)
        })
        .collect()
}

fn suite_scope_label(layer: &str, component: &str) -> String {
    if component.is_empty() {
        format!("[{layer}]")
    } else {
        format!("[{layer}/{component}]")
    }
}

fn run_cargo_test_suite(
    outcomes: &mut Vec<ScenarioOutcome>,
    suite_label: &str,
    component: &str,
    cmd: &str,
    args: &[&str],
    workdir: &str,
    envs: &[(&str, &str)],
) -> Result<()> {
    let scope = suite_scope_label(suite_label, component);
    let full_label = format!("{scope} {} {}", cmd, args.join(" "));
    eprintln!("\n{BOLD}=== {full_label} ==={RESET}");
    let t0 = Instant::now();

    let mut c = Command::new(cmd);
    c.args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }

    let mut child = c.spawn().with_context(|| format!("failed to spawn {cmd}"))?;
    let stdout = child.stdout.take().context("failed to capture child stdout")?;
    let stderr = child.stderr.take().context("failed to capture child stderr")?;

    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let tx_out = tx.clone();
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = tx_out.send((false, line));
                }
                Err(_) => break,
            }
        }
    });
    let tx_err = tx.clone();
    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = tx_err.send((true, line));
                }
                Err(_) => break,
            }
        }
    });
    drop(tx);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    for (is_err, line) in rx {
        if is_err {
            stderr_buf.push_str(&line);
            stderr_buf.push('\n');
        } else {
            stdout_buf.push_str(&line);
            stdout_buf.push('\n');
        }
        if should_live_print_rust_test_line(&line) {
            eprintln!("{line}");
        }
    }

    let status = child.wait().with_context(|| format!("failed waiting for {cmd}"))?;
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let ms = t0.elapsed().as_millis();
    if status.success() {
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} suite passed"),
            ms,
            passed: true,
            error: None,
        });
        eprintln!("{GREEN}✓{RESET} {scope} suite passed {DIM}({ms}ms){RESET}");
        return Ok(());
    }

    let combined = format!("{}\n{}", stdout_buf, stderr_buf);
    let failed_tests = parse_failed_rust_test_details(&combined);
    if failed_tests.is_empty() {
        outcomes.push(ScenarioOutcome {
            name: format!("{scope} test command failed"),
            ms,
            passed: false,
            error: Some(strip_ansi(combined.trim())),
        });
    } else {
        for (test_name, detail) in failed_tests {
            let short = truncate_error(&strip_ansi(&detail), 140);
            eprintln!("{RED}↳{RESET} {scope} {test_name}: {DIM}{short}{RESET}");
            outcomes.push(ScenarioOutcome {
                name: format!("{scope} {test_name}"),
                ms,
                passed: false,
                error: Some(detail),
            });
        }
    }
    eprintln!("{RED}✗{RESET} {scope} suite failed {DIM}({ms}ms){RESET}");
    Ok(())
}



fn main() -> Result<()> {
    let cmd = env::args().nth(1).unwrap_or_else(|| "help".to_string());
    if let Some((component, scope)) = parse_scoped_command(&cmd) {
        return cmd_scoped(component, scope);
    }

    match cmd.as_str() {
        "setup" => cmd_setup(),
        "teardown" => cmd_teardown(),
        _ => {
            eprintln!(
                "Usage: cargo run -p xtask -- <command>

                 Utility commands:
                 - setup
                 - teardown

                 Scoped commands:
                 - disburser_unit
                 - disburser_dfx_integration
                 - disburser_pocketic_integration
                 - disburser_all
                 - faucet_unit
                 - faucet_dfx_integration
                 - faucet_pocketic_integration
                 - faucet_all
                 - e2e_all
                 - e2e_pocketic_integration
                 - test_unit
                 - test_dfx_integration
                 - test_pocketic_integration
                 - test_all
"
            );
            Ok(())
        }
    }
}
