pub(crate) mod blackhole;
pub(crate) mod governance;
pub(crate) mod index;
pub(crate) mod sns_root;
pub(crate) mod sns_wasm;
pub(crate) mod xrc;

use async_trait::async_trait;
use candid::Principal;

use crate::clients::blackhole::BlackholeCanisterStatus;
use crate::clients::index::GetAccountIdentifierTransactionsResponse;
use crate::clients::sns_root::GetSnsCanistersSummaryResponse;
use crate::clients::sns_wasm::ListDeployedSnsesResponse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IcpXdrRate {
    pub rate: u64,
    pub decimals: u32,
    pub timestamp: u64,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
}

impl From<jupiter_ic_clients::ClientError> for ClientError {
    fn from(value: jupiter_ic_clients::ClientError) -> Self {
        match value {
            jupiter_ic_clients::ClientError::Call(message) => Self::Call(message),
            jupiter_ic_clients::ClientError::Convert(message) => Self::Call(message),
        }
    }
}

#[async_trait]
pub(crate) trait IndexClient: Send + Sync {
    async fn get_account_identifier_transactions(
        &self,
        account_identifier: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, ClientError>;
}

#[async_trait]
pub(crate) trait BlackholeClient: Send + Sync {
    async fn canister_status(&self, canister_id: Principal) -> Result<BlackholeCanisterStatus, ClientError>;
}

#[async_trait]
pub(crate) trait GovernanceClient: Send + Sync {
    async fn claim_or_refresh_neuron_by_subaccount(&self, subaccount: [u8; 32]) -> Result<(), ClientError>;
}

#[async_trait]
pub(crate) trait SnsWasmClient: Send + Sync {
    async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError>;
}

#[async_trait]
pub(crate) trait SnsRootClient: Send + Sync {
    async fn get_sns_canisters_summary(&self, root_id: Principal) -> Result<GetSnsCanistersSummaryResponse, ClientError>;
}

#[async_trait]
pub(crate) trait ExchangeRateClient: Send + Sync {
    async fn get_icp_xdr_rate(&self) -> Result<IcpXdrRate, ClientError>;
}
