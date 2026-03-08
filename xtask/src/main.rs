use anyhow::{bail, Context, Result};
use candid::{decode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use num_traits::ToPrimitive;
use std::env;
use std::collections::BTreeSet;
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

fn run_dfx(args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("dfx");

    // Always use a dedicated, non-interactive identity
    cmd.args(["--identity", DFX_IDENTITY]);
    cmd.args(args);

    eprintln!(
        "▶ dfx {}",
        cmd.get_args()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .context("failed to spawn dfx")?;

    if !output.status.success() {
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
    // start replica    
    {
        let mut start = Command::new("dfx");
        start.args(["start", "--background", "--clean"]);
        let status = start.status().context("dfx start failed")?;
        if !status.success() {
            bail!("dfx start failed");
        }
    }

    run_dfx(&["deploy", "mock_icrc_ledger"])?;
    run_dfx(&["deploy", "mock_nns_governance"])?;

    let ledger_id = canister_id("mock_icrc_ledger")?;
    let gov_id = canister_id("mock_nns_governance")?;
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

            main_interval_seconds = opt (60:nat64);
            rescue_interval_seconds = opt (60:nat64);    
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

fn cmd_test() -> Result<()> {
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

fn cmd_test_all() -> Result<()> {
    cmd_test_all_impl(true)
}

fn cmd_test_all_fast() -> Result<()> {
    // Same as test-all, but skips PocketIC E2E (useful for quick iterations).
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

    if include_pocketic {
        // PocketIC E2Es are marked #[ignore] so they don't run by default.
        // Keep them quiet by default (no --nocapture) and mute replica logs.
        eprintln!("\n{BOLD}=== PocketIC E2E: cargo test -p jupiter-disburser --test pocketic_e2e -- --ignored ==={RESET}");
        run_host_in_dir_env(
            "cargo",
            &["test", "-p", "jupiter-disburser", "--test", "pocketic_e2e", "--", "--ignored"],
            &root,
            &[("POCKET_IC_MUTE_SERVER", "1"), ("RUST_TEST_THREADS", "1")],
        )?;
    }

    eprintln!("{GREEN}{BOLD}✅ test-all complete{RESET}\n");
    Ok(())
}

fn cmd_setup_test_all_teardown() -> Result<()> {
    cmd_setup()?;

    let test_res = cmd_test_all(); // includes PocketIC E2E
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
    // Fast path: integration scenarios + unit tests (skips PocketIC E2E)
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
		"test" => cmd_test(), // integration scenarios only
		"test-all" => cmd_test_all(), // integration + unit + PocketIC E2E
		"test-all-fast" => cmd_test_all_fast(), // integration + unit (no PocketIC)
		"setup_test_teardown" => cmd_setup_test_teardown(),
		"setup_test_all_teardown" => cmd_setup_test_all_teardown(),
		_ => {
				eprintln!(
					"Usage: cargo run -p xtask -- <command>\n\n\
					 Commands:\n\
					 - setup\n\
					 - test                  (integration scenarios)\n\
					 - test-all              (integration + unit + PocketIC E2E)\n\
					 - test-all-fast         (integration + unit; skips PocketIC)\n\
					 - teardown\n\
					 - setup_test_teardown        (setup + test-all-fast + teardown)\n\
					 - setup_test_all_teardown    (setup + test-all + teardown)\n"
				);
			Ok(())
		}
	}
}



