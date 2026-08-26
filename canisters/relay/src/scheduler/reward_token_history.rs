use std::collections::{BTreeSet, VecDeque};

use async_trait::async_trait;
use candid::Nat;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc3::transactions::{
    Transaction, TRANSACTION_AUTHORIZED_MINT, TRANSACTION_MINT, TRANSACTION_TRANSFER,
};
use jupiter_ic_clients::icrc_index::{
    GetAccountTransactionsResponse, IcrcIndexCanister, TransactionWithId,
};

const REWARD_HISTORY_PAGE_SIZE: u64 = 1_000;

#[async_trait]
pub(super) trait RewardTokenHistoryClient: Send + Sync {
    async fn get_transactions(
        &self,
        account: Account,
        start: Option<Nat>,
        max_results: Nat,
    ) -> Result<GetAccountTransactionsResponse, String>;
}

#[async_trait]
impl RewardTokenHistoryClient for IcrcIndexCanister {
    async fn get_transactions(
        &self,
        account: Account,
        start: Option<Nat>,
        max_results: Nat,
    ) -> Result<GetAccountTransactionsResponse, String> {
        self.get_account_transactions(account, start, max_results)
            .await
            .map_err(|_| "reward_index_unavailable".to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RewardCredit {
    remaining_amount: Nat,
    ledger_block_timestamp_nanos: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BalanceEffect {
    Credit { amount: Nat, timestamp_nanos: u64 },
    Debit { amount: Nat },
}

fn nat_to_u64(value: &Nat) -> Result<u64, String> {
    u64::try_from(value.0.clone()).map_err(|_| "reward_history_malformed".to_string())
}

fn transaction_effect(
    transaction: &Transaction,
    account: &Account,
) -> Result<BalanceEffect, String> {
    let populated = usize::from(transaction.mint.is_some())
        + usize::from(transaction.burn.is_some())
        + usize::from(transaction.transfer.is_some())
        + usize::from(transaction.approve.is_some())
        + usize::from(transaction.fee_collector.is_some())
        + usize::from(transaction.authorized_mint.is_some())
        + usize::from(transaction.authorized_burn.is_some());
    if populated != 1 {
        return Err("reward_history_malformed".to_string());
    }

    if let Some(mint) = &transaction.mint {
        if transaction.kind != TRANSACTION_MINT || mint.to != *account {
            return Err("reward_history_malformed".to_string());
        }
        let fee = mint.fee.clone().unwrap_or_else(|| Nat::from(0_u8));
        if mint.amount.0 < fee.0 {
            return Err("reward_history_malformed".to_string());
        }
        return Ok(BalanceEffect::Credit {
            amount: Nat(mint.amount.0.clone() - fee.0),
            timestamp_nanos: transaction.timestamp,
        });
    }

    if let Some(transfer) = &transaction.transfer {
        if transaction.kind != TRANSACTION_TRANSFER {
            return Err("reward_history_malformed".to_string());
        }
        let from_relay = transfer.from == *account;
        let to_relay = transfer.to == *account;
        if from_relay == to_relay {
            return Err("reward_history_malformed".to_string());
        }
        if to_relay {
            return Ok(BalanceEffect::Credit {
                amount: transfer.amount.clone(),
                timestamp_nanos: transaction.timestamp,
            });
        }
        if transfer.spender.is_some() {
            return Err("reward_history_malformed".to_string());
        }
        let fee = transfer
            .fee
            .clone()
            .ok_or_else(|| "reward_history_malformed".to_string())?;
        return Ok(BalanceEffect::Debit {
            amount: Nat(transfer.amount.0.clone() + fee.0),
        });
    }

    if let Some(mint) = &transaction.authorized_mint {
        if transaction.kind != TRANSACTION_AUTHORIZED_MINT || mint.to != *account {
            return Err("reward_history_malformed".to_string());
        }
        return Ok(BalanceEffect::Credit {
            amount: mint.amount.clone(),
            timestamp_nanos: transaction.timestamp,
        });
    }

    if transaction
        .burn
        .as_ref()
        .is_some_and(|burn| burn.from == *account)
        || transaction
            .approve
            .as_ref()
            .is_some_and(|approve| approve.from == *account)
        || transaction
            .authorized_burn
            .as_ref()
            .is_some_and(|burn| burn.from == *account)
    {
        return Err("reward_history_malformed".to_string());
    }
    Err("reward_history_malformed".to_string())
}

/// Heap-only backwards reconstruction of the current reward-balance epoch. Nothing in this
/// reader survives the adjudication that created it.
struct BackwardRewardHistory {
    account: Account,
    live_balance: Nat,
    balance_after: Nat,
    next_start: Option<Nat>,
    seen_starts: BTreeSet<u64>,
    previous_tx_id: Option<u64>,
    transactions: Vec<TransactionWithId>,
    boundary_found: bool,
    exhausted: bool,
    #[cfg(test)]
    pages_requested: usize,
}

impl BackwardRewardHistory {
    fn new(account: Account, live_balance: Nat) -> Self {
        Self {
            account,
            balance_after: live_balance.clone(),
            live_balance,
            next_start: None,
            seen_starts: BTreeSet::new(),
            previous_tx_id: None,
            transactions: Vec::new(),
            boundary_found: false,
            exhausted: false,
            #[cfg(test)]
            pages_requested: 0,
        }
    }

    async fn extend<I: RewardTokenHistoryClient>(&mut self, index: &I) -> Result<(), String> {
        if self.boundary_found || self.exhausted {
            return Ok(());
        }
        let requested_start = self.next_start.clone();
        let page = index
            .get_transactions(
                self.account,
                requested_start.clone(),
                Nat::from(REWARD_HISTORY_PAGE_SIZE),
            )
            .await?;
        #[cfg(test)]
        {
            self.pages_requested += 1;
        }
        if page.balance != self.live_balance {
            return Err("reward_history_not_caught_up".to_string());
        }
        if page.transactions.is_empty() {
            self.exhausted = true;
            return if self.balance_after.0 == 0_u8.into() {
                Ok(())
            } else {
                Err("reward_history_not_caught_up".to_string())
            };
        }

        let requested_start_u64 = requested_start.as_ref().map(nat_to_u64).transpose()?;
        let mut page_ids = Vec::with_capacity(page.transactions.len());
        let mut previous = self.previous_tx_id;
        for entry in &page.transactions {
            let id = nat_to_u64(&entry.id)?;
            if requested_start_u64.is_some_and(|start| id >= start)
                || previous.is_some_and(|previous_id| id >= previous_id)
            {
                return Err("reward_history_pagination_non_progressing".to_string());
            }
            previous = Some(id);
            page_ids.push(id);
        }
        let oldest_in_page = *page_ids.last().expect("nonempty reward history page");
        let documented_oldest = page.oldest_tx_id.as_ref().map(nat_to_u64).transpose()?;
        if documented_oldest.is_some_and(|oldest| oldest > oldest_in_page) {
            return Err("reward_history_pagination_inconsistent_oldest".to_string());
        }

        for entry in page.transactions {
            let effect = transaction_effect(&entry.transaction, &self.account)?;
            match effect {
                BalanceEffect::Credit { amount, .. } => {
                    if self.balance_after.0 < amount.0 {
                        return Err("reward_history_malformed".to_string());
                    }
                    self.balance_after.0 -= amount.0;
                }
                BalanceEffect::Debit { amount } => {
                    self.balance_after.0 += amount.0;
                }
            }
            self.previous_tx_id = Some(nat_to_u64(&entry.id)?);
            self.transactions.push(entry);
            if self.balance_after.0 == 0_u8.into() {
                self.boundary_found = true;
                return Ok(());
            }
        }

        self.exhausted = documented_oldest == Some(oldest_in_page);
        if self.exhausted {
            return Err("reward_history_not_caught_up".to_string());
        }
        if requested_start_u64 == Some(oldest_in_page) || !self.seen_starts.insert(oldest_in_page) {
            return Err("reward_history_pagination_non_progressing".to_string());
        }
        self.next_start = Some(Nat::from(oldest_in_page));
        Ok(())
    }

    fn cutoff(&self) -> Result<u64, String> {
        if !self.boundary_found && !self.exhausted {
            return Err("reward_history_not_caught_up".to_string());
        }
        let mut credits = VecDeque::<RewardCredit>::new();
        for entry in self.transactions.iter().rev() {
            match transaction_effect(&entry.transaction, &self.account)? {
                BalanceEffect::Credit {
                    amount,
                    timestamp_nanos,
                } => {
                    if amount.0 != 0_u8.into() {
                        credits.push_back(RewardCredit {
                            remaining_amount: amount,
                            ledger_block_timestamp_nanos: timestamp_nanos,
                        });
                    }
                }
                BalanceEffect::Debit { amount } => {
                    let mut remaining = amount.0;
                    while remaining > 0_u8.into() {
                        let Some(front) = credits.front_mut() else {
                            return Err("reward_history_malformed".to_string());
                        };
                        if front.remaining_amount.0 <= remaining {
                            remaining -= front.remaining_amount.0.clone();
                            credits.pop_front();
                        } else {
                            front.remaining_amount.0 -= remaining;
                            remaining = 0_u8.into();
                        }
                    }
                }
            }
        }
        let reconstructed = credits.iter().fold(Nat::from(0_u8).0, |sum, credit| {
            sum + credit.remaining_amount.0.clone()
        });
        if reconstructed != self.live_balance.0 {
            return Err("reward_history_not_caught_up".to_string());
        }
        credits
            .front()
            .map(|credit| credit.ledger_block_timestamp_nanos)
            .ok_or_else(|| "reward_history_not_caught_up".to_string())
    }
}

pub(super) async fn reward_pot_cutoff<I: RewardTokenHistoryClient>(
    index: &I,
    account: Account,
    live_balance: Nat,
) -> Result<u64, String> {
    let mut history = BackwardRewardHistory::new(account, live_balance);
    while !history.boundary_found && !history.exhausted {
        history.extend(index).await?;
    }
    history.cutoff()
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Principal;
    use icrc_ledger_types::icrc3::transactions::{Approve, AuthorizedMint, Burn, Mint, Transfer};
    use std::sync::Mutex;

    fn account(byte: u8) -> Account {
        Account {
            owner: Principal::from_slice(&[byte]),
            subaccount: None,
        }
    }

    fn mint(id: u64, to: Account, amount: u64, timestamp: u64) -> TransactionWithId {
        TransactionWithId {
            id: Nat::from(id),
            transaction: Transaction::mint(
                Mint {
                    amount: Nat::from(amount),
                    to,
                    memo: None,
                    created_at_time: Some(timestamp.saturating_sub(1)),
                    fee: None,
                },
                timestamp,
            ),
        }
    }

    fn transfer(
        id: u64,
        from: Account,
        to: Account,
        amount: u64,
        fee: u64,
        timestamp: u64,
    ) -> TransactionWithId {
        TransactionWithId {
            id: Nat::from(id),
            transaction: Transaction::transfer(
                Transfer {
                    amount: Nat::from(amount),
                    from,
                    to,
                    spender: None,
                    memo: None,
                    fee: Some(Nat::from(fee)),
                    created_at_time: Some(timestamp.saturating_sub(1)),
                },
                timestamp,
            ),
        }
    }

    struct MockIndex {
        pages: Mutex<VecDeque<GetAccountTransactionsResponse>>,
        starts: Mutex<Vec<Option<Nat>>>,
    }

    #[async_trait]
    impl RewardTokenHistoryClient for MockIndex {
        async fn get_transactions(
            &self,
            _account: Account,
            start: Option<Nat>,
            _max_results: Nat,
        ) -> Result<GetAccountTransactionsResponse, String> {
            self.starts.lock().unwrap().push(start);
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "unexpected_reward_history_call".to_string())
        }
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn page(
        balance: u64,
        transactions: Vec<TransactionWithId>,
        oldest: Option<u64>,
    ) -> GetAccountTransactionsResponse {
        GetAccountTransactionsResponse {
            balance: Nat::from(balance),
            transactions,
            oldest_tx_id: oldest.map(Nat::from),
        }
    }

    #[test]
    fn fifo_replay_uses_oldest_partially_unspent_credit_timestamp() {
        let relay = account(1);
        let other = account(2);
        let index = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                80,
                vec![
                    transfer(3, relay, other, 60, 10, 40),
                    mint(2, relay, 50, 30),
                    mint(1, relay, 100, 10),
                ],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&index, relay, Nat::from(80_u64))).unwrap(),
            10
        );
    }

    #[test]
    fn once_old_credit_is_consumed_next_credit_supplies_cutoff() {
        let relay = account(1);
        let other = account(2);
        let index = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                30,
                vec![
                    transfer(3, relay, other, 110, 10, 40),
                    mint(2, relay, 50, 30),
                    mint(1, relay, 100, 10),
                ],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&index, relay, Nat::from(30_u64))).unwrap(),
            30
        );
    }

    #[test]
    fn zero_value_credits_never_supply_the_cutoff() {
        let relay = account(1);
        let other = account(2);
        let index = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                100,
                vec![
                    transfer(4, relay, other, 100, 0, 40),
                    mint(3, relay, 100, 30),
                    mint(2, relay, 0, 20),
                    mint(1, relay, 100, 10),
                ],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&index, relay, Nat::from(100_u64))).unwrap(),
            30
        );
    }

    #[test]
    fn hidden_net_zero_suffix_can_only_bias_cutoff_older() {
        let relay = account(1);
        let other = account(2);
        let synchronized = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                101,
                vec![
                    mint(3, relay, 101, 30),
                    transfer(2, relay, other, 100, 1, 20),
                    mint(1, relay, 101, 10),
                ],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        let synchronized_cutoff =
            block_on(reward_pot_cutoff(&synchronized, relay, Nat::from(101_u64))).unwrap();
        assert_eq!(synchronized_cutoff, 30);

        // If the Index has not yet exposed the net-zero suffix (-101, +101), its indexed balance
        // still equals the live Ledger balance. The resulting cutoff is stale but conservative.
        let lagged = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                101,
                vec![mint(1, relay, 101, 10)],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        let lagged_cutoff =
            block_on(reward_pot_cutoff(&lagged, relay, Nat::from(101_u64))).unwrap();
        assert_eq!(lagged_cutoff, 10);
        assert!(lagged_cutoff <= synchronized_cutoff);
    }

    #[test]
    fn recent_zero_boundary_stops_without_reading_huge_older_history() {
        let relay = account(1);
        let other = account(2);
        let mut recent = vec![mint(12_001, relay, 100, 20_000)];
        for pair in 0..499_u64 {
            let outgoing_id = 12_000 - pair * 2;
            recent.push(transfer(outgoing_id, relay, other, 1, 0, outgoing_id));
            recent.push(mint(outgoing_id - 1, relay, 1, outgoing_id - 1));
        }
        let mut pages = VecDeque::from([page(100, recent, Some(1))]);
        let mut high = 11_002_u64;
        for _ in 0..11 {
            let mut older_page = Vec::with_capacity(REWARD_HISTORY_PAGE_SIZE as usize);
            for _ in 0..500 {
                older_page.push(transfer(high, relay, other, 1, 0, high));
                older_page.push(mint(high - 1, relay, 1, high - 1));
                high -= 2;
            }
            pages.push_back(page(100, older_page, Some(1)));
        }
        let index = MockIndex {
            pages: Mutex::new(pages),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&index, relay, Nat::from(100_u64))).unwrap(),
            20_000
        );
        assert_eq!(index.starts.lock().unwrap().len(), 1);
    }

    #[test]
    fn history_without_zero_until_genesis_has_no_depth_limit() {
        let relay = account(1);
        let mut pages = VecDeque::new();
        for page_number in 0..12_u64 {
            let high = 12_000 - page_number * REWARD_HISTORY_PAGE_SIZE;
            let low = high - REWARD_HISTORY_PAGE_SIZE + 1;
            let transactions = (low..=high)
                .rev()
                .map(|id| mint(id, relay, 1, id))
                .collect();
            pages.push_back(page(12_000, transactions, Some(1)));
        }
        let index = MockIndex {
            pages: Mutex::new(pages),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&index, relay, Nat::from(12_000_u64))).unwrap(),
            1
        );
        assert_eq!(index.starts.lock().unwrap().len(), 12);
    }

    #[test]
    fn lag_pagination_corruption_and_arithmetic_inconsistency_fail_closed() {
        let relay = account(1);
        let lagged = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                50,
                vec![mint(1, relay, 50, 1)],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&lagged, relay, Nat::from(100_u64))).unwrap_err(),
            "reward_history_not_caught_up"
        );

        let underflow = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                100,
                vec![mint(1, relay, 101, 1)],
                Some(1),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&underflow, relay, Nat::from(100_u64))).unwrap_err(),
            "reward_history_malformed"
        );

        let repeated = MockIndex {
            pages: Mutex::new(VecDeque::from([
                page(2, vec![mint(2, relay, 1, 2)], Some(1)),
                page(2, vec![mint(2, relay, 1, 2)], Some(1)),
            ])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&repeated, relay, Nat::from(2_u64))).unwrap_err(),
            "reward_history_pagination_non_progressing"
        );

        let inconsistent_oldest = MockIndex {
            pages: Mutex::new(VecDeque::from([page(
                2,
                vec![mint(2, relay, 1, 2)],
                Some(3),
            )])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(
                &inconsistent_oldest,
                relay,
                Nat::from(2_u64)
            ))
            .unwrap_err(),
            "reward_history_pagination_inconsistent_oldest"
        );

        let oversized_id = Nat::from(u128::from(u64::MAX) + 1);
        let oversized = MockIndex {
            pages: Mutex::new(VecDeque::from([GetAccountTransactionsResponse {
                balance: Nat::from(1_u64),
                transactions: vec![TransactionWithId {
                    id: oversized_id.clone(),
                    transaction: mint(1, relay, 1, 1).transaction,
                }],
                oldest_tx_id: Some(oversized_id),
            }])),
            starts: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(reward_pot_cutoff(&oversized, relay, Nat::from(1_u64))).unwrap_err(),
            "reward_history_malformed"
        );
    }

    #[test]
    fn supported_credits_and_unexpected_reward_account_debits_are_classified_exactly() {
        let relay = account(1);
        let other = account(2);
        let incoming_transfer_from = Transaction::transfer(
            Transfer {
                amount: Nat::from(25_u64),
                from: other,
                to: relay,
                spender: Some(account(3)),
                memo: None,
                fee: Some(Nat::from(7_u64)),
                created_at_time: Some(1),
            },
            20,
        );
        assert_eq!(
            transaction_effect(&incoming_transfer_from, &relay).unwrap(),
            BalanceEffect::Credit {
                amount: Nat::from(25_u64),
                timestamp_nanos: 20,
            }
        );

        let fee_bearing_mint = Transaction::mint(
            Mint {
                amount: Nat::from(100_u64),
                to: relay,
                memo: None,
                created_at_time: Some(2),
                fee: Some(Nat::from(10_u64)),
            },
            30,
        );
        assert_eq!(
            transaction_effect(&fee_bearing_mint, &relay).unwrap(),
            BalanceEffect::Credit {
                amount: Nat::from(90_u64),
                timestamp_nanos: 30,
            }
        );

        let authorized_mint = Transaction::authorized_mint(
            AuthorizedMint {
                to: relay,
                amount: Nat::from(11_u64),
                created_at_time: Some(3),
                caller: Some(other.owner),
                mthd: Some("mint".to_string()),
                reason: None,
            },
            31,
        );
        assert_eq!(authorized_mint.kind, TRANSACTION_AUTHORIZED_MINT);
        assert_eq!(
            transaction_effect(&authorized_mint, &relay).unwrap(),
            BalanceEffect::Credit {
                amount: Nat::from(11_u64),
                timestamp_nanos: 31,
            }
        );

        let spender_debit = Transaction::transfer(
            Transfer {
                amount: Nat::from(1_u64),
                from: relay,
                to: other,
                spender: Some(account(3)),
                memo: None,
                fee: Some(Nat::from(1_u64)),
                created_at_time: None,
            },
            40,
        );
        let burn = Transaction::burn(
            Burn {
                amount: Nat::from(1_u64),
                from: relay,
                spender: None,
                memo: None,
                created_at_time: None,
                fee: None,
            },
            41,
        );
        let approve = Transaction::approve(
            Approve {
                from: relay,
                spender: other,
                amount: Nat::from(1_u64),
                expected_allowance: None,
                expires_at: None,
                memo: None,
                fee: Some(Nat::from(1_u64)),
                created_at_time: None,
            },
            42,
        );
        for unsupported in [spender_debit, burn, approve] {
            assert_eq!(
                transaction_effect(&unsupported, &relay).unwrap_err(),
                "reward_history_malformed"
            );
        }
    }
}
