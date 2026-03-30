use anyhow::{bail, Context, Result};
use candid::{decode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use num_traits::ToPrimitive;
use sha2::{Digest, Sha224};
use std::collections::BTreeSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::process::{Command, Stdio};
use std::time::Instant;

const DFX_IDENTITY: &str = "xtask-dev";

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";

#[derive(Debug)]
struct ScenarioOutcome {
    name: &'static str,
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

fn run_scenario<F>(outcomes: &mut Vec<ScenarioOutcome>, name: &'static str, f: F)
where
    F: FnOnce() -> anyhow::Result<()>,
{
    eprintln!("\n{BOLD}=== Scenario: {name} ==={RESET}");
    let t0 = Instant::now();

    match f() {
        Ok(()) => {
            let ms = t0.elapsed().as_millis();
            outcomes.push(ScenarioOutcome {
                name,
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
                name,
                ms,
                passed: false,
                error: Some(msg.clone()),
            });
            eprintln!("{RED}✗{RESET} {name} {DIM}({ms}ms){RESET}");
            eprintln!("{DIM}  {msg}{RESET}");
        }
    }
}

fn print_summary(outcomes: &[ScenarioOutcome]) -> bool {
    let passed = outcomes.iter().filter(|o| o.passed).count();
    let failed = outcomes.len().saturating_sub(passed);

    if failed == 0 {
        eprintln!(
            "\n{GREEN}{BOLD}✅ xtask:test PASSED{RESET} {DIM}({} scenarios){RESET}",
            outcomes.len()
        );
    } else {
        eprintln!(
            "\n{RED}{BOLD}❌ xtask:test FAILED{RESET} {DIM}({} scenarios; {passed} passed, {failed} failed){RESET}",
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

fn run_host_in_dir(cmd: &str, args: &[&str], workdir: &str) -> Result<()> {
    eprintln!(
        "▶ {} {} {}",
        cmd,
        args.iter().copied().collect::<Vec<_>>().join(" "),
        format!("{}(cwd={}){}", DIM, workdir, RESET)
    );

    let status = Command::new(cmd)
        .args(args)
        .current_dir(workdir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to spawn {cmd}"))?;

    if !status.success() {
        bail!("{cmd} {:?} failed with status {:?}", args, status);
    }
    Ok(())
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
struct FaucetDebugState {
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    active_payout_job_present: bool,
    last_summary_present: bool,
}

#[derive(Debug, Clone, CandidType, Deserialize)]
struct FaucetDebugAccounts {
    payout: Account,
    staking: Account,
}

#[derive(Debug, Clone, CandidType, Deserialize)]
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

#[derive(Debug, Clone, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
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

fn deploy_disburser_dbg() -> Result<()> {
    let ledger_id = canister_id("mock_icrc_ledger")?;
    let gov_id = canister_id("mock_nns_governance")?;
    let rescue = principal_of_identity()?;

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

            main_interval_seconds = opt (60:nat64);
            rescue_interval_seconds = opt (60:nat64);
        }},)"#,
        r1 = r1.to_text(),
        r2 = r2.to_text(),
        r3 = r3.to_text(),
    );

    run_dfx(&["deploy", "jupiter_disburser_dbg", "--argument", &args])?;

    let disb_id = canister_id("jupiter_disburser_dbg")?;
    run_dfx(&[
        "canister",
        "update-settings",
        "jupiter_disburser_dbg",
        "--add-controller",
        disb_id.trim(),
    ])?;

    Ok(())
}

fn deploy_faucet_dbg(mode: Option<&str>) -> Result<()> {
    let ledger_id = canister_id("mock_icrc_ledger")?;
    let index_id = canister_id("mock_icp_index")?;
    let cmc_id = canister_id("mock_cmc")?;
    let rescue = principal_of_identity()?;

    let args = format!(
        r#"(record {{
            staking_account = record {{ owner = principal "{staking_owner}"; subaccount = opt vec {{ {staking_sub} }} }};
            payout_subaccount = null;
            ledger_canister_id = opt principal "{ledger_id}";
            index_canister_id = opt principal "{index_id}";
            cmc_canister_id = opt principal "{cmc_id}";
            rescue_controller = principal "{rescue}";
            blackhole_armed = opt true;
            main_interval_seconds = opt (60:nat64);
            rescue_interval_seconds = opt (60:nat64);
            min_tx_e8s = opt (10000000:nat64);
        }},)"#,
        staking_owner = Principal::anonymous().to_text(),
        staking_sub = (0u8..32).map(|_| "7:nat8").collect::<Vec<_>>().join("; "),
        ledger_id = ledger_id.trim(),
        index_id = index_id.trim(),
        cmc_id = cmc_id.trim(),
        rescue = rescue.to_text(),
    );

    match mode {
        Some(mode) => run_dfx(&["deploy", "jupiter_faucet_dbg", "--mode", mode, "--argument", &args])?,
        None => run_dfx(&["deploy", "jupiter_faucet_dbg", "--argument", &args])?,
    };

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

fn reset_faucet_fixture() -> Result<FaucetDebugAccounts> {
    let _: () = call_raw_noargs("mock_icrc_ledger", "debug_reset")?;
    let _: () = call_raw_noargs("mock_icp_index", "debug_reset")?;
    let _: () = call_raw_noargs("mock_cmc", "debug_reset")?;
    deploy_faucet_dbg(Some("reinstall"))?;
    call_raw_noargs("jupiter_faucet_dbg", "debug_accounts")
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

    let _ = run_dfx(&["stop"]);
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

    deploy_faucet_dbg(None)?;
    deploy_disburser_dbg()?;

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

fn cmd_test_disburser_integration() -> Result<()> {
    let mut outcomes: Vec<ScenarioOutcome> = Vec::new();

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
    run_scenario(&mut outcomes, "Setup: reset mocks + set aging_since", || {
        let _: () = call_raw_noargs("mock_icrc_ledger", "debug_reset")?;
        let _: () = call_raw_noargs("mock_nns_governance", "debug_reset")?;
        let _: () = call_raw(
            "mock_nns_governance",
            "debug_set_aging_since",
            &format!("({}:nat64)", now_secs.saturating_sub(100)),
        )?;
        Ok(())
    });

    run_scenario(&mut outcomes, "In-flight skip is a true no-op", || {
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

    run_scenario(&mut outcomes, "Happy path: bonus split (3 transfers, 99/19/4 net)", || {
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

    run_scenario(&mut outcomes, "Retry: TemporarilyUnavailable preserves plan and later succeeds", || {
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

    run_scenario(&mut outcomes, "BadFee: clears plan then rebuilds with new fee", || {
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
        &mut outcomes,
        "Rescue controllers invariants (broken→rescue+self, healthy→self-only)",
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
                &format!("(opt ({}:nat64))", now_secs),
            )?;

            let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

            let actual2 = get_canister_controllers("jupiter_disburser_dbg")?;
            let expected_healthy: BTreeSet<String> = [self_txt].into_iter().collect();
            assert_controllers_eq("jupiter_disburser_dbg", &actual2, &expected_healthy)?;

            Ok(())
        },
    );

    run_scenario(&mut outcomes, "Rescue healthy no-op (controllers remain self-only)", || {
        let self_id = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
        let self_txt = self_id.to_text();
    
        // Ensure we are in healthy window.
        let _: () = call_raw(
            "jupiter_disburser_dbg",
            "debug_set_last_successful_transfer_ts",
            &format!("(opt ({}:nat64))", now_secs),
        )?;
    
        // Controllers should already be self-only from previous rescue scenario, but assert it.
        let expected: BTreeSet<String> = [self_txt.clone()].into_iter().collect();
        let before = get_canister_controllers("jupiter_disburser_dbg")?;
        assert_controllers_eq("jupiter_disburser_dbg", &before, &expected)?;
    
        // Run rescue tick again; should remain unchanged.
        let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;
        let after = get_canister_controllers("jupiter_disburser_dbg")?;
        assert_controllers_eq("jupiter_disburser_dbg", &after, &expected)?;
    
        Ok(())
    });

    run_scenario(&mut outcomes, "Rescue is not armed before first successful payout", || {
        let self_id = Principal::from_text(canister_id("jupiter_disburser_dbg")?.trim())?;
        let rescue = principal_of_identity()?;

        let expected: BTreeSet<String> = [self_id.to_text()].into_iter().collect();
        let before = get_canister_controllers("jupiter_disburser_dbg")?;
        assert_controllers_eq("jupiter_disburser_dbg", &before, &expected)?;

        let _: () = call_raw(
            "jupiter_disburser_dbg",
            "debug_set_last_successful_transfer_ts",
            "(null)",
        )?;

        let _: () = call_raw_noargs::<()>("jupiter_disburser_dbg", "debug_rescue_tick")?;

        let after = get_canister_controllers("jupiter_disburser_dbg")?;
        assert_controllers_eq("jupiter_disburser_dbg", &after, &expected)?;

        if after.contains(&rescue.to_text()) {
            bail!("rescue controller should not be added before any successful payout is recorded");
        }

        Ok(())
    });

    run_scenario(&mut outcomes, "Plan persistence: present after failure, cleared after retry success", || {
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

    run_scenario(&mut outcomes, "Dust stays in staging when below fee (no transfers, no plan)", || {
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

    // Print final summary and decide overall result.
    let ok = print_summary(&outcomes);
    if ok {
        Ok(())
    } else {
        bail!("one or more integration scenarios failed")
    }
}

fn cmd_test_faucet_integration() -> Result<()> {
    let mut outcomes: Vec<ScenarioOutcome> = Vec::new();

    run_scenario(&mut outcomes, "Faucet happy path: one eligible contribution top-ups beneficiary and returns remainder", || {
        let accounts = reset_faucet_fixture()?;
        let target = Principal::from_text(canister_id("mock_icp_index")?.trim())?;
        let payout_credit = 100_000_000u64;
        let denom = 400_000_000u64;
        let contribution = 100_000_000u64;
        let staking_id = account_identifier_text(&accounts.staking);

        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = null }}, {}:nat64)", accounts.payout.owner.to_text(), payout_credit))?;
        let staking_sub = accounts.staking.subaccount.expect("staking subaccount should be configured");
        let sub_vec = staking_sub.iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");
        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = opt vec {{ {} }} }}, {}:nat64)", accounts.staking.owner.to_text(), sub_vec, denom))?;
        let memo_vec = target.as_slice().iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");
        let _: u64 = call_raw("mock_icp_index", "debug_append_transfer", &format!("(\"{}\", {}:nat64, opt vec {{ {} }})", staking_id, contribution, memo_vec))?;

        let _: () = call_raw_noargs("jupiter_faucet_dbg", "debug_main_tick")?;

        let st: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if st.active_payout_job_present || !st.last_summary_present {
            bail!("expected completed payout job and persisted summary after happy path");
        }

        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected faucet summary")?;
        if summary.topped_up_count != 1 || summary.ignored_under_threshold != 0 || summary.ignored_bad_memo != 0 {
            bail!("unexpected faucet summary counts: topped_up_count={}, ignored_under_threshold={}, ignored_bad_memo={}", summary.topped_up_count, summary.ignored_under_threshold, summary.ignored_bad_memo);
        }
        if summary.topped_up_sum_e8s != 24_990_000 || summary.remainder_to_self_e8s != 74_990_000 || summary.pot_remaining_e8s != 0 {
            bail!("unexpected faucet summary amounts: topped_up_sum_e8s={}, remainder_to_self_e8s={}, pot_remaining_e8s={}", summary.topped_up_sum_e8s, summary.remainder_to_self_e8s, summary.pot_remaining_e8s);
        }

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 2 {
            bail!("expected 2 ledger transfers (beneficiary + remainder), got {}", transfers.len());
        }

        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        if notes.len() != 2 {
            bail!("expected 2 CMC notifications, got {}", notes.len());
        }

        Ok(())
    });

    run_scenario(&mut outcomes, "Faucet replays full history on every new payout job", || {
        let accounts = reset_faucet_fixture()?;
        let target = Principal::from_text(canister_id("mock_icp_index")?.trim())?;
        let staking_id = account_identifier_text(&accounts.staking);
        let memo_vec = target.as_slice().iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");
        let staking_sub = accounts.staking.subaccount.expect("staking subaccount should be configured");
        let sub_vec = staking_sub.iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");

        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = opt vec {{ {} }} }}, 100000000:nat64)", accounts.staking.owner.to_text(), sub_vec))?;
        let _: u64 = call_raw("mock_icp_index", "debug_append_transfer", &format!("(\"{}\", 100000000:nat64, opt vec {{ {} }})", staking_id, memo_vec))?;

        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = null }}, 40000000:nat64)", accounts.payout.owner.to_text()))?;
        let _: () = call_raw_noargs("jupiter_faucet_dbg", "debug_main_tick")?;
        let first_summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let first_summary = first_summary.context("expected first faucet summary")?;
        if first_summary.topped_up_count != 1 || first_summary.topped_up_sum_e8s != 39_990_000 {
            bail!("unexpected first summary: topped_up_count={}, topped_up_sum_e8s={}", first_summary.topped_up_count, first_summary.topped_up_sum_e8s);
        }

        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = null }}, 60000000:nat64)", accounts.payout.owner.to_text()))?;
        let _: () = call_raw_noargs("jupiter_faucet_dbg", "debug_main_tick")?;
        let second_summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let second_summary = second_summary.context("expected second faucet summary")?;
        if second_summary.topped_up_count != 1 || second_summary.topped_up_sum_e8s != 59_990_000 {
            bail!("expected historical contribution to be revisited on second run; got topped_up_count={}, topped_up_sum_e8s={}", second_summary.topped_up_count, second_summary.topped_up_sum_e8s);
        }

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 2 {
            bail!("expected exactly 2 beneficiary transfers across two full-history runs, got {}", transfers.len());
        }
        let notes: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        if notes.len() != 2 {
            bail!("expected exactly 2 CMC notifications across two full-history runs, got {}", notes.len());
        }
        Ok(())
    });

    run_scenario(&mut outcomes, "Faucet scans past the first index page and still processes later eligible contributions", || {
        let accounts = reset_faucet_fixture()?;
        let target = Principal::from_text(canister_id("mock_icp_index")?.trim())?;
        let staking_id = account_identifier_text(&accounts.staking);
        let memo_good = target.as_slice().iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");
        let memo_bad = b"bad-memo".iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");
        let staking_sub = accounts.staking.subaccount.expect("staking subaccount should be configured");
        let sub_vec = staking_sub.iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");

        let total_denom = 638_000_000u64;
        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = null }}, 63800000:nat64)", accounts.payout.owner.to_text()))?;
        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = opt vec {{ {} }} }}, {}:nat64)", accounts.staking.owner.to_text(), sub_vec, total_denom))?;
        let _: u64 = call_raw("mock_icp_index", "debug_append_repeated_transfer", &format!("(\"{}\", 498:nat64, 1000000:nat64, opt vec {{ {} }})", staking_id, memo_good))?;
        let _: u64 = call_raw("mock_icp_index", "debug_append_repeated_transfer", &format!("(\"{}\", 2:nat64, 20000000:nat64, opt vec {{ {} }})", staking_id, memo_bad))?;
        let _: u64 = call_raw("mock_icp_index", "debug_append_transfer", &format!("(\"{}\", 100000000:nat64, opt vec {{ {} }})", staking_id, memo_good))?;

        let _: () = call_raw_noargs("jupiter_faucet_dbg", "debug_main_tick")?;
        let summary: Option<FaucetSummary> = call_raw_noargs("jupiter_faucet_dbg", "debug_last_summary")?;
        let summary = summary.context("expected summary after paginated scan")?;
        if summary.ignored_under_threshold != 498 || summary.ignored_bad_memo != 2 || summary.topped_up_count != 1 {
            bail!("expected paginated scan counts (498 under threshold, 2 bad memo, 1 top-up); got under_threshold={}, bad_memo={}, topped_up_count={}", summary.ignored_under_threshold, summary.ignored_bad_memo, summary.topped_up_count);
        }
        if summary.topped_up_sum_e8s != 9_990_000 || summary.remainder_to_self_e8s != 53_790_000 {
            bail!("unexpected paginated summary amounts: topped_up_sum_e8s={}, remainder_to_self_e8s={}", summary.topped_up_sum_e8s, summary.remainder_to_self_e8s);
        }

        let transfers: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers.len() != 2 {
            bail!("expected 2 ledger transfers after paginated scan, got {}", transfers.len());
        }
        Ok(())
    });

    run_scenario(&mut outcomes, "Faucet retries pending CMC notification without duplicating the ledger transfer", || {
        let accounts = reset_faucet_fixture()?;
        let target = Principal::from_text(canister_id("mock_icp_index")?.trim())?;
        let staking_id = account_identifier_text(&accounts.staking);
        let memo_vec = target.as_slice().iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");
        let staking_sub = accounts.staking.subaccount.expect("staking subaccount should be configured");
        let sub_vec = staking_sub.iter().map(|b| format!("{}:nat8", b)).collect::<Vec<_>>().join("; ");

        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = null }}, 80000000:nat64)", accounts.payout.owner.to_text()))?;
        let _: () = call_raw("mock_icrc_ledger", "debug_credit", &format!("(record {{ owner = principal \"{}\"; subaccount = opt vec {{ {} }} }}, 80000000:nat64)", accounts.staking.owner.to_text(), sub_vec))?;
        let _: u64 = call_raw("mock_icp_index", "debug_append_transfer", &format!("(\"{}\", 80000000:nat64, opt vec {{ {} }})", staking_id, memo_vec))?;
        let _: () = call_raw("mock_cmc", "debug_set_script", "(vec { variant { Processing }; variant { Ok } })")?;

        let _: () = call_raw_noargs("jupiter_faucet_dbg", "debug_main_tick")?;
        let st2: FaucetDebugState = call_raw_noargs("jupiter_faucet_dbg", "debug_state")?;
        if st2.active_payout_job_present || !st2.last_summary_present {
            bail!("expected inline notify retry to clear active job and persist summary");
        }
        let transfers2: Vec<TransferRecord> = call_raw_noargs("mock_icrc_ledger", "debug_transfers")?;
        if transfers2.len() != 2 {
            bail!("expected beneficiary + remainder transfers after inline retry, got {} total transfers", transfers2.len());
        }
        let notes2: Vec<NotifyRecord> = call_raw_noargs("mock_cmc", "debug_notifications")?;
        if notes2.len() != 1 {
            bail!("expected exactly one successful CMC notification after retry, got {}", notes2.len());
        }
        Ok(())
    });

    let ok = print_summary(&outcomes);
    if ok {
        Ok(())
    } else {
        bail!("one or more faucet integration scenarios failed")
    }
}

fn cmd_test() -> Result<()> {
    cmd_test_disburser_integration()?;
    cmd_test_faucet_integration()
}

fn cmd_test_all() -> Result<()> {
    cmd_test_all_impl(true)
}

fn cmd_test_all_fast() -> Result<()> {
    // Same as test-all, but skips PocketIC integration/e2e (useful for quick iterations).
    cmd_test_all_impl(false)
}

fn run_host_in_dir_env(
    cmd: &str,
    args: &[&str],
    workdir: &str,
    envs: &[(&str, &str)],
) -> Result<()> {
    eprintln!(
        "▶ {} {} {}",
        cmd,
        args.iter().copied().collect::<Vec<_>>().join(" "),
        format!("{}(cwd={}){}", DIM, workdir, RESET)
    );

    let mut c = Command::new(cmd);
    c.args(args)
        .current_dir(workdir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (k, v) in envs {
        c.env(k, v);
    }

    let status = c.status().with_context(|| format!("failed to spawn {cmd}"))?;
    if !status.success() {
        bail!("{cmd} {:?} failed with status {:?}", args, status);
    }
    Ok(())
}

fn cmd_test_all_impl(include_pocketic: bool) -> Result<()> {
    cmd_test()?;

    let root = repo_root();

    eprintln!("\n{BOLD}=== Unit tests: cargo test -p jupiter-disburser --lib ==={RESET}");
    run_host_in_dir("cargo", &["test", "-p", "jupiter-disburser", "--lib"], &root)?;

    eprintln!("\n{BOLD}=== Unit tests: cargo test -p jupiter-faucet --lib ==={RESET}");
    run_host_in_dir("cargo", &["test", "-p", "jupiter-faucet", "--lib"], &root)?;

    if include_pocketic {
        // PocketIC tests are marked #[ignore] so they don't run by default.
        // Keep them quiet by default (no --nocapture) and mute replica logs.
        eprintln!("\n{BOLD}=== PocketIC integration: cargo test -p jupiter-disburser --test jupiter_disburser_integration -- --ignored ==={RESET}");
        run_host_in_dir_env(
            "cargo",
            &["test", "-p", "jupiter-disburser", "--test", "jupiter_disburser_integration", "--", "--ignored"],
            &root,
            &[("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")],
        )?;

        eprintln!("\n{BOLD}=== PocketIC integration: cargo test -p jupiter-faucet --test jupiter_faucet_integration -- --ignored ==={RESET}");
        run_host_in_dir_env(
            "cargo",
            &["test", "-p", "jupiter-faucet", "--test", "jupiter_faucet_integration", "--", "--ignored"],
            &root,
            &[("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")],
        )?;

        eprintln!("\n{BOLD}=== PocketIC end-to-end: cargo test -p xtask --test e2e -- --ignored ==={RESET}");
        run_host_in_dir_env(
            "cargo",
            &["test", "-p", "xtask", "--test", "e2e", "--", "--ignored"],
            &root,
            &[("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")],
        )?;
    }

    eprintln!("{GREEN}{BOLD}✅ test-all complete{RESET}\n");
    Ok(())
}

fn cmd_setup_test_all_teardown() -> Result<()> {
    cmd_setup()?;

    let test_res = cmd_test_all(); // includes PocketIC integration + end-to-end
    let td_res = cmd_teardown();

    match (test_res, td_res) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(test_err), Ok(_)) => Err(test_err),
        (Ok(_), Err(td_err)) => Err(td_err),
        (Err(test_err), Err(td_err)) => {
            eprintln!("⚠️ teardown also failed after test error: {td_err:?}");
            Err(test_err)
        }
    }
}

fn cmd_setup_test_teardown() -> Result<()> {
    cmd_setup()?;
    // Fast path: dfx/mock integration scenarios + unit tests (skips PocketIC integration/e2e)
    let test = cmd_test_all_fast();
    let td = cmd_teardown();
    if let Err(e) = td {
        eprintln!("⚠️ teardown error: {e:?}");
    }
    test
}

fn main() -> Result<()> {
    let cmd = env::args().nth(1).unwrap_or_else(|| "help".to_string());
	match cmd.as_str() {
		"setup" => cmd_setup(),
		"teardown" => cmd_teardown(),
		"test-disburser" => cmd_test_disburser_integration(),
		"test-faucet" => cmd_test_faucet_integration(),
		"test" => cmd_test(), // dfx/mock integration scenarios only
		"test-all" => cmd_test_all(), // dfx/mock integration + unit + PocketIC integration/e2e
		"test-all-fast" => cmd_test_all_fast(), // dfx/mock integration + unit (no PocketIC)
		"setup_test_teardown" => cmd_setup_test_teardown(),
		"setup_test_all_teardown" => cmd_setup_test_all_teardown(),
		_ => {
				eprintln!(
					"Usage: cargo run -p xtask -- <command>\n\n\
					 Commands:\n\
					 - setup\n\
					 - test-disburser         (dfx/mock integration scenarios for jupiter-disburser)\n\
					 - test-faucet            (dfx/mock integration scenarios for jupiter-faucet)\n\
					 - test                   (all dfx/mock integration scenarios)\n\
					 - test-all               (dfx/mock integration + unit + PocketIC integration/e2e)\n\
					 - test-all-fast          (dfx/mock integration + unit; skips PocketIC)\n\
					 - teardown\n\
					 - setup_test_teardown        (setup + test-all-fast + teardown)\n\
					 - setup_test_all_teardown    (setup + test-all + teardown)\n"
				);
			Ok(())
		}
	}
}



