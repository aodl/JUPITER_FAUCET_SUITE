use std::collections::BTreeSet;

use async_trait::async_trait;
use candid::Principal;
use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsResponse, IcpIndexCanister, IndexTransactionWithId,
};

pub(super) const ICP_HISTORY_PAGE_SIZE: u64 = 1_000;
const ICP_HISTORY_MAX_PAGES: usize = 10;
pub(super) const ICP_HISTORY_MAX_TRANSACTIONS: usize = 10_000;

pub(super) async fn scan_history(
    index_id: Principal,
    account_identifier: String,
    prior_cursor: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
) -> Result<Vec<IndexTransactionWithId>, String> {
    scan_history_with_index(
        &IcpIndexCanister::new(index_id),
        account_identifier,
        prior_cursor,
        carried_credit_start_tx_id,
    )
    .await
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

pub(super) async fn scan_history_with_index<I: RewardHistoryClient>(
    index: &I,
    account_identifier: String,
    prior_cursor: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
) -> Result<Vec<IndexTransactionWithId>, String> {
    match (prior_cursor, carried_credit_start_tx_id) {
        (None, Some(_)) => return Err("history_carry_without_cursor".to_string()),
        (Some(cursor), Some(carried)) if carried >= cursor => {
            return Err("history_carry_invalid".to_string())
        }
        _ => {}
    }
    let mut start = None;
    let mut seen_starts = BTreeSet::new();
    let mut transactions = Vec::new();
    let mut boundary_found = false;
    let mut cursor_found = prior_cursor.is_none();
    let mut previous_tx_id = None;
    for _ in 0..ICP_HISTORY_MAX_PAGES {
        let page = index
            .get_transactions(account_identifier.clone(), start, ICP_HISTORY_PAGE_SIZE)
            .await
            .map_err(|_| "history_read_failed".to_string())?;
        if page.transactions.is_empty() {
            boundary_found = prior_cursor.is_none();
            break;
        }
        for transaction in &page.transactions {
            if previous_tx_id.is_some_and(|previous| transaction.id >= previous) {
                return Err("history_pagination_non_progressing".to_string());
            }
            previous_tx_id = Some(transaction.id);

            if let Some(cursor) = prior_cursor {
                if !cursor_found {
                    match transaction.id.cmp(&cursor) {
                        std::cmp::Ordering::Greater => {}
                        std::cmp::Ordering::Equal => {
                            cursor_found = true;
                            if carried_credit_start_tx_id.is_none() {
                                boundary_found = true;
                                break;
                            }
                            continue;
                        }
                        std::cmp::Ordering::Less => {
                            return Err("history_cursor_not_found".to_string())
                        }
                    }
                } else if let Some(carried) = carried_credit_start_tx_id {
                    if transaction.id < carried {
                        return Err("history_carried_credit_not_found".to_string());
                    }
                }
            }

            transactions.push(transaction.clone());
            if transactions.len() > ICP_HISTORY_MAX_TRANSACTIONS {
                return Err("history_limit_exceeded".to_string());
            }
            if carried_credit_start_tx_id == Some(transaction.id) {
                boundary_found = true;
                break;
            }
        }
        if boundary_found {
            break;
        }
        let oldest_in_page = page.transactions.last().expect("nonempty page").id;
        let reached_history_start = page.transactions.len() < ICP_HISTORY_PAGE_SIZE as usize
            || page.oldest_tx_id == Some(oldest_in_page);
        if reached_history_start {
            if prior_cursor.is_none() {
                boundary_found = true;
                break;
            }
            return Err(if !cursor_found {
                "history_cursor_not_found"
            } else {
                "history_carried_credit_not_found"
            }
            .to_string());
        }
        if start == Some(oldest_in_page) || !seen_starts.insert(oldest_in_page) {
            return Err("history_pagination_non_progressing".to_string());
        }
        start = Some(oldest_in_page);
    }
    if !boundary_found {
        return Err(if transactions.len() >= ICP_HISTORY_MAX_TRANSACTIONS {
            "history_limit_exceeded"
        } else if !cursor_found {
            "history_cursor_not_found"
        } else if carried_credit_start_tx_id.is_some() {
            "history_carried_credit_not_found"
        } else {
            "history_limit_exceeded"
        }
        .to_string());
    }
    Ok(transactions)
}
