use candid::{CandidType, Deserialize};
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct IndexTimeStamp {
    pub timestamp_nanos: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Tokens {
    pub e8s: u64,
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
pub struct GetArgs {
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

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct DebugGetCall {
    pub account_identifier: String,
    pub start: Option<u64>,
    pub max_results: u64,
    pub returned_count: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum DebugGetBehavior {
    Ok,
    Err(String),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum GetResp {
    Ok(GetAccountIdentifierTransactionsResponse),
    Err(GetAccountIdentifierTransactionsError),
}

#[derive(Default)]
struct State {
    next_id: u64,
    txs: Vec<IndexTransactionWithId>,
    get_calls: Vec<DebugGetCall>,
    scripted_get_behaviors: Vec<DebugGetBehavior>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn get_account_identifier_transactions(args: GetArgs) -> GetResp {
    ST.with(|s| {
        let mut st = s.borrow_mut();

        let behavior = if st.scripted_get_behaviors.is_empty() {
            None
        } else {
            Some(st.scripted_get_behaviors.remove(0))
        };

        if let Some(DebugGetBehavior::Err(message)) = behavior {
            st.get_calls.push(DebugGetCall {
                account_identifier: args.account_identifier.clone(),
                start: args.start,
                max_results: args.max_results,
                returned_count: 0,
            });
            return GetResp::Err(GetAccountIdentifierTransactionsError { message });
        }

        let start_idx = match args.start {
            None => 0,
            Some(last_seen) => st
                .txs
                .iter()
                .position(|t| t.id == last_seen)
                .map(|i| i + 1)
                .unwrap_or(st.txs.len()),
        };

        let mut out = Vec::new();
        for tx in st.txs[start_idx..].iter() {
            let include = matches!(
                &tx.transaction.operation,
                IndexOperation::Transfer { to, .. } if to == &args.account_identifier
            );
            if include {
                out.push(tx.clone());
            }
            if out.len() >= args.max_results as usize {
                break;
            }
        }

        st.get_calls.push(DebugGetCall {
            account_identifier: args.account_identifier.clone(),
            start: args.start,
            max_results: args.max_results,
            returned_count: out.len() as u64,
        });

        GetResp::Ok(GetAccountIdentifierTransactionsResponse {
            balance: 0,
            oldest_tx_id: st.txs.first().map(|t| t.id),
            transactions: out,
        })
    })
}

#[ic_cdk::update]
fn debug_reset() {
    ST.with(|s| *s.borrow_mut() = State::default());
}

#[ic_cdk::update]
fn debug_append_transfer(to: String, amount_e8s: u64, memo: Option<Vec<u8>>) -> u64 {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.next_id = st.next_id.saturating_add(1);
        let id = st.next_id;
        st.txs.push(IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: memo,
                operation: IndexOperation::Transfer {
                    to,
                    fee: Tokens { e8s: 10_000 },
                    from: "mock-sender".to_string(),
                    amount: Tokens { e8s: amount_e8s },
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: ic_cdk::api::time() as u64,
                }),
            },
        });
        id
    })
}

#[ic_cdk::update]
fn debug_append_repeated_transfer(to: String, count: u64, amount_e8s: u64, memo: Option<Vec<u8>>) -> u64 {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        let mut last_id = 0;
        for _ in 0..count {
            st.next_id = st.next_id.saturating_add(1);
            last_id = st.next_id;
            st.txs.push(IndexTransactionWithId {
                id: last_id,
                transaction: IndexTransaction {
                    memo: 0,
                    icrc1_memo: memo.clone(),
                    operation: IndexOperation::Transfer {
                        to: to.clone(),
                        fee: Tokens { e8s: 10_000 },
                        from: "mock-sender".to_string(),
                        amount: Tokens { e8s: amount_e8s },
                        spender: None,
                    },
                    created_at_time: None,
                    timestamp: Some(IndexTimeStamp {
                        timestamp_nanos: ic_cdk::api::time() as u64,
                    }),
                },
            });
        }
        last_id
    })
}

#[ic_cdk::query]
fn debug_get_calls() -> Vec<DebugGetCall> {
    ST.with(|s| s.borrow().get_calls.clone())
}

#[ic_cdk::update]
fn debug_set_get_script(behaviors: Vec<DebugGetBehavior>) {
    ST.with(|s| s.borrow_mut().scripted_get_behaviors = behaviors);
}

ic_cdk::export_candid!();
