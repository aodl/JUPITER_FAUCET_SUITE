use std::sync::OnceLock;

use anyhow::Result;
use candid::{encode_args, encode_one, CandidType, Deserialize, Principal};

#[path = "support/mod.rs"]
mod support;

use support::calls::{query_one, update_noargs, update_one};

static ROOT_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static GOVERNANCE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static REWARDS_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn wasm(cache: &OnceLock<Vec<u8>>, package: &str, features: Option<&str>) -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(cache, package, features)
}

#[derive(CandidType, Deserialize, Clone)]
struct InitArgs {
    reward_sns_root_canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone)]
struct UpgradeArgs {
    reward_sns_root_canister_id: Option<Option<Principal>>,
}

#[derive(CandidType, Deserialize, Clone)]
struct NeuronId {
    id: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone)]
struct NeuronPermission {
    principal: Option<Principal>,
    permission_type: Vec<i32>,
}

#[derive(CandidType, Deserialize, Clone)]
struct Neuron {
    id: Option<NeuronId>,
    permissions: Vec<NeuronPermission>,
    cached_neuron_stake_e8s: u64,
    neuron_fees_e8s: u64,
}

#[derive(CandidType)]
struct ListSnsCanistersResponse {
    root: Option<Principal>,
    governance: Option<Principal>,
    ledger: Option<Principal>,
    swap: Option<Principal>,
    index: Option<Principal>,
    dapps: Vec<Principal>,
    archives: Vec<Principal>,
    extensions: Option<SnsExtensions>,
}

#[derive(CandidType)]
struct SnsExtensions {
    extension_canister_ids: Vec<Principal>,
}

#[derive(CandidType, Deserialize, Debug)]
struct RelayRewardContext {
    sns_root_canister_id: Principal,
    sns_governance_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    scan_started_at_timestamp_nanos: u64,
    scan_completed_at_timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize, Debug)]
struct DebugState {
    configured_root: Option<Principal>,
    scan_active: bool,
    scan_cursor: Option<Vec<u8>>,
    pages_processed: u32,
    neurons_processed: u64,
    active_snapshot: Option<OwnerSnapshot>,
    active_slot_size: u64,
    staging_slot_size: u64,
}

#[derive(CandidType, Deserialize, Debug)]
struct OwnerSnapshot {
    snapshot_id: u64,
    active_slot: OwnerIndexSlot,
    sns_root_canister_id: Principal,
    sns_governance_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    scan_started_at_timestamp_nanos: u64,
    scan_completed_at_timestamp_nanos: u64,
    neuron_count: u64,
    owner_count: u64,
}

#[derive(CandidType, Deserialize, Debug)]
enum OwnerIndexSlot {
    A,
    B,
}

fn neuron_id(value: u32) -> Vec<u8> {
    let mut id = vec![0; 28];
    id.extend_from_slice(&value.to_be_bytes());
    id
}

#[test]
#[ignore = "PocketIC integration"]
fn owner_scan_is_resumable_atomic_and_invalidated_on_root_change() -> Result<()> {
    support::assertions::require_ignored_flag()?;
    let pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
    let root = pic.create_canister();
    let root_two = pic.create_canister();
    let governance = pic.create_canister();
    let ledger = pic.create_canister();
    let rewards = pic.create_canister();
    for canister in [root, root_two, governance, ledger, rewards] {
        pic.add_cycles(canister, 5_000_000_000_000);
    }
    pic.install_canister(root, wasm(&ROOT_WASM, "mock-sns-root", None)?, vec![], None);
    pic.install_canister(
        root_two,
        wasm(&ROOT_WASM, "mock-sns-root", None)?,
        vec![],
        None,
    );
    pic.install_canister(
        governance,
        wasm(&GOVERNANCE_WASM, "mock-sns-governance", None)?,
        vec![],
        None,
    );
    pic.install_canister(
        ledger,
        wasm(&LEDGER_WASM, "mock-icrc-ledger", None)?,
        vec![],
        None,
    );
    let canisters = |root_id| ListSnsCanistersResponse {
        root: Some(root_id),
        governance: Some(governance),
        ledger: Some(ledger),
        swap: None,
        index: None,
        dapps: vec![],
        archives: vec![],
        extensions: None,
    };
    let _: () = update_one(
        &pic,
        root,
        Principal::anonymous(),
        "debug_set_canisters",
        canisters(root),
    )?;
    let _: () = update_one(
        &pic,
        root_two,
        Principal::anonymous(),
        "debug_set_canisters",
        canisters(root_two),
    )?;
    let owner = Principal::from_slice(&[7]);
    let neurons = (0..101)
        .map(|value| Neuron {
            id: Some(NeuronId {
                id: neuron_id(value),
            }),
            permissions: vec![NeuronPermission {
                principal: Some(owner),
                permission_type: vec![1, 2],
            }],
            cached_neuron_stake_e8s: 100,
            neuron_fees_e8s: 0,
        })
        .collect::<Vec<_>>();
    let _: () = update_one(
        &pic,
        governance,
        Principal::anonymous(),
        "debug_set_neurons",
        neurons,
    )?;
    let rewards_wasm = wasm(&REWARDS_WASM, "jupiter-sns-rewards", Some("debug_api"))?;
    pic.install_canister(
        rewards,
        rewards_wasm.clone(),
        encode_one(InitArgs {
            reward_sns_root_canister_id: Some(root),
        })?,
        None,
    );

    let context: Option<RelayRewardContext> = query_one(
        &pic,
        rewards,
        Principal::anonymous(),
        "get_relay_reward_context",
        (),
    )?;
    assert!(context.is_none(), "partial staging scan must not be public");
    let mut before: DebugState =
        query_one(&pic, rewards, Principal::anonymous(), "debug_state", ())?;
    if !before.scan_active {
        let _: () = update_noargs(&pic, rewards, Principal::anonymous(), "debug_scan_tick")?;
        before = query_one(&pic, rewards, Principal::anonymous(), "debug_state", ())?;
    }
    assert!(before.scan_active);
    assert_eq!(before.pages_processed, 1);
    assert_eq!(before.neurons_processed, 100);
    assert!(before.scan_cursor.is_some());

    pic.advance_time(std::time::Duration::from_secs(5 * 60));
    support::calls::tick_n(&pic, 5);
    pic.upgrade_canister(rewards, rewards_wasm.clone(), vec![], None)
        .map_err(anyhow::Error::msg)?;
    let restored: DebugState = query_one(&pic, rewards, Principal::anonymous(), "debug_state", ())?;
    assert!(
        !restored.scan_active,
        "startup resume should finish the second page"
    );
    let context: Option<RelayRewardContext> = query_one(
        &pic,
        rewards,
        Principal::anonymous(),
        "get_relay_reward_context",
        (),
    )?;
    let context = context.expect("complete scan context");
    assert_eq!(context.sns_root_canister_id, root);
    assert_eq!(context.sns_governance_canister_id, governance);
    assert_eq!(context.sns_ledger_canister_id, ledger);
    let completed: DebugState =
        query_one(&pic, rewards, Principal::anonymous(), "debug_state", ())?;
    assert!(!completed.scan_active);
    assert_eq!(completed.active_slot_size, 1);

    let upgrade = encode_args((Some(UpgradeArgs {
        reward_sns_root_canister_id: Some(Some(root_two)),
    }),))?;
    pic.upgrade_canister(rewards, rewards_wasm, upgrade, None)
        .map_err(anyhow::Error::msg)?;
    let context: Option<RelayRewardContext> = query_one(
        &pic,
        rewards,
        Principal::anonymous(),
        "get_relay_reward_context",
        (),
    )?;
    assert!(context.is_none());
    let invalidated: DebugState =
        query_one(&pic, rewards, Principal::anonymous(), "debug_state", ())?;
    assert_eq!(invalidated.configured_root, Some(root_two));
    assert_eq!(invalidated.active_slot_size, 0);
    assert!(invalidated.active_snapshot.is_none());
    assert!(
        invalidated.scan_active,
        "new Root may begin repopulating staging immediately"
    );
    Ok(())
}
