use anyhow::{anyhow, bail, Context, Result};
use candid::{decode_one, encode_one, CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
use pocket_ic::common::rest::{IcpFeatures, IcpFeaturesConfig};
use slog::Level;
use pocket_ic::{PocketIc, PocketIcBuilder};
use serde_bytes::ByteBuf;
use sha2::{Digest, Sha256};
use std::process::Command;
use std::time::Duration;
use std::sync::OnceLock;

// ----- Mainnet IDs (PocketIC bootstraps system canisters at mainnet IDs) -----
const ICP_LEDGER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
const NNS_GOVERNANCE_ID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";
const NNS_ROOT_ID: &str = "r7inp-6aaaa-aaaaa-aaabq-cai";

// We keep these E2E tests opt-in.
fn require_ignored_flag() -> Result<()> {
    // Running with: cargo test -p jupiter-disburser --test jupiter_disburser_integration -- --ignored --nocapture
    Ok(())
}

// ------------------------- Test harness helpers -------------------------


fn pic_log_level() -> Level {
    match std::env::var("E2E_PIC_LOG_LEVEL").ok().as_deref() {
        Some("critical") => Level::Critical,
        Some("error") => Level::Error,
        Some("warn") | Some("warning") => Level::Warning,
        Some("info") => Level::Info,
        Some("debug") => Level::Debug,
        Some("trace") => Level::Trace,
        Some(other) => {
            // Default to Error on invalid values.
            if trace_enabled() {
                eprintln!("[E2E] invalid E2E_PIC_LOG_LEVEL={other:?}; defaulting to error");
            }
            Level::Error
        }
        None => Level::Error,
    }
}

const DAY_SECS: u64 = 24 * 60 * 60;

fn advance_time_steps(pic: &PocketIc, total_secs: u64, step_secs: u64, ticks_per_step: usize) {
    let mut remaining = total_secs;
    while remaining > 0 {
        let step = remaining.min(step_secs);
        pic.advance_time(Duration::from_secs(step));
        tick_n(pic, ticks_per_step);
        remaining -= step;
    }
}

fn advance_days(pic: &PocketIc, days: u64) {
    // 6h steps keeps timer backlogs manageable.
    advance_time_steps(pic, days * DAY_SECS, 6 * 60 * 60, 10);
}



fn trace_enabled() -> bool {
    std::env::var_os("E2E_TRACE").is_some()
}

macro_rules! e2e_log {
    ($($tt:tt)*) => {{
        if trace_enabled() {
            eprintln!($($tt)*);
        }
    }};
}


fn nns_root() -> Principal {
    Principal::from_text(NNS_ROOT_ID).expect("valid NNS root principal")
}

fn stop_canister_as(pic: &PocketIc, canister: Principal, sender: Principal) -> Result<()> {
    pic.stop_canister(canister, Some(sender))
        .map_err(|r| anyhow!("stop_canister({canister}) reject: {r:?}"))
}

fn start_canister_as(pic: &PocketIc, canister: Principal, sender: Principal) -> Result<()> {
    pic.start_canister(canister, Some(sender))
        .map_err(|r| anyhow!("start_canister({canister}) reject: {r:?}"))
}

// ------------------------- Minimal candid types -------------------------
//
// We intentionally define only what we *need*.
// For decoding responses, extra fields from the canister are ignored.

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NeuronId {
    id: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GovernanceError {
    error_type: i32,
    error_message: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum NeuronResult {
    Ok(NeuronMinimal),
    Err(GovernanceError),
}

// governance.did: DissolveState = variant { DissolveDelaySeconds : nat64; WhenDissolvedTimestampSeconds : nat64 }
#[derive(Clone, Debug, CandidType, Deserialize)]
enum DissolveState {
    DissolveDelaySeconds(u64),
    WhenDissolvedTimestampSeconds(u64),
}

// governance.did: type Account = record { owner: principal; subaccount: opt blob; }
// We decode permissively: owner is optional here so we can decode both `principal` and `opt principal`.
#[derive(Clone, Debug, CandidType, Deserialize)]
struct GovAccount {
    owner: Option<Principal>,
    subaccount: Option<ByteBuf>,
}

// governance.did: type MaturityDisbursement = record { ... }
// We only decode a few fields; the rest can be ignored.
#[derive(Clone, Debug, CandidType, Deserialize)]
struct MaturityDisbursement {
    timestamp_of_disbursement_seconds: Option<u64>,
    finalize_disbursement_timestamp_seconds: Option<u64>,
    amount_e8s: Option<u64>,
    account_to_disburse_to: Option<GovAccount>,
    // account_identifier_to_disburse_to is a blob; we treat it as opaque bytes if present.
    account_identifier_to_disburse_to: Option<ByteBuf>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NeuronMinimal {
    id: Option<NeuronId>,
    account: ByteBuf, // 32-byte staking subaccount
    controller: Option<Principal>,
    dissolve_state: Option<DissolveState>,
    maturity_e8s_equivalent: u64,
    cached_neuron_stake_e8s: u64,
    aging_since_timestamp_seconds: u64,
    maturity_disbursements_in_progress: Option<Vec<MaturityDisbursement>>,
    voting_power_refreshed_timestamp_seconds: Option<u64>,
    deciding_voting_power: Option<u64>,
    potential_voting_power: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum NeuronIdOrSubaccount {
    NeuronId(NeuronId),
    Subaccount(ByteBuf),
}

#[derive(Clone, Debug, Default, CandidType, Deserialize)]
struct Empty {}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ManageNeuronRequest {
    // Deprecated but still present in candid; keep optional.
    id: Option<NeuronId>,
    neuron_id_or_subaccount: Option<NeuronIdOrSubaccount>,
    command: Option<ManageNeuronCommandRequest>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ManageNeuronResponse {
    command: Option<ManageNeuronCommandResponse>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum ManageNeuronCommandRequest {
    Configure(Configure),
    ClaimOrRefresh(ClaimOrRefresh),
    MakeProposal(MakeProposal),
    RegisterVote(RegisterVote),
    DisburseMaturity(DisburseMaturity),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum ManageNeuronCommandResponse {
    Configure(Empty),
    ClaimOrRefresh(ClaimOrRefreshResponse),
    MakeProposal(MakeProposalResponse),
    RegisterVote(Empty),
    DisburseMaturity(DisburseMaturityResponse),
    Error(GovernanceError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct Configure {
    operation: Option<Operation>,
}

// Keep only what we need.
#[derive(Clone, Debug, CandidType, Deserialize)]
enum Operation {
    AddHotKey(AddHotKey),
    IncreaseDissolveDelay(IncreaseDissolveDelay),
    StartDissolving(Empty),
    StopDissolving(Empty),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct AddHotKey {
    new_hot_key: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct IncreaseDissolveDelay {
    additional_dissolve_delay_seconds: u32,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ClaimOrRefresh {
    by: Option<By>,
}

// governance.did: By = variant { NeuronIdOrSubaccount : record {}; Memo : nat64; MemoAndController : record {...} }
#[derive(Clone, Debug, CandidType, Deserialize)]
enum By {
    NeuronIdOrSubaccount(Empty),
    Memo(u64),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ClaimOrRefreshResponse {
    refreshed_neuron_id: Option<NeuronId>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct MakeProposal {
    url: String,
    title: Option<String>,
    summary: String,
    action: Option<ProposalActionRequest>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum ProposalActionRequest {
    Motion(Motion),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct Motion {
    motion_text: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ProposalId {
    id: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct MakeProposalResponse {
    proposal_id: Option<ProposalId>,
    message: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RegisterVote {
    proposal: Option<ProposalId>,
    vote: i32, // 1 = yes
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DisburseMaturity {
    percentage_to_disburse: u32,
    to_account: Option<GovAccount>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DisburseMaturityResponse {
    amount_disbursed_e8s: Option<u64>,
}

// ------------------------- Jupiter-disburser init arg -------------------------

#[derive(Clone, Debug, CandidType, Deserialize)]
struct InitArg {
    neuron_id: u64,

    normal_recipient: Account,
    age_bonus_recipient_1: Account,
    age_bonus_recipient_2: Account,

    ledger_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,

    rescue_controller: Principal,
    blackhole_armed: Option<bool>,

    main_interval_seconds: Option<u64>,
    rescue_interval_seconds: Option<u64>,
}

// ------------------------- Debug API types -------------------------

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum ForcedRescueReason {
    BootstrapNoSuccess,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    prev_age_seconds: u64,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    payout_plan_present: bool,
    blackhole_armed_since_ts: Option<u64>,
    forced_rescue_reason: Option<ForcedRescueReason>,
}

// Logic constants (duplicated from crate::logic for E2E assertions)
const SECS_PER_DAY: u64 = 86_400;
const SECS_PER_YEAR: u64 = 365 * SECS_PER_DAY;
const MAX_AGE_FOR_BONUS_SECS: u64 = 4 * SECS_PER_YEAR;

// ------------------------- Helpers -------------------------

static DISBURSER_WASM_CACHE: OnceLock<Vec<u8>> = OnceLock::new();

fn build_disburser_wasm() -> Result<Vec<u8>> {
    if let Some(bytes) = DISBURSER_WASM_CACHE.get() {
        return Ok(bytes.clone());
    }

    if let Ok(path) = std::env::var("JUPITER_DISBURSER_WASM_PATH") {
        return std::fs::read(path).context("reading JUPITER_DISBURSER_WASM_PATH");
    }

    let status = Command::new("cargo")
        .args([
            "build",
            "--quiet",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "-p",
            "jupiter-disburser",
            "--features",
            "debug_api",
        ])
        .status()
        .context("failed to run cargo build for wasm")?;

    if !status.success() {
        bail!("cargo build (wasm) failed");
    }

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("CARGO_MANIFEST_DIR has no parent")?;

    let p = workspace_root.join("target/wasm32-unknown-unknown/release/jupiter_disburser.wasm");

    let bytes = std::fs::read(&p).with_context(|| format!("reading wasm at {}", p.display()))?;
    let _ = DISBURSER_WASM_CACHE.set(bytes.clone());
    Ok(bytes)
}

fn tick_n(pic: &PocketIc, n: usize) {
    for _ in 0..n {
        pic.tick();
    }
}

fn advance_and_tick(pic: &PocketIc, secs: u64, ticks: usize) {
    pic.advance_time(Duration::from_secs(secs));
    tick_n(pic, ticks);
}

fn nat_to_u64(n: &candid::Nat) -> u64 {
    n.0.to_u64_digits().get(0).copied().unwrap_or(0)
}

fn update_call<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    let bytes = encode_one(arg)?;
    let out = pic
        .update_call(canister, sender, method, bytes)
        .map_err(|r| anyhow!("reject: {:?}", r))?;
    Ok(decode_one(&out)?)
}

fn update_noargs<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
) -> Result<R> {
    let out = pic
        .update_call(canister, sender, method, encode_one(())?)
        .map_err(|r| anyhow!("reject: {:?}", r))?;
    Ok(decode_one(&out)?)
}

fn query_call<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    let bytes = encode_one(arg)?;
    let out = pic
        .query_call(canister, sender, method, bytes)
        .map_err(|r| anyhow!("reject: {:?}", r))?;
    Ok(decode_one(&out)?)
}



fn debug_state(pic: &PocketIc, canister: Principal) -> Result<DebugState> {
    query_call(pic, canister, Principal::anonymous(), "debug_state", ())
}

fn debug_set_prev_age_seconds(pic: &PocketIc, canister: Principal, age_seconds: u64) -> Result<()> {
    let _: () = update_call(
        pic,
        canister,
        Principal::anonymous(),
        "debug_set_prev_age_seconds",
        age_seconds,
    )?;
    Ok(())
}


fn debug_set_pause_after_planning(pic: &PocketIc, canister: Principal, enabled: bool) -> Result<()> {
    let _: () = update_call(
        pic,
        canister,
        Principal::anonymous(),
        "debug_set_pause_after_planning",
        enabled,
    )?;
    Ok(())
}

fn debug_set_trap_after_successful_transfers(
    pic: &PocketIc,
    canister: Principal,
    n: Option<u32>,
) -> Result<()> {
    let _: () = update_call(
        pic,
        canister,
        Principal::anonymous(),
        "debug_set_trap_after_successful_transfers",
        n,
    )?;
    Ok(())
}

fn debug_set_simulate_low_cycles(pic: &PocketIc, canister: Principal, enabled: bool) -> Result<()> {
    let _: () = update_call(
        pic,
        canister,
        Principal::anonymous(),
        "debug_set_simulate_low_cycles",
        enabled,
    )?;
    Ok(())
}

fn debug_set_skip_maturity_initiation(
    pic: &PocketIc,
    canister: Principal,
    enabled: bool,
) -> Result<()> {
    let _: () = update_call(
        pic,
        canister,
        Principal::anonymous(),
        "debug_set_skip_maturity_initiation",
        enabled,
    )?;
    Ok(())
}

fn debug_state_size_bytes(pic: &PocketIc, canister: Principal) -> Result<u64> {
    query_call(pic, canister, Principal::anonymous(), "debug_state_size_bytes", ())
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
struct ExpectedPlanned {
    to: Account,
    gross_share_e8s: u64,
    amount_e8s: u64,
}

#[derive(Clone, Debug)]
struct ExpectedGrossSplit {
    base_e8s: u64,
    bonus80_e8s: u64,
    bonus20_e8s: u64,
}

fn expected_compute_gross_split(total_e8s: u64, age_seconds: u64) -> ExpectedGrossSplit {
    if total_e8s == 0 {
        return ExpectedGrossSplit {
            base_e8s: 0,
            bonus80_e8s: 0,
            bonus20_e8s: 0,
        };
    }

    // m = 1 + min(age,4y) / (16y)
    let den: u128 = (16 * SECS_PER_YEAR) as u128;
    let bonus_secs: u128 = age_seconds.min(MAX_AGE_FOR_BONUS_SECS) as u128;
    let num: u128 = den + bonus_secs;

    let base = ((total_e8s as u128) * den / num) as u64;
    let bonus = total_e8s.saturating_sub(base);

    // 80/20, rounding toward 80 side (ceil)
    let b80 = (((bonus as u128) * 80) + 99) / 100;
    let b80 = b80 as u64;
    let b20 = bonus.saturating_sub(b80);

    ExpectedGrossSplit {
        base_e8s: base,
        bonus80_e8s: b80,
        bonus20_e8s: b20,
    }
}

fn expected_plan_payout_transfers(
    staging_balance_e8s: u64,
    fee_e8s: u64,
    age_seconds: u64,
    normal_to: &Account,
    bonus1_to: &Account,
    bonus2_to: &Account,
) -> (ExpectedGrossSplit, Vec<ExpectedPlanned>) {
    let gross = expected_compute_gross_split(staging_balance_e8s, age_seconds);

    let mut out: Vec<ExpectedPlanned> = Vec::with_capacity(3);
    let mut push = |to: &Account, share: u64| {
        if share <= fee_e8s {
            return;
        }
        out.push(ExpectedPlanned {
            to: to.clone(),
            gross_share_e8s: share,
            amount_e8s: share - fee_e8s,
        });
    };

    push(normal_to, gross.base_e8s);
    push(bonus1_to, gross.bonus80_e8s);
    push(bonus2_to, gross.bonus20_e8s);

    (gross, out)
}
fn get_full_neuron(
    pic: &PocketIc,
    gov: Principal,
    sender: Principal,
    neuron_id: u64,
) -> Result<NeuronMinimal> {
    let res: NeuronResult = query_call(pic, gov, sender, "get_full_neuron", neuron_id)?;
    match res {
        NeuronResult::Ok(n) => Ok(n),
        NeuronResult::Err(e) => bail!("get_full_neuron err: {:?}", e),
    }
}


fn neuron_snapshot(
    pic: &PocketIc,
    gov: Principal,
    sender: Principal,
    neuron_id: u64,
    label: &str,
) -> Result<String> {
    let n = get_full_neuron(pic, gov, sender, neuron_id)?;
    let now_secs = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    let age_seconds = now_secs.saturating_sub(n.aging_since_timestamp_seconds);
    let inflight_len = n
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);

    Ok(format!(
        "snapshot[{label}]: queried_as={sender:?} requested_neuron_id={neuron_id} reported_neuron_id={:?} reported_controller={:?} stake_e8s={} dissolve_state={:?} age_seconds={} aging_since={} maturity_e8s_equivalent={} inflight_len={} inflight={:?}",
        n.id.as_ref().map(|id| id.id),
        n.controller,
        n.cached_neuron_stake_e8s,
        n.dissolve_state,
        age_seconds,
        n.aging_since_timestamp_seconds,
        n.maturity_e8s_equivalent,
        inflight_len,
        n.maturity_disbursements_in_progress,
    ))
}

fn icrc1_fee(pic: &PocketIc, ledger: Principal) -> Result<u64> {
    // icrc1_fee : () -> (nat) query
    let fee_nat: candid::Nat = query_call(pic, ledger, Principal::anonymous(), "icrc1_fee", ())?;
    Ok(nat_to_u64(&fee_nat))
}

fn icrc1_balance(pic: &PocketIc, ledger: Principal, acct: &Account) -> Result<u64> {
    let bal_nat: candid::Nat =
        query_call(pic, ledger, Principal::anonymous(), "icrc1_balance_of", acct.clone())?;
    Ok(nat_to_u64(&bal_nat))
}

fn icrc1_transfer(pic: &PocketIc, ledger: Principal, from: Principal, arg: TransferArg) -> Result<()> {
    let res: std::result::Result<candid::Nat, TransferError> =
        update_call(pic, ledger, from, "icrc1_transfer", arg)?;
    match &res {
        Ok(_) => Ok(()),
        Err(e) => bail!("icrc1_transfer error: {:?}", e),
    }
}

// NNS staking subaccount derivation (matches NNS tooling):
// sha256( b"\x0cneuron-stake" ++ controller_principal_bytes ++ memo_be_bytes )
fn neuron_staking_subaccount(controller: Principal, memo: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"\x0cneuron-stake");
    hasher.update(controller.as_slice());
    hasher.update(memo.to_be_bytes());
    let out = hasher.finalize();
    let mut sa = [0u8; 32];
    sa.copy_from_slice(&out[..]);
    sa
}

// Stake ICP into the governance staking subaccount and claim the neuron.
// Returns the neuron id from ClaimOrRefreshResponse (no searching).
fn stake_and_claim_neuron(
    pic: &PocketIc,
    ledger: Principal,
    gov: Principal,
    controller: Principal,
    memo: u64,
    stake_e8s: u64,
) -> Result<u64> {
    let fee_e8s = icrc1_fee(pic, ledger)?;
    let sa = neuron_staking_subaccount(controller, memo);

    let staking_account = Account {
        owner: gov,
        subaccount: Some(sa),
    };

    let before = icrc1_balance(pic, ledger, &staking_account)?;
    e2e_log!("staking_account before={before} fee={fee_e8s} memo={memo}");

    icrc1_transfer(
        pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: staking_account.clone(),
            fee: Some(candid::Nat::from(fee_e8s)),
            created_at_time: None,
            memo: None,
            amount: candid::Nat::from(stake_e8s),
        },
    )?;

    let after = icrc1_balance(pic, ledger, &staking_account)?;
    e2e_log!("staking_account after={after}");
    if after < before.saturating_add(stake_e8s) {
        e2e_log!(
            "warn: staking account balance did not increase as expected (before={before}, after={after}, expected_add={stake_e8s})"
        );
    }

    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: None,
        command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(ClaimOrRefresh {
            by: Some(By::Memo(memo)),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    match resp.command {
        Some(ManageNeuronCommandResponse::ClaimOrRefresh(r)) => {
            let nid = r
                .refreshed_neuron_id
                .ok_or_else(|| anyhow!("claim_or_refresh returned no refreshed_neuron_id"))?
                .id;
            e2e_log!("created neuron_id={nid} controller={controller}");
            Ok(nid)
        }
        Some(ManageNeuronCommandResponse::Error(e)) => bail!("claim_or_refresh failed: {:?}", e),
        other => bail!("unexpected claim_or_refresh response: {:?}", other),
    }
}

fn add_hotkey(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    hotkey: Principal,
) -> Result<()> {
    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::Configure(Configure {
            operation: Some(Operation::AddHotKey(AddHotKey {
                new_hot_key: Some(hotkey),
            })),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    if let Some(ManageNeuronCommandResponse::Error(e)) = resp.command {
        bail!("add_hotkey failed: {:?}", e);
    }
    Ok(())
}

// Make neuron eligible for voting rewards by ensuring dissolve delay >= minimum.
// (Docs: minimum dissolve delay ~6 months to vote.)
fn increase_dissolve_delay(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    additional_seconds: u32,
) -> Result<()> {
    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::Configure(Configure {
            operation: Some(Operation::IncreaseDissolveDelay(IncreaseDissolveDelay {
                additional_dissolve_delay_seconds: additional_seconds,
            })),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    if let Some(ManageNeuronCommandResponse::Error(e)) = resp.command {
        bail!("increase_dissolve_delay failed: {:?}", e);
    }
    Ok(())
}



fn transfer_to_neuron_staking_subaccount(
    pic: &PocketIc,
    ledger: Principal,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    amount_e8s: u64,
) -> Result<()> {
    let fee_e8s = icrc1_fee(pic, ledger)?;
    let n = get_full_neuron(pic, gov, controller, neuron_id)?;
    let sub = n.account.to_vec();
    if sub.len() != 32 {
        bail!("unexpected neuron.account len={}, expected 32", sub.len());
    }
    let mut sa = [0u8; 32];
    sa.copy_from_slice(&sub);

    let to = Account {
        owner: gov,
        subaccount: Some(sa),
    };

    icrc1_transfer(
        pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to,
            fee: Some(candid::Nat::from(fee_e8s)),
            created_at_time: None,
            memo: None,
            amount: candid::Nat::from(amount_e8s),
        },
    )?;

    Ok(())
}

fn refresh_neuron_stake(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
) -> Result<()> {
    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(ClaimOrRefresh {
            by: Some(By::NeuronIdOrSubaccount(Empty::default())),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    if let Some(ManageNeuronCommandResponse::Error(e)) = resp.command {
        bail!("claim_or_refresh (refresh) failed: {:?}", e);
    }
    Ok(())
}

fn top_up_neuron_stake_and_refresh(
    pic: &PocketIc,
    ledger: Principal,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    amount_e8s: u64,
) -> Result<()> {
    transfer_to_neuron_staking_subaccount(pic, ledger, gov, controller, neuron_id, amount_e8s)?;
    refresh_neuron_stake(pic, gov, controller, neuron_id)
}

fn make_and_settle_motion_proposal(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    motion_text: &str,
    proposal_seq: u64,
) -> Result<ProposalId> {
    // Use an allowed URL domain: forum.dfinity.org
    let url = format!("https://forum.dfinity.org/t/pocketic-e2e-motion/{proposal_seq}");

    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::MakeProposal(MakeProposal {
            url,
            title: Some(format!("PocketIC E2E Motion #{proposal_seq}")),
            summary: "Trigger reward distribution in PocketIC".to_string(),
            action: Some(ProposalActionRequest::Motion(Motion {
                motion_text: motion_text.to_string(),
            })),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    let pid = match resp.command {
        Some(ManageNeuronCommandResponse::MakeProposal(r)) => {
            r.proposal_id.ok_or_else(|| anyhow!("no proposal_id"))?
        }
        Some(ManageNeuronCommandResponse::Error(e)) => bail!("make_proposal failed: {:?}", e),
        other => bail!("unexpected make_proposal response: {:?}", other),
    };

    e2e_log!("made proposal id={}", pid.id);

    // Try to vote YES. Some NNS versions auto-cast a vote for the proposer; tolerate "already voted".
    let vote_req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::RegisterVote(RegisterVote {
            proposal: Some(pid.clone()),
            vote: 1,
        })),
    };

    let vote_resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", vote_req)?;
    if let Some(ManageNeuronCommandResponse::Error(e)) = vote_resp.command {
        // Observed: error_type=19, "Neuron already voted on proposal."
        if e.error_type != 19 {
            bail!("register_vote failed: {:?}", e);
        }
    }

    // Let voting/reward periods elapse; tick a few rounds to run timers.
    advance_days(pic, 8);

    Ok(pid)
}


fn make_motion_proposal(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    motion_text: &str,
    proposal_seq: u64,
) -> Result<ProposalId> {
    // Use an allowed URL domain: forum.dfinity.org
    let url = format!("https://forum.dfinity.org/t/pocketic-e2e-motion/{proposal_seq}");

    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::MakeProposal(MakeProposal {
            url,
            title: Some(format!("PocketIC E2E Motion #{proposal_seq}")),
            summary: "Trigger reward distribution in PocketIC".to_string(),
            action: Some(ProposalActionRequest::Motion(Motion {
                motion_text: motion_text.to_string(),
            })),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    let pid = match resp.command {
        Some(ManageNeuronCommandResponse::MakeProposal(r)) => {
            r.proposal_id.ok_or_else(|| anyhow!("no proposal_id"))?
        }
        Some(ManageNeuronCommandResponse::Error(e)) => bail!("make_proposal failed: {:?}", e),
        other => bail!("unexpected make_proposal response: {:?}", other),
    };

    Ok(pid)
}

fn register_vote_yes(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    pid: &ProposalId,
) -> Result<()> {
    let vote_req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::RegisterVote(RegisterVote {
            proposal: Some(pid.clone()),
            vote: 1,
        })),
    };

    let vote_resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", vote_req)?;
    if let Some(ManageNeuronCommandResponse::Error(e)) = vote_resp.command {
        // Observed: error_type=19, "Neuron already voted on proposal."
        if e.error_type != 19 {
            bail!("register_vote failed: {:?}", e);
        }
    }

    Ok(())
}

fn disburse_maturity_to_account(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
    to: &Account,
) -> Result<()> {
    let to_acc = GovAccount {
        owner: Some(to.owner),
        subaccount: to.subaccount.map(|sa| ByteBuf::from(sa.to_vec())),
    };

    let req = ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
        command: Some(ManageNeuronCommandRequest::DisburseMaturity(DisburseMaturity {
            percentage_to_disburse: 100,
            to_account: Some(to_acc),
        })),
    };

    let resp: ManageNeuronResponse = update_call(pic, gov, controller, "manage_neuron", req)?;
    match resp.command {
        Some(ManageNeuronCommandResponse::DisburseMaturity(_)) => Ok(()),
        Some(ManageNeuronCommandResponse::Error(e)) => bail!("disburse_maturity failed: {:?}", e),
        other => bail!("unexpected disburse_maturity response: {:?}", other),
    }
}

fn ensure_maturity_ge_1_icp(
    pic: &PocketIc,
    ledger: Principal,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
) -> Result<()> {
    let target = 200_000_000u64; // 2 ICP in e8s (>= 1 ICP after worst-case -5% maturity modulation)

    // Make the neuron big so rewards become non-trivial.
    top_up_neuron_stake_and_refresh(pic, ledger, gov, controller, neuron_id, 5_000_000 * 100_000_000)?; // 5,000,000 ICP

    for i in 0..12u64 {
        let n = get_full_neuron(pic, gov, controller, neuron_id)?;
        e2e_log!(
            "maturity loop {i}: maturity={} stake={} dissolve_state={:?} age_since={}",
            n.maturity_e8s_equivalent,
            n.cached_neuron_stake_e8s,
            n.dissolve_state,
            n.aging_since_timestamp_seconds
        );

        if n.maturity_e8s_equivalent >= target {
            return Ok(());
        }

        // Create+vote a proposal and advance beyond its reward period to trigger reward distribution.
        let _ = make_and_settle_motion_proposal(
            pic,
            gov,
            controller,
            neuron_id,
            "E2E reward trigger",
            i + 1,
        )?;
        tick_n(pic, 60);
    }

    let n = get_full_neuron(pic, gov, controller, neuron_id)?;
    bail!(
        "failed to reach 1 ICP maturity after retries (maturity_e8s_equivalent={}, dissolve_state={:?})",
        n.maturity_e8s_equivalent,
        n.dissolve_state
    );
}

fn wait_for_inflight_disbursement(
    pic: &PocketIc,
    gov: Principal,
    controller: Principal,
    neuron_id: u64,
) -> Result<(bool, NeuronMinimal)> {
    // Empirically, PocketIC may require at least one advance_time + tick before
    // governance surfaces maturity_disbursements_in_progress.
    let mut last = get_full_neuron(pic, gov, controller, neuron_id)?;
    for _ in 0..60 {
        let inflight = last
            .maturity_disbursements_in_progress
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        if inflight {
            return Ok((true, last));
        }

        // Advance a little and tick; this is the important part.
        advance_and_tick(pic, 1, 2);
        last = get_full_neuron(pic, gov, controller, neuron_id)?;
    }
    Ok((false, last))
}

fn wait_for_staging_credit(
    pic: &PocketIc,
    ledger: Principal,
    staging: &Account,
    before: u64,
) -> Result<u64> {
    // In NNS the finalize-maturity transfer happens ~7 days after initiation.
    // We allow slack for PocketIC timer scheduling and avoid a single huge time jump
    // (which can create timer backlogs and very noisy logs).
    const MAX_WAIT_SECS: u64 = 10 * DAY_SECS;
    const STEP_SECS: u64 = 60 * 60; // 1 hour

    let steps = (MAX_WAIT_SECS + STEP_SECS - 1) / STEP_SECS;
    for _ in 0..steps {
        let after = icrc1_balance(pic, ledger, staging)?;
        if after > before {
            return Ok(after);
        }
        advance_time_steps(pic, STEP_SECS, STEP_SECS, 10);
    }

    let after = icrc1_balance(pic, ledger, staging)?;
    bail!("staging not credited within {}s: before={} after={}", MAX_WAIT_SECS, before, after)
}


fn wait_for_cached_stake_increase(
    pic: &PocketIc,
    gov: Principal,
    sender: Principal,
    neuron_id: u64,
    before: u64,
) -> Result<NeuronMinimal> {
    const MAX_WAIT_SECS: u64 = 5 * 60;
    const STEP_SECS: u64 = 5;

    let steps = (MAX_WAIT_SECS + STEP_SECS - 1) / STEP_SECS;
    let mut last = get_full_neuron(pic, gov, sender, neuron_id)?;
    for _ in 0..steps {
        if last.cached_neuron_stake_e8s > before {
            return Ok(last);
        }
        advance_time_steps(pic, STEP_SECS, STEP_SECS, 2);
        last = get_full_neuron(pic, gov, sender, neuron_id)?;
    }

    bail!(
        "cached stake did not increase within {}s: before={} after={}",
        MAX_WAIT_SECS,
        before,
        last.cached_neuron_stake_e8s
    )
}



fn build_pic() -> PocketIc {
    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        // Required for maturity disbursement finalization: governance needs maturity modulation,
        // which depends on the cycles minting canister being present in the NNS subnet.
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build()
}

fn set_self_only_controller(pic: &PocketIc, canister: Principal) -> Result<()> {
    let current = pic.get_controllers(canister);
    let sender = current
        .get(0)
        .cloned()
        .unwrap_or_else(Principal::anonymous);

    pic.set_controllers(canister, Some(sender), vec![canister])
        .map_err(|e| anyhow!("set_controllers reject: {:?}", e))?;
    Ok(())
}

fn set_controllers_exact(pic: &PocketIc, canister: Principal, controllers: Vec<Principal>) -> Result<()> {
    let current = pic.get_controllers(canister);
    let sender = current
        .get(0)
        .cloned()
        .unwrap_or_else(Principal::anonymous);

    pic.set_controllers(canister, Some(sender), controllers)
        .map_err(|e| anyhow!("set_controllers reject: {:?}", e))?;
    Ok(())
}

// ------------------------- Tests -------------------------

#[test]
#[ignore]
fn e2e_nns_maturity_disbursement_lands_in_staging() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        // Required for maturity disbursement finalization: governance needs maturity modulation,
        // which depends on the cycles minting canister being present in the NNS subnet.
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    // IMPORTANT: Hotkeys have limited permissions in the NNS; they cannot disburse stake
    // (and by extension cannot perform sensitive manage-neuron operations like disburse maturity).
    // For realism and to make DisburseMaturity succeed, the neuron controller must be the
    // disburser canister itself.
    let controller = disburser_canister;

    // Stake+claim a neuron controlled by the disburser canister.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 42, 10_000 * 100_000_000)?;

    // Make it eligible for rewards by ensuring dissolve delay >= ~6 months.
    // We add 1 year worth of seconds for margin.
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    // Ensure maturity >= 1 ICP so disburse_maturity should be meaningful.
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // Build + install jupiter-disburser (debug_api enabled).
    let wasm = build_disburser_wasm()?;

    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        // Rescue controller is explicit in all test installs.
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        // Keep timers effectively disabled in e2e (we drive execution manually).
        main_interval_seconds: Some(365 * 24 * 60 * 60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    // Allow the disburser canister to manage its own controllers (like production).
    set_self_only_controller(&pic, disburser_canister)?;

    // NOTE: no hotkey needed because the disburser canister is the neuron controller.

    let staging = Account { owner: disburser_canister, subaccount: None };
    let before = icrc1_balance(&pic, ledger, &staging)?;
    e2e_log!("staging before={before}");

    // Trigger your canister's logic.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    // Give PocketIC a chance to surface in-flight disbursement state (REQUIRED).
    // This is a critical observability signal used by the scheduler to decide whether to no-op.
    let (inflight_seen, n_after_start) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !inflight_seen {
        bail!(
            "expected in-flight maturity disbursement to be observable, but it was not; maturity={} stake={} disbursements={:?}",
            n_after_start.maturity_e8s_equivalent,
            n_after_start.cached_neuron_stake_e8s,
            n_after_start.maturity_disbursements_in_progress,
        );
    }
    e2e_log!(
        "inflight detected: disbursements={:?}",
        n_after_start.maturity_disbursements_in_progress.as_ref().map(|v| v.len())
    );

    // On NNS, maturity disbursement finalizes ~7 days after initiation.
    // Stop the disburser so staging can't be drained while we fast-forward.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    // Advance beyond that window and drive execution.
    advance_days(&pic, 8);

    // Assert by balance (this is the robust signal).
    let after = wait_for_staging_credit(&pic, ledger, &staging, before)?;
    e2e_log!("staging after={after}");

    if after <= before {
        let n_dbg = get_full_neuron(&pic, gov, controller, neuron_id)?;
        bail!(
            "expected staging balance to increase after maturity disbursement (before={before}, after={after}); \
             maturity={} stake={} maturity_disbursements_in_progress={:?}",
            n_dbg.maturity_e8s_equivalent,
            n_dbg.cached_neuron_stake_e8s,
            n_dbg.maturity_disbursements_in_progress,
        );
    }

    Ok(())
}



// ------------------------- Additional E2E scenarios -------------------------

#[test]
#[ignore]
fn e2e_full_pipeline_maturity_to_transfers_real_ledger() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    // Disburser canister and neuron controller are the same.
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 42, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // Install disburser with short main interval (min_gap=0, so debug_main_tick is never suppressed).
    let wasm = build_disburser_wasm()?;

    let normal_owner = pic.create_canister();
    let bonus1_owner = pic.create_canister();
    let bonus2_owner = pic.create_canister();

    let normal = Account { owner: normal_owner, subaccount: None };
    let bonus1 = Account { owner: bonus1_owner, subaccount: None };
    let bonus2 = Account { owner: bonus2_owner, subaccount: None };

    let init = InitArg {
        neuron_id,
        normal_recipient: normal.clone(),
        age_bonus_recipient_1: bonus1.clone(),
        age_bonus_recipient_2: bonus2.clone(),
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    // Production posture: self-only controller.
    set_self_only_controller(&pic, disburser_canister)?;

    let staging = Account { owner: disburser_canister, subaccount: None };
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;

    // Recipients before.
    let b0_before = icrc1_balance(&pic, ledger, &normal)?;
    let b1_before = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_before = icrc1_balance(&pic, ledger, &bonus2)?;

    // 1) Initiate maturity disbursement.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    let (inflight_seen, n_after_start) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !inflight_seen {
        bail!(
            "expected in-flight maturity disbursement to be observable, but it was not; maturity={} stake={} disbursements={:?}",
            n_after_start.maturity_e8s_equivalent,
            n_after_start.cached_neuron_stake_e8s,
            n_after_start.maturity_disbursements_in_progress,
        );
    }

    // Prevent timers from draining staging while we fast-forward time.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;

    // 2) Finalize (~7d). This should mint into the staging account.
    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    if staging_after <= staging_before {
        bail!("expected staging to increase (before={staging_before}, after={staging_after})");
    }

    // 3) Start canister and run payout stage.
    start_canister_as(&pic, disburser_canister, disburser_canister)?;

    // Force deterministic max bonus for assertions.
    debug_set_prev_age_seconds(&pic, disburser_canister, MAX_AGE_FOR_BONUS_SECS)?;

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let (_gross, planned) = expected_plan_payout_transfers(
        staging_after,
        fee_e8s,
        MAX_AGE_FOR_BONUS_SECS,
        &normal,
        &bonus1,
        &bonus2,
    );
    if planned.len() != 3 {
        bail!("expected 3 planned transfers at max age and large staging, got {}", planned.len());
    }

    // Run payout (and wait until the ledger reflects the transfers).
    let mut paid = false;
    for _ in 0..50 {
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 25);

        let b0_after = icrc1_balance(&pic, ledger, &normal)?;
        let b1_after = icrc1_balance(&pic, ledger, &bonus1)?;
        let b2_after = icrc1_balance(&pic, ledger, &bonus2)?;

        let d0 = b0_after.saturating_sub(b0_before);
        let d1 = b1_after.saturating_sub(b1_before);
        let d2 = b2_after.saturating_sub(b2_before);

        let st = debug_state(&pic, disburser_canister)?;
        if d0 == planned[0].amount_e8s
            && d1 == planned[1].amount_e8s
            && d2 == planned[2].amount_e8s
            && !st.payout_plan_present
        {
            paid = true;
            break;
        }
    }

    if !paid {
        let b0_after = icrc1_balance(&pic, ledger, &normal)?;
        let b1_after = icrc1_balance(&pic, ledger, &bonus1)?;
        let b2_after = icrc1_balance(&pic, ledger, &bonus2)?;

        let d0 = b0_after.saturating_sub(b0_before);
        let d1 = b1_after.saturating_sub(b1_before);
        let d2 = b2_after.saturating_sub(b2_before);
        let st = debug_state(&pic, disburser_canister)?;
        bail!(
            "unexpected recipient deltas: got [d0={d0}, d1={d1}, d2={d2}] expected [a0={}, a1={}, a2={}] (fee={fee_e8s}) plan_present={}",
            planned[0].amount_e8s,
            planned[1].amount_e8s,
            planned[2].amount_e8s,
            st.payout_plan_present
        );
    }

let st = debug_state(&pic, disburser_canister)?;
    if st.payout_plan_present {
        bail!("expected payout plan to be cleared after successful payout");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_inflight_idempotency_no_double_initiation() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 7, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: pic.create_canister(), subaccount: None },
        age_bonus_recipient_1: Account { owner: pic.create_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: pic.create_canister(), subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    let staging = Account { owner: disburser_canister, subaccount: None };
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;

    // Initiate once.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    // Require in-flight is visible.
    let (inflight_seen, _) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !inflight_seen {
        bail!("expected in-flight to become visible after initiation");
    }

    // Call main tick repeatedly while in-flight.
    for _ in 0..10 {
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        // Keep time advance below 60s so timers don't fire implicitly.
        advance_and_tick(&pic, 1, 1);
        let n = get_full_neuron(&pic, gov, controller, neuron_id)?;
        let len = n
            .maturity_disbursements_in_progress
            .as_ref()
            .map(|v| v.len())
            .unwrap_or(0);
        if len != 1 {
            bail!("expected exactly 1 in-flight disbursement, got len={len}");
        }
    }

    // And staging should not have been credited yet.
    let staging_mid = icrc1_balance(&pic, ledger, &staging)?;
    if staging_mid != staging_before {
        bail!("expected staging to remain unchanged during in-flight (before={staging_before}, now={staging_mid})");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_upgrade_mid_inflight_preserves_state_and_completes() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 99, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: pic.create_canister(), subaccount: None },
        age_bonus_recipient_1: Account { owner: pic.create_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: pic.create_canister(), subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm.clone(), encode_one(init)?, None);

    // Self-only controller so we can stop/start using the canister principal.
    set_self_only_controller(&pic, disburser_canister)?;

    // Initiate.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    let (inflight_seen, _) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !inflight_seen {
        bail!("expected in-flight to become visible before upgrade");
    }

    // Set a sentinel value in stable state so we can prove it survives upgrade.
    debug_set_prev_age_seconds(&pic, disburser_canister, 1_234_567)?;
    let st0 = debug_state(&pic, disburser_canister)?;
    if st0.prev_age_seconds != 1_234_567 {
        bail!("expected sentinel prev_age_seconds to be set, got {}", st0.prev_age_seconds);
    }

    // Upgrade with same WASM (arg is empty; post_upgrade ignores args).
    pic.upgrade_canister(disburser_canister, wasm, encode_one(())?, Some(disburser_canister))
        .map_err(|e| anyhow!("upgrade_canister reject: {:?}", e))?;

    let st1 = debug_state(&pic, disburser_canister)?;
    if st1.prev_age_seconds != 1_234_567 {
        bail!("expected prev_age_seconds to survive upgrade, got {}", st1.prev_age_seconds);
    }

    // Still in-flight.
    let n = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let len = n
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);
    if len != 1 {
        bail!("expected still exactly 1 in-flight disbursement after upgrade, got len={len}");
    }

    // Completion still works.
    let staging = Account { owner: disburser_canister, subaccount: None };
    let before = icrc1_balance(&pic, ledger, &staging)?;
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 8);
    let after = wait_for_staging_credit(&pic, ledger, &staging, before)?;
    if after <= before {
        bail!("expected staging to increase after upgrade (before={before}, after={after})");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_payout_plan_persists_across_ledger_stop_and_resumes() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 555, 1_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    let wasm = build_disburser_wasm()?;

    let normal_owner = pic.create_canister();
    let bonus1_owner = pic.create_canister();
    let bonus2_owner = pic.create_canister();

    let normal = Account { owner: normal_owner, subaccount: None };
    let bonus1 = Account { owner: bonus1_owner, subaccount: None };
    let bonus2 = Account { owner: bonus2_owner, subaccount: None };

    let init = InitArg {
        neuron_id,
        normal_recipient: normal.clone(),
        age_bonus_recipient_1: bonus1.clone(),
        age_bonus_recipient_2: bonus2.clone(),
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    // Fund staging with a known amount that guarantees 3 transfers at max bonus.
    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let staging = Account { owner: disburser_canister, subaccount: None };

    // total > 25*fee ensures even the 4% share is > fee at max bonus (0.04*total).
    let staging_fund = 100 * fee_e8s;

    icrc1_transfer(
        &pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: staging.clone(),
            fee: Some(candid::Nat::from(fee_e8s)),
            created_at_time: None,
            memo: None,
            amount: candid::Nat::from(staging_fund),
        },
    )?;

    debug_set_prev_age_seconds(&pic, disburser_canister, MAX_AGE_FOR_BONUS_SECS)?;

    let (_gross, planned) = expected_plan_payout_transfers(
        icrc1_balance(&pic, ledger, &staging)?,
        fee_e8s,
        MAX_AGE_FOR_BONUS_SECS,
        &normal,
        &bonus1,
        &bonus2,
    );
    if planned.len() != 3 {
        bail!("expected 3 planned transfers for staging_fund={staging_fund} fee={fee_e8s}, got {}", planned.len());
    }

    let b0_before = icrc1_balance(&pic, ledger, &normal)?;
    let b1_before = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_before = icrc1_balance(&pic, ledger, &bonus2)?;

    // Build the payout plan while the ledger is running (so fee/balance queries succeed),
    // but do not execute it yet. This lets us later simulate a ledger outage *after* planning,
    // and confirm the persisted plan survives the outage and completes on resume.
    let built: bool = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_build_payout_plan")?;
    if !built {
        bail!("expected debug_build_payout_plan to succeed");
    }

    let st0 = debug_state(&pic, disburser_canister)?;
    if !st0.payout_plan_present {
        bail!("expected payout plan to be present after planning");
    }

    // Stop the ledger before attempting payout so the balance/transfer calls reject deterministically.
    stop_canister_as(&pic, ledger, nns_root())?;

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 20);

    // IMPORTANT: Don't query the ledger while it's stopped — PocketIC rejects calls to a stopped
    // canister (including balance queries). Instead, assert that the disburser persisted the plan.
    // We verify that *no transfers landed during downtime* after we restart the ledger.

    // Plan should persist after failure.
    let st = debug_state(&pic, disburser_canister)?;
    if !st.payout_plan_present {
        bail!("expected payout plan to persist after ledger failure");
    }

    // Restart ledger and retry until the persisted plan completes and clears.
    start_canister_as(&pic, ledger, nns_root())?;

    // Now that the ledger is running again, verify nothing was transferred during downtime.
    let d0 = icrc1_balance(&pic, ledger, &normal)?.saturating_sub(b0_before);
    let d1 = icrc1_balance(&pic, ledger, &bonus1)?.saturating_sub(b1_before);
    let d2 = icrc1_balance(&pic, ledger, &bonus2)?.saturating_sub(b2_before);
    if d0 != 0 || d1 != 0 || d2 != 0 {
        bail!("expected no transfers while ledger was stopped, got [d0={d0}, d1={d1}, d2={d2}]");
    }

    let mut completed = false;
    for _ in 0..30 {
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 20);

        let b0_after = icrc1_balance(&pic, ledger, &normal)?;
        let b1_after = icrc1_balance(&pic, ledger, &bonus1)?;
        let b2_after = icrc1_balance(&pic, ledger, &bonus2)?;

        let d0 = b0_after.saturating_sub(b0_before);
        let d1 = b1_after.saturating_sub(b1_before);
        let d2 = b2_after.saturating_sub(b2_before);

        let st2 = debug_state(&pic, disburser_canister)?;
        if d0 == planned[0].amount_e8s
            && d1 == planned[1].amount_e8s
            && d2 == planned[2].amount_e8s
            && !st2.payout_plan_present
        {
            completed = true;
            break;
        }
    }

    if !completed {
        let b0_after = icrc1_balance(&pic, ledger, &normal)?;
        let b1_after = icrc1_balance(&pic, ledger, &bonus1)?;
        let b2_after = icrc1_balance(&pic, ledger, &bonus2)?;
        let d0 = b0_after.saturating_sub(b0_before);
        let d1 = b1_after.saturating_sub(b1_before);
        let d2 = b2_after.saturating_sub(b2_before);
        let st2 = debug_state(&pic, disburser_canister)?;
        bail!(
            "unexpected recipient deltas after resume: got [d0={d0}, d1={d1}, d2={d2}] expected [a0={}, a1={}, a2={}] plan_present={}",
            planned[0].amount_e8s,
            planned[1].amount_e8s,
            planned[2].amount_e8s,
            st2.payout_plan_present
        );
    }

    Ok(())

}

#[test]
#[ignore]
fn e2e_hotkey_only_cannot_disburse_maturity() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    // Neuron controller is *not* the disburser.
    let controller = Principal::anonymous();
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 4242, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // Add disburser as hotkey only (should still not allow disburse maturity).
    add_hotkey(&pic, gov, controller, neuron_id, disburser_canister)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: pic.create_canister(), subaccount: None },
        age_bonus_recipient_1: Account { owner: pic.create_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: pic.create_canister(), subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    let staging = Account { owner: disburser_canister, subaccount: None };
    let before = icrc1_balance(&pic, ledger, &staging)?;

    // Attempt initiation.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    // Should not observe in-flight since DisburseMaturity should be rejected.
    let (seen, _) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if seen {
        bail!("expected hotkey-only to be unable to initiate maturity disbursement, but in-flight was observed");
    }

    // And staging should not be credited later.
    advance_days(&pic, 8);
    let after = icrc1_balance(&pic, ledger, &staging)?;
    if after != before {
        bail!("expected staging balance to remain unchanged for hotkey-only case (before={before}, after={after})");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_blackhole_timers_only_progresses_pipeline() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 9090, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;

    let normal_owner = pic.create_canister();
    let bonus1_owner = pic.create_canister();
    let bonus2_owner = pic.create_canister();

    let normal = Account { owner: normal_owner, subaccount: None };
    let bonus1 = Account { owner: bonus1_owner, subaccount: None };
    let bonus2 = Account { owner: bonus2_owner, subaccount: None };

    let init = InitArg {
        neuron_id,
        normal_recipient: normal.clone(),
        age_bonus_recipient_1: bonus1.clone(),
        age_bonus_recipient_2: bonus2.clone(),
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        // Let timers drive.
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    // Blackhole posture: self-only controller.
    set_self_only_controller(&pic, disburser_canister)?;

    // (Do not force prev_age here; it is overwritten on initiation.)

    let staging = Account { owner: disburser_canister, subaccount: None };
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;

    let b0_before = icrc1_balance(&pic, ledger, &normal)?;
    let b1_before = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_before = icrc1_balance(&pic, ledger, &bonus2)?;

    // 1) Wait for in-flight (timers only).
    let mut inflight_seen = false;
    for _ in 0..600 {
        // Step time so the 60s main timer can fire, then progress rounds.
        advance_and_tick(&pic, 61, 20);
        let n = get_full_neuron(&pic, gov, controller, neuron_id)?;
        inflight_seen = n
            .maturity_disbursements_in_progress
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        if inflight_seen {
            break;
        }
    }
    if !inflight_seen {
        bail!("expected in-flight maturity disbursement to become visible via timers");
    }

    let plan_age = debug_state(&pic, disburser_canister)?.prev_age_seconds;

    // 2) Wait for staging credit after finalization window.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    if staging_after <= staging_before {
        bail!("expected staging to increase via timers (before={staging_before}, after={staging_after})");
    }

    // 3) Wait for payout transfers (timers only).
    start_canister_as(&pic, disburser_canister, disburser_canister)?;
    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let (_gross, planned) = expected_plan_payout_transfers(
        staging_after,
        fee_e8s,
        plan_age,
        &normal,
        &bonus1,
        &bonus2,
    );
    if planned.len() != 3 {
        bail!("expected 3 planned transfers for timer-driven payout, got {}", planned.len());
    }

    let mut paid = false;
    for _ in 0..800 {
        advance_and_tick(&pic, 61, 20);

        let b0 = icrc1_balance(&pic, ledger, &normal)?.saturating_sub(b0_before);
        let b1 = icrc1_balance(&pic, ledger, &bonus1)?.saturating_sub(b1_before);
        let b2 = icrc1_balance(&pic, ledger, &bonus2)?.saturating_sub(b2_before);

        let st = debug_state(&pic, disburser_canister)?;

        if b0 == planned[0].amount_e8s
            && b1 == planned[1].amount_e8s
            && b2 == planned[2].amount_e8s
            && !st.payout_plan_present
        {
            paid = true;
            break;
        }
    }

    if !paid {
        let b0 = icrc1_balance(&pic, ledger, &normal)?.saturating_sub(b0_before);
        let b1 = icrc1_balance(&pic, ledger, &bonus1)?.saturating_sub(b1_before);
        let b2 = icrc1_balance(&pic, ledger, &bonus2)?.saturating_sub(b2_before);
        let st = debug_state(&pic, disburser_canister)?;
        bail!(
            "timer-driven payout did not complete as expected: deltas [b0={b0}, b1={b1}, b2={b2}] plan_present={}",
            st.payout_plan_present
        );
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_rescue_controller_roundtrip_real_management_canister() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let controller = Principal::anonymous();
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 7, 1_000 * 100_000_000)?; // 1,000 ICP
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    let wasm = build_disburser_wasm()?;
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: controller,
        blackhole_armed: Some(true),
        // Keep timers effectively disabled in e2e (we drive execution manually).
        main_interval_seconds: Some(365 * 24 * 60 * 60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    // Start fully blackholed (self-only controller).
    set_self_only_controller(&pic, disburser_canister)?;
    let c0 = pic.get_controllers(disburser_canister);
    if c0 != vec![disburser_canister] {
        bail!("expected self-only controller at start, got {:?}", c0);
    }

    // Simulate broken: last_successful_transfer_ts far in the past.
    let now_secs = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    let old = now_secs.saturating_sub(30 * 24 * 60 * 60);

    let _: () = update_call(
        &pic,
        disburser_canister,
        Principal::anonymous(),
        "debug_set_last_successful_transfer_ts",
        Some(old),
    )?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_rescue_tick")?;

    let c1 = pic.get_controllers(disburser_canister);
    if !(c1.contains(&disburser_canister) && c1.contains(&controller) && c1.len() == 2) {
        bail!("expected controllers=[self,rescue], got {:?}", c1);
    }

    // Now simulate healthy: recent successful transfer => must re-blackhole to self-only.
    let _: () = update_call(
        &pic,
        disburser_canister,
        Principal::anonymous(),
        "debug_set_last_successful_transfer_ts",
        Some(now_secs),
    )?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_rescue_tick")?;

    let c2 = pic.get_controllers(disburser_canister);
    if c2 != vec![disburser_canister] {
        bail!("expected controllers to return to self-only after healthy tick, got {:?}", c2);
    }

    Ok(())
}


#[test]
#[ignore]
fn e2e_blackhole_does_not_reconcile_when_unarmed() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let controller = Principal::anonymous();
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 72, 1_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    let wasm = build_disburser_wasm()?;
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: controller,
        blackhole_armed: Some(false),
        main_interval_seconds: Some(365 * 24 * 60 * 60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    set_controllers_exact(&pic, disburser_canister, vec![disburser_canister, controller])?;
    let before = pic.get_controllers(disburser_canister);
    if !(before.contains(&disburser_canister) && before.contains(&controller) && before.len() == 2) {
        bail!("expected initial controllers=[self,rescue], got {:?}", before);
    }

    let now_secs = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    let _: () = update_call(
        &pic,
        disburser_canister,
        Principal::anonymous(),
        "debug_set_last_successful_transfer_ts",
        Some(now_secs),
    )?;

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_rescue_tick")?;

    let after = pic.get_controllers(disburser_canister);
    if after != before {
        bail!(
            "expected controllers to remain unchanged while blackhole_armed=false; before={:?} after={:?}",
            before,
            after
        );
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_bootstrap_rescue_fires_before_first_successful_payout() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let controller = Principal::anonymous();
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 71, 1_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    let wasm = build_disburser_wasm()?;
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: controller,
        blackhole_armed: Some(true),
        main_interval_seconds: Some(365 * 24 * 60 * 60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);

    set_self_only_controller(&pic, disburser_canister)?;
    let c0 = pic.get_controllers(disburser_canister);
    if c0 != vec![disburser_canister] {
        bail!("expected self-only controller at start, got {:?}", c0);
    }

    // No successful payout has ever been recorded.
    let _: () = update_call(
        &pic,
        disburser_canister,
        Principal::anonymous(),
        "debug_set_last_successful_transfer_ts",
        None::<u64>,
    )?;

    pic.advance_time(Duration::from_secs(30 * DAY_SECS));
    pic.tick();

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_rescue_tick")?;

    let c1 = pic.get_controllers(disburser_canister);
    if !(c1.contains(&disburser_canister) && c1.contains(&controller) && c1.len() == 2) {
        bail!(
            "expected bootstrap rescue to widen controllers before first successful payout; got {:?}",
            c1
        );
    }

    let dbg: DebugState = query_call(&pic, disburser_canister, Principal::anonymous(), "debug_state", ())?;
    if !dbg.rescue_triggered || dbg.forced_rescue_reason != Some(ForcedRescueReason::BootstrapNoSuccess) {
        bail!("expected bootstrap forced rescue to latch before first successful payout, got {:?}", dbg);
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_maturity_to_staging_then_transfers_real_ledger() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    // Set up disburser canister.
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    // IMPORTANT: the disburser must be the neuron controller for DisburseMaturity to succeed.
    let controller = disburser_canister;

    // Stake + claim + configure neuron.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 100, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // Create three distinct recipient principals.
    let r_normal = pic.create_canister();
    let r_bonus1 = pic.create_canister();
    let r_bonus2 = pic.create_canister();

    let wasm = build_disburser_wasm()?;

    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: r_normal, subaccount: None },
        age_bonus_recipient_1: Account { owner: r_bonus1, subaccount: None },
        age_bonus_recipient_2: Account { owner: r_bonus2, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        // Disable suppression to make explicit ticks deterministic.
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init.clone())?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let staging = Account { owner: disburser_canister, subaccount: None };

    let staging_before = icrc1_balance(&pic, ledger, &staging)?;
    let rn_before = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    if staging_before != 0 {
        bail!("expected staging to be empty at start, got {staging_before}");
    }

    // 1) Initiate maturity disbursement.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    // 2) REQUIRED: observe in-flight disbursement.
    let (seen, n_inflight) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!(
            "expected in-flight maturity disbursement to be observable, but it was not; disbursements={:?}",
            n_inflight.maturity_disbursements_in_progress
        );
    }

    // 3) Advance beyond the finalization window and assert staging credit.
    // Stop the disburser so we don't accidentally drain staging during the same fast-forward.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    if staging_after <= staging_before {
        bail!("expected staging to increase after finalization, before={staging_before} after={staging_after}");
    }

    // Ensure in-flight is now cleared (or at least empty).
    let n_post = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let inflight_now = n_post
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if inflight_now {
        bail!("expected no in-flight disbursement after finalization, got {:?}", n_post.maturity_disbursements_in_progress);
    }

    // 4) Deterministic payout split: force prev_age_seconds for plan creation.
    start_canister_as(&pic, disburser_canister, disburser_canister)?;

    let forced_age = MAX_AGE_FOR_BONUS_SECS;
    debug_set_prev_age_seconds(&pic, disburser_canister, forced_age)?;

    let (_gross, planned) = expected_plan_payout_transfers(
        staging_after,
        fee_e8s,
        forced_age,
        &init.normal_recipient,
        &init.age_bonus_recipient_1,
        &init.age_bonus_recipient_2,
    );
    let expected_gross_sent: u64 = planned.iter().map(|p| p.gross_share_e8s).sum();

    // 5) Run payout stage (and initiate the next maturity disbursement).
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    // 6) Assert ledger effects (real ICP ledger semantics).
    let rn_after = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    for p in &planned {
        let (before, after) = if p.to == init.normal_recipient {
            (rn_before, rn_after)
        } else if p.to == init.age_bonus_recipient_1 {
            (rb1_before, rb1_after)
        } else if p.to == init.age_bonus_recipient_2 {
            (rb2_before, rb2_after)
        } else {
            bail!("planned transfer targets unknown account: {:?}", p.to);
        };

        let got = after.saturating_sub(before);
        if got != p.amount_e8s {
            bail!(
                "recipient balance mismatch for {:?}: expected +{}, got +{}",
                p.to,
                p.amount_e8s,
                got
            );
        }
    }

    // Staging should decrease by the total *gross* of executed transfers.
    let staging_post_payout = icrc1_balance(&pic, ledger, &staging)?;
    let expected_staging_post = staging_after.saturating_sub(expected_gross_sent);
    if staging_post_payout != expected_staging_post {
        bail!(
            "staging mismatch after payout: expected {}, got {} (after_finalization={})",
            expected_staging_post,
            staging_post_payout,
            staging_after
        );
    }

    // Plan should be cleared after success.
    let dbg = debug_state(&pic, disburser_canister)?;
    if dbg.payout_plan_present {
        bail!("expected payout plan to be cleared after success");
    }

    Ok(())
}


#[test]
#[ignore]
fn e2e_partial_execution_retry_duplicate_proof() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let controller = disburser_canister;

    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 105, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // Recipients.
    let r_normal = pic.create_canister();
    let r_bonus1 = pic.create_canister();
    let r_bonus2 = pic.create_canister();

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: r_normal, subaccount: None },
        age_bonus_recipient_1: Account { owner: r_bonus1, subaccount: None },
        age_bonus_recipient_2: Account { owner: r_bonus2, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init.clone())?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let staging = Account { owner: disburser_canister, subaccount: None };

    // --- Get staging funded via real NNS maturity disbursement ---
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    let (seen, _n) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!("expected in-flight maturity disbursement to be observable");
    }

    // Stop the disburser so staging cannot be drained while we fast-forward.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    if staging_after <= staging_before {
        bail!("expected staging to be credited");
    }

    // Ensure in-flight is cleared before payout.
    let n_post = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let inflight_now = n_post
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if inflight_now {
        bail!("expected no in-flight disbursement before payout");
    }

    // Deterministic plan.
    start_canister_as(&pic, disburser_canister, disburser_canister)?;
    let forced_age = MAX_AGE_FOR_BONUS_SECS;
    debug_set_prev_age_seconds(&pic, disburser_canister, forced_age)?;

    let (_gross, planned) = expected_plan_payout_transfers(
        staging_after,
        fee_e8s,
        forced_age,
        &init.normal_recipient,
        &init.age_bonus_recipient_1,
        &init.age_bonus_recipient_2,
    );
    let expected_gross_sent: u64 = planned.iter().map(|p| p.gross_share_e8s).sum();

    let rn_before = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    // 1) Persist plan but do NOT execute transfers (debug-only pause after planning).
    debug_set_pause_after_planning(&pic, disburser_canister, true)?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    let dbg = debug_state(&pic, disburser_canister)?;
    if !dbg.payout_plan_present {
        bail!("expected payout plan to be present after planning pause");
    }

    // 2) Execute transfers, but trap after the first successful transfer reply.
    debug_set_pause_after_planning(&pic, disburser_canister, false)?;
    // Execute transfers, but abort after the first successful transfer reply.
    debug_set_trap_after_successful_transfers(&pic, disburser_canister, Some(1))?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    // We should have observed at least one transfer landing (the first one), while the plan remains present.
    let rn_mid = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let d0_mid = rn_mid.saturating_sub(rn_before);
    if d0_mid == 0 {
        bail!("expected at least one transfer to land before injected abort");
    }
    let dbg_mid = debug_state(&pic, disburser_canister)?;
    if !dbg_mid.payout_plan_present {
        bail!("expected payout plan to remain present after injected abort");
    }

    // 3) Clear the abort and retry: the already-executed transfer must be treated as Duplicate (exactly-once),
    // and the plan must complete.
    debug_set_trap_after_successful_transfers(&pic, disburser_canister, None)?;

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    // Assert recipients received exactly once.
    let rn_after = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    for p in &planned {
        let (before, after) = if p.to == init.normal_recipient {
            (rn_before, rn_after)
        } else if p.to == init.age_bonus_recipient_1 {
            (rb1_before, rb1_after)
        } else if p.to == init.age_bonus_recipient_2 {
            (rb2_before, rb2_after)
        } else {
            bail!("planned transfer targets unknown account: {:?}", p.to);
        };

        let got = after.saturating_sub(before);
        if got != p.amount_e8s {
            bail!(
                "recipient balance mismatch for {:?}: expected +{}, got +{}",
                p.to,
                p.amount_e8s,
                got
            );
        }
    }

    // Staging decreases by the gross of executed transfers.
    let staging_post = icrc1_balance(&pic, ledger, &staging)?;
    let expected_staging_post = staging_after.saturating_sub(expected_gross_sent);
    if staging_post != expected_staging_post {
        bail!("staging mismatch after retry: expected {expected_staging_post}, got {staging_post}");
    }

    let dbg2 = debug_state(&pic, disburser_canister)?;
    if dbg2.payout_plan_present {
        bail!("expected payout plan to be cleared after successful retry");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_long_downtime_catchup_does_not_double_initiate() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    // Real neuron so get_full_neuron works.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 77, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;

    let normal_owner = pic.create_canister();
    let bonus1_owner = pic.create_canister();
    let bonus2_owner = pic.create_canister();

    let normal = Account { owner: normal_owner, subaccount: None };
    let bonus1 = Account { owner: bonus1_owner, subaccount: None };
    let bonus2 = Account { owner: bonus2_owner, subaccount: None };

    let init = InitArg {
        neuron_id,
        normal_recipient: normal.clone(),
        age_bonus_recipient_1: bonus1.clone(),
        age_bonus_recipient_2: bonus2.clone(),
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    // Simulate long downtime (no timers fire while stopped).
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 60);
    start_canister_as(&pic, disburser_canister, disburser_canister)?;

    let staging = Account { owner: disburser_canister, subaccount: None };
    let staging_before_initiation = icrc1_balance(&pic, ledger, &staging)?;

    // First tick should initiate at most one disbursement (no "catch-up loop").
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    let (seen, n1) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!(
            "expected in-flight disbursement to become observable after long downtime; maturity={} disbursements={:?}",
            n1.maturity_e8s_equivalent,
            n1.maturity_disbursements_in_progress
        );
    }
    let len1 = n1.maturity_disbursements_in_progress.as_ref().map(|v| v.len()).unwrap_or(0);
    if len1 > 1 {
        bail!("expected <=1 in-flight disbursement after one tick; got {len1}");
    }

    // Second tick must be a no-op (still exactly one in-flight).
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let n2 = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let len2 = n2.maturity_disbursements_in_progress.as_ref().map(|v| v.len()).unwrap_or(0);
    if len2 != len1 {
        bail!("expected in-flight disbursement count stable across ticks; before={len1} after={len2}");
    }

    // Prevent staging from being drained automatically while we fast-forward finalization.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 8);
    start_canister_as(&pic, disburser_canister, disburser_canister)?;

    // Wait until the NNS finalization actually credits staging (robust signal).
    let credited = wait_for_staging_credit(&pic, ledger, &staging, staging_before_initiation)?;
    e2e_log!("staging credited={credited}");

    // Recipients before payout.
    let b0_before = icrc1_balance(&pic, ledger, &normal)?;
    let b1_before = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_before = icrc1_balance(&pic, ledger, &bonus2)?;

    // Run payout.
    tick_n(&pic, 30);
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 50);

    let b0_after = icrc1_balance(&pic, ledger, &normal)?;
    let b1_after = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_after = icrc1_balance(&pic, ledger, &bonus2)?;

    if b0_after <= b0_before && b1_after <= b1_before && b2_after <= b2_before {
        bail!("expected at least one recipient to receive funds after catch-up payout");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_simulated_low_cycles_fails_closed_and_recovers() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    // Need a real neuron so get_full_neuron succeeds; no maturity accrual needed.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 88, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    let wasm = build_disburser_wasm()?;

    let normal_owner = pic.create_canister();
    let bonus1_owner = pic.create_canister();
    let bonus2_owner = pic.create_canister();

    let normal = Account { owner: normal_owner, subaccount: None };
    let bonus1 = Account { owner: bonus1_owner, subaccount: None };
    let bonus2 = Account { owner: bonus2_owner, subaccount: None };

    let init = InitArg {
        neuron_id,
        normal_recipient: normal.clone(),
        age_bonus_recipient_1: bonus1.clone(),
        age_bonus_recipient_2: bonus2.clone(),
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    // Payout-only mode for this test.
    debug_set_skip_maturity_initiation(&pic, disburser_canister, true)?;

    // Make sure we produce a 3-transfer plan by setting max age.
    debug_set_prev_age_seconds(&pic, disburser_canister, 4 * 365 * DAY_SECS)?;

    let staging = Account { owner: disburser_canister, subaccount: None };

    // Credit staging.
    let fee = icrc1_fee(&pic, ledger)?;
    let amount_e8s = 10 * 100_000_000u64;
    icrc1_transfer(
        &pic,
        ledger,
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: staging.clone(),
            fee: Some(candid::Nat::from(fee)),
            created_at_time: None,
            memo: None,
            amount: candid::Nat::from(amount_e8s),
        },
    )?;

    let staging_before = icrc1_balance(&pic, ledger, &staging)?;

    // Simulate low cycles: tick must refuse to do anything (fail closed).
    debug_set_simulate_low_cycles(&pic, disburser_canister, true)?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);

    let staging_after = icrc1_balance(&pic, ledger, &staging)?;
    if staging_after != staging_before {
        bail!("expected staging unchanged under simulated low cycles; before={staging_before} after={staging_after}");
    }

    // Recover: disable flag and tick again; payout should proceed.
    debug_set_simulate_low_cycles(&pic, disburser_canister, false)?;
    let b0_before = icrc1_balance(&pic, ledger, &normal)?;
    let b1_before = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_before = icrc1_balance(&pic, ledger, &bonus2)?;

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 50);

    let b0_after = icrc1_balance(&pic, ledger, &normal)?;
    let b1_after = icrc1_balance(&pic, ledger, &bonus1)?;
    let b2_after = icrc1_balance(&pic, ledger, &bonus2)?;

    if b0_after <= b0_before && b1_after <= b1_before && b2_after <= b2_before {
        bail!("expected at least one recipient to increase after recovery");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_state_size_does_not_grow_unbounded_under_repeated_payouts() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    // Real neuron so get_full_neuron succeeds; no maturity accrual needed.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 99, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    let wasm = build_disburser_wasm()?;

    let normal_owner = pic.create_canister();
    let bonus1_owner = pic.create_canister();
    let bonus2_owner = pic.create_canister();

    let normal = Account { owner: normal_owner, subaccount: None };
    let bonus1 = Account { owner: bonus1_owner, subaccount: None };
    let bonus2 = Account { owner: bonus2_owner, subaccount: None };

    let init = InitArg {
        neuron_id,
        normal_recipient: normal.clone(),
        age_bonus_recipient_1: bonus1.clone(),
        age_bonus_recipient_2: bonus2.clone(),
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    // Payout-only mode: we do not want the test to be gated on 7-day maturity finalization.
    debug_set_skip_maturity_initiation(&pic, disburser_canister, true)?;
    debug_set_prev_age_seconds(&pic, disburser_canister, 4 * 365 * DAY_SECS)?;

    let staging = Account { owner: disburser_canister, subaccount: None };
    let fee = icrc1_fee(&pic, ledger)?;

    let mut last_size = debug_state_size_bytes(&pic, disburser_canister)?;
    e2e_log!("initial debug_state_size_bytes={last_size}");

    for i in 0..20u64 {
        // Credit staging with 10 ICP.
        icrc1_transfer(
            &pic,
            ledger,
            Principal::anonymous(),
            TransferArg {
                from_subaccount: None,
                to: staging.clone(),
                fee: Some(candid::Nat::from(fee)),
                created_at_time: None,
                memo: None,
                amount: candid::Nat::from(10 * 100_000_000u64),
            },
        )?;

        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 50);

        let size = debug_state_size_bytes(&pic, disburser_canister)?;
        e2e_log!("state size after payout {i} => {size}");

        // The stable state is designed to be O(1) bounded (no historical accumulation).
        if size > 80_000 {
            bail!("state size grew unexpectedly large: {size} bytes");
        }

        // Allow small encoding variance, but forbid monotonically increasing growth.
        if size > last_size + 2_000 {
            bail!("state size increased too much between iterations: prev={last_size} now={size}");
        }
        last_size = size;
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_inflight_idempotent_under_repeated_ticks() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let controller = disburser_canister;
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 101, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    // Initiate disbursement.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    // Wait until in-flight is observable.
    let (seen, n1) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!("expected in-flight disbursement to be observable");
    }

    let disb_1 = n1
        .maturity_disbursements_in_progress
        .as_ref()
        .and_then(|v| v.first().cloned())
        .ok_or_else(|| anyhow!("inflight vector unexpectedly empty"))?;

    // Re-run main tick multiple times while still in-flight.
    for _ in 0..5 {
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&pic, 3);

        let n = get_full_neuron(&pic, gov, controller, neuron_id)?;
        let v = n
            .maturity_disbursements_in_progress
            .as_ref()
            .ok_or_else(|| anyhow!("expected maturity_disbursements_in_progress to be Some"))?;
        if v.len() != 1 {
            bail!("expected exactly 1 in-flight disbursement, got {}", v.len());
        }

        // Best-effort invariant: initiation timestamp should not change.
        let d = v[0].clone();
        if d.timestamp_of_disbursement_seconds != disb_1.timestamp_of_disbursement_seconds {
            bail!(
                "in-flight disbursement changed unexpectedly: was {:?}, now {:?}",
                disb_1.timestamp_of_disbursement_seconds,
                d.timestamp_of_disbursement_seconds
            );
        }
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_upgrade_persists_inflight() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let controller = disburser_canister;
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 102, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm.clone(), encode_one(init)?, None);

    // Self-only controller so we can stop/start using the canister principal.
    set_self_only_controller(&pic, disburser_canister)?;


    // Initiate disbursement.
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    // Wait until in-flight is observable.
    let (seen, n1) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!("expected in-flight disbursement before upgrade");
    }

    let dbg_before = debug_state(&pic, disburser_canister)?;

    // Upgrade the canister (stable memory roundtrip).
    pic.upgrade_canister(
        disburser_canister,
        wasm,
        encode_one(())?,
        Some(disburser_canister),
    )
    .map_err(|e| anyhow!("upgrade_canister reject: {:?}", e))?;

    tick_n(&pic, 5);

    // State should persist.
    let dbg_after = debug_state(&pic, disburser_canister)?;
    if dbg_after.prev_age_seconds != dbg_before.prev_age_seconds {
        bail!(
            "prev_age_seconds changed across upgrade: {} -> {}",
            dbg_before.prev_age_seconds,
            dbg_after.prev_age_seconds
        );
    }

    // In-flight should still be visible; upgrade must not cause a double initiation.
    let n2 = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let inflight = n2
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if !inflight {
        bail!("expected in-flight disbursement to remain visible after upgrade");
    }

    // Re-run main tick; should remain a no-op (still in-flight).
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 3);
    let n3 = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let len3 = n3
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0);
    if len3 != n1
        .maturity_disbursements_in_progress
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0)
    {
        bail!("expected no change in disbursement count across upgrade+tick");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_blackhole_smoke_timers_only() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let controller = disburser_canister;
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 104, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // Recipients.
    let r1 = pic.create_canister();
    let r2 = pic.create_canister();
    let r3 = pic.create_canister();

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: r1, subaccount: None },
        age_bonus_recipient_1: Account { owner: r2, subaccount: None },
        age_bonus_recipient_2: Account { owner: r3, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        // Timers enabled (short); we will not call debug_main_tick.
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init.clone())?, None);

    // Blackhole posture: self-only controllers.
    set_self_only_controller(&pic, disburser_canister)?;

    let staging = Account { owner: disburser_canister, subaccount: None };
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;

    // Drive time forward to let the timer fire main_tick.
    // (Timer interval is 60s, but advance a bit more for safety.)
    advance_and_tick(&pic, 75, 30);

    // REQUIRED: observe in-flight (signals scheduler behavior).
    let (seen, _n) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!("expected timer-driven in-flight disbursement to be observable");
    }

    // Finalize maturity disbursement.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    if staging_after <= staging_before {
        bail!("expected staging to be credited after timer-driven disbursement");
    }

    // Let the next timer tick process payout.
    start_canister_as(&pic, disburser_canister, disburser_canister)?;
    let r1_before = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let r2_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let r3_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    advance_and_tick(&pic, 75, 20);

    let r1_after = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let r2_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let r3_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    let any_paid = (r1_after > r1_before) || (r2_after > r2_before) || (r3_after > r3_before);
    if !any_paid {
        bail!("expected at least one recipient to receive funds via timer-driven payout");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_age_bonus_routes_incremental_rewards_to_bonus_accounts() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    // Disburser canister (also the neuron controller).
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    // Stake + claim + configure neuron.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 909, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;

    // Make rewards large enough that rounding noise is negligible.
    // (This is the same magnitude used elsewhere to get deterministic maturity accrual.)
    top_up_neuron_stake_and_refresh(&pic, ledger, gov, controller, neuron_id, 5_000_000 * 100_000_000)?;

    // Recipients.
    let r_normal = pic.create_canister();
    let r_bonus1 = pic.create_canister();
    let r_bonus2 = pic.create_canister();

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: r_normal, subaccount: None },
        age_bonus_recipient_1: Account { owner: r_bonus1, subaccount: None },
        age_bonus_recipient_2: Account { owner: r_bonus2, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        // Drive manually for determinism.
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init.clone())?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let staging = Account { owner: disburser_canister, subaccount: None };

    #[derive(Clone, Debug)]
    struct Cycle {
        total_disbursed_e8s: u64,
        age_seconds_used: u64,
bonus_gross_e8s: u64,
bonus1_net_e8s: u64,
        bonus2_net_e8s: u64,
    }

    let run_cycle = |proposal_seq: u64| -> Result<Cycle> {
        // Ensure clean staging.
        let staging_before = icrc1_balance(&pic, ledger, &staging)?;
        if staging_before != 0 {
            bail!("expected empty staging at cycle start, got {staging_before}");
        }

        // Snapshot recipient balances.
        let rn_before = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
        let rb1_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
        let rb2_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

        // Trigger voting rewards (maturity increases as a function of voting power, which includes age bonus).
        let _pid = make_and_settle_motion_proposal(
            &pic,
            gov,
            controller,
            neuron_id,
            "E2E age-bonus reward trigger",
            proposal_seq,
        )?;

        // Initiate maturity disbursement via the disburser.
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

        // REQUIRED: observe in-flight.
        let (seen, _n_inflight) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
        if !seen {
            bail!("expected in-flight maturity disbursement to be observable");
        }

        // Capture the exact age the disburser will use for the payout split.
        let dbg = debug_state(&pic, disburser_canister)?;
        let age_used = dbg.prev_age_seconds;

        // Finalize maturity (mint to staging). Stop disburser during the fast-forward so we don't drain staging.
        stop_canister_as(&pic, disburser_canister, disburser_canister)?;
        advance_days(&pic, 8);
        let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
        if staging_after <= staging_before {
            bail!("expected staging to increase after finalization");
        }

        start_canister_as(&pic, disburser_canister, disburser_canister)?;

        // Compute expected split for *this* cycle using the age captured at initiation time.
        let (gross, planned) = expected_plan_payout_transfers(
            staging_after,
            fee_e8s,
            age_used,
            &init.normal_recipient,
            &init.age_bonus_recipient_1,
            &init.age_bonus_recipient_2,
        );
        if planned.len() != 3 {
            bail!(
                "expected 3 transfers (normal+bonus80+bonus20) for staging_after={staging_after}, fee={fee_e8s}, age={age_used}; got {}",
                planned.len()
            );
        }
        // Execute payout (manual tick). Timers are disabled for determinism, so we may need
        // multiple manual ticks to move through (build plan) -> (execute transfers).
        let (normal_delta, b1_delta, b2_delta) = {
            let mut out: Option<(u64, u64, u64)> = None;

            for attempt in 1..=3u32 {
                let _: () = update_noargs(
                    &pic,
                    disburser_canister,
                    Principal::anonymous(),
                    "debug_main_tick",
                )?;
                tick_n(&pic, 10);

                let rn_after = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
                let rb1_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
                let rb2_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

                let normal_delta = rn_after.saturating_sub(rn_before);
                let b1_delta = rb1_after.saturating_sub(rb1_before);
                let b2_delta = rb2_after.saturating_sub(rb2_before);

                if normal_delta > 0 || b1_delta > 0 || b2_delta > 0 {
                    out = Some((normal_delta, b1_delta, b2_delta));
                    break;
                }

                if attempt == 3 {
                    let dbg_now = debug_state(&pic, disburser_canister)?;
                    let snap_n = neuron_snapshot(&pic, gov, controller, neuron_id, "no-payout")?;
                    let staging_post = icrc1_balance(&pic, ledger, &staging)?;
                    bail!(
                        "payout did not move any recipient balances after {attempt} manual ticks; expected planned transfers to execute; staging_after={staging_after} staging_post={staging_post} fee={fee_e8s} age_used={age_used}; disburser_dbg={dbg_now:?}; {snap_n}",
                    );
                }
            }

            out.expect("loop must either produce deltas or bail on final attempt")
        };
        
                // Verify the exact split landed on the real ledger.
        for p in &planned {
            let got = if p.to == init.normal_recipient {
                normal_delta
            } else if p.to == init.age_bonus_recipient_1 {
                b1_delta
            } else if p.to == init.age_bonus_recipient_2 {
                b2_delta
            } else {
                bail!("planned transfer targets unknown account: {:?}", p.to);
            };

            if got != p.amount_e8s {
                bail!(
                    "recipient balance mismatch for {:?}: expected +{}, got +{}",
                    p.to,
                    p.amount_e8s,
                    got
                );
            }
        }

        // Staging should be fully drained by the gross shares (i.e. end at 0 when all 3 transfers executed).
        let staging_post = icrc1_balance(&pic, ledger, &staging)?;
        let gross_sent: u64 = planned.iter().map(|p| p.gross_share_e8s).sum();
        let expected_staging_post = staging_after.saturating_sub(gross_sent);
        if staging_post != expected_staging_post {
            bail!(
                "staging mismatch after payout: expected {expected_staging_post}, got {staging_post}"
            );
        }

        // Plan should be cleared after success.
        let dbg2 = debug_state(&pic, disburser_canister)?;
        if dbg2.payout_plan_present {
            bail!("expected payout plan to be cleared after success");
        }

        Ok(Cycle {
            total_disbursed_e8s: staging_after,
            age_seconds_used: age_used,
bonus_gross_e8s: gross.bonus80_e8s + gross.bonus20_e8s,
bonus1_net_e8s: b1_delta,
            bonus2_net_e8s: b2_delta,
        })
    };

    // Run two reward+disburse+payout cycles. Between cycles, the neuron's stake stays constant but its age increases.
    let c1 = run_cycle(9_001)?;
    let c2 = run_cycle(9_002)?;

    // The key property we want to observe in a *real* NNS+ledger environment:
    //   - total maturity disbursed increases as the neuron ages (because voting power increases),
    //   - the "base" portion (what you'd get with no age bonus) remains approximately stable,
    //   - the additional amount shows up in the bonus recipients.

    if c2.total_disbursed_e8s <= c1.total_disbursed_e8s {
        bail!(
            "expected total disbursed maturity to increase with age (cycle1={}@age={}, cycle2={}@age={})",
            c1.total_disbursed_e8s,
            c1.age_seconds_used,
            c2.total_disbursed_e8s,
            c2.age_seconds_used
        );
    }

    // Bonus should increase with age (fraction increases and total is non-decreasing in realistic settings).
    if c2.bonus_gross_e8s <= c1.bonus_gross_e8s {
        bail!(
            "expected bonus gross to increase with age (bonus1={}, bonus2={}); totals [t1={}, t2={}]",
            c1.bonus_gross_e8s,
            c2.bonus_gross_e8s,
            c1.total_disbursed_e8s,
            c2.total_disbursed_e8s
        );
    }

    // On-ledger, the bonus recipients should see larger combined net amounts in later cycles.
    if (c2.bonus1_net_e8s + c2.bonus2_net_e8s) <= (c1.bonus1_net_e8s + c1.bonus2_net_e8s) {
        bail!(
            "expected total bonus net to increase (b1_net={} b2_net={})",
            c1.bonus1_net_e8s + c1.bonus2_net_e8s,
            c2.bonus1_net_e8s + c2.bonus2_net_e8s
        );
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_age_bonus_scales_at_2y_and_clamps_at_4y() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    // Disburser canister (also the neuron controller).
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    // Stake + claim + configure neuron.
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 911, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    top_up_neuron_stake_and_refresh(&pic, ledger, gov, controller, neuron_id, 5_000_000 * 100_000_000)?;

    let r_normal = pic.create_canister();
    let r_bonus1 = pic.create_canister();
    let r_bonus2 = pic.create_canister();

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: r_normal, subaccount: None },
        age_bonus_recipient_1: Account { owner: r_bonus1, subaccount: None },
        age_bonus_recipient_2: Account { owner: r_bonus2, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * 24 * 60 * 60),
    };

    let fee_e8s = icrc1_fee(&pic, ledger)?;
    let staging = Account { owner: disburser_canister, subaccount: None };

    const YEAR_SECS: u64 = 365 * DAY_SECS;
    const TWO_YEARS: u64 = 2 * YEAR_SECS;
    const FOUR_YEARS: u64 = 4 * YEAR_SECS;
    const FIVE_YEARS: u64 = 5 * YEAR_SECS;

    fn now_secs(pic: &PocketIc) -> u64 {
        (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64
    }

    fn neuron_age_secs(pic: &PocketIc, gov: Principal, controller: Principal, neuron_id: u64) -> Result<u64> {
        let n = get_full_neuron(pic, gov, controller, neuron_id)?;
        let now = now_secs(pic);
        Ok(now.saturating_sub(n.aging_since_timestamp_seconds))
    }

    fn advance_neuron_age_to(
        pic: &PocketIc,
        gov: Principal,
        controller: Principal,
        neuron_id: u64,
        target_age_secs: u64,
    ) -> Result<()> {
        let age = neuron_age_secs(pic, gov, controller, neuron_id)?;
        if age >= target_age_secs {
            return Ok(());
        }
        let delta = target_age_secs - age;

        // Coarse stepping keeps timer backlogs manageable without being slow for multi-year jumps.
        advance_time_steps(pic, delta, 7 * DAY_SECS, 5);
        Ok(())
    }

    fn expected_base_for_age(total_e8s: u64, age_secs: u64) -> u64 {
        let den: u128 = (16 * YEAR_SECS) as u128;
        let bonus_secs: u128 = (age_secs.min(FOUR_YEARS)) as u128;
        let num: u128 = den + bonus_secs;
        ((total_e8s as u128) * den / num) as u64
    }

    fn pct_bp(numer: u64, denom: u64) -> u64 {
        if denom == 0 {
            0
        } else {
            ((numer as u128) * 10_000u128 / (denom as u128)) as u64
        }
    }

    fn assert_eq_with_diff(label: &str, expected: u64, got: u64) -> Result<()> {
        if expected == got {
            return Ok(());
        }
        let diff = if expected > got { expected - got } else { got - expected };
        let diff_bp = pct_bp(diff, expected.max(1));
        bail!("{label}: expected={expected}, got={got}, diff={diff} e8s ({diff_bp} bp)");
    }

    // Ensure we have enough maturity to disburse. This may advance time.
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    // ----------------- Check at ~2 years -----------------
    // Set age precisely, then initiate disbursement (no time advances between).
    advance_neuron_age_to(&pic, gov, controller, neuron_id, TWO_YEARS)?;
    let age_pre = neuron_age_secs(&pic, gov, controller, neuron_id)?;
    let pre_diff = if age_pre > TWO_YEARS { age_pre - TWO_YEARS } else { TWO_YEARS - age_pre };
    if pre_diff > 60 {
        bail!("2y checkpoint: expected pre-age to be ~2 years; age_pre={age_pre} (diff {pre_diff}s)");
    }

    pic.install_canister(disburser_canister, wasm, encode_one(init.clone())?, None);
    set_self_only_controller(&pic, disburser_canister)?;
    
    // Initiate maturity disbursement via the disburser. In PocketIC, the in-flight entry may take
    // a couple of ticks to surface, so we retry a few times.
    let mut seen = false;
    let mut last = get_full_neuron(&pic, gov, controller, neuron_id)?;
    for _attempt in 1..=3u32 {
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        let (s, l) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
        seen = s;
        last = l;
        if seen {
            break;
        }
    }
    if !seen {
        let snap = neuron_snapshot(&pic, gov, controller, neuron_id, "no-inflight-2y")?;
        let dbg = debug_state(&pic, disburser_canister)?;
        bail!(
            "expected in-flight maturity disbursement to be observable (2y checkpoint); disburser_dbg={dbg:?}; last_inflight={:?}; maturity_e8s_equivalent={}; {snap}",
            last.maturity_disbursements_in_progress,
            last.maturity_e8s_equivalent,
        );
    }

    let dbg = debug_state(&pic, disburser_canister)?;
    let age_used_2y = dbg.prev_age_seconds;

    // Age should be exactly at the target (within a few seconds of stepping).
    let age_diff = if age_used_2y > age_pre { age_used_2y - age_pre } else { age_pre - age_used_2y };
    if age_diff > 5 {
        bail!("2y checkpoint: expected age_used to match current age; age_pre={age_pre}, age_used={age_used_2y}, diff={age_diff}s");
    }

    // Finalize maturity (mint to staging) and execute payout.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;
    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    start_canister_as(&pic, disburser_canister, disburser_canister)?;

    // Compute expected split for exactly 2y (bonus = 12.5%, multiplier = 1.125x).
    // Note: NNS age bonus is capped at +25% at 4 years; at 2 years it is +12.5%.
    let expected_base = expected_base_for_age(staging_after, age_used_2y);
    let expected_bonus = staging_after.saturating_sub(expected_base);

    // Compute the exact plan the disburser should execute at exactly 2y (bonus = 12.5%, multiplier = 1.125x).
    // Note: NNS age bonus is capped at +25% at 4 years; at 2 years it is +12.5%.
    let (gross2, planned2) = expected_plan_payout_transfers(
        staging_after,
        fee_e8s,
        age_used_2y,
        &init.normal_recipient,
        &init.age_bonus_recipient_1,
        &init.age_bonus_recipient_2,
    );
    if planned2.len() != 3 {
        bail!("2y checkpoint: expected 3 transfers (normal+bonus80+bonus20), got {}", planned2.len());
    }

    // Execute payout and verify exact on-ledger deltas.
    let rn_before = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let rn_after = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    let normal_delta = rn_after.saturating_sub(rn_before);
    let b1_delta = rb1_after.saturating_sub(rb1_before);
    let b2_delta = rb2_after.saturating_sub(rb2_before);

    for p in &planned2 {
        let got = if p.to == init.normal_recipient {
            normal_delta
        } else if p.to == init.age_bonus_recipient_1 {
            b1_delta
        } else if p.to == init.age_bonus_recipient_2 {
            b2_delta
        } else {
            bail!("2y checkpoint: planned transfer targets unknown account: {:?}", p.to);
        };
        if got != p.amount_e8s {
            bail!(
                "2y checkpoint: recipient balance mismatch for {:?}: expected +{}, got +{} (diff={} e8s)",
                p.to,
                p.amount_e8s,
                got,
                if p.amount_e8s > got { p.amount_e8s - got } else { got - p.amount_e8s }
            );
        }
    }

    // Exact split checks (gross shares).
    assert_eq_with_diff("2y base_gross", expected_base, gross2.base_e8s)?;
    assert_eq_with_diff("2y bonus_gross_total", expected_bonus, gross2.bonus80_e8s + gross2.bonus20_e8s)?;
    assert_eq_with_diff("2y total_gross", staging_after, gross2.base_e8s + gross2.bonus80_e8s + gross2.bonus20_e8s)?;
    // ----------------- Check clamp beyond 4 years -----------------
    // Rebuild some maturity for the second checkpoint. Keep the disburser stopped so long
    // reward/age advances cannot consume maturity before the explicit 5y checkpoint.
    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    advance_neuron_age_to(&pic, gov, controller, neuron_id, FIVE_YEARS)?;
    start_canister_as(&pic, disburser_canister, disburser_canister)?;
    let age_pre5 = neuron_age_secs(&pic, gov, controller, neuron_id)?;
    let pre5_diff = if age_pre5 > FIVE_YEARS { age_pre5 - FIVE_YEARS } else { FIVE_YEARS - age_pre5 };
    if pre5_diff > 60 {
        bail!("5y checkpoint: expected pre-age to be ~5 years; age_pre={age_pre5} (diff {pre5_diff}s)");
    }
    
    let mut seen5 = false;
    let mut last5 = get_full_neuron(&pic, gov, controller, neuron_id)?;
    for _attempt in 1..=3u32 {
        let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
        let (s, l) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
        seen5 = s;
        last5 = l;
        if seen5 {
            break;
        }
    }
    if !seen5 {
        let snap = neuron_snapshot(&pic, gov, controller, neuron_id, "no-inflight-5y")?;
        let dbg = debug_state(&pic, disburser_canister)?;
        bail!(
            "expected in-flight maturity disbursement to be observable (5y checkpoint); disburser_dbg={dbg:?}; last_inflight={:?}; maturity_e8s_equivalent={}; {snap}",
            last5.maturity_disbursements_in_progress,
            last5.maturity_e8s_equivalent,
        );
    }

    let dbg5 = debug_state(&pic, disburser_canister)?;
    let age_used_5y = dbg5.prev_age_seconds;
    if age_used_5y < FOUR_YEARS {
        bail!("5y checkpoint: expected age_used >= 4y, got age_used={age_used_5y}");
    }

    stop_canister_as(&pic, disburser_canister, disburser_canister)?;
    let staging_before5 = icrc1_balance(&pic, ledger, &staging)?;
    advance_days(&pic, 8);
    let staging_after5 = wait_for_staging_credit(&pic, ledger, &staging, staging_before5)?;
    start_canister_as(&pic, disburser_canister, disburser_canister)?;

    // At 5y, the age bonus must be clamped to the 4y maximum (+25% => multiplier 1.25x).
    let (gross4, planned4) = expected_plan_payout_transfers(
        staging_after5,
        fee_e8s,
        FOUR_YEARS,
        &init.normal_recipient,
        &init.age_bonus_recipient_1,
        &init.age_bonus_recipient_2,
    );
    let (gross5, planned5) = expected_plan_payout_transfers(
        staging_after5,
        fee_e8s,
        age_used_5y,
        &init.normal_recipient,
        &init.age_bonus_recipient_1,
        &init.age_bonus_recipient_2,
    );

    if gross5.base_e8s != gross4.base_e8s || gross5.bonus80_e8s != gross4.bonus80_e8s || gross5.bonus20_e8s != gross4.bonus20_e8s {
        bail!(
            "expected age bonus clamp at 4y: gross@4y={:?}, gross@age_used({})={:?}",
            gross4,
            age_used_5y,
            gross5
        );
    }
    if planned5 != planned4 {
        bail!(
            "expected age bonus clamp at 4y: planned@4y={:?}, planned@age_used({})={:?}",
            planned4,
            age_used_5y,
            planned5
        );
    }

    // Execute payout and verify exact on-ledger deltas match the (clamped) plan.
    let rn_before5 = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_before5 = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_before5 = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;
    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
    tick_n(&pic, 10);
    let rn_after5 = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_after5 = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_after5 = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;
    let normal_delta5 = rn_after5.saturating_sub(rn_before5);
    let b1_delta5 = rb1_after5.saturating_sub(rb1_before5);
    let b2_delta5 = rb2_after5.saturating_sub(rb2_before5);

    for p in &planned4 {
        let got = if p.to == init.normal_recipient {
            normal_delta5
        } else if p.to == init.age_bonus_recipient_1 {
            b1_delta5
        } else if p.to == init.age_bonus_recipient_2 {
            b2_delta5
        } else {
            bail!("5y checkpoint: planned transfer targets unknown account: {:?}", p.to);
        };
        if got != p.amount_e8s {
            bail!(
                "5y checkpoint: recipient balance mismatch for {:?}: expected +{}, got +{} (diff={} e8s)",
                p.to,
                p.amount_e8s,
                got,
                if p.amount_e8s > got { p.amount_e8s - got } else { got - p.amount_e8s }
            );
        }
    }
    Ok(())
}



#[test]
#[ignore]
fn e2e_age_bonus_baseline_matches_age0_with_whale_background() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;

    // Disburser canister principal (also controller of our test neurons).
    // We intentionally defer wasm install until after the cross-neuron baseline stage,
    // so timers cannot interfere with maturity earning or finalization.
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);
    let controller = disburser_canister;

    fn now_secs(pic: &PocketIc) -> u64 {
        (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64
    }

    fn neuron_age_secs(pic: &PocketIc, gov: Principal, controller: Principal, neuron_id: u64) -> Result<u64> {
        let n = get_full_neuron(pic, gov, controller, neuron_id)?;
        Ok(now_secs(pic).saturating_sub(n.aging_since_timestamp_seconds))
    }

    fn diff_bp(a: u64, b: u64) -> u64 {
        let hi = a.max(b) as u128;
        let lo = a.min(b) as u128;
        if hi == 0 {
            0
        } else {
            (((hi - lo) * 10_000u128) / hi) as u64
        }
    }

    fn diff_bp_u128(a: u128, b: u128) -> u64 {
        let hi = a.max(b);
        let lo = a.min(b);
        if hi == 0 {
            0
        } else {
            (((hi - lo) * 10_000u128) / hi) as u64
        }
    }

    const MAX_RATIO_BP: u64 = 0; // exact
    const MAX_BASELINE_BP: u64 = 0; // exact

    // Create a huge whale neuron to dominate the background voting power.
    // This reduces sensitivity to any preconfigured genesis neurons and makes the reward environment stable.
    let whale_id = stake_and_claim_neuron(
        &pic,
        ledger,
        gov,
        Principal::anonymous(),
        990_001,
        10_000 * 100_000_000,
    )?;
    increase_dissolve_delay(&pic, gov, Principal::anonymous(), whale_id, 31_557_600)?;
    // Make the whale extremely large (aim for 1B ICP), but cap to half of the anonymous account balance
    // so we never exhaust funds needed for the rest of the test.
    let anon_acct = Account { owner: Principal::anonymous(), subaccount: None };
    let anon_bal = icrc1_balance(&pic, ledger, &anon_acct)?;
    let desired_whale: u64 = 1_000_000_000u64 * 100_000_000u64;
    let fee_e8s = icrc1_fee(&pic, ledger)?;
    // Keep a generous reserve for other test transfers and fees.
    let reserve: u64 = 50_000 * 100_000_000 + 100 * fee_e8s;
    let safe_cap = anon_bal.saturating_sub(reserve);
    let whale_topup: u64 = desired_whale.min(safe_cap / 2).saturating_sub(10 * fee_e8s);
    if whale_topup < 1_000_000u64 * 100_000_000u64 {
        bail!("insufficient anonymous balance for whale top-up: anon_bal={} e8s", anon_bal);
    }
    top_up_neuron_stake_and_refresh(&pic, ledger, gov, Principal::anonymous(), whale_id, whale_topup)?;

    // Create the 4y neuron at t=0.
    let n4 = stake_and_claim_neuron(&pic, ledger, gov, controller, 990_004, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, n4, 31_557_600)?;
    top_up_neuron_stake_and_refresh(&pic, ledger, gov, controller, n4, 50_000_000 * 100_000_000)?;

    // Advance 2 years and create the 2y neuron.
    advance_time_steps(&pic, 2 * SECS_PER_YEAR, 7 * DAY_SECS, 3);
    let n2 = stake_and_claim_neuron(&pic, ledger, gov, controller, 990_002, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, n2, 31_557_600)?;
    top_up_neuron_stake_and_refresh(&pic, ledger, gov, controller, n2, 50_000_000 * 100_000_000)?;

    // Advance another 2 years and create the age-0 neuron.
    advance_time_steps(&pic, 2 * SECS_PER_YEAR, 7 * DAY_SECS, 3);
    let n0 = stake_and_claim_neuron(&pic, ledger, gov, controller, 990_003, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, n0, 31_557_600)?;
    top_up_neuron_stake_and_refresh(&pic, ledger, gov, controller, n0, 50_000_000 * 100_000_000)?;

    // Create rewardable proposals and ensure ALL neurons vote YES on them.
    let pid = make_motion_proposal(
        &pic,
        gov,
        Principal::anonymous(),
        whale_id,
        "E2E whale-stabilized age bonus baseline check",
        99_000,
    )?;

    register_vote_yes(&pic, gov, Principal::anonymous(), whale_id, &pid)?;
    register_vote_yes(&pic, gov, controller, n4, &pid)?;
    register_vote_yes(&pic, gov, controller, n2, &pid)?;
    register_vote_yes(&pic, gov, controller, n0, &pid)?;

    // Let the proposal settle and rewards distribute.
    advance_days(&pic, 8);
    tick_n(&pic, 20);

    // Ensure each neuron has enough maturity to make the later direct disbursement stage meaningful.
    const MIN_DISBURSE_E8S: u64 = 200_000_000;
    for round in 0..25u64 {
        let m4 = get_full_neuron(&pic, gov, controller, n4)?.maturity_e8s_equivalent;
        let m2 = get_full_neuron(&pic, gov, controller, n2)?.maturity_e8s_equivalent;
        let m0 = get_full_neuron(&pic, gov, controller, n0)?.maturity_e8s_equivalent;
        if m4 >= MIN_DISBURSE_E8S && m2 >= MIN_DISBURSE_E8S && m0 >= MIN_DISBURSE_E8S {
            break;
        }

        let pid = make_motion_proposal(
            &pic,
            gov,
            Principal::anonymous(),
            whale_id,
            "E2E whale-stabilized age bonus baseline check (top-up rewards)",
            99_100 + round,
        )?;
        register_vote_yes(&pic, gov, Principal::anonymous(), whale_id, &pid)?;
        register_vote_yes(&pic, gov, controller, n4, &pid)?;
        register_vote_yes(&pic, gov, controller, n2, &pid)?;
        register_vote_yes(&pic, gov, controller, n0, &pid)?;
        advance_days(&pic, 8);
        tick_n(&pic, 20);
    }

    let m4 = get_full_neuron(&pic, gov, controller, n4)?.maturity_e8s_equivalent;
    let m2 = get_full_neuron(&pic, gov, controller, n2)?.maturity_e8s_equivalent;
    let m0 = get_full_neuron(&pic, gov, controller, n0)?.maturity_e8s_equivalent;
    if m4 < MIN_DISBURSE_E8S || m2 < MIN_DISBURSE_E8S || m0 < MIN_DISBURSE_E8S {
        bail!(
            "insufficient maturity for disbursement after reward rounds: m4={}, m2={}, m0={} (target {} e8s)",
            m4, m2, m0, MIN_DISBURSE_E8S
        );
    }

    // Dedicated single reward window for the cross-neuron baseline assertion.
    // This avoids mixing different execution paths (disburser-vs-governance) into the baseline proof.
    let pre4 = get_full_neuron(&pic, gov, controller, n4)?;
    let pre2 = get_full_neuron(&pic, gov, controller, n2)?;
    let pre0 = get_full_neuron(&pic, gov, controller, n0)?;

    let age4_pre = now_secs(&pic).saturating_sub(pre4.aging_since_timestamp_seconds);
    let age2_pre = now_secs(&pic).saturating_sub(pre2.aging_since_timestamp_seconds);
    let age0_pre = now_secs(&pic).saturating_sub(pre0.aging_since_timestamp_seconds);

    let pid = make_motion_proposal(
        &pic,
        gov,
        Principal::anonymous(),
        whale_id,
        "E2E whale-stabilized age bonus baseline check (comparison window)",
        99_500,
    )?;
    register_vote_yes(&pic, gov, Principal::anonymous(), whale_id, &pid)?;
    register_vote_yes(&pic, gov, controller, n4, &pid)?;
    register_vote_yes(&pic, gov, controller, n2, &pid)?;
    register_vote_yes(&pic, gov, controller, n0, &pid)?;
    advance_days(&pic, 8);
    tick_n(&pic, 20);

    let post4 = get_full_neuron(&pic, gov, controller, n4)?;
    let post2 = get_full_neuron(&pic, gov, controller, n2)?;
    let post0 = get_full_neuron(&pic, gov, controller, n0)?;

    let delta4 = post4.maturity_e8s_equivalent.saturating_sub(pre4.maturity_e8s_equivalent);
    let delta2 = post2.maturity_e8s_equivalent.saturating_sub(pre2.maturity_e8s_equivalent);
    let delta0 = post0.maturity_e8s_equivalent.saturating_sub(pre0.maturity_e8s_equivalent);

    if delta4 == 0 || delta2 == 0 || delta0 == 0 {
        bail!(
            "expected non-zero maturity deltas in comparison window: delta4={} delta2={} delta0={}; ages_pre=[{}, {}, {}]",
            delta4,
            delta2,
            delta0,
            age4_pre,
            age2_pre,
            age0_pre,
        );
    }

    let den: u128 = (16 * SECS_PER_YEAR) as u128;
    let num4: u128 = den + age4_pre.min(MAX_AGE_FOR_BONUS_SECS) as u128;
    let num2: u128 = den + age2_pre.min(MAX_AGE_FOR_BONUS_SECS) as u128;
    let num0: u128 = den + age0_pre.min(MAX_AGE_FOR_BONUS_SECS) as u128;

    let ratio42_lhs = (delta4 as u128) * num2;
    let ratio42_rhs = (delta2 as u128) * num4;
    let ratio42_bp = diff_bp_u128(ratio42_lhs, ratio42_rhs);
    let ratio42_abs = ratio42_lhs.max(ratio42_rhs) - ratio42_lhs.min(ratio42_rhs);
    if ratio42_bp > MAX_RATIO_BP {
        bail!(
            "expected exact maturity delta ratio 4y:2y match to age multiplier ratio; tolerated={} bp; observed={} bp; delta4={} delta2={} ages_pre=[{}, {}] num4={} num2={} lhs(delta4*num2)={} rhs(delta2*num4)={} abs_diff={}",
            MAX_RATIO_BP,
            ratio42_bp,
            delta4,
            delta2,
            age4_pre,
            age2_pre,
            num4,
            num2,
            ratio42_lhs,
            ratio42_rhs,
            ratio42_abs,
        );
    }

    let ratio20_lhs = (delta2 as u128) * num0;
    let ratio20_rhs = (delta0 as u128) * num2;
    let ratio20_bp = diff_bp_u128(ratio20_lhs, ratio20_rhs);
    let ratio20_abs = ratio20_lhs.max(ratio20_rhs) - ratio20_lhs.min(ratio20_rhs);
    if ratio20_bp > MAX_RATIO_BP {
        bail!(
            "expected exact maturity delta ratio 2y:age0 match to age multiplier ratio; tolerated={} bp; observed={} bp; delta2={} delta0={} ages_pre=[{}, {}] num2={} num0={} lhs(delta2*num0)={} rhs(delta0*num2)={} abs_diff={}",
            MAX_RATIO_BP,
            ratio20_bp,
            delta2,
            delta0,
            age2_pre,
            age0_pre,
            num2,
            num0,
            ratio20_lhs,
            ratio20_rhs,
            ratio20_abs,
        );
    }

    let base4 = expected_compute_gross_split(delta4, age4_pre).base_e8s;
    let base2 = expected_compute_gross_split(delta2, age2_pre).base_e8s;
    let base0 = expected_compute_gross_split(delta0, age0_pre).base_e8s;

    let base40_bp = diff_bp(base4, base0);
    let base40_abs = base4.max(base0) - base4.min(base0);
    if base40_bp > MAX_BASELINE_BP {
        bail!(
            "expected exact baseline(4y) == baseline(age0): tolerated={} bp; observed={} bp; base4={} base0={} abs_diff={} delta4={} delta0={} ages_pre=[{}, {}]",
            MAX_BASELINE_BP,
            base40_bp,
            base4,
            base0,
            base40_abs,
            delta4,
            delta0,
            age4_pre,
            age0_pre,
        );
    }
    let base20_bp = diff_bp(base2, base0);
    let base20_abs = base2.max(base0) - base2.min(base0);
    if base20_bp > MAX_BASELINE_BP {
        bail!(
            "expected exact baseline(2y) == baseline(age0): tolerated={} bp; observed={} bp; base2={} base0={} abs_diff={} delta2={} delta0={} ages_pre=[{}, {}]",
            MAX_BASELINE_BP,
            base20_bp,
            base2,
            base0,
            base20_abs,
            delta2,
            delta0,
            age2_pre,
            age0_pre,
        );
    }

    // Use a payout-only stage for the disburser: fund its staging account via a direct governance disbursement,
    // then verify exact ledger deltas from the disburser's split logic.
    let staging = Account { owner: disburser_canister, subaccount: None };
    let staging_before = icrc1_balance(&pic, ledger, &staging)?;
    let age_used_4 = neuron_age_secs(&pic, gov, controller, n4)?;

    disburse_maturity_to_account(&pic, gov, controller, n4, &staging)?;
    let (seen4, _) = wait_for_inflight_disbursement(&pic, gov, controller, n4)?;
    if !seen4 {
        let snap = neuron_snapshot(&pic, gov, controller, n4, "baseline-direct-n4")?;
        bail!("expected in-flight maturity disbursement to be observable for 4y neuron; {snap}");
    }

    advance_days(&pic, 8);
    let staging_after = wait_for_staging_credit(&pic, ledger, &staging, staging_before)?;
    let minted4 = staging_after.saturating_sub(staging_before);
    if minted4 == 0 {
        bail!("expected non-zero staging mint for 4y neuron: staging_before={} staging_after={}", staging_before, staging_after);
    }

    let wasm = build_disburser_wasm()?;
    let r_normal = pic.create_canister();
    let r_bonus1 = pic.create_canister();
    let r_bonus2 = pic.create_canister();

    let init = InitArg {
        neuron_id: n4,
        normal_recipient: Account { owner: r_normal, subaccount: None },
        age_bonus_recipient_1: Account { owner: r_bonus1, subaccount: None },
        age_bonus_recipient_2: Account { owner: r_bonus2, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(60),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init.clone())?, None);
    set_self_only_controller(&pic, disburser_canister)?;
    debug_set_skip_maturity_initiation(&pic, disburser_canister, true)?;
    debug_set_prev_age_seconds(&pic, disburser_canister, age_used_4)?;

    let rn_before = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
    let rb1_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
    let rb2_before = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

    let (_gross, planned) = expected_plan_payout_transfers(
        staging_after,
        fee_e8s,
        age_used_4,
        &init.normal_recipient,
        &init.age_bonus_recipient_1,
        &init.age_bonus_recipient_2,
    );

    let (normal_delta, b1_delta, b2_delta) = {
        let mut out: Option<(u64, u64, u64)> = None;

        for attempt in 1..=3u32 {
            let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;
            tick_n(&pic, 10);

            let rn_after = icrc1_balance(&pic, ledger, &init.normal_recipient)?;
            let rb1_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_1)?;
            let rb2_after = icrc1_balance(&pic, ledger, &init.age_bonus_recipient_2)?;

            let normal_delta = rn_after.saturating_sub(rn_before);
            let b1_delta = rb1_after.saturating_sub(rb1_before);
            let b2_delta = rb2_after.saturating_sub(rb2_before);

            if normal_delta > 0 || b1_delta > 0 || b2_delta > 0 {
                out = Some((normal_delta, b1_delta, b2_delta));
                break;
            }

            if attempt == 3 {
                let dbg_now = debug_state(&pic, disburser_canister)?;
                let snap_n = neuron_snapshot(&pic, gov, controller, n4, "baseline-payout-no-recipient-delta")?;
                let staging_post = icrc1_balance(&pic, ledger, &staging)?;
                bail!(
                    "payout did not move any recipient balances after {attempt} manual ticks; expected staged 4y baseline payout to execute; staging_after={} staging_post={} fee={} age_used={}; disburser_dbg={:?}; {snap_n}",
                    staging_after,
                    staging_post,
                    fee_e8s,
                    age_used_4,
                    dbg_now,
                );
            }
        }

        out.expect("loop must either produce deltas or bail on final attempt")
    };

    for p in &planned {
        let got = if p.to == init.normal_recipient {
            normal_delta
        } else if p.to == init.age_bonus_recipient_1 {
            b1_delta
        } else if p.to == init.age_bonus_recipient_2 {
            b2_delta
        } else {
            bail!("planned transfer targets unknown account: {:?}", p.to);
        };
        if got != p.amount_e8s {
            bail!(
                "recipient balance mismatch for {:?}: expected +{}, got +{}",
                p.to,
                p.amount_e8s,
                got
            );
        }
    }

    let staging_post = icrc1_balance(&pic, ledger, &staging)?;
    let gross_sent: u64 = planned.iter().map(|p| p.gross_share_e8s).sum();
    let expected_staging_post = staging_after.saturating_sub(gross_sent);
    if staging_post != expected_staging_post {
        bail!(
            "staging mismatch after payout: expected {expected_staging_post}, got {staging_post}"
        );
    }

    let dbg_end = debug_state(&pic, disburser_canister)?;
    if dbg_end.payout_plan_present {
        bail!("expected payout plan to be cleared after success");
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_claim_or_refresh_top_up_is_driven_by_disburser_tick() -> Result<()> {
    require_ignored_flag()?;

    let pic = build_pic();
    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let controller = disburser_canister;
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 8_888_000, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(365 * DAY_SECS),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    let before = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let before_stake = before.cached_neuron_stake_e8s;

    let top_up_e8s = 25 * 100_000_000;
    transfer_to_neuron_staking_subaccount(&pic, ledger, gov, controller, neuron_id, top_up_e8s)?;

    let mid = get_full_neuron(&pic, gov, controller, neuron_id)?;
    if mid.cached_neuron_stake_e8s != before_stake {
        bail!(
            "expected cached stake to remain unchanged before the disburser tick runs claim_or_refresh; before={} mid={}",
            before_stake,
            mid.cached_neuron_stake_e8s
        );
    }

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    let after = wait_for_cached_stake_increase(&pic, gov, controller, neuron_id, before_stake)?;
    if after.cached_neuron_stake_e8s < before_stake.saturating_add(top_up_e8s) {
        bail!(
            "expected cached stake to reflect the full top-up after disburser-driven claim_or_refresh; before={} top_up={} after={}",
            before_stake,
            top_up_e8s,
            after.cached_neuron_stake_e8s
        );
    }

    Ok(())
}

#[test]
#[ignore]
fn e2e_refresh_voting_power_after_successful_disbursement_initiation() -> Result<()> {
    require_ignored_flag()?;

    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let pic = PocketIcBuilder::new().with_log_level(pic_log_level())
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build();

    let ledger = Principal::from_text(ICP_LEDGER_ID)?;
    let gov = Principal::from_text(NNS_GOVERNANCE_ID)?;
    let disburser_canister = pic.create_canister();
    pic.add_cycles(disburser_canister, 5_000_000_000_000);

    let controller = disburser_canister;
    let neuron_id = stake_and_claim_neuron(&pic, ledger, gov, controller, 8_888_001, 10_000 * 100_000_000)?;
    increase_dissolve_delay(&pic, gov, controller, neuron_id, 31_557_600)?;
    ensure_maturity_ge_1_icp(&pic, ledger, gov, controller, neuron_id)?;

    let wasm = build_disburser_wasm()?;
    let init = InitArg {
        neuron_id,
        normal_recipient: Account { owner: Principal::anonymous(), subaccount: None },
        age_bonus_recipient_1: Account { owner: Principal::management_canister(), subaccount: None },
        age_bonus_recipient_2: Account { owner: disburser_canister, subaccount: None },
        ledger_canister_id: Some(ledger),
        governance_canister_id: Some(gov),
        rescue_controller: disburser_canister,
        blackhole_armed: None,
        main_interval_seconds: Some(365 * DAY_SECS),
        rescue_interval_seconds: Some(365 * DAY_SECS),
    };

    pic.install_canister(disburser_canister, wasm, encode_one(init)?, None);
    set_self_only_controller(&pic, disburser_canister)?;

    let before = get_full_neuron(&pic, gov, controller, neuron_id)?;
    let before_refresh = before
        .voting_power_refreshed_timestamp_seconds
        .ok_or_else(|| anyhow!("governance did not expose voting_power_refreshed_timestamp_seconds before refresh"))?;

    // Force the next refresh to be observably newer if the canister performs it.
    advance_days(&pic, 8);

    let _: () = update_noargs(&pic, disburser_canister, Principal::anonymous(), "debug_main_tick")?;

    let (seen, after) = wait_for_inflight_disbursement(&pic, gov, controller, neuron_id)?;
    if !seen {
        bail!(
            "expected in-flight maturity disbursement to be observable when refresh is triggered; inflight={:?}",
            after.maturity_disbursements_in_progress
        );
    }

    let after_refresh = after
        .voting_power_refreshed_timestamp_seconds
        .ok_or_else(|| anyhow!("governance did not expose voting_power_refreshed_timestamp_seconds after refresh"))?;

    if after_refresh <= before_refresh {
        bail!(
            "expected voting power refresh timestamp to advance after successful maturity initiation; before={} after={}",
            before_refresh,
            after_refresh
        );
    }

    let now = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    let refresh_age = now.saturating_sub(after_refresh);
    if refresh_age > DAY_SECS {
        bail!(
            "expected refreshed timestamp to be recent after main tick; now={} refreshed={} age_secs={}",
            now,
            after_refresh,
            refresh_age
        );
    }

    if let (Some(deciding), Some(potential)) = (after.deciding_voting_power, after.potential_voting_power) {
        if deciding != potential {
            bail!(
                "expected deciding_voting_power == potential_voting_power after refresh; deciding={} potential={}",
                deciding,
                potential
            );
        }
    }

    Ok(())
}


