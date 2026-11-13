//! Reserve funding through the existing real-SNS/finalised-child acceptance fixture.
//! The actual immutable child performs every downstream token allocation.
use super::*;
use std::path::Path;

#[derive(CandidType, Deserialize, Debug, PartialEq)]
enum PotRoute {
    Direct,
    RelayPot {
        relay: Principal,
        factory_setup_key: Option<Vec<u8>>,
    },
    UnsupportedRelay {
        claimed_relay: Principal,
    },
}
#[derive(CandidType, Deserialize, Debug)]
enum Outcome {
    Prepared,
    Dispatched { at_nanos: u64 },
    Verified { block: Nat, duplicate: bool },
    AssumedPaidUnverified { reason: String },
    Failed { error: String },
    Skipped { reason: String },
}
#[derive(CandidType, Deserialize, Debug)]
struct Pot {
    contribution_block: u64,
    contributor_icp_account: Vec<u8>,
    qualifying_icp_e8s: u64,
    amount_e8s: u64,
    fee_e8s: u64,
    to: Account,
    route: PotRoute,
    outcome: Outcome,
}

pub(super) struct Fixture<'a> {
    pub pic: &'a PocketIc,
    pub repo: &'a Path,
    pub profile: &'a Path,
    pub evidence: &'a Path,
    pub rewards: Principal,
    pub root: Principal,
    pub relay: Principal,
    pub child: Principal,
    pub operator: Principal,
    pub ledger: Principal,
    pub index: Principal,
    pub staking: Account,
    pub production: Vec<u8>,
}
fn drive(pic: &PocketIc) {
    pic.advance_time(Duration::from_secs(31));
    tick_n(pic, 20);
}
pub(super) fn run(f: Fixture<'_>) -> Result<Vec<u64>> {
    // Profile-bound constructors cannot invent or override Ledger/staking/factory identities.
    let policy = json!({"version":1, "launch_timestamp_seconds":9_000_000_000_u64,
        "patron_enabled":true, "protocol_governance_neuron":"ef".repeat(32),
        "excluded_jup_neurons":["ef".repeat(32)], "excluded_icp_accounts":[],
        "reserve_subaccount":null, "reserve_transport_neuron":null,
        "snapshot_max_age_seconds":172800, "automation":null});
    let policy_path = f.evidence.join("reward-pot-policy.json");
    fs::write(&policy_path, serde_json::to_vec_pretty(&policy)?)?;
    let args_path = f.evidence.join("reward-pot-upgrade.bin");
    checked_command(Command::new("python3").arg("-c").arg(r#"
import json,sys
from pathlib import Path
sys.path.insert(0,'tools/acceptance')
import profile,prepare,candid as cd
p=json.loads(Path(sys.argv[1]).read_text()); policy=json.loads(Path(sys.argv[2]).read_text())
args=cd.record(programme='opt '+prepare.programme_arguments(profile.validate(p),policy), reward_sns_root_canister_id='null')
did=profile.REPO/'canisters/sns-rewards/jupiter_sns_rewards.did'
types='(opt record { programme : opt ProgrammeConfig; reward_sns_root_canister_id : opt opt principal })'
Path(sys.argv[3]).write_bytes(cd.encode_checked(did,types,'(opt '+args+')'))
"#).arg(f.profile).arg(&policy_path).arg(&args_path).current_dir(f.repo))?;
    let reserve = Account {
        owner: f.rewards,
        subaccount: None,
    };
    let transfer: Result<Nat, icrc_ledger_types::icrc1::transfer::TransferError> = update_one(
        f.pic,
        f.ledger,
        f.operator,
        "icrc1_transfer",
        TransferArg {
            from_subaccount: None,
            to: reserve,
            amount: 90_000_000_u64.into(),
            fee: Some(1_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    assert!(transfer.is_ok(), "reserve fixture: {transfer:?}");
    let reserve_before = support::ledger::icrc1_balance(f.pic, f.ledger, &reserve)?;
    let child_hash = f
        .pic
        .canister_status(f.child, Some(f.operator))
        .map_err(|e| anyhow::anyhow!("public child: {e:?}"))?
        .module_hash;
    tick_n(f.pic, 100);
    f.pic
        .upgrade_canister(f.rewards, f.production, fs::read(args_path)?, Some(f.root))
        .map_err(|e| anyhow::anyhow!("production patron activation: {e:?}"))?;
    let account = account_identifier_text(f.staking.owner, f.staking.subaccount);
    let page: GetAccountIdentifierTransactionsResult = query_one(
        f.pic,
        support::principals::icp_index(),
        f.operator,
        "get_account_identifier_transactions",
        GetAccountIdentifierTransactionsArgs {
            account_identifier: account,
            start: None,
            max_results: 100,
        },
    )?;
    let GetAccountIdentifierTransactionsResult::Ok(page) = page else {
        bail!("staking Index")
    };
    let mut expected = Vec::new();
    for relay in [f.relay, f.child] {
        let source = account_identifier_text(relay, Some(relay_subaccount_one()));
        let tx = page
            .transactions
            .iter()
            .find(|tx| {
                matches!(&tx.transaction.operation,
            jupiter_ic_clients::index::IndexOperation::Transfer { from, .. } if *from == source)
            })
            .context("actual historical Relay endowment")?;
        let jupiter_ic_clients::index::IndexOperation::Transfer { amount, .. } =
            &tx.transaction.operation
        else {
            unreachable!()
        };
        expected.push((relay, tx.id, amount.e8s()));
    }
    let mut pots = Vec::new();
    for (relay, block, net_icp) in expected {
        let mut found = None;
        for _ in 0..160 {
            drive(f.pic);
            let value: Option<Pot> =
                query_one(f.pic, f.rewards, f.operator, "get_patron_record", block)?;
            if let Some(p) = value {
                if matches!(p.outcome, Outcome::Verified { .. }) {
                    found = Some(p);
                    break;
                }
            }
        }
        let p = found.context("reserve must fund authentic Relay without a neuron")?;
        assert_eq!(p.contribution_block, block);
        assert_eq!(
            p.to,
            Account {
                owner: relay,
                subaccount: None
            }
        );
        assert_eq!(
            p.contributor_icp_account,
            hex::decode(account_identifier_text(relay, Some(relay_subaccount_one())))?
        );
        assert_eq!(p.qualifying_icp_e8s, net_icp);
        assert_eq!(p.amount_e8s, net_icp / 10);
        assert_eq!(p.fee_e8s, 1_000);
        match &p.route {
            PotRoute::RelayPot {
                relay: actual,
                factory_setup_key,
            } => {
                assert_eq!(*actual, relay);
                assert_eq!(factory_setup_key.is_some(), relay == f.child);
            }
            other => bail!("wrong endowment classification: {other:?}"),
        }
        pots.push(p);
    }
    let spent: u64 = pots.iter().map(|p| p.amount_e8s + p.fee_e8s).sum();
    assert_eq!(
        reserve_before - support::ledger::icrc1_balance(f.pic, f.ledger, &reserve)?,
        spent
    );
    for _ in 0..8 {
        drive(f.pic);
    }
    assert_eq!(
        reserve_before - support::ledger::icrc1_balance(f.pic, f.ledger, &reserve)?,
        spent,
        "factory setup/residual and repeated history cannot create a second opportunity"
    );
    // An arbitrary principal with the exact source subaccount and copied memo
    // is still unsupported by the real factory. It cannot hold the scan forever.
    let spoof = f.pic.create_canister();
    real_icp_transfer(
        f.pic,
        support::principals::icp_ledger(),
        Principal::anonymous(),
        Account {
            owner: spoof,
            subaccount: Some(relay_subaccount_one()),
        },
        100_010_000,
    )?;
    let spoof_block = support::ledger::icrc1_transfer(
        f.pic,
        support::principals::icp_ledger(),
        spoof,
        TransferArg {
            from_subaccount: Some(relay_subaccount_one()),
            to: f.staking,
            amount: 100_000_000_u64.into(),
            fee: Some(10_000_u64.into()),
            memo: Some(
                format!("{}.Relay", spoof.to_text().replace('-', ""))
                    .into_bytes()
                    .into(),
            ),
            created_at_time: None,
        },
    )?;
    let mut rejected = false;
    for _ in 0..120 {
        drive(f.pic);
        let record: Option<Pot> = query_one(
            f.pic,
            f.rewards,
            f.operator,
            "get_patron_record",
            spoof_block,
        )?;
        if let Some(p) = record {
            assert!(matches!(p.outcome, Outcome::Skipped { .. }));
            assert_eq!(
                p.route,
                PotRoute::UnsupportedRelay {
                    claimed_relay: spoof
                }
            );
            rejected = true;
            break;
        }
    }
    assert!(
        rejected,
        "unsupported source must finish assessment without funding or indefinite retry"
    );
    assert_eq!(
        support::ledger::icrc1_balance(
            f.pic,
            f.ledger,
            &Account {
                owner: spoof,
                subaccount: None
            }
        )?,
        0
    );
    // The controllerless production child receives a new real contributor credit,
    // forwards it itself, and distributes the reserve-funded token pot unchanged.
    let child_jup_before = support::ledger::icrc1_balance(
        f.pic,
        f.ledger,
        &Account {
            owner: f.operator,
            subaccount: None,
        },
    )?;
    real_icp_transfer(
        f.pic,
        support::principals::icp_ledger(),
        Principal::anonymous(),
        Account {
            owner: f.operator,
            subaccount: None,
        },
        400_000_000,
    )?;
    real_icp_transfer(
        f.pic,
        support::principals::icp_ledger(),
        f.operator,
        Account {
            owner: f.child,
            subaccount: Some(relay_subaccount_one()),
        },
        300_010_000,
    )?;
    f.pic.advance_time(Duration::from_secs(86_401));
    tick_n(f.pic, 200);
    let mut new_pot = None;
    for _ in 0..120 {
        drive(f.pic);
        let reply: GetAccountIdentifierTransactionsResult = query_one(
            f.pic,
            support::principals::icp_index(),
            f.operator,
            "get_account_identifier_transactions",
            GetAccountIdentifierTransactionsArgs {
                account_identifier: account_identifier_text(f.staking.owner, f.staking.subaccount),
                start: None,
                max_results: 100,
            },
        )?;
        let GetAccountIdentifierTransactionsResult::Ok(page) = reply else {
            bail!("new child Index")
        };
        if let Some(tx) = page.transactions.iter().find(|tx| matches!(&tx.transaction.operation,
            jupiter_ic_clients::index::IndexOperation::Transfer { from, amount, .. }
            if *from == account_identifier_text(f.child, Some(relay_subaccount_one())) && amount.e8s() == 300_000_000)) {
            let p: Option<Pot> = query_one(f.pic, f.rewards, f.operator, "get_patron_record", tx.id)?;
            if let Some(p) = p { if matches!(p.outcome, Outcome::Verified { .. }) { new_pot = Some(p); break; } }
        }
    }
    let new_pot = new_pot.context("new factory endowment reserve funding")?;
    assert_eq!(new_pot.amount_e8s, 30_000_000);
    assert!(
        matches!(&new_pot.route, PotRoute::RelayPot { relay, factory_setup_key: Some(key) } if *relay == f.child && key.len() == 32)
    );
    let total_spent = spent + new_pot.amount_e8s + new_pot.fee_e8s;
    let funded_at = match &new_pot.outcome {
        Outcome::Verified { block, .. } => block.clone(),
        _ => unreachable!("verified child pot"),
    };
    pots.push(new_pot);
    assert_eq!(
        reserve_before - support::ledger::icrc1_balance(f.pic, f.ledger, &reserve)?,
        total_spent
    );
    let mut child_receipt = None;
    // Run ordinary daily reconciliation so a newly completed owner snapshot can
    // be consumed by a later child tick. Repeated eight-day jumps make every
    // child attempt observe a stale snapshot before discovery completes again.
    for day in 0..32 {
        f.pic.advance_time(Duration::from_secs(86401));
        tick_n(f.pic, 200);
        for _ in 0..4 {
            drive(f.pic);
        }
        if day == 0 || day == 31 {
            if let Ok(logs) = f.pic.fetch_canister_logs(f.child, f.operator) {
                for entry in logs {
                    eprintln!(
                        "factory child day {day}: {}",
                        String::from_utf8_lossy(&entry.content)
                    );
                }
            }
        }
        let page: jupiter_ic_clients::icrc_index::GetAccountTransactionsResult = update_one(
            f.pic,
            f.index,
            f.operator,
            "get_account_transactions",
            jupiter_ic_clients::icrc_index::GetAccountTransactionsArgs {
                account: Account {
                    owner: f.child,
                    subaccount: None,
                },
                start: None,
                max_results: 100_u64.into(),
            },
        )?;
        let page = page.map_err(|e| anyhow::anyhow!("child token Index: {}", e.message))?;
        child_receipt = page.transactions.into_iter().find(|tx| {
            tx.id > funded_at
                && tx.transaction.transfer.as_ref().is_some_and(|transfer| {
                    transfer.from
                        == Account {
                            owner: f.child,
                            subaccount: None,
                        }
                        && transfer.to
                            == Account {
                                owner: f.operator,
                                subaccount: None,
                            }
                        && transfer.amount > 0_u8
                })
        });
        if child_receipt.is_some() {
            break;
        }
    }
    assert!(child_receipt.is_some(), "actual child-to-contributor Ledger transfer is required; a concurrent canonical Relay receipt cannot substitute");
    assert!(
        support::ledger::icrc1_balance(
            f.pic,
            f.ledger,
            &Account {
                owner: f.operator,
                subaccount: None
            }
        )? > child_jup_before,
        "actual immutable child must deliver economical jUP to the recognised external contributor"
    );
    assert_eq!(
        reserve_before - support::ledger::icrc1_balance(f.pic, f.ledger, &reserve)?,
        total_spent,
        "later sweeps and snapshots cannot issue another reserve pot for a processed endowment"
    );
    assert_eq!(
        f.pic
            .canister_status(f.child, Some(f.operator))
            .unwrap()
            .module_hash,
        child_hash
    );
    assert!(f
        .pic
        .canister_status(f.child, Some(f.operator))
        .unwrap()
        .settings
        .controllers
        .is_empty());
    Ok(pots.iter().map(|p| p.contribution_block).collect())
}
