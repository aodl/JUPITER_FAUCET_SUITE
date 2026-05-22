pub(crate) mod blackhole;
pub(crate) mod cmc;
pub(crate) mod governance;
pub(crate) mod ledger;

use async_trait::async_trait;
use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
use jupiter_ic_clients::xrc::XrcCanister;

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
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

pub(crate) type IcpXdrRate = jupiter_ic_clients::xrc::IcpXdrRate;

impl From<jupiter_ic_clients::ClientError> for ClientError {
    fn from(value: jupiter_ic_clients::ClientError) -> Self {
        match value {
            jupiter_ic_clients::ClientError::Call(message) => Self::Call(message),
            jupiter_ic_clients::ClientError::Convert(message) => Self::Convert(message),
        }
    }
}

pub(crate) fn nat_to_u128(n: &Nat) -> Result<u128, ClientError> {
    u128::try_from(n.0.clone())
        .map_err(|_| ClientError::Convert(format!("Nat does not fit u128: {n}")))
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
pub(crate) trait CmcClient: Send + Sync {
    async fn notify_top_up(
        &self,
        canister_id: Principal,
        block_index: u64,
    ) -> Result<u128, ClientError>;
}

#[async_trait]
pub(crate) trait BlackholeClient: Send + Sync {
    async fn cycles_balance(&self, canister_id: Principal) -> Result<u128, ClientError>;
}

#[async_trait]
pub(crate) trait GovernanceClient: Send + Sync {
    async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], ClientError>;
    async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), ClientError>;
}

#[async_trait]
pub(crate) trait ExchangeRateClient: Send + Sync {
    async fn get_icp_xdr_rate(&self) -> Result<IcpXdrRate, ClientError>;
}

#[async_trait]
impl ExchangeRateClient for XrcCanister {
    async fn get_icp_xdr_rate(&self) -> Result<IcpXdrRate, ClientError> {
        Ok(XrcCanister::get_icp_xdr_rate(self).await?)
    }
}
