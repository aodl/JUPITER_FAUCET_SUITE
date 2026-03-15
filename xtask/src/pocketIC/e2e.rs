use anyhow::{anyhow, bail, Context, Result};
use candid::{decode_one, encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{PocketIc, PocketIcBuilder};
use sha2::{Digest, Sha224};
use std::process::Command;
use std::time::Duration;

fn require_ignored_flag() -> Result<()> { Ok(()) }
fn repo_root() -> &'static str { env!("CARGO_MANIFEST_DIR") }

fn build_wasm(package: &str, features: Option<&str>) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--target", "wasm32-unknown-unknown", "--release", "-p", package, "--locked"])
        .current_dir(format!("{}/..", repo_root()));
    if let Some(f) = features {
        cmd.args(["--features", f]);
    }
    let status = cmd.status().with_context(|| format!("failed to build {package}"))?;
    if !status.success() {
        bail!("cargo build failed for {package}");
    }
    let raw_name = package.replace('-', "_");
    let path = format!("{}/../target/wasm32-unknown-unknown/release/{raw_name}.wasm", repo_root());
    std::fs::read(path).with_context(|| format!("failed to read wasm for {package}"))
}

fn tick_n(pic: &PocketIc, n: usize) {
    for _ in 0..n {
        pic.tick();
    }
}

fn update_bytes<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    bytes: Vec<u8>,
) -> Result<R> {
    let reply = pic
        .update_call(canister, sender, method, bytes)
        .map_err(|e| anyhow!("update_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

fn update_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    update_bytes(pic, canister, sender, method, encode_one(arg)?)
}

fn update_noargs<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
) -> Result<R> {
    update_one(pic, canister, sender, method, ())
}

fn query_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    let bytes = encode_one(arg)?;
    let reply = pic
        .query_call(canister, sender, method, bytes)
        .map_err(|e| anyhow!("query_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct FaucetInitArg {
    staking_account: Account,
    payout_subaccount: Option<Vec<u8>>,
    ledger_canister_id: Option<Principal>,
    index_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    rescue_controller: Principal,
    blackhole_armed: Option<bool>,
    expected_first_staking_tx_id: Option<u64>,
    main_interval_seconds: Option<u64>,
    rescue_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DisburserInitArg {
    neuron_id: u64,
    normal_recipient: Account,
    age_bonus_recipient_1: Account,
    age_bonus_recipient_2: Account,
    ledger_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    rescue_controller: Principal,
    blackhole_armed: Option<bool>,
    main_interval_seconds: Option<u64>,
    rescue_interval_seconds: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugAccounts {
    payout: Account,
    staking: Account,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum ForcedRescueReason {
    BootstrapNoSuccess,
    IndexAnchorMissing,
    IndexLatestInvariantBroken,
    CmcZeroSuccessRuns,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    active_payout_job_present: bool,
    retry_state_present: bool,
    last_summary_present: bool,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    blackhole_armed_since_ts: Option<u64>,
    forced_rescue_reason: Option<ForcedRescueReason>,
    consecutive_index_anchor_failures: u8,
    consecutive_index_latest_invariant_failures: u8,
    consecutive_cmc_zero_success_runs: u8,
    last_observed_staking_balance_e8s: Option<u64>,
    last_observed_latest_tx_id: Option<u64>,
    expected_first_staking_tx_id: Option<u64>,
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

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

fn nat_to_u64(n: &Nat) -> Result<u64> {
    u64::try_from(n.0.clone()).map_err(|_| anyhow!("Nat does not fit into u64: {n}"))
}

fn icrc1_balance(pic: &PocketIc, ledger: Principal, acct: &Account) -> Result<u64> {
    let n: Nat = query_one(pic, ledger, Principal::anonymous(), "icrc1_balance_of", acct.clone())?;
    nat_to_u64(&n)
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

#[test]
#[ignore]
fn suite_disburser_pays_faucet_and_faucet_tops_up_target() -> Result<()> {
    require_ignored_flag()?;
    let pic = PocketIcBuilder::new().with_application_subnet().build();
    let ledger_wasm = build_wasm("mock-icrc-ledger", None)?;
    let gov_wasm = build_wasm("mock-nns-governance", None)?;
    let index_wasm = build_wasm("mock-icp-index", None)?;
    let cmc_wasm = build_wasm("mock-cmc", None)?;
    let faucet_wasm = build_wasm("jupiter-faucet", Some("debug_api"))?;
    let disburser_wasm = build_wasm("jupiter-disburser", Some("debug_api"))?;

    let ledger = pic.create_canister();
    let gov = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let faucet = pic.create_canister();
    let disburser = pic.create_canister();

    for c in [ledger, gov, index, cmc, faucet, disburser] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(gov, gov_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);

    let staking_account = Account {
        owner: Principal::anonymous(),
        subaccount: Some([7u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: faucet,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let accounts: DebugAccounts = query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = Principal::from_text("aaaaa-aa")?;
    let staking_id = account_identifier_text(&staking_account);
    let denom_e8s = 250_000_000u64;
    let pot_e8s = 100_000_000u64;

    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), denom_e8s))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id, denom_e8s, Some(target.to_text().into_bytes())))?,
    )?;
    let disburser_staging = Account { owner: disburser, subaccount: None };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, pot_e8s))?,
    )?;

    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let faucet_balance = icrc1_balance(&pic, ledger, &accounts.payout)?;
    if faucet_balance == 0 {
        bail!("expected disburser to transfer ICP into faucet payout account");
    }

    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let summary: Option<FaucetSummary> = query_one(&pic, faucet, Principal::anonymous(), "debug_last_summary", ())?;
    let summary = summary.ok_or_else(|| anyhow!("expected faucet summary after successful tick"))?;
    if summary.topped_up_count != 1 {
        bail!("expected one beneficiary top-up, got {}", summary.topped_up_count);
    }
    if summary.ignored_bad_memo != 0 || summary.ignored_under_threshold != 0 {
        bail!(
            "expected no ignored contributions, got bad_memo={} under_threshold={}",
            summary.ignored_bad_memo,
            summary.ignored_under_threshold
        );
    }

    let notes: Vec<NotifyRecord> = query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if !notes.iter().any(|n| n.canister_id == target) {
        bail!("expected mock CMC to record a top-up notification for target canister {target}");
    }

    Ok(())
}

#[test]
#[ignore]
fn suite_repeated_disburser_payouts_make_faucet_replay_full_history() -> Result<()> {
    require_ignored_flag()?;
    let pic = PocketIcBuilder::new().with_application_subnet().build();
    let ledger_wasm = build_wasm("mock-icrc-ledger", None)?;
    let gov_wasm = build_wasm("mock-nns-governance", None)?;
    let index_wasm = build_wasm("mock-icp-index", None)?;
    let cmc_wasm = build_wasm("mock-cmc", None)?;
    let faucet_wasm = build_wasm("jupiter-faucet", Some("debug_api"))?;
    let disburser_wasm = build_wasm("jupiter-disburser", Some("debug_api"))?;

    let ledger = pic.create_canister();
    let gov = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let faucet = pic.create_canister();
    let disburser = pic.create_canister();

    for c in [ledger, gov, index, cmc, faucet, disburser] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(gov, gov_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);

    let staking_account = Account {
        owner: Principal::anonymous(),
        subaccount: Some([8u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: faucet,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let accounts: DebugAccounts = query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account {
            owner: Principal::management_canister(),
            subaccount: None,
        },
        age_bonus_recipient_2: Account {
            owner: disburser,
            subaccount: None,
        },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = Principal::from_text("aaaaa-aa")?;
    let staking_id = account_identifier_text(&staking_account);
    let denom_e8s = 300_000_000u64;

    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), denom_e8s))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_repeated_transfer",
        encode_args((staking_id, 3u64, 100_000_000u64, Some(target.to_text().into_bytes())))?,
    )?;

    for (i, pot_e8s) in [90_000_000u64, 60_000_000u64].into_iter().enumerate() {
        // Advance PocketIC time between payout cycles so the disburser and faucet
        // use distinct created_at_time values and the mock ledger does not dedup
        // the second cycle as a duplicate replay of the first transfer set.
        if i > 0 {
            pic.advance_time(Duration::from_secs(1));
            tick_n(&pic, 1);
        }

        let disburser_staging = Account { owner: disburser, subaccount: None };
        let _: () = update_bytes(
            &pic,
            ledger,
            Principal::anonymous(),
            "debug_credit",
            encode_args((disburser_staging, pot_e8s))?,
        )?;
        let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 10);
        let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 10);

        // In this suite-level replay scenario we want two distinct completed disburser
        // payout cycles. The mock governance marks a disbursement as in-flight after a
        // successful initiation and keeps it there until explicitly cleared, which would
        // otherwise cause the next disburser tick to skip its payout stage entirely.
        let _: () = update_one(&pic, gov, Principal::anonymous(), "debug_set_in_flight", false)?;

        let summary: Option<FaucetSummary> = query_one(&pic, faucet, Principal::anonymous(), "debug_last_summary", ())?;
        let summary = summary.ok_or_else(|| anyhow!("expected faucet summary after payout cycle"))?;
        if summary.topped_up_count != 3 {
            bail!("expected each payout cycle to replay the same three contributions, got {}", summary.topped_up_count);
        }
    }

    let notes: Vec<NotifyRecord> = query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    let beneficiary_notes = notes.iter().filter(|n| n.canister_id == target).count();
    if beneficiary_notes != 6 {
        bail!("expected repeated suite payouts to re-notify the same beneficiary three times per cycle, got {beneficiary_notes}");
    }

    Ok(())
}


#[test]
#[ignore]
fn suite_retry_path_across_disburser_faucet_and_cmc_boundary_avoids_duplicate_transfer() -> Result<()> {
    require_ignored_flag()?;
    let pic = PocketIcBuilder::new().with_application_subnet().build();
    let ledger_wasm = build_wasm("mock-icrc-ledger", None)?;
    let gov_wasm = build_wasm("mock-nns-governance", None)?;
    let index_wasm = build_wasm("mock-icp-index", None)?;
    let cmc_wasm = build_wasm("mock-cmc", None)?;
    let faucet_wasm = build_wasm("jupiter-faucet", Some("debug_api"))?;
    let disburser_wasm = build_wasm("jupiter-disburser", Some("debug_api"))?;

    let ledger = pic.create_canister();
    let gov = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let faucet = pic.create_canister();
    let disburser = pic.create_canister();

    for c in [ledger, gov, index, cmc, faucet, disburser] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(gov, gov_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);

    let staking_account = Account {
        owner: Principal::anonymous(),
        subaccount: Some([6u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: faucet,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let accounts: DebugAccounts = query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = Principal::from_text("aaaaa-aa")?;
    let staking_id = account_identifier_text(&staking_account);
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), 80_000_000u64))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id, 80_000_000u64, Some(target.to_text().into_bytes())))?,
    )?;
    let disburser_staging = Account { owner: disburser, subaccount: None };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, 80_000_000u64))?,
    )?;

    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let _: () = update_one(&pic, cmc, Principal::anonymous(), "debug_set_fail", true)?;
    let transfers_before_faucet: Vec<TransferRecord> = query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st1: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if !st1.active_payout_job_present || !st1.retry_state_present {
        bail!("expected faucet to persist a retry state when CMC fails in suite retry path");
    }
    let transfers_after_first: Vec<TransferRecord> = query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after_first.len() != transfers_before_faucet.len().saturating_add(1) {
        bail!(
            "expected exactly one additional faucet beneficiary transfer before retry; before={} after={}",
            transfers_before_faucet.len(),
            transfers_after_first.len()
        );
    }

    let _: () = update_one(&pic, cmc, Principal::anonymous(), "debug_set_fail", false)?;
    pic.advance_time(Duration::from_secs(61));
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st2: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if st2.active_payout_job_present || st2.retry_state_present {
        bail!("expected suite retry path to clear retry state after recovery");
    }
    let transfers_after_retry: Vec<TransferRecord> = query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after_retry.len() != transfers_after_first.len() {
        bail!(
            "expected suite retry path not to duplicate faucet beneficiary transfer; first={} retry={}",
            transfers_after_first.len(),
            transfers_after_retry.len()
        );
    }
    let notes: Vec<NotifyRecord> = query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected one eventual target notification after suite retry recovery, got {notes:?}");
    }

    Ok(())
}


#[test]
#[ignore]
fn suite_upgrade_faucet_mid_retry_state_preserves_recovery() -> Result<()> {
    require_ignored_flag()?;
    let pic = PocketIcBuilder::new().with_application_subnet().build();
    let ledger_wasm = build_wasm("mock-icrc-ledger", None)?;
    let gov_wasm = build_wasm("mock-nns-governance", None)?;
    let index_wasm = build_wasm("mock-icp-index", None)?;
    let cmc_wasm = build_wasm("mock-cmc", None)?;
    let faucet_wasm = build_wasm("jupiter-faucet", Some("debug_api"))?;
    let disburser_wasm = build_wasm("jupiter-disburser", Some("debug_api"))?;

    let ledger = pic.create_canister();
    let gov = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let faucet = pic.create_canister();
    let disburser = pic.create_canister();

    for c in [ledger, gov, index, cmc, faucet, disburser] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(gov, gov_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);

    let staking_account = Account {
        owner: Principal::anonymous(),
        subaccount: Some([5u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: faucet,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
    };
    pic.install_canister(faucet, faucet_wasm.clone(), encode_one(faucet_init)?, None);

    let accounts: DebugAccounts = query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser,
        blackhole_armed: Some(false),
        expected_first_staking_tx_id: None,
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = Principal::from_text("aaaaa-aa")?;
    let staking_id = account_identifier_text(&staking_account);
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), 80_000_000u64))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id, 80_000_000u64, Some(target.to_text().into_bytes())))?,
    )?;
    let disburser_staging = Account { owner: disburser, subaccount: None };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, 80_000_000u64))?,
    )?;

    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let _: () = update_one(&pic, cmc, Principal::anonymous(), "debug_set_fail", true)?;
    let transfers_before_faucet: Vec<TransferRecord> = query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st1: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if !st1.active_payout_job_present || !st1.retry_state_present {
        bail!("expected retry state before faucet upgrade in suite path");
    }
    let transfers_after_first: Vec<TransferRecord> = query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after_first.len() != transfers_before_faucet.len().saturating_add(1) {
        bail!("expected exactly one faucet beneficiary transfer before upgrade");
    }

    let faucet_upgrade_sender = pic.get_controllers(faucet).first().copied().unwrap_or(faucet);
    pic.upgrade_canister(faucet, faucet_wasm, encode_one(())?, Some(faucet_upgrade_sender))
        .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;
    tick_n(&pic, 10);

    let st2: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if !st2.active_payout_job_present || !st2.retry_state_present {
        bail!("expected retry state to survive faucet upgrade in suite path");
    }

    let _: () = update_one(&pic, cmc, Principal::anonymous(), "debug_set_fail", false)?;
    pic.advance_time(Duration::from_secs(61));
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st3: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if st3.active_payout_job_present || st3.retry_state_present {
        bail!("expected upgraded faucet to clear retry state after recovery");
    }
    let transfers_after_retry: Vec<TransferRecord> = query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after_retry.len() != transfers_after_first.len() {
        bail!("expected suite upgrade retry path to avoid duplicate beneficiary transfer");
    }
    let notes: Vec<NotifyRecord> = query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected exactly one beneficiary notification after post-upgrade retry, got {notes:?}");
    }

    Ok(())
}
