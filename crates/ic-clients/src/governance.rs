//! Shared NNS governance request/response plumbing.
//!
//! This module owns common request construction and response classification used
//! by faucet/relay neuron-stake flows. Canister-specific maturity and
//! disbursement policy remains local to the relevant canister.

use candid::Principal;
use jupiter_nns_types::{
    list_neurons, manage_neuron, manage_neuron_response, ListNeurons, ListNeuronsResponse,
    ManageNeuronCommandRequest, ManageNeuronRequest, ManageNeuronResponse, NeuronId,
};

use crate::generated::nns_governance_transport::{self, GovernanceCallWait};
use crate::ClientError;

fn list_neurons_request(neuron_id: u64) -> ListNeurons {
    ListNeurons {
        neuron_ids: vec![neuron_id],
        include_neurons_readable_by_caller: false,
        include_empty_neurons_readable_by_caller: Some(false),
        include_public_neurons_in_full_neurons: Some(true),
        page_number: None,
        page_size: None,
        neuron_subaccounts: None::<Vec<list_neurons::NeuronSubaccount>>,
    }
}

fn claim_or_refresh_request(neuron_id: u64) -> ManageNeuronRequest {
    ManageNeuronRequest {
        id: None,
        neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(NeuronId {
            id: neuron_id,
        })),
        command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(
            manage_neuron::ClaimOrRefresh {
                by: Some(manage_neuron::claim_or_refresh::By::NeuronIdOrSubaccount {}),
            },
        )),
    }
}

fn staking_subaccount_from_list_neurons_response(
    neuron_id: u64,
    decoded: ListNeuronsResponse,
) -> Result<[u8; 32], ClientError> {
    let neuron = decoded
        .full_neurons
        .into_iter()
        .find(|neuron| neuron.id.as_ref().map(|id| id.id) == Some(neuron_id))
        .ok_or_else(|| {
            ClientError::Call(format!(
                "list_neurons returned no public full neuron for {neuron_id}"
            ))
        })?;
    let account: &[u8] = neuron.account.as_ref();
    if account.len() != 32 {
        return Err(ClientError::Convert(format!(
            "list_neurons returned {}-byte staking subaccount",
            account.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(account);
    Ok(out)
}

fn classify_claim_or_refresh_response(decoded: ManageNeuronResponse) -> Result<(), ClientError> {
    match decoded.command {
        Some(manage_neuron_response::Command::ClaimOrRefresh(_)) => Ok(()),
        Some(manage_neuron_response::Command::Error(err)) => Err(ClientError::Call(format!(
            "claim_or_refresh failed: type={} message={}",
            err.error_type, err.error_message
        ))),
        None => Err(ClientError::Call(
            "claim_or_refresh returned no command".to_string(),
        )),
        other => Err(ClientError::Call(format!(
            "unexpected claim_or_refresh response: {other:?}"
        ))),
    }
}

pub struct NnsGovernanceCanister {
    governance_id: Principal,
}

impl NnsGovernanceCanister {
    pub fn new(governance_id: Principal) -> Self {
        Self { governance_id }
    }

    pub async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], ClientError> {
        let req = list_neurons_request(neuron_id);
        let resp = nns_governance_transport::list_neurons(
            self.governance_id,
            &req,
            GovernanceCallWait::bounded_default(),
        )
        .await
        .map_err(|e| ClientError::Call(format!("list_neurons transport failed: {e:?}")))?;
        let decoded: ListNeuronsResponse = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode list_neurons failed: {e:?}")))?;
        staking_subaccount_from_list_neurons_response(neuron_id, decoded)
    }

    pub async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), ClientError> {
        let req = claim_or_refresh_request(neuron_id);
        let resp = nns_governance_transport::manage_neuron(
            self.governance_id,
            &req,
            GovernanceCallWait::bounded_default(),
        )
        .await
        .map_err(|e| ClientError::Call(format!("claim_or_refresh transport failed: {e:?}")))?;
        let decoded: ManageNeuronResponse = resp.candid().map_err(|e| {
            ClientError::Call(format!("decode manage_neuron ClaimOrRefresh failed: {e:?}"))
        })?;
        classify_claim_or_refresh_response(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_nns_types::{governance_error, GovernanceError, Neuron};

    #[test]
    fn list_neurons_request_targets_public_full_neuron() {
        let req = list_neurons_request(42);
        assert_eq!(req.neuron_ids, vec![42]);
        assert!(!req.include_neurons_readable_by_caller);
        assert_eq!(req.include_empty_neurons_readable_by_caller, Some(false));
        assert_eq!(req.include_public_neurons_in_full_neurons, Some(true));
        assert!(req.neuron_subaccounts.is_none());
    }

    #[test]
    fn claim_or_refresh_request_targets_neuron_id() {
        let req = claim_or_refresh_request(42);
        assert!(matches!(
            req.neuron_id_or_subaccount,
            Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(NeuronId {
                id: 42
            }))
        ));
        assert!(matches!(
            req.command,
            Some(ManageNeuronCommandRequest::ClaimOrRefresh(_))
        ));
    }

    #[test]
    fn extracts_matching_staking_subaccount() {
        let account = [7u8; 32];
        let resp = ListNeuronsResponse {
            full_neurons: vec![Neuron {
                id: Some(NeuronId { id: 42 }),
                account: account.to_vec(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            staking_subaccount_from_list_neurons_response(42, resp).unwrap(),
            account
        );
    }

    #[test]
    fn rejects_wrong_length_staking_subaccount() {
        let resp = ListNeuronsResponse {
            full_neurons: vec![Neuron {
                id: Some(NeuronId { id: 42 }),
                account: vec![7; 31],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(matches!(
            staking_subaccount_from_list_neurons_response(42, resp),
            Err(ClientError::Convert(_))
        ));
    }

    #[test]
    fn classifies_claim_or_refresh_success() {
        let resp = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::ClaimOrRefresh(
                manage_neuron_response::ClaimOrRefreshResponse {
                    refreshed_neuron_id: Some(NeuronId { id: 42 }),
                },
            )),
        };
        assert!(classify_claim_or_refresh_response(resp).is_ok());
    }

    #[test]
    fn classifies_claim_or_refresh_error() {
        let resp = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::Error(GovernanceError {
                error_type: governance_error::ErrorType::PreconditionFailed as i32,
                error_message: "no".to_string(),
            })),
        };
        assert!(matches!(
            classify_claim_or_refresh_response(resp),
            Err(ClientError::Call(message)) if message.contains("claim_or_refresh failed")
        ));
    }
}
