use binread::{io::Cursor, BinRead};
use candid::{
    types::{subtype, Field, Type, TypeEnv, TypeInner},
    CandidType, Deserialize, Principal,
};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::account::{account_text, subaccount_text};
use serde::Serialize;
use std::borrow::Cow;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct Config {
    pub staking_account: Account,
    pub payout_subaccount: Option<[u8; 32]>,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub cmc_canister_id: Principal,
    #[serde(default)]
    pub governance_canister_id: Option<Principal>,
    pub funding_source_account: Account,
    pub rescue_controller: Principal,
    pub autonomous_rescue_armed: Option<bool>,
    pub expected_first_staking_tx_id: Option<u64>,
    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
    pub min_tx_e8s: u64,
    #[serde(default)]
    pub stake_recognition_delay_seconds: Option<u64>,
}

fn opt_principal_text(principal: Option<Principal>) -> String {
    principal
        .map(|p| p.to_text())
        .unwrap_or_else(|| "none".to_string())
}

fn opt_bool_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "none",
    }
}

fn opt_u64_text(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn opt_forced_rescue_reason_text(value: Option<&ForcedRescueReason>) -> String {
    value
        .map(|reason| format!("{reason:?}"))
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn runtime_config_log_line(cfg: &Config) -> String {
    format!(
        "CONFIG staking_account={}, payout_subaccount={}, ledger_canister_id={}, index_canister_id={}, cmc_canister_id={}, governance_canister_id={}, canonical_relay_canister_id={}, funding_source_account={}, rescue_controller={}, autonomous_rescue_armed={}, expected_first_staking_tx_id={}, main_interval_seconds={}, rescue_interval_seconds={}, min_tx_e8s={}, stake_recognition_delay_seconds={}",
        account_text(&cfg.staking_account),
        subaccount_text(&cfg.payout_subaccount),
        cfg.ledger_canister_id.to_text(),
        cfg.index_canister_id.to_text(),
        cfg.cmc_canister_id.to_text(),
        opt_principal_text(cfg.governance_canister_id),
        crate::canonical_relay_canister_id().to_text(),
        account_text(&cfg.funding_source_account),
        cfg.rescue_controller.to_text(),
        opt_bool_text(cfg.autonomous_rescue_armed),
        opt_u64_text(cfg.expected_first_staking_tx_id),
        cfg.main_interval_seconds,
        cfg.rescue_interval_seconds,
        cfg.min_tx_e8s,
        opt_u64_text(cfg.stake_recognition_delay_seconds)
    )
}

pub(crate) fn runtime_state_log_line(st: &State) -> String {
    let active_funding_scan = st.active_funding_scan.as_ref();
    let active_payout_job = st.active_payout_job.as_ref();
    format!(
        "STATE:last_processed_funding_tx_id={} forced_rescue_reason={} last_observed_staking_balance_e8s={} last_observed_latest_tx_id={} consecutive_index_anchor_failures={} consecutive_index_latest_invariant_failures={} consecutive_index_latest_unreadable_failures={} active_funding_scan_cursor={} active_funding_scan_candidate_tx_id={} active_funding_scan_candidate_amount_e8s={} active_funding_scan_anchor_last_processed_funding_tx_id={} active_payout_job_present={} active_payout_funding_tx_id={} active_payout_funding_amount_e8s={} round_start_time_nanos={} round_start_tx_id={}",
        opt_u64_text(st.last_processed_funding_tx_id),
        opt_forced_rescue_reason_text(st.forced_rescue_reason.as_ref()),
        opt_u64_text(st.last_observed_staking_balance_e8s),
        opt_u64_text(st.last_observed_latest_tx_id),
        opt_u64_text(st.consecutive_index_anchor_failures.map(u64::from)),
        opt_u64_text(
            st.consecutive_index_latest_invariant_failures
                .map(u64::from)
        ),
        opt_u64_text(
            st.consecutive_index_latest_unreadable_failures
                .map(u64::from)
        ),
        opt_u64_text(active_funding_scan.and_then(|scan| scan.cursor)),
        opt_u64_text(active_funding_scan.and_then(|scan| scan.candidate).map(|candidate| candidate.tx_id)),
        opt_u64_text(active_funding_scan.and_then(|scan| scan.candidate).map(|candidate| candidate.amount_e8s)),
        opt_u64_text(active_funding_scan.and_then(|scan| scan.anchor_last_processed_funding_tx_id)),
        active_payout_job.is_some(),
        opt_u64_text(active_payout_job.and_then(|job| job.funding_tx_id)),
        opt_u64_text(active_payout_job.and_then(|job| job.funding_amount_e8s)),
        opt_u64_text(st.current_round_start_time_nanos),
        opt_u64_text(st.current_round_start_latest_tx_id),
    )
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransferKind {
    Beneficiary,
    CyclesTopUpRawFallback,
    RawIcp,
    NeuronStake,
    RemainderToRelay,
}

impl TransferKind {
    pub(crate) fn is_beneficiary_payout(&self) -> bool {
        matches!(
            self,
            Self::Beneficiary | Self::CyclesTopUpRawFallback | Self::RawIcp | Self::NeuronStake
        )
    }

    pub(crate) fn requires_cmc_notify(&self) -> bool {
        matches!(self, Self::Beneficiary)
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingNotification {
    pub kind: TransferKind,
    pub beneficiary: Principal,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub block_index: u64,
    pub next_start: Option<u64>,
    #[serde(default)]
    pub transfer_memo: Option<Vec<u8>>,
    #[serde(default)]
    pub destination_subaccount: Option<[u8; 32]>,
    #[serde(default)]
    pub neuron_id: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PendingTransferPhase {
    AwaitingTransfer,
    TransferAccepted,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingTransfer {
    pub notification: PendingNotification,
    pub created_at_time_nanos: u64,
    pub phase: PendingTransferPhase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkipRange {
    pub start_tx_id: u64,
    pub end_tx_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct U64Key(u64);

impl U64Key {
    fn get(&self) -> u64 {
        self.0
    }
}

impl From<u64> for U64Key {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Storable for U64Key {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.to_be_bytes().to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.to_be_bytes().to_vec()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let slice = bytes.as_ref();
        assert_eq!(slice.len(), 8, "invalid faucet u64 key length");
        let mut raw = [0u8; 8];
        raw.copy_from_slice(slice);
        Self(u64::from_be_bytes(raw))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 8,
        is_fixed_size: true,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct U64Value(u64);

impl U64Value {
    fn get(&self) -> u64 {
        self.0
    }
}

impl From<u64> for U64Value {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Storable for U64Value {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.to_be_bytes().to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.to_be_bytes().to_vec()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let slice = bytes.as_ref();
        assert_eq!(slice.len(), 8, "invalid faucet u64 value length");
        let mut raw = [0u8; 8];
        raw.copy_from_slice(slice);
        Self(u64::from_be_bytes(raw))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 8,
        is_fixed_size: true,
    };
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum ForcedRescueReason {
    BootstrapNoSuccess,
    IndexAnchorMissing,
    IndexLatestInvariantBroken,
    IndexLatestUnreadable,
    CmcZeroSuccessRuns,
    AccountingInvariantBroken,
    FundingTrancheBalanceMismatch,
    FundingDiscoveryUnreadable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SkipRangeInsertError {
    InvalidRange,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub pot_start_e8s: u64,
    pub pot_remaining_e8s: u64,
    pub denom_staking_balance_e8s: u64,
    #[serde(default)]
    pub effective_denom_staking_balance_e8s: Option<u64>,
    #[serde(default)]
    pub funding_tx_id: Option<u64>,
    #[serde(default)]
    pub funding_amount_e8s: Option<u64>,
    #[serde(default)]
    pub round_end_latest_tx_id: Option<u64>,
    #[serde(default)]
    pub round_end_time_nanos: Option<u64>,
    #[serde(default)]
    pub last_processed_funding_tx_id: Option<u64>,
    pub topped_up_count: u64,
    pub topped_up_sum_e8s: u64,
    pub topped_up_min_e8s: Option<u64>,
    pub topped_up_max_e8s: Option<u64>,
    pub failed_topups: u64,
    #[serde(default)]
    pub ambiguous_topups: u64,
    pub ignored_under_threshold: u64,
    pub ignored_bad_memo: u64,
    pub remainder_to_relay_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FundingTrancheState {
    pub tx_id: u64,
    pub timestamp_nanos: u64,
    pub amount_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FundingScanState {
    pub anchor_last_processed_funding_tx_id: Option<u64>,
    pub cursor: Option<u64>,
    pub candidate: Option<FundingTrancheState>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub(crate) struct ActivePayoutJob {
    pub id: u64,
    pub fee_e8s: u64,
    pub pot_start_e8s: u64,
    pub denom_staking_balance_e8s: u64,
    pub next_start: Option<u64>,
    pub scan_complete: bool,
    pub ignored_under_threshold: u64,
    pub ignored_bad_memo: u64,
    pub gross_outflow_e8s: u64,
    pub topped_up_count: u64,
    pub topped_up_sum_e8s: u64,
    pub topped_up_min_e8s: Option<u64>,
    pub topped_up_max_e8s: Option<u64>,
    pub failed_topups: u64,
    #[serde(default)]
    pub ambiguous_topups: u64,
    pub remainder_to_relay_e8s: u64,
    #[serde(default)]
    pub pending_transfer: Option<PendingTransfer>,
    #[serde(default)]
    pub skip_candidate_start_tx_id: Option<u64>,
    #[serde(default)]
    pub skip_candidate_end_tx_id: Option<u64>,
    #[serde(default)]
    pub skip_candidate_tx_count: u64,
    pub next_created_at_time_nanos: u64,
    pub observed_oldest_tx_id: Option<u64>,
    pub observed_latest_tx_id: Option<u64>,
    pub cmc_attempt_count: Option<u64>,
    pub cmc_success_count: Option<u64>,
    #[serde(default)]
    pub cmc_attempted_beneficiaries: Option<Vec<Principal>>,
    #[serde(default)]
    pub round_start_time_nanos: Option<u64>,
    #[serde(default)]
    pub round_start_latest_tx_id: Option<u64>,
    #[serde(default)]
    pub round_end_time_nanos: Option<u64>,
    #[serde(default)]
    pub round_end_latest_tx_id: Option<u64>,
    #[serde(default)]
    pub effective_denom_staking_balance_e8s: Option<u64>,
    #[serde(default)]
    pub effective_denom_scan_complete: Option<bool>,
    #[serde(default)]
    pub funding_tx_id: Option<u64>,
    #[serde(default)]
    pub funding_tx_timestamp_nanos: Option<u64>,
    #[serde(default)]
    pub funding_amount_e8s: Option<u64>,
}

impl ActivePayoutJob {
    pub(crate) fn new(
        id: u64,
        fee_e8s: u64,
        pot_start_e8s: u64,
        denom_staking_balance_e8s: u64,
        created_at_time_nanos: u64,
    ) -> Self {
        Self {
            id,
            fee_e8s,
            pot_start_e8s,
            denom_staking_balance_e8s,
            next_start: None,
            scan_complete: false,
            ignored_under_threshold: 0,
            ignored_bad_memo: 0,
            gross_outflow_e8s: 0,
            topped_up_count: 0,
            topped_up_sum_e8s: 0,
            topped_up_min_e8s: None,
            topped_up_max_e8s: None,
            failed_topups: 0,
            ambiguous_topups: 0,
            remainder_to_relay_e8s: 0,
            pending_transfer: None,
            skip_candidate_start_tx_id: None,
            skip_candidate_end_tx_id: None,
            skip_candidate_tx_count: 0,
            next_created_at_time_nanos: created_at_time_nanos,
            observed_oldest_tx_id: None,
            observed_latest_tx_id: None,
            cmc_attempt_count: Some(0),
            cmc_success_count: Some(0),
            cmc_attempted_beneficiaries: Some(Vec::new()),
            round_start_time_nanos: None,
            round_start_latest_tx_id: None,
            round_end_time_nanos: None,
            round_end_latest_tx_id: None,
            effective_denom_staking_balance_e8s: None,
            effective_denom_scan_complete: None,
            funding_tx_id: None,
            funding_tx_timestamp_nanos: None,
            funding_amount_e8s: None,
        }
    }

    // Test/setup helper intentionally mirrors the stable-state round-boundary fields.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn configure_round_accounting(
        &mut self,
        round_start_time_nanos: Option<u64>,
        round_start_latest_tx_id: Option<u64>,
        round_end_time_nanos: u64,
        round_end_latest_tx_id: Option<u64>,
        effective_denom_staking_balance_e8s: u64,
        effective_denom_scan_complete: bool,
    ) {
        self.round_start_time_nanos = round_start_time_nanos;
        self.round_start_latest_tx_id = round_start_latest_tx_id;
        self.round_end_time_nanos = Some(round_end_time_nanos);
        self.round_end_latest_tx_id = round_end_latest_tx_id;
        self.effective_denom_staking_balance_e8s = Some(effective_denom_staking_balance_e8s);
        self.effective_denom_scan_complete = Some(effective_denom_scan_complete);
    }

    pub(crate) fn configure_funding_tranche(
        &mut self,
        tx_id: u64,
        timestamp_nanos: u64,
        amount_e8s: u64,
    ) {
        self.funding_tx_id = Some(tx_id);
        self.funding_tx_timestamp_nanos = Some(timestamp_nanos);
        self.funding_amount_e8s = Some(amount_e8s);
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct State {
    pub config: Config,
    pub last_summary: Option<Summary>,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub autonomous_rescue_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
    #[serde(default)]
    pub skip_range_invariant_fault: Option<bool>,
    pub consecutive_index_anchor_failures: Option<u8>,
    pub consecutive_index_latest_invariant_failures: Option<u8>,
    #[serde(default)]
    pub consecutive_index_latest_unreadable_failures: Option<u8>,
    pub consecutive_cmc_zero_success_runs: Option<u8>,
    pub last_observed_staking_balance_e8s: Option<u64>,
    pub last_observed_latest_tx_id: Option<u64>,
    pub main_lock_state_ts: Option<u64>,
    pub payout_nonce: u64,
    pub active_payout_job: Option<ActivePayoutJob>,
    pub last_main_run_ts: u64,
    #[serde(default)]
    pub current_round_start_time_nanos: Option<u64>,
    #[serde(default)]
    pub current_round_start_latest_tx_id: Option<u64>,
    #[serde(default)]
    pub last_processed_funding_tx_id: Option<u64>,
    #[serde(default)]
    // Stable runtime progress for payout-account funding discovery. Do not clear
    // during upgrades unless funding discovery is deliberately restarted from a
    // safe cursor.
    pub active_funding_scan: Option<FundingScanState>,
}

impl State {
    pub(crate) fn new(config: Config, now_secs: u64) -> Self {
        let autonomous_rescue_armed_since_ts = config
            .autonomous_rescue_armed
            .unwrap_or(false)
            .then_some(now_secs);
        Self {
            config,
            last_summary: None,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            autonomous_rescue_armed_since_ts,
            forced_rescue_reason: None,
            skip_range_invariant_fault: Some(false),
            consecutive_index_anchor_failures: Some(0),
            consecutive_index_latest_invariant_failures: Some(0),
            consecutive_index_latest_unreadable_failures: Some(0),
            consecutive_cmc_zero_success_runs: Some(0),
            last_observed_staking_balance_e8s: None,
            last_observed_latest_tx_id: None,
            main_lock_state_ts: Some(0),
            payout_nonce: 1,
            active_payout_job: None,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
            current_round_start_time_nanos: None,
            current_round_start_latest_tx_id: None,
            last_processed_funding_tx_id: None,
            active_funding_scan: None,
        }
    }
}

pub(crate) const UPGRADE_QUIESCENCE_ERROR: &str =
    "faucet upgrade requires no active payout job; allow the payout to finish before upgrading";

// Stable-state enum shape is part of the upgrade contract; boxing V1 would change Candid.
#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum VersionedStableState {
    Uninitialized,
    V1(State),
}

const RETIRED_STATE_FIELD: &str = "current_round_start_staking_balance_e8s";
const RETIRED_JOB_FIELDS: [&str; 2] = [
    "round_start_staking_balance_e8s",
    "round_end_staking_balance_e8s",
];

fn field_type(fields: &[Field], name: &str) -> Result<Type, String> {
    let id = candid::idl_hash(name);
    fields
        .iter()
        .find(|field| field.id.get_id() == id)
        .map(|field| field.ty.clone())
        .ok_or_else(|| format!("required stable field {name} is missing"))
}

fn referenced_record_name(env: &TypeEnv, ty: &Type, context: &str) -> Result<String, String> {
    let traced = env
        .trace_type(ty)
        .map_err(|error| format!("invalid {context} type: {error}"))?;
    let inner = match traced.as_ref() {
        TypeInner::Opt(inner) => inner,
        _ => return Err(format!("{context} is not optional")),
    };
    match inner.as_ref() {
        TypeInner::Var(name) => Ok(name.clone()),
        _ => Err(format!("{context} does not reference a record type")),
    }
}

fn remove_retired_field(
    env: &mut TypeEnv,
    record_name: &str,
    field_name: &str,
) -> Result<(), String> {
    let record = env
        .find_type(record_name)
        .map_err(|error| format!("invalid stable record {record_name}: {error}"))?
        .clone();
    let TypeInner::Record(mut fields) = record.as_ref().clone() else {
        return Err(format!("stable type {record_name} is not a record"));
    };
    let id = candid::idl_hash(field_name);
    let index = fields
        .iter()
        .position(|field| field.id.get_id() == id)
        .ok_or_else(|| format!("retired stable field {field_name} is missing"))?;
    let removed = fields.remove(index);
    subtype::equal(
        &mut subtype::Gamma::default(),
        env,
        &removed.ty,
        &Option::<u64>::ty(),
    )
    .map_err(|error| format!("retired stable field {field_name} has wrong type: {error}"))?;
    env.0
        .insert(record_name.to_string(), TypeInner::Record(fields).into());
    Ok(())
}

// The only accepted non-current wire shape is the immediately deployed V1 schema,
// which differs by exactly three opt nat64 record fields. Strict structural equality
// after removing those fields prevents Candid's optional fallback from dropping any
// still-supported state.
fn validate_deployed_wider_v1_schema(bytes: &[u8]) -> Result<(), String> {
    let mut reader = Cursor::new(bytes);
    let header = candid::binary_parser::Header::read_args(&mut reader, (None,))
        .map_err(|error| format!("failed to parse faucet stable-state type table: {error}"))?;
    let (mut env, args) = header
        .to_types()
        .map_err(|error| format!("invalid faucet stable-state type table: {error}"))?;
    if args.len() != 1 {
        return Err(format!(
            "faucet stable state must contain exactly one value, found {}",
            args.len()
        ));
    }
    let root = env
        .trace_type(&args[0])
        .map_err(|error| format!("invalid faucet stable-state root type: {error}"))?;
    let TypeInner::Variant(versions) = root.as_ref() else {
        return Err("faucet stable-state root is not a variant".to_string());
    };
    let v1_type = field_type(versions, "V1")?;
    let state_name = match v1_type.as_ref() {
        TypeInner::Var(name) => name.clone(),
        _ => return Err("faucet V1 state does not reference a record type".to_string()),
    };
    let state_record = env
        .find_type(&state_name)
        .map_err(|error| format!("invalid faucet V1 state type: {error}"))?;
    let TypeInner::Record(state_fields) = state_record.as_ref() else {
        return Err("faucet V1 state is not a record".to_string());
    };
    let active_job_type = field_type(state_fields, "active_payout_job")?;
    let job_name = referenced_record_name(&env, &active_job_type, "active_payout_job")?;

    remove_retired_field(&mut env, &state_name, RETIRED_STATE_FIELD)?;
    for field in RETIRED_JOB_FIELDS {
        remove_retired_field(&mut env, &job_name, field)?;
    }

    subtype::equal(
        &mut subtype::Gamma::default(),
        &env,
        &args[0],
        &VersionedStableState::ty(),
    )
    .map_err(|error| format!("unsupported faucet stable-state schema: {error}"))
}

pub(crate) fn decode_versioned_stable_state(bytes: &[u8]) -> Result<VersionedStableState, String> {
    let current = candid::decode_one::<VersionedStableState>(bytes)
        .map_err(|error| format!("failed to decode faucet stable state: {error}"))?;
    let canonical_current = candid::encode_one(&current)
        .map_err(|error| format!("failed to re-encode faucet stable state: {error}"))?;
    if canonical_current == bytes {
        return Ok(current);
    }
    validate_deployed_wider_v1_schema(bytes)?;
    Ok(current)
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode faucet stable state"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode faucet stable state")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        decode_versioned_stable_state(bytes.as_ref()).unwrap_or_else(|err| panic!("{err}"))
    }

    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        const { std::cell::RefCell::new(None) };
    static STABLE_SKIP_RANGE_MAP: std::cell::RefCell<Option<StableBTreeMap<U64Key, U64Value, Memory>>> =
        const { std::cell::RefCell::new(None) };
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
    static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static PERSISTENCE_DIRTY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn with_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized);
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("faucet stable cell not initialized"))
    })
}

fn persist_snapshot(st: &State) {
    with_stable_cell(|cell| {
        cell.set(VersionedStableState::V1(st.clone()));
    });
}

fn with_skip_range_map<R>(f: impl FnOnce(&mut StableBTreeMap<U64Key, U64Value, Memory>) -> R) -> R {
    STABLE_SKIP_RANGE_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(1));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("faucet skip-range stable map not initialized"))
    })
}

// Skip ranges are a durable replay-work cache for history spans that are known to be
// irrelevant under the current faucet attribution policy. Rescue upgrades conservatively
// clear the cache before the faucet resumes, and any future maintenance that bypasses that
// default must still clear the cache whenever commitment-validity rules change.
#[cfg(test)]
pub(crate) fn list_skip_ranges() -> Vec<SkipRange> {
    with_skip_range_map(|map| {
        map.iter()
            .map(|entry| {
                let (start, end) = entry.into_pair();
                SkipRange {
                    start_tx_id: start.get(),
                    end_tx_id: end.get(),
                }
            })
            .collect()
    })
}

/// Lookup one interval, without materialising the stable map. Endpoints are
/// inclusive low/high global block IDs, independent of traversal direction.
pub(crate) fn skip_range_containing(id: u64) -> Result<Option<SkipRange>, SkipRangeInsertError> {
    #[cfg(test)]
    if skip_cache_test_access(false)? {
        return Ok(None);
    }
    with_skip_range_map(|map| {
        let Some(entry) = map.range(..=U64Key::from(id)).next_back() else {
            return Ok(None);
        };
        let (low, high) = entry.into_pair();
        if low.get() > high.get() {
            return Err(SkipRangeInsertError::InvalidRange);
        }
        Ok((id <= high.get()).then_some(SkipRange {
            start_tx_id: low.get(),
            end_tx_id: high.get(),
        }))
    })
}

/// Insert only independently proven exclusion evidence. Union is safe only for
/// overlap or numerical adjacency; a gap is never invented as covered evidence.
/// Inspect at most four neighbours (one predecessor and three successors). If a new interval would absorb more than
/// two old entries, conservatively retain the old cache instead of doing
/// unbounded merge work. Normal descending learning stops at existing evidence.
pub(crate) fn insert_skip_range(mut range: SkipRange) -> Result<(), SkipRangeInsertError> {
    if range.start_tx_id > range.end_tx_id {
        return Err(SkipRangeInsertError::InvalidRange);
    }
    #[cfg(test)]
    if skip_cache_test_access(true)? {
        return Ok(());
    }
    with_skip_range_map(|map| {
        let mut remove = Vec::with_capacity(2);
        if let Some(entry) = map.range(..=U64Key::from(range.start_tx_id)).next_back() {
            let (low, high) = entry.into_pair();
            if low.get() > high.get() {
                return Err(SkipRangeInsertError::InvalidRange);
            }
            if high.get() >= range.end_tx_id {
                return Ok(());
            }
            if high.get().saturating_add(1) >= range.start_tx_id {
                range.start_tx_id = low.get();
                range.end_tx_id = range.end_tx_id.max(high.get());
                remove.push(low);
            }
        }
        for entry in map
            .range((
                std::ops::Bound::Excluded(U64Key::from(range.start_tx_id)),
                std::ops::Bound::Unbounded,
            ))
            .take(3)
        {
            let (low, high) = entry.into_pair();
            if low.get() > high.get() {
                return Err(SkipRangeInsertError::InvalidRange);
            }
            if low.get() > range.end_tx_id.saturating_add(1) {
                break;
            }
            if remove.len() == 2 {
                return Ok(());
            }
            range.end_tx_id = range.end_tx_id.max(high.get());
            remove.push(low);
        }
        for key in remove {
            map.remove(&key);
        }
        map.insert(
            U64Key::from(range.start_tx_id),
            U64Value::from(range.end_tx_id),
        );
        Ok(())
    })
}

#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(crate) struct SkipCacheTestStats {
    pub lookups: u64,
    pub insertions: u64,
    pub disabled: bool,
    pub fail_next_insert: bool,
}
#[cfg(test)]
thread_local! { pub(crate) static SKIP_CACHE_TEST_STATS: std::cell::RefCell<SkipCacheTestStats> = std::cell::RefCell::new(SkipCacheTestStats::default()); }
#[cfg(test)]
pub(crate) fn skip_cache_test_memory_bytes() -> u64 {
    MEMORY_MANAGER.with(|manager| {
        ic_stable_structures::Memory::size(&manager.borrow().get(MemoryId::new(1))) * 65_536
    })
}
#[cfg(test)]
fn skip_cache_test_access(insert: bool) -> Result<bool, SkipRangeInsertError> {
    SKIP_CACHE_TEST_STATS.with(|cell| {
        let mut stats = cell.borrow_mut();
        if insert {
            stats.insertions += 1;
            if std::mem::take(&mut stats.fail_next_insert) {
                return Err(SkipRangeInsertError::InvalidRange);
            }
        } else {
            stats.lookups += 1;
        }
        Ok(stats.disabled)
    })
}

pub(crate) fn latch_forced_rescue_reason(reason: ForcedRescueReason) {
    with_state_mut(|st| {
        if st.forced_rescue_reason.is_none() {
            st.forced_rescue_reason = Some(reason);
        }
    });
}

pub(crate) fn latch_skip_range_invariant_fault() {
    with_state_mut(|st| {
        st.skip_range_invariant_fault = Some(true);
    });
}

pub(crate) fn clear_skip_ranges() {
    // Reset the existing map/allocator in place; do not allocate a whole-map
    // key list during upgrade. The memory ID and wire representation are unchanged.
    with_skip_range_map(|map| map.clear_new());
}

pub(crate) fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub(crate) fn restore_state_from_stable() -> Option<State> {
    with_stable_cell(|cell| match cell.get().clone() {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::V1(st) => Some(st),
    })
}

pub(crate) fn set_state(st: State) {
    persist_snapshot(&st);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub(crate) fn get_state() -> State {
    STATE
        .with(|s| s.borrow().clone())
        .expect("state not initialized")
}

pub(crate) fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

pub(crate) fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| jupiter_persistence_batch::is_active(depth.get()))
}

fn mark_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(true));
}

fn clear_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
}

pub(crate) fn persist_dirty_state() {
    let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
    if !dirty {
        return;
    }
    let snapshot = get_state();
    persist_snapshot(&snapshot);
    clear_persistence_dirty();
}

pub(crate) type PersistenceBatch = jupiter_persistence_batch::PersistenceBatch;

#[must_use]
pub(crate) fn begin_persistence_batch() -> PersistenceBatch {
    PERSISTENCE_BATCH_DEPTH
        .with(|depth| depth.set(jupiter_persistence_batch::begin_depth(depth.get())));
    PersistenceBatch::new(|| {
        let should_flush = PERSISTENCE_BATCH_DEPTH.with(|depth| {
            let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
            let (next_depth, should_flush) =
                jupiter_persistence_batch::finish_depth(depth.get(), dirty);
            depth.set(next_depth);
            should_flush
        });
        if should_flush {
            persist_dirty_state();
        }
    })
}

pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        let immediate_persist = !persistence_batch_active();
        let out = f(st);
        if immediate_persist {
            let snapshot = st.clone();
            drop(borrow);
            persist_snapshot(&snapshot);
            return out;
        }
        mark_persistence_dirty();
        drop(borrow);
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::types::{value::IDLField, value::IDLValue, Label};
    use candid::IDLArgs;

    fn reset_test_storage() {
        with_stable_cell(|cell| {
            cell.set(VersionedStableState::Uninitialized);
        });
        clear_skip_ranges();
        PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(0));
        PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn principal(bytes: &[u8]) -> Principal {
        Principal::from_slice(bytes)
    }

    fn sample_config() -> Config {
        Config {
            staking_account: Account {
                owner: principal(&[1]),
                subaccount: None,
            },
            payout_subaccount: Some([7; 32]),
            ledger_canister_id: principal(&[2]),
            index_canister_id: principal(&[3]),
            cmc_canister_id: principal(&[14]),
            governance_canister_id: Some(principal(&[9])),
            funding_source_account: Account {
                owner: principal(&[8]),
                subaccount: None,
            },
            rescue_controller: principal(&[5]),
            autonomous_rescue_armed: Some(false),
            expected_first_staking_tx_id: Some(11),
            main_interval_seconds: 60,
            rescue_interval_seconds: 120,
            min_tx_e8s: 100_000_000,
            stake_recognition_delay_seconds: Some(24 * 60 * 60),
        }
    }

    fn parse_wire(bytes: &[u8]) -> (TypeEnv, Vec<Type>, IDLArgs, String, String) {
        let mut reader = Cursor::new(bytes);
        let header = candid::binary_parser::Header::read_args(&mut reader, (None,))
            .expect("parse current stable-state type table");
        let (env, types) = header
            .to_types()
            .expect("resolve current stable-state types");
        let args = IDLArgs::from_bytes_with_types(bytes, &env, &types)
            .expect("decode current stable-state values with wire types");

        let root = env.trace_type(&types[0]).expect("trace current root");
        let TypeInner::Variant(versions) = root.as_ref() else {
            panic!("current stable-state root must be a variant");
        };
        let v1_type = field_type(versions, "V1").expect("current V1 variant");
        let TypeInner::Var(state_name) = v1_type.as_ref() else {
            panic!("current V1 must reference the State record");
        };
        let state_record = env.find_type(state_name).expect("current State record");
        let TypeInner::Record(state_fields) = state_record.as_ref() else {
            panic!("current State type must be a record");
        };
        let active_job_type =
            field_type(state_fields, "active_payout_job").expect("current active payout job field");
        let job_name = referenced_record_name(&env, &active_job_type, "active_payout_job")
            .expect("current ActivePayoutJob record");

        (env, types, args, state_name.clone(), job_name)
    }

    fn current_wire_state(state: State) -> (TypeEnv, Vec<Type>, IDLArgs, String, String) {
        let bytes = candid::encode_one(VersionedStableState::V1(state))
            .expect("encode current stable state");
        parse_wire(&bytes)
    }

    fn replace_record_field_type(env: &mut TypeEnv, record_name: &str, name: &str, ty: Type) {
        let record = env
            .find_type(record_name)
            .unwrap_or_else(|error| panic!("find record {record_name}: {error}"))
            .clone();
        let TypeInner::Record(mut fields) = record.as_ref().clone() else {
            panic!("{record_name} must be a record");
        };
        let id = candid::idl_hash(name);
        let field = fields
            .iter_mut()
            .find(|field| field.id.get_id() == id)
            .unwrap_or_else(|| panic!("field {name} must exist in {record_name}"));
        field.ty = ty;
        env.0
            .insert(record_name.to_string(), TypeInner::Record(fields).into());
    }

    fn insert_record_field_type(env: &mut TypeEnv, record_name: &str, name: &str, ty: Type) {
        let record = env
            .find_type(record_name)
            .unwrap_or_else(|error| panic!("find record {record_name}: {error}"))
            .clone();
        let TypeInner::Record(mut fields) = record.as_ref().clone() else {
            panic!("{record_name} must be a record");
        };
        let id = Label::Id(candid::idl_hash(name)).into();
        assert!(fields.iter().all(|field| field.id != id));
        fields.push(Field { id, ty });
        fields.sort_by_key(|field| field.id.get_id());
        env.0
            .insert(record_name.to_string(), TypeInner::Record(fields).into());
    }

    fn remove_record_field_type(env: &mut TypeEnv, record_name: &str, name: &str) {
        let record = env
            .find_type(record_name)
            .unwrap_or_else(|error| panic!("find record {record_name}: {error}"))
            .clone();
        let TypeInner::Record(mut fields) = record.as_ref().clone() else {
            panic!("{record_name} must be a record");
        };
        let id = candid::idl_hash(name);
        let before = fields.len();
        fields.retain(|field| field.id.get_id() != id);
        assert_eq!(fields.len() + 1, before);
        env.0
            .insert(record_name.to_string(), TypeInner::Record(fields).into());
    }

    fn record_fields_mut(value: &mut IDLValue) -> &mut Vec<IDLField> {
        let IDLValue::Record(fields) = value else {
            panic!("expected record value");
        };
        fields
    }

    fn state_fields_mut(args: &mut IDLArgs) -> &mut Vec<IDLField> {
        let IDLValue::Variant(version) = &mut args.args[0] else {
            panic!("expected versioned stable-state value");
        };
        record_fields_mut(&mut version.0.val)
    }

    fn value_field_mut<'a>(fields: &'a mut [IDLField], name: &str) -> &'a mut IDLValue {
        let id = candid::idl_hash(name);
        &mut fields
            .iter_mut()
            .find(|field| field.id.get_id() == id)
            .unwrap_or_else(|| panic!("value field {name} must exist"))
            .val
    }

    fn insert_value_field(fields: &mut Vec<IDLField>, name: &str, value: IDLValue) {
        let id = Label::Id(candid::idl_hash(name));
        assert!(fields.iter().all(|field| field.id != id));
        fields.push(IDLField { id, val: value });
        fields.sort_by_key(|field| field.id.get_id());
    }

    fn remove_value_field(fields: &mut Vec<IDLField>, name: &str) {
        let id = candid::idl_hash(name);
        let before = fields.len();
        fields.retain(|field| field.id.get_id() != id);
        assert_eq!(fields.len() + 1, before);
    }

    fn active_job_fields_mut(args: &mut IDLArgs) -> &mut Vec<IDLField> {
        let active = value_field_mut(state_fields_mut(args), "active_payout_job");
        let IDLValue::Opt(job) = active else {
            panic!("expected a present active payout job");
        };
        record_fields_mut(job)
    }

    fn encode_wire(env: &TypeEnv, types: &[Type], args: &IDLArgs) -> Vec<u8> {
        args.to_bytes_with_types(env, types)
            .expect("encode synthetic stable-state wire value")
    }

    fn deployed_wider_v1_bytes(
        state: State,
        carried_e8s: u64,
        job_start_e8s: u64,
        job_end_e8s: u64,
    ) -> Vec<u8> {
        let (mut env, types, mut args, state_name, job_name) = current_wire_state(state);
        let opt_nat64: Type = TypeInner::Opt(TypeInner::Nat64.into()).into();
        insert_record_field_type(
            &mut env,
            &state_name,
            RETIRED_STATE_FIELD,
            opt_nat64.clone(),
        );
        for field in RETIRED_JOB_FIELDS {
            insert_record_field_type(&mut env, &job_name, field, opt_nat64.clone());
        }
        insert_value_field(
            state_fields_mut(&mut args),
            RETIRED_STATE_FIELD,
            IDLValue::Opt(Box::new(IDLValue::Nat64(carried_e8s))),
        );
        if matches!(
            value_field_mut(state_fields_mut(&mut args), "active_payout_job"),
            IDLValue::Opt(_)
        ) {
            let job_fields = active_job_fields_mut(&mut args);
            insert_value_field(
                job_fields,
                RETIRED_JOB_FIELDS[0],
                IDLValue::Opt(Box::new(IDLValue::Nat64(job_start_e8s))),
            );
            insert_value_field(
                job_fields,
                RETIRED_JOB_FIELDS[1],
                IDLValue::Opt(Box::new(IDLValue::Nat64(job_end_e8s))),
            );
        }
        encode_wire(&env, &types, &args)
    }

    fn wider_wire_state(state: State) -> (TypeEnv, Vec<Type>, IDLArgs, String, String) {
        parse_wire(&deployed_wider_v1_bytes(state, 987_654_321, 123, 456))
    }

    fn sample_pending_transfer(kind: TransferKind, phase: PendingTransferPhase) -> PendingTransfer {
        PendingTransfer {
            notification: PendingNotification {
                kind,
                beneficiary: principal(&[44]),
                gross_share_e8s: 90_000_000,
                amount_e8s: 89_990_000,
                block_index: 777,
                next_start: Some(88),
                transfer_memo: Some(vec![1, 2, 3]),
                destination_subaccount: Some([6; 32]),
                neuron_id: Some(55),
            },
            created_at_time_nanos: 123_456_789,
            phase,
        }
    }

    fn sample_active_job() -> ActivePayoutJob {
        let mut job = ActivePayoutJob::new(23, 10_000, 500_000_000, 700_000_000, 999);
        job.configure_round_accounting(
            Some(10_000_000_000),
            Some(101),
            20_000_000_000,
            Some(202),
            345_678_901,
            false,
        );
        job.configure_funding_tranche(202, 20_000_000_000, 500_000_000);
        job.next_start = Some(150);
        job.observed_oldest_tx_id = Some(11);
        job.observed_latest_tx_id = Some(222);
        job
    }

    #[test]
    fn runtime_config_log_line_includes_all_config_fields() {
        let line = runtime_config_log_line(&sample_config());
        assert!(line.starts_with("CONFIG "));
        assert!(line.contains("staking_account="));
        assert!(line.contains(
            "payout_subaccount=0707070707070707070707070707070707070707070707070707070707070707"
        ));
        assert!(line.contains("ledger_canister_id="));
        assert!(line.contains("index_canister_id="));
        assert!(line.contains("cmc_canister_id="));
        assert!(line.contains("governance_canister_id="));
        assert!(line.contains(&format!(
            "canonical_relay_canister_id={}",
            crate::canonical_relay_canister_id()
        )));
        assert!(line.contains("funding_source_account="));
        assert!(line.contains("rescue_controller="));
        assert!(line.contains("autonomous_rescue_armed=false"));
        assert!(line.contains("expected_first_staking_tx_id=11"));
        assert!(line.contains("main_interval_seconds=60"));
        assert!(line.contains("rescue_interval_seconds=120"));
        assert!(line.contains("min_tx_e8s=100000000"));
        assert!(line.contains("stake_recognition_delay_seconds=86400"));
    }

    #[test]
    fn transfer_kind_route_semantics_are_explicit() {
        assert!(TransferKind::Beneficiary.is_beneficiary_payout());
        assert!(TransferKind::Beneficiary.requires_cmc_notify());
        assert!(TransferKind::CyclesTopUpRawFallback.is_beneficiary_payout());
        assert!(!TransferKind::CyclesTopUpRawFallback.requires_cmc_notify());
        assert!(TransferKind::RawIcp.is_beneficiary_payout());
        assert!(!TransferKind::RawIcp.requires_cmc_notify());
        assert!(TransferKind::NeuronStake.is_beneficiary_payout());
        assert!(!TransferKind::NeuronStake.requires_cmc_notify());
        assert!(!TransferKind::RemainderToRelay.is_beneficiary_payout());
        assert!(!TransferKind::RemainderToRelay.requires_cmc_notify());
    }

    #[test]
    fn runtime_state_log_line_includes_recovery_observability_fields() {
        let mut st = State::new(sample_config(), 0);
        assert!(runtime_state_log_line(&st).contains("active_payout_job_present=false"));
        st.last_processed_funding_tx_id = Some(42);
        st.forced_rescue_reason = Some(ForcedRescueReason::FundingTrancheBalanceMismatch);
        st.last_observed_staking_balance_e8s = Some(300_000_000);
        st.last_observed_latest_tx_id = Some(1234);
        st.consecutive_index_anchor_failures = Some(1);
        st.consecutive_index_latest_invariant_failures = Some(2);
        st.consecutive_index_latest_unreadable_failures = Some(3);
        st.active_funding_scan = Some(FundingScanState {
            anchor_last_processed_funding_tx_id: Some(41),
            cursor: Some(500),
            candidate: Some(FundingTrancheState {
                tx_id: 43,
                timestamp_nanos: 123,
                amount_e8s: 100_000_000,
            }),
        });
        let mut job = ActivePayoutJob::new(7, 10_000, 100_000_000, 200_000_000, 1);
        job.configure_funding_tranche(43, 123, 100_000_000);
        st.active_payout_job = Some(job);

        let line = runtime_state_log_line(&st);

        assert!(line.starts_with("STATE:"));
        assert!(line.contains("last_processed_funding_tx_id=42"));
        assert!(line.contains("forced_rescue_reason=FundingTrancheBalanceMismatch"));
        assert!(line.contains("last_observed_staking_balance_e8s=300000000"));
        assert!(line.contains("last_observed_latest_tx_id=1234"));
        assert!(line.contains("consecutive_index_anchor_failures=1"));
        assert!(line.contains("consecutive_index_latest_invariant_failures=2"));
        assert!(line.contains("consecutive_index_latest_unreadable_failures=3"));
        assert!(line.contains("active_funding_scan_cursor=500"));
        assert!(line.contains("active_funding_scan_candidate_tx_id=43"));
        assert!(line.contains("active_funding_scan_candidate_amount_e8s=100000000"));
        assert!(line.contains("active_funding_scan_anchor_last_processed_funding_tx_id=41"));
        assert!(line.contains("active_payout_job_present=true"));
        assert!(line.contains("active_payout_funding_tx_id=43"));
        assert!(line.contains("active_payout_funding_amount_e8s=100000000"));
    }

    #[test]
    fn stable_restore_is_none_before_first_persist() {
        reset_test_storage();
        assert!(restore_state_from_stable().is_none());
    }

    #[test]
    fn current_v1_state_round_trips_through_stable_storage() {
        const ROUND_START_TIME_NANOS: u64 = 20_000_000_000;
        const ROUND_START_TX_ID: u64 = 41;

        reset_test_storage();
        let mut st = State::new(sample_config(), 1_000);
        st.last_successful_transfer_ts = Some(77);
        st.main_lock_state_ts = Some(33);
        st.payout_nonce = 19;
        st.config.autonomous_rescue_armed = Some(true);
        st.autonomous_rescue_armed_since_ts = Some(999);
        st.rescue_triggered = true;
        st.last_summary = Some(Summary {
            pot_start_e8s: 300_000_000,
            pot_remaining_e8s: 12_345_678,
            funding_tx_id: Some(41),
            funding_amount_e8s: Some(300_000_000),
            last_processed_funding_tx_id: Some(41),
            remainder_to_relay_e8s: 12_345_678,
            ..Summary::default()
        });
        st.last_processed_funding_tx_id = Some(41);
        st.current_round_start_time_nanos = Some(ROUND_START_TIME_NANOS);
        st.current_round_start_latest_tx_id = Some(ROUND_START_TX_ID);
        st.active_funding_scan = Some(FundingScanState {
            anchor_last_processed_funding_tx_id: Some(41),
            cursor: Some(500),
            candidate: Some(FundingTrancheState {
                tx_id: 42,
                timestamp_nanos: 123,
                amount_e8s: 400_000_000,
            }),
        });
        set_state(st.clone());

        let restored = restore_state_from_stable().expect("expected persisted faucet state");
        assert_eq!(restored.last_successful_transfer_ts, Some(77));
        assert_eq!(restored.main_lock_state_ts, Some(33));
        assert_eq!(restored.payout_nonce, 19);
        assert_eq!(restored.config.min_tx_e8s, st.config.min_tx_e8s);
        assert_eq!(restored.config.autonomous_rescue_armed, Some(true));
        assert_eq!(restored.autonomous_rescue_armed_since_ts, Some(999));
        assert!(restored.rescue_triggered);
        assert_eq!(restored.last_summary, st.last_summary);
        assert_eq!(restored.last_processed_funding_tx_id, Some(41));
        assert_eq!(
            restored.current_round_start_time_nanos,
            Some(ROUND_START_TIME_NANOS)
        );
        assert_eq!(
            restored.current_round_start_latest_tx_id,
            Some(ROUND_START_TX_ID)
        );
        assert_eq!(restored.active_funding_scan, st.active_funding_scan);
    }

    #[test]
    fn deployed_wider_v1_decodes_supported_state_and_reencodes_reduced_schema() {
        let mut state = State::new(sample_config(), 7_000);
        state.last_successful_transfer_ts = Some(6_999);
        state.last_rescue_check_ts = 6_998;
        state.rescue_triggered = true;
        state.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestInvariantBroken);
        state.consecutive_index_anchor_failures = Some(2);
        state.consecutive_cmc_zero_success_runs = Some(3);
        state.last_observed_staking_balance_e8s = Some(876_543_210);
        state.last_observed_latest_tx_id = Some(808);
        state.payout_nonce = 27;
        state.current_round_start_time_nanos = Some(11_000_000_000);
        state.current_round_start_latest_tx_id = Some(707);
        state.last_processed_funding_tx_id = Some(707);
        state.last_summary = Some(Summary {
            pot_start_e8s: 500_000_000,
            pot_remaining_e8s: 12_345,
            denom_staking_balance_e8s: 765_432_100,
            effective_denom_staking_balance_e8s: Some(654_321_000),
            funding_tx_id: Some(707),
            funding_amount_e8s: Some(500_000_000),
            round_end_latest_tx_id: Some(707),
            round_end_time_nanos: Some(11_000_000_000),
            last_processed_funding_tx_id: Some(707),
            topped_up_count: 2,
            topped_up_sum_e8s: 487_654_321,
            topped_up_min_e8s: Some(200_000_000),
            topped_up_max_e8s: Some(287_654_321),
            failed_topups: 1,
            ambiguous_topups: 1,
            ignored_under_threshold: 4,
            ignored_bad_memo: 5,
            remainder_to_relay_e8s: 12_345,
        });
        state.active_funding_scan = Some(FundingScanState {
            anchor_last_processed_funding_tx_id: Some(707),
            cursor: Some(900),
            candidate: Some(FundingTrancheState {
                tx_id: 808,
                timestamp_nanos: 12_000_000_000,
                amount_e8s: 600_000_000,
            }),
        });

        let bytes = deployed_wider_v1_bytes(state.clone(), 999_999_999, 111, 222);
        let VersionedStableState::V1(restored) =
            decode_versioned_stable_state(&bytes).expect("decode deployed wider V1")
        else {
            panic!("expected V1 state");
        };

        assert_eq!(
            restored.config.staking_account,
            state.config.staking_account
        );
        assert_eq!(
            restored.config.payout_subaccount,
            state.config.payout_subaccount
        );
        assert_eq!(
            restored.config.ledger_canister_id,
            state.config.ledger_canister_id
        );
        assert_eq!(
            restored.config.index_canister_id,
            state.config.index_canister_id
        );
        assert_eq!(
            restored.config.cmc_canister_id,
            state.config.cmc_canister_id
        );
        assert_eq!(
            restored.config.governance_canister_id,
            state.config.governance_canister_id
        );
        assert_eq!(
            restored.config.funding_source_account,
            state.config.funding_source_account
        );
        assert_eq!(
            restored.config.rescue_controller,
            state.config.rescue_controller
        );
        assert_eq!(
            restored.config.autonomous_rescue_armed,
            state.config.autonomous_rescue_armed
        );
        assert_eq!(
            restored.config.expected_first_staking_tx_id,
            state.config.expected_first_staking_tx_id
        );
        assert_eq!(
            restored.config.main_interval_seconds,
            state.config.main_interval_seconds
        );
        assert_eq!(
            restored.config.rescue_interval_seconds,
            state.config.rescue_interval_seconds
        );
        assert_eq!(restored.config.min_tx_e8s, state.config.min_tx_e8s);
        assert_eq!(
            restored.config.stake_recognition_delay_seconds,
            state.config.stake_recognition_delay_seconds
        );
        assert_eq!(restored.last_summary, state.last_summary);
        assert_eq!(restored.last_successful_transfer_ts, Some(6_999));
        assert_eq!(restored.last_rescue_check_ts, 6_998);
        assert!(restored.rescue_triggered);
        assert_eq!(restored.forced_rescue_reason, state.forced_rescue_reason);
        assert_eq!(restored.consecutive_index_anchor_failures, Some(2));
        assert_eq!(restored.consecutive_cmc_zero_success_runs, Some(3));
        assert_eq!(
            restored.last_observed_staking_balance_e8s,
            Some(876_543_210)
        );
        assert_eq!(restored.last_observed_latest_tx_id, Some(808));
        assert_eq!(restored.payout_nonce, 27);
        assert_eq!(
            restored.current_round_start_time_nanos,
            Some(11_000_000_000)
        );
        assert_eq!(restored.current_round_start_latest_tx_id, Some(707));
        assert_eq!(restored.last_processed_funding_tx_id, Some(707));
        assert_eq!(restored.active_funding_scan, state.active_funding_scan);
        assert!(restored.active_payout_job.is_none());

        let reduced = candid::encode_one(VersionedStableState::V1(restored))
            .expect("encode reduced current V1");
        let (env, _, _, state_name, job_name) = parse_wire(&reduced);
        let TypeInner::Record(state_fields) = env
            .find_type(&state_name)
            .expect("reduced State record")
            .as_ref()
        else {
            panic!("reduced State must remain a record");
        };
        assert!(field_type(state_fields, RETIRED_STATE_FIELD).is_err());
        let TypeInner::Record(job_fields) = env
            .find_type(&job_name)
            .expect("reduced ActivePayoutJob record")
            .as_ref()
        else {
            panic!("reduced ActivePayoutJob must remain a record");
        };
        for field in RETIRED_JOB_FIELDS {
            assert!(field_type(job_fields, field).is_err());
        }
    }

    #[test]
    fn deployed_wider_v1_active_jobs_remain_present_for_quiescence_in_all_phases() {
        let mut phases = Vec::new();

        let mut denominator_scan = sample_active_job();
        denominator_scan.effective_denom_scan_complete = Some(false);
        phases.push(("denominator scan", denominator_scan));

        let mut beneficiary_scan = sample_active_job();
        beneficiary_scan.effective_denom_scan_complete = Some(true);
        beneficiary_scan.scan_complete = false;
        phases.push(("beneficiary scan", beneficiary_scan));

        let mut pending_ledger = sample_active_job();
        pending_ledger.effective_denom_scan_complete = Some(true);
        pending_ledger.pending_transfer = Some(sample_pending_transfer(
            TransferKind::Beneficiary,
            PendingTransferPhase::AwaitingTransfer,
        ));
        phases.push(("pending Ledger transfer", pending_ledger));

        let mut awaiting_cmc = sample_active_job();
        awaiting_cmc.effective_denom_scan_complete = Some(true);
        awaiting_cmc.pending_transfer = Some(sample_pending_transfer(
            TransferKind::Beneficiary,
            PendingTransferPhase::TransferAccepted,
        ));
        phases.push(("accepted Ledger transfer awaiting CMC", awaiting_cmc));

        let mut remainder = sample_active_job();
        remainder.effective_denom_scan_complete = Some(true);
        remainder.scan_complete = true;
        remainder.pending_transfer = Some(sample_pending_transfer(
            TransferKind::RemainderToRelay,
            PendingTransferPhase::AwaitingTransfer,
        ));
        phases.push(("remainder processing", remainder));

        for (phase, job) in phases {
            let expected_pending = job.pending_transfer.clone();
            let mut state = State::new(sample_config(), 1_000);
            state.active_payout_job = Some(job);
            let bytes = deployed_wider_v1_bytes(state, 999, 111, 222);
            let VersionedStableState::V1(restored) = decode_versioned_stable_state(&bytes)
                .unwrap_or_else(|error| panic!("decode {phase}: {error}"))
            else {
                panic!("expected V1 state for {phase}");
            };
            let restored_job = restored
                .active_payout_job
                .as_ref()
                .unwrap_or_else(|| panic!("{phase} active job must not disappear"));
            assert_eq!(restored_job.id, 23, "{phase}");
            assert_eq!(restored_job.pending_transfer, expected_pending, "{phase}");
            assert_eq!(
                crate::validate_upgrade_quiescence(&restored),
                Err(UPGRADE_QUIESCENCE_ERROR.to_string()),
                "{phase}"
            );
        }
    }

    #[test]
    fn deployed_wider_v1_rejects_incompatible_optional_payloads_and_missing_fields() {
        let opt_text: Type = TypeInner::Opt(TypeInner::Text.into()).into();

        let mut active_state = State::new(sample_config(), 1_000);
        active_state.active_payout_job = Some(sample_active_job());
        let (mut env, types, mut args, state_name, _) = wider_wire_state(active_state);
        replace_record_field_type(&mut env, &state_name, "active_payout_job", opt_text.clone());
        *value_field_mut(state_fields_mut(&mut args), "active_payout_job") =
            IDLValue::Opt(Box::new(IDLValue::Text("incompatible job".to_string())));
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());

        let mut pending_state = State::new(sample_config(), 1_000);
        let mut pending_job = sample_active_job();
        pending_job.pending_transfer = Some(sample_pending_transfer(
            TransferKind::Beneficiary,
            PendingTransferPhase::TransferAccepted,
        ));
        pending_state.active_payout_job = Some(pending_job);
        let (mut env, types, mut args, _, job_name) = wider_wire_state(pending_state);
        replace_record_field_type(&mut env, &job_name, "pending_transfer", opt_text.clone());
        *value_field_mut(active_job_fields_mut(&mut args), "pending_transfer") = IDLValue::Opt(
            Box::new(IDLValue::Text("incompatible transfer".to_string())),
        );
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());

        let mut summary_state = State::new(sample_config(), 1_000);
        summary_state.last_summary = Some(Summary::default());
        let (mut env, types, mut args, state_name, _) = wider_wire_state(summary_state);
        replace_record_field_type(&mut env, &state_name, "last_summary", opt_text.clone());
        *value_field_mut(state_fields_mut(&mut args), "last_summary") =
            IDLValue::Opt(Box::new(IDLValue::Text("incompatible summary".to_string())));
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());

        let mut scan_state = State::new(sample_config(), 1_000);
        scan_state.active_funding_scan = Some(FundingScanState::default());
        let (mut env, types, mut args, state_name, _) = wider_wire_state(scan_state);
        replace_record_field_type(
            &mut env,
            &state_name,
            "active_funding_scan",
            opt_text.clone(),
        );
        *value_field_mut(state_fields_mut(&mut args), "active_funding_scan") =
            IDLValue::Opt(Box::new(IDLValue::Text("incompatible scan".to_string())));
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());

        let mut rescue_state = State::new(sample_config(), 1_000);
        rescue_state.forced_rescue_reason = Some(ForcedRescueReason::IndexLatestInvariantBroken);
        let (mut env, types, mut args, state_name, _) = wider_wire_state(rescue_state);
        replace_record_field_type(
            &mut env,
            &state_name,
            "forced_rescue_reason",
            opt_text.clone(),
        );
        *value_field_mut(state_fields_mut(&mut args), "forced_rescue_reason") =
            IDLValue::Opt(Box::new(IDLValue::Text("incompatible rescue".to_string())));
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());

        let config_state = State::new(sample_config(), 1_000);
        let (mut env, types, mut args, state_name, _) = wider_wire_state(config_state);
        let state_record = env.find_type(&state_name).expect("State record");
        let TypeInner::Record(state_type_fields) = state_record.as_ref() else {
            panic!("State must be a record");
        };
        let config_type = field_type(state_type_fields, "config").expect("config field");
        let TypeInner::Var(config_name) = config_type.as_ref() else {
            panic!("config must reference a record");
        };
        let config_name = config_name.clone();
        replace_record_field_type(&mut env, &config_name, "autonomous_rescue_armed", opt_text);
        let config_value = value_field_mut(state_fields_mut(&mut args), "config");
        *value_field_mut(record_fields_mut(config_value), "autonomous_rescue_armed") =
            IDLValue::Opt(Box::new(IDLValue::Text("incompatible config".to_string())));
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());

        let missing_state = State::new(sample_config(), 1_000);
        let (mut env, types, mut args, state_name, _) = wider_wire_state(missing_state);
        remove_record_field_type(&mut env, &state_name, "current_round_start_time_nanos");
        remove_value_field(
            state_fields_mut(&mut args),
            "current_round_start_time_nanos",
        );
        assert!(decode_versioned_stable_state(&encode_wire(&env, &types, &args)).is_err());
    }

    #[test]
    fn stable_decoder_rejects_wrong_variant_malformed_and_invalid_message_shapes() {
        let wrong_variant = IDLArgs::new(&[IDLValue::Variant(candid::types::value::VariantValue(
            Box::new(IDLField {
                id: Label::Named("V2".to_string()),
                val: IDLValue::Null,
            }),
            0,
        ))])
        .to_bytes()
        .expect("encode wrong stable-state variant");
        assert!(decode_versioned_stable_state(&wrong_variant).is_err());

        let invalid_shape = candid::encode_one(42_u64).expect("encode invalid root shape");
        assert!(decode_versioned_stable_state(&invalid_shape).is_err());

        let two_values = candid::encode_args((VersionedStableState::Uninitialized, 42_u64))
            .expect("encode invalid two-value message");
        assert!(decode_versioned_stable_state(&two_values).is_err());

        let mut truncated = deployed_wider_v1_bytes(State::new(sample_config(), 1_000), 9, 8, 7);
        truncated.truncate(truncated.len() / 2);
        assert!(decode_versioned_stable_state(&truncated).is_err());

        assert!(decode_versioned_stable_state(b"not candid").is_err());
    }

    #[test]
    fn current_config_preserves_explicit_funding_source() {
        let explicit = Account {
            owner: principal(&[42]),
            subaccount: Some([11; 32]),
        };
        let mut st = State::new(sample_config(), 1_000);
        st.config.funding_source_account = explicit;
        let current = VersionedStableState::V1(st);
        let bytes = current.to_bytes();

        let decoded = VersionedStableState::from_bytes(bytes);
        let VersionedStableState::V1(restored) = decoded else {
            panic!("expected V1 faucet state");
        };
        assert_eq!(restored.config.funding_source_account, explicit);
    }

    #[test]
    fn current_state_roundtrip_preserves_nonzero_remainder_to_relay_e8s() {
        let mut st = State::new(sample_config(), 1_000);
        st.last_summary = Some(Summary {
            remainder_to_relay_e8s: 12_345_678,
            ..Summary::default()
        });
        let bytes = candid::encode_one(VersionedStableState::V1(st))
            .expect("encode current faucet stable state");

        let VersionedStableState::V1(restored) =
            decode_versioned_stable_state(&bytes).expect("decode current faucet stable state")
        else {
            panic!("expected current V1 faucet state");
        };

        assert_eq!(
            restored
                .last_summary
                .expect("current summary should survive")
                .remainder_to_relay_e8s,
            12_345_678
        );
    }

    #[test]
    fn reduced_current_v1_active_job_persists_without_applying_upgrade_quiescence() {
        let mut state = State::new(sample_config(), 1_000);
        let mut job = sample_active_job();
        job.pending_transfer = Some(sample_pending_transfer(
            TransferKind::Beneficiary,
            PendingTransferPhase::TransferAccepted,
        ));
        state.active_payout_job = Some(job);
        let bytes =
            candid::encode_one(VersionedStableState::V1(state)).expect("encode reduced active job");

        let VersionedStableState::V1(restored) = decode_versioned_stable_state(&bytes)
            .expect("ordinary reduced active-job persistence must decode")
        else {
            panic!("expected V1 state");
        };
        let restored_job = restored
            .active_payout_job
            .as_ref()
            .expect("active job must persist");
        assert_eq!(restored_job.id, 23);
        assert_eq!(
            restored_job
                .pending_transfer
                .as_ref()
                .expect("pending transfer must persist")
                .phase,
            PendingTransferPhase::TransferAccepted
        );
        assert_eq!(
            crate::validate_upgrade_quiescence(&restored),
            Err(UPGRADE_QUIESCENCE_ERROR.to_string())
        );
    }

    #[test]
    fn with_state_mut_persists_updates_to_stable_storage() {
        reset_test_storage();
        set_state(State::new(sample_config(), 2_000));

        with_state_mut(|st| {
            st.last_observed_staking_balance_e8s = Some(555);
            st.main_lock_state_ts = Some(99);
        });

        let restored =
            restore_state_from_stable().expect("expected persisted faucet state after mutation");
        assert_eq!(restored.last_observed_staking_balance_e8s, Some(555));
        assert_eq!(restored.main_lock_state_ts, Some(99));
    }

    #[test]
    fn persistence_batch_defers_writes_until_flush_boundary() {
        reset_test_storage();
        set_state(State::new(sample_config(), 3_000));

        {
            let _batch = begin_persistence_batch();
            with_state_mut(|st| {
                st.last_observed_staking_balance_e8s = Some(777);
                st.main_lock_state_ts = Some(123);
            });
            let restored_mid = restore_state_from_stable()
                .expect("expected persisted state before batch mutation");
            assert_ne!(restored_mid.last_observed_staking_balance_e8s, Some(777));
            assert_ne!(restored_mid.main_lock_state_ts, Some(123));
            persist_dirty_state();
        }

        let restored =
            restore_state_from_stable().expect("expected persisted state after batch flush");
        assert_eq!(restored.last_observed_staking_balance_e8s, Some(777));
        assert_eq!(restored.main_lock_state_ts, Some(123));
    }

    #[test]
    fn skip_ranges_round_trip_through_dedicated_stable_map() {
        reset_test_storage();
        insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 25,
        })
        .expect("first skip range should persist");
        insert_skip_range(SkipRange {
            start_tx_id: 40,
            end_tx_id: 60,
        })
        .expect("second skip range should persist");

        assert_eq!(
            list_skip_ranges(),
            vec![
                SkipRange {
                    start_tx_id: 10,
                    end_tx_id: 25
                },
                SkipRange {
                    start_tx_id: 40,
                    end_tx_id: 60
                },
            ]
        );
    }

    #[test]
    fn clear_skip_ranges_removes_all_entries() {
        reset_test_storage();
        insert_skip_range(SkipRange {
            start_tx_id: 100,
            end_tx_id: 200,
        })
        .expect("first skip range should persist");
        insert_skip_range(SkipRange {
            start_tx_id: 400,
            end_tx_id: 800,
        })
        .expect("second skip range should persist");

        clear_skip_ranges();

        assert!(list_skip_ranges().is_empty());
    }

    #[test]
    fn skip_range_insertion_merges_adjacent_ranges() {
        reset_test_storage();
        insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 20,
        })
        .expect("baseline range should persist");

        let result = insert_skip_range(SkipRange {
            start_tx_id: 21,
            end_tx_id: 30,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn skip_range_insertion_is_idempotent_for_same_start() {
        reset_test_storage();
        insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 20,
        })
        .expect("baseline range should persist");

        let result = insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 30,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn latch_forced_rescue_reason_only_sets_the_first_reason() {
        reset_test_storage();
        set_state(State::new(sample_config(), 42));

        latch_forced_rescue_reason(ForcedRescueReason::IndexLatestInvariantBroken);
        latch_forced_rescue_reason(ForcedRescueReason::BootstrapNoSuccess);

        let forced = with_state(|st| st.forced_rescue_reason.clone());
        assert_eq!(forced, Some(ForcedRescueReason::IndexLatestInvariantBroken));
    }

    #[test]
    fn latch_skip_range_invariant_fault_sets_sticky_fault_flag() {
        reset_test_storage();
        set_state(State::new(sample_config(), 42));

        latch_skip_range_invariant_fault();
        latch_skip_range_invariant_fault();

        let fault = with_state(|st| st.skip_range_invariant_fault);
        assert_eq!(fault, Some(true));
    }
}
