use super::*;
#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub staking_account: Account,
    pub output_source_account: Option<Account>,
    pub output_account: Option<Account>,
    pub rewards_account: Option<Account>,
    pub ledger_canister_id: Option<Principal>,
    pub index_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub faucet_canister_id: Option<Principal>,
    pub sns_wasm_canister_id: Option<Principal>,
    pub xrc_canister_id: Option<Principal>,
    pub enable_sns_tracking: Option<bool>,
    pub scan_interval_seconds: Option<u64>,
    pub cycles_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
    pub max_cycles_entries_per_canister: Option<u32>,
    pub max_commitment_entries_per_canister: Option<u32>,
    pub max_index_pages_per_tick: Option<u32>,
    pub max_canisters_per_cycles_tick: Option<u32>,
    pub relay_factory_enabled: Option<bool>,
    pub relay_setup_min_e8s: Option<u64>,
    pub relay_setup_dust_e8s: Option<u64>,
    pub relay_setup_refund_cooldown_seconds: Option<u64>,
    pub relay_initial_cycles: Option<u128>,
    pub relay_cycle_safety_margin_e8s: Option<u64>,
    pub relay_min_subaccount_one_seed_e8s: Option<u64>,
    pub self_service_relay_interval_seconds: Option<u64>,
    pub self_service_relay_max_transfers_per_tick: Option<u32>,
    pub io_surplus_neuron_id: Option<u64>,
    pub canonical_relay_canister_id: Option<Principal>,
    pub canonical_relay_targets: Option<Vec<Principal>>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct UpgradeArgs {
    pub staking_account: Option<Account>,
    pub ledger_canister_id: Option<Principal>,
    pub index_canister_id: Option<Principal>,
    pub enable_sns_tracking: Option<bool>,
    pub clear_commitment_index_fault: Option<bool>,
    pub output_source_account: Option<Account>,
    pub output_account: Option<Account>,
    pub rewards_account: Option<Account>,
    pub scan_interval_seconds: Option<u64>,
    pub cycles_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
    pub max_cycles_entries_per_canister: Option<u32>,
    pub max_commitment_entries_per_canister: Option<u32>,
    pub max_index_pages_per_tick: Option<u32>,
    pub max_canisters_per_cycles_tick: Option<u32>,
    pub sns_wasm_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub faucet_canister_id: Option<Principal>,
    pub xrc_canister_id: Option<Principal>,
    pub relay_factory_enabled: Option<bool>,
    pub relay_setup_min_e8s: Option<u64>,
    pub relay_setup_dust_e8s: Option<u64>,
    pub relay_setup_refund_cooldown_seconds: Option<u64>,
    pub relay_initial_cycles: Option<u128>,
    pub relay_cycle_safety_margin_e8s: Option<u64>,
    pub relay_min_subaccount_one_seed_e8s: Option<u64>,
    pub self_service_relay_interval_seconds: Option<u64>,
    pub self_service_relay_max_transfers_per_tick: Option<Option<u32>>,
    pub io_surplus_neuron_id: Option<u64>,
    pub canonical_relay_canister_id: Option<Option<Principal>>,
    pub canonical_relay_targets: Option<Vec<Principal>>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListCanistersArgs {
    pub start_after: Option<Principal>,
    pub limit: Option<u32>,
    pub tracking_reason_filter: Option<CanisterTrackingReason>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterListItem {
    pub canister_id: Principal,
    pub tracking_reasons: Vec<CanisterTrackingReason>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListCanistersResponse {
    pub items: Vec<CanisterListItem>,
    pub next_start_after: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct GetCyclesHistoryArgs {
    pub canister_id: Principal,
    pub start_after_ts: Option<u64>,
    pub limit: Option<u32>,
    pub descending: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct GetCommitmentHistoryArgs {
    pub canister_id: Principal,
    pub start_after_tx_id: Option<u64>,
    pub limit: Option<u32>,
    pub descending: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CyclesHistoryPage {
    pub items: Vec<CyclesSample>,
    pub next_start_after_ts: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CommitmentHistoryPage {
    pub items: Vec<CommitmentSample>,
    pub next_start_after_tx_id: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterOverview {
    pub canister_id: Principal,
    pub tracking_reasons: Vec<CanisterTrackingReason>,
    pub meta: CanisterMeta,
    pub cycles_points: u32,
    pub commitment_points: u32,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicCounts {
    pub tracked_canister_count: u64,
    pub memo_registered_canister_count: u64,
    pub raw_icp_declared_canister_count: Option<u64>,
    pub declared_neuron_count: Option<u64>,
    pub qualifying_commitment_count: u64,
    pub sns_discovered_canister_count: u64,
    pub relay_target_canister_count: u64,
    pub relay_instance_canister_count: u64,
    pub total_output_e8s: u64,
    pub total_rewards_e8s: u64,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicStatus {
    pub staking_account: Account,
    pub ledger_canister_id: Principal,
    pub faucet_canister_id: Principal,
    pub cmc_canister_id: Option<Principal>,
    pub output_source_account: Option<Account>,
    pub output_account: Option<Account>,
    pub rewards_account: Option<Account>,
    pub index_canister_id: Option<Principal>,
    pub last_index_run_ts: Option<u64>,
    pub index_interval_seconds: u64,
    pub last_completed_cycles_sweep_ts: Option<u64>,
    pub cycles_interval_seconds: u64,
    pub heap_memory_bytes: Option<u64>,
    pub stable_memory_bytes: Option<u64>,
    pub total_memory_bytes: Option<u64>,
    pub commitment_index_fault: Option<CommitmentIndexFault>,
    pub icp_xdr_rate: Option<IcpXdrRateSnapshot>,
    pub last_icp_xdr_rate_error: Option<String>,
    pub relay_factory_enabled: Option<bool>,
    pub relay_setup_min_e8s: Option<u64>,
    pub relay_setup_dust_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct GetRelaySetupViewArgs {
    pub target_canister_id: Principal,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct GetRelaySetupRecoveryViewArgs {
    pub target_canister_id: Principal,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRelayRegistrationsArgs {
    pub start_after: Option<Principal>,
    pub limit: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRelayRegistrationsResponse {
    pub items: Vec<RelayRegistration>,
    pub next_start_after: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RelayRegistration {
    pub target_canister_id: Principal,
    pub relay_canister_id: Principal,
    pub kind: RelayRegistryKind,
    pub created_at_ts: Option<u64>,
}

impl From<RelayRegistryEntry> for RelayRegistration {
    fn from(value: RelayRegistryEntry) -> Self {
        Self {
            target_canister_id: value.target_canister_id,
            relay_canister_id: value.relay_canister_id,
            kind: value.kind,
            created_at_ts: value.created_at_ts,
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Serialize, Debug, PartialEq, Eq)]
pub enum RelaySetupPublicStatus {
    NotFunded,
    BelowMinimum,
    PaymentNotAllowed,
    IndexNotReady,
    Pending,
    CreatingRelay,
    Active,
    SweepingToExistingRelay,
    Refunding,
    Refunded,
    FailedRetryable,
    ManualRecoveryRequired,
}

impl From<RelaySetupStatus> for RelaySetupPublicStatus {
    fn from(value: RelaySetupStatus) -> Self {
        match value {
            RelaySetupStatus::NotFunded => Self::NotFunded,
            RelaySetupStatus::BelowMinimum | RelaySetupStatus::SweepBelowDust => Self::BelowMinimum,
            RelaySetupStatus::TargetNotObservable | RelaySetupStatus::FailedTerminal => {
                Self::ManualRecoveryRequired
            }
            RelaySetupStatus::RefundAvailable | RelaySetupStatus::Refunding => Self::Refunding,
            RelaySetupStatus::Refunded => Self::Refunded,
            RelaySetupStatus::IndexNotReady => Self::IndexNotReady,
            RelaySetupStatus::CreatingCanister
            | RelaySetupStatus::CanisterCreated
            | RelaySetupStatus::InstallingCode
            | RelaySetupStatus::CodeInstalled
            | RelaySetupStatus::SettingPublicLogs
            | RelaySetupStatus::Blackholing => Self::CreatingRelay,
            RelaySetupStatus::Active => Self::Active,
            RelaySetupStatus::SweepingToExistingRelay | RelaySetupStatus::SweptToExistingRelay => {
                Self::SweepingToExistingRelay
            }
            RelaySetupStatus::FailedRetryable | RelaySetupStatus::Ambiguous => {
                Self::FailedRetryable
            }
            RelaySetupStatus::ManualRecoveryRequired => Self::ManualRecoveryRequired,
            RelaySetupStatus::Pending
            | RelaySetupStatus::ConvertingCycles
            | RelaySetupStatus::CycleTransferAccepted
            | RelaySetupStatus::CycleNotifySucceeded
            | RelaySetupStatus::FundingRelaySubaccountOne => Self::Pending,
            RelaySetupStatus::InsufficientForCurrentRate => Self::BelowMinimum,
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RelaySetupView {
    pub target_canister_id: Principal,
    pub setup_account: Account,
    pub setup_account_identifier: String,
    pub minimum_e8s: u64,
    pub current_required_e8s: Option<u64>,
    pub nominal_minimum_e8s: u64,
    pub payment_allowed: bool,
    pub payment_blocked_reason: Option<String>,
    pub existing_relay: Option<RelayRegistration>,
    pub status: RelaySetupPublicStatus,
    pub factory_available: bool,
    pub warning_text: Option<String>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RedactedTransferRecord {
    pub kind: RelaySetupTransferKind,
    pub from_account_identifier: String,
    pub to_account_identifier: String,
    pub amount_e8s: u64,
    pub fee_e8s: u64,
    pub created_at_time_nanos: u64,
    pub block_index: Option<u64>,
    pub completed: bool,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RelayCreateAttemptView {
    pub target_canister_id: Principal,
    pub created_at_ts: u64,
    pub initial_cycles: u128,
    pub create_attach_cycles: u128,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RelaySetupRecoveryView {
    pub target_canister_id: Principal,
    pub status: RelaySetupPublicStatus,
    pub last_error: Option<String>,
    pub relay_canister_id: Option<Principal>,
    pub setup_account_identifier: String,
    pub setup_amount_seen_e8s: u64,
    pub setup_amount_processed_e8s: u64,
    pub cycle_conversion_e8s: Option<u64>,
    pub cycles_minted: Option<u128>,
    pub configured_relay_create_attach_cycles: u128,
    pub relay_onchain_module_hash_hex: Option<String>,
    pub cycle_transfer: Option<RedactedTransferRecord>,
    pub relay_funding_transfer: Option<RedactedTransferRecord>,
    pub existing_relay_sweep_transfer: Option<RedactedTransferRecord>,
    pub refund_transfer_count: u32,
    pub relay_create_attempt: Option<RelayCreateAttemptView>,
    pub created_at_ts: u64,
    pub updated_at_ts: u64,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub enum RelaySetupNotifyResult {
    BelowMinimum {
        minimum_e8s: u64,
        current_balance_e8s: u64,
    },
    InsufficientForCurrentRate {
        required_e8s: u64,
        current_balance_e8s: u64,
    },
    TargetNotObservable {
        message: String,
    },
    Pending {
        status: RelaySetupPublicStatus,
    },
    Active {
        relay: RelayRegistration,
    },
    SweptToExistingRelay {
        relay: RelayRegistration,
        amount_e8s: u64,
        block_index: u64,
    },
    SweepBelowDust {
        relay: RelayRegistration,
        current_balance_e8s: u64,
    },
    Refunded {
        blocks: Vec<u64>,
    },
    RefundPending {
        reason: String,
    },
    Failed {
        status: RelaySetupPublicStatus,
        message: String,
    },
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub enum RelaySetupRefundResult {
    NotEligible { status: Option<RelaySetupStatus> },
    Cooldown { retry_after_seconds: u64 },
    Refunded { blocks: Vec<u64> },
    NoRefundableAmount,
    Failed { message: String },
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRegisteredCanisterSummariesArgs {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RegisteredCanisterSummary {
    pub canister_id: Principal,
    pub tracking_reasons: Vec<CanisterTrackingReason>,
    pub qualifying_commitment_count: u64,
    pub total_qualifying_committed_e8s: u64,
    pub last_commitment_ts: Option<u64>,
    pub latest_cycles: Option<u128>,
    pub last_cycles_probe_ts: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRegisteredCanisterSummariesResponse {
    pub items: Vec<RegisteredCanisterSummary>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct FindCanistersByMemoPrefixArgs {
    pub prefix: String,
    pub limit: Option<u32>,
    pub tracking_reason_filter: Option<CanisterTrackingReason>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterPrefixMatch {
    pub canister_id: Principal,
    pub tracking_reasons: Vec<CanisterTrackingReason>,
    pub matched_prefix: String,
    pub qualifying_commitment_count: u64,
    pub total_qualifying_committed_e8s: u64,
    pub last_commitment_ts: Option<u64>,
    pub latest_cycles: Option<u128>,
    pub last_cycles_probe_ts: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct FindCanistersByMemoPrefixResponse {
    pub items: Vec<CanisterPrefixMatch>,
    pub truncated: bool,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRecentCommitmentsArgs {
    pub limit: Option<u32>,
    pub qualifying_only: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone, Serialize, Debug, PartialEq, Eq)]
pub enum RecentCommitmentOutcomeCategory {
    QualifyingCommitment,
    UnderThresholdCommitment,
    InvalidTargetMemo,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RecentCommitmentListItem {
    pub canister_id: Option<Principal>,
    pub neuron_id: Option<u64>,
    pub raw_icp_memo_text: Option<String>,
    pub neuron_memo_text: Option<String>,
    pub memo_text: Option<String>,
    pub tx_id: u64,
    pub timestamp_nanos: Option<u64>,
    pub amount_e8s: u64,
    pub counts_toward_faucet: bool,
    pub outcome_category: RecentCommitmentOutcomeCategory,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct ListRecentCommitmentsResponse {
    pub items: Vec<RecentCommitmentListItem>,
}

#[derive(CandidType, Deserialize, Clone, Serialize, Debug, PartialEq, Eq)]
pub struct CanisterModuleHash {
    pub canister_id: Principal,
    pub module_hash_hex: Option<String>,
    pub controllers: Option<Vec<Principal>>,
    pub heap_memory_bytes: Option<u64>,
    pub stable_memory_bytes: Option<u64>,
    pub total_memory_bytes: Option<u64>,
}
