//! Base-routing assertions on the sale rehearsal's real ICP Ledger/Index.
//! Staging pots here are controlled Ledger credits, not claimed NNS maturity.
//! Actual NNS maturity and gross 95/5 receipts remain in Disburser's real-NNS suite.
use super::*;
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsArgs, GetAccountIdentifierTransactionsResult,
};

#[derive(CandidType)]
struct Init {
    staking_account: Account,
    payout_subaccount: Option<[u8; 32]>,
    funding_source_account: Account,
    ledger_canister_id: Option<Principal>,
    index_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    rescue_controller: Principal,
    autonomous_rescue_armed: Option<bool>,
    main_interval_seconds: Option<u64>,
    rescue_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    stake_recognition_delay_seconds: Option<u64>,
}
#[derive(CandidType, Deserialize, Debug)]
struct Summary {
    pot_start_e8s: u64,
    effective_denom_staking_balance_e8s: Option<u64>,
    topped_up_count: u64,
    ignored_bad_memo: u64,
    remainder_to_relay_e8s: u64,
}
pub(super) fn install(
    pic: &PocketIc,
    wasm: Vec<u8>,
    faucet: Principal,
    disburser: Principal,
    staking: Account,
    rescue: Principal,
) -> Result<()> {
    pic.install_canister(
        faucet,
        wasm,
        encode_one(Init {
            staking_account: staking,
            payout_subaccount: None,
            funding_source_account: Account {
                owner: disburser,
                subaccount: None,
            },
            ledger_canister_id: Some(support::principals::icp_ledger()),
            index_canister_id: Some(support::principals::icp_index()),
            governance_canister_id: Some(support::principals::nns_governance()),
            rescue_controller: rescue,
            autonomous_rescue_armed: Some(false),
            main_interval_seconds: Some(365 * 86400),
            rescue_interval_seconds: Some(365 * 86400),
            min_tx_e8s: Some(100_000_000),
            stake_recognition_delay_seconds: Some(1),
        })?,
        None,
    );
    Ok(())
}
fn balance(pic: &PocketIc, account: Account) -> Result<u64> {
    support::ledger::icrc1_balance(pic, support::principals::icp_ledger(), &account)
}
fn send(pic: &PocketIc, to: Account, amount: u64, memo: Option<Vec<u8>>) -> Result<u64> {
    support::ledger::icrc1_transfer(
        pic,
        support::principals::icp_ledger(),
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to,
            amount: amount.into(),
            fee: Some(10_000_u64.into()),
            memo: memo.map(Into::into),
            created_at_time: None,
        },
    )
}
fn sync(pic: &PocketIc, account: Account) -> Result<()> {
    for _ in 0..80 {
        let page: GetAccountIdentifierTransactionsResult = query_one(
            pic,
            support::principals::icp_index(),
            Principal::anonymous(),
            "get_account_identifier_transactions",
            GetAccountIdentifierTransactionsArgs {
                account_identifier: jupiter_ic_clients::account_identifier::account_identifier_text(
                    account.owner,
                    account.subaccount,
                ),
                start: None,
                max_results: 100,
            },
        )?;
        if matches!(page, GetAccountIdentifierTransactionsResult::Ok(p) if p.balance == balance(pic, account)?)
        {
            return Ok(());
        }
        pic.advance_time(std::time::Duration::from_secs(1));
        for _ in 0..5 {
            pic.tick();
        }
    }
    anyhow::bail!("real ICP account history did not synchronise")
}
fn stage_base(pic: &PocketIc, disburser: Principal, faucet: Principal) -> Result<u64> {
    let before = balance(
        pic,
        Account {
            owner: faucet,
            subaccount: None,
        },
    )?;
    #[derive(CandidType, Deserialize)]
    struct BonusAccounts {
        age_bonus_recipient_1: Account,
        age_bonus_recipient_2: Account,
    }
    let bonus: BonusAccounts =
        query_one(pic, disburser, Principal::anonymous(), "debug_config", ())?;
    let bonus_before = (
        balance(pic, bonus.age_bonus_recipient_1)?,
        balance(pic, bonus.age_bonus_recipient_2)?,
    );
    // This is an explicit controlled zero-age payout snapshot. Native weighted
    // age was tested separately; no claim of zero-age NNS reward generation.
    let _: () = update_one(
        pic,
        disburser,
        Principal::anonymous(),
        "debug_set_prev_age_seconds",
        0_u64,
    )?;

    send(
        pic,
        Account {
            owner: disburser,
            subaccount: None,
        },
        1_000_000_000,
        None,
    )?;
    for _ in 0..8 {
        let _: () = update_noargs(pic, disburser, Principal::anonymous(), "debug_main_tick")?;
        for _ in 0..5 {
            pic.tick();
        }
        let now = balance(
            pic,
            Account {
                owner: faucet,
                subaccount: None,
            },
        )?;
        if now > before {
            sync(
                pic,
                Account {
                    owner: faucet,
                    subaccount: None,
                },
            )?;
            assert_eq!(
                (
                    balance(pic, bonus.age_bonus_recipient_1)?,
                    balance(pic, bonus.age_bonus_recipient_2)?
                ),
                bonus_before,
                "the controlled zero-age boundary has no gross or net bonus payment"
            );
            return Ok(now - before);
        }
    }
    anyhow::bail!("controlled Disburser base staging did not reach Faucet")
}
pub(super) fn check(
    pic: &PocketIc,
    disburser: Principal,
    faucet: Principal,
    staking: Account,
) -> Result<()> {
    // This is the unchanged production default in the owned PocketIC world;
    // no request is sent to that principal on a live network.
    let relay = Principal::from_text("u2qkp-aqaaa-aaaar-qb7ea-cai")?;
    let relay_default = Account {
        owner: relay,
        subaccount: None,
    };
    let relay_one = Account {
        owner: relay,
        subaccount: jupiter_memo_policy::numbered_subaccount(1),
    };
    sync(pic, staking)?;
    let principal_before = balance(pic, staking)?;
    assert_eq!(principal_before, 110 * 100_000_000);
    let default_before = balance(pic, relay_default)?;
    let one_before = balance(pic, relay_one)?;
    let pot = stage_base(pic, disburser, faucet)?;
    assert_eq!(pot, 999_990_000);
    let _: () = update_noargs(pic, faucet, Principal::anonymous(), "debug_main_tick")?;
    let summary: Option<Summary> = query_one(
        pic,
        faucet,
        Principal::anonymous(),
        "debug_last_summary",
        (),
    )?;
    let summary = summary.context("all-memo-less Faucet summary")?;
    assert_eq!(summary.pot_start_e8s, pot);
    assert_eq!(summary.effective_denom_staking_balance_e8s, Some(0));
    assert_eq!(summary.topped_up_count, 0);
    assert!(summary.ignored_bad_memo >= 2);
    assert_eq!(summary.remainder_to_relay_e8s, pot - 10_000);
    assert_eq!(balance(pic, relay_default)? - default_before, pot - 10_000);
    assert_eq!(balance(pic, relay_one)?, one_before);
    assert_eq!(
        balance(pic, staking)?,
        principal_before,
        "treasury principal must remain staked"
    );

    let alice = pic.create_canister();
    let bob = pic.create_canister();
    let alice_account = Account {
        owner: alice,
        subaccount: None,
    };
    let bob_account = Account {
        owner: bob,
        subaccount: None,
    };
    send(
        pic,
        staking,
        1_000_000_000,
        Some(format!("{}.", alice.to_text().replace('-', "")).into_bytes()),
    )?;
    send(
        pic,
        staking,
        2_000_000_000,
        Some(format!("{}.", bob.to_text().replace('-', "")).into_bytes()),
    )?;
    pic.advance_time(std::time::Duration::from_secs(86400));
    for _ in 0..30 {
        pic.tick();
    }
    sync(pic, staking)?;
    let second_pot = stage_base(pic, disburser, faucet)?;
    for _ in 0..16 {
        let _: () = update_noargs(pic, faucet, Principal::anonymous(), "debug_main_tick")?;
        if balance(pic, alice_account)? > 0 && balance(pic, bob_account)? > 0 {
            break;
        }
        for _ in 0..5 {
            pic.tick();
        }
    }
    let second: Option<Summary> = query_one(
        pic,
        faucet,
        Principal::anonymous(),
        "debug_last_summary",
        (),
    )?;
    let second = second.context("ordinary memo-bearing maturity allocation")?;
    assert_eq!(second.topped_up_count, 2);
    assert!(
        second
            .effective_denom_staking_balance_e8s
            .is_some_and(|n| n > 0 && n <= 3_000_000_000),
        "110 ICP of memo-less principal cannot enter the valid-beneficiary denominator: {second:?}"
    );
    let a = balance(pic, alice_account)?;
    let b = balance(pic, bob_account)?;
    assert!(a > 0 && b > a);
    // Each gross allocation floors independently. Comparing b against 2*a
    // would amplify a one-unit floor into a two-unit ratio difference.
    assert!((a + 10_000).abs_diff(second_pot / 3) <= 1
        && (b + 10_000).abs_diff((u128::from(second_pot) * 2 / 3) as u64) <= 1,
        "10/20 ICP commitments must receive their proportional gross amounts before actual downstream fees, within one e8s unit each: a={a}, b={b}");
    assert!(
        second_pot.abs_diff(a + b + 20_000) <= 1,
        "actual raw receipts plus fees must conserve the base pot"
    );
    assert_eq!(balance(pic, staking)?, principal_before + 3_000_000_000);

    // Real SNS Rewards and D-QUORUM-role Accounts receive the configured 95/5
    // net amounts. These are controlled maturity pots, not synthetic NNS rewards.
    #[derive(CandidType, Deserialize)]
    struct BonusAccounts {
        age_bonus_recipient_1: Account,
        age_bonus_recipient_2: Account,
    }
    let bonus: BonusAccounts =
        query_one(pic, disburser, Principal::anonymous(), "debug_config", ())?;
    for age in [2 * 365 * 86400_u64, 4 * 365 * 86400_u64] {
        let before = [
            balance(pic, bonus.age_bonus_recipient_1)?,
            balance(pic, bonus.age_bonus_recipient_2)?,
        ];
        let _: () = update_one(
            pic,
            disburser,
            Principal::anonymous(),
            "debug_set_prev_age_seconds",
            age,
        )?;
        let gross = 1_000_000_000_u64;
        send(
            pic,
            Account {
                owner: disburser,
                subaccount: None,
            },
            gross,
            None,
        )?;
        let denominator = 16 * 365 * 86400_u128;
        let base = (u128::from(gross) * denominator / (denominator + u128::from(age))) as u64;
        let total_bonus = gross - base;
        let sns_gross = (u128::from(total_bonus) * 95).div_ceil(100) as u64;
        let dquorum_gross = total_bonus - sns_gross;
        for _ in 0..8 {
            let _: () = update_noargs(pic, disburser, Principal::anonymous(), "debug_main_tick")?;
            if balance(pic, bonus.age_bonus_recipient_1)? > before[0] {
                break;
            }
            for _ in 0..5 {
                pic.tick();
            }
        }
        assert_eq!(
            balance(pic, bonus.age_bonus_recipient_1)? - before[0],
            sns_gross - 10_000
        );
        assert_eq!(
            balance(pic, bonus.age_bonus_recipient_2)? - before[1],
            dquorum_gross - 10_000
        );
        assert_eq!(balance(pic, staking)?, principal_before + 3_000_000_000);
    }
    Ok(())
}
