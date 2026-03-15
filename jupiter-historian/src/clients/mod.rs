pub mod blackhole;
pub mod index;
pub mod sns_root;
pub mod sns_wasm;

use async_trait::async_trait;
use candid::Principal;

use crate::clients::blackhole::BlackholeCanisterStatus;
use crate::clients::index::GetAccountIdentifierTransactionsResponse;
use crate::clients::sns_root::GetSnsCanistersSummaryResponse;
use crate::clients::sns_wasm::ListDeployedSnsesResponse;

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
}

#[async_trait]
pub trait IndexClient: Send + Sync {
    async fn get_account_identifier_transactions(
        &self,
        account_identifier: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, ClientError>;
}

#[async_trait]
pub trait BlackholeClient: Send + Sync {
    async fn canister_status(&self, canister_id: Principal) -> Result<BlackholeCanisterStatus, ClientError>;
}

#[async_trait]
pub trait SnsWasmClient: Send + Sync {
    async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError>;
}

#[async_trait]
pub trait SnsRootClient: Send + Sync {
    async fn get_sns_canisters_summary(&self, root_id: Principal) -> Result<GetSnsCanistersSummaryResponse, ClientError>;
}
