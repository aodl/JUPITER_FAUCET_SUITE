use async_trait::async_trait;
use candid::{Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
use crate::clients::{ClientError, LedgerClient};

pub struct IcrcLedgerCanister {
    ledger_id: Principal,
}

impl IcrcLedgerCanister {
    pub fn new(ledger_id: Principal) -> Self {
        Self { ledger_id }
    }
}

fn nat_to_u64(n: &Nat) -> Result<u64, ClientError> {
    u64::try_from(n.0.clone())
        .map_err(|_| ClientError::Convert(format!("Nat does not fit u64: {n}")))
}

#[async_trait]
impl LedgerClient for IcrcLedgerCanister {
    async fn fee_e8s(&self) -> Result<u64, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "icrc1_fee")
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        let fee_nat: Nat = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode icrc1_fee failed: {e:?}")))?;

        nat_to_u64(&fee_nat)
    }

    async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "icrc1_balance_of")
            .with_arg(account)
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        let bal_nat: Nat = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode icrc1_balance_of failed: {e:?}")))?;

        nat_to_u64(&bal_nat)
    }

    async fn transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "icrc1_transfer")
            .with_arg(arg)
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        let res: Result<BlockIndex, TransferError> = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode icrc1_transfer failed: {e:?}")))?;

        Ok(res)
    }
}
