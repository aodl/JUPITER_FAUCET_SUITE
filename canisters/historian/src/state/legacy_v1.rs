#![allow(dead_code)]

use super::*;

pub(crate) const LEGACY_HISTORIAN_V1_REVISION: &str = "98c871a85af91320a5dfc59b5b040727e21aa094";

// Frozen stable-schema subset copied from the Historian at
// LEGACY_HISTORIAN_V1_REVISION. These types are private compatibility decoders
// for stable memory and tests; they are not public API or runtime source of truth.

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LegacyCanisterSourceV1 {
    MemoCommitment,
    SnsDiscovery,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyCyclesSampleSourceV1 {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyCyclesProbeResultV1 {
    Ok(LegacyCyclesSampleSourceV1),
    NotAvailable,
    Error(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyCyclesSampleV1 {
    pub timestamp_nanos: u64,
    pub cycles: u128,
    pub source: LegacyCyclesSampleSourceV1,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct LegacyStableSourceSetV1(pub BTreeSet<LegacyCanisterSourceV1>);

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LegacyStableCanisterMetaV1 {
    pub first_seen_ts: Option<u64>,
    pub last_commitment_ts: Option<u64>,
    pub last_cycles_probe_ts: Option<u64>,
    pub last_cycles_probe_result: Option<LegacyCyclesProbeResultV1>,
    #[serde(default)]
    pub last_burn_tx_id: Option<u64>,
    #[serde(default)]
    pub last_burn_scan_tx_id: Option<u64>,
    #[serde(default)]
    pub burned_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRelayRegistryKindV1 {
    Canonical,
    SelfService,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRelayRegistryStatusV1 {
    Pending,
    Active,
    Failed,
    Superseded,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRelayRegistryEntryV1 {
    pub relay_canister_id: Principal,
    pub target_canister_id: Principal,
    pub kind: LegacyRelayRegistryKindV1,
    pub status: LegacyRelayRegistryStatusV1,
    pub setup_account: Option<Account>,
    pub setup_account_identifier: Option<String>,
    pub setup_amount_e8s: Option<u64>,
    pub setup_tx_ids: Vec<u64>,
    pub relay_wasm_hash_hex: Option<String>,
    pub final_controllers: Option<Vec<Principal>>,
    pub log_visibility_public: Option<bool>,
    pub created_at_ts: Option<u64>,
    pub activated_at_ts: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRelaySetupStatusV1 {
    NotFunded,
    BelowMinimum,
    InsufficientForCurrentRate,
    TargetNotObservable,
    Pending,
    ConvertingCycles,
    CycleTransferAccepted,
    CycleNotifySucceeded,
    CreatingCanister,
    CanisterCreated,
    InstallingCode,
    CodeInstalled,
    SettingPublicLogs,
    FundingRelaySubaccountOne,
    Blackholing,
    Active,
    SweepingToExistingRelay,
    SweptToExistingRelay,
    SweepBelowDust,
    RefundAvailable,
    Refunding,
    Refunded,
    IndexNotReady,
    FailedRetryable,
    FailedTerminal,
    Ambiguous,
    ManualRecoveryRequired,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRelaySetupPaymentV1 {
    pub target_canister_id: Principal,
    pub tx_id: u64,
    pub from_account_identifier: String,
    pub amount_e8s: u64,
    pub timestamp_nanos: Option<u64>,
    pub processed: bool,
    pub refunded: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRelaySetupTransferKindV1 {
    CmcConversion,
    RelayFunding,
    ExistingRelaySweep,
    Refund,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRelaySetupPhaseV1 {
    PreSpend,
    CycleTransferAccepted,
    CycleNotifySucceeded,
    RelayCanisterCreated,
    RelayCodeInstalled,
    RelayFundingAccepted,
    BlackholeUpdateAttempted,
    Active,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRelaySetupTransferRecordV1 {
    pub kind: LegacyRelaySetupTransferKindV1,
    pub from_subaccount: Option<[u8; 32]>,
    pub from_account_identifier: String,
    pub to: Account,
    pub to_account_identifier: String,
    pub amount_e8s: u64,
    pub fee_e8s: u64,
    pub memo: Option<Vec<u8>>,
    pub created_at_time_nanos: u64,
    pub block_index: Option<u64>,
    pub completed: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRelayCreateAttemptV1 {
    pub target_canister_id: Principal,
    pub created_at_ts: u64,
    pub initial_cycles: u128,
    #[serde(default)]
    pub raw_relay_wasm_hash_hex: Option<String>,
    #[serde(default)]
    pub install_payload_hash_hex: Option<String>,
    #[serde(default)]
    pub relay_wasm_hash_hex: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRelaySetupJobV1 {
    pub target_canister_id: Principal,
    pub setup_account: Account,
    pub setup_account_identifier: String,
    pub status: LegacyRelaySetupStatusV1,
    pub relay_canister_id: Option<Principal>,
    pub last_indexed_setup_tx_id: Option<u64>,
    pub setup_tx_ids: Vec<u64>,
    pub setup_amount_seen_e8s: u64,
    pub setup_amount_processed_e8s: u64,
    pub payments: Vec<LegacyRelaySetupPaymentV1>,
    pub cycle_conversion_e8s: Option<u64>,
    pub cycle_transfer_block_index: Option<u64>,
    pub cycles_minted: Option<u128>,
    pub relay_initial_cycles: Option<u128>,
    pub relay_funding_e8s: Option<u64>,
    pub relay_funding_block_index: Option<u64>,
    #[serde(default)]
    pub phase: Option<LegacyRelaySetupPhaseV1>,
    #[serde(default)]
    pub cycle_transfer: Option<LegacyRelaySetupTransferRecordV1>,
    #[serde(default)]
    pub relay_funding_transfer: Option<LegacyRelaySetupTransferRecordV1>,
    #[serde(default)]
    pub existing_relay_sweep_transfer: Option<LegacyRelaySetupTransferRecordV1>,
    #[serde(default)]
    pub refund_transfers: Vec<LegacyRelaySetupTransferRecordV1>,
    #[serde(default)]
    pub relay_create_attempt: Option<LegacyRelayCreateAttemptV1>,
    #[serde(default)]
    pub code_installed: bool,
    #[serde(default)]
    pub relay_funding_accepted: bool,
    #[serde(default)]
    pub blackhole_update_attempted: bool,
    #[serde(default)]
    pub blackhole_confirmed: bool,
    pub refund_attempt_count: u32,
    pub last_refund_attempt_ts: Option<u64>,
    pub refund_blocks: Vec<u64>,
    pub created_at_ts: u64,
    pub updated_at_ts: u64,
    pub last_error: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct LegacyStableConfigV1 {
    pub staking_account: Account,
    #[serde(default)]
    pub output_source_account: Option<Account>,
    #[serde(default)]
    pub output_account: Option<Account>,
    #[serde(default)]
    pub rewards_account: Option<Account>,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    #[serde(default)]
    pub cmc_canister_id: Option<Principal>,
    #[serde(default)]
    pub faucet_canister_id: Option<Principal>,
    pub blackhole_canister_id: Principal,
    pub sns_wasm_canister_id: Principal,
    #[serde(default)]
    pub xrc_canister_id: Option<Principal>,
    pub enable_sns_tracking: bool,
    pub scan_interval_seconds: u64,
    pub cycles_interval_seconds: u64,
    pub min_tx_e8s: u64,
    pub max_cycles_entries_per_canister: u32,
    pub max_commitment_entries_per_canister: u32,
    pub max_index_pages_per_tick: u32,
    pub max_canisters_per_cycles_tick: u32,
    #[serde(default)]
    pub relay_factory_enabled: Option<bool>,
    #[serde(default)]
    pub relay_setup_min_e8s: Option<u64>,
    #[serde(default)]
    pub relay_setup_dust_e8s: Option<u64>,
    #[serde(default)]
    pub relay_setup_refund_cooldown_seconds: Option<u64>,
    #[serde(default)]
    pub relay_initial_cycles: Option<u128>,
    #[serde(default)]
    pub relay_cycle_safety_margin_e8s: Option<u64>,
    #[serde(default)]
    pub relay_min_subaccount_one_seed_e8s: Option<u64>,
    #[serde(default)]
    pub self_service_relay_interval_seconds: Option<u64>,
    #[serde(default)]
    pub self_service_relay_max_transfers_per_tick: Option<Option<u32>>,
    #[serde(default)]
    pub io_surplus_neuron_id: Option<u64>,
    #[serde(default)]
    pub canonical_relay_canister_id: Option<Option<Principal>>,
    #[serde(default)]
    pub canonical_relay_targets: Option<Vec<Principal>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct LegacyStableRootStateV1 {
    pub config: LegacyStableConfigV1,
    pub last_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_staking_tx_id: Option<u64>,
    #[serde(default)]
    pub staking_index_descending: Option<bool>,
    #[serde(default)]
    pub staking_backfill_complete: Option<bool>,
    #[serde(default)]
    pub last_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_output_tx_id: Option<u64>,
    #[serde(default)]
    pub output_route_index_descending: Option<bool>,
    #[serde(default)]
    pub output_route_backfill_complete: Option<bool>,
    #[serde(default)]
    pub last_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    pub oldest_indexed_rewards_tx_id: Option<u64>,
    #[serde(default)]
    pub rewards_route_index_descending: Option<bool>,
    #[serde(default)]
    pub rewards_route_backfill_complete: Option<bool>,
    pub last_sns_discovery_ts: u64,
    pub last_completed_cycles_sweep_ts: u64,
    #[serde(default)]
    pub last_completed_route_sweep_ts: Option<u64>,
    pub active_cycles_sweep: Option<ActiveCyclesSweep>,
    #[serde(default)]
    pub initial_cycles_probe_queue: Vec<Principal>,
    #[serde(default)]
    pub active_route_sweep: Option<ActiveRouteSweep>,
    #[serde(default)]
    pub active_sns_discovery: Option<ActiveSnsDiscovery>,
    pub main_lock_state_ts: Option<u64>,
    pub last_main_run_ts: u64,
    #[serde(default)]
    pub qualifying_commitment_count: Option<u64>,
    #[serde(default)]
    pub total_output_e8s: Option<u64>,
    #[serde(default)]
    pub total_rewards_e8s: Option<u64>,
    #[serde(default)]
    pub icp_burned_e8s: Option<u64>,
    #[serde(default)]
    pub recent_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    pub recent_under_threshold_commitments: Option<Vec<RecentCommitment>>,
    #[serde(default)]
    pub recent_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    pub recent_under_threshold_neuron_commitments: Option<Vec<RecentNeuronCommitment>>,
    #[serde(default)]
    pub recent_invalid_commitments: Option<Vec<InvalidCommitment>>,
    #[serde(default)]
    pub recent_burns: Option<Vec<RecentBurn>>,
    #[serde(default)]
    pub last_index_run_ts: Option<u64>,
    #[serde(default)]
    pub commitment_index_fault: Option<CommitmentIndexFault>,
    #[serde(default)]
    pub icp_xdr_rate: Option<IcpXdrRateSnapshot>,
    #[serde(default)]
    pub last_icp_xdr_rate_attempt_ts: Option<u64>,
    #[serde(default)]
    pub last_icp_xdr_rate_error: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum LegacyVersionedStableStateV1 {
    Uninitialized,
    Current(LegacyStableRootStateV1),
}

impl From<LegacyCanisterSourceV1> for CanisterTrackingReason {
    fn from(value: LegacyCanisterSourceV1) -> Self {
        match value {
            LegacyCanisterSourceV1::MemoCommitment => Self::MemoCommitment,
            LegacyCanisterSourceV1::SnsDiscovery => Self::SnsDiscovery,
        }
    }
}

impl From<LegacyCyclesSampleSourceV1> for CyclesSampleSource {
    fn from(value: LegacyCyclesSampleSourceV1) -> Self {
        match value {
            LegacyCyclesSampleSourceV1::BlackholeStatus => Self::BlackholeStatus,
            LegacyCyclesSampleSourceV1::SelfCanister => Self::SelfCanister,
            LegacyCyclesSampleSourceV1::SnsRootSummary => Self::SnsRootSummary,
        }
    }
}

impl From<LegacyCyclesProbeResultV1> for CyclesProbeResult {
    fn from(value: LegacyCyclesProbeResultV1) -> Self {
        match value {
            LegacyCyclesProbeResultV1::Ok(source) => Self::Ok(source.into()),
            LegacyCyclesProbeResultV1::NotAvailable => Self::NotAvailable,
            LegacyCyclesProbeResultV1::Error(message) => Self::Error(message),
        }
    }
}

impl From<LegacyCyclesSampleV1> for CyclesSample {
    fn from(value: LegacyCyclesSampleV1) -> Self {
        Self {
            timestamp_nanos: value.timestamp_nanos,
            cycles: value.cycles,
            source: value.source.into(),
        }
    }
}

impl From<LegacyStableCanisterMetaV1> for StableCanisterMeta {
    fn from(value: LegacyStableCanisterMetaV1) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_commitment_ts: value.last_commitment_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result.map(Into::into),
            last_burn_tx_id: value.last_burn_tx_id,
            last_burn_scan_tx_id: value.last_burn_scan_tx_id,
            burned_e8s: value.burned_e8s,
        }
    }
}

impl From<LegacyRelayRegistryEntryV1> for RelayRegistryEntry {
    fn from(value: LegacyRelayRegistryEntryV1) -> Self {
        Self {
            relay_canister_id: value.relay_canister_id,
            target_canister_id: value.target_canister_id,
            kind: match value.kind {
                LegacyRelayRegistryKindV1::Canonical => RelayRegistryKind::Canonical,
                LegacyRelayRegistryKindV1::SelfService => RelayRegistryKind::SelfService,
            },
            status: match value.status {
                LegacyRelayRegistryStatusV1::Pending => RelayRegistryStatus::Pending,
                LegacyRelayRegistryStatusV1::Active => RelayRegistryStatus::Active,
                LegacyRelayRegistryStatusV1::Failed => RelayRegistryStatus::Failed,
                LegacyRelayRegistryStatusV1::Superseded => RelayRegistryStatus::Superseded,
            },
            setup_account: value.setup_account,
            setup_account_identifier: value.setup_account_identifier,
            setup_amount_e8s: value.setup_amount_e8s,
            setup_tx_ids: value.setup_tx_ids,
            final_controllers: value.final_controllers,
            log_visibility_public: value.log_visibility_public,
            created_at_ts: value.created_at_ts,
            activated_at_ts: value.activated_at_ts,
        }
    }
}

fn convert_status(value: LegacyRelaySetupStatusV1) -> (RelaySetupStatus, bool) {
    match value {
        LegacyRelaySetupStatusV1::NotFunded => (RelaySetupStatus::NotFunded, false),
        LegacyRelaySetupStatusV1::BelowMinimum => (RelaySetupStatus::BelowMinimum, false),
        LegacyRelaySetupStatusV1::InsufficientForCurrentRate => {
            (RelaySetupStatus::InsufficientForCurrentRate, false)
        }
        LegacyRelaySetupStatusV1::TargetNotObservable => (RelaySetupStatus::RefundAvailable, true),
        LegacyRelaySetupStatusV1::Pending => (RelaySetupStatus::Pending, false),
        LegacyRelaySetupStatusV1::ConvertingCycles => (RelaySetupStatus::ConvertingCycles, false),
        LegacyRelaySetupStatusV1::CycleTransferAccepted => {
            (RelaySetupStatus::CycleTransferAccepted, false)
        }
        LegacyRelaySetupStatusV1::CycleNotifySucceeded => {
            (RelaySetupStatus::CycleNotifySucceeded, false)
        }
        LegacyRelaySetupStatusV1::CreatingCanister => (RelaySetupStatus::CreatingCanister, false),
        LegacyRelaySetupStatusV1::CanisterCreated => (RelaySetupStatus::CanisterCreated, false),
        LegacyRelaySetupStatusV1::InstallingCode => (RelaySetupStatus::InstallingCode, false),
        LegacyRelaySetupStatusV1::CodeInstalled => (RelaySetupStatus::CodeInstalled, false),
        LegacyRelaySetupStatusV1::SettingPublicLogs => (RelaySetupStatus::SettingPublicLogs, false),
        LegacyRelaySetupStatusV1::FundingRelaySubaccountOne => {
            (RelaySetupStatus::FundingRelaySubaccountOne, false)
        }
        LegacyRelaySetupStatusV1::Blackholing => (RelaySetupStatus::Blackholing, false),
        LegacyRelaySetupStatusV1::Active => (RelaySetupStatus::Active, false),
        LegacyRelaySetupStatusV1::SweepingToExistingRelay => {
            (RelaySetupStatus::SweepingToExistingRelay, false)
        }
        LegacyRelaySetupStatusV1::SweptToExistingRelay => {
            (RelaySetupStatus::SweptToExistingRelay, false)
        }
        LegacyRelaySetupStatusV1::SweepBelowDust => (RelaySetupStatus::SweepBelowDust, false),
        LegacyRelaySetupStatusV1::RefundAvailable => (RelaySetupStatus::RefundAvailable, false),
        LegacyRelaySetupStatusV1::Refunding => (RelaySetupStatus::Refunding, false),
        LegacyRelaySetupStatusV1::Refunded => (RelaySetupStatus::Refunded, false),
        LegacyRelaySetupStatusV1::IndexNotReady => (RelaySetupStatus::IndexNotReady, false),
        LegacyRelaySetupStatusV1::FailedRetryable => (RelaySetupStatus::FailedRetryable, false),
        LegacyRelaySetupStatusV1::FailedTerminal => (RelaySetupStatus::FailedTerminal, false),
        LegacyRelaySetupStatusV1::Ambiguous => (RelaySetupStatus::Ambiguous, false),
        LegacyRelaySetupStatusV1::ManualRecoveryRequired => {
            (RelaySetupStatus::ManualRecoveryRequired, false)
        }
    }
}

impl From<LegacyRelaySetupPaymentV1> for RelaySetupPayment {
    fn from(value: LegacyRelaySetupPaymentV1) -> Self {
        Self {
            target_canister_id: value.target_canister_id,
            tx_id: value.tx_id,
            from_account_identifier: value.from_account_identifier,
            amount_e8s: value.amount_e8s,
            timestamp_nanos: value.timestamp_nanos,
            processed: value.processed,
            refunded: value.refunded,
        }
    }
}

impl From<LegacyRelaySetupTransferKindV1> for RelaySetupTransferKind {
    fn from(value: LegacyRelaySetupTransferKindV1) -> Self {
        match value {
            LegacyRelaySetupTransferKindV1::CmcConversion => Self::CmcConversion,
            LegacyRelaySetupTransferKindV1::RelayFunding => Self::RelayFunding,
            LegacyRelaySetupTransferKindV1::ExistingRelaySweep => Self::ExistingRelaySweep,
            LegacyRelaySetupTransferKindV1::Refund => Self::Refund,
        }
    }
}

impl From<LegacyRelaySetupPhaseV1> for RelaySetupPhase {
    fn from(value: LegacyRelaySetupPhaseV1) -> Self {
        match value {
            LegacyRelaySetupPhaseV1::PreSpend => Self::PreSpend,
            LegacyRelaySetupPhaseV1::CycleTransferAccepted => Self::CycleTransferAccepted,
            LegacyRelaySetupPhaseV1::CycleNotifySucceeded => Self::CycleNotifySucceeded,
            LegacyRelaySetupPhaseV1::RelayCanisterCreated => Self::RelayCanisterCreated,
            LegacyRelaySetupPhaseV1::RelayCodeInstalled => Self::RelayCodeInstalled,
            LegacyRelaySetupPhaseV1::RelayFundingAccepted => Self::RelayFundingAccepted,
            LegacyRelaySetupPhaseV1::BlackholeUpdateAttempted => Self::BlackholeUpdateAttempted,
            LegacyRelaySetupPhaseV1::Active => Self::Active,
        }
    }
}

impl From<LegacyRelaySetupTransferRecordV1> for RelaySetupTransferRecord {
    fn from(value: LegacyRelaySetupTransferRecordV1) -> Self {
        Self {
            kind: value.kind.into(),
            from_subaccount: value.from_subaccount,
            from_account_identifier: value.from_account_identifier,
            to: value.to,
            to_account_identifier: value.to_account_identifier,
            amount_e8s: value.amount_e8s,
            fee_e8s: value.fee_e8s,
            memo: value.memo,
            created_at_time_nanos: value.created_at_time_nanos,
            block_index: value.block_index,
            completed: value.completed,
        }
    }
}

impl From<LegacyRelayCreateAttemptV1> for RelayCreateAttempt {
    fn from(value: LegacyRelayCreateAttemptV1) -> Self {
        Self {
            target_canister_id: value.target_canister_id,
            created_at_ts: value.created_at_ts,
            initial_cycles: value.initial_cycles,
        }
    }
}

impl From<LegacyRelaySetupJobV1> for RelaySetupJob {
    fn from(value: LegacyRelaySetupJobV1) -> Self {
        let (status, target_not_observable) = convert_status(value.status);
        let mut job = Self {
            target_canister_id: value.target_canister_id,
            setup_account: value.setup_account,
            setup_account_identifier: value.setup_account_identifier,
            status,
            relay_canister_id: value.relay_canister_id,
            last_indexed_setup_tx_id: value.last_indexed_setup_tx_id,
            setup_tx_ids: value.setup_tx_ids,
            setup_amount_seen_e8s: value.setup_amount_seen_e8s,
            setup_amount_processed_e8s: value.setup_amount_processed_e8s,
            payments: value.payments.into_iter().map(Into::into).collect(),
            cycle_conversion_e8s: value.cycle_conversion_e8s,
            cycle_transfer_block_index: value.cycle_transfer_block_index,
            cycles_minted: value.cycles_minted,
            relay_initial_cycles: value.relay_initial_cycles,
            relay_funding_e8s: value.relay_funding_e8s,
            relay_funding_block_index: value.relay_funding_block_index,
            phase: value.phase.map(Into::into),
            cycle_transfer: value.cycle_transfer.map(Into::into),
            relay_funding_transfer: value.relay_funding_transfer.map(Into::into),
            existing_relay_sweep_transfer: value.existing_relay_sweep_transfer.map(Into::into),
            refund_transfers: value.refund_transfers.into_iter().map(Into::into).collect(),
            relay_create_attempt: value.relay_create_attempt.map(Into::into),
            code_installed: value.code_installed,
            relay_funding_accepted: value.relay_funding_accepted,
            blackhole_update_attempted: value.blackhole_update_attempted,
            blackhole_confirmed: value.blackhole_confirmed,
            refund_attempt_count: value.refund_attempt_count,
            last_refund_attempt_ts: value.last_refund_attempt_ts,
            refund_blocks: value.refund_blocks,
            created_at_ts: value.created_at_ts,
            updated_at_ts: value.updated_at_ts,
            last_error: value.last_error,
        };
        if target_not_observable && !crate::relay_setup::refund_allowed_before_spend(&job) {
            job.status = RelaySetupStatus::ManualRecoveryRequired;
        }
        job
    }
}

impl From<LegacyStableConfigV1> for StableConfig {
    fn from(value: LegacyStableConfigV1) -> Self {
        Self {
            staking_account: value.staking_account,
            output_source_account: value.output_source_account,
            output_account: value.output_account,
            rewards_account: value.rewards_account,
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: value.cmc_canister_id,
            faucet_canister_id: value.faucet_canister_id,
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            xrc_canister_id: value.xrc_canister_id,
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_commitment_entries_per_canister: value.max_commitment_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
            relay_factory_enabled: value.relay_factory_enabled,
            relay_setup_min_e8s: value.relay_setup_min_e8s,
            relay_setup_dust_e8s: value.relay_setup_dust_e8s,
            relay_setup_refund_cooldown_seconds: value.relay_setup_refund_cooldown_seconds,
            relay_initial_cycles: value.relay_initial_cycles,
            relay_cycle_safety_margin_e8s: value.relay_cycle_safety_margin_e8s,
            relay_min_subaccount_one_seed_e8s: value.relay_min_subaccount_one_seed_e8s,
            self_service_relay_interval_seconds: value.self_service_relay_interval_seconds,
            self_service_relay_max_transfers_per_tick: value
                .self_service_relay_max_transfers_per_tick,
            io_surplus_neuron_id: value.io_surplus_neuron_id,
            canonical_relay_canister_id: value.canonical_relay_canister_id,
            canonical_relay_targets: value.canonical_relay_targets,
        }
    }
}

impl From<LegacyStableRootStateV1> for StableRootState {
    fn from(value: LegacyStableRootStateV1) -> Self {
        Self {
            config: value.config.into(),
            last_indexed_staking_tx_id: value.last_indexed_staking_tx_id,
            oldest_indexed_staking_tx_id: value.oldest_indexed_staking_tx_id,
            staking_index_descending: value.staking_index_descending,
            staking_backfill_complete: value.staking_backfill_complete,
            last_indexed_output_tx_id: value.last_indexed_output_tx_id,
            oldest_indexed_output_tx_id: value.oldest_indexed_output_tx_id,
            output_route_index_descending: value.output_route_index_descending,
            output_route_backfill_complete: value.output_route_backfill_complete,
            last_indexed_rewards_tx_id: value.last_indexed_rewards_tx_id,
            oldest_indexed_rewards_tx_id: value.oldest_indexed_rewards_tx_id,
            rewards_route_index_descending: value.rewards_route_index_descending,
            rewards_route_backfill_complete: value.rewards_route_backfill_complete,
            last_sns_discovery_ts: value.last_sns_discovery_ts,
            last_completed_cycles_sweep_ts: value.last_completed_cycles_sweep_ts,
            last_completed_route_sweep_ts: value.last_completed_route_sweep_ts,
            active_cycles_sweep: value.active_cycles_sweep,
            initial_cycles_probe_queue: value.initial_cycles_probe_queue,
            active_route_sweep: value.active_route_sweep,
            active_sns_discovery: value.active_sns_discovery,
            main_lock_state_ts: value.main_lock_state_ts,
            last_main_run_ts: value.last_main_run_ts,
            qualifying_commitment_count: value.qualifying_commitment_count,
            total_output_e8s: value.total_output_e8s,
            total_rewards_e8s: value.total_rewards_e8s,
            icp_burned_e8s: value.icp_burned_e8s,
            recent_commitments: value.recent_commitments,
            recent_under_threshold_commitments: value.recent_under_threshold_commitments,
            recent_neuron_commitments: value.recent_neuron_commitments,
            recent_under_threshold_neuron_commitments: value
                .recent_under_threshold_neuron_commitments,
            recent_invalid_commitments: value.recent_invalid_commitments,
            recent_burns: value.recent_burns,
            last_index_run_ts: value.last_index_run_ts,
            commitment_index_fault: value.commitment_index_fault,
            icp_xdr_rate: value.icp_xdr_rate,
            last_icp_xdr_rate_attempt_ts: value.last_icp_xdr_rate_attempt_ts,
            last_icp_xdr_rate_error: value.last_icp_xdr_rate_error,
        }
    }
}

impl From<LegacyVersionedStableStateV1> for VersionedStableState {
    fn from(value: LegacyVersionedStableStateV1) -> Self {
        match value {
            LegacyVersionedStableStateV1::Uninitialized => Self::Uninitialized,
            LegacyVersionedStableStateV1::Current(root) => Self::Current(root.into()),
        }
    }
}

pub(crate) fn decode_legacy_root(bytes: &[u8]) -> Result<VersionedStableState, candid::Error> {
    candid::decode_one::<LegacyVersionedStableStateV1>(bytes).map(Into::into)
}

pub(crate) fn decode_legacy_relay_setup_job(bytes: &[u8]) -> Result<RelaySetupJob, candid::Error> {
    candid::decode_one::<LegacyRelaySetupJobV1>(bytes).map(Into::into)
}
