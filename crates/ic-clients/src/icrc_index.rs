use candid::{CandidType, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc3::transactions::Transaction;
use serde::Deserialize;

use crate::ClientError;

/// Wire types from the committed ICRC index-ng `get_account_transactions` interface.
#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct GetAccountTransactionsArgs {
    pub account: Account,
    pub start: Option<Nat>,
    pub max_results: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct TransactionWithId {
    pub id: Nat,
    pub transaction: Transaction,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct GetAccountTransactionsResponse {
    pub balance: Nat,
    pub transactions: Vec<TransactionWithId>,
    pub oldest_tx_id: Option<Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct GetAccountTransactionsError {
    pub message: String,
}

pub type GetAccountTransactionsResult =
    Result<GetAccountTransactionsResponse, GetAccountTransactionsError>;

pub struct IcrcIndexCanister {
    index_id: Principal,
}

impl IcrcIndexCanister {
    pub fn new(index_id: Principal) -> Self {
        Self { index_id }
    }

    pub async fn ledger_id(&self) -> Result<Principal, ClientError> {
        let response = Call::bounded_wait(self.index_id, "ledger_id")
            .change_timeout(60)
            .await
            .map_err(|error| ClientError::Call(format!("ledger_id failed: {error:?}")))?;
        response
            .candid()
            .map_err(|error| ClientError::Call(format!("decode ledger_id failed: {error:?}")))
    }

    pub async fn get_account_transactions(
        &self,
        account: Account,
        start: Option<Nat>,
        max_results: Nat,
    ) -> Result<GetAccountTransactionsResponse, ClientError> {
        let response = Call::bounded_wait(self.index_id, "get_account_transactions")
            .with_arg(GetAccountTransactionsArgs {
                account,
                start,
                max_results,
            })
            .change_timeout(60)
            .await
            .map_err(|error| {
                ClientError::Call(format!("get_account_transactions failed: {error:?}"))
            })?;
        let result: GetAccountTransactionsResult = response.candid().map_err(|error| {
            ClientError::Call(format!("decode get_account_transactions failed: {error:?}"))
        })?;
        result.map_err(|error| ClientError::Call(error.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{Decode, Encode};
    use icrc_ledger_types::icrc3::transactions::Mint;

    #[test]
    fn committed_index_wire_shape_decodes_transaction_block_time() {
        let account = Account {
            owner: Principal::from_slice(&[1]),
            subaccount: None,
        };
        let encoded = Encode!(&GetAccountTransactionsResult::Ok(
            GetAccountTransactionsResponse {
                balance: Nat::from(100_u64),
                transactions: vec![TransactionWithId {
                    id: Nat::from(42_u64),
                    transaction: Transaction::mint(
                        Mint {
                            amount: Nat::from(100_u64),
                            to: account,
                            memo: None,
                            created_at_time: Some(7),
                            fee: None,
                        },
                        99,
                    ),
                }],
                oldest_tx_id: Some(Nat::from(42_u64)),
            }
        ))
        .unwrap();

        let decoded = Decode!(&encoded, GetAccountTransactionsResult).unwrap();
        let page = decoded.unwrap();
        assert_eq!(page.transactions[0].transaction.timestamp, 99);
        assert_eq!(
            page.transactions[0]
                .transaction
                .mint
                .as_ref()
                .unwrap()
                .created_at_time,
            Some(7)
        );
    }
}
