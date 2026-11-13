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

#[path = "sale_faucet.rs"]
mod sale_faucet;
#[path = "sale_nns.rs"]
mod sale_nns;

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

/// Funding/receipt errors fail the fixture; a native minimum-buyer rejection is
/// returned separately so the below-threshold control can inspect it.
fn deposit_buyer(
    pic: &PocketIc,
    swap: Principal,
    buyer: Principal,
    amount: u64,
) -> Result<Result<Accepted>> {
    let ledger = support::principals::icp_ledger();
    support::ledger::icrc1_transfer(
        pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: Account {
                owner: buyer,
                subaccount: None,
            },
            amount: (amount + 10_000).into(),
            fee: Some(10_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    let mut sub = [0; 32];
    sub[0] = buyer.as_slice().len() as u8;
    sub[1..1 + buyer.as_slice().len()].copy_from_slice(buyer.as_slice());
    support::ledger::icrc1_transfer(
        pic,
        ledger,
        buyer,
        TransferArg {
            from_subaccount: None,
            to: Account {
                owner: swap,
                subaccount: Some(sub),
            },
            amount: amount.into(),
            fee: Some(10_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    Ok(update_one(
        pic,
        swap,
        buyer,
        "refresh_buyer_tokens",
        Refresh {
            buyer: buyer.to_text(),
            confirmation_text: None,
        },
    ))
}

#[test]
#[ignore = "official pinned SNS-W, six SNS components and owned real NNS/ICP topology"]
fn production_snsw_launch_success_reserve_transport_and_failed_sale_refund() -> Result<()> {
    let rewards_wasm = wasm(&REWARDS_WASM, "jupiter-sns-rewards", Some("debug_api"))?;
    let faucet_wasm = wasm(&OnceLock::new(), "jupiter-faucet", Some("debug_api"))?;
    let disburser_wasm = wasm(&OnceLock::new(), "jupiter-disburser", Some("debug_api"))?;
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
    for (label, success, raise) in [
        ("minimum", true, e("sale_min_icp_e8s")),
        ("maximum", true, e("sale_max_icp_e8s")),
        ("failure", false, e("sale_min_participant_icp_e8s")),
    ] {
        let rewards = pic.create_canister();
        pic.add_cycles(rewards, 5_000_000_000_000);
        pic.install_canister(rewards, dummy.clone(), encode_args(())?, None);
        pic.set_controllers(rewards, None, vec![support::principals::nns_root()])
            .map_err(|error| anyhow!("NNS Root launch custody: {error:?}"))?;
        let nnx = Principal::self_authenticating(b"production launch NNX FIXTURE ONLY");
        let fallback = Principal::self_authenticating(b"sale failure fallback FIXTURE ONLY");
        let buyer = Principal::self_authenticating(format!("{label} buyer"));
        let minimum_buyer = Principal::self_authenticating(format!("{label} minimum buyer"));
        let below_buyer = Principal::self_authenticating(format!("{label} below minimum"));
        let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
        let context = serde_json::json!({"nnx_fixture":nnx,"rewards":rewards,"fallback":fallback,
            "nns_proposal_id": if success {9001} else {9002},"start":now+60,"due":now+60+7*86400});
        let path = base.join(format!("{label}-context.json"));
        fs::write(&path, serde_json::to_vec_pretty(&context)?)?;
        let output = base.join(label);
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
        let parameters: sns::NervousSystemParameters =
            query_one(&pic, governance, nnx, "get_nervous_system_parameters", ())?;
        for (actual, key) in [
            (parameters.transaction_fee_e8s, "transfer_fee_e8s"),
            (parameters.reject_cost_e8s, "proposal_rejection_e8s"),
            (
                parameters.neuron_minimum_stake_e8s,
                "minimum_neuron_stake_e8s",
            ),
            (
                parameters.neuron_minimum_dissolve_delay_to_vote_seconds,
                "minimum_voting_dissolve_delay_seconds",
            ),
            (
                parameters.max_dissolve_delay_seconds,
                "max_dissolve_delay_seconds",
            ),
            (
                parameters.max_neuron_age_for_age_bonus,
                "max_neuron_age_for_age_bonus",
            ),
            (parameters.max_age_bonus_percentage, "age_bonus_percentage"),
            (
                parameters.max_dissolve_delay_bonus_percentage,
                "dissolve_bonus_percentage",
            ),
            (
                parameters.initial_voting_period_seconds,
                "jupiter_voting_seconds",
            ),
            (
                parameters.wait_for_quiet_deadline_increase_seconds,
                "wait_for_quiet_seconds",
            ),
        ] {
            assert_eq!(actual, Some(e(key)), "official launch parameter {key}");
        }
        let native = parameters.voting_rewards_parameters.as_ref().unwrap();
        assert_eq!(native.initial_reward_rate_basis_points, Some(0));
        assert_eq!(native.final_reward_rate_basis_points, Some(0));
        assert_eq!(native.reward_rate_transition_duration_seconds, Some(0));
        assert_eq!(native.round_duration_seconds, Some(86_400));
        assert!(parameters
            .default_followees
            .as_ref()
            .unwrap()
            .followees
            .is_empty());
        assert_eq!(parameters.max_followees_per_function, Some(15));
        assert_eq!(parameters.max_proposals_to_keep_per_action, Some(100));
        assert_eq!(parameters.max_number_of_proposals_with_ballots, Some(700));
        // SnsInitPayload has no automatic-version field. Observe its real inherited
        // default, then use a native governed change after Normal mode below.
        assert_eq!(parameters.automatically_advance_target_version, Some(true));
        fs::write(
            output.join("effective-governance-parameters.json"),
            serde_json::to_vec_pretty(&parameters)?,
        )?;
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
        let relay = pic.create_canister();
        pic.add_cycles(relay, 5_000_000_000_000);
        let forwarding_governance = support::principals::nns_governance();
        let disburser = pic.create_canister();
        pic.add_cycles(disburser, 5_000_000_000_000);
        let staking_subaccount: [u8; 32] = Sha256::digest(
            [
                b"\x0cneuron-stake".as_slice(),
                disburser.as_slice(),
                &0_u64.to_be_bytes(),
            ]
            .concat(),
        )
        .into();
        let staking_account = Account {
            owner: forwarding_governance,
            subaccount: Some(staking_subaccount),
        };
        let nns_neuron = sale_nns::claim(&pic, disburser, staking_account)?;
        let base_recipient = pic.create_canister();
        pic.add_cycles(base_recipient, 5_000_000_000_000);
        sale_faucet::install(
            &pic,
            faucet_wasm.clone(),
            base_recipient,
            disburser,
            staking_account,
            fallback,
        )?;
        let dquorum_recipient = pic.create_canister();
        #[derive(CandidType)]
        struct DisburserInit {
            neuron_id: u64,
            normal_recipient: Account,
            age_bonus_recipient_1: Account,
            age_bonus_recipient_2: Account,
            ledger_canister_id: Option<Principal>,
            governance_canister_id: Option<Principal>,
            rescue_controller: Principal,
            main_interval_seconds: Option<u64>,
            rescue_interval_seconds: Option<u64>,
        }
        pic.install_canister(
            disburser,
            disburser_wasm.clone(),
            encode_one(DisburserInit {
                neuron_id: nns_neuron,
                normal_recipient: Account {
                    owner: base_recipient,
                    subaccount: None,
                },
                age_bonus_recipient_1: Account {
                    owner: rewards,
                    subaccount: None,
                },
                age_bonus_recipient_2: Account {
                    owner: dquorum_recipient,
                    subaccount: None,
                },
                ledger_canister_id: Some(support::principals::icp_ledger()),
                governance_canister_id: Some(forwarding_governance),
                rescue_controller: fallback,
                main_interval_seconds: Some(365 * 86400),
                rescue_interval_seconds: Some(365 * 86400),
            })?,
            None,
        );
        let config = ProgrammeConfig {
            version: 1,
            jup_root: root,
            relay_factory: None,
            icp_ledger: support::principals::icp_ledger(),
            icp_index: support::principals::icp_index(),
            staking_account,
            canonical_relay: relay,
            treasury_stake: Some(TreasuryStakeBinding {
                disburser,
                neuron_id: nns_neuron,
            }),
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
            raise - e("sale_min_participant_icp_e8s")
        } else {
            raise
        };
        if success {
            let below = deposit_buyer(
                &pic,
                swap,
                below_buyer,
                e("sale_min_participant_icp_e8s") - 1,
            )?;
            assert!(
                below
                    .as_ref()
                    .map_or(true, |r| r.icp_accepted_participation_e8s == 0),
                "a buyer below the configured 11 ICP minimum must not be accepted"
            );
            let small =
                deposit_buyer(&pic, swap, minimum_buyer, e("sale_min_participant_icp_e8s"))??;
            assert_eq!(
                small.icp_accepted_participation_e8s,
                e("sale_min_participant_icp_e8s")
            );
        }
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
        if !success || raise < e("sale_max_icp_e8s") {
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
            let delegated: sns::ManageNeuronResponse = update_one(
                &pic,
                governance,
                nnx,
                "manage_neuron",
                sns::ManageNeuron {
                    subaccount: powerful.id.as_ref().unwrap().id.clone(),
                    command: Some(sns::Command::AddNeuronPermissions(
                        sns::AddNeuronPermissions {
                            principal_id: Some(buyer),
                            permissions_to_add: Some(sns::NeuronPermissionList {
                                permissions: vec![
                                    jupiter_sns_rewards::generated::NeuronPermissionType::Vote
                                        as i32,
                                ],
                            }),
                        },
                    )),
                },
            )?;
            assert!(
                matches!(
                    delegated.command,
                    Some(sns::Command1::AddNeuronPermission {})
                ),
                "{delegated:?}"
            );
            assert!(lifecycle
                .decentralization_swap_termination_timestamp_seconds
                .is_some());
            let mut participants: Neurons = query_one(
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
            assert_eq!(
                participants.neurons.len(),
                3,
                "native principal lookup includes the protocol voting delegation"
            );
            participants.neurons.retain(|n| n.id != powerful.id);
            assert_eq!(
                participants.neurons.len(),
                2,
                "only the exact excluded protocol ID is removed from the sale basket"
            );
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
                (u128::from(e("sale_allocation_e8s")) * u128::from(amount) / u128::from(raise))
                    as u64
                    - 2 * e("transfer_fee_e8s")
            );
            let small: Neurons = query_one(
                &pic,
                governance,
                minimum_buyer,
                "list_neurons",
                ListNeurons {
                    of_principal: Some(minimum_buyer),
                    limit: 100,
                    start_page_at: None,
                },
            )?;
            assert_eq!(small.neurons.len(), 2);
            let small_gross = (u128::from(e("sale_allocation_e8s"))
                * u128::from(e("sale_min_participant_icp_e8s"))
                / u128::from(raise)) as u64;
            for n in &small.neurons {
                assert_eq!(
                    n.cached_neuron_stake_e8s,
                    small_gross / 2 - e("transfer_fee_e8s")
                );
                assert!(n.cached_neuron_stake_e8s >= e("minimum_neuron_stake_e8s"));
            }
            assert_eq!(
                participants
                    .neurons
                    .iter()
                    .chain(&small.neurons)
                    .map(|n| n.cached_neuron_stake_e8s)
                    .sum::<u64>(),
                e("sale_allocation_e8s") - 4 * e("transfer_fee_e8s")
            );
            let _: RefundReply = update_one(
                &pic,
                swap,
                below_buyer,
                "error_refund_icp",
                Refund {
                    source_principal_id: Some(below_buyer),
                },
            )?;
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &Account {
                        owner: below_buyer,
                        subaccount: None
                    }
                )?,
                e("sale_min_participant_icp_e8s") - 1 - 10_000
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
                raise - 2 * 10000
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
                Nat::from(e("initial_supply_e8s") - 5 * e("transfer_fee_e8s"))
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
            let mut pinned_parameters = parameters.clone();
            pinned_parameters.automatically_advance_target_version = Some(false);
            let changed: sns::ManageNeuronResponse = update_one(&pic, governance, nnx, "manage_neuron", sns::ManageNeuron {
                subaccount: powerful.id.as_ref().unwrap().id.clone(),
                command: Some(sns::Command::MakeProposal(sns::Proposal {
                    title: "Pin reviewed SNS version policy".into(), summary: "Local governed compatibility rehearsal; launch converter cannot set this field.".into(), url: String::new(),
                    action: Some(sns::Action::ManageNervousSystemParameters(pinned_parameters)),
                })),
            })?;
            assert!(
                matches!(changed.command, Some(sns::Command1::MakeProposal(_))),
                "{changed:?}"
            );
            for _ in 0..20 {
                pic.tick();
            }
            let pinned: sns::NervousSystemParameters =
                query_one(&pic, governance, nnx, "get_nervous_system_parameters", ())?;
            assert_eq!(
                pinned.automatically_advance_target_version,
                Some(false),
                "native governed pin must precede financial activation in this fixture"
            );
            // A minimum-raise sale closes after seven days; the native CMC's
            // initial local quote is then stale. Preserve the worker's freshness
            // gate and refresh only this owned official fixture's timestamp.
            #[derive(CandidType, Deserialize)]
            struct Quote {
                data: jupiter_ic_clients::cmc::IcpXdrConversionRate,
            }
            #[derive(CandidType)]
            struct Rate {
                data_source: String,
                timestamp_seconds: u64,
                xdr_permyriad_per_icp: u64,
            }
            let quote: Quote = query_one(
                &pic,
                support::principals::cycles_minting_canister(),
                nnx,
                "get_icp_xdr_conversion_rate",
                (),
            )?;
            let at = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
            eprintln!(
                "{label} sale CMC quote age before local refresh: {} seconds",
                at.saturating_sub(quote.data.timestamp_seconds)
            );
            let refreshed: Result<(), String> = update_one(
                &pic,
                support::principals::cycles_minting_canister(),
                support::principals::nns_governance(),
                "set_icp_xdr_conversion_rate",
                Rate {
                    data_source: "owned sale rehearsal; unchanged conversion value".into(),
                    timestamp_seconds: at,
                    xdr_permyriad_per_icp: quote.data.xdr_permyriad_per_icp,
                },
            )?;
            assert!(refreshed.is_ok(), "{refreshed:?}");
            // Reinvest actual successful-sale ICP proceeds. ICP treasury limits use
            // Ledger/CMC valuation; SNS-token valuation separately needs Swap.
            let mut previous_operation = None;
            for (version, vote) in [(2, 1), (2, 2)] {
                let mut activated = config.clone();
                activated.version = version;
                activated.launch_timestamp_seconds =
                    lifecycle.decentralization_swap_termination_timestamp_seconds;
                let auto = activated.automation.as_mut().unwrap();
                activated.patron_enabled = true;
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
                let id = if let Some(id) = proposal_id {
                    id
                } else {
                    let treasury: Option<TreasuryState> =
                        query_one(&pic, rewards, buyer, "get_treasury_status", ())?;
                    let runtime: Runtime =
                        query_one(&pic, rewards, buyer, "get_programme_status", ())?;
                    anyhow::bail!("native treasury proposal was not submitted: treasury={treasury:?}; programme={runtime:?}")
                };
                let voting_buyer = participants
                    .neurons
                    .iter()
                    .find(|n| {
                        n.dissolve_state
                            == Some(sns::DissolveState::DissolveDelaySeconds(e("year_seconds")))
                    })
                    .context("one-year sale participant")?;
                let buyer_vote: sns::ManageNeuronResponse = update_one(
                    &pic,
                    governance,
                    buyer,
                    "manage_neuron",
                    sns::ManageNeuron {
                        subaccount: voting_buyer.id.as_ref().unwrap().id.clone(),
                        command: Some(sns::Command::RegisterVote(sns::RegisterVote {
                            proposal: Some(sns::ProposalId { id }),
                            vote: 1,
                        })),
                    },
                )?;
                assert!(
                    matches!(buyer_vote.command, Some(sns::Command1::RegisterVote {})),
                    "{buyer_vote:?}"
                );
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
                assert_eq!(
                    support::ledger::icrc1_balance(
                        &pic,
                        support::principals::icp_ledger(),
                        &Account {
                            owner: relay,
                            subaccount: jupiter_memo_policy::numbered_subaccount(1)
                        }
                    )?,
                    0
                );
                assert_eq!(
                    support::ledger::icrc1_balance(
                        &pic,
                        support::principals::icp_ledger(),
                        &staking_account
                    )?,
                    110 * 100_000_000
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
            // Settle actual Governance reward events despite zero native token inflation.
            pic.advance_time(std::time::Duration::from_secs(6 * 86400));
            for _ in 0..80 {
                pic.tick();
            }
            for _ in 0..12 {
                let _: () = update_noargs(&pic, rewards, buyer, "debug_programme_tick")?;
            }
            let native_event: sns::RewardEvent =
                query_one(&pic, governance, buyer, "get_latest_reward_event", ())?;
            assert_eq!(native_event.distributed_e8s_equivalent, 0);
            let participation: ParticipationState =
                query_one(&pic, rewards, buyer, "get_participation_status", ())?;
            let published = participation
                .active
                .context("sale-participant reward event")?;
            assert_eq!(
                Some(published.end_timestamp_seconds),
                native_event.end_timestamp_seconds
            );
            let mut buyer_shares = Nat::from(0_u8);
            for participant in &participants.neurons {
                let neuron = get_neuron(
                    &pic,
                    governance,
                    participant.id.as_ref().unwrap().id.clone(),
                )?;
                if let Some(p) = neuron.latest_reward_event_participation.filter(|p| {
                    p.reward_event_end_timestamp_seconds == native_event.end_timestamp_seconds
                }) {
                    buyer_shares.0 += p.reward_shares.context("official sale shares")?.0;
                }
            }
            assert!(
                buyer_shares > 0_u8,
                "one-year external voter must receive participation shares"
            );
            assert_eq!(published.eligible_shares, buyer_shares, "governance, reserve and proposer neurons cannot dilute external native voting weights; native={native_event:?}; published={published:?}; participants={:?}", participants.neurons);
            let icp_inventory = support::ledger::icrc1_balance(
                &pic,
                support::principals::icp_ledger(),
                &Account {
                    owner: rewards,
                    subaccount: None,
                },
            )?;
            let quote: Option<Nat> = support::calls::update_bytes(
                &pic,
                rewards,
                buyer,
                "quote_gross_io_share",
                encode_args((1_000_000_000_u64, buyer))?,
            )?;
            assert_eq!(quote, Some(Nat::from(1_000_000_000_u64)));
            // The buyer's extra voting permission on the excluded protocol
            // neuron cannot exclude its independently owned sale neuron.
            assert!(
                get_neuron(&pic, governance, powerful.id.as_ref().unwrap().id.clone())?
                    .permissions
                    .iter()
                    .any(|p| p.principal == Some(buyer))
            );
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    support::principals::icp_ledger(),
                    &Account {
                        owner: rewards,
                        subaccount: None
                    }
                )?,
                icp_inventory,
                "an unfunded IO quote must not dispatch ICP or create an IO entitlement"
            );
            let endowment = staking_account;
            let before_refresh = sale_nns::probe(&pic, disburser, nns_neuron)?;
            assert_eq!(before_refresh.cached_neuron_stake_e8s, 100 * 100_000_000);
            let refresh_start = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
            let _: () = update_noargs(&pic, disburser, buyer, "debug_main_tick")?;
            let after_refresh = sale_nns::probe(&pic, disburser, nns_neuron)?;
            let refresh_end = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
            assert_eq!(after_refresh.cached_neuron_stake_e8s, 110 * 100_000_000);
            let expected_since = |t: u64| {
                t - u64::try_from(
                    u128::from(t.saturating_sub(before_refresh.aging_since_timestamp_seconds))
                        * u128::from(before_refresh.cached_neuron_stake_e8s)
                        / u128::from(after_refresh.cached_neuron_stake_e8s),
                )
                .unwrap()
            };
            assert!(
                (expected_since(refresh_start)..=expected_since(refresh_end))
                    .contains(&after_refresh.aging_since_timestamp_seconds)
            );
            assert!(
                after_refresh.aging_since_timestamp_seconds
                    > before_refresh.aging_since_timestamp_seconds
            );
            // Native SNS proposal -> real canonical Ledger -> real NNS cached stake.
            // No Relay forwarding, routing memo or jUP payment is involved.
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
                if transfers.len() >= 2 {
                    break;
                }
            }
            let forwarded = transfers.iter().find(|tx| matches!(&tx.transaction.operation,
                jupiter_ic_clients::index::IndexOperation::Transfer { from, .. }
                if *from == jupiter_ic_clients::account_identifier::account_identifier_text(governance, None)))
                .context("direct native treasury receipt")?;
            assert!(forwarded.transaction.icrc1_memo.is_none());
            assert_eq!(forwarded.transaction.memo, 0);
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
                &jupiter_ic_clients::account_identifier::account_identifier_text(governance, None)
            );
            assert_eq!(to, &account);
            assert_eq!(amount.e8s(), 10 * 100_000_000);
            assert_eq!(fee.e8s(), 10_000);
            assert!(spender.is_none());
            let patron: Runtime = query_one(&pic, rewards, buyer, "get_programme_status", ())?;
            assert_eq!(patron.verified_e8s, 0);
            assert_eq!(patron.assumed_e8s, 0);
            let record: Option<PatronRecord> =
                query_one(&pic, rewards, buyer, "get_patron_record", forwarded.id)?;
            assert!(
                record.is_none(),
                "memo-less treasury principal must never issue jUP"
            );
            assert_eq!(
                support::ledger::icrc1_balance(
                    &pic,
                    ledger,
                    &Account {
                        owner: relay,
                        subaccount: None
                    }
                )?,
                0
            );
            sale_faucet::check(&pic, disburser, base_recipient, staking_account)?;
            let inventory_account = Account {
                owner: rewards,
                subaccount: None,
            };
            let bonus_inventory = support::ledger::icrc1_balance(
                &pic,
                support::principals::icp_ledger(),
                &inventory_account,
            )?;
            assert!(
                bonus_inventory > 0,
                "actual age-bonus ICP must reach SNS Rewards"
            );
            let _quote: Option<Nat> = support::calls::update_bytes(
                &pic,
                rewards,
                buyer,
                "quote_gross_io_share",
                encode_args((1_000_000_000_u64, buyer))?,
            )?;
            assert_eq!(support::ledger::icrc1_balance(&pic, support::principals::icp_ledger(), &inventory_account)?, bonus_inventory,
                "positive ICP inventory and a participation quote are not authenticated IO settlement or a funded IO entitlement");
            let after_base: Runtime = query_one(&pic, rewards, buyer, "get_programme_status", ())?;
            assert_eq!(after_base.verified_e8s, 0);
            assert_eq!(after_base.assumed_e8s, 0);
            if label == "maximum" {
                // A real sale-created basket acquires native age; its zero-delay
                // member stays outside voting without an increased commitment.
                pic.advance_time(std::time::Duration::from_secs(e("year_seconds")));
                for _ in 0..30 {
                    pic.tick();
                }
                let long = participants
                    .neurons
                    .iter()
                    .find(|n| {
                        n.dissolve_state
                            == Some(sns::DissolveState::DissolveDelaySeconds(e("year_seconds")))
                    })
                    .unwrap();
                let short = participants
                    .neurons
                    .iter()
                    .find(|n| n.dissolve_state == Some(sns::DissolveState::DissolveDelaySeconds(0)))
                    .unwrap();
                let aged = get_neuron(&pic, governance, long.id.as_ref().unwrap().id.clone())?;
                let made: sns::ManageNeuronResponse = update_one(
                    &pic,
                    governance,
                    nnx,
                    "manage_neuron",
                    sns::ManageNeuron {
                        subaccount: powerful.id.as_ref().unwrap().id.clone(),
                        command: Some(sns::Command::MakeProposal(sns::Proposal {
                            title: "Native age on a real sale basket".into(),
                            summary: "No transfer; local canonical age regression".into(),
                            url: String::new(),
                            action: Some(sns::Action::Motion(sns::Motion {
                                motion_text: "Native sale-neuron participation".into(),
                            })),
                        })),
                    },
                )?;
                let Some(sns::Command1::MakeProposal(made)) = made.command else {
                    anyhow::bail!("aged basket proposal: {made:?}")
                };
                let id = made.proposal_id.unwrap();
                let data: sns::GetProposalResponse = query_one(
                    &pic,
                    governance,
                    buyer,
                    "get_proposal",
                    sns::GetProposal {
                        proposal_id: Some(id.clone()),
                    },
                )?;
                let Some(sns::Result1::Proposal(data)) = data.result else {
                    anyhow::bail!("aged basket ballot")
                };
                let at = data.proposal_creation_timestamp_seconds;
                let age = at
                    .saturating_sub(aged.aging_since_timestamp_seconds)
                    .min(e("max_neuron_age_for_age_bonus"));
                assert!(age >= e("year_seconds"));
                let stake = aged.cached_neuron_stake_e8s - aged.neuron_fees_e8s;
                let duration_weight = u128::from(stake) * 2;
                let expected = duration_weight
                    + duration_weight * u128::from(age) * 2
                        / u128::from(e("max_neuron_age_for_age_bonus"));
                assert_eq!(
                    data.ballots
                        .iter()
                        .find(|(key, _)| *key == hex::encode(&long.id.as_ref().unwrap().id))
                        .unwrap()
                        .1
                        .voting_power,
                    expected as u64
                );
                assert!(!data
                    .ballots
                    .iter()
                    .any(|(key, _)| *key == hex::encode(&short.id.as_ref().unwrap().id)));
                for (n, valid) in [(short, false), (long, true)] {
                    let voted: sns::ManageNeuronResponse = update_one(
                        &pic,
                        governance,
                        buyer,
                        "manage_neuron",
                        sns::ManageNeuron {
                            subaccount: n.id.as_ref().unwrap().id.clone(),
                            command: Some(sns::Command::RegisterVote(sns::RegisterVote {
                                proposal: Some(id.clone()),
                                vote: 2,
                            })),
                        },
                    )?;
                    assert_eq!(
                        matches!(voted.command, Some(sns::Command1::RegisterVote {})),
                        valid,
                        "{voted:?}"
                    );
                }
                let before: Nat = query_one(&pic, ledger, buyer, "icrc1_total_supply", ())?;
                pic.advance_time(std::time::Duration::from_secs(6 * 86400));
                for _ in 0..80 {
                    pic.tick();
                }
                let event: sns::RewardEvent =
                    query_one(&pic, governance, buyer, "get_latest_reward_event", ())?;
                assert_eq!(event.distributed_e8s_equivalent, 0);
                let rewarded = get_neuron(&pic, governance, long.id.as_ref().unwrap().id.clone())?;
                assert_eq!(rewarded.maturity_e8s_equivalent, 0);
                assert_eq!(
                    rewarded
                        .latest_reward_event_participation
                        .as_ref()
                        .unwrap()
                        .reward_shares,
                    Some(Nat::from(expected as u64))
                );
                assert_eq!(
                    query_one::<_, Nat>(&pic, ledger, buyer, "icrc1_total_supply", ())?,
                    before
                );
                // This authentic SNS-W deployment has the registered Ledger
                // Wasm needed by native ManageLedgerParameters. Unlike the
                // direct-genesis payout fixture, no controller-side substitute
                // supplies this governed upgrade.
                let fee: sns::ManageNeuronResponse = update_one(
                    &pic,
                    governance,
                    nnx,
                    "manage_neuron",
                    sns::ManageNeuron {
                        subaccount: powerful.id.as_ref().unwrap().id.clone(),
                        command: Some(sns::Command::MakeProposal(sns::Proposal {
                            title: "Native Ledger fee validation".into(),
                            summary: "Owned SNS-W rehearsal; preserve all Ledger state".into(),
                            url: String::new(),
                            action: Some(sns::Action::ManageLedgerParameters(
                                sns::ManageLedgerParameters {
                                    transfer_fee: Some(2_000_000),
                                    token_name: None,
                                    token_symbol: None,
                                    token_logo: None,
                                },
                            )),
                        })),
                    },
                )?;
                let Some(sns::Command1::MakeProposal(fee)) = fee.command else {
                    anyhow::bail!("native fee proposal: {fee:?}")
                };
                let id = fee.proposal_id.unwrap();
                let mut executed = false;
                for _ in 0..120 {
                    pic.advance_time(std::time::Duration::from_secs(1));
                    pic.tick();
                    let proposal: sns::GetProposalResponse = query_one(
                        &pic,
                        governance,
                        buyer,
                        "get_proposal",
                        sns::GetProposal {
                            proposal_id: Some(id.clone()),
                        },
                    )?;
                    let Some(sns::Result1::Proposal(proposal)) = proposal.result else {
                        anyhow::bail!("fee proposal lookup")
                    };
                    assert_eq!(
                        proposal.failed_timestamp_seconds, 0,
                        "native SNS-W fee update: {:?}",
                        proposal.failure_reason
                    );
                    if proposal.executed_timestamp_seconds > 0 {
                        executed = true;
                        break;
                    }
                }
                assert!(executed, "native SNS-W Ledger fee proposal must execute");
                // Governance can observe install_code before Root finishes
                // restarting the Ledger. Require the actual usable new fee.
                let mut ledger_fee = None;
                let mut fee_error = None;
                for _ in 0..120 {
                    pic.advance_time(std::time::Duration::from_secs(1));
                    for _ in 0..10 {
                        pic.tick();
                    }
                    match query_one::<_, Nat>(&pic, ledger, buyer, "icrc1_fee", ()) {
                        Ok(fee) if fee == 2_000_000_u64 => {
                            ledger_fee = Some(fee);
                            break;
                        }
                        Ok(fee) => fee_error = Some(format!("old Ledger fee: {fee}")),
                        Err(error) => fee_error = Some(error.to_string()),
                    }
                }
                assert_eq!(
                    ledger_fee,
                    Some(2_000_000_u64.into()),
                    "native Ledger restart/fee: {fee_error:?}"
                );
                let parameters: sns::NervousSystemParameters =
                    query_one(&pic, governance, buyer, "get_nervous_system_parameters", ())?;
                assert_eq!(parameters.transaction_fee_e8s, Some(2_000_000));
                assert_eq!(
                    query_one::<_, Nat>(&pic, ledger, buyer, "icrc1_total_supply", ())?,
                    before
                );
            }
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
