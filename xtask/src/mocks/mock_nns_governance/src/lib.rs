use jupiter_nns_types::{
    manage_neuron, manage_neuron_response, GovernanceError, ListNeurons, ListNeuronsResponse,
    ManageNeuronCommandRequest, ManageNeuronRequest, ManageNeuronResponse, MaturityDisbursement,
    Neuron, NeuronId,
};
use std::cell::RefCell;

type NeuronResult = Result<Neuron, GovernanceError>;

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
        Some(vec![MaturityDisbursement {
            amount_e8s: Some(1),
            ..Default::default()
        }])
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
        ..Default::default()
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
                    ..Default::default()
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    ListNeuronsResponse {
        full_neurons,
        ..Default::default()
    }
}

#[ic_cdk::update]
fn manage_neuron(req: ManageNeuronRequest) -> ManageNeuronResponse {
    let ManageNeuronRequest {
        neuron_id_or_subaccount,
        command,
        id,
    } = req;

    let refreshed_neuron_id = id.or(match neuron_id_or_subaccount {
        Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(id)) => Some(id),
        Some(manage_neuron::NeuronIdOrSubaccount::Subaccount(_)) => None,
        None => None,
    });

    let cmd = match command {
        Some(ManageNeuronCommandRequest::DisburseMaturity(_d)) => {
            Some(manage_neuron_response::Command::DisburseMaturity(
                manage_neuron_response::DisburseMaturityResponse {
                    amount_disbursed_e8s: None,
                },
            ))
        }
        Some(ManageNeuronCommandRequest::RefreshVotingPower(_)) => {
            Some(manage_neuron_response::Command::RefreshVotingPower(
                manage_neuron_response::RefreshVotingPowerResponse {},
            ))
        }
        Some(ManageNeuronCommandRequest::ClaimOrRefresh(_)) => {
            Some(manage_neuron_response::Command::ClaimOrRefresh(
                manage_neuron_response::ClaimOrRefreshResponse {
                    refreshed_neuron_id,
                },
            ))
        }
        _ => Some(manage_neuron_response::Command::Error(GovernanceError {
            error_message: "unsupported".to_string(),
            error_type: -1,
        })),
    };

    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.manage_calls += 1;
        // Any successful DisburseMaturity marks in-flight in this mock.
        if matches!(
            cmd,
            Some(manage_neuron_response::Command::DisburseMaturity(_))
        ) {
            st.in_flight = true;
        }
        if matches!(
            cmd,
            Some(manage_neuron_response::Command::RefreshVotingPower(_))
        ) {
            st.refresh_calls += 1;
        }
        if matches!(
            cmd,
            Some(manage_neuron_response::Command::ClaimOrRefresh(_))
        ) {
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
