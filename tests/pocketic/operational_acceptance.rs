//! Exact acceptance economics with owned local authorities, never live NNX proof.
use super::*;

#[path = "maintenance_rehearsal.rs"]
mod maintenance;

#[derive(CandidType, Deserialize)]
struct Parameters {
    automatically_advance_target_version: Option<bool>,
    max_age_bonus_percentage: Option<u64>,
    max_dissolve_delay_bonus_percentage: Option<u64>,
    voting_rewards_parameters: Option<RewardParameters>,
}
#[derive(CandidType, Deserialize)]
struct RewardParameters {
    initial_reward_rate_basis_points: Option<u64>,
}
#[derive(CandidType, Deserialize, Debug)]
struct NeuronId {
    id: Vec<u8>,
}
#[derive(CandidType, Deserialize, Debug)]
struct Neuron {
    id: Option<NeuronId>,
    cached_neuron_stake_e8s: u64,
    dissolve_state: Option<DissolveState>,
    aging_since_timestamp_seconds: u64,
    voting_power_percentage_multiplier: u64,
    neuron_fees_e8s: u64,
    auto_stake_maturity: Option<bool>,
    permissions: Vec<Permission>,
}
#[derive(CandidType, Deserialize, Debug)]
struct Permission {
    principal: Option<Principal>,
    permission_type: Vec<i32>,
}
#[derive(CandidType, Deserialize, Debug, PartialEq)]
enum DissolveState {
    DissolveDelaySeconds(u64),
    WhenDissolvedTimestampSeconds(u64),
}
#[derive(CandidType)]
struct GetNeuron {
    neuron_id: Option<NeuronId>,
}
#[derive(CandidType, Deserialize)]
enum NeuronResult {
    Error(candid::Reserved),
    Neuron(Neuron),
}
#[derive(CandidType, Deserialize)]
struct NeuronReply {
    result: Option<NeuronResult>,
}

fn succeeded(reply: &IDLValue, name: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(opted(field(reply, "command")?)?, IDLValue::Variant(v)
        if v.0.id.get_id() == candid::idl_hash(name)),
        "{reply}"
    );
    Ok(())
}

fn permission_codes(permission: &Permission) -> Vec<i32> {
    // Native permission mutation uses an unordered set. Sorting preserves the
    // exact-code/length assertion, including rejection of extras or duplicates.
    let mut codes = permission.permission_type.clone();
    codes.sort_unstable();
    codes
}

fn not_authorized(reply: &IDLValue) -> Result<()> {
    succeeded(reply, "Error")?;
    let IDLValue::Variant(error) = opted(field(reply, "command")?)? else {
        unreachable!()
    };
    // Exact official ErrorType::NotAuthorized, not any generic failure.
    assert_eq!(field(&error.0.val, "error_type")?, &IDLValue::Int32(2));
    Ok(())
}

fn neuron_state(
    pic: &PocketIc,
    governance: Principal,
    caller: Principal,
    id: &[u8],
) -> Result<Neuron> {
    let reply: NeuronReply = query_one(
        pic,
        governance,
        caller,
        "get_neuron",
        GetNeuron {
            neuron_id: Some(NeuronId { id: id.to_vec() }),
        },
    )?;
    match reply.result {
        Some(NeuronResult::Neuron(n)) => Ok(n),
        _ => bail!("governance neuron missing"),
    }
}

fn ballot_power(
    pic: &PocketIc,
    governance: Principal,
    caller: Principal,
    proposal: u64,
    neuron: &[u8],
) -> Result<u64> {
    #[derive(CandidType, Deserialize)]
    struct Ballot {
        voting_power: u64,
    }
    #[derive(CandidType, Deserialize)]
    struct Data {
        ballots: Vec<(String, Ballot)>,
    }
    #[derive(CandidType, Deserialize)]
    enum ResultValue {
        Proposal(Data),
        Error(candid::Reserved),
    }
    #[derive(CandidType, Deserialize)]
    struct Reply {
        result: Option<ResultValue>,
    }
    let reply: Reply = query_one(
        pic,
        governance,
        caller,
        "get_proposal",
        GetProposal {
            proposal_id: Some(ProposalId { id: proposal }),
        },
    )?;
    let Some(ResultValue::Proposal(p)) = reply.result else {
        bail!("proposal ballot missing")
    };
    Ok(p.ballots
        .into_iter()
        .find(|(id, _)| *id == hex::encode(neuron))
        .context("dominant ballot missing")?
        .1
        .voting_power)
}

fn proposal_id(reply: &IDLValue) -> Result<u64> {
    succeeded(reply, "MakeProposal")?;
    let IDLValue::Variant(v) = opted(field(reply, "command")?)? else {
        unreachable!()
    };
    let IDLValue::Nat64(id) = field(opted(field(&v.0.val, "proposal_id")?)?, "id")? else {
        bail!("proposal ID missing")
    };
    Ok(*id)
}

fn wait_executed(pic: &PocketIc, governance: Principal, caller: Principal, id: u64) -> Result<()> {
    for _ in 0..100 {
        pic.advance_time(Duration::from_secs(2));
        pic.tick();
        let reply: ProposalReply = query_one(
            pic,
            governance,
            caller,
            "get_proposal",
            GetProposal {
                proposal_id: Some(ProposalId { id }),
            },
        )?;
        let Some(ProposalResult::Proposal(status)) = reply.result else {
            bail!("proposal {id} missing")
        };
        anyhow::ensure!(
            status.failed_timestamp_seconds == 0,
            "governed upgrade {id} failed"
        );
        if status.executed_timestamp_seconds > 0 {
            return Ok(());
        }
    }
    bail!("governed upgrade {id} did not execute within bounded rounds")
}

fn query_running(
    pic: &PocketIc,
    canister: Principal,
    caller: Principal,
    method: &str,
) -> Result<Vec<u8>> {
    // SNS Root acknowledges change_canister before its stop/install/start tail
    // finishes. Wait for that specific platform state, never ignore other errors.
    for _ in 0..100 {
        match pic.query_call(canister, caller, method, encode_args(())?) {
            Ok(reply) => return Ok(reply),
            Err(error) if error.error_code == pocket_ic::ErrorCode::CanisterStopped => {
                pic.advance_time(Duration::from_secs(2));
                pic.tick();
            }
            Err(error) => bail!("{method}: {error:?}"),
        }
    }
    bail!("{method}: canister did not restart within bounded rounds")
}

fn wait_module(
    pic: &PocketIc,
    canister: Principal,
    controller: Principal,
    expected: &[u8],
) -> Result<()> {
    for _ in 0..100 {
        let status = pic
            .canister_status(canister, Some(controller))
            .map_err(|error| anyhow::anyhow!("physical upgrade status: {error:?}"))?;
        if status.module_hash.as_deref() == Some(expected) {
            return Ok(());
        }
        pic.advance_time(Duration::from_secs(2));
        pic.tick();
    }
    bail!("physical module did not reach the proposed package hash")
}

#[test]
#[ignore = "official SNS tuple and owned PocketIC topology"]
fn exact_genesis_governance_handover_and_production_reserve() -> Result<()> {
    // Build before starting a server; linking is not bounded by its idle TTL.
    let production =
        support::wasm::build_wasm_cached_for_test(&OnceLock::new(), "jupiter-sns-rewards", None)?;
    let repo = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let provision = Command::new("python3")
        .args(["tools/acceptance/artifacts.py", "--fetch"])
        .current_dir(&repo)
        .output()?;
    anyhow::ensure!(
        provision.status.success(),
        "verified official tuple required"
    );
    let artifacts = PathBuf::from(String::from_utf8(provision.stdout)?.trim());
    let evidence = repo
        .join("target")
        .join(format!("operational-acceptance-{}", std::process::id()));
    fs::create_dir(&evidence)?;
    // Preserve the dated rehearsal independently of the unresolved live schedule.
    let candidate_inputs: serde_json::Value = serde_json::from_slice(&fs::read(
        repo.join("deploy/acceptance/historical/2026-11-15-candidate-inputs.json"),
    )?)?;
    let genesis = candidate_inputs["genesis_timestamp_seconds"]
        .as_u64()
        .unwrap();
    let activation = candidate_inputs["acceptance_start_timestamp_seconds"]
        .as_u64()
        .unwrap();
    assert!(genesis < activation);
    let pic = support::pocketic::builder()
        .with_sns_subnet()
        .with_nns_subnet()
        .with_application_subnet()
        .with_initial_time(pocket_ic::Time::from_nanos_since_unix_epoch(
            genesis * 1_000_000_000,
        ))
        .build();
    let bindings: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join("deploy/acceptance/profile.json"))?)?;
    let inputs: serde_json::Value = serde_json::from_slice(&fs::read(
        repo.join("deploy/acceptance/operator-inputs.json"),
    )?)?;
    let operator = Principal::from_text(bindings["operator"].as_str().unwrap())?;
    let bootstrap = operator;
    let owner_decisions: serde_json::Value = serde_json::from_slice(&fs::read(
        repo.join("deploy/acceptance/owner-decisions.json"),
    )?)?;
    // Retained only in controller sets. No call is sent as the personal user.
    let personal = Principal::from_text(
        owner_decisions["protected_personal_principal"]
            .as_str()
            .unwrap(),
    )?;
    // The final executor remains an explicitly owned local fixture, not the
    // unavailable real NNX authority. No mainnet keys enter PocketIC.
    let executor = Principal::self_authenticating([85; 32]);
    let participant_a =
        Principal::from_text(inputs["test_allocations"][0]["owner"].as_str().unwrap())?;
    let user = Principal::from_text(inputs["test_allocations"][2]["owner"].as_str().unwrap())?;
    let mut roles = serde_json::Map::new();
    for name in [
        "sns_root",
        "sns_governance",
        "sns_ledger",
        "sns_index",
        "sns_swap",
        "jupiter_disburser",
        "jupiter_faucet",
        "jupiter_lifeline",
        "jupiter_sns_rewards",
        "jupiter_historian",
        "jupiter_relay",
        "jupiter_faucet_frontend",
    ] {
        // These are fresh local canisters at the approved principals, not a
        // copy or observation of any existing mainnet code, state or authority.
        let expected = Principal::from_text(bindings["roles"][name].as_str().unwrap())?;
        let id = pic
            .create_canister_with_params(
                Some(operator),
                pocket_ic::CreateCanisterParams {
                    cycles: Some(3_000_000_000_000),
                    placement: Some(pocket_ic::CreateCanisterPlacement::CanisterId(expected)),
                    ..Default::default()
                },
            )
            .map_err(|error| anyhow::anyhow!("local approved identity {name}: {error}"))?;
        assert_eq!(id, expected);
        assert_eq!(pic.cycle_balance(id), 3_000_000_000_000);
        pic.set_controllers(id, Some(operator), vec![operator, personal])
            .map_err(|e| anyhow::anyhow!("local bootstrap controllers: {e:?}"))?;
        roles.insert(name.into(), json!(id.to_text()));
    }
    let role = |name: &str| Principal::from_text(roles[name].as_str().unwrap()).unwrap();

    fs::write(
        evidence.join("context.json"),
        serde_json::to_vec(&json!({"roles":roles,
        "operator":operator.to_text(), "bootstrap":bootstrap.to_text(), "user":user.to_text(),
        "participant_a":participant_a.to_text(),
        "timestamp":pic.get_time().as_nanos_since_unix_epoch()/1_000_000_000}))?,
    )?;
    checked_command(
        Command::new("python3")
            .arg("-c")
            .arg(
                r#"
import json, sys
from pathlib import Path
sys.path.insert(0, 'tools/acceptance')
import profile, operational as op
c=json.loads(Path(sys.argv[1]).read_text()); out=Path(sys.argv[2])
p=json.loads(profile.PROFILE.read_text())
d=json.loads((profile.PROFILE.parent/'historical/2026-11-15-candidate-inputs.json').read_text())
assert p['roles']==c['roles'] and d['operator']==c['operator']
assert d['genesis_timestamp_seconds']==c['timestamp']
assert d['test_allocations'][0]['owner']==c['participant_a']
assert d['test_allocations'][2]['owner']==c['user']
op.candidate(p,d,sys.argv[3],out/'candidate')
# Local archive capacity acceleration only. Keep the exact live 2000/1000
# threshold, 4.5T funding and initial controllers. Retain canonical arguments.
import candid as cd
ledger=out/'candidate/sns/ledger.did'
text=ledger.read_text()
assert 'opt 1073741824' in text
(out/'candidate/local-small-archive-ledger.bin').write_bytes(cd.encode_checked(
    out/'candidate/sns/ledger.interface.did','(LedgerArg)',text.replace('opt 1073741824','opt 65536')))

"#,
            )
            .arg(evidence.join("context.json"))
            .arg(&evidence)
            .arg(&artifacts)
            .current_dir(&repo),
    )?;
    let evidence = evidence.join("candidate");
    for name in ["governance", "ledger", "index", "swap", "root"] {
        let package = fs::read(artifacts.join(format!("{name}.wasm.gz")))?;
        let expected_hash = sha2::Sha256::digest(&package).to_vec();
        pic.install_canister(
            role(&format!("sns_{name}")),
            package,
            fs::read(if name == "ledger" {
                evidence.join("local-small-archive-ledger.bin")
            } else {
                evidence.join(format!("sns/{name}.bin"))
            })?,
            Some(operator),
        );
        assert_eq!(
            pic.canister_status(role(&format!("sns_{name}")), Some(operator))
                .expect("installed official package status")
                .module_hash,
            Some(expected_hash)
        );
    }
    let governance = role("sns_governance");
    let root = role("sns_root");
    let rewards = role("jupiter_sns_rewards");
    let ledger = role("sns_ledger");
    pic.install_canister(
        rewards,
        production.clone(),
        fs::read(evidence.join("applications/jupiter_sns_rewards.bin"))?,
        Some(operator),
    );
    for (id, controller) in [
        (governance, root),
        (root, governance),
        (ledger, root),
        (role("sns_index"), root),
        (role("sns_swap"), root),
        (rewards, root),
    ] {
        pic.set_controllers(id, Some(operator), vec![operator, personal, controller])
            .map_err(|e| anyhow::anyhow!("controllers: {e:?}"))?;
    }
    let mode: GovernanceMode = query_one(&pic, governance, operator, "get_mode", Empty {})?;
    assert_eq!(mode.mode, Some(1));
    let params: Parameters = query_one(
        &pic,
        governance,
        operator,
        "get_nervous_system_parameters",
        (),
    )?;
    assert_eq!(params.automatically_advance_target_version, Some(false));
    assert_eq!(params.max_age_bonus_percentage, Some(200));
    assert_eq!(params.max_dissolve_delay_bonus_percentage, Some(100));
    assert_eq!(
        params
            .voting_rewards_parameters
            .unwrap()
            .initial_reward_rate_basis_points,
        Some(0)
    );
    let ids: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence.join("neuron-ids.json"))?)?;
    let dominant = hex::decode(ids["governance"].as_str().unwrap())?;
    let transport = hex::decode(ids["reserve_transport"].as_str().unwrap())?;
    let user_sub = account_neuron_subaccount(user, 21);
    #[derive(CandidType, Deserialize)]
    struct Unconfigured {
        config: Option<candid::Reserved>,
    }
    let runtime: Unconfigured = query_one(&pic, rewards, operator, "get_programme_status", ())?;
    assert!(runtime.config.is_none());
    let reserve = Account {
        owner: rewards,
        subaccount: None,
    };
    assert_eq!(support::ledger::icrc1_balance(&pic, ledger, &reserve)?, 0);
    let total: Nat = query_one(&pic, ledger, operator, "icrc1_total_supply", ())?;
    assert_eq!(total, Nat::from(100_000_000_u64 * 100_000_000));
    for allocation in candidate_inputs["test_allocations"].as_array().unwrap() {
        let owner = Principal::from_text(allocation["owner"].as_str().unwrap())?;
        let sub = account_neuron_subaccount(owner, allocation["memo"].as_u64().unwrap());
        let n = neuron_state(&pic, governance, operator, &sub)?;
        assert_eq!(n.id.unwrap().id, sub);
        assert_eq!(n.cached_neuron_stake_e8s, 5_000 * 100_000_000);
        assert_eq!(
            n.dissolve_state,
            Some(DissolveState::DissolveDelaySeconds(
                allocation["dissolve_delay_seconds"].as_u64().unwrap()
            ))
        );
        assert_eq!(n.permissions.len(), 1);
        assert_eq!(n.permissions[0].principal, Some(owner));
        assert_eq!(
            permission_codes(&n.permissions[0]),
            (1..=10).collect::<Vec<_>>()
        );
        assert!(!n.auto_stake_maturity.unwrap_or(false));
        assert_eq!(
            support::ledger::icrc1_balance(
                &pic,
                ledger,
                &Account {
                    owner: governance,
                    subaccount: Some(sub.try_into().unwrap())
                }
            )?,
            5_000 * 100_000_000
        );
    }
    let did = evidence.join("sns/governance.interface.did");
    let initial = neuron_state(&pic, governance, bootstrap, &dominant)?;
    assert_eq!(initial.permissions.len(), 1);
    assert_eq!(permission_codes(&initial.permissions[0]), vec![3, 4, 10]);
    let action="variant { ManageNervousSystemParameters = record { automatically_advance_target_version = opt false } }";
    // Authentic native submission by the temporary voter (automatic Yes).
    let submitted = proposal_id(&manage(
        &pic, governance, bootstrap, &dominant, action, &did,
    )?)?;
    let submitter_power = ballot_power(&pic, governance, bootstrap, submitted, &dominant)?;
    assert!(submitter_power > 10_000_000_u64 * 100_000_000);
    // Every non-voting permission, including mixed requests, must fail for both
    // self and another principal while global grantability remains unchanged.
    for recipient in [bootstrap, executor] {
        for code in [1, 2, 5, 6, 7, 8, 9] {
            for permissions in [format!("{code} : int32"), format!("3 : int32; {code}")] {
                let escalation = format!("variant {{ AddNeuronPermissions = record {{ principal_id = opt principal \"{recipient}\"; permissions_to_add = opt record {{ permissions = vec {{ {permissions} }} }} }} }}");
                not_authorized(&manage_command(
                    &pic,
                    governance,
                    bootstrap,
                    &dominant,
                    &escalation,
                    &did,
                )?)?;
            }
        }
    }
    // Explicit native Vote by the temporary voter on a small participant's proposal.
    let id = proposal_id(&manage(&pic, governance, user, &user_sub, action, &did)?)?;
    let initial_power = ballot_power(&pic, governance, bootstrap, id, &dominant)?;
    let vote = format!("variant {{ RegisterVote = record {{ proposal = opt record {{ id = {id} }}; vote = 1 : int32 }} }}");
    succeeded(
        &manage_command(&pic, governance, bootstrap, &dominant, &vote, &did)?,
        "RegisterVote",
    )?;
    // Grant voting-only rights. The original staking identity stays fixed.
    let grant = format!("variant {{ AddNeuronPermissions = record {{ principal_id = opt principal \"{executor}\"; permissions_to_add = opt record {{ permissions = vec {{ 3 : int32; 4; 10 }} }} }} }}");
    succeeded(
        &manage_command(&pic, governance, bootstrap, &dominant, &grant, &did)?,
        "AddNeuronPermission",
    )?;
    let submitted = proposal_id(&manage(
        &pic, governance, executor, &dominant, action, &did,
    )?)?;
    assert_eq!(
        ballot_power(&pic, governance, executor, submitted, &dominant)?,
        submitter_power,
        "permission changes preserve actual native ballot weight at the fixed local clock"
    );
    // The final executor's explicit Vote decides a distinct pending proposal.
    let id = proposal_id(&manage(&pic, governance, user, &user_sub, action, &did)?)?;
    let before: ProposalReply = query_one(
        &pic,
        governance,
        operator,
        "get_proposal",
        GetProposal {
            proposal_id: Some(ProposalId { id }),
        },
    )?;
    let Some(ProposalResult::Proposal(before)) = before.result else {
        bail!("proposal missing")
    };
    assert_eq!(before.executed_timestamp_seconds, 0);
    let vote = format!("variant {{ RegisterVote = record {{ proposal = opt record {{ id = {id} }}; vote = 1 : int32 }} }}");
    succeeded(
        &manage_command(&pic, governance, executor, &dominant, &vote, &did)?,
        "RegisterVote",
    )?;
    for _ in 0..40 {
        pic.tick();
    }
    let after: ProposalReply = query_one(
        &pic,
        governance,
        operator,
        "get_proposal",
        GetProposal {
            proposal_id: Some(ProposalId { id }),
        },
    )?;
    let Some(ProposalResult::Proposal(after)) = after.result else {
        bail!("proposal missing")
    };
    assert!(after.executed_timestamp_seconds > 0);
    // Temporary voter removes its OWN rights only after the successor proved
    // both SubmitProposal and Vote, retaining the successor's permission manager.
    let remove = format!("variant {{ RemoveNeuronPermissions = record {{ principal_id = opt principal \"{bootstrap}\"; permissions_to_remove = opt record {{ permissions = vec {{ 3 : int32; 4; 10 }} }} }} }}");
    succeeded(
        &manage_command(&pic, governance, bootstrap, &dominant, &remove, &did)?,
        "RemoveNeuronPermission",
    )?;
    let handed = neuron_state(&pic, governance, executor, &dominant)?;
    assert_eq!(
        handed.id.as_ref().unwrap().id,
        initial.id.as_ref().unwrap().id
    );
    assert_eq!(
        handed.cached_neuron_stake_e8s,
        initial.cached_neuron_stake_e8s
    );
    assert_eq!(handed.dissolve_state, initial.dissolve_state);
    assert_eq!(
        handed.dissolve_state,
        Some(DissolveState::DissolveDelaySeconds(31_557_600))
    );
    assert_eq!(
        handed.aging_since_timestamp_seconds,
        initial.aging_since_timestamp_seconds
    );
    assert_eq!(
        handed.voting_power_percentage_multiplier,
        initial.voting_power_percentage_multiplier
    );
    assert_eq!(handed.neuron_fees_e8s, initial.neuron_fees_e8s);
    assert_eq!(handed.permissions.len(), 1);
    assert_eq!(handed.permissions[0].principal, Some(executor));
    assert_eq!(permission_codes(&handed.permissions[0]), vec![3, 4, 10]);
    not_authorized(&manage(
        &pic, governance, bootstrap, &dominant, action, &did,
    )?)?;
    not_authorized(&manage_command(
        &pic, governance, bootstrap, &dominant, &grant, &did,
    )?)?;
    not_authorized(&manage_command(
        &pic, governance, bootstrap, &dominant, &remove, &did,
    )?)?;
    let id = proposal_id(&manage(&pic, governance, user, &user_sub, action, &did)?)?;
    let vote = format!("variant {{ RegisterVote = record {{ proposal = opt record {{ id = {id} }}; vote = 1 : int32 }} }}");
    not_authorized(&manage_command(
        &pic, governance, bootstrap, &dominant, &vote, &did,
    )?)?;
    succeeded(
        &manage_command(&pic, governance, executor, &dominant, &vote, &did)?,
        "RegisterVote",
    )?;
    assert_eq!(
        ballot_power(&pic, governance, executor, id, &dominant)?,
        initial_power
    );
    println!("native [3,4,10] handover: proposal/vote; 28 escalation denials; self-revoke; four NotAuthorized denials; unchanged neuron/stake/delay/age/ballot weight");
    // Use the generated typed recognition args through Governance's native
    // upgrade action, with production application code and real disbursement.
    let path = evidence.join("production.wasm");
    fs::write(&path, &production)?;
    let compressed = Command::new("gzip")
        .args(["-n", "-c"])
        .arg(&path)
        .output()?;
    anyhow::ensure!(compressed.status.success(), "gzip candidate");
    fs::write(evidence.join("production.wasm.gz"), &compressed.stdout)?;
    let args = fs::read(evidence.join("governance/recognition-upgrade-args.bin"))?;
    let action=format!("variant {{ UpgradeSnsControlledCanister = record {{ canister_id = opt principal \"{rewards}\"; new_canister_wasm = blob \"{}\"; mode = opt (3 : int32); canister_upgrade_arg = opt blob \"{}\" }} }}",
        compressed.stdout.iter().map(|b|format!("\\{b:02x}")).collect::<String>(),args.iter().map(|b|format!("\\{b:02x}")).collect::<String>());
    let id = proposal_id(&manage(&pic, governance, user, &user_sub, &action, &did)?)?;
    succeeded(&manage_command(&pic,governance,executor,&dominant,&format!("variant {{ RegisterVote = record {{ proposal = opt record {{ id = {id} }}; vote = 1 : int32 }} }}"),&did)?,"RegisterVote")?;
    wait_executed(&pic, governance, operator, id)?;
    wait_module(
        &pic,
        rewards,
        root,
        &sha2::Sha256::digest(&compressed.stdout),
    )?;
    let mut verified = false;
    for _ in 0..100 {
        pic.advance_time(Duration::from_secs(60));
        pic.tick();
        let reply = query_running(&pic, rewards, operator, "get_reserve_transport_status")?;
        let state = IDLArgs::from_bytes(&reply)?.args.remove(0);
        if let IDLValue::Variant(v) = state {
            if v.0.id.get_id() != candid::idl_hash("Verified") {
                continue;
            }
            assert_eq!(
                field(&v.0.val, "amount_e8s")?,
                &IDLValue::Nat64(89_979_900_u64 * 100_000_000 - 1_000_000)
            );
            assert_eq!(field(&v.0.val, "fee_e8s")?, &IDLValue::Nat64(1_000_000));
            verified = true;
            break;
        }
    }
    assert!(verified, "native reserve transport must complete");
    #[derive(CandidType, Deserialize)]
    struct Exclusions {
        protocol_governance_neuron: Vec<u8>,
        excluded_jup_neurons: Vec<Vec<u8>>,
    }
    #[derive(CandidType, Deserialize)]
    struct Configured {
        config: Option<Exclusions>,
    }
    let configured: Configured = query_one(&pic, rewards, executor, "get_programme_status", ())?;
    let configured = configured.config.context("installed programme config")?;
    assert_eq!(configured.protocol_governance_neuron, dominant);
    assert!(
        configured.excluded_jup_neurons.contains(&dominant),
        "external IO exclusion keeps the original neuron ID across permission handover"
    );
    let paid = support::ledger::icrc1_balance(&pic, ledger, &reserve)?;
    assert_eq!(paid, 89_979_900_u64 * 100_000_000 - 1_000_000);
    let source = Account {
        owner: governance,
        subaccount: Some(transport.try_into().unwrap()),
    };
    assert_eq!(support::ledger::icrc1_balance(&pic, ledger, &source)?, 0);
    let n: NeuronReply = query_one(
        &pic,
        governance,
        operator,
        "get_neuron",
        GetNeuron {
            neuron_id: Some(NeuronId {
                id: dominant.clone(),
            }),
        },
    )?;
    let Some(NeuronResult::Neuron(n)) = n.result else {
        bail!("dominant neuron missing")
    };
    assert_eq!(n.id.unwrap().id, dominant);
    assert_eq!(n.cached_neuron_stake_e8s, 10_000_000_u64 * 100_000_000);
    assert_eq!(n.permissions.len(), 1);
    assert_eq!(n.permissions[0].principal, Some(executor));
    assert_eq!(permission_codes(&n.permissions[0]), vec![3, 4, 10]);
    assert_eq!(
        support::ledger::icrc1_balance(
            &pic,
            ledger,
            &Account {
                owner: governance,
                subaccount: Some(dominant.clone().try_into().unwrap())
            }
        )?,
        10_000_000_u64 * 100_000_000,
        "handover must not move the original staking Account"
    );
    for _ in 0..20 {
        pic.advance_time(Duration::from_secs(60));
        pic.tick();
    }
    assert_eq!(
        support::ledger::icrc1_balance(&pic, ledger, &reserve)?,
        paid,
        "no reserve replay"
    );
    // A second actual governed upgrade omits configuration. The settled
    // transport journal and its Ledger receipt must survive without replay.
    // A complete second gzip package of the same production module gives this
    // upgrade a distinct physical hash; an early Root acknowledgement cannot
    // satisfy the post-upgrade journal assertion with the predecessor module.
    let second = Command::new("gzip")
        .args(["-n", "-9", "-c"])
        .arg(&path)
        .output()?;
    anyhow::ensure!(second.status.success(), "second deterministic package");
    assert_ne!(second.stdout, compressed.stdout);
    let preserve_action = format!("variant {{ UpgradeSnsControlledCanister = record {{ canister_id = opt principal \"{rewards}\"; new_canister_wasm = blob \"{}\"; mode = opt (3 : int32); canister_upgrade_arg = opt blob \"\\44\\49\\44\\4c\\00\\00\" }} }}",
        second.stdout.iter().map(|b| format!("\\{b:02x}")).collect::<String>());
    let id = proposal_id(&manage(
        &pic,
        governance,
        user,
        &user_sub,
        &preserve_action,
        &did,
    )?)?;
    succeeded(&manage_command(&pic,governance,executor,&dominant,&format!("variant {{ RegisterVote = record {{ proposal = opt record {{ id = {id} }}; vote = 1 : int32 }} }}"),&did)?,"RegisterVote")?;
    wait_executed(&pic, governance, operator, id)?;
    wait_module(&pic, rewards, root, &sha2::Sha256::digest(&second.stdout))?;
    let after: ProposalReply = query_one(
        &pic,
        governance,
        operator,
        "get_proposal",
        GetProposal {
            proposal_id: Some(ProposalId { id }),
        },
    )?;
    let Some(ProposalResult::Proposal(after)) = after.result else {
        bail!("preserving upgrade proposal missing")
    };
    assert!(after.executed_timestamp_seconds > 0);
    assert_eq!(after.failed_timestamp_seconds, 0);
    let reply = query_running(&pic, rewards, operator, "get_reserve_transport_status")?;
    let state = IDLArgs::from_bytes(&reply)?.args.remove(0);
    anyhow::ensure!(
        matches!(state, IDLValue::Variant(v) if v.0.id.get_id() == candid::idl_hash("Verified")),
        "reserve journal must survive upgrade"
    );
    assert_eq!(
        support::ledger::icrc1_balance(&pic, ledger, &reserve)?,
        paid,
        "governed upgrade cannot repeat reserve disbursement"
    );
    let status = pic
        .canister_status(rewards, Some(root))
        .expect("Root controls application");
    let mut expected = vec![operator, personal, root];
    expected.sort();
    let mut actual = status.settings.controllers;
    actual.sort();
    assert_eq!(
        actual, expected,
        "retain both bootstrap controllers through rehearsals"
    );
    assert_eq!(
        status.module_hash,
        Some(sha2::Sha256::digest(&second.stdout).to_vec())
    );
    // Remove bootstrap authority only after actual governed upgrades and durable
    // receipt preservation succeeded. These are owned local canisters only.
    for (id, controller) in [
        (governance, root),
        (root, governance),
        (ledger, root),
        (role("sns_index"), root),
        (role("sns_swap"), root),
        (rewards, root),
    ] {
        let final_controllers = if id == rewards {
            vec![controller]
        } else {
            vec![controller, operator]
        };
        pic.set_controllers(id, Some(operator), final_controllers.clone())
            .map_err(|e| anyhow::anyhow!("post-rehearsal handoff: {e:?}"))?;
        assert_eq!(
            pic.canister_status(id, Some(controller))
                .expect("final controller checkpoint")
                .settings
                .controllers,
            final_controllers
        );
    }
    let denied = pic
        .upgrade_canister(
            rewards,
            second.stdout.clone(),
            candid::encode_args(())?,
            Some(operator),
        )
        .expect_err("removed bootstrap must not retain app-upgrade authority");
    assert_eq!(
        denied.error_code,
        pocket_ic::ErrorCode::CanisterInvalidController
    );
    maintenance::rehearse(
        &pic,
        &repo,
        &evidence,
        &artifacts,
        &roles,
        operator,
        executor,
        &dominant,
        &did,
        &second.stdout,
    )?;
    println!(
        "exact allocation / local voter handover / governed production reserve passed: {}",
        evidence.display()
    );
    Ok(())
}

fn account_neuron_subaccount(controller: Principal, memo: u64) -> Vec<u8> {
    let mut hash = sha2::Sha256::new();
    hash.update(b"\x0cneuron-stake");
    hash.update(controller.as_slice());
    hash.update(memo.to_be_bytes());
    hash.finalize().to_vec()
}
