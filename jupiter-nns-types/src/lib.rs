pub use ic_base_types::PrincipalId;
pub use ic_nns_common::pb::v1::{NeuronId, ProposalId};
pub use ic_nns_governance_api::{
    claim_or_refresh_neuron_from_account_response, governance_error, list_neurons, manage_neuron,
    manage_neuron_response, neuron, Account, Empty, GovernanceError, ListNeurons,
    ListNeuronsResponse, MakeProposalRequest, ManageNeuronCommandRequest, ManageNeuronRequest,
    ManageNeuronResponse, MaturityDisbursement, Motion, Neuron, ProposalActionRequest,
};

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{decode_one, encode_one, Principal};

    fn neuron_id(id: u64) -> NeuronId {
        NeuronId { id }
    }

    fn manage_request(command: ManageNeuronCommandRequest) -> ManageNeuronRequest {
        ManageNeuronRequest {
            id: None,
            neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(
                neuron_id(42),
            )),
            command: Some(command),
        }
    }

    #[test]
    fn list_neurons_round_trips_through_candid() {
        let request = ListNeurons {
            neuron_ids: vec![42],
            include_neurons_readable_by_caller: false,
            include_empty_neurons_readable_by_caller: Some(false),
            include_public_neurons_in_full_neurons: Some(true),
            page_number: None,
            page_size: None,
            neuron_subaccounts: None,
        };
        let decoded: ListNeurons = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert_eq!(decoded.neuron_ids, vec![42]);

        let response = ListNeuronsResponse {
            full_neurons: vec![Neuron {
                id: Some(neuron_id(42)),
                account: vec![1; 32],
                ..Default::default()
            }],
            ..Default::default()
        };
        let decoded: ListNeuronsResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert_eq!(
            decoded.full_neurons[0].id.as_ref().map(|id| id.id),
            Some(42)
        );
    }

    #[test]
    fn manage_neuron_claim_or_refresh_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::ClaimOrRefresh(
            manage_neuron::ClaimOrRefresh {
                by: Some(manage_neuron::claim_or_refresh::By::NeuronIdOrSubaccount(
                    Empty {},
                )),
            },
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::ClaimOrRefresh(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::ClaimOrRefresh(
                manage_neuron_response::ClaimOrRefreshResponse {
                    refreshed_neuron_id: Some(neuron_id(42)),
                },
            )),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(manage_neuron_response::Command::ClaimOrRefresh(_))
        ));
    }

    #[test]
    fn manage_neuron_error_round_trips_through_candid() {
        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::Error(GovernanceError {
                error_type: governance_error::ErrorType::PreconditionFailed as i32,
                error_message: "not allowed".to_string(),
            })),
        };

        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        match decoded.command {
            Some(manage_neuron_response::Command::Error(err)) => {
                assert_eq!(
                    err.error_type,
                    governance_error::ErrorType::PreconditionFailed as i32
                );
                assert_eq!(err.error_message, "not allowed");
            }
            other => panic!("expected manage_neuron error response, got {other:?}"),
        }
    }

    #[test]
    fn manage_neuron_refresh_voting_power_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::RefreshVotingPower(
            manage_neuron::RefreshVotingPower {},
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::RefreshVotingPower(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::RefreshVotingPower(
                manage_neuron_response::RefreshVotingPowerResponse {},
            )),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(manage_neuron_response::Command::RefreshVotingPower(_))
        ));
    }

    #[test]
    fn manage_neuron_disburse_maturity_round_trips_through_candid() {
        let request = manage_request(ManageNeuronCommandRequest::DisburseMaturity(
            manage_neuron::DisburseMaturity {
                percentage_to_disburse: 100,
                to_account: Some(Account {
                    owner: Some(PrincipalId::from(Principal::anonymous())),
                    subaccount: Some(vec![7; 32]),
                }),
                to_account_identifier: None,
            },
        ));
        let decoded: ManageNeuronRequest = decode_one(&encode_one(&request).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(ManageNeuronCommandRequest::DisburseMaturity(_))
        ));

        let response = ManageNeuronResponse {
            command: Some(manage_neuron_response::Command::DisburseMaturity(
                manage_neuron_response::DisburseMaturityResponse {
                    amount_disbursed_e8s: Some(1_000),
                },
            )),
        };
        let decoded: ManageNeuronResponse = decode_one(&encode_one(&response).unwrap()).unwrap();
        assert!(matches!(
            decoded.command,
            Some(manage_neuron_response::Command::DisburseMaturity(_))
        ));
    }

    #[test]
    fn get_full_neuron_result_round_trips_through_candid() {
        let response: Result<Neuron, GovernanceError> = Ok(Neuron {
            id: Some(neuron_id(42)),
            account: vec![1; 32],
            aging_since_timestamp_seconds: 1_234,
            maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                amount_e8s: Some(55),
                ..Default::default()
            }]),
            ..Default::default()
        });
        let decoded: Result<Neuron, GovernanceError> =
            decode_one(&encode_one(&response).unwrap()).unwrap();
        assert_eq!(
            decoded.unwrap().maturity_disbursements_in_progress.unwrap()[0].amount_e8s,
            Some(55)
        );
    }
}
