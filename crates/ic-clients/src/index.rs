use candid::{CandidType, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

use crate::ClientError;

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTimeStamp {
    pub timestamp_nanos: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Tokens {
    e8s: u64,
}
impl Tokens {
    pub fn e8s(&self) -> u64 {
        self.e8s
    }

    pub fn new(e8s: u64) -> Self {
        Self { e8s }
    }
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum IndexOperation {
    Approve {
        fee: Tokens,
        from: String,
        allowance: Tokens,
        expires_at: Option<IndexTimeStamp>,
        spender: String,
        expected_allowance: Option<Tokens>,
    },
    Burn {
        from: String,
        amount: Tokens,
        spender: Option<String>,
    },
    Mint {
        to: String,
        amount: Tokens,
    },
    Transfer {
        to: String,
        fee: Tokens,
        from: String,
        amount: Tokens,
        spender: Option<String>,
    },
    TransferFrom {
        to: String,
        fee: Tokens,
        from: String,
        amount: Tokens,
        spender: String,
    },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTransaction {
    pub memo: u64,
    pub icrc1_memo: Option<Vec<u8>>,
    pub operation: IndexOperation,
    pub created_at_time: Option<IndexTimeStamp>,
    pub timestamp: Option<IndexTimeStamp>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTransactionWithId {
    pub id: u64,
    pub transaction: IndexTransaction,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetAccountIdentifierTransactionsArgs {
    pub max_results: u64,
    pub start: Option<u64>,
    pub account_identifier: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetAccountIdentifierTransactionsError {
    pub message: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetAccountIdentifierTransactionsResponse {
    pub balance: u64,
    pub transactions: Vec<IndexTransactionWithId>,
    pub oldest_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct IndexStatus {
    pub num_blocks_synced: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum GetAccountIdentifierTransactionsResult {
    Ok(GetAccountIdentifierTransactionsResponse),
    Err(GetAccountIdentifierTransactionsError),
}

pub struct IcpIndexCanister {
    index_id: Principal,
}
impl IcpIndexCanister {
    pub fn new(index_id: Principal) -> Self {
        Self { index_id }
    }

    pub async fn get_account_identifier_transactions(
        &self,
        account_identifier: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, ClientError> {
        let args = GetAccountIdentifierTransactionsArgs {
            max_results,
            start,
            account_identifier,
        };
        let resp = Call::bounded_wait(self.index_id, "get_account_identifier_transactions")
            .with_arg(args)
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        let decoded: GetAccountIdentifierTransactionsResult = resp.candid().map_err(|e| {
            ClientError::Call(format!(
                "decode get_account_identifier_transactions failed: {e:?}"
            ))
        })?;
        match decoded {
            GetAccountIdentifierTransactionsResult::Ok(r) => Ok(r),
            GetAccountIdentifierTransactionsResult::Err(e) => Err(ClientError::Call(e.message)),
        }
    }

    pub async fn status(&self) -> Result<IndexStatus, ClientError> {
        let resp = Call::bounded_wait(self.index_id, "status")
            .change_timeout(20)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        resp.candid()
            .map_err(|e| ClientError::Call(format!("decode status failed: {e:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_legacy_transfer_from_variant_roundtrips() {
        let encoded = candid::encode_one(GetAccountIdentifierTransactionsResult::Ok(
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(41),
                transactions: vec![IndexTransactionWithId {
                    id: 42,
                    transaction: IndexTransaction {
                        memo: 0,
                        icrc1_memo: None,
                        operation: IndexOperation::TransferFrom {
                            to: "to-account".to_string(),
                            fee: Tokens::new(10_000),
                            from: "from-account".to_string(),
                            amount: Tokens::new(123_456),
                            spender: "spender-account".to_string(),
                        },
                        created_at_time: Some(IndexTimeStamp {
                            timestamp_nanos: 123,
                        }),
                        timestamp: Some(IndexTimeStamp {
                            timestamp_nanos: 456,
                        }),
                    },
                }],
            },
        ))
        .expect("encoding should succeed");

        let decoded: GetAccountIdentifierTransactionsResult =
            candid::decode_one(&encoded).expect("decoding should succeed");
        match decoded {
            GetAccountIdentifierTransactionsResult::Ok(resp) => {
                match &resp.transactions[0].transaction.operation {
                    IndexOperation::TransferFrom {
                        spender, amount, ..
                    } => {
                        assert_eq!(spender, "spender-account");
                        assert_eq!(amount.e8s(), 123_456);
                    }
                    other => panic!("expected TransferFrom, got {other:?}"),
                }
            }
            other => panic!("expected Ok result, got {other:?}"),
        }
    }

    fn decode_hex_fixture(text: &str) -> Vec<u8> {
        let compact: String = text.split_whitespace().collect();
        assert_eq!(compact.len() % 2, 0, "hex fixture must contain byte pairs");
        compact
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).expect("ASCII hex fixture");
                u8::from_str_radix(text, 16).expect("valid hex fixture")
            })
            .collect()
    }

    #[test]
    fn pinned_real_index_delegated_transfer_decodes_as_transfer_with_spender() {
        // Frozen reply from the pinned real local ICP Index: a delegated transfer
        // is encoded as Transfer with spender populated.
        let bytes = decode_hex_fixture(include_str!(
            "../tests/fixtures/icp-index-delegated-transfer-response.hex"
        ));
        let decoded: GetAccountIdentifierTransactionsResult =
            candid::decode_one(&bytes).expect("decode pinned real Index response");
        let GetAccountIdentifierTransactionsResult::Ok(response) = decoded else {
            panic!("expected successful real Index response");
        };
        assert_eq!(response.transactions.len(), 2);
        let delegated = &response.transactions[0];
        assert_eq!(delegated.id, 4);
        match &delegated.transaction.operation {
            IndexOperation::Transfer {
                from,
                to,
                spender: Some(spender),
                amount,
                fee,
            } => {
                assert_eq!(
                    from,
                    "1c7a48ba6a562aa9eaa2481a9049cdf0433b9738c992d698c31d8abf89cadc79"
                );
                assert_eq!(
                    to,
                    "6c19589384c6767f4a5632dd884c4b94239a23d71cf4196acb3cf42d99048495"
                );
                assert_eq!(
                    spender,
                    "82a867f8955a6516fb7de0ce1bf4ee258fb6bf0ccef09e650e1c918860556726"
                );
                assert_eq!(amount.e8s(), 20_000_000);
                assert_eq!(fee.e8s(), 10_000);
            }
            other => panic!("expected delegated Transfer with spender, got {other:?}"),
        }
    }
}
