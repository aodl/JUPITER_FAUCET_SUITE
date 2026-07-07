use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};

use crate::ClientError;

pub struct IcrcLedgerCanister {
    ledger_id: Principal,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub e8s: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TimeStamp {
    pub timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LegacyTransferArg {
    pub memo: u64,
    pub amount: Tokens,
    pub fee: Tokens,
    pub from_subaccount: Option<[u8; 32]>,
    pub to: Vec<u8>,
    pub created_at_time: Option<TimeStamp>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum LegacyTransferError {
    BadFee { expected_fee: Tokens },
    InsufficientFunds { balance: Tokens },
    TxTooOld { allowed_window_nanos: u64 },
    TxCreatedInFuture,
    TxDuplicate { duplicate_of: u64 },
}

pub type LegacyTransferResult = Result<u64, LegacyTransferError>;

impl IcrcLedgerCanister {
    pub fn new(ledger_id: Principal) -> Self {
        Self { ledger_id }
    }

    pub async fn fee_e8s(&self) -> Result<u64, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "icrc1_fee")
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        let fee_nat: Nat = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode icrc1_fee failed: {e:?}")))?;

        nat_to_u64(&fee_nat)
    }

    pub async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
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

    pub async fn transfer(
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

    pub async fn legacy_transfer_to_account_identifier(
        &self,
        from_subaccount: Option<[u8; 32]>,
        to_account_identifier_hex: String,
        amount_e8s: u64,
        fee_e8s: u64,
        memo: u64,
        created_at_time_nanos: Option<u64>,
    ) -> Result<LegacyTransferResult, ClientError> {
        let to = hex::decode(&to_account_identifier_hex).map_err(|err| {
            ClientError::Convert(format!("invalid ICP account identifier hex: {err}"))
        })?;
        if to.len() != 32 {
            return Err(ClientError::Convert(format!(
                "ICP account identifier must be 32 bytes, got {}",
                to.len()
            )));
        }
        let arg = LegacyTransferArg {
            memo,
            amount: Tokens { e8s: amount_e8s },
            fee: Tokens { e8s: fee_e8s },
            from_subaccount,
            to,
            created_at_time: created_at_time_nanos
                .map(|timestamp_nanos| TimeStamp { timestamp_nanos }),
        };
        let resp = Call::bounded_wait(self.ledger_id, "transfer")
            .with_arg(arg)
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        let res: LegacyTransferResult = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("decode transfer failed: {e:?}")))?;
        Ok(res)
    }
}

fn nat_to_u64(n: &Nat) -> Result<u64, ClientError> {
    u64::try_from(n.0.clone())
        .map_err(|_| ClientError::Convert(format!("Nat does not fit u64: {n}")))
}
