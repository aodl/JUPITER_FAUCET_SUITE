//! Acceptance-only mechanism proof over the populated official SNS fixture.
//! Principals below are local callers, never loaded signing identities.
use super::*;

#[derive(CandidType)]
struct StatusArg {
    canister_id: Principal,
}
#[derive(CandidType, Deserialize)]
enum Visibility {
    #[serde(rename = "allowed_viewers")]
    AllowedViewers(Vec<Principal>),
    #[serde(rename = "controllers")]
    Controllers,
}
#[derive(CandidType)]
struct ViewerSettings {
    status_visibility: Option<Visibility>,
    log_visibility: Option<Visibility>,
    snapshot_visibility: Option<Visibility>,
}
#[derive(CandidType)]
struct SetViewers {
    canister_id: Principal,
    settings: ViewerSettings,
}
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq)]
struct Archive {
    canister_id: Principal,
    block_range_start: Nat,
    block_range_end: Nat,
}
#[derive(CandidType)]
struct Range {
    start: Nat,
    length: Nat,
}

fn encode(did: &Path, types: &str, text: &str) -> Result<Vec<u8>> {
    let mut child = Command::new("didc")
        .arg("encode")
        .arg("--defs")
        .arg(did)
        .args(["--types", types])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    let result = child.wait_with_output()?;
    anyhow::ensure!(result.status.success(), "typed maintenance args");
    Ok(hex::decode(String::from_utf8(result.stdout)?.trim())?)
}

fn decode_viewer(repo: &Path, method: &str, bytes: &[u8]) -> Result<serde_json::Value> {
    let mut child = Command::new("node")
        .arg(repo.join("tools/acceptance/observe.mjs"))
        .args(["--decode-management", method])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(hex::encode(bytes).as_bytes())?;
    let result = child.wait_with_output()?;
    anyhow::ensure!(
        result.status.success(),
        "real management reply SDK projection"
    );
    Ok(serde_json::from_slice(&result.stdout)?)
}

fn upgrade_ledger(
    pic: &PocketIc,
    ledger: Principal,
    operator: Principal,
    package: Vec<u8>,
    did: &Path,
    extras: bool,
    funded: bool,
) -> Result<()> {
    // Small archive thresholds/memory are LOCAL fixture acceleration only.
    let more = if extras {
        format!("; more_controller_ids = opt vec {{ principal \"{operator}\" }}")
    } else {
        String::new()
    };
    let funding = if funded {
        "; cycles_for_archive_creation = opt 4500000000000"
    } else {
        ""
    };
    let arg = format!("(variant {{ Upgrade = opt record {{ change_archive_options = opt record {{ trigger_threshold = opt 20; num_blocks_to_archive = opt 10; node_max_memory_size_bytes = opt 8192{more}{funding} }} }} }})");
    for _ in 0..20 {
        pic.tick();
    }
    pic.upgrade_canister(
        ledger,
        package,
        encode(did, "(LedgerArg)", &arg)?,
        Some(operator),
    )
    .map_err(|e| anyhow::anyhow!("official Ledger upgrade: {e:?}"))?;
    Ok(())
}

fn transfers(
    pic: &PocketIc,
    ledger: Principal,
    source: Principal,
    recipient: Principal,
    count: u64,
) -> Result<()> {
    for _ in 0..count {
        support::ledger::icrc1_transfer(
            pic,
            ledger,
            source,
            TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: recipient,
                    subaccount: None,
                },
                amount: Nat::from(100_000_000_u64),
                fee: Some(Nat::from(1_000_000_u64)),
                created_at_time: None,
                memo: None,
            },
        )?;
    }
    for _ in 0..30 {
        pic.tick();
    }
    Ok(())
}

pub(super) fn rehearse(
    pic: &PocketIc,
    repo: &Path,
    evidence: &Path,
    artifacts: &Path,
    roles: &serde_json::Map<String, serde_json::Value>,
    operator: Principal,
    voter: Principal,
    dominant: &[u8],
    did: &Path,
    application_package: &[u8],
) -> Result<()> {
    let role = |name: &str| Principal::from_text(roles[name].as_str().unwrap()).unwrap();
    let governance = role("sns_governance");
    let root = role("sns_root");
    let ledger = role("sns_ledger");
    let rewards = role("jupiter_sns_rewards");
    let viewer = Principal::self_authenticating([131; 32]);
    let outsider = Principal::self_authenticating([132; 32]);
    let candidate = Command::new("python3")
        .args([
            "tools/acceptance/artifacts.py",
            "--fetch",
            "--lock",
            "deploy/acceptance/maintenance-candidate.json",
        ])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(
        candidate.status.success(),
        "official mechanism tuple verification"
    );
    let candidate = PathBuf::from(String::from_utf8(candidate.stdout)?.trim());
    let candidate_did = evidence.join("maintenance-ledger.did");
    let extracted = Command::new("ic-wasm")
        .arg(candidate.join("sns_ledger.wasm"))
        .args(["metadata", "candid:service"])
        .output()?;
    anyhow::ensure!(extracted.status.success(), "official candidate interface");
    fs::write(&candidate_did, extracted.stdout)?;
    let before_version = pic
        .query_call(
            governance,
            operator,
            "get_running_sns_version",
            encode_args((Empty {},))?,
        )
        .map_err(|e| anyhow::anyhow!("baseline version: {e:?}"))?;
    let before_neuron = neuron_state(pic, governance, voter, dominant)?;
    let reserve = Account {
        owner: rewards,
        subaccount: None,
    };
    let before_balance = support::ledger::icrc1_balance(pic, ledger, &reserve)?;
    let receiver = Account {
        owner: outsider,
        subaccount: None,
    };
    let before_receiver = support::ledger::icrc1_balance(pic, ledger, &receiver)?;
    let before_supply: Nat = query_one(pic, ledger, operator, "icrc1_total_supply", ())?;
    let small = fs::read(artifacts.join("root.wasm.gz"))?;
    let mut controller_evidence = Vec::new();
    for name in [
        "sns_root",
        "sns_governance",
        "sns_ledger",
        "sns_index",
        "sns_swap",
    ] {
        let id = role(name);
        let native = if name == "sns_root" { governance } else { root };
        let status = pic.canister_status(id, Some(operator)).unwrap();
        let mut actual = status.settings.controllers.clone();
        actual.sort();
        let mut expected = vec![native, operator];
        expected.sort();
        assert_eq!(actual, expected);
        let bytes = encode_one(SetViewers {
            canister_id: id,
            settings: ViewerSettings {
                status_visibility: Some(Visibility::AllowedViewers(vec![viewer])),
                log_visibility: Some(Visibility::AllowedViewers(vec![viewer])),
                snapshot_visibility: Some(Visibility::Controllers),
            },
        })?;
        pic.update_call_with_effective_principal(
            Principal::management_canister(),
            pocket_ic::common::rest::RawEffectivePrincipal::CanisterId(id.as_slice().to_vec()),
            operator,
            "update_settings",
            bytes,
        )
        .map_err(|e| anyhow::anyhow!("viewer settings: {e:?}"))?;
        // Exercise the actual observer SDK projection against native replies.
        let status_reply = pic
            .update_call_with_effective_principal(
                Principal::management_canister(),
                pocket_ic::common::rest::RawEffectivePrincipal::CanisterId(id.as_slice().to_vec()),
                viewer,
                "canister_status",
                encode_one(StatusArg { canister_id: id })?,
            )
            .map_err(|e| anyhow::anyhow!("allowed viewer status: {e:?}"))?;
        let decoded = decode_viewer(repo, "canister_status", &status_reply)?;
        anyhow::ensure!(
            decoded["cycles"].as_str().unwrap().parse::<u128>()? > 0,
            "viewer obtains real main cycles"
        );
        anyhow::ensure!(
            decoded["reserved_cycles"].as_str().is_some(),
            "reserved cycles are a separate native field"
        );
        let logs_reply = pic
            .query_call_with_effective_principal(
                Principal::management_canister(),
                pocket_ic::common::rest::RawEffectivePrincipal::CanisterId(id.as_slice().to_vec()),
                viewer,
                "fetch_canister_logs",
                encode_one(StatusArg { canister_id: id })?,
            )
            .map_err(|e| anyhow::anyhow!("native viewer log reply: {e:?}"))?;
        let logs = decode_viewer(repo, "fetch_canister_logs", &logs_reply)?;
        anyhow::ensure!(
            logs["canister_log_records"].is_array(),
            "actual log projection"
        );
        fs::write(
            evidence.join(format!("viewer-{name}.json")),
            serde_json::to_vec(&json!({"status":decoded,"logs":logs,"local_only":true}))?,
        )?;
        // PocketIC ingress denial of outsider canister_status has a client
        // response-decoding limitation; don't credit that panic as a proof.
        // Actual allowed-viewer replies and controller-only mutations follow.
        assert!(pic.fetch_canister_logs(id, outsider).is_err());
        assert!(pic.take_canister_snapshot(id, Some(viewer), None).is_err());
        assert!(pic.set_controllers(id, Some(viewer), vec![viewer]).is_err());
        let denied = pic
            .upgrade_canister(id, small.clone(), encode_args(())?, Some(viewer))
            .unwrap_err();
        assert_eq!(
            denied.error_code,
            pocket_ic::ErrorCode::CanisterInvalidController
        );
        controller_evidence.push(json!({"role":name,"controllers":actual,"viewer":viewer.to_text(),"writes_and_snapshots_denied":true}));
    }
    #[derive(CandidType)]
    struct SnapshotArg {
        canister_id: Principal,
        snapshot_id: Vec<u8>,
    }
    let index = role("sns_index");
    pic.stop_canister(index, Some(operator))
        .map_err(|e| anyhow::anyhow!("local snapshot stop: {e:?}"))?;
    let snapshot = pic
        .take_canister_snapshot(index, Some(operator), None)
        .map_err(|e| anyhow::anyhow!("local private snapshot: {e:?}"))?;
    let snapshot_arg = encode_one(SnapshotArg {
        canister_id: index,
        snapshot_id: snapshot.id.clone(),
    })?;
    let route =
        pocket_ic::common::rest::RawEffectivePrincipal::CanisterId(index.as_slice().to_vec());
    pic.update_call_with_effective_principal(
        Principal::management_canister(),
        route.clone(),
        operator,
        "read_canister_snapshot_metadata",
        snapshot_arg.clone(),
    )
    .map_err(|e| anyhow::anyhow!("controller snapshot metadata read: {e:?}"))?;
    let denied = pic
        .update_call_with_effective_principal(
            Principal::management_canister(),
            route,
            viewer,
            "read_canister_snapshot_metadata",
            snapshot_arg,
        )
        .expect_err("status/log viewer must not read a real private snapshot");
    anyhow::ensure!(
        denied.reject_message.contains("controller")
            || denied.reject_message.contains("not allowed"),
        "private snapshot denial: {denied:?}"
    );
    pic.delete_canister_snapshot(index, Some(operator), snapshot.id)
        .map_err(|e| anyhow::anyhow!("local snapshot delete: {e:?}"))?;
    pic.start_canister(index, Some(operator))
        .map_err(|e| anyhow::anyhow!("local snapshot restart: {e:?}"))?;
    // Exact genesis InitArgs: 2000-block trigger, 4.5T allocation and Root +
    // maintainer from birth. No Ledger upgrade/migration before first archive.
    let empty: Vec<Archive> = query_one(pic, ledger, operator, "archives", ())?;
    assert!(
        empty.is_empty(),
        "unexpected pre-existing archive: fail closed"
    );
    transfers(pic, ledger, rewards, outsider, 2000)?;
    let blocked: Vec<Archive> = query_one(pic, ledger, operator, "archives", ())?;
    assert!(blocked.is_empty(), "3T is not archive-trigger headroom");
    let logs: HttpResponse = query_one(
        pic,
        ledger,
        viewer,
        "http_request",
        HttpRequest {
            url: "/logs".into(),
            method: "GET".into(),
            body: vec![],
            headers: vec![],
            certificate_version: None,
        },
    )?;
    assert_eq!(logs.status_code, 200);
    let text = String::from_utf8(logs.body)?;
    anyhow::ensure!(
        text.contains("cycles"),
        "native archive headroom rejection: {text}"
    );
    fs::write(evidence.join("archive-headroom-limitation.txt"), &text)?;
    // 17.5T pre-trigger target = 4.5T creation + 10T native retained floor +
    // the existing 3T operating margin. Add only the observed local deficit.
    let target = 17_500_000_000_000_u128;
    pic.add_cycles(ledger, target.saturating_sub(pic.cycle_balance(ledger)));
    transfers(pic, ledger, rewards, outsider, 1)?;
    anyhow::ensure!(
        pic.cycle_balance(ledger) >= 10_000_000_000_000,
        "native Ledger retained liquid headroom"
    );
    let old: Vec<Archive> = query_one(pic, ledger, operator, "archives", ())?;
    anyhow::ensure!(!old.is_empty(), "real baseline archive must exist");
    let first = old[0].canister_id;
    assert_eq!(
        pic.canister_status(first, Some(root))
            .unwrap()
            .settings
            .controllers,
        vec![root, operator],
        "the official Ledger grants maintenance at creation, not through a simulated Root delegation"
    );
    let archived_before = pic
        .query_call(
            first,
            operator,
            "get_transactions",
            encode_one(Range {
                start: Nat::from(0_u64),
                length: Nat::from(1_u64),
            })?,
        )
        .map_err(|e| anyhow::anyhow!("archived receipt: {e:?}"))?;
    // Compare complete canonical transaction values, not Candid wire tables:
    // the newer interface adds optional authorized-mint/burn fields.
    let archived_before: icrc_ledger_types::icrc3::transactions::TransactionRange =
        candid::decode_one(&archived_before)?;
    assert_eq!(archived_before.transactions.len(), 1);
    let genesis_mint = archived_before.transactions[0]
        .mint
        .as_ref()
        .context("original genesis mint")?;
    assert_eq!(genesis_mint.amount, Nat::from(10_000_000_u64 * 100_000_000));
    assert_eq!(
        genesis_mint.to,
        Account {
            owner: governance,
            subaccount: Some(dominant.try_into().unwrap())
        }
    );
    assert!(pic
        .set_controllers(first, Some(viewer), vec![viewer])
        .is_err());
    for _ in 0..20 {
        pic.tick();
    }
    let denied = pic
        .upgrade_canister(
            first,
            fs::read(candidate.join("archive.wasm.gz"))?,
            encode_args(())?,
            Some(viewer),
        )
        .unwrap_err();
    assert_eq!(
        denied.error_code,
        pocket_ic::ErrorCode::CanisterInvalidController
    );
    pic.upgrade_canister(
        first,
        fs::read(candidate.join("archive.wasm.gz"))?,
        encode_args(())?,
        Some(operator),
    )
    .map_err(|e| anyhow::anyhow!("existing archive upgrade: {e:?}"))?;
    let archived_after: icrc_ledger_types::icrc3::transactions::TransactionRange = query_one(
        pic,
        first,
        operator,
        "get_transactions",
        Range {
            start: Nat::from(0_u64),
            length: Nat::from(1_u64),
        },
    )?;
    assert_eq!(
        archived_after, archived_before,
        "all canonical transaction fields preserved across archive migration"
    );
    // Local accelerated small-memory archives need their own finite test budget.
    pic.add_cycles(ledger, 100_000_000_000_000);
    // Direct state-preserving upgrades, not a native blessed SNS-W path.
    for _ in 0..20 {
        pic.tick();
    }
    let root_package = fs::read(candidate.join("root.wasm.gz"))?;
    pic.upgrade_canister(root, root_package.clone(), encode_args(())?, Some(operator))
        .map_err(|e| anyhow::anyhow!("official Root candidate upgrade: {e:?}"))?;
    assert_eq!(
        pic.canister_status(root, Some(operator))
            .unwrap()
            .module_hash,
        Some(sha2::Sha256::digest(&root_package).to_vec())
    );
    upgrade_ledger(
        pic,
        ledger,
        operator,
        fs::read(candidate.join("ledger.wasm.gz"))?,
        &candidate_did,
        true,
        true,
    )?;
    transfers(pic, ledger, rewards, outsider, 160)?;
    let archives: Vec<Archive> = query_one(pic, ledger, operator, "archives", ())?;
    let new: Vec<_> = archives
        .iter()
        .filter(|a| !old.iter().any(|o| o.canister_id == a.canister_id))
        .collect();
    anyhow::ensure!(
        !new.is_empty(),
        "a real archive must be created after the Ledger upgrade"
    );
    for a in &new {
        let status = pic.canister_status(a.canister_id, Some(operator)).unwrap();
        let mut actual = status.settings.controllers;
        actual.sort();
        let mut expected = vec![root, operator];
        expected.sort();
        assert_eq!(actual, expected);
        assert!(pic
            .set_controllers(a.canister_id, Some(viewer), vec![viewer])
            .is_err());
    }
    for pair in archives.windows(2) {
        assert_eq!(
            pair[0].block_range_end.clone() + Nat::from(1_u64),
            pair[1].block_range_start
        );
    }
    assert_eq!(archives[0].block_range_start, old[0].block_range_start);
    assert!(archives[0].block_range_end >= old[0].block_range_end);
    assert_eq!(
        support::ledger::icrc1_balance(pic, ledger, &reserve)?,
        before_balance - 2161 * 101_000_000
    );
    assert_eq!(
        support::ledger::icrc1_balance(pic, ledger, &receiver)?,
        before_receiver + 2161 * 100_000_000
    );
    let after_supply: Nat = query_one(pic, ledger, operator, "icrc1_total_supply", ())?;
    assert_eq!(after_supply, before_supply - Nat::from(2161_000_000_u64));
    assert_eq!(
        pic.query_call(
            governance,
            operator,
            "get_running_sns_version",
            encode_args((Empty {},))?
        )
        .unwrap(),
        before_version,
        "physical manual upgrades leave Governance metadata unchanged; never fake convergence"
    );
    let after_neuron = neuron_state(pic, governance, voter, dominant)?;
    assert_eq!(after_neuron.id.unwrap().id, dominant);
    assert_eq!(
        after_neuron.cached_neuron_stake_e8s,
        before_neuron.cached_neuron_stake_e8s
    );
    assert_eq!(after_neuron.dissolve_state, before_neuron.dissolve_state);
    #[derive(CandidType)]
    struct SummaryRequest {
        update_canister_list: Option<bool>,
    }
    pic.update_call(
        root,
        governance,
        "get_sns_canisters_summary",
        encode_one(SummaryRequest {
            update_canister_list: Some(true),
        })?,
    )
    .map_err(|e| {
        anyhow::anyhow!("official Root summary/archive refresh with maintenance topology: {e:?}")
    })?;
    for name in [
        "sns_root",
        "sns_governance",
        "sns_ledger",
        "sns_index",
        "sns_swap",
    ] {
        let id = role(name);
        let native = if name == "sns_root" { governance } else { root };
        let mut actual = pic
            .canister_status(id, Some(operator))
            .unwrap()
            .settings
            .controllers;
        actual.sort();
        let mut expected = vec![native, operator];
        expected.sort();
        assert_eq!(
            actual, expected,
            "native Root must preserve the explicit framework exception"
        );
    }
    // Native Governance still executes an ordinary dapp upgrade through the
    // candidate Root. Same code, distinct complete gzip package; no source edits.
    let compressed_path = evidence.join("governed-app-current.wasm.gz");
    let raw_path = evidence.join("governed-app-current.wasm");
    fs::write(&compressed_path, application_package)?;
    let raw = Command::new("gzip")
        .args(["-d", "-c"])
        .arg(&compressed_path)
        .output()?;
    anyhow::ensure!(raw.status.success(), "unpack existing app package");
    fs::write(&raw_path, &raw.stdout)?;
    let app = Command::new("gzip")
        .args(["-n", "-1", "-c"])
        .arg(&raw_path)
        .output()?;
    anyhow::ensure!(app.status.success(), "complete app package");
    let action=format!("variant {{ UpgradeSnsControlledCanister = record {{ canister_id = opt principal \"{rewards}\"; new_canister_wasm = blob \"{}\"; mode = opt (3 : int32); canister_upgrade_arg = opt blob \"\\44\\49\\44\\4c\\00\\00\" }} }}",app.stdout.iter().map(|b|format!("\\{b:02x}")).collect::<String>());
    let proposal = proposal_id(&manage(pic, governance, voter, dominant, &action, did)?)?;
    wait_executed(pic, governance, voter, proposal)?;
    wait_module(pic, rewards, root, &sha2::Sha256::digest(&app.stdout))?;
    let state = IDLArgs::from_bytes(&query_running(
        pic,
        rewards,
        operator,
        "get_reserve_transport_status",
    )?)?
    .args
    .remove(0);
    anyhow::ensure!(
        matches!(state, IDLValue::Variant(v) if v.0.id.get_id()==candid::idl_hash("Verified")),
        "settled app journal survived candidate Root governed upgrade"
    );
    assert_eq!(
        support::ledger::icrc1_balance(pic, ledger, &reserve)?,
        before_balance - 2161 * 101_000_000
    );
    // Authentic official SNS-W with no registered candidate. Its query proves
    // these exact bytes are unavailable through the ordinary blessed path.
    #[derive(CandidType)]
    struct SnsWasmInit {
        allowed_principals: Vec<Principal>,
        access_controls_enabled: bool,
        sns_subnet_ids: Vec<Principal>,
    }
    #[derive(CandidType)]
    struct GetWasm {
        hash: Vec<u8>,
    }
    #[derive(CandidType, Deserialize)]
    struct WasmReply {
        wasm: Option<candid::Reserved>,
    }
    let prepared = Command::new("python3")
        .args(["tools/acceptance/launch.py", "--sns-wasm", "--fetch"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(prepared.status.success(), "verified official SNS-W");
    let snsw = support::principals::sns_wasm();
    pic.create_canister_with_id(Some(operator), None, snsw)
        .map_err(|e| anyhow::anyhow!("local SNS-W: {e:?}"))?;
    pic.add_cycles(snsw, 3_000_000_000_000);
    pic.install_canister(
        snsw,
        fs::read(String::from_utf8(prepared.stdout)?.trim())?,
        encode_one(SnsWasmInit {
            allowed_principals: vec![],
            access_controls_enabled: true,
            sns_subnet_ids: vec![],
        })?,
        Some(operator),
    );
    let missing: WasmReply = query_one(
        pic,
        snsw,
        viewer,
        "get_wasm",
        GetWasm {
            hash: sha2::Sha256::digest(&root_package).to_vec(),
        },
    )?;
    assert!(missing.wasm.is_none());
    // Official Governance cannot select arbitrary Wasm bytes in this native action.
    // With no registered next path in this direct-install fixture it must reject.
    let unavailable = manage(
        pic,
        governance,
        voter,
        dominant,
        "variant { UpgradeSnsToNextVersion = record {} }",
        did,
    )?;
    succeeded(&unavailable, "Error")?;
    fs::write(
        evidence.join("maintenance-mechanism.json"),
        serde_json::to_vec_pretty(&json!({
            "fixture":"two source-pinned official versions / not an NNS candidate", "framework":controller_evidence,
            "existing_archive":first.to_text(),"new_archives":new.iter().map(|a|a.canister_id.to_text()).collect::<Vec<_>>(),
            "canonical_fee_burn_e8s":2161_000_000_u64,"version_metadata_unchanged":true,
            "native_next_version_rejected":true,"recovery":"experiment epoch; no downgrade or safe later blessed convergence proven",
            "application_upgrade":"ordinary native path passed before handover AND through candidate Root afterward"
        }))?,
    )?;
    println!("official acceptance maintenance/archive/version-drift mechanism passed");
    Ok(())
}
