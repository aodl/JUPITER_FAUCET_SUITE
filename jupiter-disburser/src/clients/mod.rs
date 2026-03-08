pub mod governance;
pub mod ledger;

use async_trait::async_trait;
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};

use crate::nns_types::{GovernanceError, Neuron};

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
    #[error("conversion error: {0}")]
    Convert(String),
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
pub trait GovernanceClient: Send + Sync {
    async fn get_full_neuron(&self, neuron_id: u64) -> Result<Neuron, GovernanceError>;
    async fn disburse_maturity_to_account(
        &self,
        neuron_id: u64,
        percentage: u32,
        to_owner: Principal,
        to_subaccount: Option<Vec<u8>>,
    ) -> Result<Option<u64>, GovernanceError>;

    async fn refresh_voting_power(&self, neuron_id: u64) -> Result<(), GovernanceError>;
}

