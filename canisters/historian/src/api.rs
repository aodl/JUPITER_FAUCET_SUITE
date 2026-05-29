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
    pub blackhole_canister_id: Option<Principal>,
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
    pub blackhole_canister_id: Option<Principal>,
    pub sns_wasm_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub faucet_canister_id: Option<Principal>,
    pub xrc_canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListCanistersArgs {
    pub start_after: Option<Principal>,
    pub limit: Option<u32>,
    pub source_filter: Option<CanisterSource>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterListItem {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
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
    pub sources: Vec<CanisterSource>,
    pub meta: CanisterMeta,
    pub cycles_points: u32,
    pub commitment_points: u32,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct PublicCounts {
    pub registered_canister_count: u64,
    pub qualifying_commitment_count: u64,
    pub sns_discovered_canister_count: u64,
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
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct ListRegisteredCanisterSummariesArgs {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct RegisteredCanisterSummary {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
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
    pub source_filter: Option<CanisterSource>,
}

#[derive(CandidType, Deserialize, Clone, Serialize)]
pub struct CanisterPrefixMatch {
    pub canister_id: Principal,
    pub sources: Vec<CanisterSource>,
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
