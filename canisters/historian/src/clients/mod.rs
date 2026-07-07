pub(crate) mod blackhole;
pub(crate) mod governance;
pub(crate) mod index;
pub(crate) mod sns_root;
pub(crate) mod sns_wasm;

use async_trait::async_trait;
use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
use jupiter_ic_clients::xrc::XrcCanister;

use crate::clients::blackhole::BlackholeCanisterStatus;
use crate::clients::index::GetAccountIdentifierTransactionsResponse;
use crate::clients::sns_root::GetSnsCanistersSummaryResponse;
use crate::clients::sns_wasm::ListDeployedSnsesResponse;

pub(crate) type IcpXdrRate = jupiter_ic_clients::xrc::IcpXdrRate;
#[allow(dead_code)]
pub(crate) type IcpXdrConversionRate = jupiter_ic_clients::cmc::IcpXdrConversionRate;
pub(crate) type LegacyTransferResult = jupiter_ic_clients::ledger::LegacyTransferResult;

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
    async fn canister_status(
        &self,
        canister_id: Principal,
    ) -> Result<BlackholeCanisterStatus, ClientError>;
}

#[async_trait]
pub(crate) trait GovernanceClient: Send + Sync {
    async fn claim_or_refresh_neuron_by_subaccount(
        &self,
        subaccount: [u8; 32],
    ) -> Result<(), ClientError>;
}

#[async_trait]
pub(crate) trait SnsWasmClient: Send + Sync {
    async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError>;
}

#[async_trait]
pub(crate) trait SnsRootClient: Send + Sync {
    async fn get_sns_canisters_summary(
        &self,
        root_id: Principal,
    ) -> Result<GetSnsCanistersSummaryResponse, ClientError>;
}

#[async_trait]
pub(crate) trait ExchangeRateClient: Send + Sync {
    async fn get_icp_xdr_rate(&self) -> Result<IcpXdrRate, ClientError>;
}

#[async_trait]
pub(crate) trait LedgerClient: Send + Sync {
    async fn fee_e8s(&self) -> Result<u64, ClientError>;
    async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError>;
    async fn icrc1_transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError>;
    async fn legacy_transfer_to_account_identifier(
        &self,
        from_subaccount: Option<[u8; 32]>,
        to_account_identifier_hex: String,
        amount_e8s: u64,
        fee_e8s: u64,
        memo: u64,
        created_at_time_nanos: Option<u64>,
    ) -> Result<LegacyTransferResult, ClientError>;
}

#[async_trait]
impl LedgerClient for jupiter_ic_clients::ledger::IcrcLedgerCanister {
    async fn fee_e8s(&self) -> Result<u64, ClientError> {
        Ok(jupiter_ic_clients::ledger::IcrcLedgerCanister::fee_e8s(self).await?)
    }

    async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
        Ok(jupiter_ic_clients::ledger::IcrcLedgerCanister::balance_of_e8s(self, account).await?)
    }

    async fn icrc1_transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
        Ok(jupiter_ic_clients::ledger::IcrcLedgerCanister::transfer(self, arg).await?)
    }

    async fn legacy_transfer_to_account_identifier(
        &self,
        from_subaccount: Option<[u8; 32]>,
        to_account_identifier_hex: String,
        amount_e8s: u64,
        fee_e8s: u64,
        memo: u64,
        created_at_time_nanos: Option<u64>,
    ) -> Result<LegacyTransferResult, ClientError> {
        Ok(
            jupiter_ic_clients::ledger::IcrcLedgerCanister::legacy_transfer_to_account_identifier(
                self,
                from_subaccount,
                to_account_identifier_hex,
                amount_e8s,
                fee_e8s,
                memo,
                created_at_time_nanos,
            )
            .await?,
        )
    }
}

#[async_trait]
pub(crate) trait CmcClient: Send + Sync {
    #[allow(dead_code)]
    async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError>;
    async fn notify_top_up(&self, canister_id: Principal, block_index: u64)
        -> Result<u128, String>;
}

pub(crate) struct CmcCanister {
    canister_id: Principal,
}

impl CmcCanister {
    pub(crate) fn new(canister_id: Principal) -> Self {
        Self { canister_id }
    }
}

#[async_trait]
impl CmcClient for CmcCanister {
    async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
        Ok(jupiter_ic_clients::cmc::get_icp_xdr_conversion_rate(self.canister_id).await?)
    }

    async fn notify_top_up(
        &self,
        canister_id: Principal,
        block_index: u64,
    ) -> Result<u128, String> {
        jupiter_ic_clients::cmc::notify_top_up(self.canister_id, canister_id, block_index)
            .await
            .map_err(|err| format!("{err:?}"))
    }
}

#[async_trait]
impl ExchangeRateClient for XrcCanister {
    async fn get_icp_xdr_rate(&self) -> Result<IcpXdrRate, ClientError> {
        Ok(XrcCanister::get_icp_xdr_rate(self).await?)
    }
}
