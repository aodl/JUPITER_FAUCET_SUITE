// Generated from candid/nns-governance/governance.subset.did.
// Upstream source: dfinity/ic rs/nns/governance/canister/governance.did.
// Upstream commit: 0c7c8b83144844e1a598633585b3ee1beebe338b.
// Generator: nns-bindgen-check using candid_parser = 0.2.4
// emit_bindgen(...).type_defs.
//
// Do not edit manually. Run:
//   cargo run -p nns-bindgen-check -- --update
// Then review the generated diff.

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronId {
    pub id: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ProposalId {
    pub id: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct BallotInfo {
    pub vote: i32,
    pub proposal_id: Option<ProposalId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AccountIdentifier {
    pub hash: Vec<u8>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Account {
    pub owner: Option<Principal>,
    pub subaccount: Option<Vec<u8>>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MaturityDisbursement {
    pub account_identifier_to_disburse_to: Option<AccountIdentifier>,
    pub timestamp_of_disbursement_seconds: Option<u64>,
    pub amount_e8s: Option<u64>,
    pub account_to_disburse_to: Option<Account>,
    pub finalize_disbursement_timestamp_seconds: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum DissolveState {
    DissolveDelaySeconds(u64),
    WhenDissolvedTimestampSeconds(u64),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Followees {
    pub followees: Vec<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronStakeTransfer {
    pub to_subaccount: Vec<u8>,
    pub neuron_stake_e8s: u64,
    pub from: Option<Principal>,
    pub memo: u64,
    pub from_subaccount: Vec<u8>,
    pub transfer_timestamp: u64,
    pub block_height: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct KnownNeuronData {
    pub name: String,
    pub description: Option<String>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Neuron {
    pub id: Option<NeuronId>,
    pub staked_maturity_e8s_equivalent: Option<u64>,
    pub controller: Option<Principal>,
    pub recent_ballots: Vec<BallotInfo>,
    pub voting_power_refreshed_timestamp_seconds: Option<u64>,
    pub kyc_verified: bool,
    pub potential_voting_power: Option<u64>,
    pub neuron_type: Option<i32>,
    pub not_for_profit: bool,
    pub maturity_e8s_equivalent: u64,
    pub deciding_voting_power: Option<u64>,
    pub cached_neuron_stake_e8s: u64,
    pub created_timestamp_seconds: u64,
    pub auto_stake_maturity: Option<bool>,
    pub aging_since_timestamp_seconds: u64,
    pub hot_keys: Vec<Principal>,
    pub account: Vec<u8>,
    pub joined_community_fund_timestamp_seconds: Option<u64>,
    pub eight_year_gang_bonus_base_e8s: Option<u64>,
    pub maturity_disbursements_in_progress: Option<Vec<MaturityDisbursement>>,
    pub dissolve_state: Option<DissolveState>,
    pub followees: Vec<(i32, Followees)>,
    pub neuron_fees_e8s: u64,
    pub visibility: Option<i32>,
    pub transfer: Option<NeuronStakeTransfer>,
    pub known_neuron_data: Option<KnownNeuronData>,
    pub spawn_at_timestamp_seconds: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct GovernanceError {
    pub error_message: String,
    pub error_type: i32,
}
pub type Result2 = std::result::Result<Neuron, GovernanceError>;
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum NeuronIdOrSubaccount {
    Subaccount(Vec<u8>),
    NeuronId(NeuronId),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronSubaccount {
    pub subaccount: Vec<u8>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ListNeurons {
    pub page_size: Option<u64>,
    pub include_public_neurons_in_full_neurons: Option<bool>,
    pub neuron_ids: Vec<u64>,
    pub page_number: Option<u64>,
    pub include_empty_neurons_readable_by_caller: Option<bool>,
    pub neuron_subaccounts: Option<Vec<NeuronSubaccount>>,
    pub include_neurons_readable_by_caller: bool,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct NeuronInfo {}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ListNeuronsResponse {
    pub neuron_infos: Vec<(u64, NeuronInfo)>,
    pub full_neurons: Vec<Neuron>,
    pub total_pages_available: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DisburseMaturity {
    pub to_account_identifier: Option<AccountIdentifier>,
    pub to_account: Option<Account>,
    pub percentage_to_disburse: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RefreshVotingPower {}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum By {
    NeuronIdOrSubaccount {},
    Memo(u64),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ClaimOrRefresh {
    pub by: Option<By>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AddHotKey {
    pub new_hot_key: Option<Principal>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct IncreaseDissolveDelay {
    pub additional_dissolve_delay_seconds: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SetVisibility {
    pub visibility: Option<i32>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Operation {
    AddHotKey(AddHotKey),
    IncreaseDissolveDelay(IncreaseDissolveDelay),
    SetVisibility(SetVisibility),
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
pub struct Motion {
    pub motion_text: String,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum ProposalActionRequest {
    Motion(Motion),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MakeProposalRequest {
    pub url: String,
    pub title: Option<String>,
    pub action: Option<ProposalActionRequest>,
    pub summary: String,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum ManageNeuronCommandRequest {
    DisburseMaturity(DisburseMaturity),
    RefreshVotingPower(RefreshVotingPower),
    ClaimOrRefresh(ClaimOrRefresh),
    Configure(Configure),
    RegisterVote(RegisterVote),
    MakeProposal(MakeProposalRequest),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageNeuronRequest {
    pub id: Option<NeuronId>,
    pub command: Option<ManageNeuronCommandRequest>,
    pub neuron_id_or_subaccount: Option<NeuronIdOrSubaccount>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct DisburseMaturityResponse {
    pub amount_disbursed_e8s: Option<u64>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RefreshVotingPowerResponse {}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ClaimOrRefreshResponse {
    pub refreshed_neuron_id: Option<NeuronId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct MakeProposalResponse {
    pub message: Option<String>,
    pub proposal_id: Option<ProposalId>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum Command {
    Error(GovernanceError),
    DisburseMaturity(DisburseMaturityResponse),
    RefreshVotingPower(RefreshVotingPowerResponse),
    ClaimOrRefresh(ClaimOrRefreshResponse),
    Configure {},
    RegisterVote {},
    MakeProposal(MakeProposalResponse),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ManageNeuronResponse {
    pub command: Option<Command>,
}
