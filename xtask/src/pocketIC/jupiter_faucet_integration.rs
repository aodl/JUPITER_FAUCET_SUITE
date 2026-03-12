use anyhow::{anyhow, bail, Context, Result};
use candid::{decode_one, encode_args, encode_one, CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{PocketIc, PocketIcBuilder};
use sha2::{Digest, Sha224};
use std::process::Command;

fn require_ignored_flag() -> Result<()> {
    Ok(())
}

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn build_wasm(package: &str, features: Option<&str>) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        "wasm32-unknown-unknown",
        "--release",
        "-p",
        package,
        "--locked",
    ])
    .current_dir(format!("{}/..", repo_root()));
    if let Some(f) = features {
        cmd.args(["--features", f]);
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to build {package}"))?;
    if !status.success() {
        bail!("cargo build failed for {package}");
    }
    let raw_name = package.replace('-', "_");
    let path = format!(
        "{}/../target/wasm32-unknown-unknown/release/{raw_name}.wasm",
        repo_root()
    );
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
    main_interval_seconds: Option<u64>,
    rescue_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugAccounts {
    payout: Account,
    staking: Account,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    active_payout_job_present: bool,
    pending_notification_present: bool,
    last_summary_present: bool,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
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
fn faucet_retries_persisted_notification_after_cmc_failure() -> Result<()> {
    require_ignored_flag()?;

    let pic = PocketIcBuilder::new().with_application_subnet().build();

    let ledger_wasm = build_wasm("mock-icrc-ledger", None)?;
    let index_wasm = build_wasm("mock-icp-index", None)?;
    let cmc_wasm = build_wasm("mock-cmc", None)?;
    let faucet_wasm = build_wasm("jupiter-faucet", Some("debug_api"))?;

    let ledger = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let faucet = pic.create_canister();

    for c in [ledger, index, cmc, faucet] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);

    let staking_account = Account {
        owner: Principal::anonymous(),
        subaccount: Some([9u8; 32]),
    };

    let init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: faucet,
        blackhole_armed: Some(false),
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(60),
        min_tx_e8s: Some(10_000_000),
    };

    pic.install_canister(faucet, faucet_wasm, encode_one(init)?, None);

    let accounts: DebugAccounts =
        query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;

    let target = Principal::from_text("aaaaa-aa")?;

    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((accounts.payout.clone(), 80_000_000u64))?,
    )?;

    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), 80_000_000u64))?,
    )?;

    let staking_id = account_identifier_text(&staking_account);

    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id, 80_000_000u64, Some(target.to_text().into_bytes())))?,
    )?;

    let _: () = update_one(&pic, cmc, Principal::anonymous(), "debug_set_fail", true)?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st1: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if !st1.active_payout_job_present || !st1.pending_notification_present {
        bail!("expected an active payout job with a persisted pending notification after CMC failure");
    }

    let notes_before: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if !notes_before.is_empty() {
        bail!("expected no notifications while mock CMC is failing");
    }

    let _: () = update_one(&pic, cmc, Principal::anonymous(), "debug_set_fail", false)?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st2: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if st2.active_payout_job_present || st2.pending_notification_present {
        bail!("expected active payout job to clear after retry success");
    }

    let summary: Option<FaucetSummary> =
        query_one(&pic, faucet, Principal::anonymous(), "debug_last_summary", ())?;
    if summary.is_none() {
        bail!("expected summary after retry success");
    }

    let notes_after: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes_after.len() != 1 {
        bail!(
            "expected exactly one CMC notification after retry, got {}",
            notes_after.len()
        );
    }

    Ok(())
}
