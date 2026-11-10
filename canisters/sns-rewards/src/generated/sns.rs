// Generated DTOs from candid/sns-governance/governance.did.
// Official DFINITY revision 98c898f8de068a1fc0e97ebc805f9fa7f315d862.
// candid_parser 0.2.4 emit_bindgen(...).type_defs; no call stubs.
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ProposalId {
    pub id: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RewardEvent {
    pub rounds_since_last_distribution: Option<u64>,
    pub actual_timestamp_seconds: u64,
    pub end_timestamp_seconds: Option<u64>,
    pub total_available_e8s_equivalent: Option<u64>,
    pub distributed_e8s_equivalent: u64,
    pub round: u64,
    pub settled_proposals: Vec<ProposalId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronId {
    pub id: Vec<u8>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Followees {
    pub followees: Vec<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DefaultFollowees {
    pub followees: Vec<(u64, Followees)>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronPermissionList {
    pub permissions: Vec<i32>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct CustomProposalCriticality {
    pub additional_critical_native_action_ids: Vec<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct VotingRewardsParameters {
    pub final_reward_rate_basis_points: Option<u64>,
    pub initial_reward_rate_basis_points: Option<u64>,
    pub reward_rate_transition_duration_seconds: Option<u64>,
    pub round_duration_seconds: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NervousSystemParameters {
    pub default_followees: Option<DefaultFollowees>,
    pub max_dissolve_delay_seconds: Option<u64>,
    pub max_dissolve_delay_bonus_percentage: Option<u64>,
    pub max_followees_per_function: Option<u64>,
    pub automatically_advance_target_version: Option<bool>,
    pub neuron_claimer_permissions: Option<NeuronPermissionList>,
    pub neuron_minimum_stake_e8s: Option<u64>,
    pub max_neuron_age_for_age_bonus: Option<u64>,
    pub initial_voting_period_seconds: Option<u64>,
    pub neuron_minimum_dissolve_delay_to_vote_seconds: Option<u64>,
    pub reject_cost_e8s: Option<u64>,
    pub max_proposals_to_keep_per_action: Option<u32>,
    pub wait_for_quiet_deadline_increase_seconds: Option<u64>,
    pub max_number_of_neurons: Option<u64>,
    pub transaction_fee_e8s: Option<u64>,
    pub custom_proposal_criticality: Option<CustomProposalCriticality>,
    pub max_number_of_proposals_with_ballots: Option<u64>,
    pub max_age_bonus_percentage: Option<u64>,
    pub neuron_grantable_permissions: Option<NeuronPermissionList>,
    pub voting_rewards_parameters: Option<VotingRewardsParameters>,
    pub maturity_modulation_disabled: Option<bool>,
    pub max_number_of_principals_per_neuron: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GetNeuron {
    pub neuron_id: Option<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GovernanceError {
    pub error_message: String,
    pub error_type: i32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronPermission {
    pub principal: Option<Principal>,
    pub permission_type: Vec<i32>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Topic {
    DappCanisterManagement,
    DaoCommunitySettings,
    ApplicationBusinessLogic,
    CriticalDappOperations,
    TreasuryAssetManagement,
    Governance,
    SnsFrameworkManagement,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Followee {
    pub alias: Option<String>,
    pub neuron_id: Option<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct FolloweesForTopic {
    pub topic: Option<Topic>,
    pub followees: Vec<Followee>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronTopicFolloweesInner {
    pub topic_id_to_followees: Vec<(i32, FolloweesForTopic)>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RewardEventParticipation {
    pub reward_event_end_timestamp_seconds: Option<u64>,
    pub reward_shares: Option<candid::Nat>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum DissolveState {
    DissolveDelaySeconds(u64),
    WhenDissolvedTimestampSeconds(u64),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Subaccount {
    pub subaccount: Vec<u8>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Account {
    pub owner: Option<Principal>,
    pub subaccount: Option<Subaccount>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DisburseMaturityInProgress {
    pub timestamp_of_disbursement_seconds: u64,
    pub amount_e8s: u64,
    pub account_to_disburse_to: Option<Account>,
    pub finalize_disbursement_timestamp_seconds: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Neuron {
    pub id: Option<NeuronId>,
    pub staked_maturity_e8s_equivalent: Option<u64>,
    pub permissions: Vec<NeuronPermission>,
    pub maturity_e8s_equivalent: u64,
    pub cached_neuron_stake_e8s: u64,
    pub created_timestamp_seconds: u64,
    pub topic_followees: Option<NeuronTopicFolloweesInner>,
    pub source_nns_neuron_id: Option<u64>,
    pub auto_stake_maturity: Option<bool>,
    pub aging_since_timestamp_seconds: u64,
    pub latest_reward_event_participation: Option<RewardEventParticipation>,
    pub dissolve_state: Option<DissolveState>,
    pub voting_power_percentage_multiplier: u64,
    pub vesting_period_seconds: Option<u64>,
    pub disburse_maturity_in_progress: Vec<DisburseMaturityInProgress>,
    pub followees: Vec<(u64, Followees)>,
    pub neuron_fees_e8s: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Result {
    Error(GovernanceError),
    Neuron(Neuron),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GetNeuronResponse {
    pub result: Option<Result>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GetProposal {
    pub proposal_id: Option<ProposalId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Decimal {
    pub human_readable: Option<String>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Tokens {
    pub e8s: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ValuationFactors {
    pub xdrs_per_icp: Option<Decimal>,
    pub icps_per_token: Option<Decimal>,
    pub tokens: Option<Tokens>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Valuation {
    pub token: Option<i32>,
    pub account: Option<Account>,
    pub valuation_factors: Option<ValuationFactors>,
    pub timestamp_seconds: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MintSnsTokensActionAuxiliary {
    pub valuation: Option<Valuation>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SnsVersion {
    pub archive_wasm_hash: Option<Vec<u8>>,
    pub root_wasm_hash: Option<Vec<u8>>,
    pub swap_wasm_hash: Option<Vec<u8>>,
    pub ledger_wasm_hash: Option<Vec<u8>>,
    pub governance_wasm_hash: Option<Vec<u8>>,
    pub index_wasm_hash: Option<Vec<u8>>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AdvanceSnsTargetVersionActionAuxiliary {
    pub target_version: Option<SnsVersion>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum ActionAuxiliary {
    TransferSnsTreasuryFunds(MintSnsTokensActionAuxiliary),
    MintSnsTokens(MintSnsTokensActionAuxiliary),
    AdvanceSnsTargetVersion(AdvanceSnsTargetVersionActionAuxiliary),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Ballot {
    pub vote: i32,
    pub cast_timestamp_seconds: u64,
    pub voting_power: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Percentage {
    pub basis_points: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Tally {
    pub no: u64,
    pub yes: u64,
    pub total: u64,
    pub timestamp_seconds: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GenericNervousSystemFunction {
    pub topic: Option<Topic>,
    pub validator_canister_id: Option<Principal>,
    pub target_canister_id: Option<Principal>,
    pub validator_method_name: Option<String>,
    pub target_method_name: Option<String>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum FunctionType {
    NativeNervousSystemFunction {},
    GenericNervousSystemFunction(GenericNervousSystemFunction),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NervousSystemFunction {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub function_type: Option<FunctionType>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageDappCanisterSettings {
    pub freezing_threshold: Option<u64>,
    pub wasm_memory_threshold: Option<u64>,
    pub canister_ids: Vec<Principal>,
    pub reserved_cycles_limit: Option<u64>,
    pub log_visibility: Option<i32>,
    pub snapshot_visibility: Option<i32>,
    pub wasm_memory_limit: Option<u64>,
    pub memory_allocation: Option<u64>,
    pub compute_allocation: Option<u64>,
}
/// This type is equivalant to `ICRC3Value`, but we give it another name since it is used here not
/// in the context of the ICRC-3 ledger standard. The justification is the same: The candid format
/// supports sharing information even when the client and the server involved do not have the same
/// schema (see the Upgrading and subtyping section of the candid spec). While this mechanism allows
/// to evolve services and clients independently without breaking them, it also means that a client
/// may not receive all the information that the server is sending, e.g. in case the client schema
/// lacks some fields that the server schema has.
///
/// This loss of information is not an option for SNS voters deciding if an extension with particular
/// init args should be installed or if an extension function with particular arguments should be
/// called. The client must receive the same exact data the server sent in order to verify it.
///
/// Verification of a priorly installed extension is done by hashing the extension's init arg data
/// and checking that the result is consistent with what has been certified by the SNS.
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum PreciseValue {
    Int(i64),
    Map(Vec<(String, Box<PreciseValue>)>),
    Nat(u64),
    Blob(Vec<u8>),
    Bool(bool),
    Text(String),
    Array(Vec<Box<PreciseValue>>),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ExtensionOperationArg {
    pub value: Option<Box<PreciseValue>>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ExecuteExtensionOperation {
    pub extension_canister_id: Option<Principal>,
    pub operation_name: Option<String>,
    pub operation_arg: Option<ExtensionOperationArg>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ChunkedCanisterWasm {
    pub wasm_module_hash: Vec<u8>,
    pub chunk_hashes_list: Vec<Vec<u8>>,
    pub store_canister_id: Option<Principal>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Wasm {
    Chunked(ChunkedCanisterWasm),
    Bytes(Vec<u8>),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ExtensionUpgradeArg {
    pub value: Option<Box<PreciseValue>>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct UpgradeExtension {
    pub extension_canister_id: Option<Principal>,
    pub wasm: Option<Wasm>,
    pub canister_upgrade_arg: Option<ExtensionUpgradeArg>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SetTopicsForCustomProposals {
    pub custom_function_id_to_topic: Vec<(u64, Topic)>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ExtensionInit {
    pub value: Option<Box<PreciseValue>>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RegisterExtension {
    pub chunked_canister_wasm: Option<ChunkedCanisterWasm>,
    pub extension_init: Option<ExtensionInit>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RegisterDappCanisters {
    pub canister_ids: Vec<Principal>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct TransferSnsTreasuryFunds {
    pub from_treasury: i32,
    pub to_principal: Option<Principal>,
    pub to_subaccount: Option<Subaccount>,
    pub memo: Option<u64>,
    pub amount_e8s: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct CanisterUpgradeOptions {
    pub wasm_memory_persistence: Option<i32>,
    pub skip_pre_upgrade: Option<bool>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct UpgradeSnsControlledCanister {
    pub new_canister_wasm: Vec<u8>,
    pub mode: Option<i32>,
    pub canister_id: Option<Principal>,
    pub canister_upgrade_options: Option<CanisterUpgradeOptions>,
    pub chunked_canister_wasm: Option<ChunkedCanisterWasm>,
    pub canister_upgrade_arg: Option<Vec<u8>>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DeregisterDappCanisters {
    pub canister_ids: Vec<Principal>,
    pub new_controllers: Vec<Principal>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MintSnsTokens {
    pub to_principal: Option<Principal>,
    pub to_subaccount: Option<Subaccount>,
    pub memo: Option<u64>,
    pub amount_e8s: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AdvanceSnsTargetVersion {
    pub new_target: Option<SnsVersion>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageSnsMetadata {
    pub url: Option<String>,
    pub logo: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ExecuteGenericNervousSystemFunction {
    pub function_id: u64,
    pub payload: Vec<u8>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageLedgerParameters {
    pub token_symbol: Option<String>,
    pub transfer_fee: Option<u64>,
    pub token_logo: Option<String>,
    pub token_name: Option<String>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Motion {
    pub motion_text: String,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Action {
    ManageNervousSystemParameters(NervousSystemParameters),
    AddGenericNervousSystemFunction(NervousSystemFunction),
    ManageDappCanisterSettings(ManageDappCanisterSettings),
    ExecuteExtensionOperation(ExecuteExtensionOperation),
    UpgradeExtension(UpgradeExtension),
    RemoveGenericNervousSystemFunction(u64),
    SetTopicsForCustomProposals(SetTopicsForCustomProposals),
    RegisterExtension(RegisterExtension),
    UpgradeSnsToNextVersion {},
    RegisterDappCanisters(RegisterDappCanisters),
    TransferSnsTreasuryFunds(TransferSnsTreasuryFunds),
    UpgradeSnsControlledCanister(UpgradeSnsControlledCanister),
    DeregisterDappCanisters(DeregisterDappCanisters),
    MintSnsTokens(MintSnsTokens),
    AdvanceSnsTargetVersion(AdvanceSnsTargetVersion),
    Unspecified {},
    ManageSnsMetadata(ManageSnsMetadata),
    ExecuteGenericNervousSystemFunction(ExecuteGenericNervousSystemFunction),
    ManageLedgerParameters(ManageLedgerParameters),
    Motion(Motion),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Proposal {
    pub url: String,
    pub title: String,
    pub action: Option<Action>,
    pub summary: String,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct WaitForQuietState {
    pub current_deadline_timestamp_seconds: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ProposalData {
    pub id: Option<ProposalId>,
    pub payload_text_rendering: Option<String>,
    pub topic: Option<Topic>,
    pub action: u64,
    pub failure_reason: Option<GovernanceError>,
    pub action_auxiliary: Option<ActionAuxiliary>,
    pub ballots: Vec<(String, Ballot)>,
    pub minimum_yes_proportion_of_total: Option<Percentage>,
    pub reward_event_round: u64,
    pub failed_timestamp_seconds: u64,
    pub reward_event_end_timestamp_seconds: Option<u64>,
    pub proposal_creation_timestamp_seconds: u64,
    pub initial_voting_period_seconds: u64,
    pub reject_cost_e8s: u64,
    pub latest_tally: Option<Tally>,
    pub wait_for_quiet_deadline_increase_seconds: u64,
    pub decided_timestamp_seconds: u64,
    pub proposal: Option<Proposal>,
    pub proposer: Option<NeuronId>,
    pub wait_for_quiet_state: Option<WaitForQuietState>,
    pub minimum_yes_proportion_of_exercised: Option<Percentage>,
    pub is_eligible_for_rewards: bool,
    pub executed_timestamp_seconds: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Result1 {
    Error(GovernanceError),
    Proposal(ProposalData),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GetProposalResponse {
    pub result: Option<Result1>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct TopicSelector {
    pub topic: Option<Topic>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ListProposals {
    pub include_reward_status: Vec<i32>,
    pub before_proposal: Option<ProposalId>,
    pub limit: u32,
    pub exclude_type: Vec<u64>,
    pub include_topics: Option<Vec<TopicSelector>>,
    pub include_status: Vec<i32>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ListProposalsResponse {
    pub include_ballots_by_caller: Option<bool>,
    pub proposals: Vec<ProposalData>,
    pub include_topic_filtering: Option<bool>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Split {
    pub memo: u64,
    pub amount_e8s: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Follow {
    pub function_id: u64,
    pub followees: Vec<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DisburseMaturity {
    pub to_account: Option<Account>,
    pub percentage_to_disburse: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MemoAndController {
    pub controller: Option<Principal>,
    pub memo: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum By {
    MemoAndController(MemoAndController),
    NeuronId {},
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ClaimOrRefresh {
    pub by: Option<By>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ChangeAutoStakeMaturity {
    pub requested_setting_for_auto_stake_maturity: bool,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct IncreaseDissolveDelay {
    pub additional_dissolve_delay_seconds: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SetDissolveTimestamp {
    pub dissolve_timestamp_seconds: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Operation {
    ChangeAutoStakeMaturity(ChangeAutoStakeMaturity),
    StopDissolving {},
    StartDissolving {},
    IncreaseDissolveDelay(IncreaseDissolveDelay),
    SetDissolveTimestamp(SetDissolveTimestamp),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Configure {
    pub operation: Option<Operation>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RegisterVote {
    pub vote: i32,
    pub proposal: Option<ProposalId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SetFollowing {
    pub topic_following: Vec<FolloweesForTopic>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct StakeMaturity {
    pub percentage_to_stake: Option<u32>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RemoveNeuronPermissions {
    pub permissions_to_remove: Option<NeuronPermissionList>,
    pub principal_id: Option<Principal>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AddNeuronPermissions {
    pub permissions_to_add: Option<NeuronPermissionList>,
    pub principal_id: Option<Principal>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MergeMaturity {
    pub percentage_to_merge: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Amount {
    pub e8s: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Disburse {
    pub to_account: Option<Account>,
    pub amount: Option<Amount>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Command {
    Split(Split),
    Follow(Follow),
    DisburseMaturity(DisburseMaturity),
    ClaimOrRefresh(ClaimOrRefresh),
    Configure(Configure),
    RegisterVote(RegisterVote),
    SetFollowing(SetFollowing),
    MakeProposal(Proposal),
    StakeMaturity(StakeMaturity),
    RemoveNeuronPermissions(RemoveNeuronPermissions),
    AddNeuronPermissions(AddNeuronPermissions),
    MergeMaturity(MergeMaturity),
    Disburse(Disburse),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageNeuron {
    pub subaccount: Vec<u8>,
    pub command: Option<Command>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SplitResponse {
    pub created_neuron_id: Option<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DisburseMaturityResponse {
    pub amount_disbursed_e8s: u64,
    pub amount_deducted_e8s: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ClaimOrRefreshResponse {
    pub refreshed_neuron_id: Option<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct StakeMaturityResponse {
    pub maturity_e8s: u64,
    pub staked_maturity_e8s: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MergeMaturityResponse {
    pub merged_maturity_e8s: u64,
    pub new_stake_e8s: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DisburseResponse {
    pub transfer_block_height: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Command1 {
    Error(GovernanceError),
    Split(SplitResponse),
    Follow {},
    DisburseMaturity(DisburseMaturityResponse),
    ClaimOrRefresh(ClaimOrRefreshResponse),
    Configure {},
    RegisterVote {},
    SetFollowing {},
    MakeProposal(GetProposal),
    RemoveNeuronPermission {},
    StakeMaturity(StakeMaturityResponse),
    MergeMaturity(MergeMaturityResponse),
    Disburse(DisburseResponse),
    AddNeuronPermission {},
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageNeuronResponse {
    pub command: Option<Command1>,
}
