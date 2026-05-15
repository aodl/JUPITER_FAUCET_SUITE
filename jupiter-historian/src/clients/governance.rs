use async_trait::async_trait;
use candid::Principal;
use ic_cdk::call::Call;
use jupiter_nns_types::{
    manage_neuron, manage_neuron_response, Empty, ManageNeuronCommandRequest, ManageNeuronRequest,
    ManageNeuronResponse,
};

use crate::clients::{ClientError, GovernanceClient};

pub struct NnsGovernanceCanister {
    canister_id: Principal,
}

impl NnsGovernanceCanister {
    pub fn new(canister_id: Principal) -> Self {
        Self { canister_id }
    }
}

#[async_trait]
impl GovernanceClient for NnsGovernanceCanister {
    async fn claim_or_refresh_neuron_by_subaccount(
        &self,
        subaccount: [u8; 32],
    ) -> Result<(), ClientError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(manage_neuron::NeuronIdOrSubaccount::Subaccount(
                subaccount.to_vec(),
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

        let resp = Call::bounded_wait(self.canister_id, "manage_neuron")
            .with_arg(req)
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("claim_or_refresh call failed: {e:?}")))?;

        let decoded: ManageNeuronResponse = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode claim_or_refresh failed: {e:?}")))?;

        match decoded.command {
            Some(manage_neuron_response::Command::ClaimOrRefresh(_)) => Ok(()),
            Some(manage_neuron_response::Command::Error(e)) => Err(ClientError::Call(format!(
                "claim_or_refresh rejected: type={} message={}",
                e.error_type, e.error_message
            ))),
            other => Err(ClientError::Call(format!(
                "unexpected claim_or_refresh response: {other:?}"
            ))),
        }
    }
}
