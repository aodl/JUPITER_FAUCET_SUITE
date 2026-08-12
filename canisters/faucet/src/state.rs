use candid::{CandidType, Deserialize, Principal};
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
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,
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
        "CONFIG staking_account={}, payout_subaccount={}, ledger_canister_id={}, index_canister_id={}, cmc_canister_id={}, governance_canister_id={}, canonical_relay_canister_id={}, funding_source_account={}, rescue_controller={}, blackhole_controller={}, blackhole_armed={}, expected_first_staking_tx_id={}, main_interval_seconds={}, rescue_interval_seconds={}, min_tx_e8s={}, stake_recognition_delay_seconds={}",
        account_text(&cfg.staking_account),
        subaccount_text(&cfg.payout_subaccount),
        cfg.ledger_canister_id.to_text(),
        cfg.index_canister_id.to_text(),
        cfg.cmc_canister_id.to_text(),
        opt_principal_text(cfg.governance_canister_id),
        crate::canonical_relay_canister_id().to_text(),
        account_text(&cfg.funding_source_account),
        cfg.rescue_controller.to_text(),
        opt_principal_text(cfg.blackhole_controller),
        opt_bool_text(cfg.blackhole_armed),
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
        "STATE:last_processed_funding_tx_id={} forced_rescue_reason={} last_observed_staking_balance_e8s={} last_observed_latest_tx_id={} consecutive_index_anchor_failures={} consecutive_index_latest_invariant_failures={} consecutive_index_latest_unreadable_failures={} active_funding_scan_cursor={} active_funding_scan_candidate_tx_id={} active_funding_scan_candidate_amount_e8s={} active_funding_scan_anchor_last_processed_funding_tx_id={} active_payout_job_present={} active_payout_funding_tx_id={} active_payout_funding_amount_e8s={}",
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
    DuplicateStart,
    OverlapsOrAbutsPredecessor,
    OverlapsOrAbutsSuccessor,
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
    pub round_start_staking_balance_e8s: Option<u64>,
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
    pub round_end_staking_balance_e8s: Option<u64>,
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
            round_start_staking_balance_e8s: None,
            round_start_latest_tx_id: None,
            round_end_time_nanos: None,
            round_end_latest_tx_id: None,
            effective_denom_staking_balance_e8s: None,
            effective_denom_scan_complete: None,
            round_end_staking_balance_e8s: None,
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
        round_start_staking_balance_e8s: Option<u64>,
        round_start_latest_tx_id: Option<u64>,
        round_end_time_nanos: u64,
        round_end_latest_tx_id: Option<u64>,
        effective_denom_staking_balance_e8s: u64,
        effective_denom_scan_complete: bool,
    ) {
        self.round_start_time_nanos = round_start_time_nanos;
        self.round_start_staking_balance_e8s = round_start_staking_balance_e8s;
        self.round_start_latest_tx_id = round_start_latest_tx_id;
        self.round_end_time_nanos = Some(round_end_time_nanos);
        self.round_end_latest_tx_id = round_end_latest_tx_id;
        self.effective_denom_staking_balance_e8s = Some(effective_denom_staking_balance_e8s);
        self.effective_denom_scan_complete = Some(effective_denom_scan_complete);
        self.round_end_staking_balance_e8s = Some(effective_denom_staking_balance_e8s);
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
    pub blackhole_armed_since_ts: Option<u64>,
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
    pub current_round_start_staking_balance_e8s: Option<u64>,
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
        let blackhole_armed_since_ts = config.blackhole_armed.unwrap_or(false).then_some(now_secs);
        Self {
            config,
            last_summary: None,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            blackhole_armed_since_ts,
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
            current_round_start_staking_balance_e8s: None,
            current_round_start_latest_tx_id: None,
            last_processed_funding_tx_id: None,
            active_funding_scan: None,
        }
    }
}

// Decode-only representation of the exact stable wire shape written before Relay remainder
// routing was introduced. These types must never enter current payout execution.
#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
enum PreRelayTransferKind {
    Beneficiary,
    RawIcp,
    NeuronStake,
    RemainderToSelf,
}

#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
struct PreRelayPendingNotification {
    kind: PreRelayTransferKind,
    beneficiary: Principal,
    gross_share_e8s: u64,
    amount_e8s: u64,
    block_index: u64,
    next_start: Option<u64>,
    transfer_memo: Option<Vec<u8>>,
    destination_subaccount: Option<[u8; 32]>,
    neuron_id: Option<u64>,
}

#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
enum PreRelayPendingTransferPhase {
    AwaitingTransfer,
    TransferAccepted,
}

#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
struct PreRelayPendingTransfer {
    notification: PreRelayPendingNotification,
    created_at_time_nanos: u64,
    phase: PreRelayPendingTransferPhase,
}

#[derive(CandidType, Deserialize)]
struct PreRelaySummary {
    pot_start_e8s: u64,
    pot_remaining_e8s: u64,
    denom_staking_balance_e8s: u64,
    effective_denom_staking_balance_e8s: Option<u64>,
    funding_tx_id: Option<u64>,
    funding_amount_e8s: Option<u64>,
    round_end_latest_tx_id: Option<u64>,
    round_end_time_nanos: Option<u64>,
    last_processed_funding_tx_id: Option<u64>,
    topped_up_count: u64,
    topped_up_sum_e8s: u64,
    topped_up_min_e8s: Option<u64>,
    topped_up_max_e8s: Option<u64>,
    failed_topups: u64,
    ambiguous_topups: u64,
    ignored_under_threshold: u64,
    ignored_bad_memo: u64,
    remainder_to_self_e8s: u64,
}

impl From<PreRelaySummary> for Summary {
    fn from(value: PreRelaySummary) -> Self {
        Self {
            pot_start_e8s: value.pot_start_e8s,
            pot_remaining_e8s: value.pot_remaining_e8s,
            denom_staking_balance_e8s: value.denom_staking_balance_e8s,
            effective_denom_staking_balance_e8s: value.effective_denom_staking_balance_e8s,
            funding_tx_id: value.funding_tx_id,
            funding_amount_e8s: value.funding_amount_e8s,
            round_end_latest_tx_id: value.round_end_latest_tx_id,
            round_end_time_nanos: value.round_end_time_nanos,
            last_processed_funding_tx_id: value.last_processed_funding_tx_id,
            topped_up_count: value.topped_up_count,
            topped_up_sum_e8s: value.topped_up_sum_e8s,
            topped_up_min_e8s: value.topped_up_min_e8s,
            topped_up_max_e8s: value.topped_up_max_e8s,
            failed_topups: value.failed_topups,
            ambiguous_topups: value.ambiguous_topups,
            ignored_under_threshold: value.ignored_under_threshold,
            ignored_bad_memo: value.ignored_bad_memo,
            remainder_to_relay_e8s: 0,
        }
    }
}

#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
struct PreRelayActivePayoutJob {
    id: u64,
    fee_e8s: u64,
    pot_start_e8s: u64,
    denom_staking_balance_e8s: u64,
    next_start: Option<u64>,
    scan_complete: bool,
    ignored_under_threshold: u64,
    ignored_bad_memo: u64,
    gross_outflow_e8s: u64,
    topped_up_count: u64,
    topped_up_sum_e8s: u64,
    topped_up_min_e8s: Option<u64>,
    topped_up_max_e8s: Option<u64>,
    failed_topups: u64,
    ambiguous_topups: u64,
    remainder_to_self_e8s: u64,
    pending_transfer: Option<PreRelayPendingTransfer>,
    skip_candidate_start_tx_id: Option<u64>,
    skip_candidate_end_tx_id: Option<u64>,
    skip_candidate_tx_count: u64,
    next_created_at_time_nanos: u64,
    observed_oldest_tx_id: Option<u64>,
    observed_latest_tx_id: Option<u64>,
    cmc_attempt_count: Option<u64>,
    cmc_success_count: Option<u64>,
    cmc_attempted_beneficiaries: Option<Vec<Principal>>,
    round_start_time_nanos: Option<u64>,
    round_start_staking_balance_e8s: Option<u64>,
    round_start_latest_tx_id: Option<u64>,
    round_end_time_nanos: Option<u64>,
    round_end_latest_tx_id: Option<u64>,
    effective_denom_staking_balance_e8s: Option<u64>,
    effective_denom_scan_complete: Option<bool>,
    round_end_staking_balance_e8s: Option<u64>,
    funding_tx_id: Option<u64>,
    funding_tx_timestamp_nanos: Option<u64>,
    funding_amount_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize)]
struct PreRelayState {
    config: Config,
    last_summary: Option<PreRelaySummary>,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    blackhole_armed_since_ts: Option<u64>,
    forced_rescue_reason: Option<ForcedRescueReason>,
    skip_range_invariant_fault: Option<bool>,
    consecutive_index_anchor_failures: Option<u8>,
    consecutive_index_latest_invariant_failures: Option<u8>,
    consecutive_index_latest_unreadable_failures: Option<u8>,
    consecutive_cmc_zero_success_runs: Option<u8>,
    last_observed_staking_balance_e8s: Option<u64>,
    last_observed_latest_tx_id: Option<u64>,
    main_lock_state_ts: Option<u64>,
    payout_nonce: u64,
    active_payout_job: Option<PreRelayActivePayoutJob>,
    last_main_run_ts: u64,
    current_round_start_time_nanos: Option<u64>,
    current_round_start_staking_balance_e8s: Option<u64>,
    current_round_start_latest_tx_id: Option<u64>,
    last_processed_funding_tx_id: Option<u64>,
    active_funding_scan: Option<FundingScanState>,
}

pub(crate) const UPGRADE_QUIESCENCE_ERROR: &str =
    "faucet upgrade requires no active payout job; allow the payout to finish before upgrading";

impl TryFrom<PreRelayState> for State {
    type Error = String;

    fn try_from(value: PreRelayState) -> Result<Self, Self::Error> {
        if value.active_payout_job.is_some() {
            return Err(UPGRADE_QUIESCENCE_ERROR.to_string());
        }
        Ok(Self {
            config: value.config,
            last_summary: value.last_summary.map(Into::into),
            last_successful_transfer_ts: value.last_successful_transfer_ts,
            last_rescue_check_ts: value.last_rescue_check_ts,
            rescue_triggered: value.rescue_triggered,
            blackhole_armed_since_ts: value.blackhole_armed_since_ts,
            forced_rescue_reason: value.forced_rescue_reason,
            skip_range_invariant_fault: value.skip_range_invariant_fault,
            consecutive_index_anchor_failures: value.consecutive_index_anchor_failures,
            consecutive_index_latest_invariant_failures: value
                .consecutive_index_latest_invariant_failures,
            consecutive_index_latest_unreadable_failures: value
                .consecutive_index_latest_unreadable_failures,
            consecutive_cmc_zero_success_runs: value.consecutive_cmc_zero_success_runs,
            last_observed_staking_balance_e8s: value.last_observed_staking_balance_e8s,
            last_observed_latest_tx_id: value.last_observed_latest_tx_id,
            main_lock_state_ts: value.main_lock_state_ts,
            payout_nonce: value.payout_nonce,
            active_payout_job: None,
            last_main_run_ts: value.last_main_run_ts,
            current_round_start_time_nanos: value.current_round_start_time_nanos,
            current_round_start_staking_balance_e8s: value.current_round_start_staking_balance_e8s,
            current_round_start_latest_tx_id: value.current_round_start_latest_tx_id,
            last_processed_funding_tx_id: value.last_processed_funding_tx_id,
            active_funding_scan: value.active_funding_scan,
        })
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize)]
enum PreRelayVersionedStableState {
    Uninitialized,
    V1(PreRelayState),
}

// Stable-state enum shape is part of the upgrade contract; boxing V1 would change Candid.
#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum VersionedStableState {
    Uninitialized,
    V1(State),
}

pub(crate) fn decode_versioned_stable_state(bytes: &[u8]) -> Result<VersionedStableState, String> {
    let current_decode = candid::decode_one::<VersionedStableState>(bytes).and_then(|current| {
        let canonical_current = candid::encode_one(&current)?;
        if canonical_current == bytes {
            Ok(current)
        } else {
            // Candid may coerce an incompatible `opt record` payload to `null`. Requiring the
            // stable cell's canonical current encoding makes the current-path decode lossless.
            Err(candid::Error::msg(
                "current faucet stable state did not decode losslessly",
            ))
        }
    });
    match current_decode {
        Ok(current) => Ok(current),
        Err(current_error) => match candid::decode_one::<PreRelayVersionedStableState>(bytes) {
            Ok(PreRelayVersionedStableState::Uninitialized) => {
                Ok(VersionedStableState::Uninitialized)
            }
            Ok(PreRelayVersionedStableState::V1(state)) => {
                State::try_from(state).map(VersionedStableState::V1)
            }
            Err(pre_relay_error) => Err(format!(
                "failed to decode current or pre-Relay faucet stable state: current={current_error}; pre_relay={pre_relay_error}"
            )),
        },
    }
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

pub(crate) fn validate_skip_range_insertion(
    existing: &[SkipRange],
    range: &SkipRange,
) -> Result<(), SkipRangeInsertError> {
    if range.start_tx_id > range.end_tx_id {
        return Err(SkipRangeInsertError::InvalidRange);
    }
    if existing
        .iter()
        .any(|candidate| candidate.start_tx_id == range.start_tx_id)
    {
        return Err(SkipRangeInsertError::DuplicateStart);
    }
    if let Some(previous) = existing
        .iter()
        .rev()
        .find(|candidate| candidate.start_tx_id < range.start_tx_id)
    {
        if previous.end_tx_id.saturating_add(1) >= range.start_tx_id {
            return Err(SkipRangeInsertError::OverlapsOrAbutsPredecessor);
        }
    }
    if let Some(next) = existing
        .iter()
        .find(|candidate| candidate.start_tx_id > range.start_tx_id)
    {
        if range.end_tx_id.saturating_add(1) >= next.start_tx_id {
            return Err(SkipRangeInsertError::OverlapsOrAbutsSuccessor);
        }
    }
    Ok(())
}

pub(crate) fn insert_skip_range(range: SkipRange) -> Result<(), SkipRangeInsertError> {
    // This durable cache assumes the commitment-validity rules are unchanged since the
    // range was learned. Rescue upgrades clear the whole cache before resuming, and any
    // future maintenance path that changes commitment-validity rules must do the same
    // before relying on persisted skip ranges again.
    let existing = list_skip_ranges();
    validate_skip_range_insertion(&existing, &range)?;
    with_skip_range_map(|map| {
        map.insert(
            U64Key::from(range.start_tx_id),
            U64Value::from(range.end_tx_id),
        );
    });
    Ok(())
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
    with_skip_range_map(|map| {
        let keys: Vec<_> = map.iter().map(|entry| entry.key().clone()).collect();
        for key in keys {
            map.remove(&key);
        }
    });
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
            cmc_canister_id: principal(&[4]),
            governance_canister_id: Some(principal(&[9])),
            funding_source_account: Account {
                owner: principal(&[8]),
                subaccount: None,
            },
            rescue_controller: principal(&[5]),
            blackhole_controller: Some(principal(&[6])),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: Some(11),
            main_interval_seconds: 60,
            rescue_interval_seconds: 120,
            min_tx_e8s: 100_000_000,
            stake_recognition_delay_seconds: Some(24 * 60 * 60),
        }
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
        assert!(line.contains("blackhole_controller="));
        assert!(line.contains("blackhole_armed=false"));
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
    fn set_state_round_trips_through_stable_storage() {
        reset_test_storage();
        let mut st = State::new(sample_config(), 1_000);
        st.last_successful_transfer_ts = Some(77);
        st.main_lock_state_ts = Some(33);
        set_state(st.clone());

        let restored = restore_state_from_stable().expect("expected persisted faucet state");
        assert_eq!(restored.last_successful_transfer_ts, Some(77));
        assert_eq!(restored.main_lock_state_ts, Some(33));
        assert_eq!(restored.payout_nonce, st.payout_nonce);
        assert_eq!(restored.config.min_tx_e8s, st.config.min_tx_e8s);
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
    fn pending_notification_decodes_legacy_state_without_transfer_memo() {
        #[derive(CandidType, Deserialize)]
        enum LegacyTransferKind {
            Beneficiary,
        }

        #[derive(CandidType, Deserialize)]
        struct LegacyPendingNotification {
            kind: LegacyTransferKind,
            beneficiary: Principal,
            gross_share_e8s: u64,
            amount_e8s: u64,
            block_index: u64,
            next_start: Option<u64>,
        }

        let legacy = LegacyPendingNotification {
            kind: LegacyTransferKind::Beneficiary,
            beneficiary: principal(&[8]),
            gross_share_e8s: 50,
            amount_e8s: 40,
            block_index: 9,
            next_start: Some(10),
        };
        let bytes = candid::encode_one(legacy).expect("encode legacy pending notification");
        let decoded: PendingNotification =
            candid::decode_one(&bytes).expect("decode legacy pending notification");
        assert_eq!(decoded.kind, TransferKind::Beneficiary);
        assert_eq!(decoded.beneficiary, principal(&[8]));
        assert_eq!(decoded.transfer_memo, None);
    }

    #[allow(dead_code)]
    #[derive(CandidType, Deserialize)]
    enum FrozenPreRelayTransferKind {
        Beneficiary,
        RawIcp,
        NeuronStake,
        RemainderToSelf,
    }

    #[allow(dead_code)]
    #[derive(CandidType, Deserialize)]
    struct FrozenPreRelayPendingNotification {
        kind: FrozenPreRelayTransferKind,
        beneficiary: Principal,
        gross_share_e8s: u64,
        amount_e8s: u64,
        block_index: u64,
        next_start: Option<u64>,
        transfer_memo: Option<Vec<u8>>,
        destination_subaccount: Option<[u8; 32]>,
        neuron_id: Option<u64>,
    }

    #[allow(dead_code)]
    #[derive(CandidType, Deserialize)]
    enum FrozenPreRelayPendingTransferPhase {
        AwaitingTransfer,
        TransferAccepted,
    }

    #[allow(dead_code)]
    #[derive(CandidType, Deserialize)]
    struct FrozenPreRelayPendingTransfer {
        notification: FrozenPreRelayPendingNotification,
        created_at_time_nanos: u64,
        phase: FrozenPreRelayPendingTransferPhase,
    }

    #[derive(CandidType, Deserialize)]
    struct FrozenPreRelaySummary {
        pot_start_e8s: u64,
        pot_remaining_e8s: u64,
        denom_staking_balance_e8s: u64,
        effective_denom_staking_balance_e8s: Option<u64>,
        funding_tx_id: Option<u64>,
        funding_amount_e8s: Option<u64>,
        round_end_latest_tx_id: Option<u64>,
        round_end_time_nanos: Option<u64>,
        last_processed_funding_tx_id: Option<u64>,
        topped_up_count: u64,
        topped_up_sum_e8s: u64,
        topped_up_min_e8s: Option<u64>,
        topped_up_max_e8s: Option<u64>,
        failed_topups: u64,
        ambiguous_topups: u64,
        ignored_under_threshold: u64,
        ignored_bad_memo: u64,
        remainder_to_self_e8s: u64,
    }

    #[allow(dead_code)]
    #[derive(CandidType, Deserialize)]
    struct FrozenPreRelayActivePayoutJob {
        id: u64,
        fee_e8s: u64,
        pot_start_e8s: u64,
        denom_staking_balance_e8s: u64,
        next_start: Option<u64>,
        scan_complete: bool,
        ignored_under_threshold: u64,
        ignored_bad_memo: u64,
        gross_outflow_e8s: u64,
        topped_up_count: u64,
        topped_up_sum_e8s: u64,
        topped_up_min_e8s: Option<u64>,
        topped_up_max_e8s: Option<u64>,
        failed_topups: u64,
        ambiguous_topups: u64,
        remainder_to_self_e8s: u64,
        pending_transfer: Option<FrozenPreRelayPendingTransfer>,
        skip_candidate_start_tx_id: Option<u64>,
        skip_candidate_end_tx_id: Option<u64>,
        skip_candidate_tx_count: u64,
        next_created_at_time_nanos: u64,
        observed_oldest_tx_id: Option<u64>,
        observed_latest_tx_id: Option<u64>,
        cmc_attempt_count: Option<u64>,
        cmc_success_count: Option<u64>,
        cmc_attempted_beneficiaries: Option<Vec<Principal>>,
        round_start_time_nanos: Option<u64>,
        round_start_staking_balance_e8s: Option<u64>,
        round_start_latest_tx_id: Option<u64>,
        round_end_time_nanos: Option<u64>,
        round_end_latest_tx_id: Option<u64>,
        effective_denom_staking_balance_e8s: Option<u64>,
        effective_denom_scan_complete: Option<bool>,
        round_end_staking_balance_e8s: Option<u64>,
        funding_tx_id: Option<u64>,
        funding_tx_timestamp_nanos: Option<u64>,
        funding_amount_e8s: Option<u64>,
    }

    #[derive(CandidType, Deserialize)]
    struct FrozenPreRelayState {
        config: Config,
        last_summary: Option<FrozenPreRelaySummary>,
        last_successful_transfer_ts: Option<u64>,
        last_rescue_check_ts: u64,
        rescue_triggered: bool,
        blackhole_armed_since_ts: Option<u64>,
        forced_rescue_reason: Option<ForcedRescueReason>,
        skip_range_invariant_fault: Option<bool>,
        consecutive_index_anchor_failures: Option<u8>,
        consecutive_index_latest_invariant_failures: Option<u8>,
        consecutive_index_latest_unreadable_failures: Option<u8>,
        consecutive_cmc_zero_success_runs: Option<u8>,
        last_observed_staking_balance_e8s: Option<u64>,
        last_observed_latest_tx_id: Option<u64>,
        main_lock_state_ts: Option<u64>,
        payout_nonce: u64,
        active_payout_job: Option<FrozenPreRelayActivePayoutJob>,
        last_main_run_ts: u64,
        current_round_start_time_nanos: Option<u64>,
        current_round_start_staking_balance_e8s: Option<u64>,
        current_round_start_latest_tx_id: Option<u64>,
        last_processed_funding_tx_id: Option<u64>,
        active_funding_scan: Option<FundingScanState>,
    }

    #[allow(clippy::large_enum_variant)]
    #[derive(CandidType, Deserialize)]
    enum FrozenPreRelayVersionedStableState {
        Uninitialized,
        V1(FrozenPreRelayState),
    }

    fn frozen_pre_relay_summary() -> FrozenPreRelaySummary {
        FrozenPreRelaySummary {
            pot_start_e8s: 1_000,
            pot_remaining_e8s: 100,
            denom_staking_balance_e8s: 2_000,
            effective_denom_staking_balance_e8s: Some(2_000),
            funding_tx_id: Some(7),
            funding_amount_e8s: Some(1_000),
            round_end_latest_tx_id: Some(6),
            round_end_time_nanos: Some(70),
            last_processed_funding_tx_id: Some(7),
            topped_up_count: 1,
            topped_up_sum_e8s: 890,
            topped_up_min_e8s: Some(890),
            topped_up_max_e8s: Some(890),
            failed_topups: 0,
            ambiguous_topups: 0,
            ignored_under_threshold: 0,
            ignored_bad_memo: 0,
            remainder_to_self_e8s: 90,
        }
    }

    fn frozen_pre_relay_active_job() -> FrozenPreRelayActivePayoutJob {
        FrozenPreRelayActivePayoutJob {
            id: 9,
            fee_e8s: 10,
            pot_start_e8s: 1_000,
            denom_staking_balance_e8s: 2_000,
            next_start: Some(6),
            scan_complete: true,
            ignored_under_threshold: 0,
            ignored_bad_memo: 0,
            gross_outflow_e8s: 900,
            topped_up_count: 1,
            topped_up_sum_e8s: 890,
            topped_up_min_e8s: Some(890),
            topped_up_max_e8s: Some(890),
            failed_topups: 0,
            ambiguous_topups: 0,
            remainder_to_self_e8s: 0,
            pending_transfer: Some(FrozenPreRelayPendingTransfer {
                notification: FrozenPreRelayPendingNotification {
                    kind: FrozenPreRelayTransferKind::RemainderToSelf,
                    beneficiary: principal(&[8]),
                    gross_share_e8s: 100,
                    amount_e8s: 90,
                    block_index: 10,
                    next_start: None,
                    transfer_memo: None,
                    destination_subaccount: None,
                    neuron_id: None,
                },
                created_at_time_nanos: 99,
                phase: FrozenPreRelayPendingTransferPhase::TransferAccepted,
            }),
            skip_candidate_start_tx_id: None,
            skip_candidate_end_tx_id: None,
            skip_candidate_tx_count: 0,
            next_created_at_time_nanos: 100,
            observed_oldest_tx_id: Some(1),
            observed_latest_tx_id: Some(6),
            cmc_attempt_count: Some(1),
            cmc_success_count: Some(1),
            cmc_attempted_beneficiaries: Some(vec![principal(&[8])]),
            round_start_time_nanos: Some(1),
            round_start_staking_balance_e8s: Some(2_000),
            round_start_latest_tx_id: Some(1),
            round_end_time_nanos: Some(70),
            round_end_latest_tx_id: Some(6),
            effective_denom_staking_balance_e8s: Some(2_000),
            effective_denom_scan_complete: Some(true),
            round_end_staking_balance_e8s: Some(2_000),
            funding_tx_id: Some(7),
            funding_tx_timestamp_nanos: Some(70),
            funding_amount_e8s: Some(1_000),
        }
    }

    fn frozen_pre_relay_state(
        active_payout_job: Option<FrozenPreRelayActivePayoutJob>,
    ) -> FrozenPreRelayVersionedStableState {
        FrozenPreRelayVersionedStableState::V1(FrozenPreRelayState {
            config: sample_config(),
            last_summary: Some(frozen_pre_relay_summary()),
            last_successful_transfer_ts: Some(7),
            last_rescue_check_ts: 8,
            rescue_triggered: false,
            blackhole_armed_since_ts: None,
            forced_rescue_reason: None,
            skip_range_invariant_fault: Some(false),
            consecutive_index_anchor_failures: Some(0),
            consecutive_index_latest_invariant_failures: Some(0),
            consecutive_index_latest_unreadable_failures: Some(0),
            consecutive_cmc_zero_success_runs: Some(0),
            last_observed_staking_balance_e8s: Some(2_000),
            last_observed_latest_tx_id: Some(6),
            main_lock_state_ts: Some(0),
            payout_nonce: 9,
            active_payout_job,
            last_main_run_ts: 7,
            current_round_start_time_nanos: Some(1),
            current_round_start_staking_balance_e8s: Some(2_000),
            current_round_start_latest_tx_id: Some(1),
            last_processed_funding_tx_id: Some(6),
            active_funding_scan: Some(FundingScanState {
                anchor_last_processed_funding_tx_id: Some(5),
                cursor: Some(4),
                candidate: Some(FundingTrancheState {
                    tx_id: 6,
                    timestamp_nanos: 70,
                    amount_e8s: 1_000,
                }),
            }),
        })
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
    fn quiescent_pre_relay_state_decodes_and_discards_old_self_remainder() {
        let bytes = candid::encode_one(frozen_pre_relay_state(None))
            .expect("encode frozen quiescent pre-Relay state");

        let VersionedStableState::V1(restored) =
            decode_versioned_stable_state(&bytes).expect("decode quiescent pre-Relay state")
        else {
            panic!("expected restored V1 state");
        };

        assert!(restored.active_payout_job.is_none());
        assert_eq!(restored.last_successful_transfer_ts, Some(7));
        assert_eq!(restored.last_rescue_check_ts, 8);
        assert_eq!(restored.payout_nonce, 9);
        assert_eq!(restored.current_round_start_time_nanos, Some(1));
        assert_eq!(restored.last_processed_funding_tx_id, Some(6));
        assert_eq!(
            restored
                .active_funding_scan
                .expect("funding scan should survive")
                .cursor,
            Some(4)
        );
        assert_eq!(
            restored
                .last_summary
                .expect("summary should survive")
                .remainder_to_relay_e8s,
            0
        );
    }

    #[test]
    fn pre_relay_state_with_active_payout_job_is_rejected() {
        let bytes = candid::encode_one(frozen_pre_relay_state(Some(frozen_pre_relay_active_job())))
            .expect("encode frozen active pre-Relay state");

        let err = decode_versioned_stable_state(&bytes)
            .err()
            .expect("active pre-Relay state must be rejected");

        assert_eq!(err, UPGRADE_QUIESCENCE_ERROR);
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
    fn skip_range_insertion_rejects_adjacent_ranges() {
        reset_test_storage();
        insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 20,
        })
        .expect("baseline range should persist");

        let err = insert_skip_range(SkipRange {
            start_tx_id: 21,
            end_tx_id: 30,
        })
        .expect_err("adjacent skip range should be rejected");
        assert_eq!(err, SkipRangeInsertError::OverlapsOrAbutsPredecessor);
    }

    #[test]
    fn skip_range_insertion_rejects_same_start_as_existing_range() {
        reset_test_storage();
        insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 20,
        })
        .expect("baseline range should persist");

        let err = insert_skip_range(SkipRange {
            start_tx_id: 10,
            end_tx_id: 30,
        })
        .expect_err("duplicate-start skip range should be rejected");
        assert_eq!(err, SkipRangeInsertError::DuplicateStart);
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
