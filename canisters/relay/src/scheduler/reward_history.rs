use std::collections::BTreeSet;

use super::reward_budget::WorkBudget;
use async_trait::async_trait;
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsResponse, IcpIndexCanister, IndexOperation,
    IndexTransactionWithId,
};

pub(super) const ICP_HISTORY_PAGE_SIZE: u64 = 1_000;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HistoricalReconstruction<T> {
    Complete(T),
    NeedOlderHistory,
    Malformed(String),
}

#[cfg(test)]
impl<T> HistoricalReconstruction<T> {
    pub(super) fn unwrap_complete(self) -> T {
        match self {
            Self::Complete(value) => value,
            Self::NeedOlderHistory => {
                panic!("expected complete reconstruction, needs older history")
            }
            Self::Malformed(error) => panic!("expected complete reconstruction, got {error}"),
        }
    }

    pub(super) fn unwrap_malformed(self) -> String {
        match self {
            Self::Malformed(error) => error,
            Self::NeedOlderHistory => {
                panic!("expected malformed reconstruction, needs older history")
            }
            Self::Complete(_) => panic!("expected malformed reconstruction, got complete"),
        }
    }
}

#[async_trait]
pub(super) trait RewardHistoryClient: Send + Sync {
    async fn get_transactions(
        &self,
        account_identifier: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, String>;
}

#[async_trait]
impl RewardHistoryClient for IcpIndexCanister {
    async fn get_transactions(
        &self,
        account_identifier: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, String> {
        self.get_account_identifier_transactions(account_identifier, start, max_results)
            .await
            .map_err(|_| "history_read_failed".to_string())
    }
}

/// Heap-only state for one backwards account-history read. It is deliberately discarded after
/// one reward adjudication and is not an attribution cursor.
#[derive(Debug)]
pub(super) struct BackwardHistory {
    budget: WorkBudget,
    account_identifier: String,
    next_start: Option<u64>,
    seen_starts: BTreeSet<u64>,
    transactions: Vec<IndexTransactionWithId>,
    previous_tx_id: Option<u64>,
    current_balance_e8s: Option<u64>,
    balance_before_oldest_e8s: Option<u64>,
    authoritative_len: usize,
    exhausted: bool,
    #[cfg(test)]
    pages_requested: usize,
}

impl BackwardHistory {
    #[cfg(test)]
    pub(super) fn new(account_identifier: String) -> Self {
        Self::with_budget(account_identifier, WorkBudget::for_test())
    }

    pub(super) fn with_budget(account_identifier: String, budget: WorkBudget) -> Self {
        Self {
            budget,
            account_identifier,
            next_start: None,
            seen_starts: BTreeSet::new(),
            transactions: Vec::new(),
            previous_tx_id: None,
            current_balance_e8s: None,
            balance_before_oldest_e8s: None,
            authoritative_len: 0,
            exhausted: false,
            #[cfg(test)]
            pages_requested: 0,
        }
    }

    pub(super) fn transactions(&self) -> &[IndexTransactionWithId] {
        &self.transactions
    }

    pub(super) fn exhausted(&self) -> bool {
        self.exhausted
    }

    /// The newest collected suffix whose opening balance is proven zero. Reaching genuine
    /// account genesis proves that no older transactions exist, but the reconstructed opening
    /// balance must still be zero.
    pub(super) fn authoritative_transactions(&self) -> &[IndexTransactionWithId] {
        &self.transactions[..self.authoritative_len]
    }

    pub(super) fn authoritative(&self) -> bool {
        self.authoritative_len > 0 || (self.exhausted && self.transactions.is_empty())
    }

    #[cfg(test)]
    pub(super) fn pages_requested(&self) -> usize {
        self.pages_requested
    }

    pub(super) async fn extend<I: RewardHistoryClient>(&mut self, index: &I) -> Result<(), String> {
        if self.exhausted {
            return Ok(());
        }
        let requested_start = self.next_start;
        let page = self
            .budget
            .call(index.get_transactions(
                self.account_identifier.clone(),
                requested_start,
                ICP_HISTORY_PAGE_SIZE,
            ))
            .await?
            .map_err(|_| "history_read_failed".to_string())?;
        if page.transactions.len() > ICP_HISTORY_PAGE_SIZE as usize {
            return Err("history_oversized_page".into());
        }
        self.budget.retain(
            page.transactions.len(),
            candid::encode_one(&page)
                .map_err(|_| "history_encoding_failed".to_string())?
                .len(),
        )?;
        #[cfg(test)]
        {
            self.pages_requested += 1;
        }

        match self.current_balance_e8s {
            Some(balance) if balance != page.balance => {
                return Err("history_balance_changed_during_pagination".to_string())
            }
            None => {
                self.current_balance_e8s = Some(page.balance);
                self.balance_before_oldest_e8s = Some(page.balance);
            }
            Some(_) => {}
        }

        if page.transactions.is_empty() {
            self.require_zero_opening_balance()?;
            self.exhausted = true;
            self.authoritative_len = self.transactions.len();
            return Ok(());
        }

        for transaction in &page.transactions {
            self.budget.work(1)?;
            if requested_start.is_some_and(|start| transaction.id >= start)
                || self
                    .previous_tx_id
                    .is_some_and(|previous| transaction.id >= previous)
            {
                return Err("history_pagination_non_progressing".to_string());
            }
            self.previous_tx_id = Some(transaction.id);
            let balance_after = self
                .balance_before_oldest_e8s
                .expect("balance initialized before page replay");
            let balance_before =
                invert_account_transaction(&self.account_identifier, balance_after, transaction)?;
            self.balance_before_oldest_e8s = Some(balance_before);
            self.transactions.push(transaction.clone());
            if balance_before == 0 {
                self.authoritative_len = self.transactions.len();
            }
        }

        let oldest_in_page = page.transactions.last().expect("nonempty page").id;
        if page
            .oldest_tx_id
            .is_some_and(|oldest| oldest > oldest_in_page)
        {
            return Err("history_pagination_inconsistent_oldest".to_string());
        }
        // A short nonempty page is not proof of genesis: the Index may legally return fewer
        // transactions than requested. Only an empty response or its documented oldest ID proves
        // that no older transactions exist, and the reconstructed opening balance must still be
        // zero.
        if page.oldest_tx_id == Some(oldest_in_page) {
            self.require_zero_opening_balance()?;
            self.exhausted = true;
            self.authoritative_len = self.transactions.len();
            return Ok(());
        }
        if requested_start == Some(oldest_in_page) || !self.seen_starts.insert(oldest_in_page) {
            return Err("history_pagination_non_progressing".to_string());
        }
        self.next_start = Some(oldest_in_page);
        Ok(())
    }

    fn require_zero_opening_balance(&self) -> Result<(), String> {
        if self.balance_before_oldest_e8s == Some(0) {
            Ok(())
        } else {
            Err("history_nonzero_opening_balance".to_string())
        }
    }
}

fn invert_account_transaction(
    account: &str,
    balance_after: u64,
    entry: &IndexTransactionWithId,
) -> Result<u64, String> {
    let add = |value: u64| {
        balance_after
            .checked_add(value)
            .ok_or_else(|| "history_balance_overflow".to_string())
    };
    let subtract = |value: u64| {
        balance_after
            .checked_sub(value)
            .ok_or_else(|| "history_balance_underflow".to_string())
    };
    match &entry.transaction.operation {
        IndexOperation::Transfer {
            from,
            to,
            amount,
            fee,
            ..
        } => match (from == account, to == account) {
            (true, true) => add(fee.e8s()),
            (true, false) => add(amount
                .e8s()
                .checked_add(fee.e8s())
                .ok_or_else(|| "history_balance_overflow".to_string())?),
            (false, true) => subtract(amount.e8s()),
            (false, false) => Ok(balance_after),
        },
        IndexOperation::Mint { to, amount } if to == account => subtract(amount.e8s()),
        IndexOperation::Burn { from, amount, .. } if from == account => add(amount.e8s()),
        IndexOperation::Approve { from, fee, .. } if from == account => add(fee.e8s()),
        _ => Ok(balance_after),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_ic_clients::index::{IndexOperation, IndexTimeStamp, IndexTransaction, Tokens};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct MockHistory {
        pages: Mutex<VecDeque<Result<GetAccountIdentifierTransactionsResponse, String>>>,
        requests: Mutex<Vec<(String, Option<u64>)>>,
    }

    #[async_trait]
    impl RewardHistoryClient for MockHistory {
        async fn get_transactions(
            &self,
            account_identifier: String,
            start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, String> {
            self.requests
                .lock()
                .unwrap()
                .push((account_identifier, start));
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected_history_call".to_string()))
        }
    }

    fn page(
        ids: impl IntoIterator<Item = u64>,
        oldest_tx_id: u64,
    ) -> GetAccountIdentifierTransactionsResponse {
        GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: ids
                .into_iter()
                .map(|id| IndexTransactionWithId {
                    id,
                    transaction: IndexTransaction {
                        memo: 0,
                        icrc1_memo: None,
                        operation: IndexOperation::Mint {
                            to: "unrelated".to_string(),
                            amount: Tokens::new(1),
                        },
                        created_at_time: None,
                        timestamp: Some(IndexTimeStamp {
                            timestamp_nanos: id,
                        }),
                    },
                })
                .collect(),
            oldest_tx_id: Some(oldest_tx_id),
        }
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn real_delegated_transfer_debits_owner_and_credits_recipient_not_spender() {
        let bytes = hex::decode(include_str!("../../../../crates/ic-clients/tests/fixtures/icp-index-delegated-transfer-response.hex").split_whitespace().collect::<String>()).unwrap();
        let decoded: jupiter_ic_clients::index::GetAccountIdentifierTransactionsResult =
            candid::decode_one(&bytes).unwrap();
        let jupiter_ic_clients::index::GetAccountIdentifierTransactionsResult::Ok(page) = decoded
        else {
            panic!("expected successful real Index page");
        };
        let tx = &page.transactions[0];
        let IndexOperation::Transfer {
            from,
            to,
            spender: Some(spender),
            ..
        } = &tx.transaction.operation
        else {
            panic!("expected real delegated Transfer");
        };
        assert_eq!(invert_account_transaction(from, 0, tx).unwrap(), 20_010_000);
        assert_eq!(invert_account_transaction(to, 20_000_000, tx).unwrap(), 0);
        assert_eq!(invert_account_transaction(spender, 0, tx).unwrap(), 0);
    }

    #[test]
    fn supported_attempt_reads_twelve_pages_without_truncating_provenance() {
        let mut pages = VecDeque::new();
        for page_number in 0..12_u64 {
            let high = 12_000 - page_number * ICP_HISTORY_PAGE_SIZE;
            let low = high - ICP_HISTORY_PAGE_SIZE + 1;
            pages.push_back(Ok(page((low..=high).rev(), 1)));
        }
        let index = MockHistory {
            pages: Mutex::new(pages),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        while !history.exhausted() {
            block_on(history.extend(&index)).unwrap();
        }
        assert_eq!(history.transactions().len(), 12_000);
        assert_eq!(history.pages_requested(), 12);
        assert_eq!(history.transactions().first().unwrap().id, 12_000);
        assert_eq!(history.transactions().last().unwrap().id, 1);
        eprintln!(
            "reward-history valid pages=12 records=12000 usage={:?}",
            history.budget.usage()
        );
    }

    #[test]
    fn account_readers_share_record_and_call_allowances_and_reject_oversized_pages() {
        use super::super::reward_budget::{EXHAUSTED, MAX_CALLS, MAX_RECORDS};
        let budget = WorkBudget::for_test();
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(page([3, 2], 1)), Ok(page([1], 1))])),
            requests: Mutex::new(Vec::new()),
        };
        let mut first = BackwardHistory::with_budget("first".into(), budget.clone());
        block_on(first.extend(&index)).unwrap();
        budget.retain(MAX_RECORDS - 2, 0).unwrap();
        let mut nested = BackwardHistory::with_budget("splitter".into(), budget.clone());
        assert_eq!(block_on(nested.extend(&index)).unwrap_err(), EXHAUSTED);
        assert!(nested.transactions().is_empty());
        assert_eq!(budget.usage().records, MAX_RECORDS);
        for _ in 2..MAX_CALLS {
            block_on(budget.call(async {})).unwrap();
        }
        assert_eq!(block_on(first.extend(&index)).unwrap_err(), EXHAUSTED);
        assert_eq!(index.requests.lock().unwrap().len(), 2);

        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(page((1..=1001).rev(), 1))])),
            requests: Mutex::new(Vec::new()),
        };
        let mut oversized = BackwardHistory::new("oversized".into());
        assert_eq!(
            block_on(oversized.extend(&index)).unwrap_err(),
            "history_oversized_page"
        );
        assert!(oversized.transactions().is_empty());
    }

    #[test]
    fn repeated_or_non_descending_pagination_fails_closed() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([
                Ok(page((2..=1_001).rev(), 1)),
                Ok(page([2], 1)),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        assert_eq!(
            block_on(history.extend(&index)).unwrap_err(),
            "history_pagination_non_progressing"
        );
    }

    #[test]
    fn initially_empty_zero_balance_account_is_authoritative() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(page([], 0))])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        assert!(history.exhausted());
        assert!(history.authoritative());
        assert!(history.transactions().is_empty());
    }

    #[test]
    fn documented_genesis_with_nonzero_opening_balance_fails_closed() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(
                GetAccountIdentifierTransactionsResponse {
                    balance: 5,
                    transactions: page([1], 1).transactions,
                    oldest_tx_id: Some(1),
                },
            )])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        assert_eq!(
            block_on(history.extend(&index)).unwrap_err(),
            "history_nonzero_opening_balance"
        );
        assert!(!history.exhausted());
        assert!(!history.authoritative());
    }

    #[test]
    fn empty_page_exhaustion_with_nonzero_opening_balance_fails_closed() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 5,
                    transactions: page([2], 1).transactions,
                    oldest_tx_id: None,
                }),
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 5,
                    transactions: Vec::new(),
                    oldest_tx_id: Some(1),
                }),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        assert_eq!(
            block_on(history.extend(&index)).unwrap_err(),
            "history_nonzero_opening_balance"
        );
        assert!(!history.exhausted());
        assert!(!history.authoritative());
    }

    #[test]
    fn documented_genesis_with_zero_opening_balance_is_authoritative() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(page([1], 1))])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        assert!(history.exhausted());
        assert!(history.authoritative());
        assert_eq!(history.authoritative_transactions().len(), 1);
    }

    #[test]
    fn backward_history_is_account_generic_for_relay_and_splitter_histories() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(page([], 0)), Ok(page([], 0))])),
            requests: Mutex::new(Vec::new()),
        };
        let mut relay_history = BackwardHistory::new("relay-subaccount-1".to_string());
        let mut splitter_history = BackwardHistory::new("relay-splitter-50".to_string());

        block_on(relay_history.extend(&index)).unwrap();
        block_on(splitter_history.extend(&index)).unwrap();

        assert!(relay_history.authoritative());
        assert!(splitter_history.authoritative());
        assert_eq!(
            *index.requests.lock().unwrap(),
            vec![
                ("relay-subaccount-1".to_string(), None),
                ("relay-splitter-50".to_string(), None),
            ]
        );
    }

    #[test]
    fn short_page_without_documented_oldest_keeps_paginating() {
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: page([10, 9], 1).transactions,
                    oldest_tx_id: None,
                }),
                Ok(page([8, 7], 7)),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        assert!(!history.exhausted());
        block_on(history.extend(&index)).unwrap();
        assert!(history.exhausted());
        assert_eq!(history.transactions().len(), 4);
    }

    #[test]
    fn account_balance_proof_detects_page_drift_and_arithmetic_faults() {
        let inconsistent = MockHistory {
            pages: Mutex::new(VecDeque::from([
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: page([2], 1).transactions,
                    oldest_tx_id: Some(1),
                }),
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 1,
                    transactions: page([1], 1).transactions,
                    oldest_tx_id: Some(1),
                }),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        block_on(history.extend(&inconsistent)).unwrap();
        assert_eq!(
            block_on(history.extend(&inconsistent)).unwrap_err(),
            "history_balance_changed_during_pagination"
        );

        let incoming_underflow = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(
                GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: vec![IndexTransactionWithId {
                        id: 1,
                        transaction: IndexTransaction {
                            memo: 0,
                            icrc1_memo: None,
                            operation: IndexOperation::Mint {
                                to: "relay".to_string(),
                                amount: Tokens::new(1),
                            },
                            created_at_time: None,
                            timestamp: None,
                        },
                    }],
                    oldest_tx_id: Some(1),
                },
            )])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        assert_eq!(
            block_on(history.extend(&incoming_underflow)).unwrap_err(),
            "history_balance_underflow"
        );

        let outgoing_overflow = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(
                GetAccountIdentifierTransactionsResponse {
                    balance: u64::MAX,
                    transactions: vec![IndexTransactionWithId {
                        id: 1,
                        transaction: IndexTransaction {
                            memo: 0,
                            icrc1_memo: None,
                            operation: IndexOperation::Burn {
                                from: "relay".to_string(),
                                amount: Tokens::new(1),
                                spender: None,
                            },
                            created_at_time: None,
                            timestamp: None,
                        },
                    }],
                    oldest_tx_id: Some(1),
                },
            )])),
            requests: Mutex::new(Vec::new()),
        };
        let mut history = BackwardHistory::new("relay".to_string());
        assert_eq!(
            block_on(history.extend(&outgoing_overflow)).unwrap_err(),
            "history_balance_overflow"
        );
    }
}
