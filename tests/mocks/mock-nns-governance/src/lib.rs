use jupiter_nns_types::{
    manage_neuron, manage_neuron_response, GovernanceError, ListNeurons, ListNeuronsResponse,
    ManageNeuronCommandRequest, ManageNeuronRequest, ManageNeuronResponse, MaturityDisbursement,
    Neuron, NeuronId,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

type NeuronResult = Result<Neuron, GovernanceError>;

#[derive(Default)]
struct GovState {
    in_flight: bool,
    aging_since: u64,
    manage_calls: u64,
    refresh_calls: u64,
    claim_or_refresh_calls: u64,
    claim_or_refresh_neuron_ids: Vec<u64>,
    list_neurons_calls: u64,
    claim_or_refresh_fails: bool,
    hidden_neuron_ids: BTreeSet<u64>,
    neuron_accounts: BTreeMap<u64, Vec<u8>>,
    neuron_controllers: BTreeMap<u64, candid::Principal>,
}

thread_local! {
    static ST: RefCell<GovState> = RefCell::new(GovState::default());
}

#[ic_cdk::init]
fn init() {}

fn neuron_account(neuron_id: u64) -> Vec<u8> {
    ST.with(|state| {
        state
            .borrow()
            .neuron_accounts
            .get(&neuron_id)
            .cloned()
            .unwrap_or_else(|| {
                let mut account = vec![0u8; 32];
                account[24..].copy_from_slice(&neuron_id.to_be_bytes());
                account
            })
    })
}

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

    NeuronResult::Ok(Neuron {
        id: Some(NeuronId { id: neuron_id }),
        account: neuron_account(neuron_id),
        controller: ST.with(|s| s.borrow().neuron_controllers.get(&neuron_id).copied()),
        aging_since_timestamp_seconds: aging_since,
        maturity_disbursements_in_progress: disb,
        ..Default::default()
    })
}

#[ic_cdk::update]
fn list_neurons(req: ListNeurons) -> ListNeuronsResponse {
    ST.with(|state| state.borrow_mut().list_neurons_calls += 1);
    let hidden_neuron_ids = ST.with(|state| state.borrow().hidden_neuron_ids.clone());
    let full_neurons = if req.include_public_neurons_in_full_neurons == Some(true) {
        req.neuron_ids
            .into_iter()
            .filter(|id| *id != 0 && !hidden_neuron_ids.contains(id))
            .map(|id| Neuron {
                id: Some(NeuronId { id }),
                account: neuron_account(id),
                controller: ST.with(|s| s.borrow().neuron_controllers.get(&id).copied()),
                aging_since_timestamp_seconds: 0,
                maturity_disbursements_in_progress: Some(vec![]),
                ..Default::default()
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
    let refreshed_neuron_id_value = refreshed_neuron_id.as_ref().map(|value| value.id);

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
            if ST.with(|s| s.borrow().claim_or_refresh_fails) {
                Some(manage_neuron_response::Command::Error(GovernanceError {
                    error_message: "injected claim_or_refresh failure".to_string(),
                    error_type: -2,
                }))
            } else {
                Some(manage_neuron_response::Command::ClaimOrRefresh(
                    manage_neuron_response::ClaimOrRefreshResponse {
                        refreshed_neuron_id,
                    },
                ))
            }
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
            if let Some(neuron_id) = refreshed_neuron_id_value {
                st.claim_or_refresh_neuron_ids.push(neuron_id);
            }
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

#[ic_cdk::update]
fn debug_set_claim_or_refresh_fails(v: bool) {
    ST.with(|s| s.borrow_mut().claim_or_refresh_fails = v);
}

#[ic_cdk::update]
fn debug_set_neuron_public(neuron_id: u64, is_public: bool) {
    ST.with(|state| {
        let mut state = state.borrow_mut();
        if is_public {
            state.hidden_neuron_ids.remove(&neuron_id);
        } else {
            state.hidden_neuron_ids.insert(neuron_id);
        }
    });
}

#[ic_cdk::update]
fn debug_set_neuron_account(neuron_id: u64, account: Vec<u8>) {
    assert_eq!(account.len(), 32);
    ST.with(|state| {
        state
            .borrow_mut()
            .neuron_accounts
            .insert(neuron_id, account)
    });
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

#[ic_cdk::query]
fn debug_get_claim_or_refresh_neuron_ids() -> Vec<u64> {
    ST.with(|s| s.borrow().claim_or_refresh_neuron_ids.clone())
}

#[ic_cdk::query]
fn debug_get_list_neurons_calls() -> u64 {
    ST.with(|s| s.borrow().list_neurons_calls)
}

ic_cdk::export_candid!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_neuron_account_agrees_across_both_reads_and_reset() {
        debug_reset();
        let original = neuron_account(42);
        assert_eq!(&original[24..], &42u64.to_be_bytes());
        debug_set_neuron_account(42, vec![7; 32]);
        assert_eq!(get_full_neuron(42).unwrap().account, vec![7; 32]);
        let response = list_neurons(ListNeurons {
            neuron_ids: vec![42],
            include_public_neurons_in_full_neurons: Some(true),
            ..Default::default()
        });
        assert_eq!(response.full_neurons[0].account, vec![7; 32]);
        debug_reset();
        assert_eq!(neuron_account(42), original);
    }
}

#[ic_cdk::update]
fn debug_set_neuron_controller(neuron_id: u64, controller: candid::Principal) {
    ST.with_borrow_mut(|s| {
        s.neuron_controllers.insert(neuron_id, controller);
    });
}
