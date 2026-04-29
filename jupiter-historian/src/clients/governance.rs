use async_trait::async_trait;
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;

use crate::clients::{ClientError, GovernanceClient};

pub struct NnsGovernanceCanister {
    canister_id: Principal,
}

impl NnsGovernanceCanister {
    pub fn new(canister_id: Principal) -> Self { Self { canister_id } }
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GovernanceError {
    pub error_message: String,
    pub error_type: i32,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Empty {}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum NeuronIdOrSubaccount {
    Subaccount(Vec<u8>),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum By {
    NeuronIdOrSubaccount(Empty),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ClaimOrRefresh {
    pub by: Option<By>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct NeuronId {
    pub id: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ClaimOrRefreshResponse {
    pub refreshed_neuron_id: Option<NeuronId>,
    #[serde(default)]
    pub error: Option<GovernanceError>,
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
pub enum Command1 {
    Error(GovernanceError),
    ClaimOrRefresh(ClaimOrRefreshResponse),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ManageNeuronResponse {
    pub command: Option<Command1>,
}

#[async_trait]
impl GovernanceClient for NnsGovernanceCanister {
    async fn claim_or_refresh_neuron_by_subaccount(&self, subaccount: [u8; 32]) -> Result<(), ClientError> {
        let req = ManageNeuronRequest {
            neuron_id_or_subaccount: Some(NeuronIdOrSubaccount::Subaccount(subaccount.to_vec())),
            command: Some(ManageNeuronCommandRequest::ClaimOrRefresh(ClaimOrRefresh {
                by: Some(By::NeuronIdOrSubaccount(Empty {})),
            })),
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
            Some(Command1::ClaimOrRefresh(resp)) => {
                if let Some(e) = resp.error {
                    Err(ClientError::Call(format!(
                        "claim_or_refresh rejected: type={} message={}",
                        e.error_type, e.error_message
                    )))
                } else {
                    Ok(())
                }
            }
            Some(Command1::Error(e)) => Err(ClientError::Call(format!(
                "claim_or_refresh rejected: type={} message={}",
                e.error_type, e.error_message
            ))),
            other => Err(ClientError::Call(format!(
                "unexpected claim_or_refresh response: {other:?}"
            ))),
        }
    }
}
