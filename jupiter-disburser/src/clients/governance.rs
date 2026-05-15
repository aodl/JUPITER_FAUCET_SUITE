use async_trait::async_trait;
use candid::Principal;
use ic_cdk::call::Call;
use jupiter_nns_types::{
    manage_neuron, manage_neuron_response, Account, Empty, GovernanceError,
    ManageNeuronCommandRequest, ManageNeuronRequest, ManageNeuronResponse, Neuron, NeuronId,
    PrincipalId,
};

use crate::clients::GovernanceClient;

pub struct NnsGovernanceCanister {
    gov_id: Principal,
}

impl NnsGovernanceCanister {
    pub fn new(gov_id: Principal) -> Self {
        Self { gov_id }
    }
}

#[async_trait]
impl GovernanceClient for NnsGovernanceCanister {
    async fn get_full_neuron(&self, neuron_id: u64) -> Result<Neuron, GovernanceError> {
        let resp = Call::bounded_wait(self.gov_id, "get_full_neuron")
            .with_arg(neuron_id)
            .change_timeout(20)
            .await
            .map_err(|e| GovernanceError {
                error_message: format!("call failed: {e:?}"),
                error_type: -1,
            })?;

        let res: Result<Neuron, GovernanceError> = resp.candid().map_err(|e| GovernanceError {
            error_message: format!("decode failed: {e:?}"),
            error_type: -1,
        })?;

        res
    }

    async fn disburse_maturity_to_account(
        &self,
        neuron_id: u64,
        percentage: u32,
        to_owner: Principal,
        to_subaccount: Option<Vec<u8>>,
    ) -> Result<Option<u64>, GovernanceError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(
                NeuronId { id: neuron_id },
            )),
            command: Some(ManageNeuronCommandRequest::DisburseMaturity(
                manage_neuron::DisburseMaturity {
                    percentage_to_disburse: percentage,
                    to_account: Some(Account {
                        owner: Some(PrincipalId::from(to_owner)),
                        subaccount: to_subaccount,
                    }),
                    to_account_identifier: None,
                },
            )),
            id: None,
        };

        let resp = Call::bounded_wait(self.gov_id, "manage_neuron")
            .with_arg(req)
            .change_timeout(60)
            .await
            .map_err(|e| GovernanceError {
                error_message: format!("call failed: {e:?}"),
                error_type: -1,
            })?;

        let decoded: ManageNeuronResponse = resp.candid().map_err(|e| GovernanceError {
            error_message: format!("decode failed: {e:?}"),
            error_type: -1,
        })?;

        match decoded.command {
            Some(manage_neuron_response::Command::DisburseMaturity(r)) => {
                Ok(r.amount_disbursed_e8s)
            }
            Some(manage_neuron_response::Command::Error(e)) => Err(e),
            other => Err(GovernanceError {
                error_message: format!("unexpected manage_neuron response: {other:?}"),
                error_type: -2,
            }),
        }
    }

    async fn refresh_voting_power(&self, neuron_id: u64) -> Result<(), GovernanceError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(
                NeuronId { id: neuron_id },
            )),
            command: Some(ManageNeuronCommandRequest::RefreshVotingPower(
                manage_neuron::RefreshVotingPower {},
            )),
            id: None,
        };

        let resp = Call::bounded_wait(self.gov_id, "manage_neuron")
            .with_arg(req)
            .change_timeout(60)
            .await
            .map_err(|e| GovernanceError {
                error_message: format!("call failed: {e:?}"),
                error_type: -1,
            })?;

        let decoded: ManageNeuronResponse = resp.candid().map_err(|e| GovernanceError {
            error_message: format!("decode failed: {e:?}"),
            error_type: -1,
        })?;

        match decoded.command {
            Some(manage_neuron_response::Command::RefreshVotingPower(_)) => Ok(()),
            Some(manage_neuron_response::Command::Error(e)) => Err(e),
            other => Err(GovernanceError {
                error_message: format!("unexpected manage_neuron response: {other:?}"),
                error_type: -2,
            }),
        }
    }

    async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), GovernanceError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::NeuronId(
                NeuronId { id: neuron_id },
            )),
            command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(
                manage_neuron::ClaimOrRefresh {
                    by: Some(manage_neuron::claim_or_refresh::By::NeuronIdOrSubaccount(
                        Empty {},
                    )),
                },
            )),
            id: None,
        };

        let resp = Call::bounded_wait(self.gov_id, "manage_neuron")
            .with_arg(req)
            .change_timeout(60)
            .await
            .map_err(|e| GovernanceError {
                error_message: format!("call failed: {e:?}"),
                error_type: -1,
            })?;

        let decoded: ManageNeuronResponse = resp.candid().map_err(|e| GovernanceError {
            error_message: format!("decode failed: {e:?}"),
            error_type: -1,
        })?;

        match decoded.command {
            Some(manage_neuron_response::Command::ClaimOrRefresh(_)) => Ok(()),
            Some(manage_neuron_response::Command::Error(e)) => Err(e),
            other => Err(GovernanceError {
                error_message: format!("unexpected manage_neuron response: {other:?}"),
                error_type: -2,
            }),
        }
    }
}
