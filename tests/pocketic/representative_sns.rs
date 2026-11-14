//! Two authentic official SNS instances; local ICP is real, application is tiny.
//! Genesis uses contested fixture voting weights, not a completed production sale.
use super::*;
use anyhow::{anyhow, Context};
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferArg};
use jupiter_sns_rewards::{generated as sns, *};
use pocket_ic::PocketIc;
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

static RELAY_DEBUG: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_PRODUCTION: OnceLock<Vec<u8>> = OnceLock::new();
static NNS_BOUNDARY: OnceLock<Vec<u8>> = OnceLock::new();

struct Env {
    pic: PocketIc,
    rewards: Principal,
    relay: Principal,
    production: bool,
    relay_wasm: Vec<u8>,
    forwarding_governance: Principal,
    jup: BTreeMap<String, Principal>,
    target: BTreeMap<String, Principal>,
    jup_neurons: BTreeMap<String, Vec<u8>>,
    target_neurons: BTreeMap<String, Vec<u8>>,
    a: Principal,
    b: Principal,
    io_only: Principal,
    staking: Account,
    rewards_wasm: Vec<u8>,
    app: Principal,
    factory: Option<Principal>,
}

fn command(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

impl Env {
    fn new(automation: bool) -> Result<Self> {
        Self::setup(automation, false)
    }
    fn setup(automation: bool, fault_ledger: bool) -> Result<Self> {
        Self::setup_sources(automation, fault_ledger, true)
    }
    fn setup_sources(automation: bool, fault_ledger: bool, install_io: bool) -> Result<Self> {
        Self::setup_programme(automation, fault_ledger, install_io, false)
    }
    fn setup_programme(
        automation: bool,
        fault_ledger: bool,
        install_io: bool,
        production: bool,
    ) -> Result<Self> {
        Self::setup_with_economics(automation, fault_ledger, install_io, production, false)
    }
    fn setup_with_economics(
        automation: bool,
        fault_ledger: bool,
        install_io: bool,
        production: bool,
        native_economics: bool,
    ) -> Result<Self> {
        Self::setup_with_factory(
            automation,
            fault_ledger,
            install_io,
            production,
            native_economics,
            false,
        )
    }
    fn setup_with_factory(
        automation: bool,
        fault_ledger: bool,
        install_io: bool,
        production: bool,
        native_economics: bool,
        factory_fixture: bool,
    ) -> Result<Self> {
        // Compile every application before starting the owned server.
        let rewards_wasm = if production {
            wasm(&PRODUCTION_REWARDS_WASM, "jupiter-sns-rewards", None)?
        } else {
            wasm(&REWARDS_WASM, "jupiter-sns-rewards", Some("debug_api"))?
        };
        let relay_wasm = support::wasm::build_wasm_cached_for_test(
            if production {
                &RELAY_PRODUCTION
            } else {
                &RELAY_DEBUG
            },
            "jupiter-relay",
            if production { None } else { Some("debug_api") },
        )?;
        let nns_wasm =
            support::wasm::build_wasm_cached_for_test(&NNS_BOUNDARY, "mock-nns-governance", None)?;
        let app_wasm =
            support::wasm::build_wasm_cached_for_test(&OnceLock::new(), "mock-cycle-burner", None)?;
        let fault_wasm = if fault_ledger {
            Some(wasm(&LEDGER_WASM, "mock-icrc-ledger", None)?)
        } else {
            None
        };
        let factory_wasm = if factory_fixture {
            Some(wasm(&ROOT_WASM, "mock-sns-root", None)?)
        } else {
            None
        };
        let repo = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
        let artifact = command(
            Command::new("python3")
                .args(["tools/acceptance/artifacts.py", "--fetch"])
                .current_dir(&repo),
        )?;
        let artifacts = PathBuf::from(artifact);
        let pic = support::pocketic::sns_topology_builder()
            .with_icp_features(pocket_ic::common::rest::IcpFeatures {
                registry: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
                icp_token: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
                ..Default::default()
            })
            .build();
        let allocate = || {
            let id = pic.create_canister();
            pic.add_cycles(id, 5_000_000_000_000);
            id
        };
        let rewards = allocate();
        let app = allocate();
        let relay = allocate();
        let factory = factory_wasm.map(|module| {
            let id = allocate();
            pic.install_canister(id, module, encode_args(()).unwrap(), None);
            id
        });
        let forwarding_governance = support::principals::nns_governance();
        pic.create_canister_with_id(None, None, forwarding_governance)
            .map_err(anyhow::Error::msg)?;
        pic.add_cycles(forwarding_governance, 5_000_000_000_000);
        pic.install_canister(forwarding_governance, nns_wasm, encode_args(())?, None);
        let _: () = support::calls::update_bytes(
            &pic,
            forwarding_governance,
            Principal::anonymous(),
            "debug_set_neuron_account",
            encode_args((11_614_578_985_374_291_210_u64, vec![42_u8; 32]))?,
        )?;
        let jup: BTreeMap<String, Principal> = ["root", "governance", "ledger", "index", "swap"]
            .into_iter()
            .map(|role| (role.to_string(), allocate()))
            .collect();
        let target: BTreeMap<String, Principal> = ["root", "governance", "ledger", "index", "swap"]
            .into_iter()
            .map(|role| (role.to_string(), allocate()))
            .collect();
        let a = Principal::self_authenticating(b"representative owner a");
        let b = Principal::self_authenticating(b"representative owner b");
        let io_only = Principal::self_authenticating(b"representative IO-only owner");
        let fixture = Principal::self_authenticating(b"NNX ROLE FIXTURE ONLY");
        let directory = repo.join(format!(
            "target/representative-sns-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&directory)?;
        let context = serde_json::json!({ "jup": jup, "target": target, "rewards": rewards,
            "operator": Principal::self_authenticating(b"local constructor fallback fixture"), "a": a, "b": b, "io_only": io_only,
            "nnx_fixture": fixture, "app": app, "proposer_stake": if automation {10} else {100},
            "native_economics": native_economics,
            "timestamp": pic.get_time().as_nanos_since_unix_epoch()/1_000_000_000,
            "icp_ledger": support::principals::icp_ledger(), "nns_governance": support::principals::nns_governance() });
        let context_path = directory.join("context.json");
        fs::write(&context_path, serde_json::to_vec_pretty(&context)?)?;
        command(
            Command::new("python3")
                .arg("tools/acceptance/representative.py")
                .arg(&context_path)
                .arg(&artifacts)
                .arg(&directory)
                .current_dir(&repo),
        )?;
        for (name, ids) in [("jup", &jup), ("target", &target)] {
            if name == "target" && !install_io {
                continue;
            }
            for role in ["governance", "ledger", "index", "swap", "root"] {
                if name == "jup" && role == "ledger" && fault_ledger {
                    pic.install_canister(
                        ids[role],
                        fault_wasm.clone().unwrap(),
                        encode_args(())?,
                        None,
                    );
                    let _: () = support::calls::update_bytes(
                        &pic,
                        ids[role],
                        Principal::anonymous(),
                        "debug_credit",
                        encode_args((
                            Account {
                                owner: rewards,
                                subaccount: None,
                            },
                            1_000_000_000_000_u64,
                        ))?,
                    )?;
                    let _: () = update_one(
                        &pic,
                        ids[role],
                        Principal::anonymous(),
                        "debug_set_minting_account",
                        Account {
                            owner: jup["governance"],
                            subaccount: None,
                        },
                    )?;
                    let _: () = update_one(
                        &pic,
                        ids[role],
                        Principal::anonymous(),
                        "debug_set_fee",
                        1_000_000_u64,
                    )?;
                } else {
                    pic.install_canister(
                        ids[role],
                        fs::read(artifacts.join(format!("sns_{role}.wasm")))?,
                        fs::read(directory.join(name).join(format!("{role}.bin")))?,
                        None,
                    );
                }
            }
            for (role, controller) in [
                ("governance", ids["root"]),
                ("root", ids["governance"]),
                ("ledger", ids["root"]),
                ("index", ids["root"]),
                ("swap", ids["root"]),
            ] {
                pic.set_controllers(ids[role], None, vec![controller])
                    .map_err(|e| anyhow!("controllers: {e:?}"))?;
            }
        }
        pic.install_canister(app, app_wasm, encode_args(())?, None);
        pic.set_controllers(app, None, vec![target["root"]])
            .map_err(|e| anyhow!("app controller: {e:?}"))?;
        let neuron_ids = |name: &str| -> Result<BTreeMap<String, Vec<u8>>> {
            let values: BTreeMap<String, String> =
                serde_json::from_slice(&fs::read(directory.join(name).join("neuron-ids.json"))?)?;
            values
                .into_iter()
                .map(|(k, v)| Ok((k, hex::decode(v)?)))
                .collect()
        };
        let jup_neurons = neuron_ids("jup")?;
        let target_neurons = neuron_ids("target")?;
        let staking = Account {
            owner: forwarding_governance,
            subaccount: Some([42; 32]),
        };
        let config = ProgrammeConfig {
            version: 1,
            jup_root: jup["root"],
            relay_factory: factory,
            icp_ledger: support::principals::icp_ledger(),
            icp_index: support::principals::icp_index(),
            staking_account: staking,
            canonical_relay: relay,
            treasury_stake: None,
            protocol_governance_neuron: jup_neurons["nnx_fixture"].clone(),
            excluded_icp_accounts: vec![],
            excluded_jup_neurons: vec![
                jup_neurons["nnx_fixture"].clone(),
                jup_neurons["proposer"].clone(),
            ],
            reserve_subaccount: None,
            reserve_transport_neuron: None,
            launch_timestamp_seconds: Some(u64::MAX / 1_000_000_000),
            patron_enabled: !automation && !factory_fixture,
            snapshot_max_age_seconds: 172_800,
            automation: automation.then(|| AutomationConfig {
                proposer_neuron: jup_neurons["proposer"].clone(),
                targets: vec![MirrorTarget {
                    root: target["root"],
                    governance: target["governance"],
                    neuron: target_neurons["external"].clone(),
                    first_proposal: 1,
                    delivery_margin_seconds: 3600,
                }],
                treasury_enabled: false,
                treasury_retain_icp_e8s: 0,
                treasury_max_proposal_icp_e8s: 0,
            }),
        };
        pic.install_canister(
            rewards,
            rewards_wasm.clone(),
            encode_one(jupiter_sns_rewards::InitArgs {
                reward_sns_root_canister_id: None,
                programme: Some(config),
            })?,
            None,
        );
        // Application authority follows the existing SNS Root policy; no operator remains.
        pic.set_controllers(rewards, None, vec![jup["root"]])
            .map_err(|e| anyhow!("rewards controller: {e:?}"))?;
        Ok(Self {
            pic,
            rewards,
            relay,
            production,
            relay_wasm,
            forwarding_governance,
            jup,
            target,
            jup_neurons,
            target_neurons,
            a,
            b,
            io_only,
            staking,
            rewards_wasm,
            app,
            factory,
        })
    }
    fn relay_context(&self) -> Result<super::RelayRewardContext> {
        let context: Option<super::RelayRewardContext> = query_one(
            &self.pic,
            self.rewards,
            self.a,
            "get_relay_reward_context",
            (),
        )?;
        context.context("independent complete jUP snapshot")
    }
    fn assert_relay_owners(&self, context: &super::RelayRewardContext) -> Result<()> {
        use jupiter_ic_clients::account_identifier::account_identifier_bytes;
        let reply: ResolveDefaultIcpAccountsResult = query_one(
            &self.pic,
            self.rewards,
            self.a,
            "resolve_default_icp_accounts",
            ResolveDefaultIcpAccountsArgs {
                snapshot_id: context.snapshot_id,
                account_identifiers: [self.a, self.io_only]
                    .map(|p| account_identifier_bytes(p, None).to_vec())
                    .to_vec(),
            },
        )?;
        assert_eq!(
            reply,
            ResolveDefaultIcpAccountsResult::Ok(vec![Some(self.a), None]),
            "IO-only owners must not enter the independent jUP index"
        );
        assert_eq!(context.sns_root_canister_id, self.jup["root"]);
        assert_eq!(context.sns_ledger_canister_id, self.jup["ledger"]);
        Ok(())
    }
    fn tick(&self) -> Result<()> {
        if self.production {
            self.pic.advance_time(std::time::Duration::from_secs(31));
            for _ in 0..12 {
                self.pic.tick();
            }
            return Ok(());
        }
        update_noargs(
            &self.pic,
            self.rewards,
            Principal::anonymous(),
            "debug_programme_tick",
        )
    }
    fn status(&self) -> Result<Runtime> {
        query_one(
            &self.pic,
            self.rewards,
            Principal::anonymous(),
            "get_programme_status",
            (),
        )
    }
    fn drive(&self, count: usize) -> Result<()> {
        for _ in 0..count {
            self.tick()?;
        }
        Ok(())
    }
    fn manage(
        &self,
        governance: Principal,
        caller: Principal,
        id: &[u8],
        command: sns::Command,
    ) -> Result<sns::Command1> {
        let reply: sns::ManageNeuronResponse = update_one(
            &self.pic,
            governance,
            caller,
            "manage_neuron",
            sns::ManageNeuron {
                subaccount: id.to_vec(),
                command: Some(command),
            },
        )?;
        match reply.command {
            Some(sns::Command1::Error(e)) => anyhow::bail!("{}: {}", e.error_type, e.error_message),
            Some(reply) => Ok(reply),
            None => anyhow::bail!("missing manage response"),
        }
    }
    fn commit(&self, principal: Principal) -> Result<u64> {
        support::ledger::icrc1_transfer(
            &self.pic,
            support::principals::icp_ledger(),
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: principal,
                    subaccount: None,
                },
                amount: 2_000_010_000_u64.into(),
                fee: Some(10_000_u64.into()),
                memo: None,
                created_at_time: None,
            },
        )?;
        support::ledger::icrc1_transfer(
            &self.pic,
            support::principals::icp_ledger(),
            principal,
            TransferArg {
                from_subaccount: None,
                to: self.staking,
                amount: 1_000_000_000_u64.into(),
                fee: Some(10_000_u64.into()),
                memo: Some(self.rewards.to_text().into_bytes().into()),
                created_at_time: None,
            },
        )
    }
    fn record(&self, block: u64) -> Result<Option<PatronRecord>> {
        query_one(
            &self.pic,
            self.rewards,
            Principal::anonymous(),
            "get_patron_record",
            block,
        )
    }
    fn balance(&self, account: Account) -> Result<u64> {
        support::ledger::icrc1_balance(&self.pic, self.jup["ledger"], &account)
    }
}

#[test]
#[ignore = "two real official SNS instances and owned real ICP topology"]
fn jup_only_direct_patron_compounding_and_fixed_history() -> Result<()> {
    let env = Env::new(false)?;
    // Hold the unrelated IO mirror Root unavailable before the first complete snapshot.
    env.pic
        .stop_canister(env.target["root"], Some(env.target["governance"]))
        .map_err(|e| anyhow!("stop unrelated IO source: {e:?}"))?;
    let direct = env.commit(env.a)?;
    let liquid = env.commit(env.io_only)?;
    let unmatched = Principal::self_authenticating(b"no recognised neuron");
    let unmatched_block = env.commit(unmatched)?;
    for _ in 0..120 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.pic.tick();
        env.tick()?;
        if env.record(direct)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    let record = env
        .record(direct)?
        .context("historical direct contribution not processed")?;
    assert!(
        matches!(record.outcome, PatronOutcome::Verified { .. }),
        "{record:?}; {:?}",
        env.status()?
    );
    assert_eq!(record.amount_e8s, 200_000_000);
    assert_eq!(record.fee_e8s, 1_000_000);
    let expected = [&env.jup_neurons["a"], &env.jup_neurons["a_second"]]
        .into_iter()
        .min()
        .unwrap();
    assert_eq!(record.compounded_neuron.as_ref(), Some(expected));
    assert_eq!(
        record.refresh_result.as_deref(),
        Some("confirmed stake refresh")
    );
    let liquid_record = env.record(liquid)?.context("IO-only owner contribution")?;
    assert_eq!(liquid_record.amount_e8s, 0);
    assert!(matches!(
        liquid_record.outcome,
        PatronOutcome::Skipped { .. }
    ));
    assert!(liquid_record.compounded_neuron.is_none());
    assert_eq!(
        env.balance(Account {
            owner: env.io_only,
            subaccount: None
        })?,
        0
    );
    let skipped = env
        .record(unmatched_block)?
        .context("unmatched final decision")?;
    assert!(matches!(skipped.outcome, PatronOutcome::Skipped { .. }));
    assert_eq!(record.snapshot_id, liquid_record.snapshot_id);
    assert_eq!(record.ownership_snapshot.sources[0].root, env.jup["root"]);
    assert_eq!(record.ownership_snapshot.sources.len(), 1);
    assert_eq!(record.recognition.as_ref().unwrap().principal, env.a);
    assert!(skipped.recognition.is_none());
    assert_eq!(record.snapshot_id, skipped.snapshot_id);
    let snapshot = env.status()?.snapshot.unwrap();
    let context: Option<super::RelayRewardContext> = query_one(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "get_relay_reward_context",
        (),
    )?;
    assert_eq!(context.unwrap().sns_ledger_canister_id, env.jup["ledger"]);
    // Existing installation upgrade preserves the completed cohort/replay cursor.
    env.pic.advance_time(std::time::Duration::from_secs(3600));
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
        .map_err(|e| anyhow!("programme upgrade: {e:?}"))?;
    env.drive(8)?;
    assert_eq!(env.record(direct)?.unwrap(), record);
    assert_eq!(env.status()?.snapshot.unwrap().id, snapshot.id);
    assert_eq!(env.status()?.verified_e8s, 200_000_000);
    // An authentic ICRC-2 spender does not replace the economic source owner.
    // The Ledger's exact default `from` Account remains the direct contributor.
    let delegate =
        Principal::self_authenticating(b"approved ICP spender without a recognised neuron");
    support::ledger::icrc1_transfer(
        &env.pic,
        support::principals::icp_ledger(),
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: Account {
                owner: env.a,
                subaccount: None,
            },
            amount: 100_000_000_u64.into(),
            fee: Some(10_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    let approved: Result<candid::Nat, icrc_ledger_types::icrc2::approve::ApproveError> =
        update_one(
            &env.pic,
            support::principals::icp_ledger(),
            env.a,
            "icrc2_approve",
            icrc_ledger_types::icrc2::approve::ApproveArgs {
                from_subaccount: None,
                spender: Account {
                    owner: delegate,
                    subaccount: None,
                },
                amount: 1_000_010_000_u64.into(),
                expected_allowance: None,
                expires_at: None,
                fee: Some(10_000_u64.into()),
                memo: None,
                created_at_time: None,
            },
        )?;
    assert!(approved.is_ok(), "{approved:?}");
    let delegated: Result<candid::Nat, icrc_ledger_types::icrc2::transfer_from::TransferFromError> =
        update_one(
            &env.pic,
            support::principals::icp_ledger(),
            delegate,
            "icrc2_transfer_from",
            icrc_ledger_types::icrc2::transfer_from::TransferFromArgs {
                spender_subaccount: None,
                from: Account {
                    owner: env.a,
                    subaccount: None,
                },
                to: env.staking,
                amount: 1_000_000_000_u64.into(),
                fee: Some(10_000_u64.into()),
                memo: Some(env.rewards.to_text().into_bytes().into()),
                created_at_time: None,
            },
        )?;
    let delegated = u64::try_from(
        delegated
            .map_err(|e| anyhow!("real delegated commitment: {e:?}"))?
            .0,
    )?;
    for _ in 0..80 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.tick()?;
        if env.record(delegated)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    let delegated = env
        .record(delegated)?
        .context("canonical delegated commitment")?;
    assert!(matches!(delegated.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(delegated.amount_e8s, 200_000_000);
    assert_eq!(delegated.compounded_neuron.as_ref(), Some(expected));
    // The native preference can be revoked after discovery. Recheck the frozen
    // candidate before dispatch rather than treating the snapshot as a lock.
    for name in ["a", "a_second"] {
        env.manage(
            env.jup["governance"],
            env.a,
            &env.jup_neurons[name],
            sns::Command::Configure(sns::Configure {
                operation: Some(sns::Operation::ChangeAutoStakeMaturity(
                    sns::ChangeAutoStakeMaturity {
                        requested_setting_for_auto_stake_maturity: false,
                    },
                )),
            }),
        )?;
    }
    let later_direct = env.commit(env.a)?;
    env.pic
        .stop_canister(env.jup["governance"], Some(env.jup["root"]))
        .map_err(|e| anyhow!("stop selected-neuron lookup: {e:?}"))?;
    env.drive(6)?;
    assert!(
        env.record(later_direct)?.is_none(),
        "unavailable lookup cannot consume or classify the contribution"
    );
    env.pic
        .start_canister(env.jup["governance"], Some(env.jup["root"]))
        .map_err(|e| anyhow!("restore selected-neuron lookup: {e:?}"))?;
    for _ in 0..80 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.tick()?;
        if env.record(later_direct)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    let later = env
        .record(later_direct)?
        .context("preference-revoked contribution")?;
    assert!(matches!(later.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(later.amount_e8s, 100_000_000);
    assert!(later.compounded_neuron.is_none());
    // Re-enable both preferences, then share economic control only on the pinned
    // smallest candidate. Dispatch must fall back to liquid without rescanning
    // for the other still-secure compounding neuron or replaying an older block.
    for name in ["a", "a_second"] {
        env.manage(
            env.jup["governance"],
            env.a,
            &env.jup_neurons[name],
            sns::Command::Configure(sns::Configure {
                operation: Some(sns::Operation::ChangeAutoStakeMaturity(
                    sns::ChangeAutoStakeMaturity {
                        requested_setting_for_auto_stake_maturity: true,
                    },
                )),
            }),
        )?;
    }
    env.manage(
        env.jup["governance"],
        env.a,
        expected,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: vec![
                    jupiter_sns_rewards::generated::NeuronPermissionType::ManagePrincipals as i32,
                ],
            }),
        }),
    )?;
    let shared_block = env.commit(env.a)?;
    for _ in 0..80 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.tick()?;
        if env.record(shared_block)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    let shared = env
        .record(shared_block)?
        .context("permission-revoked bonus")?;
    assert!(matches!(shared.outcome, PatronOutcome::Verified { .. }));
    assert!(shared.compounded_neuron.is_none());
    assert_eq!(shared.amount_e8s, 100_000_000);
    assert_eq!(env.record(direct)?.unwrap(), record);
    // A real economic ownership change can qualify a later contribution but
    // cannot reopen the contribution finally skipped under the historical cohort.
    env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(unmatched),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: (1..=10).collect(),
            }),
        }),
    )?;
    env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_remove: Some(sns::NeuronPermissionList {
                permissions: (1..=10).collect(),
            }),
        }),
    )?;
    env.pic.advance_time(std::time::Duration::from_secs(86401));
    env.drive(12)?;
    assert!(env.status()?.snapshot.unwrap().id > snapshot.id);
    let newly_eligible = env.commit(unmatched)?;
    for _ in 0..80 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.tick()?;
        if env.record(newly_eligible)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    assert!(matches!(
        env.record(newly_eligible)?.unwrap().outcome,
        PatronOutcome::Verified { .. }
    ));
    assert_eq!(env.record(unmatched_block)?.unwrap(), skipped);
    Ok(())
}

#[test]
#[ignore = "two real official SNS instances and canonical source/target ballots"]
fn authentic_mirror_adoption_votes_yes_and_rejection_votes_no() -> Result<()> {
    let env = Env::new(true)?;
    // Pause the actual Ledger after the funding transfer and before native
    // refresh. Failed refreshes must retain the verified funding identity.
    let mut funded = None;
    for _ in 0..8 {
        env.tick()?;
        let state: AutomationState = query_one(
            &env.pic,
            env.rewards,
            Principal::anonymous(),
            "get_automation_status",
            (),
        )?;
        if let Some(funding) = state.funding {
            funded = Some(funding);
            break;
        }
    }
    let funded = funded.context("real proposer funding checkpoint")?;
    assert!(matches!(funded.outcome, FundingOutcome::Verified { .. }));
    env.pic
        .stop_canister(env.jup["ledger"], Some(env.jup["root"]))
        .map_err(|e| anyhow!("pause Ledger before refresh: {e:?}"))?;
    env.drive(2)?;
    let state: AutomationState = query_one(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "get_automation_status",
        (),
    )?;
    assert_eq!(state.funding, Some(funded));
    env.pic
        .start_canister(env.jup["ledger"], Some(env.jup["root"]))
        .map_err(|e| anyhow!("resume Ledger for refresh: {e:?}"))?;
    env.drive(4)?;
    let state: AutomationState = query_one(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "get_automation_status",
        (),
    )?;
    assert!(state.funding.is_none());
    for (name, expected_vote) in [
        ("adopted", 1),
        ("rejected", 2),
        ("application-setting", 1),
        ("quiet-extension", 2),
    ] {
        let action = if name == "application-setting" {
            sns::Action::ManageDappCanisterSettings(sns::ManageDappCanisterSettings {
                canister_ids: vec![env.app],
                freezing_threshold: Some(234567),
                wasm_memory_threshold: None,
                reserved_cycles_limit: None,
                log_visibility: None,
                snapshot_visibility: None,
                wasm_memory_limit: None,
                memory_allocation: None,
                compute_allocation: None,
            })
        } else {
            sns::Action::ManageSnsMetadata(sns::ManageSnsMetadata {
                name: Some(format!("Target {name}")),
                description: None,
                url: None,
                logo: None,
            })
        };
        let proposal = sns::Proposal {
            title: format!("Target {name}"),
            summary: "Contested real SNS target decision".into(),
            url: String::new(),
            action: Some(action),
        };
        let response = env.manage(
            env.target["governance"],
            env.a,
            &env.target_neurons["a"],
            sns::Command::MakeProposal(proposal),
        )?;
        let sns::Command1::MakeProposal(reply) = response else {
            anyhow::bail!("target proposal response")
        };
        let target_id = reply.proposal_id.unwrap().id;
        env.manage(
            env.target["governance"],
            env.b,
            &env.target_neurons["b"],
            sns::Command::RegisterVote(sns::RegisterVote {
                proposal: Some(sns::ProposalId { id: target_id }),
                vote: 2,
            }),
        )?;
        if expected_vote == 1 {
            env.manage(
                env.target["governance"],
                env.io_only,
                &env.target_neurons["io_only"],
                sns::Command::RegisterVote(sns::RegisterVote {
                    proposal: Some(sns::ProposalId { id: target_id }),
                    vote: 1,
                }),
            )?;
        }
        assert!(env
            .manage(
                env.target["governance"],
                env.b,
                &env.target_neurons["external"],
                sns::Command::RegisterVote(sns::RegisterVote {
                    proposal: Some(sns::ProposalId { id: target_id }),
                    vote: 1
                })
            )
            .is_err());
        let mut source_id = None;
        for _ in 0..80 {
            env.pic.advance_time(std::time::Duration::from_secs(10));
            env.tick()?;
            let record: Option<MirrorRecord> = support::calls::update_bytes(
                &env.pic,
                env.rewards,
                Principal::anonymous(),
                "get_mirror_record",
                encode_args((env.target["governance"], target_id))?,
            )?;
            if let Some(MirrorRecord {
                phase: MirrorPhase::Mapped { jupiter_proposal },
                ..
            }) = record
            {
                source_id = Some(jupiter_proposal);
                break;
            }
        }
        let source_id =
            source_id.with_context(|| format!("mirror not created: {:?}", env.status()))?;
        let proposer: sns::GetNeuronResponse = query_one(
            &env.pic,
            env.jup["governance"],
            Principal::anonymous(),
            "get_neuron",
            sns::GetNeuron {
                neuron_id: Some(sns::NeuronId {
                    id: env.jup_neurons["proposer"].clone(),
                }),
            },
        )?;
        let Some(sns::Result::Neuron(proposer)) = proposer.result else {
            anyhow::bail!("funded operational proposer missing")
        };
        assert_eq!(proposer.cached_neuron_stake_e8s, 100 * 100_000_000, "the real 10 jUP fixture must be replenished and refreshed to the canonical high watermark");
        // The 100 jUP operational proposer cannot decide either outcome. Two
        // independent 1000 jUP fixture neurons establish canonical consensus.
        env.drive(2)?;
        let target_before: sns::GetProposalResponse = query_one(
            &env.pic,
            env.target["governance"],
            Principal::anonymous(),
            "get_proposal",
            sns::GetProposal {
                proposal_id: Some(sns::ProposalId { id: target_id }),
            },
        )?;
        let Some(sns::Result1::Proposal(before)) = target_before.result else {
            anyhow::bail!("target before vote")
        };
        assert_eq!(
            before
                .ballots
                .iter()
                .find(|(id, _)| id == &hex::encode(&env.target_neurons["external"]))
                .unwrap()
                .1
                .vote,
            0
        );
        if name == "quiet-extension" {
            env.manage(
                env.jup["governance"],
                env.b,
                &env.jup_neurons["b"],
                sns::Command::RegisterVote(sns::RegisterVote {
                    proposal: Some(sns::ProposalId { id: source_id }),
                    vote: 2,
                }),
            )?;
            let response: sns::GetProposalResponse = query_one(
                &env.pic,
                env.jup["governance"],
                Principal::anonymous(),
                "get_proposal",
                sns::GetProposal {
                    proposal_id: Some(sns::ProposalId { id: source_id }),
                },
            )?;
            let Some(sns::Result1::Proposal(initial)) = response.result else {
                anyhow::bail!("initial quiet proposal")
            };
            assert_eq!(initial.decided_timestamp_seconds, 0);
            let original_deadline = initial
                .wait_for_quiet_state
                .unwrap()
                .current_deadline_timestamp_seconds;
            let now = env.pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
            env.pic.advance_time(std::time::Duration::from_secs(
                original_deadline
                    .checked_sub(now + 1)
                    .context("late vote precondition")?,
            ));
            // No leads, then a late Yes overtakes it without an absolute majority.
            env.manage(
                env.jup["governance"],
                env.a,
                &env.jup_neurons["a"],
                sns::Command::RegisterVote(sns::RegisterVote {
                    proposal: Some(sns::ProposalId { id: source_id }),
                    vote: 1,
                }),
            )?;
            let response: sns::GetProposalResponse = query_one(
                &env.pic,
                env.jup["governance"],
                Principal::anonymous(),
                "get_proposal",
                sns::GetProposal {
                    proposal_id: Some(sns::ProposalId { id: source_id }),
                },
            )?;
            let Some(sns::Result1::Proposal(extended)) = response.result else {
                anyhow::bail!("extended quiet proposal")
            };
            assert!(
                extended
                    .wait_for_quiet_state
                    .unwrap()
                    .current_deadline_timestamp_seconds
                    > original_deadline
            );
            assert_eq!(extended.decided_timestamp_seconds, 0);
            env.pic.advance_time(std::time::Duration::from_secs(2));
            env.drive(2)?;
            let response: sns::GetProposalResponse = query_one(
                &env.pic,
                env.target["governance"],
                Principal::anonymous(),
                "get_proposal",
                sns::GetProposal {
                    proposal_id: Some(sns::ProposalId { id: target_id }),
                },
            )?;
            let Some(sns::Result1::Proposal(still_open)) = response.result else {
                anyhow::bail!("target before canonical source decision")
            };
            assert_eq!(
                still_open
                    .ballots
                    .iter()
                    .find(|(id, _)| id == &hex::encode(&env.target_neurons["external"]))
                    .unwrap()
                    .1
                    .vote,
                0,
                "the elapsed original source timer is not a canonical No"
            );
            env.manage(
                env.jup["governance"],
                Principal::self_authenticating(b"NNX ROLE FIXTURE ONLY"),
                &env.jup_neurons["nnx_fixture"],
                sns::Command::RegisterVote(sns::RegisterVote {
                    proposal: Some(sns::ProposalId { id: source_id }),
                    vote: 2,
                }),
            )?;
        } else {
            for (caller, neuron) in [
                (env.a, &env.jup_neurons["a"]),
                (env.b, &env.jup_neurons["b"]),
            ] {
                env.manage(
                    env.jup["governance"],
                    caller,
                    neuron,
                    sns::Command::RegisterVote(sns::RegisterVote {
                        proposal: Some(sns::ProposalId { id: source_id }),
                        vote: expected_vote,
                    }),
                )?;
            }
            let decided: sns::GetProposalResponse = query_one(
                &env.pic,
                env.jup["governance"],
                Principal::anonymous(),
                "get_proposal",
                sns::GetProposal {
                    proposal_id: Some(sns::ProposalId { id: source_id }),
                },
            )?;
            assert!(
                matches!(decided.result, Some(sns::Result1::Proposal(p)) if p.decided_timestamp_seconds > 0),
                "the ordinary owners must decide before the NNX fixture participates"
            );
            env.manage(
                env.jup["governance"],
                Principal::self_authenticating(b"NNX ROLE FIXTURE ONLY"),
                &env.jup_neurons["nnx_fixture"],
                sns::Command::RegisterVote(sns::RegisterVote {
                    proposal: Some(sns::ProposalId { id: source_id }),
                    vote: expected_vote,
                }),
            )?;
        }
        env.drive(10)?;
        let record: Option<MirrorRecord> = support::calls::update_bytes(
            &env.pic,
            env.rewards,
            Principal::anonymous(),
            "get_mirror_record",
            encode_args((env.target["governance"], target_id))?,
        )?;
        assert!(
            matches!(record.unwrap().phase, MirrorPhase::Verified { vote, .. } if vote == expected_vote)
        );
        let target_after: sns::GetProposalResponse = query_one(
            &env.pic,
            env.target["governance"],
            Principal::anonymous(),
            "get_proposal",
            sns::GetProposal {
                proposal_id: Some(sns::ProposalId { id: target_id }),
            },
        )?;
        let Some(sns::Result1::Proposal(after)) = target_after.result else {
            anyhow::bail!("target after vote")
        };
        assert_eq!(
            after
                .ballots
                .iter()
                .find(|(id, _)| id == &hex::encode(&env.target_neurons["external"]))
                .unwrap()
                .1
                .vote,
            expected_vote
        );
        if expected_vote == 1 {
            assert!(
                after.executed_timestamp_seconds > 0,
                "canonical target effect was not executed"
            );
        }
        if name == "application-setting" {
            assert_eq!(
                env.pic
                    .canister_status(env.app, Some(env.target["root"]))
                    .unwrap()
                    .settings
                    .freezing_threshold,
                234567_u64
            );
        }
    }
    let reserve = support::ledger::icrc1_balance(
        &env.pic,
        env.jup["ledger"],
        &Account {
            owner: env.rewards,
            subaccount: None,
        },
    )?;
    let initial_supply = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../deploy/acceptance/economics.json"
    ))?["initial_supply_e8s"]
        .as_u64()
        .unwrap();
    assert_eq!(reserve, initial_supply - 3_101 * 100_000_000 - 1_000_000,
        "one 90 jUP top-up plus its fee; open, adopted and one rejected charge cannot cause repeated reserve spending");
    // Zero native inflation retains authentic participation-event shares.
    env.pic
        .advance_time(std::time::Duration::from_secs(6 * 86400 + 3600));
    for _ in 0..40 {
        env.pic.tick();
    }
    env.drive(12)?;
    let event: sns::RewardEvent = query_one(
        &env.pic,
        env.jup["governance"],
        Principal::anonymous(),
        "get_latest_reward_event",
        (),
    )?;
    assert_eq!(event.distributed_e8s_equivalent, 0);
    let participation: ParticipationState = query_one(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "get_participation_status",
        (),
    )?;
    let active = participation
        .active
        .context("complete real zero-inflation participation event was not published")?;
    assert_eq!(
        Some(active.end_timestamp_seconds),
        event.end_timestamp_seconds
    );
    assert!(active.eligible_shares > 0_u8);
    let mut exact = candid::Nat::from(0_u8);
    for name in ["a", "b"] {
        let response: sns::GetNeuronResponse = query_one(
            &env.pic,
            env.jup["governance"],
            Principal::anonymous(),
            "get_neuron",
            sns::GetNeuron {
                neuron_id: Some(sns::NeuronId {
                    id: env.jup_neurons[name].clone(),
                }),
            },
        )?;
        let Some(sns::Result::Neuron(neuron)) = response.result else {
            anyhow::bail!("real native participant missing")
        };
        let participation = neuron.latest_reward_event_participation.unwrap();
        assert_eq!(
            participation.reward_event_end_timestamp_seconds,
            Some(active.end_timestamp_seconds)
        );
        exact.0 += participation.reward_shares.unwrap().0;
    }
    assert_eq!(
        active.eligible_shares, exact,
        "native voting-power shares must not be multiplied by stake again"
    );
    let nnx: sns::GetNeuronResponse = query_one(
        &env.pic,
        env.jup["governance"],
        Principal::anonymous(),
        "get_neuron",
        sns::GetNeuron {
            neuron_id: Some(sns::NeuronId {
                id: env.jup_neurons["nnx_fixture"].clone(),
            }),
        },
    )?;
    assert!(
        matches!(nnx.result, Some(sns::Result::Neuron(n)) if n.latest_reward_event_participation.as_ref()
        .and_then(|p| p.reward_shares.as_ref()).is_some_and(|shares| shares.0.bits() > 0)),
        "a participating excluded NNX fixture must not dilute the external denominator"
    );
    let excluded: Option<candid::Nat> = support::calls::update_bytes(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "quote_gross_io_share",
        encode_args((1_000_000_u64, env.rewards))?,
    )?;
    assert!(
        excluded.is_none(),
        "protocol-only proposer entered the external reward denominator"
    );
    let io_only: Option<candid::Nat> = support::calls::update_bytes(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "quote_gross_io_share",
        encode_args((1_000_000_u64, env.io_only))?,
    )?;
    assert!(
        io_only.is_none(),
        "IO discovery is not jUP participation weighting"
    );
    Ok(())
}

#[derive(CandidType)]
enum PatronFault {
    AcceptThenTrap,
    RejectThenTrap,
    BadFee { expected_fee_e8s: u64 },
}

#[test]
#[ignore = "real ICP/SNS ownership with a deliberate Ledger fault boundary"]
fn uncertain_paid_and_unpaid_patron_transfers_are_terminal_and_do_not_block_later_payments(
) -> Result<()> {
    let env = Env::setup(false, true)?;
    env.drive(6)?;
    let recipient = Account {
        owner: env.b,
        subaccount: None,
    };
    let mut blocks = Vec::new();
    for fault in [
        PatronFault::AcceptThenTrap,
        PatronFault::RejectThenTrap,
        PatronFault::BadFee {
            expected_fee_e8s: 20_000_000,
        },
    ] {
        let _: () = update_one(
            &env.pic,
            env.jup["ledger"],
            Principal::anonymous(),
            "debug_set_next_error",
            Some(fault),
        )?;
        let block = env.commit(env.b)?;
        for _ in 0..100 {
            env.pic.advance_time(std::time::Duration::from_secs(2));
            env.pic.tick();
            env.tick()?;
            if env.record(block)?.is_some() && env.status()?.batch.is_none() {
                break;
            }
        }
        blocks.push(block);
    }
    for block in &blocks[..2] {
        let record = env
            .record(*block)?
            .context("missing uncertain payment review record")?;
        assert!(
            matches!(record.outcome, PatronOutcome::AssumedPaidUnverified { .. }),
            "{record:?}"
        );
        assert_eq!(record.amount_e8s, 100_000_000);
        assert!(!record.memo.is_empty());
        assert_eq!(
            record.snapshot_id,
            env.record(blocks[0])?.unwrap().snapshot_id
        );
    }
    assert!(matches!(
        env.record(blocks[2])?.unwrap().outcome,
        PatronOutcome::Failed { .. }
    ));
    assert_eq!(env.balance(recipient)?, 100_000_000); // only the first uncertain call really paid
    assert_eq!(env.status()?.verified_e8s, 0);
    assert_eq!(env.status()?.assumed_e8s, 200_000_000);
    let good = env.commit(env.b)?;
    for _ in 0..100 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.pic.tick();
        env.tick()?;
        if env.record(good)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    assert!(matches!(
        env.record(good)?.unwrap().outcome,
        PatronOutcome::Verified { .. }
    ));
    assert_eq!(env.balance(recipient)?, 200_000_000);
    env.drive(10)?;
    let attempts: Vec<candid::Reserved> = query_one(
        &env.pic,
        env.jup["ledger"],
        Principal::anonymous(),
        "debug_transfer_attempts",
        (),
    )?;
    assert_eq!(attempts.len(), 4);
    assert_eq!(env.status()?.verified_e8s, 100_000_000);
    Ok(())
}

#[test]
#[ignore = "actual official Ledger dispatch followed by a deliberately trapped callback and upgrade"]
fn upgrade_of_unresolved_dispatch_never_repeats_the_patron_transfer() -> Result<()> {
    let env = Env::new(false)?;
    env.drive(6)?;
    let block = env.commit(env.b)?;
    let _: () = update_noargs(
        &env.pic,
        env.rewards,
        Principal::anonymous(),
        "debug_trap_patron_callback_once",
    )?;
    let mut trapped = false;
    for _ in 0..100 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.pic.tick();
        if let Err(error) = env.tick() {
            assert!(error.to_string().contains("callback trap"), "{error:?}");
            trapped = true;
            break;
        }
    }
    assert!(
        trapped,
        "the actual dispatched Ledger callback was not exercised"
    );
    let checkpoint = env
        .record(block)?
        .context("dispatch checkpoint did not survive callback trap")?;
    assert!(
        matches!(checkpoint.outcome, PatronOutcome::Dispatched { .. }),
        "{checkpoint:?}"
    );
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        100_000_000
    );
    // Settle install-code instruction debt using ordinary rounds, with no time
    // advance that would let the programme's next timer consume the checkpoint.
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
        .map_err(|e| anyhow!("upgrade actual dispatch checkpoint: {e:?}"))?;
    let recovered = env.record(block)?.unwrap();
    assert!(
        matches!(
            recovered.outcome,
            PatronOutcome::AssumedPaidUnverified { .. }
        ),
        "{recovered:?}"
    );
    assert_eq!(
        recovered.created_at_time_nanos,
        checkpoint.created_at_time_nanos
    );
    assert_eq!(recovered.to, checkpoint.to);
    assert_eq!(recovered.memo, checkpoint.memo);
    env.drive(10)?;
    assert_eq!(
        env.balance(Account {
            owner: env.b,
            subaccount: None
        })?,
        100_000_000
    );
    assert_eq!(env.status()?.verified_e8s, 0);
    assert_eq!(env.status()?.assumed_e8s, 100_000_000);
    Ok(())
}

#[test]
#[ignore = "official jUP with IO absent and a pinned historical patron batch"]
fn relay_jup_index_is_independent_of_io_and_long_patron_history() -> Result<()> {
    let env = Env::setup_sources(false, false, false)?;
    let direct = env.commit(env.a)?;
    for _ in 0..120 {
        let _: () = update_noargs(&env.pic, env.rewards, env.a, "debug_scan_tick")?;
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.pic.tick();
        env.tick()?;
        if env.record(direct)?.is_some() && env.status()?.batch.is_none() {
            break;
        }
    }
    let first = env.relay_context()?;
    env.assert_relay_owners(&first)?;
    assert!(env.status()?.snapshot.is_some());
    assert!(matches!(
        env.record(direct)?.unwrap().outcome,
        PatronOutcome::Verified { .. }
    ));
    assert_eq!(env.status()?.snapshot.unwrap().id, first.snapshot_id);
    // A historical batch with many independently recorded contributions remains
    // pinned across two daily refresh opportunities. No fixture changes its cursor.
    let pinned_b = env.commit(env.b)?;
    for n in 0..24 {
        let outsider = Principal::self_authenticating(format!("long history {n}"));
        env.commit(outsider)?;
    }
    for _ in 0..120 {
        env.pic.advance_time(std::time::Duration::from_secs(2));
        env.pic.tick();
        env.tick()?;
        if env.status()?.batch.is_some() {
            break;
        }
    }
    let pinned = env
        .status()?
        .batch
        .context("long patron batch did not start")?;
    // Transfer B's genuine neuron after the historical generation was pinned.
    // Relay's live map must update; this batch must retain its original B owner.
    env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.io_only),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: (1..=10).collect(),
            }),
        }),
    )?;
    env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_remove: Some(sns::NeuronPermissionList {
                permissions: (1..=10).collect(),
            }),
        }),
    )?;
    env.pic.advance_time(std::time::Duration::from_secs(3600));
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
        .map_err(|e| anyhow!("pinned historical upgrade: {e:?}"))?;
    let restored_batch = env
        .status()?
        .batch
        .context("pinned batch survives upgrade")?;
    assert_eq!(restored_batch.snapshot, pinned.snapshot);
    assert_eq!(restored_batch.end_block, pinned.end_block);
    let mut prior = env.relay_context()?;
    for _ in 0..2 {
        env.pic.advance_time(std::time::Duration::from_secs(86_400));
        for _ in 0..8 {
            env.pic.tick();
        }
        env.drive(3)?;
        assert_eq!(
            env.status()?.batch.as_ref().map(|b| b.snapshot.id),
            Some(pinned.snapshot.id)
        );
        let refreshed = env.relay_context()?;
        assert!(refreshed.snapshot_id > prior.snapshot_id);
        assert_ne!(refreshed.snapshot_id, pinned.snapshot.id);
        let owners: ResolveDefaultIcpAccountsResult = query_one(
            &env.pic,
            env.rewards,
            env.a,
            "resolve_default_icp_accounts",
            ResolveDefaultIcpAccountsArgs {
                snapshot_id: refreshed.snapshot_id,
                account_identifiers: [env.b, env.io_only]
                    .map(|p| {
                        jupiter_ic_clients::account_identifier::account_identifier_bytes(p, None)
                            .to_vec()
                    })
                    .to_vec(),
            },
        )?;
        assert_eq!(
            owners,
            ResolveDefaultIcpAccountsResult::Ok(vec![None, Some(env.io_only)])
        );
        let stale: ResolveDefaultIcpAccountsResult = query_one(
            &env.pic,
            env.rewards,
            env.a,
            "resolve_default_icp_accounts",
            ResolveDefaultIcpAccountsArgs {
                snapshot_id: prior.snapshot_id,
                account_identifiers: vec![],
            },
        )?;
        assert_eq!(stale, ResolveDefaultIcpAccountsResult::SnapshotChanged);
        prior = refreshed;
    }
    env.drive(100)?;
    let historical = env
        .record(pinned_b)?
        .context("old cohort's recognised direct patron")?;
    assert!(matches!(historical.outcome, PatronOutcome::Verified { .. }));
    assert_eq!(historical.snapshot_id, pinned.snapshot.id);
    assert_eq!(historical.recognition.as_ref().unwrap().principal, env.b);
    assert_eq!(
        historical.to,
        Account {
            owner: env.b,
            subaccount: None
        }
    );
    Ok(())
}

#[test]
#[ignore = "official SNS stake refresh after confirmed/assumed payment, failure and upgrade"]
fn paid_compound_refresh_resumes_without_reselection_or_payment_replay() -> Result<()> {
    for assumed in [false, true] {
        let env = Env::new(false)?;
        env.drive(6)?;
        let block = env.commit(env.a)?;
        if assumed {
            let _: () = update_noargs(
                &env.pic,
                env.rewards,
                env.a,
                "debug_trap_patron_callback_once",
            )?;
        }
        for _ in 0..100 {
            env.pic.advance_time(std::time::Duration::from_secs(2));
            env.pic.tick();
            let result = env.tick();
            if let Err(error) = result {
                assert!(
                    assumed && error.to_string().contains("callback trap"),
                    "{error:?}"
                );
            }
            if env.record(block)?.is_some_and(|r| {
                matches!(
                    r.outcome,
                    PatronOutcome::Verified { .. } | PatronOutcome::Dispatched { .. }
                )
            }) {
                break;
            }
        }
        let paid = env.record(block)?.context("payment checkpoint")?;
        let selected = paid
            .compounded_neuron
            .clone()
            .context("selected compound neuron")?;
        assert_eq!(
            paid.to.subaccount,
            Some(selected.clone().try_into().unwrap())
        );
        let actual_balance = env.balance(paid.to)?;
        assert!(actual_balance >= paid.amount_e8s);
        // Upgrade between the irreversible transfer and idempotent refresh.
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
            .map_err(|e| anyhow!("upgrade paid compound: {e:?}"))?;
        let recovered = env.record(block)?.unwrap();
        assert!(if assumed {
            matches!(
                recovered.outcome,
                PatronOutcome::AssumedPaidUnverified { .. }
            )
        } else {
            matches!(recovered.outcome, PatronOutcome::Verified { .. })
        });
        let job: Option<RefreshJob> =
            query_one(&env.pic, env.rewards, env.a, "get_patron_refresh", block)?;
        assert_eq!(
            job.context("durable refresh job")?.status,
            RefreshStatus::Pending
        );
        env.pic
            .stop_canister(env.jup["governance"], Some(env.jup["root"]))
            .map_err(|e| anyhow!("stop refresh target: {e:?}"))?;
        env.tick()?;
        let failed: Option<RefreshJob> =
            query_one(&env.pic, env.rewards, env.a, "get_patron_refresh", block)?;
        let failed = failed.unwrap();
        assert_eq!(failed.status, RefreshStatus::Pending);
        assert!(failed.attempt > 0 && failed.attempts_remaining < 5);
        // Repeated ticks before the bounded delay do not dispatch more attempts.
        env.drive(3)?;
        let unchanged: Option<RefreshJob> =
            query_one(&env.pic, env.rewards, env.a, "get_patron_refresh", block)?;
        assert_eq!(unchanged, Some(failed));
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
            .map_err(|e| anyhow!("upgrade pending refresh: {e:?}"))?;
        env.pic
            .start_canister(env.jup["governance"], Some(env.jup["root"]))
            .map_err(|e| anyhow!("restart refresh target: {e:?}"))?;
        env.manage(
            env.jup["governance"],
            env.a,
            &selected,
            sns::Command::Configure(sns::Configure {
                operation: Some(sns::Operation::ChangeAutoStakeMaturity(
                    sns::ChangeAutoStakeMaturity {
                        requested_setting_for_auto_stake_maturity: false,
                    },
                )),
            }),
        )?;
        env.pic.advance_time(std::time::Duration::from_secs(301));
        env.drive(4)?;
        let refreshed: Option<RefreshJob> =
            query_one(&env.pic, env.rewards, env.a, "get_patron_refresh", block)?;
        assert_eq!(refreshed.unwrap().status, RefreshStatus::Confirmed);
        let final_record = env.record(block)?.unwrap();
        assert_eq!(final_record.outcome, recovered.outcome);
        assert_eq!(final_record.to, paid.to);
        assert_eq!(final_record.memo, paid.memo);
        assert_eq!(
            final_record.created_at_time_nanos,
            paid.created_at_time_nanos
        );
        assert_eq!(final_record.compounded_neuron, Some(selected.clone()));
        assert_eq!(
            env.balance(paid.to)?,
            actual_balance,
            "refresh never pays again"
        );
        let reply: sns::GetNeuronResponse = query_one(
            &env.pic,
            env.jup["governance"],
            env.a,
            "get_neuron",
            sns::GetNeuron {
                neuron_id: Some(sns::NeuronId { id: selected }),
            },
        )?;
        let Some(sns::Result::Neuron(neuron)) = reply.result else {
            anyhow::bail!("refreshed neuron unavailable: {reply:?}");
        };
        assert_eq!(neuron.cached_neuron_stake_e8s, actual_balance);
        assert!(!neuron.auto_stake_maturity.unwrap_or(false));
        let state = env.status()?;
        assert_eq!(state.verified_e8s, if assumed { 0 } else { 200_000_000 });
        assert_eq!(state.assumed_e8s, if assumed { 200_000_000 } else { 0 });
    }
    Ok(())
}

#[test]
#[ignore = "official Governance permission attack, voting delegation and production payout"]
fn economic_permission_attack_cannot_capture_or_poison_real_compounding() -> Result<()> {
    use jupiter_sns_rewards::generated::NeuronPermissionType as P;
    use sha2::{Digest, Sha256};
    let env = Env::setup_programme(false, false, false, true)?;
    let selected = [&env.jup_neurons["a"], &env.jup_neurons["a_second"]]
        .into_iter()
        .min()
        .unwrap()
        .clone();
    let (memo, expected_id) = (20_u64..10_000)
        .map(|memo| {
            let id = Sha256::digest(
                [
                    b"\x0cneuron-stake".as_slice(),
                    env.b.as_slice(),
                    &memo.to_be_bytes(),
                ]
                .concat(),
            )
            .to_vec();
            (memo, id)
        })
        .find(|(_, id)| id < &selected)
        .context("low attack nonce")?;
    // A zero-duration genesis vesting period ends after its creation instant.
    env.pic.advance_time(std::time::Duration::from_secs(1));
    env.pic.tick();
    let split = env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::Split(sns::Split {
            memo,
            amount_e8s: 100 * 100_000_000,
        }),
    )?;
    let sns::Command1::Split(split) = split else {
        anyhow::bail!("split: {split:?}");
    };
    let attacker_id = split.created_neuron_id.context("split ID")?.id;
    assert_eq!(attacker_id, expected_id);
    env.manage(
        env.jup["governance"],
        env.b,
        &attacker_id,
        sns::Command::Configure(sns::Configure {
            operation: Some(sns::Operation::ChangeAutoStakeMaturity(
                sns::ChangeAutoStakeMaturity {
                    requested_setting_for_auto_stake_maturity: true,
                },
            )),
        }),
    )?;
    // Victim gets more permission codes than attacker. Attacker retains takeover.
    env.manage(
        env.jup["governance"],
        env.b,
        &attacker_id,
        sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_remove: Some(sns::NeuronPermissionList {
                permissions: vec![
                    P::ConfigureDissolveState,
                    P::SubmitProposal,
                    P::Vote,
                    P::Disburse,
                    P::Split,
                    P::MergeMaturity,
                    P::DisburseMaturity,
                    P::StakeMaturity,
                    P::ManageVotingPermission,
                ]
                .into_iter()
                .map(|p| p as i32)
                .collect(),
            }),
        }),
    )?;
    env.manage(
        env.jup["governance"],
        env.b,
        &attacker_id,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.a),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: vec![
                    P::ConfigureDissolveState,
                    P::SubmitProposal,
                    P::Vote,
                    P::Disburse,
                    P::Split,
                    P::MergeMaturity,
                    P::DisburseMaturity,
                    P::StakeMaturity,
                    P::ManageVotingPermission,
                ]
                .into_iter()
                .map(|p| p as i32)
                .collect(),
            }),
        }),
    )?;
    // Run the predecessor's permission-count rule on the actual native record.
    // It attributes the attacker's low-ID neuron to the unconsenting victim.
    let response: sns::GetNeuronResponse = query_one(
        &env.pic,
        env.jup["governance"],
        Principal::anonymous(),
        "get_neuron",
        sns::GetNeuron {
            neuron_id: Some(sns::NeuronId {
                id: attacker_id.clone(),
            }),
        },
    )?;
    let Some(sns::Result::Neuron(attack)) = response.result else {
        anyhow::bail!("native attack neuron missing")
    };
    let mut previous_permissions = BTreeMap::<_, std::collections::BTreeSet<_>>::new();
    for permission in &attack.permissions {
        if let Some(principal) = permission.principal {
            previous_permissions
                .entry(principal)
                .or_default()
                .extend(permission.permission_type.iter().copied());
        }
    }
    let previous_owner = previous_permissions
        .into_iter()
        .max_by(|(left_principal, left), (right_principal, right)| {
            left.len()
                .cmp(&right.len())
                .then_with(|| right_principal.as_slice().cmp(left_principal.as_slice()))
        })
        .map(|(principal, _)| principal);
    assert_eq!(previous_owner, Some(env.a));
    assert!(attack.auto_stake_maturity.unwrap_or(false));
    assert!(attacker_id < selected);
    // Legitimate voting delegation is safe, including its native grant restriction.
    env.manage(
        env.jup["governance"],
        env.a,
        &selected,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.io_only),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: vec![P::Vote as i32, P::ManageVotingPermission as i32],
            }),
        }),
    )?;
    let denied = env.manage(
        env.jup["governance"],
        env.io_only,
        &selected,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: vec![P::Disburse as i32],
            }),
        }),
    );
    assert!(
        denied.is_err(),
        "voting manager cannot grant economic permissions"
    );
    // Nobody needs ManagePrincipals: freeze permission control on the valid neuron.
    env.manage(
        env.jup["governance"],
        env.a,
        &selected,
        sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
            principal_id: Some(env.a),
            permissions_to_remove: Some(sns::NeuronPermissionList {
                permissions: vec![P::ManagePrincipals as i32],
            }),
        }),
    )?;
    env.manage(
        env.jup["governance"],
        env.io_only,
        &selected,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: vec![P::Vote as i32],
            }),
        }),
    )?;
    let all_rights: Vec<i32> = [
        P::ConfigureDissolveState,
        P::ManagePrincipals,
        P::SubmitProposal,
        P::Vote,
        P::Disburse,
        P::Split,
        P::MergeMaturity,
        P::DisburseMaturity,
        P::StakeMaturity,
        P::ManageVotingPermission,
    ]
    .into_iter()
    .map(|p| p as i32)
    .collect();
    // A fully gifted, sole-Governance neuron would satisfy economic ownership,
    // but Governance's default token Account is the minter, never a recipient.
    env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.jup["governance"]),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: all_rights.clone(),
            }),
        }),
    )?;
    env.manage(
        env.jup["governance"],
        env.b,
        &env.jup_neurons["b"],
        sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_remove: Some(sns::NeuronPermissionList {
                permissions: all_rights.clone(),
            }),
        }),
    )?;
    let attack_balance = env.balance(Account {
        owner: env.jup["governance"],
        subaccount: Some(attacker_id.try_into().unwrap()),
    })?;
    env.pic.advance_time(std::time::Duration::from_secs(86_401));
    env.drive(6)?;
    let context = env.relay_context()?;
    let governance_owner: ResolveDefaultIcpAccountsResult = query_one(
        &env.pic,
        env.rewards,
        env.a,
        "resolve_default_icp_accounts",
        ResolveDefaultIcpAccountsArgs {
            snapshot_id: context.snapshot_id,
            account_identifiers: vec![
                jupiter_ic_clients::account_identifier::account_identifier_bytes(
                    env.jup["governance"],
                    None,
                )
                .to_vec(),
            ],
        },
    )?;
    assert_eq!(
        governance_owner,
        ResolveDefaultIcpAccountsResult::Ok(vec![None])
    );
    let governance_commit = env.commit(env.jup["governance"])?;
    let block = env.commit(env.a)?;
    for _ in 0..60 {
        env.tick()?;
        if env.record(block)?.is_some() && env.record(governance_commit)?.is_some() {
            break;
        }
    }
    assert!(matches!(
        env.record(governance_commit)?
            .context("Governance exclusion")?
            .outcome,
        PatronOutcome::Skipped { .. }
    ));
    let record = env.record(block)?.context("secure production payout")?;
    assert!(
        matches!(record.outcome, PatronOutcome::Verified { .. }),
        "{record:?}; {:?}",
        env.status()?
    );
    assert_eq!(record.compounded_neuron, Some(selected));
    assert_eq!(record.amount_e8s, 200_000_000);
    assert_eq!(
        env.balance(Account {
            owner: env.jup["governance"],
            subaccount: Some(expected_id.try_into().unwrap())
        })?,
        attack_balance
    );
    Ok(())
}

#[path = "secure_rewards.rs"]
mod secure_rewards;

#[path = "native_economics.rs"]
mod native_economics;
