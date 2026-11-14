//! Real official SNS integration. Test genesis is deliberately not mainnet economics.
use super::*;
use candid::types::value::{IDLArgs, IDLValue};
use serde_json::json;
use sha2::Digest;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

fn checked_command(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    anyhow::ensure!(
        status.success(),
        "acceptance preparation failed: {command:?}"
    );
    Ok(())
}

fn field<'a>(value: &'a IDLValue, name: &str) -> Result<&'a IDLValue> {
    match value {
        IDLValue::Record(fields) => fields
            .iter()
            .find(|f| f.id.get_id() == candid::idl_hash(name))
            .map(|f| &f.val)
            .with_context(|| format!("missing {name}")),
        _ => bail!("expected record for {name}: {value}"),
    }
}

fn opted(value: &IDLValue) -> Result<&IDLValue> {
    match value {
        IDLValue::Opt(value) => Ok(value),
        _ => bail!("expected present option: {value}"),
    }
}

fn manage(
    pic: &PocketIc,
    governance: Principal,
    caller: Principal,
    sub: &[u8],
    action: &str,
    did: &Path,
) -> Result<IDLValue> {
    let command = format!("variant {{ MakeProposal = record {{ title = \"Acceptance lifecycle\"; summary = \"Local source-bound acceptance exercise\"; url = \"https://example.com\"; action = opt {action} }} }}");
    manage_command(pic, governance, caller, sub, &command, did)
}

fn manage_command(
    pic: &PocketIc,
    governance: Principal,
    caller: Principal,
    sub: &[u8],
    command: &str,
    did: &Path,
) -> Result<IDLValue> {
    let arg = format!(
        "(record {{ subaccount = blob \"{}\"; command = opt {command} }})",
        sub.iter().map(|b| format!("\\{b:02x}")).collect::<String>()
    );
    let mut child = Command::new("didc")
        .args(["encode", "--defs"])
        .arg(did)
        .args(["--method", "manage_neuron"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(arg.as_bytes())?;
    let encoded = child.wait_with_output()?;
    anyhow::ensure!(
        encoded.status.success(),
        "manage_neuron Candid rejected: {}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    let reply = pic
        .update_call(
            governance,
            caller,
            "manage_neuron",
            hex::decode(String::from_utf8(encoded.stdout)?.trim())?,
        )
        .map_err(|e| anyhow::anyhow!("manage_neuron: {e:?}"))?;
    Ok(IDLArgs::from_bytes(&reply)?.args.remove(0))
}

#[derive(CandidType, Deserialize, Debug)]
struct SetupArgs {
    target_canister_ids: Vec<Principal>,
    surplus_recipients: Vec<SetupRecipient>,
}
#[derive(CandidType, Deserialize)]
struct GovernanceMode {
    mode: Option<i32>,
}
#[derive(CandidType, Deserialize, Debug)]
enum SetupRecipient {
    Principal { principal: Principal, memo: Vec<u8> },
    Neuron { neuron_id: u64, memo: Vec<u8> },
}
#[derive(CandidType, Deserialize, Debug)]
struct SetupView {
    setup_account: Option<Account>,
    nominal_minimum_e8s: u64,
}
#[derive(CandidType, Deserialize, Debug)]
enum SetupResult {
    Ok(SetupView),
    Err(String),
}
#[derive(CandidType, Deserialize, Debug)]
enum NotifyResult {
    BelowMinimum(candid::Reserved),
    BelowCurrentRequirement(candid::Reserved),
    Busy,
    InProgress(candid::Reserved),
    Active { relay_canister_id: Principal },
    FailedPreSpend { message: String },
    ManualRecoveryRequired(candid::Reserved),
}

#[derive(CandidType, Deserialize)]
struct RelayRewardContext {
    sns_root_canister_id: Principal,
    sns_governance_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
}
#[derive(CandidType)]
struct ResolveDefaultIcpAccountsArgs {
    snapshot_id: u64,
    account_identifiers: Vec<Vec<u8>>,
}
#[derive(CandidType, Deserialize, Debug, PartialEq)]
enum ResolveDefaultIcpAccountsResult {
    Ok(Vec<Option<Principal>>),
    SnapshotChanged,
    TooManyAccounts,
    InvalidAccountIdentifier { index: u32 },
}
#[derive(CandidType, Deserialize)]
struct DisburserConfig {
    neuron_id: u64,
    normal_recipient: Account,
    age_bonus_recipient_1: Account,
    age_bonus_recipient_2: Account,
    rescue_controller: Principal,
    autonomous_rescue_armed: Option<bool>,
}
#[derive(CandidType, Deserialize)]
struct FaucetConfig {
    staking_account: Account,
    funding_source_account: Account,
    rescue_controller: Principal,
    autonomous_rescue_armed: Option<bool>,
}
#[derive(CandidType)]
struct Arm {
    autonomous_rescue_armed: Option<bool>,
}
#[derive(CandidType)]
struct Empty {}
#[derive(CandidType)]
struct HttpRequest {
    url: String,
    method: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    certificate_version: Option<u16>,
}
#[derive(CandidType, Deserialize)]
struct HttpResponse {
    status_code: u16,
    body: Vec<u8>,
}
#[derive(CandidType)]
enum TrackingReason {
    SnsDiscovery,
}
#[derive(CandidType)]
struct ListCanisters {
    start_after: Option<Principal>,
    limit: Option<u32>,
    tracking_reason_filter: Option<TrackingReason>,
}
#[derive(CandidType, Deserialize)]
struct CanisterItem {
    canister_id: Principal,
}
#[derive(CandidType, Deserialize)]
struct Canisters {
    items: Vec<CanisterItem>,
}
#[derive(CandidType, Deserialize)]
struct ProposalId {
    id: u64,
}
#[derive(CandidType)]
struct GetProposal {
    proposal_id: Option<ProposalId>,
}
#[derive(CandidType, Deserialize)]
struct ProposalStatus {
    decided_timestamp_seconds: u64,
    executed_timestamp_seconds: u64,
    failed_timestamp_seconds: u64,
}
#[derive(CandidType, Deserialize)]
enum ProposalResult {
    Proposal(ProposalStatus),
    Error(candid::Reserved),
}
#[derive(CandidType, Deserialize)]
struct ProposalReply {
    result: Option<ProposalResult>,
}

#[path = "acceptance_reward_pots.rs"]
mod reward_pots;

#[test]
#[ignore = "owned PocketIC topology and exact official SNS artifacts required"]
fn real_jup_sns_ownership_rewards_factory_and_governed_upgrade() -> Result<()> {
    // Prepare shared mock binaries before starting the instance. Critical SNS
    // components below are the verified official implementations, never mocks.
    let nns_mock = governance_wasm()?;
    let rewards_debug = sns_rewards_wasm()?;
    let rewards_production =
        support::wasm::build_wasm_cached_for_test(&OnceLock::new(), "jupiter-sns-rewards", None)?;
    let lifeline_wasm =
        support::wasm::build_wasm_cached_for_test(&OnceLock::new(), "jupiter-lifeline", None)?;
    let repo = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let artifacts = if let Some(directory) = std::env::var_os("JUPITER_SNS_ARTIFACT_DIR") {
        PathBuf::from(directory)
    } else {
        let provision = Command::new("python3")
            .args(["tools/acceptance/artifacts.py", "--fetch"])
            .current_dir(&repo)
            .output()?;
        anyhow::ensure!(
            provision.status.success(),
            "official SNS prerequisite failed: {}",
            String::from_utf8_lossy(&provision.stderr)
        );
        PathBuf::from(String::from_utf8(provision.stdout)?.trim())
    };
    checked_command(
        Command::new("python3")
            .args([
                "-m",
                "unittest",
                "discover",
                "-s",
                "tools/acceptance",
                "-p",
                "test_*.py",
            ])
            .env("JUPITER_SNS_ARTIFACT_DIR", &artifacts)
            .current_dir(&repo),
    )?;
    let evidence = repo
        .join("target")
        .join(format!("jupiter-acceptance-sns-{}", std::process::id()));
    fs::create_dir(&evidence)?;
    let pic = support::pocketic::builder()
        .with_nns_subnet()
        .with_sns_subnet()
        .with_application_subnet()
        .with_icp_features(pocket_ic::common::rest::IcpFeatures {
            registry: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            cycles_minting: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            icp_token: Some(pocket_ic::common::rest::IcpFeaturesConfig::DefaultConfig),
            ..Default::default()
        })
        .build();
    let operator = Principal::self_authenticating([41; 32]);
    let outsider = Principal::self_authenticating([42; 32]);
    let mut roles = serde_json::Map::new();
    for role in [
        "sns_governance",
        "sns_root",
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
        let subnet = if role.starts_with("sns_") {
            pic.topology().get_sns().unwrap()
        } else {
            pic.topology().get_app_subnets()[0]
        };
        let id = pic.create_canister_on_subnet(Some(operator), None, subnet);
        pic.add_cycles(id, 3_000_000_000_000);
        roles.insert(role.to_string(), json!(id.to_text()));
    }
    let role = |name: &str| Principal::from_text(roles[name].as_str().unwrap()).unwrap();
    let acceptance_profile: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join("deploy/acceptance/profile.json"))?)?;
    let acceptance_neuron_id = acceptance_profile["nns_neuron"]["id"]
        .as_u64()
        .context("confirmed acceptance neuron ID")?;
    let context = evidence.join("context.json");
    fs::write(
        &context,
        serde_json::to_vec_pretty(&json!({"roles": roles,
        "operator": operator.to_text(), "other": outsider.to_text(), "acceptance_neuron_id": acceptance_neuron_id,
        "timestamp": pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000}))?,
    )?;
    checked_command(
        Command::new("python3")
            .arg("-c")
            .arg(
                r#"
import json, sys
from pathlib import Path
sys.path.insert(0, 'tools/acceptance')
import profile, prepare
from test_sns import local_genesis
c=json.loads(Path(sys.argv[1]).read_text()); out=Path(sys.argv[2])
p=json.loads(profile.PROFILE.read_text()); p['roles']=c['roles']; p['operator']=c['operator']
p['nns_neuron']['id']=c['acceptance_neuron_id']
p['age_bonus_recipient_2']={'owner':c['operator'],'subaccountHex':None}
p['relay_surplus_neuron_recipients']=[]
p['genesis']=local_genesis(c['operator'], c['other'])
(out/'profile.json').write_text(json.dumps(p, indent=2)+'\n')
prepare.applications(p, out/'applications')
prepare.framework(p, sys.argv[3], out/'sns', c['timestamp'], local_fixture=True)
"#,
            )
            .arg(&context)
            .arg(&evidence)
            .arg(&artifacts)
            .current_dir(&repo),
    )?;
    let profile = evidence.join("profile.json");
    let built = evidence.join("artifacts");
    for application in [
        "jupiter_relay",
        "jupiter_historian",
        "jupiter_disburser",
        "jupiter_faucet",
    ] {
        checked_command(
            Command::new("python3")
                .arg("tools/acceptance/build.py")
                .arg(&profile)
                .arg(&built)
                .arg(application)
                .arg("--debug-api")
                .current_dir(&repo),
        )?;
    }
    checked_command(
        Command::new("python3")
            .arg("tools/acceptance/build.py")
            .arg(&profile)
            .arg(&built)
            .arg("jupiter_faucet_frontend")
            .current_dir(&repo),
    )?;
    let sns_dir = evidence.join("sns");
    for component in ["governance", "ledger", "index", "swap", "root"] {
        let executable = fs::read(artifacts.join(format!("sns_{component}.wasm")))?;
        let expected_hash = sha2::Sha256::digest(&executable).to_vec();
        let id = role(&format!("sns_{component}"));
        pic.install_canister(
            id,
            executable,
            fs::read(sns_dir.join(format!("{component}.bin")))?,
            Some(operator),
        );
        assert_eq!(
            pic.canister_status(id, Some(operator))
                .expect("official module status")
                .module_hash,
            Some(expected_hash)
        );
    }
    let nns = support::principals::nns_governance();
    pic.create_canister_with_id(None, None, nns)
        .map_err(|e| anyhow::anyhow!("NNS mock allocation: {e:?}"))?;
    pic.add_cycles(nns, 5_000_000_000_000);
    pic.install_canister(nns, nns_mock, vec![], None);
    let accounts: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence.join("applications/accounts.json"))?)?;
    let staking_subaccount = hex::decode(
        accounts["stakingAccount"]["subaccountHex"]
            .as_str()
            .unwrap(),
    )?;
    update_bytes::<()>(
        &pic,
        nns,
        operator,
        "debug_set_neuron_account",
        encode_args((acceptance_neuron_id, staking_subaccount.clone()))?,
    )?;
    let relay = role("jupiter_relay");
    let rewards = role("jupiter_sns_rewards");
    let root = role("sns_root");
    let governance = role("sns_governance");
    let ledger = role("sns_ledger");
    let index = role("sns_index");
    // Use the acceptance-bound Relay executable, including its actual neuron
    // lookup path. NNS/ICP infrastructure is an owned local replica of the IC.
    pic.install_canister(
        relay,
        fs::read(built.join("jupiter_relay.debug.wasm"))?,
        fs::read(evidence.join("applications/jupiter_relay.bin"))?,
        Some(operator),
    );
    pic.install_canister(
        rewards,
        rewards_debug.clone(),
        fs::read(evidence.join("applications/jupiter_sns_rewards.bin"))?,
        Some(operator),
    );
    let historian = role("jupiter_historian");
    pic.add_cycles(historian, 40_000_000_000_000);
    pic.install_canister(
        historian,
        fs::read(built.join("jupiter_historian.debug.wasm"))?,
        fs::read(evidence.join("applications/jupiter_historian.bin"))?,
        Some(operator),
    );
    for application in ["jupiter_disburser", "jupiter_faucet"] {
        pic.install_canister(
            role(application),
            fs::read(built.join(format!("{application}.debug.wasm")))?,
            fs::read(evidence.join(format!("applications/{application}.bin")))?,
            Some(operator),
        );
        assert_eq!(
            pic.canister_status(role(application), Some(operator))
                .expect("unarmed bootstrap")
                .settings
                .controllers,
            vec![operator]
        );
    }
    let disburser = role("jupiter_disburser");
    let faucet = role("jupiter_faucet");
    let lifeline = role("jupiter_lifeline");
    pic.install_canister(lifeline, lifeline_wasm, encode_args(())?, Some(operator));
    let frontend = role("jupiter_faucet_frontend");
    pic.install_canister(
        frontend,
        fs::read(built.join("jupiter_faucet_frontend.wasm"))?,
        encode_args(())?,
        Some(operator),
    );
    let page: HttpResponse = query_one(
        &pic,
        frontend,
        operator,
        "http_request",
        HttpRequest {
            url: "/".into(),
            method: "GET".into(),
            body: vec![],
            headers: vec![],
            certificate_version: Some(2),
        },
    )?;
    assert_eq!(page.status_code, 200);
    let html = String::from_utf8(page.body)?;
    assert!(html.contains(&format!("/neuron/{acceptance_neuron_id}")));
    assert!(!html.contains("11614578985374291210"));
    assert!(!html.contains("22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d"));
    let domains: HttpResponse = query_one(
        &pic,
        frontend,
        operator,
        "http_request",
        HttpRequest {
            url: "/.well-known/ic-domains".into(),
            method: "GET".into(),
            body: vec![],
            headers: vec![],
            certificate_version: Some(2),
        },
    )?;
    assert_eq!(domains.status_code, 200);
    assert!(domains.body.is_empty());
    let dc: DisburserConfig = query_one(&pic, disburser, operator, "debug_config", ())?;
    assert_eq!(dc.neuron_id, acceptance_neuron_id);
    assert_eq!(dc.normal_recipient.owner, faucet);
    assert_eq!(dc.age_bonus_recipient_1.owner, rewards);
    assert_eq!(dc.age_bonus_recipient_2.owner, operator);
    assert_eq!(dc.rescue_controller, lifeline);
    assert_eq!(dc.autonomous_rescue_armed, Some(false));
    let fc: FaucetConfig = query_one(&pic, faucet, operator, "debug_config", ())?;
    assert_eq!(fc.funding_source_account.owner, disburser);
    assert_eq!(fc.rescue_controller, lifeline);
    assert_eq!(fc.autonomous_rescue_armed, Some(false));
    let profile_value: serde_json::Value = serde_json::from_slice(&fs::read(&profile)?)?;
    assert_ne!(
        fc.staking_account.subaccount,
        Some(
            hex::decode("ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68")?
                .try_into()
                .unwrap()
        )
    );
    assert_eq!(fc.staking_account.owner, nns);
    assert_eq!(
        fc.staking_account.subaccount.unwrap().as_slice(),
        staking_subaccount
    );
    for (id, controller) in [
        (governance, root),
        (root, governance),
        (ledger, root),
        (index, root),
        (role("sns_swap"), root),
        (rewards, root),
        (historian, root),
        (relay, root),
        (frontend, root),
        (lifeline, root),
    ] {
        let before = pic
            .canister_status(id, Some(operator))
            .expect("bootstrap status");
        assert_eq!(before.settings.controllers, vec![operator]);
        pic.set_controllers(id, Some(operator), vec![controller])
            .unwrap();
        assert_eq!(
            pic.canister_status(id, Some(controller))
                .expect("controller status")
                .settings
                .controllers,
            vec![controller]
        );
        assert!(pic
            .set_controllers(id, Some(outsider), vec![outsider])
            .is_err());
    }
    let mode: GovernanceMode = query_one(&pic, governance, operator, "get_mode", Empty {})?;
    assert_eq!(mode.mode, Some(1)); // Pinned official MODE_NORMAL.
    let symbol: String = query_one(&pic, ledger, operator, "icrc1_symbol", ())?;
    assert_eq!(symbol, "jUP");
    let name: String = query_one(&pic, ledger, operator, "icrc1_name", ())?;
    assert_eq!(name, "Jupiter local acceptance fixture");
    let decimals: u8 = query_one(&pic, ledger, operator, "icrc1_decimals", ())?;
    assert_eq!(decimals, 8);
    let supply: Nat = query_one(&pic, ledger, operator, "icrc1_total_supply", ())?;
    assert_eq!(supply, Nat::from(3_100_000_000u64));
    let membership: jupiter_ic_clients::sns::ListSnsCanistersResponse =
        query_one(&pic, root, operator, "list_sns_canisters", Empty {})?;
    assert_eq!(membership.root, Some(root));
    assert_eq!(membership.governance, Some(governance));
    assert_eq!(membership.ledger, Some(ledger));
    assert_eq!(membership.index, Some(index));
    assert!(membership.dapps.contains(&rewards) && membership.dapps.contains(&historian));
    let _: () = update_noargs(&pic, historian, operator, "debug_driver_tick")?;
    let tracked: Canisters = query_one(
        &pic,
        historian,
        operator,
        "list_canisters",
        ListCanisters {
            start_after: None,
            limit: Some(100),
            tracking_reason_filter: Some(TrackingReason::SnsDiscovery),
        },
    )?;
    for component in [root, governance, ledger, index, role("sns_swap")] {
        assert!(tracked
            .items
            .iter()
            .any(|item| item.canister_id == component));
    }
    let fee: Nat = query_one(&pic, ledger, operator, "icrc1_fee", ())?;
    assert_eq!(fee, Nat::from(1_000u64));
    assert_eq!(
        support::ledger::icrc1_balance(
            &pic,
            ledger,
            &Account {
                owner: operator,
                subaccount: None
            }
        )?,
        100_000_000
    );
    let mut staking = sha2::Sha256::new();
    staking.update(b"\x0cneuron-stake");
    staking.update(operator.as_slice());
    staking.update(1u64.to_be_bytes());
    let neuron_sub = staking.finalize().to_vec();
    assert_eq!(
        support::ledger::icrc1_balance(
            &pic,
            ledger,
            &Account {
                owner: governance,
                subaccount: Some(neuron_sub.clone().try_into().unwrap())
            }
        )?,
        1_000_000_000
    );

    // Qualifying endowment and real ICP Index evidence predate both the owner
    // snapshot and the first reward-token credit.
    let icp = support::principals::icp_ledger();
    let icp_index = support::principals::icp_index();
    real_icp_transfer(
        &pic,
        icp,
        Principal::anonymous(),
        Account {
            owner: operator,
            subaccount: None,
        },
        500_000_000,
    )?;
    real_icp_transfer(
        &pic,
        icp,
        operator,
        Account {
            owner: relay,
            subaccount: Some(relay_subaccount_one()),
        },
        200_000_000,
    )?;
    let _: () = update_noargs(&pic, relay, operator, "debug_main_tick")?;
    wait_for_real_index_transactions(
        &pic,
        icp_index,
        &account_identifier_text(relay, Some(relay_subaccount_one())),
        2,
    )?;
    let observed_neurons: Vec<u64> = query_one(
        &pic,
        nns,
        operator,
        "debug_get_claim_or_refresh_neuron_ids",
        (),
    )?;
    assert!(observed_neurons.contains(&acceptance_neuron_id));
    assert!(!observed_neurons.contains(&11_614_578_985_374_291_210));
    assert!(support::ledger::icrc1_balance(&pic, icp, &fc.staking_account)? > 0);
    pic.advance_time(Duration::from_secs(2));
    tick_n(&pic, 5);
    let _: () = update_noargs(&pic, rewards, operator, "debug_scan_tick")?;
    tick_n(&pic, 10);
    let context: Option<RelayRewardContext> =
        query_one(&pic, rewards, operator, "get_relay_reward_context", ())?;
    let context = context.context("real Governance owner snapshot must publish")?;
    assert_eq!(context.sns_root_canister_id, root);
    assert_eq!(context.sns_governance_canister_id, governance);
    assert_eq!(context.sns_ledger_canister_id, ledger);
    let owners: ResolveDefaultIcpAccountsResult = query_one(
        &pic,
        rewards,
        operator,
        "resolve_default_icp_accounts",
        ResolveDefaultIcpAccountsArgs {
            snapshot_id: context.snapshot_id,
            account_identifiers: vec![
                hex::decode(account_identifier_text(operator, None))?,
                hex::decode(account_identifier_text(outsider, None))?,
            ],
        },
    )?;
    assert_eq!(
        owners,
        ResolveDefaultIcpAccountsResult::Ok(vec![Some(operator), None])
    );
    let transfer = TransferArg {
        from_subaccount: None,
        to: Account {
            owner: relay,
            subaccount: None,
        },
        fee: Some(fee),
        created_at_time: Some(pic.get_time().as_nanos_since_unix_epoch()),
        memo: None,
        amount: Nat::from(1_000_000u64),
    };
    let result: Result<Nat, icrc_ledger_types::icrc1::transfer::TransferError> =
        update_one(&pic, ledger, operator, "icrc1_transfer", transfer.clone())?;
    assert!(result.is_ok());
    let duplicate: Result<Nat, icrc_ledger_types::icrc1::transfer::TransferError> =
        update_one(&pic, ledger, operator, "icrc1_transfer", transfer)?;
    assert!(matches!(
        duplicate,
        Err(icrc_ledger_types::icrc1::transfer::TransferError::Duplicate { .. })
    ));
    for _ in 0..20 {
        pic.advance_time(Duration::from_secs(5));
        tick_n(&pic, 5);
    }
    let _: () = update_noargs(&pic, relay, operator, "debug_reward_sweep")?;
    let balance = support::ledger::icrc1_balance(
        &pic,
        ledger,
        &Account {
            owner: operator,
            subaccount: None,
        },
    )?;
    assert_eq!(balance, 99_998_000); // pot sent + transfer fee, then 999000 payout
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
    let _: () = update_noargs(&pic, relay, operator, "debug_reward_sweep")?;
    assert_eq!(
        support::ledger::icrc1_balance(
            &pic,
            ledger,
            &Account {
                owner: operator,
                subaccount: None
            }
        )?,
        balance
    );

    // The real factory preflight must authenticate this direct-installed SNS
    // Root: this SNS has no SNS-W registration and the target is Root-controlled.
    let setup = || SetupArgs {
        target_canister_ids: vec![rewards],
        surplus_recipients: vec![],
    };
    let view: SetupResult = update_one(
        &pic,
        historian,
        operator,
        "get_relay_configuration_view",
        setup(),
    )?;
    let SetupResult::Ok(view) = view else {
        bail!("factory view: {view:?}")
    };
    real_icp_transfer(
        &pic,
        icp,
        Principal::anonymous(),
        view.setup_account.context("authenticated setup account")?,
        view.nominal_minimum_e8s,
    )?;
    let created: NotifyResult = update_one(
        &pic,
        historian,
        operator,
        "notify_relay_configuration",
        setup(),
    )?;
    let NotifyResult::Active {
        relay_canister_id: child,
    } = created
    else {
        bail!("factory: {created:?}")
    };
    assert!(pic
        .canister_status(child, Some(historian))
        .expect("child public status")
        .settings
        .controllers
        .is_empty());
    // Keep the denial on install_code: PocketIC's large-Wasm chunk helper
    // unwraps denied clear_chunk_store calls before returning an upgrade error.
    let denied = pic
        .upgrade_canister(
            child,
            fs::read(built.join("jupiter_relay.debug.wasm.gz"))?,
            encode_args(())?,
            Some(operator),
        )
        .expect_err("a controllerless child must reject an operator upgrade");
    assert_eq!(
        denied.error_code,
        pocket_ic::ErrorCode::CanisterInvalidController
    );
    let before_child: Vec<u64> = query_one(
        &pic,
        nns,
        operator,
        "debug_get_claim_or_refresh_neuron_ids",
        (),
    )?;
    pic.advance_time(Duration::from_secs(2));
    tick_n(&pic, 20);
    let after_child: Vec<u64> = query_one(
        &pic,
        nns,
        operator,
        "debug_get_claim_or_refresh_neuron_ids",
        (),
    )?;
    assert!(
        after_child.len() > before_child.len(),
        "child seed must reach the acceptance neuron"
    );
    assert!(after_child.iter().all(|id| *id == acceptance_neuron_id));
    for subaccount in [
        neuron_subaccount(11_614_578_985_374_291_210),
        hex::decode("ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68")?
            .try_into()
            .unwrap(),
    ] {
        assert_eq!(
            support::ledger::icrc1_balance(
                &pic,
                icp,
                &Account {
                    owner: nns,
                    subaccount: Some(subaccount)
                }
            )?,
            0
        );
    }
    let logs = pic
        .fetch_canister_logs(child, Principal::anonymous())
        .map_err(|e| anyhow::anyhow!("child logs: {e:?}"))?;
    let logs = logs
        .iter()
        .map(|entry| String::from_utf8_lossy(&entry.content))
        .collect::<String>();
    assert!(
        logs.contains(&format!("sns_rewards_canister_id={rewards}")),
        "{logs}"
    );
    assert!(!logs.contains("alk7f-5aaaa-aaaar-qb4ra-cai"));
    assert!(!logs.contains("11614578985374291210"));

    // A native SNS-controlled application upgrade; this does not change
    // autonomous Faucet/Disburser control or make child Relays upgradeable.
    let did = sns_dir.join("governance.interface.did");
    let mint = format!("variant {{ MintSnsTokens = record {{ amount_e8s = opt 100000; to_principal = opt principal \"{operator}\" }} }}");
    let mint_result = manage(&pic, governance, operator, &neuron_sub, &mint, &did)?;
    let command = opted(field(&mint_result, "command")?)?;
    assert!(
        matches!(command, IDLValue::Variant(value) if value.0.id.get_id() == candid::idl_hash("Error"))
    );
    let error = format!("{mint_result}");
    assert!(
        error.contains("sns_tokens_per_icp") || error.contains("valuation"),
        "{error}"
    );
    let supply_after: Nat = query_one(&pic, ledger, operator, "icrc1_total_supply", ())?;
    assert_eq!(supply_after, Nat::from(3_099_998_000u64));
    // The debug build intentionally forces a fresh owner scan after upgrade.
    // Use production scheduling to test preservation of the current snapshot.
    let mut candidate = rewards_production.clone();
    let name = b"icp:public acceptance-upgrade";
    let content = b"local SNS-governed upgrade";
    candidate.extend([0, (1 + name.len() + content.len()) as u8, name.len() as u8]);
    candidate.extend(name);
    candidate.extend(content);
    // SNS upgrades accept the same gzip install packages as management calls.
    // Keep the proposal below ingress limits without changing its Wasm content.
    let candidate_path = evidence.join("sns-rewards-upgrade.wasm");
    fs::write(&candidate_path, &candidate)?;
    let compressed = Command::new("gzip")
        .args(["-n", "-c"])
        .arg(&candidate_path)
        .output()?;
    anyhow::ensure!(compressed.status.success(), "compress governed upgrade");
    let candidate = compressed.stdout;
    assert!(
        candidate.len() < 1_900_000,
        "upgrade proposal needs chunking"
    );
    let candidate_hash = sha2::Sha256::digest(&candidate).to_vec();
    let action = format!("variant {{ UpgradeSnsControlledCanister = record {{ canister_id = opt principal \"{rewards}\"; new_canister_wasm = blob \"{}\"; mode = opt (3 : int32); canister_upgrade_arg = opt blob \"\\44\\49\\44\\4c\\00\\00\" }} }}",
        candidate.iter().map(|b| format!("\\{b:02x}")).collect::<String>());
    let denied = manage(&pic, governance, outsider, &neuron_sub, &action, &did)?;
    let command = opted(field(&denied, "command")?)?;
    assert!(
        matches!(command, IDLValue::Variant(value) if value.0.id.get_id() == candid::idl_hash("Error"))
    );
    let old_hash = pic
        .canister_status(rewards, Some(root))
        .expect("SNS Rewards status")
        .module_hash;
    let adopted = manage(&pic, governance, operator, &neuron_sub, &action, &did)?;
    let command = opted(field(&adopted, "command")?)?;
    assert!(
        matches!(command, IDLValue::Variant(value) if value.0.id.get_id() == candid::idl_hash("MakeProposal")),
        "{adopted}"
    );
    let IDLValue::Variant(make_proposal) = command else {
        unreachable!()
    };
    let IDLValue::Nat64(proposal_id) =
        field(opted(field(&make_proposal.0.val, "proposal_id")?)?, "id")?
    else {
        bail!("proposal ID missing")
    };
    let proposal_id = *proposal_id;
    let before_vote: ProposalReply = query_one(
        &pic,
        governance,
        operator,
        "get_proposal",
        GetProposal {
            proposal_id: Some(ProposalId { id: proposal_id }),
        },
    )?;
    let Some(ProposalResult::Proposal(before_vote)) = before_vote.result else {
        bail!("proposal not visible")
    };
    assert_eq!(before_vote.executed_timestamp_seconds, 0);
    let mut voter = sha2::Sha256::new();
    voter.update(b"\x0cneuron-stake");
    voter.update(outsider.as_slice());
    voter.update(2u64.to_be_bytes());
    let vote = manage_command(&pic, governance, outsider, &voter.finalize(),
        &format!("variant {{ RegisterVote = record {{ proposal = opt record {{ id = {proposal_id} }}; vote = 1 : int32 }} }}"), &did)?;
    assert!(
        matches!(opted(field(&vote, "command")?)?, IDLValue::Variant(value) if value.0.id.get_id() == candid::idl_hash("RegisterVote")),
        "{vote}"
    );
    for _ in 0..30 {
        pic.advance_time(Duration::from_secs(2));
        tick_n(&pic, 5);
    }
    let executed: ProposalReply = query_one(
        &pic,
        governance,
        operator,
        "get_proposal",
        GetProposal {
            proposal_id: Some(ProposalId { id: proposal_id }),
        },
    )?;
    let Some(ProposalResult::Proposal(executed)) = executed.result else {
        bail!("proposal outcome missing")
    };
    assert!(executed.decided_timestamp_seconds > 0 && executed.executed_timestamp_seconds > 0);
    assert_eq!(executed.failed_timestamp_seconds, 0);
    let after = pic
        .canister_status(rewards, Some(root))
        .expect("SNS Rewards status");
    assert_ne!(after.module_hash, old_hash);
    assert_eq!(after.module_hash, Some(candidate_hash));
    assert_eq!(after.settings.controllers, vec![root]);
    let restored: Option<RelayRewardContext> =
        query_one(&pic, rewards, operator, "get_relay_reward_context", ())?;
    let restored = restored.context("snapshot survives governed upgrade")?;
    assert_eq!(restored.snapshot_id, context.snapshot_id);
    assert_eq!(restored.sns_root_canister_id, root);
    assert_eq!(restored.sns_governance_canister_id, governance);
    assert_eq!(restored.sns_ledger_canister_id, ledger);
    let restored_owners: ResolveDefaultIcpAccountsResult = query_one(
        &pic,
        rewards,
        operator,
        "resolve_default_icp_accounts",
        ResolveDefaultIcpAccountsArgs {
            snapshot_id: restored.snapshot_id,
            account_identifiers: vec![
                hex::decode(account_identifier_text(operator, None))?,
                hex::decode(account_identifier_text(outsider, None))?,
            ],
        },
    )?;
    assert_eq!(restored_owners, owners);

    // Real Disburser -> Faucet funding, then a zero-commitment remainder. This
    // exercises Faucet's compiled Relay destination, beyond manifest checking.
    real_icp_transfer(
        &pic,
        icp,
        Principal::anonymous(),
        Account {
            owner: disburser,
            subaccount: None,
        },
        200_000_000,
    )?;
    let _: () = update_noargs(&pic, disburser, operator, "debug_main_tick")?;
    wait_for_real_index_transactions(&pic, icp_index, &account_identifier_text(faucet, None), 1)?;
    let before_remainder = support::ledger::icrc1_balance(
        &pic,
        icp,
        &Account {
            owner: relay,
            subaccount: None,
        },
    )?;
    let _: () = update_noargs(&pic, faucet, operator, "debug_main_tick")?;
    let after_remainder = support::ledger::icrc1_balance(
        &pic,
        icp,
        &Account {
            owner: relay,
            subaccount: None,
        },
    )?;
    assert!(
        after_remainder > before_remainder,
        "remainder before={before_remainder}, after={after_remainder}, Faucet logs={:?}",
        pic.fetch_canister_logs(faucet, Principal::anonymous())
    );
    assert_eq!(
        support::ledger::icrc1_balance(
            &pic,
            icp,
            &Account {
                owner: principal("u2qkp-aqaaa-aaaar-qb7ea-cai"),
                subaccount: None
            }
        )?,
        0
    );

    // Rehearse the existing local-state rescue policy only after bootstrap and
    // the ordinary upgrade path. Timestamp hooks are existing debug-only fixture
    // controls, not a change to rescue timing or an implemented Lifeline repair.
    for (name, id) in [("jupiter_disburser", disburser), ("jupiter_faucet", faucet)] {
        // PocketIC's chunk-upload helper unwraps a denied clear_chunk_store.
        // The normal compressed package keeps unauthorized upgrades on the
        // install_code path, where rejection is returned for our assertion.
        let executable = fs::read(built.join(format!("{name}.debug.wasm.gz")))?;
        pic.set_controllers(id, Some(operator), vec![operator, id])
            .unwrap();
        let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000;
        let _: () = update_one(
            &pic,
            id,
            operator,
            "debug_set_last_successful_transfer_ts",
            Some(now + 1),
        )?;
        pic.upgrade_canister(
            id,
            executable.clone(),
            encode_one(Some(Arm {
                autonomous_rescue_armed: Some(true),
            }))?,
            Some(operator),
        )
        .map_err(|e| anyhow::anyhow!("bootstrap arming upgrade: {e:?}"))?;
        tick_n(&pic, 10);
        let _: () = update_noargs(&pic, id, operator, "debug_rescue_tick")?;
        assert_eq!(
            pic.canister_status(id, Some(id))
                .expect("healthy self control")
                .settings
                .controllers,
            vec![id]
        );
        assert!(pic
            .upgrade_canister(id, executable.clone(), encode_args(())?, Some(operator))
            .is_err());
        let _: () = update_one(
            &pic,
            id,
            operator,
            "debug_set_last_successful_transfer_ts",
            Some(0u64),
        )?;
        let _: () = update_noargs(&pic, id, operator, "debug_rescue_tick")?;
        let controllers = pic
            .canister_status(id, Some(lifeline))
            .expect("rescue control")
            .settings
            .controllers;
        assert_eq!(controllers.len(), 2);
        assert!(controllers.contains(&lifeline) && controllers.contains(&id));
        let _: () = update_one(
            &pic,
            id,
            operator,
            "debug_set_last_successful_transfer_ts",
            Some(now + 1),
        )?;
        let _: () = update_noargs(&pic, id, operator, "debug_rescue_tick")?;
        assert_eq!(
            pic.canister_status(id, Some(id))
                .expect("healthy return")
                .settings
                .controllers,
            vec![id]
        );
    }
    let pot_blocks = reward_pots::run(reward_pots::Fixture {
        pic: &pic,
        repo: &repo,
        profile: &profile,
        evidence: &evidence,
        rewards,
        root,
        relay,
        child,
        operator,
        ledger,
        index,
        staking: fc.staking_account,
        production: rewards_production,
    })?;
    fs::write(
        evidence.join("outcome.json"),
        serde_json::to_vec_pretty(&json!({
            "framework_root": root.to_text(), "governance": governance.to_text(),
            "ledger": ledger.to_text(), "child": child.to_text(), "snapshot_id": context.snapshot_id,
            "reserve_pot_blocks": pot_blocks, "reward_fee": 1000, "owner_balance": balance, "governed_upgrade_module_hash": after.module_hash.map(hex::encode),
            "local_fixture": true, "mainnet_genesis_approved": false,
            "profile": profile_value,
        }))?,
    )?;
    eprintln!("acceptance SNS evidence: {}", evidence.display());
    Ok(())
}
