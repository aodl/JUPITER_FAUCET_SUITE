use candid::{CandidType, Deserialize, Principal};
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct GovernanceError {
    pub error_message: String,
    pub error_type: i32,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct MaturityDisbursement {
    pub amount_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Neuron {
    pub id: Option<NeuronId>,
    pub account: Vec<u8>,
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
pub struct NeuronSubaccount {
    pub subaccount: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ListNeurons {
    pub neuron_ids: Vec<u64>,
    pub include_neurons_readable_by_caller: bool,
    pub include_empty_neurons_readable_by_caller: Option<bool>,
    pub include_public_neurons_in_full_neurons: Option<bool>,
    pub page_number: Option<u64>,
    pub page_size: Option<u64>,
    pub neuron_subaccounts: Option<Vec<NeuronSubaccount>>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ListNeuronsResponse {
    pub full_neurons: Vec<Neuron>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Account {
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
    pub to_account: Option<Account>,
    pub to_account_identifier: Option<AccountIdentifier>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct DisburseMaturityResponse {
    pub amount_disbursed_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ClaimOrRefresh {
    pub by: Option<By>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum By {
    NeuronIdOrSubaccount(Empty),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ClaimOrRefreshResponse {
    pub refreshed_neuron_id: Option<NeuronId>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum ManageNeuronCommandRequest {
    DisburseMaturity(DisburseMaturity),
    RefreshVotingPower(Empty),
    ClaimOrRefresh(ClaimOrRefresh),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Empty {}

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
    ClaimOrRefresh(ClaimOrRefreshResponse),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ManageNeuronResponse {
    pub command: Option<Command1>,
}

#[derive(Default)]
struct GovState {
    in_flight: bool,
    aging_since: u64,
    manage_calls: u64,
    refresh_calls: u64,
    claim_or_refresh_calls: u64,
}

thread_local! {
    static ST: RefCell<GovState> = RefCell::new(GovState::default());
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn get_full_neuron(neuron_id: u64) -> NeuronResult {
    let (in_flight, aging_since) = ST.with(|s| {
        let st = s.borrow();
        (st.in_flight, st.aging_since)
    });

    let disb = if in_flight {
        Some(vec![MaturityDisbursement { amount_e8s: Some(1) }])
    } else {
        Some(vec![])
    };

    let mut account = vec![0u8; 32];
    account[24..].copy_from_slice(&neuron_id.to_be_bytes());

    NeuronResult::Ok(Neuron {
        id: Some(NeuronId { id: neuron_id }),
        account,
        aging_since_timestamp_seconds: aging_since,
        maturity_disbursements_in_progress: disb,
    })
}

#[ic_cdk::update]
fn list_neurons(req: ListNeurons) -> ListNeuronsResponse {
    let full_neurons = if req.include_public_neurons_in_full_neurons == Some(true) {
        req.neuron_ids
            .into_iter()
            .filter(|id| *id != 0)
            .map(|id| {
                let mut account = vec![0u8; 32];
                account[24..].copy_from_slice(&id.to_be_bytes());
                Neuron {
                    id: Some(NeuronId { id }),
                    account,
                    aging_since_timestamp_seconds: 0,
                    maturity_disbursements_in_progress: Some(vec![]),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    ListNeuronsResponse { full_neurons }
}

#[ic_cdk::update]
fn manage_neuron(req: ManageNeuronRequest) -> ManageNeuronResponse {
    let ManageNeuronRequest {
        neuron_id_or_subaccount,
        command,
        id,
    } = req;

    let refreshed_neuron_id = id.or_else(|| match neuron_id_or_subaccount {
        Some(NeuronIdOrSubaccount::NeuronId(id)) => Some(id),
        None => None,
    });

    let cmd = match command {
        Some(ManageNeuronCommandRequest::DisburseMaturity(_d)) => Some(Command1::DisburseMaturity(
            DisburseMaturityResponse { amount_disbursed_e8s: None },
        )),
        Some(ManageNeuronCommandRequest::RefreshVotingPower(_)) => {
            Some(Command1::RefreshVotingPower(Empty {}))
        }
        Some(ManageNeuronCommandRequest::ClaimOrRefresh(_)) => {
            Some(Command1::ClaimOrRefresh(ClaimOrRefreshResponse { refreshed_neuron_id }))
        }
        _ => Some(Command1::Error(GovernanceError {
            error_message: "unsupported".to_string(),
            error_type: -1,
        })),
    };

    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.manage_calls += 1;
        // Any successful DisburseMaturity marks in-flight in this mock.
        if matches!(cmd, Some(Command1::DisburseMaturity(_))) {
            st.in_flight = true;
        }
        if matches!(cmd, Some(Command1::RefreshVotingPower(_))) {
            st.refresh_calls += 1;
        }
        if matches!(cmd, Some(Command1::ClaimOrRefresh(_))) {
            st.claim_or_refresh_calls += 1;
        }
    });

    ManageNeuronResponse { command: cmd }
}

#[ic_cdk::update]
fn debug_reset() {
    ST.with(|s| *s.borrow_mut() = GovState::default());
}

#[ic_cdk::update]
fn debug_set_in_flight(v: bool) {
    ST.with(|s| s.borrow_mut().in_flight = v);
}

#[ic_cdk::update]
fn debug_set_aging_since(ts: u64) {
    ST.with(|s| s.borrow_mut().aging_since = ts);
}

#[ic_cdk::query]
fn debug_get_manage_calls() -> u64 {
    ST.with(|s| s.borrow().manage_calls)
}

#[ic_cdk::query]
fn debug_get_refresh_calls() -> u64 {
    ST.with(|s| s.borrow().refresh_calls)
}

#[ic_cdk::query]
fn debug_get_claim_or_refresh_calls() -> u64 {
    ST.with(|s| s.borrow().claim_or_refresh_calls)
}

ic_cdk::export_candid!();
