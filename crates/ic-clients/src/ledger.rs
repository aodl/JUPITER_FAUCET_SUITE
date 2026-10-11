use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};

use crate::ClientError;

pub struct IcrcLedgerCanister {
    ledger_id: Principal,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GetBlocksArgs {
    pub start: u64,
    pub length: u64,
}

/// The `query_blocks` response is a Candid record. Relay only needs the chain length, so Candid
/// record subtyping deliberately omits the block/archive payloads from this narrow client DTO.
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct QueryBlocksChainLength {
    pub chain_length: u64,
}

impl IcrcLedgerCanister {
    pub fn new(ledger_id: Principal) -> Self {
        Self { ledger_id }
    }

    pub async fn fee_e8s(&self) -> Result<u64, ClientError> {
        nat_to_u64(&self.fee().await?)
    }

    pub async fn chain_length(&self) -> Result<u64, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "query_blocks")
            .with_arg(GetBlocksArgs {
                start: 0,
                length: 0,
            })
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        let response: QueryBlocksChainLength = resp.candid().map_err(|e| {
            ClientError::Call(format!("decode query_blocks chain length failed: {e:?}"))
        })?;
        Ok(response.chain_length)
    }

    pub async fn fee(&self) -> Result<Nat, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "icrc1_fee")
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        resp.candid()
            .map_err(|e| ClientError::Call(format!("decode icrc1_fee failed: {e:?}")))
    }

    pub async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
        nat_to_u64(&self.balance_of(account).await?)
    }

    pub async fn balance_of(&self, account: Account) -> Result<Nat, ClientError> {
        let resp = Call::bounded_wait(self.ledger_id, "icrc1_balance_of")
            .with_arg(account)
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;

        resp.candid()
            .map_err(|e| ClientError::Call(format!("decode icrc1_balance_of failed: {e:?}")))
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
}

fn nat_to_u64(n: &Nat) -> Result<u64, ClientError> {
    u64::try_from(n.0.clone())
        .map_err(|_| ClientError::Convert(format!("Nat does not fit u64: {n}")))
}
