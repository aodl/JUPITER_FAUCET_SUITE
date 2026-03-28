use anyhow::{anyhow, bail, Context, Result};
use candid::{decode_one, encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{PocketIc, PocketIcBuilder};
use sha2::{Digest, Sha224};

#[path = "real_blackhole.rs"]
mod real_blackhole;
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

fn set_controllers_exact(pic: &PocketIc, canister: Principal, controllers: Vec<Principal>) -> Result<()> {
    let sender = pic
        .get_controllers(canister)
        .first()
        .copied()
        .unwrap_or(Principal::anonymous());
    pic.set_controllers(canister, Some(sender), controllers)
        .map_err(|e| anyhow!("set_controllers reject: {e:?}"))?;
    Ok(())
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
    blackhole_controller: Option<Principal>,
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
    blackhole_controller: Option<Principal>,
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

fn test_blackhole_controller() -> Principal {
    Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").unwrap()
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
        expected_first_staking_tx_id: None,
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
        expected_first_staking_tx_id: None,
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
        expected_first_staking_tx_id: None,
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
        expected_first_staking_tx_id: None,
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
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
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

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianInitArg {
    staking_account: Account,
    ledger_canister_id: Option<Principal>,
    index_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    faucet_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    sns_wasm_canister_id: Option<Principal>,
    enable_sns_tracking: Option<bool>,
    scan_interval_seconds: Option<u64>,
    cycles_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    max_cycles_entries_per_canister: Option<u32>,
    max_contribution_entries_per_canister: Option<u32>,
    max_index_pages_per_tick: Option<u32>,
    max_canisters_per_cycles_tick: Option<u32>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum CanisterSource {
    MemoContribution,
    SnsDiscovery,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListCanistersArgs {
    start_after: Option<Principal>,
    limit: Option<u32>,
    source_filter: Option<CanisterSource>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianCanisterListItem {
    canister_id: Principal,
    sources: Vec<CanisterSource>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListCanistersResponse {
    items: Vec<HistorianCanisterListItem>,
    next_start_after: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianGetContributionHistoryArgs {
    canister_id: Principal,
    start_after_tx_id: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianContributionSample {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianContributionHistoryPage {
    items: Vec<HistorianContributionSample>,
    next_start_after_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum HistorianCyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianCyclesSample {
    timestamp_nanos: u64,
    cycles: u128,
    source: HistorianCyclesSampleSource,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianCyclesHistoryPage {
    items: Vec<HistorianCyclesSample>,
    next_start_after_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianGetCyclesHistoryArgs {
    canister_id: Principal,
    start_after_ts: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianPublicCounts {
    registered_canister_count: u64,
    qualifying_contribution_count: u64,
    icp_burned_e8s: u64,
    sns_discovered_canister_count: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianPublicStatus {
    staking_account: Account,
    ledger_canister_id: Principal,
    last_index_run_ts: Option<u64>,
    index_interval_seconds: u64,
    last_completed_cycles_sweep_ts: Option<u64>,
    cycles_interval_seconds: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum HistorianRegisteredCanisterSummarySort {
    CanisterIdAsc,
    LastContributionDesc,
    QualifyingContributionCountDesc,
    TotalQualifyingContributedDesc,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct HistorianListRegisteredCanisterSummariesArgs {
    page: Option<u32>,
    page_size: Option<u32>,
    sort: Option<HistorianRegisteredCanisterSummarySort>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianRegisteredCanisterSummary {
    canister_id: Principal,
    sources: Vec<CanisterSource>,
    qualifying_contribution_count: u64,
    total_qualifying_contributed_e8s: u64,
    last_contribution_ts: Option<u64>,
    latest_cycles: Option<u128>,
    last_cycles_probe_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListRegisteredCanisterSummariesResponse {
    items: Vec<HistorianRegisteredCanisterSummary>,
    page: u32,
    page_size: u32,
    total: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct HistorianListRecentContributionsArgs {
    limit: Option<u32>,
    qualifying_only: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianRecentContributionListItem {
    canister_id: Option<Principal>,
    memo_text: Option<String>,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
    tx_hash: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListRecentContributionsResponse {
    items: Vec<HistorianRecentContributionListItem>,
}

#[test]
#[ignore]
fn suite_historian_tracks_same_staking_flow_as_faucet() -> Result<()> {
    require_ignored_flag()?;
    let pic = PocketIcBuilder::new().with_application_subnet().build();
    let ledger_wasm = build_wasm("mock-icrc-ledger", None)?;
    let gov_wasm = build_wasm("mock-nns-governance", None)?;
    let index_wasm = build_wasm("mock-icp-index", None)?;
    let cmc_wasm = build_wasm("mock-cmc", None)?;
    let blackhole_wasm = real_blackhole::real_blackhole_wasm()?;
    let faucet_wasm = build_wasm("jupiter-faucet", Some("debug_api"))?;
    let disburser_wasm = build_wasm("jupiter-disburser", Some("debug_api"))?;
    let historian_wasm = build_wasm("jupiter-historian", Some("debug_api"))?;

    let ledger = pic.create_canister();
    let gov = pic.create_canister();
    let index = pic.create_canister();
    let cmc = pic.create_canister();
    let blackhole = pic.create_canister();
    let faucet = pic.create_canister();
    let disburser = pic.create_canister();
    let historian = pic.create_canister();

    for c in [ledger, gov, index, cmc, blackhole, faucet, disburser, historian] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(gov, gov_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);
    pic.install_canister(blackhole, blackhole_wasm, vec![], None);
    set_controllers_exact(&pic, blackhole, vec![blackhole])?;

    let staking_account = Account { owner: Principal::anonymous(), subaccount: Some([11u8; 32]) };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: faucet,
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(10_000_000),
        expected_first_staking_tx_id: None,
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let historian_init = HistorianInitArg {
        staking_account: staking_account.clone(),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        faucet_canister_id: Some(faucet),
        blackhole_canister_id: Some(blackhole),
        sns_wasm_canister_id: None,
        enable_sns_tracking: Some(false),
        scan_interval_seconds: Some(60),
        cycles_interval_seconds: Some(1),
        min_tx_e8s: Some(10_000_000),
        max_cycles_entries_per_canister: Some(100),
        max_contribution_entries_per_canister: Some(100),
        max_index_pages_per_tick: Some(10),
        max_canisters_per_cycles_tick: Some(10),
    };
    pic.install_canister(historian, historian_wasm, encode_one(historian_init)?, None);

    let accounts: DebugAccounts = query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser,
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = blackhole;
    let staking_id = account_identifier_text(&staking_account);
    let _: () = update_bytes(&pic, ledger, Principal::anonymous(), "debug_credit", encode_args((staking_account.clone(), 80_000_000u64))?)?;
    let _: u64 = update_bytes(&pic, index, Principal::anonymous(), "debug_append_transfer", encode_args((staking_id, 80_000_000u64, Some(target.to_text().into_bytes())))?)?;

    let disburser_staging = Account { owner: disburser, subaccount: None };
    let _: () = update_bytes(&pic, ledger, Principal::anonymous(), "debug_credit", encode_args((disburser_staging, 80_000_000u64))?)?;

    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    pic.advance_time(Duration::from_secs(2));
    let _: () = update_noargs(&pic, historian, Principal::anonymous(), "debug_driver_tick")?;
    tick_n(&pic, 10);

    let listed: HistorianListCanistersResponse = query_one(&pic, historian, Principal::anonymous(), "list_canisters", HistorianListCanistersArgs { start_after: None, limit: Some(10), source_filter: None })?;
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].canister_id, target);

    let contributions: HistorianContributionHistoryPage = query_one(&pic, historian, Principal::anonymous(), "get_contribution_history", HistorianGetContributionHistoryArgs { canister_id: target, start_after_tx_id: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(contributions.items.len(), 1);
    assert!(contributions.items[0].counts_toward_faucet);

    let cycles: HistorianCyclesHistoryPage = query_one(&pic, historian, Principal::anonymous(), "get_cycles_history", HistorianGetCyclesHistoryArgs { canister_id: target, start_after_ts: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(cycles.items.len(), 1);
    assert!(cycles.items[0].cycles > 0);
    assert!(matches!(cycles.items[0].source, HistorianCyclesSampleSource::BlackholeStatus));

    let expected_burned_e8s = 0u64;

    let counts: HistorianPublicCounts = query_one(&pic, historian, Principal::anonymous(), "get_public_counts", ())?;
    assert_eq!(counts.registered_canister_count, 1);
    assert_eq!(counts.qualifying_contribution_count, 1);
    assert_eq!(counts.icp_burned_e8s, expected_burned_e8s);
    assert_eq!(counts.sns_discovered_canister_count, 0);

    let status: HistorianPublicStatus = query_one(&pic, historian, Principal::anonymous(), "get_public_status", ())?;
    assert_eq!(status.staking_account, staking_account);
    assert_eq!(status.ledger_canister_id, ledger);
    assert_eq!(status.index_interval_seconds, 60);
    assert_eq!(status.cycles_interval_seconds, 1);
    assert!(status.last_index_run_ts.is_some());
    assert!(status.last_completed_cycles_sweep_ts.is_some());

    let registered: HistorianListRegisteredCanisterSummariesResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_registered_canister_summaries",
        HistorianListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
            sort: Some(HistorianRegisteredCanisterSummarySort::TotalQualifyingContributedDesc),
        },
    )?;
    assert_eq!(registered.total, 1);
    assert_eq!(registered.items.len(), 1);
    assert_eq!(registered.items[0].canister_id, target);
    assert_eq!(registered.items[0].qualifying_contribution_count, 1);
    assert_eq!(registered.items[0].total_qualifying_contributed_e8s, 80_000_000);
    assert!(registered.items[0].last_contribution_ts.is_some());
    assert!(registered.items[0].latest_cycles.unwrap_or_default() > 0);
    assert!(registered.items[0].last_cycles_probe_ts.is_some());

    let recent: HistorianListRecentContributionsResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_recent_contributions",
        HistorianListRecentContributionsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, Some(target));
    assert_eq!(recent.items[0].tx_id, 1);
    assert_eq!(recent.items[0].amount_e8s, 80_000_000);
    assert!(recent.items[0].counts_toward_faucet);

    let notes: Vec<NotifyRecord> = query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected faucet notify_top_up for target, got {notes:?}");
    }

    Ok(())
}
