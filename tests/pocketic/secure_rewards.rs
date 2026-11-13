//! Actual Relay boundary and reserve payments; no contributor-allocation oracle.
use super::*;
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsArgs, GetAccountIdentifierTransactionsResult, IndexOperation,
};
use std::time::Duration;

#[derive(CandidType)]
struct RelayInit {
    managed_canisters: Vec<Principal>,
    ledger_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    sns_rewards_canister_id: Option<Principal>,
    icp_index_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    surplus_neuron_recipients: Vec<candid::Empty>,
}
impl Env {
    fn install_relay(&self) -> Result<()> {
        self.install_relay_at(self.relay)
    }
    fn install_relay_at(&self, relay: Principal) -> Result<()> {
        self.pic.install_canister(
            relay,
            self.relay_wasm.clone(),
            encode_one(RelayInit {
                managed_canisters: vec![],
                ledger_canister_id: Some(support::principals::icp_ledger()),
                governance_canister_id: Some(self.forwarding_governance),
                sns_rewards_canister_id: Some(self.rewards),
                icp_index_canister_id: Some(support::principals::icp_index()),
                main_interval_seconds: Some(86_400),
                surplus_neuron_recipients: vec![],
            })?,
            None,
        );
        Ok(())
    }
    fn icp_send(
        &self,
        from: Principal,
        from_subaccount: Option<[u8; 32]>,
        to: Account,
        amount: u64,
        memo: Option<Vec<u8>>,
    ) -> Result<u64> {
        support::ledger::icrc1_transfer(
            &self.pic,
            support::principals::icp_ledger(),
            from,
            TransferArg {
                from_subaccount,
                to,
                amount: amount.into(),
                fee: Some(10_000_u64.into()),
                memo: memo.map(Into::into),
                created_at_time: None,
            },
        )
    }
    fn relay_funding(&self, from: Principal, amount: u64) -> Result<u64> {
        self.icp_send(
            Principal::anonymous(),
            None,
            Account {
                owner: from,
                subaccount: None,
            },
            amount + 10_000,
            None,
        )?;
        self.icp_send(
            from,
            None,
            Account {
                owner: self.relay,
                subaccount: jupiter_memo_policy::numbered_subaccount(1),
            },
            amount,
            None,
        )
    }
    fn relay_tick(&self, sweep: bool) -> Result<()> {
        if self.production {
            self.pic
                .advance_time(Duration::from_secs(if sweep { 8 * 86_400 } else { 86_401 }));
            for _ in 0..200 {
                self.pic.tick();
            }
            Ok(())
        } else {
            update_noargs(
                &self.pic,
                self.relay,
                self.a,
                if sweep {
                    "debug_reward_sweep"
                } else {
                    "debug_main_tick"
                },
            )
        }
    }
    fn staking_transfers(&self) -> Result<Vec<jupiter_ic_clients::index::IndexTransactionWithId>> {
        let reply: GetAccountIdentifierTransactionsResult = query_one(
            &self.pic,
            support::principals::icp_index(),
            self.a,
            "get_account_identifier_transactions",
            GetAccountIdentifierTransactionsArgs {
                account_identifier: jupiter_ic_clients::account_identifier::account_identifier_text(
                    self.staking.owner,
                    self.staking.subaccount,
                ),
                start: None,
                max_results: 100,
            },
        )?;
        match reply {
            GetAccountIdentifierTransactionsResult::Ok(page) => Ok(page.transactions),
            _ => anyhow::bail!("ICP Index query"),
        }
    }
    fn await_record(&self, block: u64) -> Result<PatronRecord> {
        for _ in 0..120 {
            self.pic.advance_time(Duration::from_secs(2));
            self.pic.tick();
            self.tick()?;
            if let Some(record) = self.record(block)? {
                return Ok(record);
            }
        }
        anyhow::bail!("missing record {block}: {:?}", self.status()?)
    }
}

#[test]
#[ignore = "unavailable real IO mirror with independent jUP recognition and payment"]
fn stopped_io_mirror_cannot_suppress_jup_snapshot_or_direct_payment() -> Result<()> {
    let env = Env::new(true)?;
    env.drive(20)?;
    let maintenance: AutomationState =
        query_one(&env.pic, env.rewards, env.a, "get_automation_status", ())?;
    assert!(maintenance.funding.is_none() && maintenance.pending.is_empty());
    let mut config = env.status()?.config.unwrap();
    config.version += 1;
    config.patron_enabled = true;
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                reward_sns_root_canister_id: None,
                programme: Some(config),
            }))?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("combined programme fixture: {e:?}"))?;
    env.pic
        .stop_canister(env.target["governance"], Some(env.target["root"]))
        .map_err(|e| anyhow!("stop independent IO mirror: {e:?}"))?;
    env.drive(12)?;
    let context = env.relay_context()?;
    env.assert_relay_owners(&context)?;
    let block = env.commit(env.b)?;
    let record = env.await_record(block)?;
    assert_eq!(record.route, PatronRoute::Direct);
    assert!(
        matches!(record.outcome, PatronOutcome::Verified { .. }),
        "{record:?}"
    );
    assert_eq!(record.amount_e8s, 100_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        100_000_000
    );
    let unavailable: AutomationState =
        query_one(&env.pic, env.rewards, env.a, "get_automation_status", ())?;
    assert!(
        unavailable.last_error.is_some(),
        "mirror dependency failure remains separately visible"
    );
    Ok(())
}

#[test]
#[ignore = "official Ledger upgrade fixture and actual direct/Relay receipts"]
fn native_fee_change_after_discovery_is_used_by_reserve_and_immutable_relay() -> Result<()> {
    let env = Env::setup_with_economics(false, false, false, false, true)?;
    env.install_relay()?;
    env.drive(6)?;
    let prepared_generation = env.relay_context()?.snapshot_id;
    let change = env.manage(
        env.jup["governance"],
        env.rewards,
        &env.jup_neurons["proposer"],
        sns::Command::MakeProposal(sns::Proposal {
            title: "Local authoritative Ledger fee change".into(),
            summary: "Exercise dynamic fee queries after ownership discovery".into(),
            url: String::new(),
            action: Some(sns::Action::ManageLedgerParameters(
                sns::ManageLedgerParameters {
                    transfer_fee: Some(2_000_000),
                    token_name: None,
                    token_symbol: None,
                    token_logo: None,
                },
            )),
        }),
    )?;
    let sns::Command1::MakeProposal(change) = change else {
        anyhow::bail!("fee proposal")
    };
    let proposal_id = change.proposal_id.clone();
    env.manage(
        env.jup["governance"],
        Principal::self_authenticating(b"NNX ROLE FIXTURE ONLY"),
        &env.jup_neurons["nnx_fixture"],
        sns::Command::RegisterVote(sns::RegisterVote {
            proposal: change.proposal_id,
            vote: 1,
        }),
    )?;
    for _ in 0..30 {
        env.pic.tick();
    }
    let failed: sns::GetProposalResponse = query_one(
        &env.pic,
        env.jup["governance"],
        env.a,
        "get_proposal",
        sns::GetProposal { proposal_id },
    )?;
    let Some(sns::Result1::Proposal(failed)) = failed.result else {
        anyhow::bail!("native fee proposal state")
    };
    assert!(failed.failed_timestamp_seconds > 0 && failed.failure_reason.is_some());
    eprintln!(
        "direct-genesis fee proposal dependency boundary: {:?}",
        failed.failure_reason
    );
    // This direct-genesis fixture has no SNS-W Wasm registry. The authentic
    // native governed update is tested in sale_rehearsal. Here Root upgrades
    // the real pinned Ledger locally to exercise authoritative changed-fee
    // payments, without claiming a production governance workflow.
    #[derive(CandidType)]
    struct FeeUpgrade {
        transfer_fee: Option<candid::Nat>,
    }
    #[derive(CandidType)]
    enum LedgerArgument {
        Upgrade(Option<FeeUpgrade>),
    }
    let repo = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let artifacts = PathBuf::from(command(
        Command::new("python3")
            .args(["tools/acceptance/artifacts.py", "--fetch"])
            .current_dir(&repo),
    )?);
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.jup["ledger"],
            fs::read(artifacts.join("ledger.wasm.gz"))?,
            encode_one(Some(LedgerArgument::Upgrade(Some(FeeUpgrade {
                transfer_fee: Some(2_000_000_u64.into()),
            }))))?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("owned Ledger fee fixture: {e:?}"))?;
    let actual_fee: candid::Nat = query_one(&env.pic, env.jup["ledger"], env.a, "icrc1_fee", ())?;
    assert_eq!(actual_fee, 2_000_000_u64);
    assert_eq!(env.relay_context()?.snapshot_id, prepared_generation);
    let reserve = Account {
        owner: env.rewards,
        subaccount: None,
    };
    let before = env.balance(reserve)?;
    let direct = env.commit(env.b)?;
    let paid = env.await_record(direct)?;
    assert!(matches!(paid.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(paid.amount_e8s, 100_000_000);
    assert_eq!(paid.fee_e8s, 2_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        100_000_000
    );
    env.relay_funding(env.b, 2_000_010_000)?;
    let mut credit = None;
    for _ in 0..30 {
        env.pic.advance_time(Duration::from_secs(2));
        for _ in 0..10 {
            env.pic.tick();
        }
        env.relay_tick(false)?;
        env.tick()?;
        let history = env.staking_transfers()?;
        if let Some(tx) = history.into_iter().find(|tx| matches!(&tx.transaction.operation, IndexOperation::Transfer { from, .. }
            if *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1)))) {
            credit = Some(tx);
            break;
        }
    }
    let tx = if let Some(tx) = credit {
        tx
    } else {
        let logs = env
            .pic
            .fetch_canister_logs(env.relay, Principal::anonymous())
            .map(|records| {
                records
                    .iter()
                    .map(|r| String::from_utf8_lossy(&r.content).into_owned())
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        anyhow::bail!("actual Relay staking credit missing: {logs:?}");
    };
    let pot = env.await_record(tx.id)?;
    assert!(matches!(pot.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(pot.amount_e8s, 200_000_000);
    assert_eq!(pot.fee_e8s, 2_000_000);
    assert_eq!(before - env.balance(reserve)?, 304_000_000);
    env.pic.advance_time(Duration::from_secs(86_401));
    env.drive(6)?;
    for _ in 0..8 {
        env.relay_tick(true)?;
    }
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        298_000_000,
        "direct net quote plus actual Relay pot less its authoritative downstream fee"
    );
    assert_eq!(
        before - env.balance(reserve)?,
        304_000_000,
        "no fee change or sweep may repeat reserve issuance"
    );
    Ok(())
}

#[test]
#[ignore = "production SNS Rewards and immutable Relay, official SNS/ICP Ledgers and Indexes"]
fn production_relay_endowment_receives_base_pot_and_distributes_actual_jup() -> Result<()> {
    let env = Env::setup_programme(false, false, false, true)?;
    env.install_relay()?;
    env.drive(6)?;
    let reserve = Account {
        owner: env.rewards,
        subaccount: None,
    };
    let before = env.balance(reserve)?;
    env.relay_funding(env.b, 2_000_010_000)?;
    env.relay_tick(false)?;
    for _ in 0..20 {
        env.tick()?;
    }
    let transfers = env.staking_transfers()?;
    let endowment = transfers.iter().find(|tx| matches!(&tx.transaction.operation, IndexOperation::Transfer { from, .. }
        if *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1))))
        .context("real Relay forwarding Ledger credit")?;
    let record = env.await_record(endowment.id)?;
    assert!(
        matches!(record.outcome, PatronOutcome::Verified { .. }),
        "{record:?}"
    );
    assert_eq!(
        record.route,
        PatronRoute::RelayPot {
            relay: env.relay,
            factory_setup_key: None
        }
    );
    assert!(record.recognition.is_none());
    assert!(record.compounded_neuron.is_none());
    assert_eq!(record.qualifying_icp_e8s, 2_000_000_000);
    assert_eq!(record.amount_e8s, 200_000_000);
    assert_eq!(
        record.to,
        Account {
            owner: env.relay,
            subaccount: None
        }
    );
    assert_eq!(before - env.balance(reserve)?, 201_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        0
    );
    // A later complete snapshot sees the real endowment before allocation.
    env.pic.advance_time(Duration::from_secs(86_401));
    env.drive(6)?;
    for _ in 0..8 {
        env.relay_tick(true)?;
        if env.balance(Account {
            owner: env.b,
            subaccount: None,
        })? > 0
        {
            break;
        }
    }
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        199_000_000
    );
    assert_eq!(
        env.balance(Account {
            owner: env.relay,
            subaccount: None
        })?,
        0
    );
    env.drive(12)?;
    assert_eq!(env.record(endowment.id)?.unwrap(), record);
    assert_eq!(before - env.balance(reserve)?, 201_000_000);
    // A minimum real endowment earns its pot without a new application
    // threshold. The immutable Relay's own fee-efficiency rule still holds it.
    env.relay_funding(env.b, 100_010_000)?;
    let mut tiny = None;
    for _ in 0..3 {
        env.relay_tick(false)?;
        let history = env.staking_transfers()?;
        tiny = history.into_iter().find(|tx| matches!(&tx.transaction.operation,
            IndexOperation::Transfer { from, amount, .. } if amount.e8s() == 100_000_000
            && *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1))));
        if tiny.is_some() {
            break;
        }
    }
    let tiny = if let Some(tx) = tiny {
        tx
    } else {
        let logs = env
            .pic
            .fetch_canister_logs(env.relay, Principal::anonymous());
        anyhow::bail!("minimum actual Relay staking credit missing: {logs:?}");
    };
    let tiny_pot = env.await_record(tiny.id)?;
    assert_eq!(tiny_pot.amount_e8s, 10_000_000);
    assert!(matches!(tiny_pot.outcome, PatronOutcome::Verified { .. }));
    for _ in 0..2 {
        env.relay_tick(true)?;
    }
    assert_eq!(
        env.balance(Account {
            owner: env.relay,
            subaccount: None
        })?,
        10_000_000
    );
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        199_000_000,
        "0.1 jUP pot minus the actual 0.01 fee is below Relay's economical-recipient threshold"
    );
    assert_eq!(before - env.balance(reserve)?, 212_000_000);
    Ok(())
}

#[test]
#[ignore = "mixed direct, no-memo treasury and authentic Relay Ledger boundaries"]
fn direct_treasury_and_relay_history_have_distinct_single_payment_opportunities() -> Result<()> {
    let env = Env::setup_sources(false, false, false)?;
    env.install_relay()?;
    env.drive(6)?;
    let treasury = env.icp_send(
        Principal::anonymous(),
        None,
        env.staking,
        2_000_000_000,
        None,
    )?;
    let direct = env.commit(env.a)?;
    // A user naming Relay in the memo is still a direct reward only.
    env.icp_send(
        Principal::anonymous(),
        None,
        Account {
            owner: env.a,
            subaccount: None,
        },
        10_000,
        None,
    )?;
    let named = env.icp_send(
        env.a,
        None,
        env.staking,
        1_000_000_000,
        Some(format!("{}.Relay", env.relay.to_text().replace('-', "")).into_bytes()),
    )?;
    env.relay_funding(env.b, 2_000_010_000)?;
    env.relay_tick(false)?;
    for _ in 0..80 {
        env.pic.advance_time(Duration::from_secs(2));
        env.pic.tick();
        env.tick()?;
    }
    let direct_record = env.await_record(direct)?;
    let named_record = env.await_record(named)?;
    assert_eq!(direct_record.route, PatronRoute::Direct);
    assert_eq!(named_record.route, PatronRoute::Direct);
    assert_eq!(direct_record.amount_e8s, 200_000_000);
    assert_eq!(named_record.amount_e8s, 200_000_000);
    assert!(
        env.record(treasury)?.is_none(),
        "invalid memo advances without a reward opportunity"
    );
    let relay = env.staking_transfers()?.into_iter().find(|tx| matches!(&tx.transaction.operation, IndexOperation::Transfer { from, .. }
        if *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1))))
        .context("Relay staking credit")?;
    let relay_record = env.await_record(relay.id)?;
    assert!(matches!(relay_record.route, PatronRoute::RelayPot { .. }));
    assert_eq!(relay_record.amount_e8s, 200_000_000);
    assert_eq!(env.status()?.verified_e8s, 600_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.relay,
            subaccount: None
        })?,
        200_000_000
    );
    Ok(())
}

#[test]
#[ignore = "actual Relay/ICP endowments with the existing jUP transport fault boundary"]
fn relay_pot_unknown_paid_and_unpaid_dispatches_are_terminal_across_upgrade() -> Result<()> {
    for paid in [false, true] {
        let env = Env::setup_sources(false, true, false)?;
        env.install_relay()?;
        env.drive(6)?;
        // Arm the transport fault before any timer can assess the real credit.
        #[derive(CandidType)]
        enum Fault {
            AcceptThenTrap,
            RejectThenTrap,
        }
        let _: () = update_one(
            &env.pic,
            env.jup["ledger"],
            env.a,
            "debug_set_next_error",
            Some(if paid {
                Fault::AcceptThenTrap
            } else {
                Fault::RejectThenTrap
            }),
        )?;
        env.relay_funding(env.b, 2_000_010_000)?;
        env.relay_tick(false)?;
        for _ in 0..40 {
            env.pic.advance_time(Duration::from_secs(2));
            env.pic.tick();
        }
        let tx = env.staking_transfers()?.into_iter().find(|tx| matches!(&tx.transaction.operation, IndexOperation::Transfer { from, .. }
            if *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1))))
            .context("real fault-case Relay endowment")?;
        let record = env.await_record(tx.id)?;
        assert!(
            matches!(record.outcome, PatronOutcome::AssumedPaidUnverified { .. }),
            "{record:?}"
        );
        assert!(matches!(record.route, PatronRoute::RelayPot { .. }));
        assert!(record.compounded_neuron.is_none());
        let pot_balance = env.balance(Account {
            owner: env.relay,
            subaccount: None,
        })?;
        assert_eq!(pot_balance, if paid { 200_000_000 } else { 0 });
        env.pic.advance_time(Duration::from_secs(3600));
        for _ in 0..100 {
            env.pic.tick();
        }
        env.pic
            .upgrade_canister(
                env.rewards,
                env.rewards_wasm.clone(),
                encode_args(())?,
                Some(env.jup["root"]),
            )
            .map_err(|e| anyhow!("pot recovery upgrade: {e:?}"))?;
        env.drive(12)?;
        assert_eq!(env.record(tx.id)?.unwrap(), record);
        assert_eq!(
            env.balance(Account {
                owner: env.relay,
                subaccount: None
            })?,
            pot_balance
        );
        assert_eq!(env.status()?.assumed_e8s, 200_000_000);
        assert_eq!(env.status()?.verified_e8s, 0);
    }
    Ok(())
}

#[test]
#[ignore = "complete zero-owner official jUP snapshot still admits authentic aggregate funding"]
fn relay_pot_is_funded_with_zero_eligible_owners_and_remains_undistributed() -> Result<()> {
    let env = Env::setup_sources(false, false, false)?;
    env.install_relay()?;
    let mut config = env.status()?.config.context("programme fixture")?;
    config.version += 1;
    config
        .excluded_jup_neurons
        .extend(["a", "a_second", "b"].map(|name| env.jup_neurons[name].clone()));
    env.pic.advance_time(Duration::from_secs(3600));
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                reward_sns_root_canister_id: None,
                programme: Some(config),
            }))?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("zero eligible fixture: {e:?}"))?;
    env.drive(6)?;
    assert_eq!(
        env.status()?
            .snapshot
            .context("complete empty recognition")?
            .owners,
        0
    );
    let outsider = Principal::self_authenticating(b"no eligible jUP neuron");
    env.relay_funding(outsider, 2_000_010_000)?;
    env.relay_tick(false)?;
    for _ in 0..40 {
        env.pic.advance_time(Duration::from_secs(2));
        env.pic.tick();
    }
    let tx = env.staking_transfers()?.into_iter().find(|tx| matches!(&tx.transaction.operation, IndexOperation::Transfer { from, .. }
        if *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1))))
        .context("zero-owner Relay credit")?;
    let record = env.await_record(tx.id)?;
    assert!(matches!(record.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(record.amount_e8s, 200_000_000);
    env.pic.advance_time(Duration::from_secs(86_401));
    env.drive(6)?;
    env.relay_tick(true)?;
    assert_eq!(
        env.balance(Account {
            owner: env.relay,
            subaccount: None
        })?,
        200_000_000
    );
    assert_eq!(
        env.balance(Account {
            owner: outsider,
            subaccount: None
        })?,
        0
    );
    Ok(())
}

#[test]
#[ignore = "real ICP Ledger archiving and Index ingestion before reserve assessment"]
fn archived_relay_staking_credit_is_funded_once_at_its_original_timestamp() -> Result<()> {
    let env = Env::setup_sources(false, false, false)?;
    let mut disabled = env.status()?.config.context("archive fixture config")?;
    disabled.version += 1;
    disabled.patron_enabled = false;
    disabled.launch_timestamp_seconds =
        Some(env.pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000);
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                reward_sns_root_canister_id: None,
                programme: Some(disabled.clone()),
            }))?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("disable before historical archive: {e:?}"))?;
    env.install_relay()?;
    env.drive(6)?;
    env.relay_funding(env.b, 2_000_010_000)?;
    env.relay_tick(false)?;
    let mut original = None;
    for _ in 0..80 {
        env.pic.advance_time(Duration::from_secs(1));
        env.pic.tick();
        original = env.staking_transfers()?.into_iter().find(|tx| matches!(&tx.transaction.operation,
            IndexOperation::Transfer { from, .. } if *from == jupiter_ic_clients::account_identifier::account_identifier_text(env.relay, jupiter_memo_policy::numbered_subaccount(1))));
        if original.is_some() {
            break;
        }
    }
    let original = original.context("original canonical endowment")?;
    assert!(env.record(original.id)?.is_none());
    #[derive(CandidType)]
    struct Request {
        start: u64,
        length: u64,
    }
    #[derive(CandidType, Deserialize, Debug)]
    struct Range {
        start: u64,
        length: u64,
    }
    #[derive(CandidType, Deserialize, Debug)]
    struct Page {
        blocks: Vec<candid::Reserved>,
        archived_blocks: Vec<Range>,
    }
    let ballast = Account {
        owner: Principal::self_authenticating(b"irrelevant ICP archive traffic"),
        subaccount: None,
    };
    let mut archived = false;
    for i in 0_u64..8_000 {
        env.icp_send(
            Principal::anonymous(),
            None,
            ballast,
            1,
            Some(i.to_be_bytes().to_vec()),
        )?;
        if i % 250 == 249 {
            env.pic.advance_time(Duration::from_secs(2));
            for _ in 0..20 {
                env.pic.tick();
            }
            let page: Page = query_one(
                &env.pic,
                support::principals::icp_ledger(),
                env.a,
                "query_blocks",
                Request {
                    start: original.id,
                    length: 1,
                },
            )?;
            if page.blocks.is_empty()
                && page
                    .archived_blocks
                    .iter()
                    .any(|r| r.start <= original.id && original.id - r.start < r.length)
            {
                archived = true;
                break;
            }
        }
    }
    assert!(archived, "fixture must demonstrate a real archive callback range, not pretend old live blocks were archived");
    disabled.version += 1;
    disabled.patron_enabled = true;
    // Assessment occurs a full halving later; the archived commitment's own
    // first-day timestamp must still earn 2 jUP, rather than the current 1 jUP.
    env.pic
        .advance_time(Duration::from_secs(1_461 * 86_400 + 3600));
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                reward_sns_root_canister_id: None,
                programme: Some(disabled),
            }))?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("activate complete archived history: {e:?}"))?;
    let record = env.await_record(original.id)?;
    assert!(matches!(record.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(record.amount_e8s, 200_000_000);
    assert_eq!(
        record.commitment_timestamp_seconds,
        original
            .transaction
            .timestamp
            .context("canonical timestamp")?
            .timestamp_nanos
            / 1_000_000_000
    );
    env.drive(20)?;
    assert_eq!(env.record(original.id)?.unwrap(), record);
    assert_eq!(env.status()?.verified_e8s, 200_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.relay,
            subaccount: None
        })?,
        200_000_000
    );
    Ok(())
}

// The factory replies below are a controlled dependency boundary. Actual
// Historian Creating/Paused/ManualRecoveryRequired semantics are tested in its
// native membership test; Governance, both Ledgers/Indices and Relay code here
// are real. No mock status is presented as production factory provenance.
#[derive(CandidType)]
enum MembershipFixture {
    Pending,
    Active { setup_key: Vec<u8> },
    Page { cursor: Vec<u8>, revision: u64 },
    RegistryChanged,
}
impl Env {
    fn membership_fixture(&self, relay: Principal, reply: MembershipFixture) -> Result<()> {
        support::calls::update_bytes(
            &self.pic,
            self.factory.context("factory fixture")?,
            self.a,
            "debug_set_relay_membership",
            encode_args((relay, reply))?,
        )
    }
    fn factory_child_fixture(&self) -> Result<Principal> {
        let child = self.pic.create_canister();
        self.pic.add_cycles(child, 5_000_000_000_000);
        self.install_relay_at(child)?;
        Ok(child)
    }
    fn actual_relay_endowment(&self, relay: Principal, net: u64) -> Result<u64> {
        self.icp_send(
            Principal::anonymous(),
            None,
            Account {
                owner: relay,
                subaccount: jupiter_memo_policy::numbered_subaccount(1),
            },
            net + 10_000,
            None,
        )?;
        let _: () = update_noargs(&self.pic, relay, self.a, "debug_main_tick")?;
        let source = jupiter_ic_clients::account_identifier::account_identifier_text(
            relay,
            jupiter_memo_policy::numbered_subaccount(1),
        );
        for _ in 0..60 {
            self.pic.advance_time(Duration::from_secs(2));
            self.pic.tick();
            if let Some(tx) = self.staking_transfers()?.into_iter().find(|tx| {
                matches!(
                    &tx.transaction.operation, IndexOperation::Transfer { from, amount, .. }
                    if *from == source && amount.e8s() == net
                )
            }) {
                return Ok(tx.id);
            }
        }
        anyhow::bail!("missing actual Relay staking credit: {relay}")
    }
    fn enable_factory_patron(&self) -> Result<()> {
        self.drive(12)?;
        let mut config = self.status()?.config.unwrap();
        assert!(config.relay_factory.is_some() && !config.patron_enabled);
        config.version += 1;
        config.patron_enabled = true;
        for _ in 0..100 {
            self.pic.tick();
        }
        self.pic
            .upgrade_canister(
                self.rewards,
                self.rewards_wasm.clone(),
                encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                    reward_sns_root_canister_id: None,
                    programme: Some(config),
                }))?,
                Some(self.jup["root"]),
            )
            .map_err(|e| anyhow!("enable isolated factory fixture: {e:?}"))?;
        Ok(())
    }
    fn await_verified(&self, block: u64) -> Result<PatronRecord> {
        for _ in 0..120 {
            self.pic.advance_time(Duration::from_secs(2));
            self.pic.tick();
            self.tick()?;
            if let Some(record) = self
                .record(block)?
                .filter(|r| matches!(r.outcome, PatronOutcome::Verified { .. }))
            {
                return Ok(record);
            }
        }
        anyhow::bail!(
            "unverified {block}: {:?}; {:?}",
            self.record(block)?,
            self.status()?
        )
    }
}

#[test]
#[ignore = "official SNS/ICP and real Relay endowments with bounded pending-factory isolation"]
fn pending_factory_membership_cannot_block_history_or_another_child_recovery() -> Result<()> {
    let env = Env::setup_with_factory(false, false, false, false, false, true)?;
    env.install_relay()?;
    let permanent = env.factory_child_fixture()?;
    let recoverable = env.factory_child_fixture()?;
    // Descending primary history encounters the unresolved children first.
    let direct = env.commit(env.b)?;
    let canonical = env.actual_relay_endowment(env.relay, 1_000_000_000)?;
    let permanent_block = env.actual_relay_endowment(permanent, 2_000_000_000)?;
    let recoverable_block = env.actual_relay_endowment(recoverable, 3_000_000_000)?;
    env.membership_fixture(permanent, MembershipFixture::Pending)?;
    env.membership_fixture(
        recoverable,
        MembershipFixture::Page {
            cursor: vec![7; 32],
            revision: 1,
        },
    )?;
    env.enable_factory_patron()?;
    assert_eq!(env.await_verified(direct)?.route, PatronRoute::Direct);
    assert_eq!(env.await_verified(canonical)?.amount_e8s, 100_000_000);
    env.drive(24)?;
    let before = env
        .record(recoverable_block)?
        .context("deferred canonical source proof")?;
    assert!(
        matches!(&before.outcome, PatronOutcome::AwaitingRelayMembership { lookup: Some(l), .. } if l.cursor == vec![7; 32])
    );
    assert_eq!(before.qualifying_icp_e8s, 3_000_000_000);
    assert_eq!(before.amount_e8s, 0, "membership is not funded eligibility");
    assert_eq!(env.status()?.relay_retries.unwrap().outstanding, 2);
    assert!(env
        .status()?
        .completed_end_block
        .is_some_and(|n| n >= recoverable_block));
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_args(())?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("deferred source/lookup upgrade: {e:?}"))?;
    assert_eq!(env.record(recoverable_block)?.unwrap(), before);
    assert_eq!(env.status()?.relay_retries.unwrap().outstanding, 2);

    // An unavailable registry also cannot suppress a new independent batch.
    let factory = env.factory.unwrap();
    env.pic
        .stop_canister(factory, None)
        .map_err(|e| anyhow!("stop factory fixture: {e:?}"))?;
    let later_direct = env.commit(env.b)?;
    env.await_verified(later_direct)?;
    env.pic
        .start_canister(factory, None)
        .map_err(|e| anyhow!("start factory fixture: {e:?}"))?;
    env.membership_fixture(recoverable, MembershipFixture::RegistryChanged)?;
    env.drive(24)?;
    assert!(matches!(
        env.record(recoverable_block)?.unwrap().outcome,
        PatronOutcome::AwaitingRelayMembership { lookup: None, .. }
    ));
    env.membership_fixture(recoverable, MembershipFixture::Pending)?;
    env.drive(24)?;
    assert!(env.status()?.batch.is_none());
    // An unresolved, undispatched opportunity does not prevent a governed
    // configuration change. Its original launch/rate inputs remain frozen.
    let mut next = env.status()?.config.unwrap();
    next.version += 1;
    next.launch_timestamp_seconds = Some(0);
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                reward_sns_root_canister_id: None,
                programme: Some(next),
            }))?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("deferred configuration/rate preservation: {e:?}"))?;
    env.drive(12)?;
    let reserve = Account {
        owner: env.rewards,
        subaccount: None,
    };
    let escrow = Account {
        owner: env.io_only,
        subaccount: None,
    };
    assert_eq!(env.balance(escrow)?, 0);
    let available = env.balance(reserve)?;
    // Controlled Ledger balance fixture, not a production transfer endpoint.
    support::ledger::icrc1_transfer(
        &env.pic,
        env.jup["ledger"],
        env.rewards,
        TransferArg {
            from_subaccount: None,
            to: escrow,
            amount: (available - 1_000_000).into(),
            fee: Some(1_000_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    env.membership_fixture(
        recoverable,
        MembershipFixture::Active {
            setup_key: vec![8; 32],
        },
    )?;
    env.drive(12)?;
    let unfunded = env.record(recoverable_block)?.unwrap();
    assert!(
        matches!(unfunded.outcome, PatronOutcome::AwaitingRelayMembership { ref reason, .. }
        if reason.contains("reserve funds unavailable"))
    );
    assert!(env.status()?.pending_block.is_none());
    assert_eq!(env.status()?.relay_retries.unwrap().outstanding, 2);
    assert_eq!(
        env.balance(Account {
            owner: recoverable,
            subaccount: None
        })?,
        0
    );
    let returnable = env.balance(escrow)?;
    support::ledger::icrc1_transfer(
        &env.pic,
        env.jup["ledger"],
        env.io_only,
        TransferArg {
            from_subaccount: None,
            to: reserve,
            amount: (returnable - 1_000_000).into(),
            fee: Some(1_000_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    let reserve_before = env.balance(Account {
        owner: env.rewards,
        subaccount: None,
    })?;
    env.membership_fixture(
        recoverable,
        MembershipFixture::Active {
            setup_key: vec![8; 32],
        },
    )?;
    let funded = env.await_verified(recoverable_block)?;
    assert_eq!(
        funded.route,
        PatronRoute::RelayPot {
            relay: recoverable,
            factory_setup_key: Some(vec![8; 32])
        }
    );
    assert_eq!(funded.qualifying_icp_e8s, before.qualifying_icp_e8s);
    assert_eq!(
        funded.commitment_timestamp_seconds,
        before.commitment_timestamp_seconds
    );
    assert_eq!(
        funded.launch_timestamp_seconds,
        before.launch_timestamp_seconds
    );
    assert_ne!(
        funded.launch_timestamp_seconds,
        env.status()?
            .config
            .unwrap()
            .launch_timestamp_seconds
            .unwrap()
    );
    assert_eq!(funded.commitment_memo, before.commitment_memo);
    assert_eq!(funded.amount_e8s, 300_000_000);
    assert_eq!(funded.fee_e8s, 1_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.rewards,
            subaccount: None
        })?,
        reserve_before - 301_000_000
    );
    assert_eq!(
        env.balance(Account {
            owner: recoverable,
            subaccount: None
        })?,
        300_000_000
    );
    assert_eq!(env.status()?.relay_retries.unwrap().outstanding, 1);
    assert!(matches!(
        env.record(permanent_block)?.unwrap().outcome,
        PatronOutcome::AwaitingRelayMembership { .. }
    ));
    assert_eq!(
        env.balance(Account {
            owner: permanent,
            subaccount: None
        })?,
        0
    );
    let completed = env.status()?.completed_end_block;
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_args(())?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("confirmed retry no-replay upgrade: {e:?}"))?;
    env.drive(60)?;
    assert_eq!(env.record(recoverable_block)?.unwrap(), funded);
    assert_eq!(env.status()?.completed_end_block, completed);
    assert_eq!(env.status()?.verified_e8s, 600_000_000);
    assert_eq!(
        env.balance(Account {
            owner: recoverable,
            subaccount: None
        })?,
        300_000_000
    );
    Ok(())
}

#[test]
#[ignore = "official token Ledger callback trap on a recovered Relay opportunity survives upgrade without replay"]
fn deferred_relay_dispatch_checkpoint_recovers_without_rewinding_or_repaying() -> Result<()> {
    let env = Env::setup_with_factory(false, false, false, false, false, true)?;
    let child = env.factory_child_fixture()?;
    let block = env.actual_relay_endowment(child, 1_000_000_000)?;
    env.membership_fixture(child, MembershipFixture::Pending)?;
    env.enable_factory_patron()?;
    let deferred = env.await_record(block)?;
    assert!(matches!(
        deferred.outcome,
        PatronOutcome::AwaitingRelayMembership { .. }
    ));
    env.drive(12)?;
    env.membership_fixture(
        child,
        MembershipFixture::Active {
            setup_key: vec![9; 32],
        },
    )?;
    let _: () = update_noargs(
        &env.pic,
        env.rewards,
        env.a,
        "debug_trap_patron_callback_once",
    )?;
    for _ in 0..100 {
        if let Err(error) = env.tick() {
            assert!(error.to_string().contains("callback trap"), "{error:?}");
            break;
        }
    }
    let checkpoint = env.record(block)?.unwrap();
    assert!(
        matches!(checkpoint.outcome, PatronOutcome::Dispatched { .. }),
        "{checkpoint:?}"
    );
    assert_eq!(
        env.status()?.relay_retries.unwrap().dispatching_block,
        Some(block)
    );
    assert_eq!(
        env.balance(Account {
            owner: child,
            subaccount: None
        })?,
        100_000_000
    );
    for _ in 0..100 {
        env.pic.tick();
    }
    env.pic
        .upgrade_canister(
            env.rewards,
            env.rewards_wasm.clone(),
            encode_args(())?,
            Some(env.jup["root"]),
        )
        .map_err(|e| anyhow!("retry dispatch callback upgrade: {e:?}"))?;
    let recovered = env.record(block)?.unwrap();
    assert!(matches!(
        recovered.outcome,
        PatronOutcome::AssumedPaidUnverified { .. }
    ));
    assert_eq!(env.status()?.relay_retries.unwrap().outstanding, 0);
    assert_eq!(env.status()?.assumed_e8s, 100_000_000);
    assert_eq!(env.status()?.verified_e8s, 0);
    let direct = env.commit(env.b)?;
    env.await_verified(direct)?;
    env.drive(20)?;
    assert_eq!(env.record(block)?.unwrap(), recovered);
    assert_eq!(
        env.balance(Account {
            owner: child,
            subaccount: None
        })?,
        100_000_000
    );
    assert_eq!(env.status()?.verified_e8s, 100_000_000);
    Ok(())
}
