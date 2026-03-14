use async_trait::async_trait;
use candid::Principal;
use ic_cdk::call::Call;

use crate::clients::GovernanceClient;
use crate::nns_types::{
    By, ClaimOrRefresh, Command1, DisburseMaturity, Empty, GovernanceAccount,
    GovernanceError, ManageNeuronCommandRequest, ManageNeuronRequest, ManageNeuronResponse,
    Neuron, NeuronId, NeuronIdOrSubaccount, NeuronResult,
};

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

        let res: NeuronResult = resp.candid().map_err(|e| GovernanceError {
            error_message: format!("decode failed: {e:?}"),
            error_type: -1,
        })?;

        match res {
            NeuronResult::Ok(n) => Ok(n),
            NeuronResult::Err(e) => Err(e),
        }
    }

    async fn disburse_maturity_to_account(
        &self,
        neuron_id: u64,
        percentage: u32,
        to_owner: Principal,
        to_subaccount: Option<Vec<u8>>,
    ) -> Result<Option<u64>, GovernanceError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
            command: Some(ManageNeuronCommandRequest::DisburseMaturity(DisburseMaturity {
                percentage_to_disburse: percentage,
                to_account: Some(GovernanceAccount {
                    owner: Some(to_owner),
                    subaccount: to_subaccount,
                }),
                to_account_identifier: None,
            })),
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
            Some(Command1::DisburseMaturity(r)) => Ok(r.amount_disbursed_e8s),
            Some(Command1::Error(e)) => Err(e),
            other => Err(GovernanceError {
                error_message: format!("unexpected manage_neuron response: {other:?}"),
                error_type: -2,
            }),
        }
    }

    async fn refresh_voting_power(&self, neuron_id: u64) -> Result<(), GovernanceError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
            command: Some(ManageNeuronCommandRequest::RefreshVotingPower(Empty {})),
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
            Some(Command1::RefreshVotingPower(_)) => Ok(()),
            Some(Command1::Error(e)) => Err(e),
            other => Err(GovernanceError {
                error_message: format!("unexpected manage_neuron response: {other:?}"),
                error_type: -2,
            }),
        }
    }

    async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), GovernanceError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
            command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(ClaimOrRefresh {
                by: Some(By::NeuronIdOrSubaccount(Empty {})),
            })),
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
            Some(Command1::ClaimOrRefresh(_)) => Ok(()),
            Some(Command1::Error(e)) => Err(e),
            other => Err(GovernanceError {
                error_message: format!("unexpected manage_neuron response: {other:?}"),
                error_type: -2,
            }),
        }
    }
}

