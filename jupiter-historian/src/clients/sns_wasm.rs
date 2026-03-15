use async_trait::async_trait;
use candid::{CandidType, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

use crate::clients::{ClientError, SnsWasmClient};

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
pub struct ListDeployedSnsesRequest {}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct DeployedSns {
    pub root_canister_id: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ListDeployedSnsesResponse {
    pub instances: Vec<DeployedSns>,
}

pub struct SnsWasmCanister {
    canister_id: Principal,
}
impl SnsWasmCanister {
    pub fn new(canister_id: Principal) -> Self { Self { canister_id } }
}

#[async_trait]
impl SnsWasmClient for SnsWasmCanister {
    async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError> {
        let resp = Call::bounded_wait(self.canister_id, "list_deployed_snses")
            .with_arg(ListDeployedSnsesRequest::default())
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        resp.candid().map_err(|e| ClientError::Call(format!("decode list_deployed_snses failed: {e:?}")))
    }
}
