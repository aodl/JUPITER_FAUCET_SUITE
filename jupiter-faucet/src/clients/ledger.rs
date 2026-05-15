use async_trait::async_trait;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
pub use jupiter_ic_clients::ledger::IcrcLedgerCanister;

use crate::clients::{ClientError, LedgerClient};

#[async_trait]
impl LedgerClient for IcrcLedgerCanister {
    async fn fee_e8s(&self) -> Result<u64, ClientError> {
        IcrcLedgerCanister::fee_e8s(self).await.map_err(Into::into)
    }

    async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
        IcrcLedgerCanister::balance_of_e8s(self, account).await.map_err(Into::into)
    }

    async fn transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
        IcrcLedgerCanister::transfer(self, arg).await.map_err(Into::into)
    }
}
