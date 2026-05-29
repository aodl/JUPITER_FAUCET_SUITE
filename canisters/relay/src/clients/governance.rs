use async_trait::async_trait;
use candid::Principal;
use ic_cdk::call::Call;
use jupiter_nns_types::{
    list_neurons, manage_neuron, manage_neuron_response, Empty, ListNeurons, ListNeuronsResponse,
    ManageNeuronCommandRequest, ManageNeuronRequest, ManageNeuronResponse, NeuronId,
};

use crate::clients::{ClientError, GovernanceClient};

pub(crate) struct NnsGovernanceCanister {
    governance_id: Principal,
}

impl NnsGovernanceCanister {
    pub(crate) fn new(governance_id: Principal) -> Self {
        Self { governance_id }
    }
}

#[async_trait]
impl GovernanceClient for NnsGovernanceCanister {
    async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], ClientError> {
        let req = ListNeurons {
            neuron_ids: vec![neuron_id],
            include_neurons_readable_by_caller: false,
            include_empty_neurons_readable_by_caller: Some(false),
            include_public_neurons_in_full_neurons: Some(true),
            page_number: None,
            page_size: None,
            neuron_subaccounts: None::<Vec<list_neurons::NeuronSubaccount>>,
        };
        let resp = Call::bounded_wait(self.governance_id, "list_neurons")
            .with_arg(req)
            .await
            .map_err(|e| ClientError::Call(format!("list_neurons transport failed: {e:?}")))?;
        let decoded: ListNeuronsResponse = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode list_neurons failed: {e:?}")))?;
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

    async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), ClientError> {
        let req = ManageNeuronRequest {
            id: None,
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
        };
        let resp = Call::bounded_wait(self.governance_id, "manage_neuron")
            .with_arg(req)
            .await
            .map_err(|e| ClientError::Call(format!("claim_or_refresh transport failed: {e:?}")))?;
        let decoded: ManageNeuronResponse = resp.candid().map_err(|e| {
            ClientError::Call(format!("decode manage_neuron ClaimOrRefresh failed: {e:?}"))
        })?;
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
}
