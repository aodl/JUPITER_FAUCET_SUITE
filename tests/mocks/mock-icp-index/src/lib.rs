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

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Status {
    pub num_blocks_synced: u64,
}

#[derive(Default)]
struct State {
    // Debug append IDs are one-based. `num_blocks_synced` is a count, so a
    // visible prefix of N includes mock IDs <= N (real Ledger IDs are zero-based).
    next_id: u64,
    txs: Vec<IndexTransactionWithId>,
    get_calls: Vec<DebugGetCall>,
    scripted_get_behaviors: Vec<DebugGetBehavior>,
    num_blocks_synced: Option<u64>,
}

fn visible_block_count(st: &State) -> u64 {
    st.num_blocks_synced.unwrap_or(st.next_id).min(st.next_id)
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

fn account_balance_e8s(txs: &[IndexTransactionWithId], account_identifier: &str) -> u64 {
    txs.iter()
        .fold(0_u64, |balance, tx| match &tx.transaction.operation {
            IndexOperation::Approve { fee, from, .. } if from == account_identifier => {
                balance.saturating_sub(fee.e8s)
            }
            IndexOperation::Burn { from, amount, .. } if from == account_identifier => {
                balance.saturating_sub(amount.e8s)
            }
            IndexOperation::Mint { to, amount } if to == account_identifier => {
                balance.saturating_add(amount.e8s)
            }
            IndexOperation::Transfer {
                to,
                fee,
                from,
                amount,
                ..
            } => {
                let balance = if from == account_identifier {
                    balance.saturating_sub(amount.e8s.saturating_add(fee.e8s))
                } else {
                    balance
                };
                if to == account_identifier {
                    balance.saturating_add(amount.e8s)
                } else {
                    balance
                }
            }
            IndexOperation::TransferFrom {
                to,
                fee,
                from,
                amount,
                ..
            } => {
                let balance = if from == account_identifier {
                    balance.saturating_sub(amount.e8s.saturating_add(fee.e8s))
                } else {
                    balance
                };
                if to == account_identifier {
                    balance.saturating_add(amount.e8s)
                } else {
                    balance
                }
            }
            _ => balance,
        })
}

fn operation_accounts(operation: &IndexOperation, account_identifier: &str) -> bool {
    match operation {
        IndexOperation::Approve { from, spender, .. } => {
            from == account_identifier || spender == account_identifier
        }
        IndexOperation::Burn { from, .. } => from == account_identifier,
        IndexOperation::Mint { to, .. } => to == account_identifier,
        IndexOperation::Transfer { to, from, .. }
        | IndexOperation::TransferFrom { to, from, .. } => {
            to == account_identifier || from == account_identifier
        }
    }
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::query]
fn status() -> Status {
    Status {
        num_blocks_synced: ST.with(|s| {
            let st = s.borrow();
            visible_block_count(&st)
        }),
    }
}

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

        let visible_count = visible_block_count(&st);
        let visible = st
            .txs
            .iter()
            .filter(|tx| tx.id <= visible_count)
            .cloned()
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        for tx in visible.iter().rev().take_while(|_| args.max_results > 0) {
            if args
                .start
                .is_some_and(|exclusive_start| tx.id >= exclusive_start)
            {
                continue;
            }
            if operation_accounts(&tx.transaction.operation, &args.account_identifier) {
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
            balance: account_balance_e8s(&visible, &args.account_identifier),
            oldest_tx_id: st
                .txs
                .iter()
                .filter(|tx| tx.id <= visible_count)
                .filter(|transaction| {
                    operation_accounts(&transaction.transaction.operation, &args.account_identifier)
                })
                .map(|transaction| transaction.id)
                .min(),
            transactions: out,
        })
    })
}

#[ic_cdk::update]
fn debug_reset() {
    ST.with(|s| *s.borrow_mut() = State::default());
}

#[ic_cdk::update]
fn debug_set_num_blocks_synced(num_blocks_synced: Option<u64>) {
    ST.with(|s| s.borrow_mut().num_blocks_synced = num_blocks_synced);
}

#[ic_cdk::update]
fn debug_append_transfer(to: String, amount_e8s: u64, memo: Option<Vec<u8>>) -> u64 {
    debug_append_transfer_from("mock-sender".to_string(), to, amount_e8s, memo)
}

#[ic_cdk::update]
fn debug_append_transfer_from(
    from: String,
    to: String,
    amount_e8s: u64,
    memo: Option<Vec<u8>>,
) -> u64 {
    debug_append_transfer_from_with_timestamp(from, to, amount_e8s, memo, ic_cdk::api::time())
}

#[ic_cdk::update]
fn debug_append_transfer_from_with_timestamp(
    from: String,
    to: String,
    amount_e8s: u64,
    memo: Option<Vec<u8>>,
    timestamp_nanos: u64,
) -> u64 {
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
                    from,
                    amount: Tokens { e8s: amount_e8s },
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        });
        id
    })
}

#[ic_cdk::update]
fn debug_append_transfer_with_numeric_memo(to: String, amount_e8s: u64, memo: u64) -> u64 {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.next_id = st.next_id.saturating_add(1);
        let id = st.next_id;
        st.txs.push(IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to,
                    fee: Tokens { e8s: 10_000 },
                    from: "mock-sender".to_string(),
                    amount: Tokens { e8s: amount_e8s },
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: ic_cdk::api::time(),
                }),
            },
        });
        id
    })
}

#[ic_cdk::update]
fn debug_append_burn(from: String, amount_e8s: u64) -> u64 {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.next_id = st.next_id.saturating_add(1);
        let id = st.next_id;
        st.txs.push(IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Burn {
                    from,
                    amount: Tokens { e8s: amount_e8s },
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: ic_cdk::api::time(),
                }),
            },
        });
        id
    })
}

#[ic_cdk::update]
fn debug_append_repeated_transfer(
    to: String,
    count: u64,
    amount_e8s: u64,
    memo: Option<Vec<u8>>,
) -> u64 {
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
                        timestamp_nanos: ic_cdk::api::time(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer_tx(id: u64, to: &str) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to: to.to_string(),
                    fee: Tokens { e8s: 10_000 },
                    from: "sender".to_string(),
                    amount: Tokens { e8s: 1 },
                    spender: None,
                },
                created_at_time: None,
                timestamp: None,
            },
        }
    }

    fn transaction(id: u64, operation: IndexOperation) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation,
                created_at_time: None,
                timestamp: None,
            },
        }
    }

    fn account_page(
        account: &str,
        start: Option<u64>,
        limit: u64,
    ) -> GetAccountIdentifierTransactionsResponse {
        match get_account_identifier_transactions(GetArgs {
            account_identifier: account.to_string(),
            start,
            max_results: limit,
        }) {
            GetResp::Ok(response) => response,
            GetResp::Err(error) => panic!("unexpected error: {}", error.message),
        }
    }

    #[test]
    fn account_history_is_newest_first_and_start_walks_exclusively_toward_older_history() {
        ST.with(|s| {
            let mut st = s.borrow_mut();
            st.next_id = 3;
            st.txs = vec![
                transfer_tx(1, "target"),
                transfer_tx(2, "target"),
                transfer_tx(3, "target"),
            ];
            st.get_calls.clear();
            st.scripted_get_behaviors.clear();
        });

        let newest = get_account_identifier_transactions(GetArgs {
            account_identifier: "target".to_string(),
            start: None,
            max_results: 2,
        });
        match newest {
            GetResp::Ok(ok) => {
                let ids: Vec<u64> = ok.transactions.into_iter().map(|tx| tx.id).collect();
                assert_eq!(ids, vec![3, 2]);
            }
            GetResp::Err(err) => panic!("unexpected error: {}", err.message),
        }

        let resp = get_account_identifier_transactions(GetArgs {
            account_identifier: "target".to_string(),
            start: Some(2),
            max_results: 10,
        });

        match resp {
            GetResp::Ok(ok) => {
                let ids: Vec<u64> = ok.transactions.into_iter().map(|tx| tx.id).collect();
                assert_eq!(ids, vec![1]);
            }
            GetResp::Err(err) => panic!("unexpected error: {}", err.message),
        }
    }

    #[test]
    fn sparse_numeric_cursor_is_exclusive_even_when_id_is_absent_from_account_history() {
        ST.with(|s| {
            let mut st = s.borrow_mut();
            st.next_id = 13;
            st.txs = vec![
                transfer_tx(2, "target"),
                transfer_tx(5, "other"),
                transfer_tx(9, "target"),
                transfer_tx(13, "target"),
            ];
        });

        assert_eq!(
            account_page("target", None, 10)
                .transactions
                .iter()
                .map(|tx| tx.id)
                .collect::<Vec<_>>(),
            vec![13, 9, 2]
        );
        assert_eq!(
            account_page("target", Some(12), 10)
                .transactions
                .iter()
                .map(|tx| tx.id)
                .collect::<Vec<_>>(),
            vec![9, 2]
        );
        assert!(account_page("target", Some(0), 10).transactions.is_empty());
        assert!(account_page("target", None, 0).transactions.is_empty());
    }

    #[test]
    fn account_history_contains_every_relevant_operation_side_and_keeps_global_oldest_anchor() {
        let target = "target".to_string();
        let other = "other".to_string();
        ST.with(|s| {
            let mut st = s.borrow_mut();
            st.next_id = 8;
            st.txs = vec![
                transaction(
                    1,
                    IndexOperation::Mint {
                        to: target.clone(),
                        amount: Tokens { e8s: 1_000_000 },
                    },
                ),
                transaction(
                    2,
                    IndexOperation::Transfer {
                        to: other.clone(),
                        fee: Tokens { e8s: 10_000 },
                        from: target.clone(),
                        amount: Tokens { e8s: 100_000 },
                        spender: None,
                    },
                ),
                transaction(
                    3,
                    IndexOperation::Transfer {
                        to: target.clone(),
                        fee: Tokens { e8s: 10_000 },
                        from: target.clone(),
                        amount: Tokens { e8s: 50_000 },
                        spender: None,
                    },
                ),
                transaction(
                    4,
                    IndexOperation::Approve {
                        fee: Tokens { e8s: 10_000 },
                        from: target.clone(),
                        allowance: Tokens { e8s: 200_000 },
                        expires_at: None,
                        spender: other.clone(),
                        expected_allowance: None,
                    },
                ),
                transaction(
                    5,
                    IndexOperation::Burn {
                        from: target.clone(),
                        amount: Tokens { e8s: 20_000 },
                        spender: Some(other.clone()),
                    },
                ),
                transaction(
                    6,
                    IndexOperation::TransferFrom {
                        to: other.clone(),
                        fee: Tokens { e8s: 10_000 },
                        from: target.clone(),
                        amount: Tokens { e8s: 30_000 },
                        spender: other.clone(),
                    },
                ),
                transfer_tx(8, "unrelated"),
            ];
        });

        let newest_two = account_page(&target, None, 2);
        assert_eq!(
            newest_two
                .transactions
                .iter()
                .map(|tx| tx.id)
                .collect::<Vec<_>>(),
            vec![6, 5]
        );
        assert_eq!(newest_two.oldest_tx_id, Some(1));
        assert_eq!(newest_two.balance, 810_000);
        assert_eq!(
            account_page(&other, None, 20)
                .transactions
                .iter()
                .map(|tx| tx.id)
                .collect::<Vec<_>>(),
            vec![6, 4, 2]
        );
        assert_eq!(account_page("empty", None, 20).oldest_tx_id, None);
    }

    #[test]
    fn status_defaults_to_indexed_prefix_and_explicit_lag_is_observable() {
        ST.with(|s| {
            let mut st = s.borrow_mut();
            st.next_id = 17;
            st.num_blocks_synced = None;
        });
        assert_eq!(status().num_blocks_synced, 17);
        debug_set_num_blocks_synced(Some(11));
        assert_eq!(status().num_blocks_synced, 11);
    }

    #[test]
    fn indexed_prefix_controls_history_balance_and_oldest_across_accounts() {
        ST.with(|s| {
            let mut st = s.borrow_mut();
            *st = State::default();
            st.next_id = 6;
            st.txs = vec![
                transaction(
                    1,
                    IndexOperation::Mint {
                        to: "alice".into(),
                        amount: Tokens { e8s: 1_000_000 },
                    },
                ),
                transaction(
                    2,
                    IndexOperation::Mint {
                        to: "bob".into(),
                        amount: Tokens { e8s: 500_000 },
                    },
                ),
                transaction(
                    4,
                    IndexOperation::Transfer {
                        from: "alice".into(),
                        to: "bob".into(),
                        amount: Tokens { e8s: 100_000 },
                        fee: Tokens { e8s: 10_000 },
                        spender: None,
                    },
                ),
                transaction(
                    6,
                    IndexOperation::Transfer {
                        from: "alice".into(),
                        to: "alice".into(),
                        amount: Tokens { e8s: 50_000 },
                        fee: Tokens { e8s: 10_000 },
                        spender: None,
                    },
                ),
            ];
            st.num_blocks_synced = Some(0);
        });
        let ids = |account: &str, start: Option<u64>| {
            account_page(account, start, 10)
                .transactions
                .into_iter()
                .map(|tx| tx.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(status().num_blocks_synced, 0);
        assert_eq!(ids("alice", None), Vec::<u64>::new());
        assert_eq!(account_page("alice", None, 10).balance, 0);
        assert_eq!(account_page("alice", None, 10).oldest_tx_id, None);
        debug_set_num_blocks_synced(Some(2));
        assert_eq!(ids("alice", None), vec![1]);
        assert_eq!(ids("bob", None), vec![2]);
        assert_eq!(account_page("alice", None, 10).balance, 1_000_000);
        debug_set_num_blocks_synced(Some(4));
        assert_eq!(ids("alice", None), vec![4, 1]);
        assert_eq!(ids("bob", None), vec![4, 2]);
        assert_eq!(ids("alice", Some(3)), vec![1]);
        assert_eq!(account_page("alice", None, 1).oldest_tx_id, Some(1));
        assert_eq!(account_page("alice", None, 10).balance, 890_000);
        assert_eq!(account_page("bob", None, 10).balance, 600_000);
        debug_set_num_blocks_synced(Some(6));
        assert_eq!(ids("alice", None), vec![6, 4, 1]);
        assert_eq!(account_page("alice", None, 10).balance, 880_000);
        assert_eq!(account_page("alice", None, 10).oldest_tx_id, Some(1));
    }
}
