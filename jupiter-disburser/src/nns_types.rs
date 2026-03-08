use candid::{CandidType, Deserialize, Principal};

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct GovernanceError {
    pub error_message: String,
    pub error_type: i32,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct MaturityDisbursement {
    // Matches NNS governance candid, where this field is `opt nat64`.
    // Keeping it optional avoids decode failures against both real NNS and our mocks.
    pub amount_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Neuron {
    pub aging_since_timestamp_seconds: u64,
    pub maturity_disbursements_in_progress: Option<Vec<MaturityDisbursement>>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum NeuronResult {
    Ok(Neuron),
    Err(GovernanceError),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct NeuronId {
    pub id: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct GovernanceAccount {
    pub owner: Option<Principal>,
    pub subaccount: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct AccountIdentifier {
    pub hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct DisburseMaturity {
    pub percentage_to_disburse: u32,
    pub to_account: Option<GovernanceAccount>,
    pub to_account_identifier: Option<AccountIdentifier>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct DisburseMaturityResponse {
    pub amount_disbursed_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Empty {}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum ManageNeuronCommandRequest {
    DisburseMaturity(DisburseMaturity),
    RefreshVotingPower(Empty),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum NeuronIdOrSubaccount {
    NeuronId(NeuronId),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ManageNeuronRequest {
    pub neuron_id_or_subaccount: Option<NeuronIdOrSubaccount>,
    pub command: Option<ManageNeuronCommandRequest>,
    pub id: Option<NeuronId>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum Command1 {
    Error(GovernanceError),
    DisburseMaturity(DisburseMaturityResponse),
    RefreshVotingPower(Empty),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ManageNeuronResponse {
    pub command: Option<Command1>,
}


