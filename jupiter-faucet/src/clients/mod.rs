pub mod canister_info;
pub mod cmc;
pub mod index;
pub mod ledger;

use async_trait::async_trait;
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};

use crate::clients::index::GetAccountIdentifierTransactionsResponse;

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
    #[error("conversion error: {0}")]
    Convert(String),
    #[error("retryable CMC notify error: {0}")]
    RetryableNotify(String),
    #[error("terminal CMC notify error: {0}")]
    TerminalNotify(String),
}

#[async_trait]
pub trait LedgerClient: Send + Sync {
    async fn fee_e8s(&self) -> Result<u64, ClientError>;
    async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError>;
    async fn transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError>;
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
pub trait CmcClient: Send + Sync {
    async fn notify_top_up(&self, canister_id: Principal, block_index: u64) -> Result<(), ClientError>;
}

#[async_trait]
pub trait CanisterStatusClient: Send + Sync {
    async fn canister_exists(&self, canister_id: Principal) -> Result<bool, ClientError>;
}
