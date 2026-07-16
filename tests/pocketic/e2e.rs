// PocketIC integration fixtures keep literal conversions/copies for readable scenario setup.
#![allow(clippy::clone_on_copy, clippy::unnecessary_cast)]

use anyhow::{anyhow, bail, Result};
use candid::{encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use pocket_ic::PocketIc;

#[path = "real_blackhole.rs"]
mod real_blackhole;
#[path = "support/mod.rs"]
mod support;
use std::sync::OnceLock;
use std::time::Duration;
use support::account_identifier::account_identifier_text;
use support::calls::{query_one, tick_n, update_bytes, update_noargs, update_one};
use support::governance::set_controllers_exact;
use support::ledger::build_pic_with_real_icp;
use support::principals::fixture_principal;

fn require_ignored_flag() -> Result<()> {
    // These PocketIC suites are intentionally #[ignore] so a plain cargo test stays fast.
    // The supported repository entry points (for example `cargo run -p xtask -- test_all`)
    // invoke them explicitly with `--ignored`.
    support::assertions::require_ignored_flag()
}

static LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static GOVERNANCE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static INDEX_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CMC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static FAUCET_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static DISBURSER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static HISTORIAN_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn build_wasm(package: &str, features: Option<&str>) -> Result<Vec<u8>> {
    let cache = match package {
        "mock-icrc-ledger" => &LEDGER_WASM,
        "mock-nns-governance" => &GOVERNANCE_WASM,
        "mock-icp-index" => &INDEX_WASM,
        "mock-cmc" => &CMC_WASM,
        "jupiter-faucet" => &FAUCET_WASM,
        "jupiter-disburser" => &DISBURSER_WASM,
        "jupiter-historian" => &HISTORIAN_WASM,
        other => bail!("no E2E wasm cache configured for {other}"),
    };
    support::wasm::build_wasm_cached_for_test(cache, package, features)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct FaucetInitArg {
    staking_account: Account,
    payout_subaccount: Option<Vec<u8>>,
    funding_source_account: Account,
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
    stake_recognition_delay_seconds: Option<u64>,
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
    IndexLatestUnreadable,
    CmcZeroSuccessRuns,
    AccountingInvariantBroken,
    FundingTrancheBalanceMismatch,
    FundingDiscoveryUnreadable,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    active_payout_job_present: bool,
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
    #[serde(default)]
    ambiguous_topups: u64,
    ignored_under_threshold: u64,
    ignored_bad_memo: u64,
    remainder_to_self_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNotifyBehavior {
    Ok,
    Processing,
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    TransactionTooOld(u64),
    InvalidTransaction(String),
    Other {
        error_code: u64,
        error_message: String,
    },
}

fn icrc1_balance(pic: &PocketIc, ledger: Principal, acct: &Account) -> Result<u64> {
    support::ledger::icrc1_balance(pic, ledger, acct)
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

fn test_blackhole_controller() -> Principal {
    Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").unwrap()
}

fn cmc_deposit_account(cmc: Principal, target: Principal) -> Account {
    Account {
        owner: cmc,
        subaccount: Some(support::account_identifier::principal_to_subaccount(target)),
    }
}

fn disburser_funding_source(disburser: Principal) -> Account {
    Account {
        owner: disburser,
        subaccount: None,
    }
}

fn append_faucet_funding_tranche(
    pic: &PocketIc,
    index: Principal,
    funding_source: &Account,
    payout: &Account,
    amount_e8s: u64,
) -> Result<u64> {
    let timestamp_nanos =
        (pic.get_time().as_nanos_since_unix_epoch() as u64).saturating_add(2_000_000_000);
    update_bytes(
        pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer_from_with_timestamp",
        encode_args((
            support::account_identifier::account_id_for(funding_source),
            support::account_identifier::account_id_for(payout),
            amount_e8s,
            Option::<Vec<u8>>::None,
            timestamp_nanos,
        ))?,
    )
}

#[test]
#[ignore]
fn suite_disburser_pays_faucet_and_faucet_tops_up_target() -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
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
        owner: fixture_principal(),
        subaccount: Some([7u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        funding_source_account: disburser_funding_source(disburser),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(100_000_000),
        expected_first_staking_tx_id: None,
        stake_recognition_delay_seconds: Some(1),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let accounts: DebugAccounts =
        query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account {
            owner: Principal::management_canister(),
            subaccount: None,
        },
        age_bonus_recipient_2: Account {
            owner: pic.create_canister(),
            subaccount: None,
        },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
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
    let disburser_staging = Account {
        owner: disburser,
        subaccount: None,
    };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, pot_e8s))?,
    )?;

    let faucet_balance_before = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let faucet_balance_after = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let funding_amount = faucet_balance_after
        .checked_sub(faucet_balance_before)
        .ok_or_else(|| anyhow!("faucet payout balance decreased during disburser tick"))?;
    append_faucet_funding_tranche(
        &pic,
        index,
        &disburser_funding_source(disburser),
        &accounts.payout,
        funding_amount,
    )?;

    let faucet_balance = icrc1_balance(&pic, ledger, &accounts.payout)?;
    if faucet_balance == 0 {
        bail!("expected disburser to transfer ICP into faucet payout account");
    }

    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let summary: Option<FaucetSummary> = query_one(
        &pic,
        faucet,
        Principal::anonymous(),
        "debug_last_summary",
        (),
    )?;
    let summary =
        summary.ok_or_else(|| anyhow!("expected faucet summary after successful tick"))?;
    if summary.topped_up_count != 1 {
        bail!(
            "expected one beneficiary top-up, got {}",
            summary.topped_up_count
        );
    }
    if summary.ignored_bad_memo != 0 || summary.ignored_under_threshold != 0 {
        bail!(
            "expected no ignored commitments, got bad_memo={} under_threshold={}",
            summary.ignored_bad_memo,
            summary.ignored_under_threshold
        );
    }

    let notes: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if !notes.iter().any(|n| n.canister_id == target) {
        bail!("expected mock CMC to record a top-up notification for target canister {target}");
    }

    Ok(())
}

#[test]
#[ignore]
fn suite_repeated_disburser_payouts_make_faucet_replay_full_history() -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
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
        owner: fixture_principal(),
        subaccount: Some([8u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        funding_source_account: disburser_funding_source(disburser),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(100_000_000),
        expected_first_staking_tx_id: None,
        stake_recognition_delay_seconds: Some(1),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let accounts: DebugAccounts =
        query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account {
            owner: Principal::management_canister(),
            subaccount: None,
        },
        age_bonus_recipient_2: Account {
            owner: pic.create_canister(),
            subaccount: None,
        },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
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
        encode_args((
            staking_id,
            3u64,
            100_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;

    for (i, pot_e8s) in [90_000_000u64, 60_000_000u64].into_iter().enumerate() {
        // Advance PocketIC time between payout cycles so the disburser and faucet
        // use distinct created_at_time values and the mock ledger does not dedup
        // the second cycle as a duplicate replay of the first transfer set.
        if i > 0 {
            pic.advance_time(Duration::from_secs(1));
            tick_n(&pic, 1);
        }

        let disburser_staging = Account {
            owner: disburser,
            subaccount: None,
        };
        let _: () = update_bytes(
            &pic,
            ledger,
            Principal::anonymous(),
            "debug_credit",
            encode_args((disburser_staging, pot_e8s))?,
        )?;
        let faucet_balance_before = icrc1_balance(&pic, ledger, &accounts.payout)?;
        let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 10);
        let faucet_balance_after = icrc1_balance(&pic, ledger, &accounts.payout)?;
        let funding_amount = faucet_balance_after
            .checked_sub(faucet_balance_before)
            .ok_or_else(|| anyhow!("faucet payout balance decreased during disburser tick"))?;
        append_faucet_funding_tranche(
            &pic,
            index,
            &disburser_funding_source(disburser),
            &accounts.payout,
            funding_amount,
        )?;
        let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 10);

        // In this suite-level replay scenario we want two distinct completed disburser
        // payout cycles. The mock governance marks a disbursement as in-flight after a
        // successful initiation and keeps it there until explicitly cleared, which would
        // otherwise cause the next disburser tick to skip its payout stage entirely.
        let _: () = update_one(
            &pic,
            gov,
            Principal::anonymous(),
            "debug_set_in_flight",
            false,
        )?;

        let summary: Option<FaucetSummary> = query_one(
            &pic,
            faucet,
            Principal::anonymous(),
            "debug_last_summary",
            (),
        )?;
        let summary =
            summary.ok_or_else(|| anyhow!("expected faucet summary after payout cycle"))?;
        if summary.topped_up_count != 3 {
            bail!(
                "expected each payout cycle to replay the same three commitments, got {}",
                summary.topped_up_count
            );
        }
    }

    let notes: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    let beneficiary_notes = notes.iter().filter(|n| n.canister_id == target).count();
    if beneficiary_notes != 6 {
        bail!("expected repeated suite payouts to re-notify the same beneficiary three times per cycle, got {beneficiary_notes}");
    }

    Ok(())
}

#[test]
#[ignore]
fn suite_retry_path_across_disburser_faucet_and_cmc_boundary_avoids_duplicate_transfer(
) -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
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
        owner: fixture_principal(),
        subaccount: Some([6u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        funding_source_account: disburser_funding_source(disburser),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(100_000_000),
        expected_first_staking_tx_id: None,
        stake_recognition_delay_seconds: Some(1),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let accounts: DebugAccounts =
        query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account {
            owner: Principal::management_canister(),
            subaccount: None,
        },
        age_bonus_recipient_2: Account {
            owner: pic.create_canister(),
            subaccount: None,
        },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), 100_000_000u64))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;
    let disburser_staging = Account {
        owner: disburser,
        subaccount: None,
    };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, 100_000_000u64))?,
    )?;

    let faucet_balance_before = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let faucet_balance_after = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let funding_amount = faucet_balance_after
        .checked_sub(faucet_balance_before)
        .ok_or_else(|| anyhow!("faucet payout balance decreased during disburser tick"))?;
    append_faucet_funding_tranche(
        &pic,
        index,
        &disburser_funding_source(disburser),
        &accounts.payout,
        funding_amount,
    )?;

    let _: () = update_one(
        &pic,
        cmc,
        Principal::anonymous(),
        "debug_set_script",
        vec![DebugNotifyBehavior::Processing, DebugNotifyBehavior::Ok],
    )?;
    let transfers_before_faucet: Vec<TransferRecord> =
        query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if st.active_payout_job_present || !st.last_summary_present {
        bail!("expected faucet inline retry path to complete within one suite tick");
    }
    let transfers_after: Vec<TransferRecord> =
        query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after.len() != transfers_before_faucet.len().saturating_add(1) {
        bail!(
            "expected exactly one additional faucet beneficiary transfer after inline retry; before={} after={}",
            transfers_before_faucet.len(),
            transfers_after.len()
        );
    }
    let notes: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected one eventual target notification after suite inline retry recovery, got {notes:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn suite_upgrade_faucet_after_inline_retry_recovery_preserves_state() -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
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
        owner: fixture_principal(),
        subaccount: Some([5u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        funding_source_account: disburser_funding_source(disburser),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(100_000_000),
        expected_first_staking_tx_id: None,
        stake_recognition_delay_seconds: Some(1),
    };
    pic.install_canister(faucet, faucet_wasm.clone(), encode_one(faucet_init)?, None);

    let accounts: DebugAccounts =
        query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account {
            owner: Principal::management_canister(),
            subaccount: None,
        },
        age_bonus_recipient_2: Account {
            owner: pic.create_canister(),
            subaccount: None,
        },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = faucet;
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), 100_000_000u64))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;
    let disburser_staging = Account {
        owner: disburser,
        subaccount: None,
    };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, 100_000_000u64))?,
    )?;

    let faucet_balance_before = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let faucet_balance_after = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let funding_amount = faucet_balance_after
        .checked_sub(faucet_balance_before)
        .ok_or_else(|| anyhow!("faucet payout balance decreased during disburser tick"))?;
    append_faucet_funding_tranche(
        &pic,
        index,
        &disburser_funding_source(disburser),
        &accounts.payout,
        funding_amount,
    )?;

    let _: () = update_one(
        &pic,
        cmc,
        Principal::anonymous(),
        "debug_set_script",
        vec![DebugNotifyBehavior::Processing, DebugNotifyBehavior::Ok],
    )?;
    let transfers_before_faucet: Vec<TransferRecord> =
        query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let st1: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if st1.active_payout_job_present || !st1.last_summary_present {
        bail!("expected inline retry path to complete before faucet upgrade in suite path");
    }
    let transfers_after_first: Vec<TransferRecord> =
        query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after_first.len() != transfers_before_faucet.len().saturating_add(1) {
        bail!("expected exactly one faucet beneficiary transfer before upgrade");
    }

    let faucet_upgrade_sender = pic
        .get_controllers(faucet)
        .first()
        .copied()
        .unwrap_or(faucet);
    pic.upgrade_canister(
        faucet,
        faucet_wasm,
        encode_one(())?,
        Some(faucet_upgrade_sender),
    )
    .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;
    tick_n(&pic, 10);

    let st2: DebugState = query_one(&pic, faucet, Principal::anonymous(), "debug_state", ())?;
    if st2.active_payout_job_present || !st2.last_summary_present {
        bail!("expected completed inline retry recovery to remain quiescent after upgrade");
    }
    let transfers_after_upgrade: Vec<TransferRecord> =
        query_one(&pic, ledger, Principal::anonymous(), "debug_transfers", ())?;
    if transfers_after_upgrade.len() != transfers_after_first.len() {
        bail!("expected suite upgrade path not to duplicate faucet beneficiary transfer");
    }
    let notes: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected exactly one beneficiary notification to remain after post-upgrade check, got {notes:?}");
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
    sns_wasm_canister_id: Option<Principal>,
    enable_sns_tracking: Option<bool>,
    scan_interval_seconds: Option<u64>,
    cycles_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    max_cycles_entries_per_canister: Option<u32>,
    max_commitment_entries_per_canister: Option<u32>,
    max_index_pages_per_tick: Option<u32>,
    max_canisters_per_cycles_tick: Option<u32>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum CanisterTrackingReason {
    MemoCommitment,
    SnsDiscovery,
    RelayTarget,
    RelayInstance,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListCanistersArgs {
    start_after: Option<Principal>,
    limit: Option<u32>,
    tracking_reason_filter: Option<CanisterTrackingReason>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianCanisterListItem {
    canister_id: Principal,
    tracking_reasons: Vec<CanisterTrackingReason>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListCanistersResponse {
    items: Vec<HistorianCanisterListItem>,
    next_start_after: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianGetCommitmentHistoryArgs {
    canister_id: Principal,
    start_after_tx_id: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianCommitmentSample {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianCommitmentHistoryPage {
    items: Vec<HistorianCommitmentSample>,
    next_start_after_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum HistorianCyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootStatus,
    SnsSwapStatus,
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
struct HistorianPublicStatus {
    staking_account: Account,
    ledger_canister_id: Principal,
    last_index_run_ts: Option<u64>,
    index_interval_seconds: u64,
    last_completed_cycles_sweep_ts: Option<u64>,
    cycles_interval_seconds: u64,
    heap_memory_bytes: Option<u64>,
    stable_memory_bytes: Option<u64>,
    total_memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct HistorianListMemoRegisteredCanisterSummariesArgs {
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianMemoRegisteredCanisterSummary {
    canister_id: Principal,
    tracking_reasons: Vec<CanisterTrackingReason>,
    qualifying_commitment_count: u64,
    total_qualifying_committed_e8s: u64,
    last_commitment_ts: Option<u64>,
    latest_cycles: Option<u128>,
    last_cycles_probe_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListMemoRegisteredCanisterSummariesResponse {
    items: Vec<HistorianMemoRegisteredCanisterSummary>,
    page: u32,
    page_size: u32,
    total: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct HistorianListRecentCommitmentsArgs {
    limit: Option<u32>,
    qualifying_only: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianRecentCommitmentListItem {
    canister_id: Option<Principal>,
    memo_text: Option<String>,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
    tx_hash: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianListRecentCommitmentsResponse {
    items: Vec<HistorianRecentCommitmentListItem>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RealNotifyTopUpArg {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RealNotifyTopUpResult {
    Ok(Nat),
    Err(RealNotifyError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RealNotifyError {
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    Processing,
    TransactionTooOld(u64),
    InvalidTransaction(String),
    Other {
        error_code: u64,
        error_message: String,
    },
}

fn describe_account(account: &Account) -> String {
    let sub_hex = account
        .subaccount
        .map(hex::encode)
        .unwrap_or_else(|| "<none>".to_string());
    format!(
        "owner={} subaccount_hex={} account_id={}",
        account.owner,
        sub_hex,
        account_identifier_text(account.owner, account.subaccount)
    )
}

#[test]
#[ignore]
fn probe_real_cmc_topup_flow_diagnostics() -> Result<()> {
    require_ignored_flag()?;
    let pic = build_pic_with_real_icp();
    let ledger = support::principals::icp_ledger();
    let cmc = support::principals::cycles_minting_canister();
    let blackhole_wasm = real_blackhole::real_blackhole_wasm()?;

    let target = pic.create_canister();
    pic.add_cycles(target, 5_000_000_000_000);
    pic.install_canister(target, blackhole_wasm, vec![], None);
    set_controllers_exact(&pic, target, vec![target])?;

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let memo_u64 = 1_347_768_404u64;
    let memo_bytes = memo_u64.to_le_bytes().to_vec();
    let deposit_account = cmc_deposit_account(cmc, target);
    let amount_e8s = 500_000_000u64;

    let anon_default = Account {
        owner: Principal::anonymous(),
        subaccount: None,
    };
    let deposit_before = icrc1_balance(&pic, ledger, &deposit_account)?;
    let anon_before = icrc1_balance(&pic, ledger, &anon_default)?;

    println!("=== real CMC top-up probe ===");
    println!("target_canister={}", target);
    println!("ledger_canister={}", ledger);
    println!("cmc_canister={}", cmc);
    println!(
        "anonymous_default_account={}",
        describe_account(&anon_default)
    );
    println!("deposit_account={}", describe_account(&deposit_account));
    println!(
        "principal_to_subaccount_hex={}",
        hex::encode(support::account_identifier::principal_to_subaccount(target))
    );
    println!("top_up_memo_u64={}", memo_u64);
    println!("top_up_memo_hex={}", hex::encode(&memo_bytes));
    println!("top_up_memo_ascii={:?}", memo_bytes);
    println!("transfer_fee_e8s={}", fee_e8s);
    println!("transfer_amount_e8s={}", amount_e8s);
    println!("deposit_balance_before_e8s={}", deposit_before);
    println!("anonymous_balance_before_e8s={}", anon_before);

    let block_index = icrc1_transfer(
        &pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: deposit_account.clone(),
            fee: Some(Nat::from(fee_e8s)),
            created_at_time: None,
            memo: Some(Memo::from(memo_bytes.clone())),
            amount: Nat::from(amount_e8s),
        },
    )?;

    let deposit_after_transfer = icrc1_balance(&pic, ledger, &deposit_account)?;
    let anon_after_transfer = icrc1_balance(&pic, ledger, &anon_default)?;
    println!("transfer_block_index={}", block_index);
    println!(
        "deposit_balance_after_transfer_e8s={}",
        deposit_after_transfer
    );
    println!(
        "anonymous_balance_after_transfer_e8s={}",
        anon_after_transfer
    );

    let notify_result: RealNotifyTopUpResult = update_one(
        &pic,
        cmc,
        Principal::anonymous(),
        "notify_top_up",
        RealNotifyTopUpArg {
            canister_id: target,
            block_index,
        },
    )?;
    println!("notify_top_up_result={notify_result:?}");

    let deposit_after_notify = icrc1_balance(&pic, ledger, &deposit_account)?;
    let anon_after_notify = icrc1_balance(&pic, ledger, &anon_default)?;
    println!("deposit_balance_after_notify_e8s={}", deposit_after_notify);
    println!("anonymous_balance_after_notify_e8s={}", anon_after_notify);
    println!("=== end real CMC top-up probe ===");

    Ok(())
}

#[test]
#[ignore]
fn suite_historian_tracks_same_staking_flow_as_faucet() -> Result<()> {
    require_ignored_flag()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
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

    for c in [
        ledger, gov, index, cmc, blackhole, faucet, disburser, historian,
    ] {
        pic.add_cycles(c, 5_000_000_000_000);
    }

    pic.install_canister(ledger, ledger_wasm, vec![], None);
    pic.install_canister(gov, gov_wasm, vec![], None);
    pic.install_canister(index, index_wasm, vec![], None);
    pic.install_canister(cmc, cmc_wasm, vec![], None);
    pic.install_canister(blackhole, blackhole_wasm, vec![], None);
    set_controllers_exact(&pic, blackhole, vec![blackhole])?;

    let staking_account = Account {
        owner: Principal::management_canister(),
        subaccount: Some([11u8; 32]),
    };
    let faucet_init = FaucetInitArg {
        staking_account: staking_account.clone(),
        payout_subaccount: None,
        funding_source_account: disburser_funding_source(disburser),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
        min_tx_e8s: Some(100_000_000),
        expected_first_staking_tx_id: None,
        stake_recognition_delay_seconds: Some(1),
    };
    pic.install_canister(faucet, faucet_wasm, encode_one(faucet_init)?, None);

    let historian_init = HistorianInitArg {
        staking_account: staking_account.clone(),
        ledger_canister_id: Some(ledger),
        index_canister_id: Some(index),
        cmc_canister_id: Some(cmc),
        faucet_canister_id: Some(faucet),
        sns_wasm_canister_id: None,
        enable_sns_tracking: Some(false),
        scan_interval_seconds: Some(60),
        cycles_interval_seconds: Some(1),
        min_tx_e8s: Some(100_000_000),
        max_cycles_entries_per_canister: Some(100),
        max_commitment_entries_per_canister: Some(100),
        max_index_pages_per_tick: Some(10),
        max_canisters_per_cycles_tick: Some(10),
    };
    pic.install_canister(historian, historian_wasm, encode_one(historian_init)?, None);

    let accounts: DebugAccounts =
        query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
    let disburser_init = DisburserInitArg {
        neuron_id: 1,
        normal_recipient: accounts.payout.clone(),
        age_bonus_recipient_1: Account {
            owner: Principal::management_canister(),
            subaccount: None,
        },
        age_bonus_recipient_2: Account {
            owner: pic.create_canister(),
            subaccount: None,
        },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: fixture_principal(),
        blackhole_controller: Some(test_blackhole_controller()),
        blackhole_armed: Some(false),
        main_interval_seconds: Some(86_400),
        rescue_interval_seconds: Some(86_400),
    };
    pic.install_canister(disburser, disburser_wasm, encode_one(disburser_init)?, None);

    let target = historian;
    let staking_id = account_identifier_text(staking_account.owner, staking_account.subaccount);
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((staking_account.clone(), 100_000_000u64))?,
    )?;
    let _: u64 = update_bytes(
        &pic,
        index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((
            staking_id,
            100_000_000u64,
            Some(target.to_text().into_bytes()),
        ))?,
    )?;

    let disburser_staging = Account {
        owner: disburser,
        subaccount: None,
    };
    let _: () = update_bytes(
        &pic,
        ledger,
        Principal::anonymous(),
        "debug_credit",
        encode_args((disburser_staging, 100_000_000u64))?,
    )?;

    let faucet_balance_before = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let _: () = update_noargs(&pic, disburser, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let faucet_balance_after = icrc1_balance(&pic, ledger, &accounts.payout)?;
    let funding_amount = faucet_balance_after
        .checked_sub(faucet_balance_before)
        .ok_or_else(|| anyhow!("faucet payout balance decreased during disburser tick"))?;
    append_faucet_funding_tranche(
        &pic,
        index,
        &disburser_funding_source(disburser),
        &accounts.payout,
        funding_amount,
    )?;
    let _: () = update_noargs(&pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    pic.advance_time(Duration::from_secs(2));
    let _: () = update_noargs(&pic, historian, Principal::anonymous(), "debug_driver_tick")?;
    tick_n(&pic, 10);

    let listed: HistorianListCanistersResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_canisters",
        HistorianListCanistersArgs {
            start_after: None,
            limit: Some(10),
            tracking_reason_filter: None,
        },
    )?;
    let listed_target = listed
        .items
        .iter()
        .find(|item| item.canister_id == target)
        .expect("historian should list the memo-derived target");
    assert!(listed_target
        .tracking_reasons
        .contains(&CanisterTrackingReason::MemoCommitment));

    let commitments: HistorianCommitmentHistoryPage = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_commitment_history",
        HistorianGetCommitmentHistoryArgs {
            canister_id: target,
            start_after_tx_id: None,
            limit: Some(10),
            descending: Some(false),
        },
    )?;
    assert_eq!(commitments.items.len(), 1);
    assert!(commitments.items[0].counts_toward_faucet);

    let cycles: HistorianCyclesHistoryPage = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_cycles_history",
        HistorianGetCyclesHistoryArgs {
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
        HistorianCyclesSampleSource::SelfCanister
    ));

    let expected_output_e8s = 0u64;

    let counts: HistorianPublicCounts = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_counts",
        (),
    )?;
    assert!(counts.tracked_canister_count >= 1);
    assert_eq!(counts.memo_registered_canister_count, 1);
    assert_eq!(counts.qualifying_commitment_count, 1);
    assert_eq!(counts.total_output_e8s, expected_output_e8s);
    assert_eq!(counts.total_rewards_e8s, 0);
    assert_eq!(counts.sns_discovered_canister_count, 0);

    let status: HistorianPublicStatus = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "get_public_status",
        (),
    )?;
    assert_eq!(status.staking_account, staking_account);
    assert_eq!(status.ledger_canister_id, ledger);
    assert_eq!(status.index_interval_seconds, 60);
    assert_eq!(status.cycles_interval_seconds, 1);
    assert!(status.last_index_run_ts.is_some());
    assert!(status.last_completed_cycles_sweep_ts.is_some());
    assert!(status.heap_memory_bytes.is_some());
    assert!(status.stable_memory_bytes.is_some());
    assert!(status.total_memory_bytes.is_some());

    let registered: HistorianListMemoRegisteredCanisterSummariesResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_memo_registered_canister_summaries",
        HistorianListMemoRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
        },
    )?;
    assert_eq!(registered.total, 1);
    assert_eq!(registered.items.len(), 1);
    assert_eq!(registered.items[0].canister_id, target);
    assert_eq!(registered.items[0].qualifying_commitment_count, 1);
    assert_eq!(
        registered.items[0].total_qualifying_committed_e8s,
        100_000_000
    );
    assert!(registered.items[0].last_commitment_ts.is_some());
    assert!(registered.items[0].latest_cycles.unwrap_or_default() > 0);
    assert!(registered.items[0].last_cycles_probe_ts.is_some());

    let recent: HistorianListRecentCommitmentsResponse = query_one(
        &pic,
        historian,
        Principal::anonymous(),
        "list_recent_commitments",
        HistorianListRecentCommitmentsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent.items.len(), 1);
    assert_eq!(recent.items[0].canister_id, Some(target));
    assert_eq!(recent.items[0].tx_id, 1);
    assert_eq!(recent.items[0].amount_e8s, 100_000_000);
    assert!(recent.items[0].counts_toward_faucet);

    let notes: Vec<NotifyRecord> =
        query_one(&pic, cmc, Principal::anonymous(), "debug_notifications", ())?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected faucet notify_top_up for target, got {notes:?}");
    }

    Ok(())
}
