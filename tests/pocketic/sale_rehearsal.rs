//! Actual SNS-W launch and real Swap success/refund. No fabricated valuation or
//! direct Governance mode transition. NNS caller identity is an isolated local
//! execution fixture; the official CLI independently exercises proposal conversion.
use super::*;
use anyhow::{anyhow, Context};
use candid::Nat;
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferArg};
use jupiter_sns_rewards::{generated as sns, *};
use pocket_ic::PocketIc;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

#[derive(CandidType)]
struct Empty {}
#[derive(CandidType)]
struct SnsWasmInit {
    allowed_principals: Vec<Principal>,
    access_controls_enabled: bool,
    sns_subnet_ids: Vec<Principal>,
}
#[derive(CandidType)]
struct SnsWasm {
    wasm: Vec<u8>,
    proposal_id: Option<u64>,
    canister_type: i32,
}
#[derive(CandidType)]
struct AddWasm {
    hash: Vec<u8>,
    wasm: Option<SnsWasm>,
    skip_update_latest_version: Option<bool>,
}
#[derive(CandidType, Deserialize, Debug)]
struct WasmError {
    message: String,
}
#[derive(CandidType, Deserialize, Debug)]
enum WasmResult {
    Hash(Vec<u8>),
    Error(WasmError),
}
#[derive(CandidType, Deserialize, Debug)]
struct AddReply {
    result: Option<WasmResult>,
}
#[derive(CandidType, Deserialize, Debug)]
struct Ids {
    root: Option<Principal>,
    governance: Option<Principal>,
    ledger: Option<Principal>,
    index: Option<Principal>,
    swap: Option<Principal>,
}
#[derive(CandidType, Deserialize, Debug)]
struct DeployReply {
    canisters: Option<Ids>,
    error: Option<WasmError>,
}
#[derive(CandidType, Deserialize, Debug)]
struct Mode {
    mode: Option<i32>,
}
#[derive(CandidType, Deserialize, Debug)]
struct Lifecycle {
    lifecycle: Option<i32>,
    decentralization_swap_termination_timestamp_seconds: Option<u64>,
}
#[derive(CandidType)]
struct Refresh {
    buyer: String,
    confirmation_text: Option<String>,
}
#[derive(CandidType, Deserialize)]
struct Accepted {
    icp_accepted_participation_e8s: u64,
}
#[derive(CandidType, Deserialize, Debug)]
struct Finalized {
    error_message: Option<String>,
}
#[derive(CandidType, Deserialize, Debug)]
struct AutoFinalization {
    auto_finalize_swap_response: Option<Finalized>,
    has_auto_finalize_been_attempted: Option<bool>,
    is_auto_finalize_enabled: Option<bool>,
}
#[derive(CandidType)]
struct ListNeurons {
    of_principal: Option<Principal>,
    limit: u32,
    start_page_at: Option<sns::NeuronId>,
}
#[derive(CandidType, Deserialize)]
struct Neurons {
    neurons: Vec<sns::Neuron>,
}
#[derive(CandidType)]
struct Refund {
    source_principal_id: Option<Principal>,
}
#[derive(CandidType, Deserialize)]
struct RefundReply {
    result: Option<candid::Reserved>,
}

fn checked(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn get_neuron(pic: &PocketIc, governance: Principal, id: Vec<u8>) -> Result<sns::Neuron> {
    let reply: sns::GetNeuronResponse = query_one(
        pic,
        governance,
        Principal::anonymous(),
        "get_neuron",
        sns::GetNeuron {
            neuron_id: Some(sns::NeuronId { id }),
        },
    )?;
    match reply.result {
        Some(sns::Result::Neuron(n)) => Ok(n),
        other => anyhow::bail!("neuron unavailable: {other:?}"),
    }
}

#[test]
#[ignore = "official pinned SNS-W, six SNS components and owned real NNS/ICP topology"]
fn production_snsw_launch_success_reserve_transport_and_failed_sale_refund() -> Result<()> {
    let rewards_wasm = wasm(&REWARDS_WASM, "jupiter-sns-rewards", Some("debug_api"))?;
    let relay_wasm = wasm(&OnceLock::new(), "jupiter-relay", Some("debug_api"))?;
    let nns_mock_wasm = wasm(&OnceLock::new(), "mock-nns-governance", None)?;
    let dummy =
        support::wasm::build_wasm_cached_for_test(&OnceLock::new(), "mock-cycle-burner", None)?;
    let repo = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let artifacts = PathBuf::from(checked(
        Command::new("python3")
            .args(["tools/acceptance/artifacts.py", "--fetch"])
            .current_dir(&repo),
    )?);
    let snsw_path = PathBuf::from(checked(
        Command::new("python3")
            .args(["tools/acceptance/launch.py", "--sns-wasm", "--fetch"])
            .current_dir(&repo),
    )?);
    let did = snsw_path.parent().unwrap().join("sns-wasm.did");
    let economics: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join("deploy/acceptance/economics.json"))?)?;
    let e = |name: &str| economics[name].as_u64().unwrap();
    let pic = support::pocketic::sns_topology_builder()
        .with_icp_features(pocket_ic::common::rest::IcpFeatures {
            registry: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            icp_token: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            nns_governance: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            cycles_minting: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            ..Default::default()
        })
        .build();
    let snsw = support::principals::sns_wasm();
    pic.create_canister_with_id(None, None, snsw)
        .map_err(|error| anyhow!("SNS-W allocation: {error:?}"))?;
    pic.add_cycles(snsw, 1_000_000_000_000_000); // creation cost plus separate local execution/archive headroom
    pic.install_canister(
        snsw,
        fs::read(snsw_path)?,
        encode_one(SnsWasmInit {
            allowed_principals: vec![],
            access_controls_enabled: true,
            sns_subnet_ids: vec![pic.topology().get_sns().context("SNS subnet")?],
        })?,
        None,
    );
    for (role, kind) in [
        ("root", 1),
        ("governance", 2),
        ("ledger", 3),
        ("swap", 4),
        ("archive", 5),
        ("index", 6),
    ] {
        let bytes = fs::read(artifacts.join(format!("{role}.wasm.gz")))?;
        let hash = Sha256::digest(&bytes).to_vec();
        let reply: AddReply = update_one(
            &pic,
            snsw,
            support::principals::nns_governance(),
            "add_wasm",
            AddWasm {
                hash: hash.clone(),
                wasm: Some(SnsWasm {
                    wasm: bytes,
                    proposal_id: Some(144202),
                    canister_type: kind,
                }),
                skip_update_latest_version: Some(false),
            },
        )?;
        assert!(
            matches!(reply.result, Some(WasmResult::Hash(found)) if found == hash),
            "SNS-W rejected {role}"
        );
    }
    let base = repo.join(format!(
        "target/sale-rehearsal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir_all(&base)?;
    for success in [true, false] {
        let rewards = pic.create_canister();
        pic.add_cycles(rewards, 5_000_000_000_000);
        pic.install_canister(rewards, dummy.clone(), encode_args(())?, None);
        pic.set_controllers(rewards, None, vec![support::principals::nns_root()])
            .map_err(|error| anyhow!("NNS Root launch custody: {error:?}"))?;
        let nnx = Principal::self_authenticating(b"production launch NNX FIXTURE ONLY");
        let fallback = Principal::self_authenticating(b"sale failure fallback FIXTURE ONLY");
        let buyer = Principal::self_authenticating(if success {
            b"successful buyer".as_slice()
        } else {
            b"refunded buyer".as_slice()
        });
        let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
        let context = serde_json::json!({"nnx_fixture":nnx,"rewards":rewards,"fallback":fallback,
            "nns_proposal_id": if success {9001} else {9002},"start":now+60,"due":now+60+7*86400});
        let path = base.join(if success {
            "success-context.json"
        } else {
            "failure-context.json"
        });
        fs::write(&path, serde_json::to_vec_pretty(&context)?)?;
        let output = base.join(if success { "success" } else { "failure" });
        checked(
            Command::new("python3")
                .arg("tools/acceptance/sale.py")
                .arg(&path)
                .arg(&did)
                .arg(&output)
                .current_dir(&repo),
        )?;
        let reply: DeployReply = support::calls::update_bytes(
            &pic,
            snsw,
            support::principals::nns_governance(),
            "deploy_new_sns",
            fs::read(output.join("deploy.bin"))?,
        )?;
        anyhow::ensure!(
            reply.error.is_none(),
            "authentic SNS-W rejection: {reply:?}"
        );
        let ids = reply
            .canisters
            .context("SNS-W canister allocation missing")?;
        let governance = ids.governance.context("Governance")?;
        let root = ids.root.context("Root")?;
        let ledger = ids.ledger.context("Ledger")?;
        let swap = ids.swap.context("Swap")?;
        let index = ids.index.context("Index")?;
        let mode: Mode = query_one(
            &pic,
            governance,
            Principal::anonymous(),
            "get_mode",
            Empty {},
        )?;
        assert_eq!(mode.mode, Some(2));
        let supply: Nat = query_one(
            &pic,
            ledger,
            Principal::anonymous(),
            "icrc1_total_supply",
            (),
        )?;
        assert_eq!(supply, Nat::from(e("initial_supply_e8s")));
        let decimals: u8 = query_one(&pic, ledger, Principal::anonymous(), "icrc1_decimals", ())?;
        assert_eq!(decimals, 8);
        let neurons: Neurons = query_one(
            &pic,
            governance,
            Principal::anonymous(),
            "list_neurons",
            ListNeurons {
                of_principal: None,
                limit: 100,
                start_page_at: None,
            },
        )?;
        assert_eq!(neurons.neurons.len(), 3);
        let transport = neurons
            .neurons
            .iter()
            .find(|n| n.cached_neuron_stake_e8s == e("reserve_transport_e8s"))
            .context("genesis reserve transport")?;
        let powerful = neurons
            .neurons
            .iter()
            .find(|n| n.cached_neuron_stake_e8s == e("governance_stake_e8s"))
            .unwrap();
        let proposer = neurons
            .neurons
            .iter()
            .find(|n| n.cached_neuron_stake_e8s == e("proposer_stake_e8s"))
            .unwrap();
        assert_eq!(
            transport.dissolve_state,
            Some(sns::DissolveState::DissolveDelaySeconds(0))
        );
        assert_eq!(transport.vesting_period_seconds, Some(0));
        assert!(transport
            .permissions
            .iter()
            .any(|p| p.principal == Some(rewards) && p.permission_type.contains(&5)));
        assert_eq!(
            support::ledger::icrc1_balance(
                &pic,
                ledger,
                &Account {
                    owner: rewards,
                    subaccount: None
                }
            )?,
            0
        );
        assert_eq!(
            support::ledger::icrc1_balance(
                &pic,
                support::principals::icp_ledger(),
                &Account {
                    owner: governance,
                    subaccount: None
                }
            )?,
            0
        );
        let reserved_io = pic.create_canister();
        let relay = pic.create_canister();
        pic.add_cycles(relay, 5_000_000_000_000);
        let forwarding_governance = pic.create_canister();
        pic.add_cycles(forwarding_governance, 5_000_000_000_000);
        pic.install_canister(
            forwarding_governance,
            nns_mock_wasm.clone(),
            encode_args(())?,
            None,
        );
        // Only the downstream NNS neuron lookup/refresh is a local mock. SNS-W,
        // Governance, Treasury, ICP Ledger and the forwarding Relay execute real code.
        let _: () = support::calls::update_bytes(
            &pic,
            forwarding_governance,
            Principal::anonymous(),
            "debug_set_neuron_account",
            encode_args((11_614_578_985_374_291_210_u64, vec![42_u8; 32]))?,
        )?;
        let config = ProgrammeConfig {
            version: 1,
            jup_root: root,
            io_root: reserved_io,
            icp_ledger: support::principals::icp_ledger(),
            icp_index: support::principals::icp_index(),
            staking_account: Account {
                owner: forwarding_governance,
                subaccount: Some([42; 32]),
            },
            canonical_relay: relay,
            protocol_governance_neuron: powerful.id.as_ref().unwrap().id.clone(),
            excluded_icp_accounts: vec![],
            excluded_jup_neurons: vec![
                transport.id.as_ref().unwrap().id.clone(),
                powerful.id.as_ref().unwrap().id.clone(),
                proposer.id.as_ref().unwrap().id.clone(),
            ],
            reserve_subaccount: None,
            reserve_transport_neuron: Some(transport.id.as_ref().unwrap().id.clone()),
            launch_timestamp_seconds: None,
            patron_enabled: false,
            snapshot_max_age_seconds: 172800,
            automation: Some(AutomationConfig {
                proposer_neuron: proposer.id.as_ref().unwrap().id.clone(),
                targets: vec![],
                treasury_enabled: false,
                treasury_retain_icp_e8s: 0,
                treasury_max_proposal_icp_e8s: 0,
            }),
        };
        // Replace the dummy dapp with the actual reserve custodian while Root
        // controls it; no operator or fabricated production NNX role is retained.
        for _ in 0..100 {
            pic.tick();
        }
        pic.reinstall_canister(
            rewards,
            rewards_wasm.clone(),
            encode_one(jupiter_sns_rewards::InitArgs {
                reward_sns_root_canister_id: None,
                programme: Some(config.clone()),
            })?,
            Some(root),
        )
        .map_err(|error| anyhow!("install governed reserve custodian: {error:?}"))?;
        // Reinstall is limited to this fresh zero-state dummy fixture. Existing
        // Jupiter state and its strict upgrade contract are never reset.
        let _: () = update_noargs(
            &pic,
            rewards,
            Principal::anonymous(),
            "debug_programme_tick",
        )?;
        let before: ReserveTransport = query_one(
            &pic,
            rewards,
            Principal::anonymous(),
            "get_reserve_transport_status",
            (),
        )?;
        assert_eq!(before, ReserveTransport::NotDispatched);
        pic.advance_time(std::time::Duration::from_secs(61));
        for _ in 0..20 {
            pic.tick();
        }
        let lifecycle: Lifecycle = query_one(
            &pic,
            swap,
            Principal::anonymous(),
            "get_lifecycle",
            Empty {},
        )?;
        assert_eq!(lifecycle.lifecycle, Some(2));
        let amount = if success {
            e("sale_max_icp_e8s")
        } else {
            e("sale_min_participant_icp_e8s")
        };
        support::ledger::icrc1_transfer(
            &pic,
            support::principals::icp_ledger(),
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: buyer,
                    subaccount: None,
                },
                amount: (amount + 10000).into(),
                fee: Some(10000_u64.into()),
                memo: None,
                created_at_time: None,
            },
        )?;
        let mut sub = [0_u8; 32];
        sub[0] = buyer.as_slice().len() as u8;
        sub[1..1 + buyer.as_slice().len()].copy_from_slice(buyer.as_slice());
        support::ledger::icrc1_transfer(
            &pic,
            support::principals::icp_ledger(),
            buyer,
            TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: swap,
                    subaccount: Some(sub),
                },
                amount: amount.into(),
                fee: Some(10000_u64.into()),
                memo: None,
                created_at_time: None,
            },
        )?;
        let accepted: Accepted = update_one(
            &pic,
            swap,
            buyer,
            "refresh_buyer_tokens",
            Refresh {
                buyer: buyer.to_text(),
                confirmation_text: None,
            },
        )?;
        assert_eq!(accepted.icp_accepted_participation_e8s, amount);
        if !success {
            pic.advance_time(std::time::Duration::from_secs(7 * 86400 + 1));
        }
        for _ in 0..120 {
            pic.advance_time(std::time::Duration::from_secs(10));
            pic.tick();
            let lifecycle: Lifecycle = query_one(&pic, swap, buyer, "get_lifecycle", Empty {})?;
            if lifecycle.lifecycle == Some(if success { 3 } else { 4 }) {
                break;
            }
        }
        let mut finalized = None;
        for _ in 0..120 {
            pic.advance_time(std::time::Duration::from_secs(1));
            pic.tick();
            let status: AutoFinalization =
                query_one(&pic, swap, buyer, "get_auto_finalization_status", Empty {})?;
            if let Some(response) = status.auto_finalize_swap_response {
                finalized = Some(response);
                break;
            }
            if status.is_auto_finalize_enabled == Some(false) {
                finalized = Some(update_one(&pic, swap, buyer, "finalize_swap", Empty {})?);
                break;
            }
            assert!(
                status.has_auto_finalize_been_attempted != Some(false)
                    || status.is_auto_finalize_enabled == Some(true)
            );
        }
        let finalized = finalized.context("authentic Swap finalization did not complete")?;
        anyhow::ensure!(
            finalized.error_message.is_none(),
            "real finalization failed: {finalized:?}"
        );
        for _ in 0..30 {
            pic.tick();
        }
        let lifecycle: Lifecycle = query_one(&pic, swap, buyer, "get_lifecycle", Empty {})?;
        assert_eq!(lifecycle.lifecycle, Some(if success { 3 } else { 4 }));
        if success {
            let mode: Mode = query_one(&pic, governance, buyer, "get_mode", Empty {})?;
            assert_eq!(mode.mode, Some(1));
            assert!(lifecycle
                .decentralization_swap_termination_timestamp_seconds
                .is_some());
            let participants: Neurons = query_one(
                &pic,
                governance,
                buyer,
                "list_neurons",
                ListNeurons {
                    of_principal: Some(buyer),
                    limit: 100,
                    start_page_at: None,
                },
            )?;
            assert_eq!(participants.neurons.len(), 2);
            assert!(participants.neurons.iter().all(|n| n.auto_stake_maturity != Some(true)
                && n.permissions.iter().any(|p| p.principal == Some(buyer) && p.permission_type.contains(&5))),
                "sale neurons must expose the actual buyer ownership and disabled direct-buyer compounding default");
            let mut delays = participants
                .neurons
                .iter()
                .map(|n| match n.dissolve_state {
                    Some(sns::DissolveState::DissolveDelaySeconds(d)) => d,
                    _ => u64::MAX,
                })
                .collect::<Vec<_>>();
            delays.sort();
            assert_eq!(delays, vec![0, e("year_seconds")]);
            assert_eq!(
                participants
                    .neurons
                    .iter()
                    .map(|n| n.cached_neuron_stake_e8s)
                    .sum::<u64>(),
                e("sale_allocation_e8s") - 2 * e("transfer_fee_e8s")
            );
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &Account {
                        owner: governance,
                        subaccount: None
                    }
                )?,
                amount - 10000
            );
            for _ in 0..8 {
                let _: () = update_noargs(&pic, rewards, buyer, "debug_programme_tick")?;
            }
            let reserve: ReserveTransport =
                query_one(&pic, rewards, buyer, "get_reserve_transport_status", ())?;
            assert!(
                matches!(reserve, ReserveTransport::Verified { amount_e8s, .. } if amount_e8s == e("reserve_transport_e8s") - e("transfer_fee_e8s")),
                "{reserve:?}"
            );
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    ledger,
                    &Account {
                        owner: rewards,
                        subaccount: None
                    }
                )?,
                e("reserve_transport_e8s") - e("transfer_fee_e8s")
            );
            let proposer = get_neuron(&pic, governance, proposer.id.as_ref().unwrap().id.clone())?;
            assert_eq!(proposer.permissions.len(), 1);
            assert_eq!(proposer.permissions[0].principal, Some(rewards));
            assert_eq!(proposer.permissions[0].permission_type, vec![3]);
            let supply: Nat = query_one(&pic, ledger, buyer, "icrc1_total_supply", ())?;
            assert_eq!(
                supply,
                Nat::from(e("initial_supply_e8s") - 3 * e("transfer_fee_e8s"))
            );
            assert_eq!(
                pic.canister_status(rewards, Some(root))
                    .unwrap()
                    .settings
                    .controllers,
                vec![root]
            );
            assert_eq!(
                pic.canister_status(governance, Some(root))
                    .unwrap()
                    .settings
                    .controllers,
                vec![root]
            );
            assert_eq!(
                pic.canister_status(root, Some(governance))
                    .unwrap()
                    .settings
                    .controllers,
                vec![governance]
            );
            // Reinvest actual successful-sale ICP proceeds. ICP treasury limits use
            // Ledger/CMC valuation; SNS-token valuation separately needs Swap.
            let mut previous_operation = None;
            for (version, vote) in [(2, 1), (2, 2)] {
                let mut activated = config.clone();
                activated.version = version;
                activated.launch_timestamp_seconds =
                    lifecycle.decentralization_swap_termination_timestamp_seconds;
                let auto = activated.automation.as_mut().unwrap();
                auto.treasury_enabled = true;
                auto.treasury_max_proposal_icp_e8s = 10 * 100_000_000; // explicit local withdrawal fixture
                if let Some(previous) = previous_operation {
                    let reply: Result<(), String> = support::calls::update_bytes(
                        &pic,
                        rewards,
                        governance,
                        "request_treasury_operation",
                        encode_args((previous + 1, Some(previous)))?,
                    )?;
                    assert!(reply.is_ok(), "{reply:?}");
                } else {
                    for _ in 0..100 {
                        pic.tick();
                    }
                    pic.upgrade_canister(
                        rewards,
                        rewards_wasm.clone(),
                        encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                            reward_sns_root_canister_id: None,
                            programme: Some(activated),
                        }))?,
                        Some(root),
                    )
                    .map_err(|error| anyhow!("activate native treasury fixture: {error:?}"))?;
                }
                let mut proposal_id = None;
                for _ in 0..30 {
                    let _: () = update_noargs(&pic, rewards, buyer, "debug_programme_tick")?;
                    let state: Option<TreasuryState> =
                        query_one(&pic, rewards, buyer, "get_treasury_status", ())?;
                    if let Some(TreasuryState {
                        phase: TreasuryPhase::Proposed { proposal_id: id },
                        ..
                    }) = state
                    {
                        proposal_id = Some(id);
                        break;
                    }
                }
                let id = proposal_id.context("native treasury proposal was not submitted")?;
                let reply: sns::ManageNeuronResponse = update_one(
                    &pic,
                    governance,
                    nnx,
                    "manage_neuron",
                    sns::ManageNeuron {
                        subaccount: powerful.id.as_ref().unwrap().id.clone(),
                        command: Some(sns::Command::RegisterVote(sns::RegisterVote {
                            proposal: Some(sns::ProposalId { id }),
                            vote,
                        })),
                    },
                )?;
                assert!(
                    matches!(reply.command, Some(sns::Command1::RegisterVote {})),
                    "{reply:?}"
                );
                for _ in 0..60 {
                    pic.advance_time(std::time::Duration::from_secs(2));
                    pic.tick();
                    let _: () = update_noargs(&pic, rewards, buyer, "debug_programme_tick")?;
                    let state: Option<TreasuryState> =
                        query_one(&pic, rewards, buyer, "get_treasury_status", ())?;
                    if state.is_some_and(|s| {
                        matches!(
                            s.phase,
                            TreasuryPhase::TransferVerified { .. } | TreasuryPhase::ManualReview(_)
                        )
                    }) {
                        break;
                    }
                }
                let state: Option<TreasuryState> =
                    query_one(&pic, rewards, buyer, "get_treasury_status", ())?;
                if vote == 1 {
                    assert!(
                        matches!(state.as_ref().map(|s| &s.phase), Some(TreasuryPhase::TransferVerified { proposal_id, .. }) if *proposal_id == id),
                        "{state:?}"
                    );
                } else {
                    assert!(
                        matches!(state.as_ref().map(|s| &s.phase), Some(TreasuryPhase::ManualReview(reason)) if reason.contains("rejected")),
                        "{state:?}"
                    );
                }
                let mut relay_subaccount = [0; 32];
                relay_subaccount[31] = 1;
                assert_eq!(
                    support::ledger::icrc1_balance(
                        &pic,
                        support::principals::icp_ledger(),
                        &Account {
                            owner: relay,
                            subaccount: Some(relay_subaccount)
                        }
                    )?,
                    10 * 100_000_000
                );
                for _ in 0..8 {
                    let _: () = update_noargs(&pic, rewards, buyer, "debug_programme_tick")?;
                }
                let unchanged: Option<TreasuryState> =
                    query_one(&pic, rewards, buyer, "get_treasury_status", ())?;
                assert_eq!(
                    unchanged, state,
                    "terminal treasury request must not repropose itself"
                );
                let operation_id = state.as_ref().unwrap().operation_id.unwrap();
                if vote == 1 {
                    previous_operation = Some(operation_id);
                }
                let retained: Option<TreasuryState> = query_one(
                    &pic,
                    rewards,
                    buyer,
                    "get_treasury_operation",
                    previous_operation.unwrap(),
                )?;
                assert!(matches!(
                    retained.unwrap().phase,
                    TreasuryPhase::TransferVerified { .. }
                ));
                if vote == 2 {
                    let rejected: Result<(), String> = support::calls::update_bytes(
                        &pic,
                        rewards,
                        governance,
                        "request_treasury_operation",
                        encode_args((operation_id + 1, Some(operation_id)))?,
                    )?;
                    assert!(
                        rejected.is_err(),
                        "rejected operations require review; no automatic loop"
                    );
                }
            }
            #[derive(CandidType)]
            struct RelayInit {
                managed_canisters: Vec<Principal>,
                governance_canister_id: Option<Principal>,
                main_interval_seconds: Option<u64>,
                surplus_neuron_recipients: Vec<RelayNeuronRecipient>,
            }
            #[derive(CandidType)]
            struct RelayNeuronRecipient {
                neuron_id: u64,
                memo: Vec<u8>,
            }
            pic.install_canister(
                relay,
                relay_wasm.clone(),
                encode_one(RelayInit {
                    managed_canisters: vec![],
                    governance_canister_id: Some(forwarding_governance),
                    main_interval_seconds: Some(86_400),
                    surplus_neuron_recipients: vec![],
                })?,
                None,
            );
            for _ in 0..6 {
                let _: () = update_noargs(&pic, relay, buyer, "debug_main_tick")?;
                pic.tick();
            }
            let mut sub = [0; 32];
            sub[31] = 1;
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &Account {
                        owner: relay,
                        subaccount: Some(sub)
                    }
                )?,
                0
            );
            let endowment = Account {
                owner: forwarding_governance,
                subaccount: Some([42; 32]),
            };
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &endowment
                )?,
                10 * 100_000_000 - 10_000
            );
            // Exact real Ledger/Index evidence joins native Treasury credit to
            // the Relay forwarding leg (including its memo and fee).
            let account = jupiter_ic_clients::account_identifier::account_identifier_text(
                endowment.owner,
                endowment.subaccount,
            );
            let mut transfers = vec![];
            for _ in 0..60 {
                pic.advance_time(std::time::Duration::from_secs(2));
                pic.tick();
                let reply: jupiter_ic_clients::index::GetAccountIdentifierTransactionsResult =
                    query_one(
                        &pic,
                        support::principals::icp_index(),
                        buyer,
                        "get_account_identifier_transactions",
                        jupiter_ic_clients::index::GetAccountIdentifierTransactionsArgs {
                            account_identifier: account.clone(),
                            start: None,
                            max_results: 10,
                        },
                    )?;
                let jupiter_ic_clients::index::GetAccountIdentifierTransactionsResult::Ok(page) =
                    reply
                else {
                    anyhow::bail!("Index forwarding query rejected");
                };
                transfers = page.transactions;
                if !transfers.is_empty() {
                    break;
                }
            }
            assert_eq!(transfers.len(), 1);
            let forwarded = &transfers[0];
            assert_eq!(
                forwarded.transaction.icrc1_memo.as_deref(),
                Some(format!("{}.Relay", relay.to_text().replace('-', "")).as_bytes())
            );
            let jupiter_ic_clients::index::IndexOperation::Transfer {
                from,
                to,
                amount,
                fee,
                spender,
            } = &forwarded.transaction.operation
            else {
                anyhow::bail!("forwarding is not a Ledger transfer");
            };
            assert_eq!(
                from,
                &jupiter_ic_clients::account_identifier::account_identifier_text(relay, Some(sub))
            );
            assert_eq!(to, &account);
            assert_eq!(amount.e8s(), 10 * 100_000_000 - 10_000);
            assert_eq!(fee.e8s(), 10_000);
            assert!(spender.is_none());
            let patron: Runtime = query_one(&pic, rewards, buyer, "get_programme_status", ())?;
            assert_eq!(patron.verified_e8s, 0);
            assert_eq!(patron.assumed_e8s, 0);
        } else {
            let _: RefundReply = update_one(
                &pic,
                swap,
                buyer,
                "error_refund_icp",
                Refund {
                    source_principal_id: Some(buyer),
                },
            )?;
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &Account {
                        owner: buyer,
                        subaccount: None
                    }
                )?,
                amount - 10000
            );
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &Account {
                        owner: governance,
                        subaccount: None
                    }
                )?,
                0
            );
        }
        let _ = index;
    }
    Ok(())
}
