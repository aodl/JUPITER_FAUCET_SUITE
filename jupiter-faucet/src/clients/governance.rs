use async_trait::async_trait;
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;
use serde_bytes::ByteBuf;

use crate::clients::{ClientError, GovernanceClient};

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GovernanceError {
    pub error_type: i32,
    pub error_message: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Neuron {
    pub id: Option<NeuronId>,
    pub account: ByteBuf,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct NeuronId {
    pub id: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct NeuronSubaccount {
    pub subaccount: ByteBuf,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ListNeurons {
    pub neuron_ids: Vec<u64>,
    pub include_neurons_readable_by_caller: bool,
    pub include_empty_neurons_readable_by_caller: Option<bool>,
    pub include_public_neurons_in_full_neurons: Option<bool>,
    pub page_number: Option<u64>,
    pub page_size: Option<u64>,
    pub neuron_subaccounts: Option<Vec<NeuronSubaccount>>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ListNeuronsResponse {
    pub full_neurons: Vec<Neuron>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Empty {}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum By {
    NeuronIdOrSubaccount(Empty),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ClaimOrRefresh {
    pub by: Option<By>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum NeuronIdOrSubaccount {
    NeuronId(NeuronId),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum ManageNeuronCommandRequest {
    ClaimOrRefresh(ClaimOrRefresh),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ManageNeuronRequest {
    pub neuron_id_or_subaccount: Option<NeuronIdOrSubaccount>,
    pub command: Option<ManageNeuronCommandRequest>,
    pub id: Option<NeuronId>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ClaimOrRefreshResponse {
    pub refreshed_neuron_id: Option<NeuronId>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum ManageNeuronCommandResponse {
    Error(GovernanceError),
    ClaimOrRefresh(ClaimOrRefreshResponse),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ManageNeuronResponse {
    pub command: Option<ManageNeuronCommandResponse>,
}

pub struct NnsGovernanceCanister {
    governance_id: Principal,
}

impl NnsGovernanceCanister {
    pub fn new(governance_id: Principal) -> Self {
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
            neuron_subaccounts: None,
        };
        let resp = Call::bounded_wait(self.governance_id, "list_neurons")
            .with_arg(req)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        let decoded: ListNeuronsResponse = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode list_neurons failed: {e:?}")))?;
        let neuron = decoded
            .full_neurons
            .into_iter()
            .find(|neuron| neuron.id.as_ref().map(|id| id.id) == Some(neuron_id))
            .ok_or_else(|| ClientError::Call(format!("list_neurons returned no public full neuron for {neuron_id}")))?;
        let account = neuron.account.as_ref();
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
            neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::NeuronId(NeuronId { id: neuron_id })),
            command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(ClaimOrRefresh {
                by: Some(By::NeuronIdOrSubaccount(Empty {})),
            })),
        };
        let resp = Call::bounded_wait(self.governance_id, "manage_neuron")
            .with_arg(req)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        let decoded: ManageNeuronResponse = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode manage_neuron ClaimOrRefresh failed: {e:?}")))?;
        match decoded.command {
            Some(ManageNeuronCommandResponse::ClaimOrRefresh(_)) => Ok(()),
            Some(ManageNeuronCommandResponse::Error(err)) => Err(ClientError::Call(format!(
                "claim_or_refresh failed: type={} message={}",
                err.error_type, err.error_message
            ))),
            None => Err(ClientError::Call("claim_or_refresh returned no command".to_string())),
        }
    }
}
