pub(crate) mod canister_info;
pub(crate) mod cmc;
pub(crate) mod governance;
pub(crate) mod index;
pub(crate) mod ledger;

use async_trait::async_trait;
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};

use crate::clients::index::GetAccountIdentifierTransactionsResponse;

#[derive(thiserror::Error, Debug)]
pub(crate) enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
    #[error("conversion error: {0}")]
    Convert(String),
    #[error("retryable CMC notify error: {0}")]
    RetryableNotify(String),
    #[error("terminal CMC notify error: {0}")]
    TerminalNotify(String),
}

impl From<jupiter_ic_clients::ClientError> for ClientError {
    fn from(value: jupiter_ic_clients::ClientError) -> Self {
        match value {
            jupiter_ic_clients::ClientError::Call(message) => Self::Call(message),
            jupiter_ic_clients::ClientError::Convert(message) => Self::Convert(message),
        }
    }
}

#[async_trait]
pub(crate) trait LedgerClient: Send + Sync {
    async fn fee_e8s(&self) -> Result<u64, ClientError>;
    async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError>;
    async fn transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError>;
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
pub(crate) trait CmcClient: Send + Sync {
    async fn notify_top_up(&self, canister_id: Principal, block_index: u64) -> Result<(), ClientError>;
}

#[async_trait]
pub(crate) trait CanisterStatusClient: Send + Sync {
    async fn canister_exists(&self, canister_id: Principal) -> Result<bool, ClientError>;
}

#[async_trait]
pub(crate) trait GovernanceClient: Send + Sync {
    async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], ClientError>;
    async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), ClientError>;
}
