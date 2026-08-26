#![allow(non_snake_case)]

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use candid::{encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager},
    storable::Bound,
    Memory, StableCell, Storable, VectorMemory,
};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::TransferArg;
use jupiter_ic_clients::account_identifier::account_identifier_text;
use jupiter_ic_clients::icrc_index::{
    GetAccountTransactionsArgs as IcrcGetAccountTransactionsArgs,
    GetAccountTransactionsResult as IcrcGetAccountTransactionsResult,
};
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsArgs, GetAccountIdentifierTransactionsResult, IndexOperation,
};
use pocket_ic::PocketIc;

#[path = "support/mod.rs"]
mod support;

use support::account_identifier::principal_to_subaccount;
use support::calls::{query_one, tick_n, update_bytes, update_noargs, update_one};

fn require_ignored_flag() -> Result<()> {
    support::assertions::require_ignored_flag()
}

fn principal(text: &str) -> Principal {
    Principal::from_text(text).unwrap()
}

static LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CMC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static GOVERNANCE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static BLACKHOLE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_PROD_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_V1_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_V2_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_REWARDS_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_ROOT_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_GOVERNANCE_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn ledger_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&LEDGER_WASM, "mock-icrc-ledger", None)
}
fn cmc_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&CMC_WASM, "mock-cmc", None)
}
fn governance_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&GOVERNANCE_WASM, "mock-nns-governance", None)
}
fn blackhole_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&BLACKHOLE_WASM, "mock-blackhole", None)
}
fn relay_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&RELAY_WASM, "jupiter-relay", Some("debug_api"))
}
fn relay_prod_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&RELAY_PROD_WASM, "jupiter-relay", None)
}

const RELAY_V1_REVISION: &str = "4b2bf3aa0e45df5da11bd089e73d527dce661794";
const RELAY_V2_REVISION: &str = "1aa518f5ee3ca25dcb86de1866761d47ea490f27";

fn historical_relay_wasm(
    revision: &str,
    label: &str,
    cache: &OnceLock<Vec<u8>>,
    override_var: &str,
) -> Result<Vec<u8>> {
    if let Some(bytes) = cache.get() {
        return Ok(bytes.clone());
    }
    if let Ok(path) = std::env::var(override_var) {
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read {override_var} historical Relay Wasm at {path}"))?;
        let _ = cache.set(bytes.clone());
        return Ok(bytes);
    }

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .context("resolve repository root")?
        .to_path_buf();
    let ancestor = Command::new("git")
        .args(["merge-base", "--is-ancestor", revision, "HEAD"])
        .current_dir(&repo)
        .status()
        .with_context(|| format!("validate historical Relay {label} revision"))?;
    if !ancestor.success() {
        bail!("historical Relay {label} revision {revision} is not an ancestor of HEAD");
    }

    let worktree = repo
        .parent()
        .context("resolve historical worktree parent")?
        .join(format!(
            ".jupiter-relay-{label}-{}-{}",
            std::process::id(),
            &revision[..12]
        ));
    let target_dir = repo.join(format!("target/historical-relay-{label}"));
    let add = Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            worktree
                .to_str()
                .context("UTF-8 historical worktree path")?,
            revision,
        ])
        .current_dir(&repo)
        .status()
        .with_context(|| format!("create historical Relay {label} worktree"))?;
    if !add.success() {
        bail!("failed to create historical Relay {label} worktree");
    }

    let build = Command::new("cargo")
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--locked",
            "--offline",
            "-p",
            "jupiter-relay",
            "--features",
            "debug_api",
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .current_dir(&worktree)
        .status()
        .with_context(|| format!("build historical Relay {label} Wasm"))?;
    let wasm_path = target_dir.join("wasm32-unknown-unknown/release/jupiter_relay.wasm");
    let bytes = if build.success() {
        std::fs::read(&wasm_path).with_context(|| format!("read historical Relay {label} Wasm"))
    } else {
        Err(anyhow::anyhow!(
            "historical Relay {label} Wasm build failed"
        ))
    };
    let remove = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            worktree
                .to_str()
                .context("UTF-8 historical worktree path")?,
        ])
        .current_dir(&repo)
        .status()
        .with_context(|| format!("remove historical Relay {label} worktree"))?;
    if !remove.success() {
        bail!("failed to remove historical Relay {label} worktree");
    }
    let bytes = bytes?;
    let _ = cache.set(bytes.clone());
    Ok(bytes)
}

fn relay_v1_wasm() -> Result<Vec<u8>> {
    historical_relay_wasm(
        RELAY_V1_REVISION,
        "v1",
        &RELAY_V1_WASM,
        "JUPITER_RELAY_V1_WASM",
    )
}

fn relay_v2_wasm() -> Result<Vec<u8>> {
    historical_relay_wasm(
        RELAY_V2_REVISION,
        "v2",
        &RELAY_V2_WASM,
        "JUPITER_RELAY_V2_WASM",
    )
}

fn sns_rewards_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(
        &SNS_REWARDS_WASM,
        "jupiter-sns-rewards",
        Some("debug_api"),
    )
}
fn sns_root_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&SNS_ROOT_WASM, "mock-sns-root", None)
}
fn sns_governance_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&SNS_GOVERNANCE_WASM, "mock-sns-governance", None)
}

fn wasm_contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayInitArg {
    managed_canisters: Vec<Principal>,
    ledger_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    max_transfers_per_tick: Option<u32>,
    surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
}

#[derive(Clone, Debug, CandidType)]
struct RewardRelayInitArg {
    managed_canisters: Vec<Principal>,
    ledger_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    sns_rewards_canister_id: Option<Principal>,
    icp_index_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    max_transfers_per_tick: Option<u32>,
    surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
}

#[derive(CandidType)]
struct SnsRewardsInitArgs {
    reward_sns_root_canister_id: Option<Principal>,
}

#[derive(CandidType)]
struct SnsNeuronId {
    id: Vec<u8>,
}

#[derive(CandidType)]
struct SnsNeuronPermission {
    principal: Option<Principal>,
    permission_type: Vec<i32>,
}

#[derive(CandidType)]
struct SnsNeuron {
    id: Option<SnsNeuronId>,
    permissions: Vec<SnsNeuronPermission>,
    cached_neuron_stake_e8s: u64,
    neuron_fees_e8s: u64,
}

#[derive(CandidType)]
struct SnsExtensions {
    extension_canister_ids: Vec<Principal>,
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

#[derive(Clone, CandidType, Deserialize, Debug, PartialEq, Eq)]
struct RewardJournalView {
    last_sweep_attempt_timestamp_seconds: u64,
    pending_payout: Option<RewardPendingPayoutView>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RewardPendingTransferStatusFixture {
    AwaitingTransfer,
    Ambiguous,
    NeedsFreshIdentity,
    WaitingForBalance,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum FrozenRewardPendingTransferStatusFixture {
    AwaitingTransfer,
    Ambiguous,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardPendingTransferFixture {
    sns_root_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    through_commitment_tx_id: u64,
    next_carried_credit_start_tx_id: Option<u64>,
    recipient: Account,
    observed_balance: Nat,
    fee: Nat,
    amount: Nat,
    memo: Vec<u8>,
    created_at_time_nanos: u64,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
    status: FrozenRewardPendingTransferStatusFixture,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardPendingTransferView {
    recipient: Account,
    observed_balance: Option<Nat>,
    amount: Nat,
    memo: Vec<u8>,
    created_at_time_nanos: u64,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
    status: RewardPendingTransferStatusFixture,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardPendingPayoutView {
    sns_root_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    attribution_commitment_tx_id: u64,
    fee: Nat,
    recipients: Vec<RewardPendingTransferView>,
    next_recipient_index: u32,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardStateFixture {
    epoch_sns_root_canister_id: Option<Principal>,
    processed_through_commitment_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
    last_sweep_attempt_timestamp_seconds: u64,
    pending_transfer: Option<RewardPendingTransferFixture>,
}

#[derive(Clone, Copy, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardHistoryBoundaryV2View {
    processed_through_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardPendingTransferV2View {
    sns_root_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    through_commitment_tx_id: u64,
    next_carried_credit_start_tx_id: Option<u64>,
    proposed_splitter_boundaries: BTreeMap<u8, RewardHistoryBoundaryV2View>,
    recipient: Account,
    observed_balance: Nat,
    fee: Nat,
    amount: Nat,
    memo: Vec<u8>,
    created_at_time_nanos: u64,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
    status: FrozenRewardPendingTransferStatusFixture,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct RewardJournalV2View {
    epoch_sns_root_canister_id: Option<Principal>,
    processed_through_commitment_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
    splitter_boundaries: BTreeMap<u8, RewardHistoryBoundaryV2View>,
    last_sweep_attempt_timestamp_seconds: u64,
    pending_transfer: Option<RewardPendingTransferV2View>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
#[allow(clippy::large_enum_variant)] // Mirrors the stable Candid bytes without boxing.
enum VersionedRewardStateFixture {
    Uninitialized,
    V1(RewardStateFixture),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum SplitterDestinationFixture {
    DefaultAccount,
    SubaccountOne,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct SplitterLegPlanFixture {
    destination: SplitterDestinationFixture,
    gross_share_e8s: u64,
    amount_e8s: u64,
    fee_e8s: u64,
    created_at_time_nanos: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct SplitterPlanFixture {
    splitter_number: u8,
    percentage_to_default: u8,
    source_subaccount: [u8; 32],
    balance_start_e8s: u64,
    default_leg: SplitterLegPlanFixture,
    subaccount_one_leg: SplitterLegPlanFixture,
}

#[derive(Clone, Copy, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum SplitterLegFixture {
    DefaultAccount,
    SubaccountOne,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum SplitterLegStatusFixture {
    Ready,
    WaitingForFunds { observed_balance_e8s: u64 },
    WaitingForFeasibleFee { expected_fee_e8s: Nat },
    Accepted { block_index: Nat },
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct SplitterLegProgressFixture {
    status: SplitterLegStatusFixture,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct ActiveSplitterJobFixture {
    pinned_ledger_canister_id: Principal,
    plan: SplitterPlanFixture,
    default_leg: SplitterLegProgressFixture,
    subaccount_one_leg: SplitterLegProgressFixture,
    driver_revision: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct QuarantinedSplitterJobFixture {
    job: ActiveSplitterJobFixture,
    blocked_leg: SplitterLegFixture,
    quarantined_at_nanos: u64,
    reason: String,
}

#[derive(Clone, Debug, Default, CandidType, Deserialize, PartialEq, Eq)]
struct SplitterStateFixture {
    active_job: Option<ActiveSplitterJobFixture>,
    quarantined_jobs: std::collections::BTreeMap<u8, QuarantinedSplitterJobFixture>,
    next_driver_revision: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)] // Mirrors the stable Candid bytes without boxing.
enum VersionedSplitterStateFixture {
    Uninitialized,
    V1(SplitterStateFixture),
}

impl Storable for VersionedSplitterStateFixture {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(encode_one(self).expect("encode splitter-state fixture"))
    }

    fn into_bytes(self) -> Vec<u8> {
        encode_one(self).expect("encode splitter-state fixture")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("decode splitter-state fixture")
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for VersionedRewardStateFixture {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(encode_one(self).expect("encode reward-state fixture"))
    }

    fn into_bytes(self) -> Vec<u8> {
        encode_one(self).expect("encode reward-state fixture")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("decode reward-state fixture")
    }

    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct SurplusCanisterRecipient {
    canister_id: Principal,
    memo: Vec<u8>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct SurplusNeuronRecipient {
    neuron_id: u64,
    memo: Vec<u8>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayMode {
    BaselineOnly,
    TopUpThenSurplus,
    Degraded,
    NoFunds,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelaySummary {
    mode: RelayMode,
    total_burn_cycles: u128,
    total_target_topup_cycles: u128,
    total_actual_minted_cycles: u128,
    total_carried_deficit_cycles: u128,
    total_remaining_deficit_cycles: u128,
    deficit_canister_count: u32,
    transfer_count: u32,
    ledger_transfer_count: u32,
    ledger_sent_e8s: u64,
    ledger_fees_e8s: u64,
    cmc_notify_success_count: u32,
    cmc_notify_failed_count: u32,
    cmc_notify_ambiguous_count: u32,
    planned_retained_e8s: u64,
    known_unspent_e8s: u64,
    ambiguous_e8s: u64,
    failed_transfers: u32,
    ambiguous_transfers: u32,
    partial_tick_count: u32,
    probe_failures: Vec<ProbeFailure>,
    canisters: Vec<CanisterBurnSample>,
    conversion_estimate_used: Option<ConversionEstimate>,
    surplus_transfers: Vec<SurplusTransferSample>,
    skipped_surplus_reason: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ConversionEstimate {
    cycles_per_e8: u128,
    timestamp_nanos: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ProbeFailure {
    canister_id: Principal,
    error: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterBurnSample {
    canister_id: Principal,
    previous_cycles: Option<u128>,
    current_cycles: u128,
    relay_minted_cycles: u128,
    burn_cycles: u128,
    carried_deficit_cycles: u128,
    target_topup_cycles: u128,
    gross_share_e8s: u64,
    amount_e8s: u64,
    sent_topup_e8s: u64,
    actual_minted_cycles: u128,
    remaining_deficit_cycles: u128,
    skipped_reason: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum SurplusTarget {
    Canister(Principal),
    Neuron(u64),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SurplusTransferSample {
    target: SurplusTarget,
    account: Account,
    gross_share_e8s: u64,
    amount_e8s: u64,
    memo_len: Option<u32>,
    skipped_reason: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNotifyBehavior {
    Ok,
    Processing,
    Other {
        error_code: u64,
        error_message: String,
    },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNextTransferError {
    PassThrough,
    AcceptThenTrap,
    TemporarilyUnavailable,
    BadFee { expected_fee_e8s: u64 },
    Duplicate { duplicate_of: u64 },
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct TransferRecord {
    from: Account,
    to: Account,
    amount: Nat,
    fee: Nat,
    memo: Option<Vec<u8>>,
    created_at_time: Option<u64>,
    result: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    last_main_run_ts: u64,
    main_lock_state_ts: Option<u64>,
    active_job_present: bool,
    active_job_pending_transfer_present: bool,
    active_faucet_commitment_transfer_present: bool,
    last_summary_present: bool,
    next_job_id: u64,
    last_completed_cycles_count: u32,
    relay_minted_cycles_since_sample_count: u32,
    recovery_deficit_cycles_count: u32,
    conversion_estimate_present: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct DebugConfig {
    managed_canisters: Vec<Principal>,
    effective_managed_canisters: Vec<Principal>,
    ledger_canister_id: Principal,
    cmc_canister_id: Principal,
    governance_canister_id: Principal,
    blackhole_canister_id: Principal,
    main_interval_seconds: u64,
    max_transfers_per_tick: Option<u32>,
    surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
}

struct RelayEnv {
    pic: PocketIc,
    ledger: Principal,
    cmc: Principal,
    governance: Principal,
    blackhole: Principal,
    relay: Principal,
}

impl RelayEnv {
    fn new(max_transfers_per_tick: Option<u32>) -> Result<Self> {
        Self::new_with_config(max_transfers_per_tick, |_, cmc, _, _| {
            (vec![cmc], None, Vec::new())
        })
    }

    fn new_with_config<F>(max_transfers_per_tick: Option<u32>, config: F) -> Result<Self>
    where
        F: FnOnce(
            Principal,
            Principal,
            Principal,
            Principal,
        ) -> (
            Vec<Principal>,
            Option<Vec<SurplusCanisterRecipient>>,
            Vec<SurplusNeuronRecipient>,
        ),
    {
        let pic = support::pocketic::builder()
            .with_application_subnet()
            .build();
        let ledger = pic.create_canister();
        let cmc = pic.create_canister();
        let governance = pic.create_canister();
        let blackhole = pic.create_canister();
        let relay = pic.create_canister();
        for canister in [ledger, cmc, governance, blackhole, relay] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(governance, governance_wasm()?, vec![], None);
        pic.install_canister(blackhole, blackhole_wasm()?, vec![], None);
        let (managed_canisters, surplus_canister_recipients, surplus_neuron_recipients) =
            config(ledger, cmc, blackhole, relay);
        let init = RelayInitArg {
            managed_canisters,
            ledger_canister_id: Some(ledger),
            cmc_canister_id: Some(cmc),
            governance_canister_id: Some(governance),
            blackhole_canister_id: Some(blackhole),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick,
            surplus_canister_recipients,
            surplus_neuron_recipients,
        };
        pic.install_canister(relay, relay_wasm()?, encode_one(init)?, None);
        Ok(Self {
            pic,
            ledger,
            cmc,
            governance,
            blackhole,
            relay,
        })
    }

    fn new_with_production_blackholes_managed() -> Result<Self> {
        let pic = support::pocketic::builder()
            .with_application_subnet()
            .build();
        let ledger = pic.create_canister();
        let cmc = pic.create_canister();
        let governance = pic.create_canister();
        let blackhole = pic.create_canister();
        let relay = pic.create_canister();
        let fiduciary_blackhole = principal("77deu-baaaa-aaaar-qb6za-cai");
        let thirteen_node_blackhole = principal("e3mmv-5qaaa-aaaah-aadma-cai");
        pic.create_canister_with_id(None, None, fiduciary_blackhole)
            .map_err(anyhow::Error::msg)?;
        pic.create_canister_with_id(None, None, thirteen_node_blackhole)
            .map_err(anyhow::Error::msg)?;

        for canister in [
            ledger,
            cmc,
            governance,
            blackhole,
            relay,
            fiduciary_blackhole,
            thirteen_node_blackhole,
        ] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(governance, governance_wasm()?, vec![], None);
        pic.install_canister(blackhole, blackhole_wasm()?, vec![], None);
        pic.install_canister(fiduciary_blackhole, blackhole_wasm()?, vec![], None);
        pic.install_canister(thirteen_node_blackhole, blackhole_wasm()?, vec![], None);

        let init = RelayInitArg {
            managed_canisters: vec![cmc, fiduciary_blackhole, thirteen_node_blackhole],
            ledger_canister_id: Some(ledger),
            cmc_canister_id: Some(cmc),
            governance_canister_id: Some(governance),
            blackhole_canister_id: Some(blackhole),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick: None,
            surplus_canister_recipients: None,
            surplus_neuron_recipients: Vec::new(),
        };
        pic.install_canister(relay, relay_wasm()?, encode_one(init)?, None);

        for (probe, target, cycles) in [
            (blackhole, cmc, 10_000_000_000_000_u128),
            (
                fiduciary_blackhole,
                fiduciary_blackhole,
                20_000_000_000_000_u128,
            ),
            (
                thirteen_node_blackhole,
                thirteen_node_blackhole,
                30_000_000_000_000_u128,
            ),
        ] {
            let _: () = update_bytes(
                &pic,
                probe,
                Principal::anonymous(),
                "debug_set_status",
                encode_args((target, Some(Nat::from(cycles)), vec![probe]))?,
            )?;
        }

        Ok(Self {
            pic,
            ledger,
            cmc,
            governance,
            blackhole,
            relay,
        })
    }

    fn set_managed_cycles(&self, cycles: u128) -> Result<()> {
        self.set_canister_cycles(self.cmc, cycles)
    }

    fn set_canister_cycles(&self, canister: Principal, cycles: u128) -> Result<()> {
        let _: () = update_bytes(
            &self.pic,
            self.blackhole,
            Principal::anonymous(),
            "debug_set_status",
            encode_args((canister, Some(Nat::from(cycles)), vec![self.blackhole]))?,
        )?;
        Ok(())
    }

    fn credit_relay(&self, amount_e8s: u64) -> Result<()> {
        let relay_account = Account {
            owner: self.relay,
            subaccount: None,
        };
        let _: () = update_bytes(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_credit",
            encode_args((relay_account, amount_e8s))?,
        )?;
        Ok(())
    }

    fn credit_relay_subaccount_one(&self, amount_e8s: u64) -> Result<()> {
        self.credit_relay_numbered_subaccount(1, amount_e8s)
    }

    fn credit_relay_numbered_subaccount(&self, number: u8, amount_e8s: u64) -> Result<()> {
        let relay_account = Account {
            owner: self.relay,
            subaccount: Some(relay_numbered_subaccount(number)),
        };
        let _: () = update_bytes(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_credit",
            encode_args((relay_account, amount_e8s))?,
        )?;
        Ok(())
    }

    fn set_ledger_fee(&self, fee_e8s: u64) -> Result<()> {
        update_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_set_fee",
            fee_e8s,
        )
    }

    fn set_ledger_fee_query_failure(&self, value: bool) -> Result<()> {
        update_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_set_fee_query_failure",
            value,
        )
    }

    fn add_relay_cycles(&self, cycles: u128) {
        self.pic.add_cycles(self.relay, cycles);
    }

    fn tick_relay(&self) -> Result<RelaySummary> {
        self.trigger_relay()?;
        self.summary()
    }

    fn trigger_relay(&self) -> Result<()> {
        let _: () = update_noargs(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_main_tick",
        )?;
        tick_n(&self.pic, 5);
        Ok(())
    }

    fn summary(&self) -> Result<RelaySummary> {
        let summary: Option<RelaySummary> = query_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_last_summary",
            (),
        )?;
        summary.context("expected relay summary")
    }

    fn logs_text(&self) -> Result<String> {
        let records = self
            .pic
            .fetch_canister_logs(self.relay, Principal::anonymous())
            .map_err(|e| anyhow::anyhow!("fetch_canister_logs reject: {e:?}"))?;
        Ok(records
            .iter()
            .map(|record| String::from_utf8_lossy(&record.content).into_owned())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn transfers(&self) -> Result<Vec<TransferRecord>> {
        query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_transfers",
            (),
        )
    }

    fn relay_balance(&self) -> Result<u64> {
        let balance: Nat = query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "icrc1_balance_of",
            Account {
                owner: self.relay,
                subaccount: None,
            },
        )?;
        Ok(nat_to_u64(&balance))
    }

    fn relay_subaccount_one_balance(&self) -> Result<u64> {
        self.relay_numbered_subaccount_balance(1)
    }

    fn relay_numbered_subaccount_balance(&self, number: u8) -> Result<u64> {
        let balance: Nat = query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "icrc1_balance_of",
            Account {
                owner: self.relay,
                subaccount: Some(relay_numbered_subaccount(number)),
            },
        )?;
        Ok(nat_to_u64(&balance))
    }

    fn splitter_transfers(&self, number: u8) -> Result<Vec<TransferRecord>> {
        Ok(self
            .transfers()?
            .into_iter()
            .filter(|transfer| {
                transfer.from
                    == (Account {
                        owner: self.relay,
                        subaccount: Some(relay_numbered_subaccount(number)),
                    })
            })
            .collect())
    }

    fn notifications(&self) -> Result<Vec<NotifyRecord>> {
        query_one(
            &self.pic,
            self.cmc,
            Principal::anonymous(),
            "debug_notifications",
            (),
        )
    }

    fn claim_or_refresh_calls(&self) -> Result<u64> {
        query_one(
            &self.pic,
            self.governance,
            Principal::anonymous(),
            "debug_get_claim_or_refresh_calls",
            (),
        )
    }

    fn set_claim_or_refresh_fails(&self, value: bool) -> Result<()> {
        update_one(
            &self.pic,
            self.governance,
            Principal::anonymous(),
            "debug_set_claim_or_refresh_fails",
            value,
        )
    }

    fn set_cmc_script(&self, script: Vec<DebugNotifyBehavior>) -> Result<()> {
        update_one(
            &self.pic,
            self.cmc,
            Principal::anonymous(),
            "debug_set_script",
            script,
        )
    }

    fn set_ledger_error_script(&self, script: Vec<DebugNextTransferError>) -> Result<()> {
        update_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_set_error_script",
            script,
        )
    }

    fn abort_after_successful_transfer(&self) -> Result<()> {
        update_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_abort_after_successful_transfer",
            true,
        )
    }

    fn debug_state(&self) -> Result<DebugState> {
        query_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_state",
            (),
        )
    }

    fn debug_config(&self) -> Result<DebugConfig> {
        query_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_config",
            (),
        )
    }

    fn advance_time_and_tick(&self, secs: u64, ticks: usize) {
        self.pic.advance_time(Duration::from_secs(secs));
        tick_n(&self.pic, ticks);
    }

    fn default_init_arg(&self) -> RelayInitArg {
        RelayInitArg {
            managed_canisters: vec![self.cmc],
            ledger_canister_id: Some(self.ledger),
            cmc_canister_id: Some(self.cmc),
            governance_canister_id: Some(self.governance),
            blackhole_canister_id: Some(self.blackhole),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick: None,
            surplus_canister_recipients: None,
            surplus_neuron_recipients: Vec::new(),
        }
    }

    fn try_upgrade_relay_without_args(&self) -> Result<Result<(), String>> {
        Ok(self
            .pic
            .upgrade_canister(
                self.relay,
                relay_wasm()?,
                vec![],
                Some(Principal::anonymous()),
            )
            .map_err(|e| format!("{e:?}")))
    }

    fn upgrade_relay_with_init_args(&self, init: RelayInitArg) -> Result<()> {
        self.pic
            .upgrade_canister(
                self.relay,
                relay_wasm()?,
                encode_one(init)?,
                Some(Principal::anonymous()),
            )
            .map_err(|e| anyhow::anyhow!("upgrade_canister reject: {e:?}"))?;
        Ok(())
    }

    fn reinstall_relay_with_default_config(&self) -> Result<()> {
        self.pic
            .reinstall_canister(
                self.relay,
                relay_wasm()?,
                encode_one(self.default_init_arg())?,
                Some(Principal::anonymous()),
            )
            .map_err(|e| anyhow::anyhow!("reinstall_canister reject: {e:?}"))?;
        Ok(())
    }
}

fn nat_to_u64(value: &Nat) -> u64 {
    value.0.to_string().parse().unwrap_or(u64::MAX)
}

fn neuron_subaccount(neuron_id: u64) -> [u8; 32] {
    let mut account = [0u8; 32];
    account[24..].copy_from_slice(&neuron_id.to_be_bytes());
    account
}

fn relay_numbered_subaccount(number: u8) -> [u8; 32] {
    let mut subaccount = [0u8; 32];
    subaccount[31] = number;
    subaccount
}

fn relay_subaccount_one() -> [u8; 32] {
    relay_numbered_subaccount(1)
}

fn assert_splitter_transfer_pair(
    env: &RelayEnv,
    splitter_number: u8,
    starting_balance_e8s: u64,
    fee_e8s: u64,
) -> Result<Vec<TransferRecord>> {
    let transfers = env.splitter_transfers(splitter_number)?;
    if transfers.len() != 2 {
        bail!("expected exactly two transfers from splitter {splitter_number}, got {transfers:?}");
    }
    assert_splitter_transfer_pair_records(
        env.relay,
        splitter_number,
        starting_balance_e8s,
        fee_e8s,
        &transfers,
    )?;
    Ok(transfers)
}

fn assert_splitter_transfer_pair_records(
    relay: Principal,
    splitter_number: u8,
    starting_balance_e8s: u64,
    fee_e8s: u64,
    transfers: &[TransferRecord],
) -> Result<()> {
    if transfers.len() != 2 {
        bail!("expected one splitter transfer pair, got {transfers:?}");
    }
    let default_gross =
        u64::try_from(u128::from(starting_balance_e8s) * u128::from(splitter_number) / 100)?;
    let subaccount_one_gross = starting_balance_e8s - default_gross;
    if transfers[0].to
        != (Account {
            owner: relay,
            subaccount: None,
        })
        || transfers[1].to
            != (Account {
                owner: relay,
                subaccount: Some(relay_subaccount_one()),
            })
        || nat_to_u64(&transfers[0].amount) + fee_e8s != default_gross
        || nat_to_u64(&transfers[1].amount) + fee_e8s != subaccount_one_gross
        || nat_to_u64(&transfers[0].fee) != fee_e8s
        || nat_to_u64(&transfers[1].fee) != fee_e8s
    {
        bail!("unexpected splitter {splitter_number} transfer pair: {transfers:?}");
    }
    if nat_to_u64(&transfers[0].amount)
        + nat_to_u64(&transfers[0].fee)
        + nat_to_u64(&transfers[1].amount)
        + nat_to_u64(&transfers[1].fee)
        != starting_balance_e8s
    {
        bail!("splitter {splitter_number} did not conserve its pinned balance");
    }
    Ok(())
}

#[test]
#[ignore]
fn fixed_splitters_10_50_90_route_exact_gross_shares_and_drain_source() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(Some(1))?;
    let starting_balance = 250_000_007;
    for splitter_number in [10, 50, 90] {
        env.credit_relay_numbered_subaccount(splitter_number, starting_balance)?;
        env.trigger_relay()?;
        assert_splitter_transfer_pair(&env, splitter_number, starting_balance, 10_000)?;
        if env.relay_numbered_subaccount_balance(splitter_number)? != 0 {
            bail!("splitter {splitter_number} did not drain its pinned starting balance");
        }
    }
    Ok(())
}

#[test]
#[ignore]
fn splitter_accumulates_below_threshold_and_splits_full_balance_after_crossing() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.credit_relay_numbered_subaccount(30, 100_009_999)?;
    env.trigger_relay()?;
    if !env.splitter_transfers(30)?.is_empty()
        || env.relay_numbered_subaccount_balance(30)? != 100_009_999
    {
        bail!("splitter 30 should remain untouched below the ordinary threshold");
    }

    env.credit_relay_numbered_subaccount(30, 1)?;
    env.trigger_relay()?;
    assert_splitter_transfer_pair(&env, 30, 100_010_000, 10_000)?;
    Ok(())
}

#[test]
#[ignore]
fn splitter_chains_into_existing_subaccount_one_commitment_in_same_tick() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.credit_relay_numbered_subaccount(10, 200_000_000)?;
    env.trigger_relay()?;

    let transfers = env.transfers()?;
    if transfers.len() < 3 {
        bail!("expected splitter pair followed by Faucet commitment, got {transfers:?}");
    }
    if transfers[0].from.subaccount != Some(relay_numbered_subaccount(10))
        || transfers[0].to.subaccount.is_some()
        || transfers[1].from.subaccount != Some(relay_numbered_subaccount(10))
        || transfers[1].to.subaccount != Some(relay_subaccount_one())
        || transfers[2].from.subaccount != Some(relay_subaccount_one())
        || transfers[2].to.owner != env.governance
    {
        bail!("unexpected same-tick splitter/Faucet ordering: {transfers:?}");
    }
    let expected_memo = format!("{}.Relay", env.relay.to_text().replace('-', "")).into_bytes();
    if transfers[2].memo.as_deref() != Some(expected_memo.as_slice()) {
        bail!("same-tick third transfer did not use the existing Faucet commitment memo");
    }
    Ok(())
}

#[test]
#[ignore]
fn splitter_default_branch_is_available_to_allocation_in_same_tick() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected allocation baseline before splitter funding, got {baseline:?}");
    }

    env.set_managed_cycles(9_000_000_000_000)?;
    env.credit_relay_numbered_subaccount(90, 500_000_000)?;
    let summary = env.tick_relay()?;
    assert_splitter_transfer_pair(&env, 90, 500_000_000, 10_000)?;
    let transfers = env.transfers()?;
    let allocation_position = transfers.iter().position(|transfer| {
        transfer.from
            == (Account {
                owner: env.relay,
                subaccount: None,
            })
    });
    if allocation_position.is_none_or(|position| position < 2)
        || summary.ledger_transfer_count == 0
        || summary.cmc_notify_success_count == 0
    {
        bail!("expected same-tick default-account allocation after splitter pair: summary={summary:?} transfers={transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn splitter_lost_response_retries_exact_identity_without_duplicate_spend() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.advance_time_and_tick(5 * 60, 5);
    let starting_balance = 500_000_007;
    env.credit_relay_numbered_subaccount(50, starting_balance)?;
    env.set_ledger_error_script(vec![DebugNextTransferError::AcceptThenTrap])?;
    env.trigger_relay()?;
    let first = env.splitter_transfers(50)?;
    if first.len() != 1 || env.transfers()?.len() != 1 {
        bail!("expected one accepted transfer with a lost response, got {first:?}");
    }
    env.advance_time_and_tick(3_601, 30);
    let completed = assert_splitter_transfer_pair(&env, 50, starting_balance, 10_000)?;
    if completed[0] != first[0] || env.relay_numbered_subaccount_balance(50)? != 0 {
        bail!("lost-response retry changed the accepted first transfer or re-split residual funds");
    }
    Ok(())
}

#[test]
#[ignore]
fn splitter_journal_survives_upgrade_and_second_leg_bad_fee_preserves_gross_budget() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    let starting_balance = 500_000_007;
    env.credit_relay_numbered_subaccount(30, starting_balance)?;
    env.abort_after_successful_transfer()?;
    let _: () = update_noargs(
        &env.pic,
        env.relay,
        Principal::anonymous(),
        "debug_main_tick",
    )?;
    if env.splitter_transfers(30)?.len() != 1 || env.transfers()?.len() != 1 {
        bail!("expected debug interruption after the accepted first splitter leg");
    }
    if !matches!(
        read_splitter_state_fixture(&env.pic, env.relay),
        VersionedSplitterStateFixture::V1(SplitterStateFixture {
            active_job: Some(_),
            ..
        })
    ) {
        bail!("expected the interrupted splitter job to remain active in memory 1");
    }

    let mismatched_ledger_args = RelayInitArg {
        ledger_canister_id: Some(Principal::from_slice(&[0x7d, 0x01])),
        ..env.default_init_arg()
    };
    let mismatch = env.pic.upgrade_canister(
        env.relay,
        relay_wasm()?,
        encode_one(mismatched_ledger_args)?,
        Some(Principal::anonymous()),
    );
    if mismatch.is_ok() {
        bail!("an active splitter journal must reject an upgrade that changes the pinned ledger");
    }
    env.pic.advance_time(Duration::from_secs(2));
    env.upgrade_relay_with_init_args(env.default_init_arg())?;
    env.trigger_relay()?;
    assert_splitter_transfer_pair(&env, 30, starting_balance, 10_000)?;

    let env = RelayEnv::new(None)?;
    env.credit_relay_numbered_subaccount(30, starting_balance)?;
    env.set_ledger_error_script(vec![
        DebugNextTransferError::PassThrough,
        DebugNextTransferError::BadFee {
            expected_fee_e8s: 20_000,
        },
    ])?;
    env.trigger_relay()?;
    if env.splitter_transfers(30)?.len() != 1 {
        bail!("expected first leg accepted before second-leg BadFee");
    }
    env.set_ledger_fee(20_000)?;
    env.trigger_relay()?;
    let transfers = env.splitter_transfers(30)?;
    if transfers.len() != 2 {
        bail!("expected safely re-pinned second leg, got {transfers:?}");
    }
    let original_second_gross =
        starting_balance - u64::try_from(u128::from(starting_balance) * 30_u128 / 100)?;
    if nat_to_u64(&transfers[1].amount) + nat_to_u64(&transfers[1].fee) != original_second_gross
        || nat_to_u64(&transfers[1].fee) != 20_000
        || env.relay_numbered_subaccount_balance(30)? != 0
    {
        bail!("second-leg fee replacement changed the original gross budget: {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn deposit_arriving_between_splitter_legs_remains_for_an_independent_later_split() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    let original_balance = 500_000_007;
    let later_deposit = 300_000_019;
    env.credit_relay_numbered_subaccount(30, original_balance)?;
    env.abort_after_successful_transfer()?;
    env.trigger_relay()?;
    let first_leg = env.splitter_transfers(30)?;
    if first_leg.len() != 1 {
        bail!("expected the original split to pause after its first accepted leg: {first_leg:?}");
    }

    env.credit_relay_numbered_subaccount(30, later_deposit)?;
    env.trigger_relay()?;
    let original_pair = env.splitter_transfers(30)?;
    assert_splitter_transfer_pair_records(env.relay, 30, original_balance, 10_000, &original_pair)?;
    if original_pair[0] != first_leg[0]
        || env.relay_numbered_subaccount_balance(30)? != later_deposit
    {
        bail!(
            "the pinned split absorbed a later deposit or changed its first leg: {original_pair:?}"
        );
    }

    env.trigger_relay()?;
    let all_transfers = env.splitter_transfers(30)?;
    if all_transfers.len() != 4 {
        bail!("expected two independent splitter operations, got {all_transfers:?}");
    }
    assert_splitter_transfer_pair_records(
        env.relay,
        30,
        later_deposit,
        10_000,
        &all_transfers[2..],
    )?;
    if env.relay_numbered_subaccount_balance(30)? != 0 {
        bail!("the independently planned later deposit was not drained");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_production_wasm_does_not_export_status_or_admin_endpoints() -> Result<()> {
    require_ignored_flag()?;
    let wasm = relay_prod_wasm()?;
    let removed_debug_marker = b"relay_"
        .iter()
        .chain(b"status")
        .copied()
        .collect::<Vec<_>>();
    let removed_debug_query_marker = b"canister_query "
        .iter()
        .chain(removed_debug_marker.iter())
        .copied()
        .collect::<Vec<_>>();
    for needle in [
        removed_debug_query_marker.as_slice(),
        b"canister_update admin_schedule_main_tick_now".as_slice(),
        removed_debug_marker.as_slice(),
        b"admin_schedule_main_tick_now".as_slice(),
    ] {
        if wasm_contains(&wasm, needle) {
            bail!(
                "production relay Wasm unexpectedly contains exported endpoint marker `{}`",
                String::from_utf8_lossy(needle)
            );
        }
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_debug_wasm_does_not_export_status_or_admin_endpoints() -> Result<()> {
    require_ignored_flag()?;
    let wasm = relay_wasm()?;
    let removed_debug_marker = b"relay_"
        .iter()
        .chain(b"status")
        .copied()
        .collect::<Vec<_>>();
    let removed_debug_query_marker = b"canister_query "
        .iter()
        .chain(removed_debug_marker.iter())
        .copied()
        .collect::<Vec<_>>();
    for needle in [
        removed_debug_query_marker.as_slice(),
        removed_debug_marker.as_slice(),
        b"canister_update admin_schedule_main_tick_now".as_slice(),
        b"admin_schedule_main_tick_now".as_slice(),
    ] {
        if wasm_contains(&wasm, needle) {
            bail!(
                "debug relay Wasm unexpectedly contains status/admin endpoint marker `{}`",
                String::from_utf8_lossy(needle)
            );
        }
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_forwards_without_default_account_funds() -> Result<()> {
    require_ignored_flag()?;
    let jupiter_faucet_neuron = 11_614_578_985_374_291_210_u64;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_010_000)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || summary.cmc_notify_success_count != 0 {
        bail!("expected default-account job to avoid ledger work, got {summary:?}");
    }
    if !env.notifications()?.is_empty() {
        bail!("expected no CMC notification for subaccount-1 forwarding");
    }

    let transfers = env.transfers()?;
    if transfers.len() != 1 {
        bail!("expected exactly one subaccount-1 transfer, got {transfers:?}");
    }
    let transfer = &transfers[0];
    if transfer.from
        != (Account {
            owner: env.relay,
            subaccount: Some(relay_subaccount_one()),
        })
    {
        bail!("expected transfer source to be Relay subaccount 1, got {transfer:?}");
    }
    if transfer.to
        != (Account {
            owner: env.governance,
            subaccount: Some(neuron_subaccount(jupiter_faucet_neuron)),
        })
    {
        bail!("expected transfer destination to be Jupiter Faucet neuron staking account, got {transfer:?}");
    }
    if nat_to_u64(&transfer.amount) != 100_000_000 || nat_to_u64(&transfer.fee) != 10_000 {
        bail!("expected transfer to send balance minus fee, got {transfer:?}");
    }
    let expected_memo = format!("{}.Relay", env.relay.to_text().replace('-', "")).into_bytes();
    if transfer.memo.as_deref() != Some(expected_memo.as_slice()) {
        bail!("expected compact Relay Faucet memo, got {transfer:?}");
    }
    if env.claim_or_refresh_calls()? != 0 {
        bail!("expected claim_or_refresh to be deferred until the accepted transfer is retried");
    }
    let _ = env.tick_relay()?;
    if env.claim_or_refresh_calls()? != 1 {
        bail!("expected one deferred Jupiter Faucet neuron claim_or_refresh call");
    }
    if env.transfers()?.len() != 1 {
        bail!("expected deferred refresh not to duplicate the ledger transfer");
    }
    if env.relay_balance()? != 0 || env.relay_subaccount_one_balance()? != 0 {
        bail!(
            "expected default and subaccount-1 balances to be zero after transfer, default={} sub1={}",
            env.relay_balance()?,
            env.relay_subaccount_one_balance()?
        );
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_FAUCET_COMMITMENT ")
        || !logs.contains("amount_e8s=100000000")
        || !logs.contains("memo_len=")
        || logs.contains(&String::from_utf8(expected_memo).unwrap())
    {
        bail!("expected faucet commitment log without raw memo bytes, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_uses_bootstrap_fee_when_fee_query_fails_without_cache() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.set_ledger_fee_query_failure(true)?;
    env.credit_relay_subaccount_one(100_010_000)?;

    let _ = env.tick_relay()?;
    let transfers = env.transfers()?;
    if transfers.len() != 1
        || nat_to_u64(&transfers[0].amount) != 100_000_000
        || nat_to_u64(&transfers[0].fee) != 10_000
    {
        bail!("expected bootstrap-fee subaccount-1 transfer, got {transfers:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("context=subaccount_1")
        || !logs.contains("fallback_source=bootstrap")
        || !logs.contains("fee_e8s=10000")
    {
        bail!("expected structured bootstrap fee fallback log, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_uses_cached_live_fee_when_later_fee_query_fails() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.set_ledger_fee(20_000)?;

    let _ = env.tick_relay()?;
    env.set_ledger_fee_query_failure(true)?;
    env.credit_relay_subaccount_one(100_020_000)?;

    let _ = env.tick_relay()?;
    let transfers = env.transfers()?;
    if transfers.len() != 1
        || nat_to_u64(&transfers[0].amount) != 100_000_000
        || nat_to_u64(&transfers[0].fee) != 20_000
    {
        bail!("expected cached-live-fee subaccount-1 transfer, got {transfers:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("context=subaccount_1")
        || !logs.contains("fallback_source=cached")
        || !logs.contains("fee_e8s=20000")
    {
        bail!("expected structured cached fee fallback log, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_waits_until_one_icp_net() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_009_999)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || !env.transfers()?.is_empty() {
        bail!("expected no transfer below 1 ICP net threshold, got {summary:?}");
    }
    if env.relay_subaccount_one_balance()? != 100_009_999 {
        bail!(
            "expected subaccount-1 balance to remain accumulated, got {}",
            env.relay_subaccount_one_balance()?
        );
    }
    let logs = env.logs_text()?;
    if logs.contains("skipped_reason=subaccount_1_below_1_icp_net") {
        bail!("expected below-threshold subaccount-1 scan to stay out of repeated public logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_no_funds_is_quiet_without_skip_log() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || !env.transfers()?.is_empty() {
        bail!("expected no transfer with empty subaccount-1, got {summary:?}");
    }

    let logs = env.logs_text()?;
    if logs.contains("skipped_reason=subaccount_1_no_funds") {
        bail!("expected no-funds scan to stay out of repeated public logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_treats_ledger_duplicate_as_accepted() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_010_000)?;
    env.set_ledger_error_script(vec![DebugNextTransferError::Duplicate { duplicate_of: 77 }])?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || summary.cmc_notify_success_count != 0 {
        bail!("expected default-account job to stay idle, got {summary:?}");
    }
    if env.claim_or_refresh_calls()? != 0 {
        bail!("expected duplicate response refresh to be deferred until the accepted transfer is retried");
    }
    if !env.transfers()?.is_empty() {
        bail!("mock duplicate response should not create a second ledger transfer record");
    }
    let _ = env.tick_relay()?;
    if env.claim_or_refresh_calls()? != 1 {
        bail!("expected accepted duplicate response to be followed by deferred claim_or_refresh");
    }
    if !env.transfers()?.is_empty() {
        bail!("mock duplicate response should not create a ledger transfer record after deferred refresh");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_FAUCET_COMMITMENT ")
        || !logs.contains("amount_e8s=100000000")
        || !logs.contains("skipped_reason=null")
    {
        bail!("expected accepted duplicate faucet commitment log, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_refresh_failure_does_not_duplicate_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_010_000)?;
    env.set_claim_or_refresh_fails(true)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || summary.cmc_notify_success_count != 0 {
        bail!("expected default-account job to stay idle, got {summary:?}");
    }
    let transfers = env.transfers()?;
    if transfers.len() != 1 {
        bail!("expected exactly one ledger transfer despite refresh failure, got {transfers:?}");
    }

    let _ = env.tick_relay()?;
    let transfers_after = env.transfers()?;
    if transfers_after.len() != 1 {
        bail!("expected no duplicate transfer on later tick after refresh failure, got {transfers_after:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_FAUCET_COMMITMENT ")
        || !logs.contains("skipped_reason=null")
        || !logs.contains("relay ERR message=faucet%20commitment%20neuron%20refresh%20failed")
    {
        bail!("expected accepted transfer and logged follow-up refresh failure, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn baseline_then_headroom_cmc_topup_records_real_async_notify() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(10_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_managed_cycles(5_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.mode != RelayMode::TopUpThenSurplus
        || topup.ledger_transfer_count == 0
        || topup.cmc_notify_success_count == 0
    {
        bail!("expected successful cycles top-up after baseline, got {topup:?}");
    }
    let notifications: Vec<NotifyRecord> = query_one(
        &env.pic,
        env.cmc,
        Principal::anonymous(),
        "debug_notifications",
        (),
    )?;
    if notifications
        .iter()
        .all(|notification| notification.canister_id != env.cmc)
    {
        bail!("expected CMC notification for managed canister, got {notifications:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("Cycles:")
        || !logs.contains("CONFIG ")
        || !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("RELAY_CANISTER ")
        || !logs.contains("burn_cycles=")
        || !logs.contains("planned_topup_e8s=")
        || !logs.contains("sent_topup_e8s=")
        || !logs.contains("total_remaining_deficit_cycles=")
    {
        bail!("expected public relay logs for cycles top-up, got {logs}");
    }
    if logs.contains("relay INFO ") {
        bail!("relay INFO logs should not be emitted, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn no_raw_recipients_routes_all_spendable_icp_as_cycles() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(10_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus
        || summary.cmc_notify_success_count == 0
        || !summary.surplus_transfers.is_empty()
        || summary.skipped_surplus_reason.as_deref() != Some("no_raw_icp_recipients")
    {
        bail!("expected all-cycles allocation with no raw surplus phase, got {summary:?}");
    }
    let sample = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing managed CMC burn sample")?;
    if sample.amount_e8s == 0 || sample.gross_share_e8s <= 99_000_000 {
        bail!(
            "expected nearly all spendable ICP gross to go to the burned canister, got {summary:?}"
        );
    }
    if summary
        .canisters
        .iter()
        .filter(|sample| sample.burn_cycles > 0)
        .any(|sample| sample.amount_e8s == 0)
    {
        bail!("expected every positive-burn canister to receive a top-up, got {summary:?}");
    }
    if summary.planned_retained_e8s >= 10_000 {
        bail!("expected only fee-unspendable dust to remain, got {summary:?}");
    }
    let transfers = env.transfers()?;
    if !transfers.iter().any(|transfer| {
        transfer.to.owner == env.cmc
            && transfer.to.subaccount == Some(principal_to_subaccount(env.cmc))
    }) {
        bail!("expected CMC top-up transfer for burned managed canister, got {transfers:?}");
    }
    if transfers
        .iter()
        .any(|transfer| transfer.to.owner != env.cmc)
    {
        bail!("expected no raw ICP surplus transfer without recipients, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn no_raw_recipients_waits_until_every_positive_burner_is_fee_efficient() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.cmc, 10_000_000_000_000)?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.credit_relay(30_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_canister_cycles(env.cmc, 1_000_000_000_000)?;
    env.set_canister_cycles(env.ledger, 9_900_000_000_000)?;
    let retained = env.tick_relay()?;
    if retained.mode != RelayMode::TopUpThenSurplus
        || retained.ledger_transfer_count != 0
        || retained.cmc_notify_success_count != 0
        || retained.planned_retained_e8s != 30_000
        || retained.skipped_surplus_reason.as_deref()
            != Some("all_cycles_batch_below_fee_efficient_threshold")
    {
        bail!("expected all-cycles batch gate to retain insufficient balance, got {retained:?}");
    }
    if retained
        .canisters
        .iter()
        .filter(|sample| sample.burn_cycles > 0)
        .any(|sample| {
            sample.amount_e8s != 0
                || sample.skipped_reason.as_deref()
                    != Some("all_cycles_batch_below_fee_efficient_threshold")
        })
    {
        bail!("expected no partial fast-burner top-up below threshold, got {retained:?}");
    }
    if !env.transfers()?.is_empty() || env.relay_balance()? != 30_000 {
        bail!("expected retained ICP to remain in relay default ledger account");
    }

    env.credit_relay(10_000_000_000)?;
    env.set_canister_cycles(env.cmc, 100_000_000_000)?;
    env.set_canister_cycles(env.ledger, 9_800_000_000_000)?;
    let funded = env.tick_relay()?;
    let positive = funded
        .canisters
        .iter()
        .filter(|sample| sample.burn_cycles > 0)
        .collect::<Vec<_>>();
    if funded.mode != RelayMode::TopUpThenSurplus
        || funded.cmc_notify_success_count != positive.len() as u32
        || !funded.surplus_transfers.is_empty()
        || funded.skipped_surplus_reason.as_deref() != Some("no_raw_icp_recipients")
    {
        bail!("expected fee-efficient all-cycles batch without raw ICP surplus, got {funded:?}");
    }
    if positive.len() < 2
        || positive
            .iter()
            .any(|sample| sample.amount_e8s == 0 || sample.gross_share_e8s < 20_000)
    {
        bail!(
            "expected all positive-burn canisters to receive fee-efficient top-ups, got {funded:?}"
        );
    }

    let transfers = env.transfers()?;
    let cmc_topup_accounts = [
        Account {
            owner: env.cmc,
            subaccount: Some(principal_to_subaccount(env.cmc)),
        },
        Account {
            owner: env.cmc,
            subaccount: Some(principal_to_subaccount(env.ledger)),
        },
    ];
    if !cmc_topup_accounts
        .iter()
        .all(|account| transfers.iter().any(|transfer| transfer.to == *account))
        || transfers
            .iter()
            .any(|transfer| transfer.to.owner != env.cmc)
    {
        bail!("expected CMC top-up transfers for positive burners and no raw surplus, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn headroom_cmc_topup_prefers_higher_burn_managed_canister() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(10_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_canister_cycles(env.ledger, 9_000_000_000_000)?;
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.cmc_notify_success_count < 2 {
        bail!("expected successful multi-canister cycles top-up, got {summary:?}");
    }

    let ledger_sample = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.ledger)
        .context("missing ledger managed-canister allocation")?;
    let cmc_sample = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing CMC managed-canister allocation")?;
    if cmc_sample.gross_share_e8s <= ledger_sample.gross_share_e8s {
        bail!("expected higher-burn CMC canister to receive larger gross share: ledger={ledger_sample:?} cmc={cmc_sample:?}");
    }
    let logs = env.logs_text()?;
    let cmc_log_fragment = format!(
        "RELAY_CANISTER canister_id={} previous_cycles=",
        env.cmc.to_text()
    );
    if !logs.contains(&cmc_log_fragment)
        || !logs.contains(&format!("burn_cycles={}", cmc_sample.burn_cycles))
        || !logs.contains(&format!("planned_topup_e8s={}", cmc_sample.amount_e8s))
        || !logs.contains(&format!("sent_topup_e8s={}", cmc_sample.sent_topup_e8s))
    {
        bail!(
            "expected public logs to include CMC burn/allocation sample {cmc_sample:?}, got {logs}"
        );
    }

    let ledger_subaccount = principal_to_subaccount(env.ledger);
    let cmc_subaccount = principal_to_subaccount(env.cmc);
    let transfers = env.transfers()?;
    let ledger_transfer = transfers
        .iter()
        .find(|transfer| {
            transfer.to.owner == env.cmc && transfer.to.subaccount == Some(ledger_subaccount)
        })
        .context("missing transfer to ledger canister CMC deposit account")?;
    let cmc_transfer = transfers
        .iter()
        .find(|transfer| {
            transfer.to.owner == env.cmc && transfer.to.subaccount == Some(cmc_subaccount)
        })
        .context("missing transfer to CMC canister CMC deposit account")?;
    if nat_to_u64(&cmc_transfer.amount) <= nat_to_u64(&ledger_transfer.amount) {
        bail!("expected higher-burn CMC transfer amount to exceed lower-burn ledger amount, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_canister_with_increased_cycles_gets_no_topup_when_others_burned() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(5_000_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 8_000_000_000_000)?;
    env.set_managed_cycles(12_000_000_000_000)?;
    let summary = env.tick_relay()?;
    let burned = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.ledger)
        .context("missing burned canister sample")?;
    let gained = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing gained canister sample")?;
    if gained.burn_cycles != 0 || gained.target_topup_cycles != 0 || gained.amount_e8s != 0 {
        bail!("expected gained canister to receive no top-up while another burned: {summary:?}");
    }
    if burned.burn_cycles == 0 {
        bail!("expected burned canister to report positive burn: {summary:?}");
    }
    let notifications = env.notifications()?;
    if notifications
        .iter()
        .any(|notification| notification.canister_id == env.cmc)
    {
        bail!("expected no notify for gained canister, got {notifications:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_splits_equally_when_no_canister_burned_cycles() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(99_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus {
        bail!("expected cycles top-up mode, got {summary:?}");
    }
    if summary
        .canisters
        .iter()
        .any(|sample| sample.burn_cycles != 0 || sample.target_topup_cycles != 0)
    {
        bail!("expected zero burn to plan no top-up, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn surplus_canister_transfer_uses_configured_memo_without_cmc_notify() -> Result<()> {
    require_ignored_flag()?;
    let external_memo = vec![0xA1, 0xB2];
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: cmc,
                memo: external_memo.clone(),
            }]),
            Vec::new(),
        )
    })?;
    env.set_managed_cycles(4_000_000_000_000)?;
    env.credit_relay(99_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick before surplus transfer, got {baseline:?}");
    }

    env.set_managed_cycles(2_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.cmc_notify_success_count == 0 {
        bail!("expected bootstrap top-up to establish conversion estimate, got {topup:?}");
    }

    env.credit_relay(99_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(4_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.ledger_transfer_count == 0 {
        bail!("expected surplus transfer after top-up phase, got {summary:?}");
    }

    let transfers = env.transfers()?;
    if !transfers.iter().any(|transfer| {
        transfer.to
            == (Account {
                owner: env.cmc,
                subaccount: None,
            })
            && transfer.memo == Some(external_memo.clone())
    }) {
        bail!(
            "expected surplus canister recipient transfer with configured memo, got {transfers:?}"
        );
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("RELAY_SURPLUS_TRANSFER ")
        || !logs.contains("memo_len=2")
    {
        bail!("expected public surplus recipient logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn surplus_recipient_memos_preserve_absent_and_binary_icrc1_forms() -> Result<()> {
    require_ignored_flag()?;
    let empty_neuron_id = 42_u64;
    let binary_neuron_id = 43_u64;
    let arbitrary_canister_memo = vec![0xa1, 0xb2];
    let arbitrary_neuron_memo = vec![0x00, 0xff, 0x80];
    let env = RelayEnv::new_with_config(None, |ledger, cmc, blackhole, _relay| {
        (
            vec![cmc],
            Some(vec![
                SurplusCanisterRecipient {
                    canister_id: cmc,
                    memo: Vec::new(),
                },
                SurplusCanisterRecipient {
                    canister_id: ledger,
                    memo: vec![0],
                },
                SurplusCanisterRecipient {
                    canister_id: blackhole,
                    memo: arbitrary_canister_memo.clone(),
                },
            ]),
            vec![
                SurplusNeuronRecipient {
                    neuron_id: empty_neuron_id,
                    memo: Vec::new(),
                },
                SurplusNeuronRecipient {
                    neuron_id: binary_neuron_id,
                    memo: arbitrary_neuron_memo.clone(),
                },
            ],
        )
    })?;
    env.set_managed_cycles(4_000_000_000_000)?;
    env.credit_relay(99_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick before surplus transfers, got {baseline:?}");
    }

    env.set_managed_cycles(2_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.cmc_notify_success_count == 0 {
        bail!("expected bootstrap top-up to establish conversion estimate, got {topup:?}");
    }

    env.credit_relay(5_000_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(4_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.surplus_transfers.len() != 5 {
        bail!("expected five surplus recipient transfers, got {summary:?}");
    }

    let transfers = env.transfers()?;
    let memo_to = |account: Account| {
        transfers
            .iter()
            .find(|transfer| transfer.to == account)
            .map(|transfer| transfer.memo.clone())
    };
    if memo_to(Account {
        owner: env.cmc,
        subaccount: None,
    }) != Some(None)
    {
        bail!("expected empty Principal recipient memo to be absent, got {transfers:?}");
    }
    if memo_to(Account {
        owner: env.governance,
        subaccount: Some(neuron_subaccount(empty_neuron_id)),
    }) != Some(None)
    {
        bail!("expected empty neuron recipient memo to be absent, got {transfers:?}");
    }
    if memo_to(Account {
        owner: env.ledger,
        subaccount: None,
    }) != Some(Some(vec![0]))
    {
        bail!("expected [0x00] Principal recipient memo to remain present, got {transfers:?}");
    }
    if memo_to(Account {
        owner: env.blackhole,
        subaccount: None,
    }) != Some(Some(arbitrary_canister_memo))
    {
        bail!("expected arbitrary Principal recipient memo bytes, got {transfers:?}");
    }
    if memo_to(Account {
        owner: env.governance,
        subaccount: Some(neuron_subaccount(binary_neuron_id)),
    }) != Some(Some(arbitrary_neuron_memo))
    {
        bail!("expected arbitrary neuron recipient memo bytes, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn recovery_deficit_carries_underfunded_topup_and_blocks_surplus_until_recovered() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: cmc,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(10_000_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick, got {baseline:?}");
    }

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(0)?;
    env.credit_relay(60_010_000)?;
    let underfunded = env.tick_relay()?;
    let underfunded_sample = underfunded
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing underfunded CMC burn sample")?;
    if underfunded.mode != RelayMode::TopUpThenSurplus
        || underfunded_sample.target_topup_cycles == 0
        || underfunded_sample.actual_minted_cycles >= underfunded_sample.target_topup_cycles
        || underfunded_sample.remaining_deficit_cycles == 0
        || !underfunded.surplus_transfers.is_empty()
    {
        bail!(
            "expected underfunded CMC top-up with retained recovery deficit, got {underfunded:?}"
        );
    }
    let carried_deficit = underfunded_sample.remaining_deficit_cycles;
    let logs_after_underfunded = env.logs_text()?;
    if !logs_after_underfunded.contains(&format!("remaining_deficit_cycles={carried_deficit}")) {
        bail!(
            "expected RELAY_CANISTER log to expose remaining deficit, got {logs_after_underfunded}"
        );
    }

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_250_000_000_000)?;
    env.credit_relay(300_000_000)?;
    let recovered = env.tick_relay()?;
    let recovered_sample = recovered
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing recovered CMC burn sample")?;
    if recovered_sample.carried_deficit_cycles != carried_deficit
        || recovered_sample.target_topup_cycles != carried_deficit
        || recovered_sample.remaining_deficit_cycles != 0
        || recovered_sample.actual_minted_cycles < recovered_sample.target_topup_cycles
    {
        bail!(
            "expected next tick to carry and clear previous deficit without headroom, got {recovered:?}"
        );
    }
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(9_000_000_000_000)?;
    env.credit_relay(200_000_000)?;
    let mut surplus = env.tick_relay()?;
    for _ in 0..2 {
        if !surplus.surplus_transfers.is_empty() || !env.debug_state()?.active_job_present {
            break;
        }
        surplus = env.tick_relay()?;
    }
    let surplus_sample = surplus
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing post-recovery CMC burn sample")?;
    if surplus_sample.carried_deficit_cycles != 0 || surplus_sample.remaining_deficit_cycles != 0 {
        bail!("expected recovery deficit to stay cleared on next clean tick, got {surplus:?}");
    }
    if surplus.surplus_transfers.iter().all(|transfer| {
        transfer.target != SurplusTarget::Canister(env.cmc) || transfer.amount_e8s == 0
    }) {
        bail!("expected raw surplus to route after recovery deficit cleared, got {surplus:?}");
    }
    let transfers = env.transfers()?;
    if transfers.iter().all(|transfer| {
        transfer.to
            != (Account {
                owner: env.cmc,
                subaccount: None,
            })
    }) {
        bail!("expected surplus transfer to CMC account after deficit recovery, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn surplus_neuron_transfers_are_suppressed_below_one_icp_each() -> Result<()> {
    require_ignored_flag()?;
    let io_neuron = 10_292_412_127_977_304_661_u64;
    let jupiter_faucet_neuron = 11_614_578_985_374_291_210_u64;
    let io_memo = b"10292412127977304661".to_vec();
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            None,
            vec![
                SurplusNeuronRecipient {
                    neuron_id: io_neuron,
                    memo: Vec::new(),
                },
                SurplusNeuronRecipient {
                    neuron_id: jupiter_faucet_neuron,
                    memo: io_memo.clone(),
                },
            ],
        )
    })?;
    env.set_managed_cycles(4_000_000_000_000)?;
    env.credit_relay(99_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick before surplus transfer, got {baseline:?}");
    }

    env.set_managed_cycles(2_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.cmc_notify_success_count == 0 {
        bail!("expected bootstrap top-up to establish conversion estimate, got {topup:?}");
    }

    env.credit_relay(99_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(4_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus
        || summary.ledger_transfer_count != summary.cmc_notify_success_count
    {
        bail!("expected only CMC top-up transfers below raw ICP threshold, got {summary:?}");
    }
    if summary.skipped_surplus_reason.as_deref() != Some("raw_icp_share_below_1_icp") {
        bail!("expected raw ICP threshold skip reason, got {summary:?}");
    }
    if !summary.surplus_transfers.iter().all(|transfer| {
        transfer.amount_e8s == 0
            && transfer.skipped_reason.as_deref() == Some("raw_icp_share_below_1_icp")
    }) {
        bail!("expected all surplus neuron transfers suppressed below threshold, got {summary:?}");
    }

    let transfers = env.transfers()?;
    if transfers.iter().any(|transfer| {
        transfer.to
            == (Account {
                owner: env.governance,
                subaccount: Some(neuron_subaccount(io_neuron)),
            })
            || transfer.to
                == (Account {
                    owner: env.governance,
                    subaccount: Some(neuron_subaccount(jupiter_faucet_neuron)),
                })
    }) {
        bail!("expected no raw ICP neuron surplus transfers below threshold, got {transfers:?}");
    }
    if !summary
        .surplus_transfers
        .iter()
        .any(|transfer| transfer.memo_len == Some(io_memo.len() as u32))
    {
        bail!("expected suppressed Jupiter Faucet memo metadata to be preserved, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_retains_funds_when_cycles_are_unchanged_and_conversion_is_missing() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: relay,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.set_managed_cycles(5_000_000_000_000)?;
    env.credit_relay(5_000_000_000)?;
    let _ = env.tick_relay()?;

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.ledger_transfer_count != 0 {
        bail!("expected unchanged cycles and missing conversion to retain funds, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_recomputes_topups_each_tick_after_prior_no_topup_tick() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: relay,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.set_managed_cycles(6_000_000_000_000)?;
    env.add_relay_cycles(2_000_000_000_000);
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_managed_cycles(6_000_000_000_000)?;
    let raw = env.tick_relay()?;
    if raw.mode != RelayMode::TopUpThenSurplus {
        bail!("expected unchanged cycles to avoid top-up, got {raw:?}");
    }

    env.credit_relay(100_000_000)?;
    env.set_managed_cycles(4_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.mode != RelayMode::TopUpThenSurplus || topup.cmc_notify_success_count == 0 {
        bail!("expected later burn tick to perform CMC top-up, got {topup:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn fail_closed_blackhole_probe_failure_spends_nothing() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.credit_relay(100_000_000)?;

    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::Degraded
        || summary.probe_failures.is_empty()
        || summary.ledger_transfer_count != 0
    {
        bail!("expected degraded no-spend summary after missing blackhole status, got {summary:?}");
    }
    let transfers: Vec<TransferRecord> = query_one(
        &env.pic,
        env.ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    if !transfers.is_empty() {
        bail!("expected no ledger transfers when blackhole probe fails, got {transfers:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=Degraded") || !logs.contains("RELAY_PROBE_FAILURE ") {
        bail!("expected public degraded probe failure logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_tick_succeeds_when_both_production_blackholes_are_managed() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_production_blackholes_managed()?;

    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::BaselineOnly || !summary.probe_failures.is_empty() {
        bail!("expected complete baseline tick with both managed blackholes, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn cmc_processing_is_retried_without_double_spending_ledger() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_cmc_script(vec![
        DebugNotifyBehavior::Processing,
        DebugNotifyBehavior::Ok,
    ])?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 1
        || summary.cmc_notify_success_count != 1
        || summary.cmc_notify_ambiguous_count != 0
    {
        bail!("expected one ledger transfer and successful retry after CMC Processing, got {summary:?}");
    }
    let transfers: Vec<TransferRecord> = query_one(
        &env.pic,
        env.ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    if transfers.len() != 1 {
        bail!("expected CMC Processing retry not to duplicate ledger transfers, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_marks_cmc_repeated_retryable_notify_as_ambiguous() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_cmc_script(vec![
        DebugNotifyBehavior::Processing,
        DebugNotifyBehavior::Other {
            error_code: 1,
            error_message: "still processing".to_string(),
        },
    ])?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 1
        || summary.cmc_notify_ambiguous_count != 1
        || summary.ambiguous_e8s == 0
    {
        bail!("expected accepted ledger spend with ambiguous repeated CMC uncertainty, got {summary:?}");
    }
    if env.transfers()?.len() != 1 {
        bail!("expected no changed-identity retry after CMC ambiguity");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_treats_ledger_duplicate_as_accepted_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_ledger_error_script(vec![DebugNextTransferError::Duplicate { duplicate_of: 77 }])?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 1
        || summary.failed_transfers != 0
        || summary.cmc_notify_success_count != 1
    {
        bail!("expected duplicate ledger response to count as accepted transfer, got {summary:?}");
    }
    let notifications = env.notifications()?;
    if notifications.len() != 1 || notifications[0].block_index != 77 {
        bail!("expected CMC notify with duplicate block index, got {notifications:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_respects_max_transfers_per_tick_and_resumes_active_job() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(Some(1), |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(5_000_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 8_000_000_000_000)?;
    env.set_managed_cycles(8_000_000_000_000)?;
    let _ = env.tick_relay()?;
    if env.transfers()?.len() != 1 || !env.debug_state()?.active_job_present {
        bail!("expected first allocation tick to start one transfer and keep active job");
    }

    let mut previous_transfer_count = env.transfers()?.len();
    let mut summary = env.summary()?;
    for _ in 0..5 {
        summary = env.tick_relay()?;
        let current_transfer_count = env.transfers()?.len();
        if current_transfer_count.saturating_sub(previous_transfer_count) > 1 {
            bail!("expected at most one new transfer per tick with limit=1, got previous={previous_transfer_count} current={current_transfer_count}");
        }
        previous_transfer_count = current_transfer_count;
        if !env.debug_state()?.active_job_present {
            break;
        }
    }
    if summary.mode != RelayMode::TopUpThenSurplus
        || summary.partial_tick_count == 0
        || summary.ledger_transfer_count < 2
        || env.debug_state()?.active_job_present
    {
        bail!("expected later tick to resume and complete transfer-limited job, got {summary:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("partial_tick_count=")
    {
        bail!("expected transfer-limit public summary logs with partial_tick_count, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_full_init_args_upgrade_reinitializes_config_and_resets_operational_heap_state(
) -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: cmc,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(10_000_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline before upgrade setup, got {baseline:?}");
    }

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(0)?;
    env.credit_relay(60_010_000)?;
    let underfunded = env.tick_relay()?;
    let underfunded_sample = underfunded
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing underfunded CMC sample before upgrade")?;
    if underfunded_sample.remaining_deficit_cycles == 0
        || underfunded.total_remaining_deficit_cycles == 0
    {
        bail!("expected pre-upgrade runtime accounting to contain a recovery deficit, got {underfunded:?}");
    }

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(0)?;
    env.credit_relay(100_000_000)?;
    env.abort_after_successful_transfer()?;
    let _ = env.tick_relay()?;
    let before = env.debug_state()?;
    if !before.active_job_present
        || !before.active_job_pending_transfer_present
        || !before.last_summary_present
        || before.last_completed_cycles_count == 0
        || before.recovery_deficit_cycles_count == 0
        || !before.conversion_estimate_present
    {
        bail!("expected pre-upgrade heap accounting, deficit, conversion estimate, and pending transfer, got {before:?}");
    }

    let replacement_args = RelayInitArg {
        main_interval_seconds: Some(120),
        max_transfers_per_tick: Some(7),
        surplus_canister_recipients: None,
        surplus_neuron_recipients: vec![SurplusNeuronRecipient {
            neuron_id: 99,
            memo: b"replacement".to_vec(),
        }],
        ..env.default_init_arg()
    };
    env.upgrade_relay_with_init_args(replacement_args)?;
    let config_after = env.debug_config()?;
    if config_after.main_interval_seconds != 120
        || config_after.max_transfers_per_tick != Some(7)
        || config_after.surplus_canister_recipients.is_some()
        || config_after.surplus_neuron_recipients
            != vec![SurplusNeuronRecipient {
                neuron_id: 99,
                memo: b"replacement".to_vec(),
            }]
    {
        bail!("expected full InitArgs upgrade to replace relay config, got {config_after:?}");
    }
    let after = env.debug_state()?;
    if after.active_job_present
        || after.active_job_pending_transfer_present
        || after.active_faucet_commitment_transfer_present
        || after.last_summary_present
        || after.last_completed_cycles_count != 0
        || after.relay_minted_cycles_since_sample_count != 0
        || after.recovery_deficit_cycles_count != 0
        || after.conversion_estimate_present
        || after.next_job_id != 1
    {
        bail!(
            "expected full InitArgs upgrade to reset relay operational heap state, got {after:?}"
        );
    }

    env.set_managed_cycles(10_000_000_000_000)?;
    env.advance_time_and_tick(1, 5);
    let first_after_upgrade = env.summary()?;
    if first_after_upgrade.mode != RelayMode::BaselineOnly
        || first_after_upgrade.ledger_transfer_count != 0
        || first_after_upgrade.total_remaining_deficit_cycles != 0
    {
        bail!("expected first tick after full InitArgs upgrade to establish a fresh baseline, got {first_after_upgrade:?}");
    }

    env.credit_relay(100_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    let unchanged_after_baseline = env.tick_relay()?;
    if unchanged_after_baseline.cmc_notify_success_count != 0
        || unchanged_after_baseline.total_carried_deficit_cycles != 0
        || unchanged_after_baseline.total_remaining_deficit_cycles != 0
    {
        bail!("expected prior recovery deficit and samples to be cleared after upgrade, got {unchanged_after_baseline:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn relay_no_arg_upgrade_is_rejected() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.advance_time_and_tick(5 * 60, 5);
    let result = env.try_upgrade_relay_without_args()?;
    let Err(err) = result else {
        bail!("expected no-arg Relay upgrade to be rejected");
    };
    if !err.contains("Canister called `ic0.trap`") && !err.contains("failed to decode") {
        bail!("expected Candid decode rejection for no-arg Relay upgrade, got {err}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_reinstall_starts_without_summary_or_active_job() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    let first = env.tick_relay()?;
    if first.mode != RelayMode::BaselineOnly {
        bail!("expected initial baseline summary before reinstall, got {first:?}");
    }

    env.reinstall_relay_with_default_config()?;
    let st = env.debug_state()?;
    if st.active_job_present || st.last_summary_present {
        bail!("expected fresh relay heap after reinstall, got {st:?}");
    }

    env.advance_time_and_tick(1, 5);
    let summary = env.summary()?;
    if summary.mode != RelayMode::BaselineOnly {
        bail!("expected first tick after reinstall to establish a fresh baseline, got {summary:?}");
    }
    Ok(())
}

fn real_icp_transfer(
    pic: &PocketIc,
    ledger: Principal,
    caller: Principal,
    to: Account,
    amount_e8s: u64,
) -> Result<u64> {
    support::ledger::icrc1_transfer(
        pic,
        ledger,
        caller,
        TransferArg {
            from_subaccount: None,
            to,
            fee: Some(Nat::from(support::ledger::ICP_LEDGER_FEE_E8S)),
            created_at_time: None,
            memo: None,
            amount: Nat::from(amount_e8s),
        },
    )
}

fn write_reward_state_fixture(pic: &PocketIc, relay: Principal, state: RewardStateFixture) {
    let fixture_memory = VectorMemory::default();
    let mut fixture_cell = StableCell::<VersionedRewardStateFixture, _>::init(
        fixture_memory.clone(),
        VersionedRewardStateFixture::Uninitialized,
    );
    fixture_cell.set(VersionedRewardStateFixture::V1(state));
    drop(fixture_cell);
    let mut fixture_bytes = vec![0; fixture_memory.size() as usize * 65_536];
    fixture_memory.read(0, &mut fixture_bytes);

    let stable_memory = Rc::new(RefCell::new(pic.get_stable_memory(relay)));
    let manager = MemoryManager::init(stable_memory.clone());
    let reward_memory = manager.get(MemoryId::new(0));
    if reward_memory.size() < fixture_memory.size() {
        reward_memory.grow(fixture_memory.size() - reward_memory.size());
    }
    reward_memory.write(0, &fixture_bytes);
    drop(reward_memory);
    drop(manager);
    let bytes = stable_memory.borrow().clone();
    pic.set_stable_memory(
        relay,
        bytes,
        pocket_ic::common::rest::BlobCompression::NoCompression,
    );
}

fn read_splitter_state_fixture(pic: &PocketIc, relay: Principal) -> VersionedSplitterStateFixture {
    let stable_memory = Rc::new(RefCell::new(pic.get_stable_memory(relay)));
    let manager = MemoryManager::init(stable_memory);
    StableCell::<VersionedSplitterStateFixture, _>::init(
        manager.get(MemoryId::new(1)),
        VersionedSplitterStateFixture::Uninitialized,
    )
    .get()
    .clone()
}

fn wait_for_real_index_transactions(
    pic: &PocketIc,
    index: Principal,
    account_identifier: &str,
    minimum: usize,
) -> Result<()> {
    for _ in 0..60 {
        let result: GetAccountIdentifierTransactionsResult = query_one(
            pic,
            index,
            Principal::anonymous(),
            "get_account_identifier_transactions",
            GetAccountIdentifierTransactionsArgs {
                max_results: 1_000,
                start: None,
                account_identifier: account_identifier.to_string(),
            },
        )?;
        if matches!(result, GetAccountIdentifierTransactionsResult::Ok(ref page) if page.transactions.len() >= minimum)
        {
            return Ok(());
        }
        pic.advance_time(Duration::from_secs(1));
        tick_n(pic, 5);
    }
    bail!("real ICP Index did not expose {minimum} expected account transactions")
}

struct RealSplitterRewardEnv {
    pic: PocketIc,
    icp_ledger: Principal,
    icp_index: Principal,
    root: Principal,
    reward_ledger: Principal,
    reward_index: Principal,
    sns_rewards: Principal,
    relay: Principal,
    relay_init: RewardRelayInitArg,
    owners: Vec<Principal>,
    rewards_installed: bool,
}

impl RealSplitterRewardEnv {
    fn new(owner_count: usize) -> Result<Self> {
        Self::new_with_relay_wasm(owner_count, relay_wasm()?)
    }

    fn new_with_relay_wasm(owner_count: usize, relay_wasm: Vec<u8>) -> Result<Self> {
        let pic = support::ledger::build_pic_with_real_icp();
        let icp_ledger = support::principals::icp_ledger();
        let icp_index = support::principals::icp_index();
        let root = pic.create_canister();
        let sns_governance = pic.create_canister();
        let reward_ledger = pic.create_canister();
        let reward_index = pic.create_canister();
        let sns_rewards = pic.create_canister();
        let cmc = pic.create_canister();
        let nns_governance = pic.create_canister();
        let blackhole = pic.create_canister();
        let relay = pic.create_canister();
        for canister in [
            root,
            sns_governance,
            reward_ledger,
            reward_index,
            sns_rewards,
            cmc,
            nns_governance,
            blackhole,
            relay,
        ] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(root, sns_root_wasm()?, vec![], None);
        pic.install_canister(sns_governance, sns_governance_wasm()?, vec![], None);
        pic.install_canister(reward_ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(reward_index, ledger_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(nns_governance, governance_wasm()?, vec![], None);
        pic.install_canister(blackhole, blackhole_wasm()?, vec![], None);
        let _: () = update_one(
            &pic,
            root,
            Principal::anonymous(),
            "debug_set_canisters",
            ListSnsCanistersResponse {
                root: Some(root),
                governance: Some(sns_governance),
                ledger: Some(reward_ledger),
                swap: None,
                index: Some(reward_index),
                dapps: vec![],
                archives: vec![],
                extensions: None,
            },
        )?;
        let owners = (0..owner_count)
            .map(|index| Principal::self_authenticating([(index + 41) as u8; 32]))
            .collect::<Vec<_>>();
        let neurons = owners
            .iter()
            .enumerate()
            .map(|(index, owner)| SnsNeuron {
                id: Some(SnsNeuronId {
                    id: vec![(index + 1) as u8; 32],
                }),
                permissions: vec![SnsNeuronPermission {
                    principal: Some(*owner),
                    permission_type: vec![1, 2, 3],
                }],
                cached_neuron_stake_e8s: 100_000_000,
                neuron_fees_e8s: 0,
            })
            .collect::<Vec<_>>();
        let _: () = update_one(
            &pic,
            sns_governance,
            Principal::anonymous(),
            "debug_set_neurons",
            neurons,
        )?;
        let relay_init = RewardRelayInitArg {
            managed_canisters: vec![],
            ledger_canister_id: Some(icp_ledger),
            cmc_canister_id: Some(cmc),
            governance_canister_id: Some(nns_governance),
            blackhole_canister_id: Some(blackhole),
            sns_rewards_canister_id: Some(sns_rewards),
            icp_index_canister_id: Some(icp_index),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick: None,
            surplus_canister_recipients: None,
            surplus_neuron_recipients: vec![],
        };
        pic.install_canister(relay, relay_wasm, encode_one(relay_init.clone())?, None);
        let _: () = update_one(
            &pic,
            reward_ledger,
            Principal::anonymous(),
            "debug_set_fee",
            1_000_u64,
        )?;
        let _: () = update_one(
            &pic,
            reward_index,
            Principal::anonymous(),
            "debug_set_index_source",
            reward_ledger,
        )?;
        Ok(Self {
            pic,
            icp_ledger,
            icp_index,
            root,
            reward_ledger,
            reward_index,
            sns_rewards,
            relay,
            relay_init,
            owners,
            rewards_installed: false,
        })
    }

    fn fund_owner(&self, owner: Principal, amount_e8s: u64) -> Result<()> {
        real_icp_transfer(
            &self.pic,
            self.icp_ledger,
            Principal::anonymous(),
            Account {
                owner,
                subaccount: None,
            },
            amount_e8s,
        )?;
        Ok(())
    }

    fn send_to_relay(
        &self,
        owner: Principal,
        subaccount: [u8; 32],
        amount_e8s: u64,
    ) -> Result<u64> {
        real_icp_transfer(
            &self.pic,
            self.icp_ledger,
            owner,
            Account {
                owner: self.relay,
                subaccount: Some(subaccount),
            },
            amount_e8s,
        )
    }

    fn main_tick(&self) -> Result<()> {
        update_noargs(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_main_tick",
        )
    }

    fn refresh_snapshot(&mut self) -> Result<()> {
        self.pic.advance_time(Duration::from_secs(1));
        tick_n(&self.pic, 5);
        if !self.rewards_installed {
            self.pic.install_canister(
                self.sns_rewards,
                sns_rewards_wasm()?,
                encode_one(SnsRewardsInitArgs {
                    reward_sns_root_canister_id: Some(self.root),
                })?,
                None,
            );
            self.rewards_installed = true;
        }
        update_noargs(
            &self.pic,
            self.sns_rewards,
            Principal::anonymous(),
            "debug_scan_tick",
        )
    }

    fn credit_reward_pot(&self, amount_e8s: u64) -> Result<()> {
        update_bytes(
            &self.pic,
            self.reward_ledger,
            Principal::anonymous(),
            "debug_credit",
            encode_args((
                Account {
                    owner: self.relay,
                    subaccount: None,
                },
                amount_e8s,
            ))?,
        )
    }

    fn set_reward_index_lag(&self, hidden_newest_transactions: u64) -> Result<()> {
        update_one(
            &self.pic,
            self.reward_index,
            Principal::anonymous(),
            "debug_set_index_hidden_newest_transactions",
            hidden_newest_transactions,
        )
    }

    fn set_reward_components(
        &self,
        root: Option<Principal>,
        ledger: Option<Principal>,
        index: Option<Principal>,
    ) -> Result<()> {
        update_one(
            &self.pic,
            self.root,
            Principal::anonymous(),
            "debug_set_canisters",
            ListSnsCanistersResponse {
                root,
                governance: None,
                ledger,
                swap: None,
                index,
                dapps: vec![],
                archives: vec![],
                extensions: None,
            },
        )
    }

    fn reward_sweep(&self) -> Result<()> {
        update_noargs(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_reward_sweep",
        )
    }

    fn wait_for_histories(&self, splitter: u8, subaccount_one_minimum: usize) -> Result<()> {
        wait_for_real_index_transactions(
            &self.pic,
            self.icp_index,
            &account_identifier_text(self.relay, Some(relay_subaccount_one())),
            subaccount_one_minimum,
        )?;
        wait_for_real_index_transactions(
            &self.pic,
            self.icp_index,
            &account_identifier_text(self.relay, Some(relay_numbered_subaccount(splitter))),
            3,
        )
    }

    fn index_transactions(
        &self,
        account_identifier: String,
    ) -> Result<Vec<jupiter_ic_clients::index::IndexTransactionWithId>> {
        let result: GetAccountIdentifierTransactionsResult = query_one(
            &self.pic,
            self.icp_index,
            Principal::anonymous(),
            "get_account_identifier_transactions",
            GetAccountIdentifierTransactionsArgs {
                max_results: 1_000,
                start: None,
                account_identifier,
            },
        )?;
        match result {
            GetAccountIdentifierTransactionsResult::Ok(page) => Ok(page.transactions),
            GetAccountIdentifierTransactionsResult::Err(error) => {
                bail!("real ICP Index history query failed: {error:?}")
            }
        }
    }

    fn journal(&self) -> Result<RewardJournalView> {
        query_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_reward_state",
            (),
        )
    }

    fn reward_balance(&self, owner: Principal) -> Result<u64> {
        support::ledger::icrc1_balance(
            &self.pic,
            self.reward_ledger,
            &Account {
                owner,
                subaccount: None,
            },
        )
    }

    fn reward_index_transactions(
        &self,
    ) -> Result<jupiter_ic_clients::icrc_index::GetAccountTransactionsResponse> {
        let result: IcrcGetAccountTransactionsResult = update_one(
            &self.pic,
            self.reward_index,
            Principal::anonymous(),
            "get_account_transactions",
            IcrcGetAccountTransactionsArgs {
                account: Account {
                    owner: self.relay,
                    subaccount: None,
                },
                start: None,
                max_results: Nat::from(1_000_u64),
            },
        )?;
        result.map_err(|error| anyhow::anyhow!(error.message))
    }
}

#[test]
#[ignore]
fn reward_context_failure_retries_on_next_daily_main_tick() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(0)?;
    tick_n(&env.pic, 30);
    assert_eq!(env.journal()?.last_sweep_attempt_timestamp_seconds, 0);

    env.main_tick()?;
    assert_eq!(
        env.journal()?.last_sweep_attempt_timestamp_seconds,
        0,
        "an unavailable reward context must not consume the weekly cadence"
    );

    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000)?;
    env.pic.advance_time(Duration::from_secs(24 * 60 * 60));
    tick_n(&env.pic, 5);
    env.main_tick()?;
    if env.journal()?.last_sweep_attempt_timestamp_seconds == 0 {
        bail!("the next daily tick did not complete the uneconomical-pot adjudication");
    }
    assert_eq!(env.reward_balance(env.relay)?, 1_000);
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|err| anyhow::anyhow!("fetch Relay logs failed: {err:?}"))?;
    if !logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD status=held")
            && line.contains("reason=balance_not_above_plan_fees")
    }) {
        let reward_logs = logs
            .iter()
            .filter_map(|entry| {
                let line = String::from_utf8_lossy(&entry.content);
                line.contains("RELAY_SNS_REWARD").then(|| line.into_owned())
            })
            .collect::<Vec<_>>();
        bail!("missing failed-then-completed daily reward cadence evidence: {reward_logs:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn rejected_reward_transfer_retries_on_next_daily_main_tick() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(1)?;
    tick_n(&env.pic, 30);
    let owner = env.owners[0];
    env.fund_owner(owner, 200_000_000)?;
    env.send_to_relay(owner, relay_subaccount_one(), 100_010_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        2,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000_000)?;
    let _: () = update_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_set_error_script",
        vec![DebugNextTransferError::BadFee {
            expected_fee_e8s: 2_000,
        }],
    )?;

    env.reward_sweep()?;
    let rejected = env.journal()?;
    if rejected.last_sweep_attempt_timestamp_seconds != 0 || rejected.pending_payout.is_some() {
        bail!(
            "definitive reward rejection consumed cadence or retained a fresh plan: {rejected:?}"
        );
    }
    assert_eq!(env.reward_balance(owner)?, 0);

    env.pic.advance_time(Duration::from_secs(24 * 60 * 60));
    tick_n(&env.pic, 5);
    env.main_tick()?;
    let accepted = env.journal()?;
    if accepted.last_sweep_attempt_timestamp_seconds == 0 || accepted.pending_payout.is_some() {
        bail!("next daily tick did not retry and accept the rejected plan: {accepted:?}");
    }
    assert_eq!(env.reward_balance(owner)?, 999_000);
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|err| anyhow::anyhow!("fetch Relay logs failed: {err:?}"))?;
    if !logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD status=failed") && line.contains("reason=bad_fee")
    }) || logs
        .iter()
        .any(|entry| String::from_utf8_lossy(&entry.content).contains("RELAY_SNS_REWARD_TRANSFER"))
    {
        bail!("reward rejection logging was not consolidated");
    }
    Ok(())
}

#[test]
#[ignore]
fn stateless_reward_lookback_is_pro_rata_reusable_and_skips_newer_ineligible_commitment(
) -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(2)?;
    tick_n(&env.pic, 30);
    let alice = env.owners[0];
    let bob = env.owners[1];
    let unknown = Principal::self_authenticating([99; 32]);
    for (owner, funding) in [
        (alice, 500_000_000),
        (bob, 900_000_000),
        (unknown, 9_300_000_000),
    ] {
        env.fund_owner(owner, funding)?;
    }
    for (owner, amount) in [
        (alice, 400_000_000),
        (bob, 600_000_000),
        (unknown, 9_000_000_000),
    ] {
        env.send_to_relay(owner, relay_subaccount_one(), amount)?;
    }
    env.main_tick()?;
    let relay_history = account_identifier_text(env.relay, Some(relay_subaccount_one()));
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 4)?;
    env.refresh_snapshot()?;

    let logs_before = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|err| anyhow::anyhow!("fetch Relay logs failed: {err:?}"))?
        .len();
    env.credit_reward_pot(1_002_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 399_800);
    assert_eq!(env.reward_balance(bob)?, 600_200);
    assert_eq!(env.reward_balance(unknown)?, 0);
    assert_eq!(env.reward_balance(env.relay)?, 0);
    let first = env.journal()?;
    if first.pending_payout.is_some() || first.last_sweep_attempt_timestamp_seconds == 0 {
        bail!("completed pro-rata plan was not cleared and recorded: {first:?}");
    }

    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|err| anyhow::anyhow!("fetch Relay logs failed: {err:?}"))?;
    let reward_lines = logs
        .iter()
        .skip(logs_before)
        .filter_map(|entry| {
            let line = String::from_utf8_lossy(&entry.content);
            line.contains("RELAY_SNS_REWARD ")
                .then(|| line.into_owned())
        })
        .collect::<Vec<_>>();
    let [reward_line] = reward_lines.as_slice() else {
        bail!(
            "one adjudication emitted {} reward summaries",
            reward_lines.len()
        );
    };
    if !reward_line.contains("status=accepted")
        || !reward_line.contains("eligible_principals=2")
        || !reward_line.contains("eligible_icp_e8s=1000000000")
        || !reward_line.contains("ineligible_icp_e8s=9000000000")
        || !reward_line.contains("recipient_count=2")
    {
        bail!("compact reward summary lost pro-rata attribution: {reward_line}");
    }
    for removed in [
        "recipient=",
        "processed_from_commitment_tx_id=",
        "splitter_credits=",
        "expanded_distinct_sources=",
        "token_amount=",
    ] {
        if reward_line.contains(removed) {
            bail!("reward summary retained obsolete field {removed}: {reward_line}");
        }
    }
    if logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD_OWNER_MISMATCH")
            || line.contains("RELAY_SNS_REWARD_TRANSFER")
    }) {
        bail!("reward execution emitted a removed per-source or transfer log");
    }

    // No new ICP commitment: the same completed commitment remains the attribution target.
    env.credit_reward_pot(2_002_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 1_199_600);
    assert_eq!(env.reward_balance(bob)?, 1_800_400);

    // A newer eligible commitment supersedes the old one.
    env.send_to_relay(bob, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 6)?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(501_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(bob)?, 2_300_400);

    // A still newer ineligible-only completed commitment is skipped, so Bob's preceding
    // qualifying commitment receives the next accrual again.
    env.send_to_relay(unknown, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 8)?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(501_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(bob)?, 2_800_400);
    assert_eq!(env.reward_balance(unknown)?, 0);
    Ok(())
}

#[test]
#[ignore]
fn reward_arrival_before_newer_commitment_fences_that_funder_out() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(2)?;
    tick_n(&env.pic, 30);
    let alice = env.owners[0];
    let bob = env.owners[1];
    for owner in [alice, bob] {
        env.fund_owner(owner, 300_000_000)?;
    }

    env.send_to_relay(alice, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    let relay_history = account_identifier_text(env.relay, Some(relay_subaccount_one()));
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 2)?;

    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.credit_reward_pot(101_000)?;
    let reward_arrival = env
        .reward_index_transactions()?
        .transactions
        .into_iter()
        .find_map(|entry| entry.transaction.mint.map(|_| entry.transaction.timestamp))
        .context("reward Index did not expose the reward credit")?;

    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.send_to_relay(bob, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 4)?;
    env.refresh_snapshot()?;

    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 100_000);
    assert_eq!(env.reward_balance(bob)?, 0);
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|error| anyhow::anyhow!("fetch Relay logs failed: {error:?}"))?;
    if !logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD status=accepted")
            && line.contains(&format!("attribution_cutoff_ts_nanos={reward_arrival}"))
    }) {
        bail!("reward arrival was not used as the effective attribution cutoff");
    }
    Ok(())
}

#[test]
#[ignore]
fn reward_arrival_after_newer_commitment_permits_that_funder() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(2)?;
    tick_n(&env.pic, 30);
    let alice = env.owners[0];
    let bob = env.owners[1];
    for owner in [alice, bob] {
        env.fund_owner(owner, 300_000_000)?;
    }
    let relay_history = account_identifier_text(env.relay, Some(relay_subaccount_one()));

    env.send_to_relay(alice, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 2)?;
    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.send_to_relay(bob, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 4)?;

    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.credit_reward_pot(101_000)?;
    env.refresh_snapshot()?;
    env.reward_sweep()?;

    assert_eq!(env.reward_balance(alice)?, 0);
    assert_eq!(env.reward_balance(bob)?, 100_000);
    Ok(())
}

#[test]
#[ignore]
fn mixed_reward_pot_uses_oldest_credit_then_later_epoch_can_select_bob() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(2)?;
    tick_n(&env.pic, 30);
    let alice = env.owners[0];
    let bob = env.owners[1];
    for owner in [alice, bob] {
        env.fund_owner(owner, 300_000_000)?;
    }
    let relay_history = account_identifier_text(env.relay, Some(relay_subaccount_one()));

    env.send_to_relay(alice, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 2)?;
    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.credit_reward_pot(101_000)?;
    let oldest_reward_arrival = env
        .reward_index_transactions()?
        .transactions
        .into_iter()
        .find_map(|entry| entry.transaction.mint.map(|_| entry.transaction.timestamp))
        .context("reward Index did not expose the oldest reward credit")?;

    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.send_to_relay(bob, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 4)?;
    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.credit_reward_pot(51_000)?;
    env.refresh_snapshot()?;

    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 151_000);
    assert_eq!(env.reward_balance(bob)?, 0);
    assert_eq!(env.reward_balance(env.relay)?, 0);
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|error| anyhow::anyhow!("fetch Relay logs failed: {error:?}"))?;
    if !logs.iter().any(|entry| {
        String::from_utf8_lossy(&entry.content).contains(&format!(
            "attribution_cutoff_ts_nanos={oldest_reward_arrival}"
        ))
    }) {
        bail!("mixed pot did not use its oldest unspent reward credit");
    }

    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.credit_reward_pot(51_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 151_000);
    assert_eq!(env.reward_balance(bob)?, 50_000);
    Ok(())
}

#[test]
#[ignore]
fn reward_index_lag_retries_without_consuming_cadence() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(1)?;
    tick_n(&env.pic, 30);
    let alice = env.owners[0];
    env.fund_owner(alice, 300_000_000)?;
    env.send_to_relay(alice, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        2,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(101_000)?;
    env.set_reward_index_lag(1)?;

    env.reward_sweep()?;
    let lagged = env.journal()?;
    if lagged.last_sweep_attempt_timestamp_seconds != 0 || lagged.pending_payout.is_some() {
        bail!("reward Index lag consumed cadence or planned a payout: {lagged:?}");
    }
    assert_eq!(env.reward_balance(alice)?, 0);
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|error| anyhow::anyhow!("fetch Relay logs failed: {error:?}"))?;
    if !logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD status=failed")
            && line.contains("reason=reward_history_not_caught_up")
    }) {
        bail!("reward Index lag did not emit one categorical summary");
    }

    env.set_reward_index_lag(0)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 100_000);
    if env.journal()?.last_sweep_attempt_timestamp_seconds == 0 {
        bail!("caught-up reward history did not complete adjudication");
    }
    Ok(())
}

#[test]
#[ignore]
fn reward_index_components_must_match_pinned_root_and_ledger() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(1)?;
    tick_n(&env.pic, 30);
    let alice = env.owners[0];
    env.fund_owner(alice, 300_000_000)?;
    env.send_to_relay(alice, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        2,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(101_000)?;

    for (root, ledger, index) in [
        (
            Some(Principal::management_canister()),
            Some(env.reward_ledger),
            Some(env.reward_index),
        ),
        (
            Some(env.root),
            Some(Principal::management_canister()),
            Some(env.reward_index),
        ),
        (Some(env.root), Some(env.reward_ledger), None),
    ] {
        env.set_reward_components(root, ledger, index)?;
        env.reward_sweep()?;
        if env.journal()?.last_sweep_attempt_timestamp_seconds != 0 {
            bail!("SNS component mismatch consumed reward cadence");
        }
    }

    env.set_reward_components(
        Some(env.root),
        Some(env.reward_ledger),
        Some(env.reward_index),
    )?;
    let _: () = update_one(
        &env.pic,
        env.reward_index,
        Principal::anonymous(),
        "debug_set_index_source",
        Principal::management_canister(),
    )?;
    env.reward_sweep()?;
    if env.journal()?.last_sweep_attempt_timestamp_seconds != 0 {
        bail!("Index ledger_id mismatch consumed reward cadence");
    }

    let _: () = update_one(
        &env.pic,
        env.reward_index,
        Principal::anonymous(),
        "debug_set_index_source",
        env.reward_ledger,
    )?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(alice)?, 100_000);
    Ok(())
}

#[test]
#[ignore]
fn splitter_provenance_is_stateless_and_same_commitment_receives_later_accrual() -> Result<()> {
    require_ignored_flag()?;
    const SPLITTER: u8 = 50;
    let mut env = RealSplitterRewardEnv::new(1)?;
    tick_n(&env.pic, 30);
    let owner = env.owners[0];
    env.fund_owner(owner, 600_000_000)?;
    env.send_to_relay(owner, relay_numbered_subaccount(SPLITTER), 500_000_000)?;
    env.main_tick()?;
    env.wait_for_histories(SPLITTER, 2)?;
    env.refresh_snapshot()?;

    env.credit_reward_pot(1_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 999_000);
    assert!(env.journal()?.pending_payout.is_none());

    env.credit_reward_pot(2_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 2_998_000);
    assert!(env.journal()?.pending_payout.is_none());
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|err| anyhow::anyhow!("fetch Relay logs failed: {err:?}"))?;
    if logs
        .iter()
        .filter(|entry| {
            let line = String::from_utf8_lossy(&entry.content);
            line.contains("RELAY_SNS_REWARD status=accepted")
                && line.contains("splitters_scanned=1")
        })
        .count()
        < 2
    {
        bail!("repeated stateless splitter attribution was not observable");
    }
    Ok(())
}

#[test]
#[ignore]
fn no_eligible_historical_commitment_holds_the_reward_pot() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(0)?;
    tick_n(&env.pic, 30);
    let unknown = Principal::self_authenticating([88; 32]);
    env.fund_owner(unknown, 200_000_000)?;
    env.send_to_relay(unknown, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        2,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(env.relay)?, 1_000_000);
    assert_eq!(env.reward_balance(unknown)?, 0);
    let journal = env.journal()?;
    if journal.pending_payout.is_some() || journal.last_sweep_attempt_timestamp_seconds == 0 {
        bail!("exhaustive no-eligible result was not a completed hold: {journal:?}");
    }
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|err| anyhow::anyhow!("fetch Relay logs failed: {err:?}"))?;
    if !logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD status=held")
            && line.contains("reason=no_eligible_historical_commitment")
    }) {
        bail!("missing exhausted-history hold summary");
    }
    Ok(())
}

#[test]
#[ignore]
fn multi_recipient_payout_survives_upgrade_and_duplicate_without_double_payment() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(3)?;
    tick_n(&env.pic, 30);
    for owner in env.owners.clone() {
        env.fund_owner(owner, 300_000_000)?;
        env.send_to_relay(owner, relay_subaccount_one(), 200_000_000)?;
    }
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        4,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_003_000)?;
    let _: () = update_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_set_error_script",
        vec![
            DebugNextTransferError::AcceptThenTrap,
            DebugNextTransferError::TemporarilyUnavailable,
        ],
    )?;
    env.reward_sweep()?;
    let ambiguous = env.journal()?;
    let payout = ambiguous
        .pending_payout
        .as_ref()
        .context("ambiguous multi-recipient payout was not durable")?;
    if payout.recipients.len() != 3
        || payout.next_recipient_index != 0
        || payout.recipients[0].status != RewardPendingTransferStatusFixture::Ambiguous
        || ambiguous.last_sweep_attempt_timestamp_seconds == 0
    {
        bail!("unexpected ambiguous payout state: {ambiguous:?}");
    }
    let current_spend = nat_to_u64(&payout.recipients[0].amount) + nat_to_u64(&payout.fee);
    env.credit_reward_pot(current_spend)?;

    env.pic.advance_time(Duration::from_secs(5 * 60));
    tick_n(&env.pic, 5);
    env.pic
        .upgrade_canister(
            env.relay,
            relay_wasm()?,
            encode_one(env.relay_init.clone())?,
            Some(Principal::anonymous()),
        )
        .map_err(|err| anyhow::anyhow!("ambiguous payout upgrade failed: {err:?}"))?;
    if env.journal()? != ambiguous {
        bail!("pinned multi-recipient payout changed across upgrade");
    }
    env.reward_sweep()?;
    let still_ambiguous = env.journal()?;
    if still_ambiguous != ambiguous {
        bail!("explicit retry failure changed the exact ambiguous identity");
    }
    env.reward_sweep()?;
    if env.journal()?.pending_payout.is_some() {
        bail!("Duplicate recovery did not finish the remaining payout");
    }
    for recipient in &payout.recipients {
        assert_eq!(
            env.reward_balance(recipient.recipient.owner)?,
            nat_to_u64(&recipient.amount),
            "a recipient was skipped or paid twice"
        );
    }
    let transfers: Vec<TransferRecord> = query_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    assert_eq!(
        transfers.len(),
        3,
        "Duplicate retry created another transfer"
    );
    assert_eq!(
        payout
            .recipients
            .iter()
            .map(|recipient| nat_to_u64(&recipient.amount))
            .sum::<u64>(),
        1_000_000
    );
    Ok(())
}

#[test]
#[ignore]
fn partially_paid_reward_payout_reprices_unpaid_recipients_after_fee_change() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(4)?;
    tick_n(&env.pic, 30);
    let original_owners = env.owners[..3].to_vec();
    let later_owner = env.owners[3];
    for owner in env.owners.clone() {
        env.fund_owner(owner, 300_000_000)?;
    }
    for owner in original_owners.clone() {
        env.send_to_relay(owner, relay_subaccount_one(), 200_000_000)?;
    }
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        4,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_003_000)?;
    let _: () = update_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_set_error_script",
        vec![
            DebugNextTransferError::PassThrough,
            DebugNextTransferError::BadFee {
                expected_fee_e8s: 2_000,
            },
        ],
    )?;

    env.reward_sweep()?;
    let rejected = env.journal()?;
    let payout = rejected
        .pending_payout
        .as_ref()
        .context("partial BadFee discarded the durable payout")?;
    if payout.next_recipient_index != 1
        || payout.recipients[0].status != RewardPendingTransferStatusFixture::AwaitingTransfer
        || payout.recipients[1].status != RewardPendingTransferStatusFixture::NeedsFreshIdentity
        || payout.recipients[1].uncertain_attempt_seen
    {
        bail!("partial BadFee did not preserve definitive unpaid progress: {rejected:?}");
    }
    let completed_recipient = payout.recipients[0].clone();
    let old_unpaid_identities = payout.recipients[1..]
        .iter()
        .map(|recipient| (recipient.memo.clone(), recipient.created_at_time_nanos))
        .collect::<Vec<_>>();
    assert_eq!(
        env.reward_balance(completed_recipient.recipient.owner)?,
        nat_to_u64(&completed_recipient.amount)
    );

    let _: () = update_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_set_fee",
        2_000_u64,
    )?;
    env.reward_sweep()?;
    let waiting = env.journal()?;
    let waiting_payout = waiting
        .pending_payout
        .as_ref()
        .context("insufficient fee headroom discarded the payout")?;
    if waiting_payout.next_recipient_index != 1
        || waiting_payout.recipients[1].status
            != RewardPendingTransferStatusFixture::WaitingForBalance
    {
        bail!("fee increase did not wait for balance safely: {waiting:?}");
    }
    assert_eq!(
        env.reward_balance(completed_recipient.recipient.owner)?,
        nat_to_u64(&completed_recipient.amount),
        "completed recipient was paid again while waiting"
    );

    // A newer commitment may complete while the old payout is pending, but it must not affect
    // the pinned recipients. Refreshing the owner snapshot only prepares the next adjudication.
    env.send_to_relay(later_owner, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        6,
    )?;
    env.refresh_snapshot()?;

    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);
    env.credit_reward_pot(1_010_000)?;
    let fee_headroom_arrival = env
        .reward_index_transactions()?
        .transactions
        .into_iter()
        .find_map(|entry| entry.transaction.mint.map(|_| entry.transaction.timestamp))
        .context("reward Index did not expose fee-headroom credit")?;
    env.refresh_snapshot()?;
    env.reward_sweep()?;
    if env.journal()?.pending_payout.is_some() {
        bail!("fee-headroom accrual did not resume the same payout");
    }
    for recipient in &payout.recipients {
        assert_eq!(
            env.reward_balance(recipient.recipient.owner)?,
            nat_to_u64(&recipient.amount),
            "repriced payout skipped or duplicated a recipient"
        );
    }
    let transfers: Vec<TransferRecord> = query_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    if transfers.len() != 3
        || transfers[0].memo.clone().unwrap_or_default() != completed_recipient.memo
        || transfers[0].created_at_time != Some(completed_recipient.created_at_time_nanos)
        || transfers[1..].iter().any(|transfer| {
            old_unpaid_identities.contains(&(
                transfer.memo.clone().unwrap_or_default(),
                transfer.created_at_time.unwrap_or_default(),
            ))
        })
        || transfers[1..]
            .iter()
            .any(|transfer| nat_to_u64(&transfer.fee) != 2_000)
    {
        bail!("unpaid identities were not safely repriced: {transfers:?}");
    }

    assert_eq!(env.reward_balance(later_owner)?, 0);
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(later_owner)?, 1_006_000);
    for recipient in &payout.recipients {
        assert_eq!(
            env.reward_balance(recipient.recipient.owner)?,
            nat_to_u64(&recipient.amount),
            "fresh residual attribution paid an already-completed old recipient"
        );
    }
    let logs = env
        .pic
        .fetch_canister_logs(env.relay, Principal::anonymous())
        .map_err(|error| anyhow::anyhow!("fetch Relay logs failed: {error:?}"))?;
    if !logs.iter().any(|entry| {
        let line = String::from_utf8_lossy(&entry.content);
        line.contains("RELAY_SNS_REWARD status=accepted")
            && line.contains(&format!(
                "attribution_cutoff_ts_nanos={fee_headroom_arrival}"
            ))
    }) {
        bail!("residual fee-headroom credit did not become the next reward cutoff");
    }
    Ok(())
}

#[test]
#[ignore]
fn legacy_pending_reward_migrates_exact_identity_and_discards_attribution_cursor() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new(1)?;
    tick_n(&env.pic, 30);
    let owner = env.owners[0];
    env.fund_owner(owner, 200_000_000)?;
    env.send_to_relay(owner, relay_subaccount_one(), 110_000_000)?;
    env.main_tick()?;
    let relay_history = account_identifier_text(env.relay, Some(relay_subaccount_one()));
    wait_for_real_index_transactions(&env.pic, env.icp_index, &relay_history, 2)?;
    let commitment_tx = env
        .index_transactions(relay_history)?
        .into_iter()
        .filter(|entry| matches!(entry.transaction.operation, IndexOperation::Transfer { ref from, .. } if from == &account_identifier_text(env.relay, Some(relay_subaccount_one()))))
        .map(|entry| entry.id)
        .max()
        .context("missing completed Faucet commitment")?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 999_000);
    let before: Vec<TransferRecord> = query_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    let accepted = before
        .last()
        .context("missing accepted reward transfer")?
        .clone();
    let created_at_time_nanos = accepted
        .created_at_time
        .context("reward transfer did not pin created_at_time")?;
    let memo = accepted
        .memo
        .clone()
        .context("reward transfer did not pin memo")?;

    env.credit_reward_pot(1_000_000)?;
    env.pic.advance_time(Duration::from_secs(60));
    tick_n(&env.pic, 5);
    write_reward_state_fixture(
        &env.pic,
        env.relay,
        RewardStateFixture {
            epoch_sns_root_canister_id: Some(env.root),
            processed_through_commitment_tx_id: Some(u64::MAX - 1),
            carried_credit_start_tx_id: Some(u64::MAX - 2),
            last_sweep_attempt_timestamp_seconds: 17,
            pending_transfer: Some(RewardPendingTransferFixture {
                sns_root_canister_id: env.root,
                sns_ledger_canister_id: env.reward_ledger,
                snapshot_id: 1,
                through_commitment_tx_id: commitment_tx,
                next_carried_credit_start_tx_id: Some(u64::MAX - 3),
                recipient: accepted.to,
                observed_balance: Nat::from(1_000_000_u64),
                fee: accepted.fee,
                amount: accepted.amount,
                memo,
                created_at_time_nanos,
                attempt_started: true,
                uncertain_attempt_seen: true,
                status: FrozenRewardPendingTransferStatusFixture::Ambiguous,
            }),
        },
    );
    env.pic
        .upgrade_canister(
            env.relay,
            relay_wasm()?,
            encode_one(env.relay_init.clone())?,
            Some(Principal::anonymous()),
        )
        .map_err(|err| anyhow::anyhow!("legacy reward-state upgrade failed: {err:?}"))?;
    let migrated = env.journal()?;
    let payout = migrated
        .pending_payout
        .as_ref()
        .context("legacy pending transfer was discarded")?;
    if migrated.last_sweep_attempt_timestamp_seconds != 0
        || payout.attribution_commitment_tx_id != commitment_tx
        || payout.recipients.len() != 1
        || payout.recipients[0].memo != accepted.memo.unwrap_or_default()
        || payout.recipients[0].created_at_time_nanos != created_at_time_nanos
        || payout.recipients[0].status != RewardPendingTransferStatusFixture::Ambiguous
    {
        bail!("legacy exact transfer identity did not survive migration: {migrated:?}");
    }
    env.reward_sweep()?;
    assert!(env.journal()?.pending_payout.is_none());
    let after_duplicate: Vec<TransferRecord> = query_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    assert_eq!(after_duplicate.len(), before.len());

    // The masked token credit is a later accrual. The discarded legacy cursor cannot prevent the
    // same historical commitment from receiving it.
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 1_998_000);
    Ok(())
}

#[test]
#[ignore]
fn real_v1_relay_wasm_migrates_pending_identity_and_resets_cadence() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new_with_relay_wasm(1, relay_v1_wasm()?)?;
    tick_n(&env.pic, 30);
    let owner = env.owners[0];
    env.fund_owner(owner, 300_000_000)?;
    env.send_to_relay(owner, relay_subaccount_one(), 100_010_000)?;
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        2,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000_000)?;
    let _: () = update_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_set_error_script",
        vec![
            DebugNextTransferError::AcceptThenTrap,
            DebugNextTransferError::TemporarilyUnavailable,
        ],
    )?;
    env.reward_sweep()?;
    let old: RewardStateFixture = query_one(
        &env.pic,
        env.relay,
        Principal::anonymous(),
        "debug_reward_state",
        (),
    )?;
    let old_pending = old
        .pending_transfer
        .as_ref()
        .context("real V1 Relay did not write an ambiguous pending transfer")?
        .clone();
    if old.last_sweep_attempt_timestamp_seconds == 0
        || old_pending.status != FrozenRewardPendingTransferStatusFixture::Ambiguous
    {
        bail!("real V1 Relay did not write the expected reward state: {old:?}");
    }

    env.pic.advance_time(Duration::from_secs(5 * 60));
    tick_n(&env.pic, 5);
    env.pic
        .upgrade_canister(
            env.relay,
            relay_wasm()?,
            encode_one(env.relay_init.clone())?,
            Some(Principal::anonymous()),
        )
        .map_err(|err| anyhow::anyhow!("real V1 Relay upgrade failed: {err:?}"))?;
    let migrated = env.journal()?;
    let payout = migrated
        .pending_payout
        .as_ref()
        .context("real V1 pending transfer did not migrate")?;
    let recipient = &payout.recipients[0];
    if migrated.last_sweep_attempt_timestamp_seconds != 0
        || payout.attribution_commitment_tx_id != old_pending.through_commitment_tx_id
        || recipient.recipient != old_pending.recipient
        || recipient.observed_balance != Some(old_pending.observed_balance.clone())
        || recipient.amount != old_pending.amount
        || recipient.memo != old_pending.memo
        || recipient.created_at_time_nanos != old_pending.created_at_time_nanos
        || recipient.status != RewardPendingTransferStatusFixture::Ambiguous
    {
        bail!("real V1 bytes did not migrate exactly into V3: {migrated:?}");
    }
    env.reward_sweep()?;
    if env.journal()?.pending_payout.is_some() || env.reward_balance(owner)? != 999_000 {
        bail!("real V1 pending transfer did not settle exactly once");
    }

    env.credit_reward_pot(1_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 1_998_000);
    Ok(())
}

#[test]
#[ignore]
fn real_v2_relay_wasm_discards_main_and_splitter_boundaries_on_v3_upgrade() -> Result<()> {
    require_ignored_flag()?;
    let mut env = RealSplitterRewardEnv::new_with_relay_wasm(1, relay_v2_wasm()?)?;
    tick_n(&env.pic, 30);
    let owner = env.owners[0];
    env.fund_owner(owner, 600_000_000)?;
    env.send_to_relay(owner, relay_numbered_subaccount(50), 300_000_000)?;
    env.main_tick()?;
    env.wait_for_histories(50, 2)?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 999_000);

    env.send_to_relay(owner, relay_subaccount_one(), 100_010_000)?;
    env.main_tick()?;
    tick_n(&env.pic, 5);
    env.main_tick()?;
    wait_for_real_index_transactions(
        &env.pic,
        env.icp_index,
        &account_identifier_text(env.relay, Some(relay_subaccount_one())),
        4,
    )?;
    env.refresh_snapshot()?;
    env.credit_reward_pot(1_000_000)?;
    let _: () = update_one(
        &env.pic,
        env.reward_ledger,
        Principal::anonymous(),
        "debug_set_error_script",
        vec![
            DebugNextTransferError::AcceptThenTrap,
            DebugNextTransferError::TemporarilyUnavailable,
        ],
    )?;
    env.reward_sweep()?;
    let old: RewardJournalV2View = query_one(
        &env.pic,
        env.relay,
        Principal::anonymous(),
        "debug_reward_state",
        (),
    )?;
    let old_pending = old
        .pending_transfer
        .as_ref()
        .context("real V2 Relay did not write an ambiguous pending transfer")?
        .clone();
    if old.processed_through_commitment_tx_id.is_none()
        || !old.splitter_boundaries.contains_key(&50)
        || old.last_sweep_attempt_timestamp_seconds == 0
        || old_pending.status != FrozenRewardPendingTransferStatusFixture::Ambiguous
    {
        bail!("real V2 Relay did not write main/splitter/pending state: {old:?}");
    }

    env.pic.advance_time(Duration::from_secs(5 * 60));
    tick_n(&env.pic, 5);
    env.pic
        .upgrade_canister(
            env.relay,
            relay_wasm()?,
            encode_one(env.relay_init.clone())?,
            Some(Principal::anonymous()),
        )
        .map_err(|err| anyhow::anyhow!("real V2 Relay upgrade failed: {err:?}"))?;
    let migrated = env.journal()?;
    let payout = migrated
        .pending_payout
        .as_ref()
        .context("real V2 pending transfer did not migrate")?;
    if migrated.last_sweep_attempt_timestamp_seconds != 0
        || payout.attribution_commitment_tx_id != old_pending.through_commitment_tx_id
        || payout.recipients[0].memo != old_pending.memo
        || payout.recipients[0].created_at_time_nanos != old_pending.created_at_time_nanos
        || payout.recipients[0].status != RewardPendingTransferStatusFixture::Ambiguous
    {
        bail!("real V2 bytes did not migrate exactly into cursor-free V3: {migrated:?}");
    }
    env.reward_sweep()?;
    if env.journal()?.pending_payout.is_some() || env.reward_balance(owner)? != 1_998_000 {
        bail!("real V2 pending transfer did not settle exactly once");
    }

    env.credit_reward_pot(1_000_000)?;
    env.reward_sweep()?;
    assert_eq!(env.reward_balance(owner)?, 2_997_000);
    Ok(())
}
