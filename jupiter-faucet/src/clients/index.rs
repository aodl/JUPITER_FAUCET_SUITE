use async_trait::async_trait;
use candid::{CandidType, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use serde::Deserialize;
use sha2::{Digest, Sha224};

use crate::clients::{ClientError, IndexClient};

pub fn account_identifier_text(account: &Account) -> String {
    let subaccount = account.subaccount.unwrap_or([0u8; 32]);
    let mut hasher = Sha224::new();
    hasher.update(b"\x0Aaccount-id");
    hasher.update(account.owner.as_slice());
    hasher.update(subaccount);
    let hash = hasher.finalize();
    let checksum = crc32fast::hash(&hash).to_be_bytes();
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&checksum);
    bytes[4..].copy_from_slice(&hash);
    hex::encode(bytes)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTimeStamp { pub timestamp_nanos: u64 }
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Tokens { e8s: u64 }
impl Tokens { pub fn e8s(&self) -> u64 { self.e8s } }

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum IndexOperation {
    Approve { fee: Tokens, from: String, allowance: Tokens, expires_at: Option<IndexTimeStamp>, spender: String, expected_allowance: Option<Tokens> },
    Burn { from: String, amount: Tokens, spender: Option<String> },
    Mint { to: String, amount: Tokens },
    Transfer { to: String, fee: Tokens, from: String, amount: Tokens, spender: Option<String> },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTransaction { pub memo: u64, pub icrc1_memo: Option<Vec<u8>>, pub operation: IndexOperation, pub created_at_time: Option<IndexTimeStamp>, pub timestamp: Option<IndexTimeStamp> }
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTransactionWithId { pub id: u64, pub transaction: IndexTransaction }
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetAccountIdentifierTransactionsArgs { pub max_results: u64, pub start: Option<u64>, pub account_identifier: String }
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetAccountIdentifierTransactionsError { pub message: String }
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetAccountIdentifierTransactionsResponse { pub balance: u64, pub transactions: Vec<IndexTransactionWithId>, pub oldest_tx_id: Option<u64> }
#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum GetAccountIdentifierTransactionsResult { Ok(GetAccountIdentifierTransactionsResponse), Err(GetAccountIdentifierTransactionsError) }

pub struct IcpIndexCanister { index_id: Principal }
impl IcpIndexCanister { pub fn new(index_id: Principal) -> Self { Self { index_id } } }

#[async_trait]
impl IndexClient for IcpIndexCanister {
    async fn get_account_identifier_transactions(&self, account_identifier: String, start: Option<u64>, max_results: u64) -> Result<GetAccountIdentifierTransactionsResponse, ClientError> {
        let args = GetAccountIdentifierTransactionsArgs { max_results, start, account_identifier };
        let resp = Call::bounded_wait(self.index_id, "get_account_identifier_transactions").with_arg(args).change_timeout(60).await.map_err(|e| ClientError::Call(format!("{e:?}")))?;
        let decoded: GetAccountIdentifierTransactionsResult = resp.candid().map_err(|e| ClientError::Call(format!("decode get_account_identifier_transactions failed: {e:?}")))?;
        match decoded { GetAccountIdentifierTransactionsResult::Ok(r) => Ok(r), GetAccountIdentifierTransactionsResult::Err(e) => Err(ClientError::Call(e.message)) }
    }
}
