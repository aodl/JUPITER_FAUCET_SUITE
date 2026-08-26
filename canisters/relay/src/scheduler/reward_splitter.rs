use std::collections::{BTreeMap, BTreeSet, VecDeque};

use candid::Principal;
use jupiter_ic_clients::account_identifier::{account_identifier_bytes, account_identifier_text};
use jupiter_ic_clients::index::{IndexOperation, IndexTransactionWithId};

use super::reward_history;
use crate::logic;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SplitterFundingCredit {
    pub splitter_number: u8,
    pub tx_id: u64,
    pub amount_e8s: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ExpandedSplitterProvenance {
    pub sources: BTreeMap<[u8; 32], u64>,
    pub ineligible_e8s: u64,
    pub scanned_transactions: usize,
    pub splitters_scanned: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UpstreamCredit {
    tx_id: u64,
    source: Option<[u8; 32]>,
    amount_e8s: u64,
}

#[derive(Clone, Copy, Debug)]
struct OutgoingLeg {
    tx_id: u64,
    amount_e8s: u64,
    fee_e8s: u64,
    funding_credits_at_pin: usize,
}

pub(super) fn intrinsic_splitter_accounts(relay: Principal) -> BTreeMap<[u8; 32], u8> {
    logic::SPLITTER_PERCENTAGES
        .into_iter()
        .map(|number| {
            (
                account_identifier_bytes(relay, Some(logic::relay_numbered_subaccount(number))),
                number,
            )
        })
        .collect()
}

pub(super) fn proportional_allocations(
    credits: &[u64],
    subaccount_one_amount_e8s: u64,
) -> Result<Vec<u64>, String> {
    let balance = credits.iter().try_fold(0_u64, |total, value| {
        total
            .checked_add(*value)
            .ok_or_else(|| "splitter_allocation_overflow".to_string())
    })?;
    if balance == 0 || subaccount_one_amount_e8s > balance {
        return Err("splitter_allocation_invalid_total".to_string());
    }
    let mut prefix = 0_u64;
    let mut previous_floor = 0_u64;
    let mut allocations = Vec::with_capacity(credits.len());
    for credit in credits {
        prefix = prefix
            .checked_add(*credit)
            .ok_or_else(|| "splitter_allocation_overflow".to_string())?;
        let floor: u64 = (u128::from(prefix) * u128::from(subaccount_one_amount_e8s)
            / u128::from(balance))
        .try_into()
        .map_err(|_| "splitter_allocation_overflow".to_string())?;
        let allocated = floor
            .checked_sub(previous_floor)
            .ok_or_else(|| "splitter_allocation_underflow".to_string())?;
        if allocated > *credit {
            return Err("splitter_allocation_exceeds_credit".to_string());
        }
        allocations.push(allocated);
        previous_floor = floor;
    }
    if previous_floor != subaccount_one_amount_e8s {
        return Err("splitter_allocation_not_conserved".to_string());
    }
    Ok(allocations)
}

fn decode_account_identifier(text: &str) -> Option<[u8; 32]> {
    hex::decode(text).ok()?.try_into().ok()
}

fn add_checked(target: &mut u64, amount: u64) -> Result<(), String> {
    *target = target
        .checked_add(amount)
        .ok_or_else(|| "contribution_overflow".to_string())?;
    Ok(())
}

fn provenance_error(splitter: u8, tx_id: Option<u64>, class: &str) -> String {
    format!(
        "splitter_provenance_failed:splitter={splitter}:tx={}:class={class}",
        tx_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string())
    )
}

fn push_incoming_credit(
    funding: &mut VecDeque<UpstreamCredit>,
    splitter_account: &str,
    entry: &IndexTransactionWithId,
) {
    match &entry.transaction.operation {
        IndexOperation::Transfer {
            to, from, amount, ..
        }
        | IndexOperation::TransferFrom {
            to, from, amount, ..
        } if to == splitter_account && from != splitter_account => {
            funding.push_back(UpstreamCredit {
                tx_id: entry.id,
                source: decode_account_identifier(from),
                amount_e8s: amount.e8s(),
            });
        }
        IndexOperation::Mint { to, amount } if to == splitter_account => {
            funding.push_back(UpstreamCredit {
                tx_id: entry.id,
                source: None,
                amount_e8s: amount.e8s(),
            });
        }
        _ => {}
    }
}

fn consume_job_funding(
    balance_e8s: u64,
    subaccount_one_amount_e8s: u64,
    funding_credits_at_pin: usize,
    funding: &mut VecDeque<UpstreamCredit>,
    sources: &mut BTreeMap<[u8; 32], u64>,
    ineligible_e8s: &mut u64,
) -> Result<(), &'static str> {
    let mut remaining = balance_e8s;
    let mut consumed = Vec::new();
    while remaining > 0 {
        if consumed.len() == funding_credits_at_pin {
            return Err("missing_funding_credit");
        }
        let credit = funding.pop_front().ok_or("missing_funding_credit")?;
        if credit.amount_e8s > remaining {
            return Err("funding_credit_exceeds_pinned_remainder");
        }
        remaining -= credit.amount_e8s;
        consumed.push(credit);
    }
    let allocations = proportional_allocations(
        &consumed
            .iter()
            .map(|credit| credit.amount_e8s)
            .collect::<Vec<_>>(),
        subaccount_one_amount_e8s,
    )
    .map_err(|_| "splitter_allocation_failed")?;
    for (credit, allocated) in consumed.into_iter().zip(allocations) {
        if let Some(source) = credit.source {
            add_checked(sources.entry(source).or_default(), allocated)
                .map_err(|_| "contribution_overflow")?;
        } else {
            add_checked(ineligible_e8s, allocated).map_err(|_| "contribution_overflow")?;
        }
    }
    Ok(())
}

pub(super) fn reconstruct_splitter_history(
    relay: Principal,
    splitter_number: u8,
    transactions: &[IndexTransactionWithId],
    required_credits: &[SplitterFundingCredit],
    history_authoritative: bool,
) -> reward_history::HistoricalReconstruction<ExpandedSplitterProvenance> {
    if !history_authoritative {
        return reward_history::HistoricalReconstruction::NeedOlderHistory;
    }
    if logic::splitter_percentage(splitter_number).is_none()
        || required_credits
            .iter()
            .any(|credit| credit.splitter_number != splitter_number)
    {
        return reward_history::HistoricalReconstruction::Malformed(provenance_error(
            splitter_number,
            None,
            "invalid_splitter_number",
        ));
    }
    let mut required = BTreeMap::<u64, u64>::new();
    for credit in required_credits {
        if required.insert(credit.tx_id, credit.amount_e8s).is_some() {
            return reward_history::HistoricalReconstruction::Malformed(provenance_error(
                splitter_number,
                Some(credit.tx_id),
                "duplicate_subaccount_one_anchor",
            ));
        }
    }
    let Some(last_anchor) = required.keys().next_back().copied() else {
        return reward_history::HistoricalReconstruction::Complete(
            ExpandedSplitterProvenance::default(),
        );
    };
    let splitter_account = account_identifier_text(
        relay,
        Some(logic::relay_numbered_subaccount(splitter_number)),
    );
    let default_account = account_identifier_text(relay, None);
    let subaccount_one = account_identifier_text(relay, Some(logic::relay_subaccount_one()));
    let mut chronological = transactions.to_vec();
    chronological.sort_by_key(|transaction| transaction.id);
    let mut funding = VecDeque::new();
    let mut pending_default = None::<OutgoingLeg>;
    let mut result = ExpandedSplitterProvenance::default();

    for entry in chronological {
        if entry.id > last_anchor {
            continue;
        }
        match &entry.transaction.operation {
            IndexOperation::Transfer {
                to,
                from,
                amount,
                fee,
                ..
            } if from == &splitter_account => {
                if entry.transaction.memo != 0 || entry.transaction.icrc1_memo.is_some() {
                    return reward_history::HistoricalReconstruction::Malformed(provenance_error(
                        splitter_number,
                        Some(entry.id),
                        "unsupported_outgoing_memo",
                    ));
                }
                let leg = OutgoingLeg {
                    tx_id: entry.id,
                    amount_e8s: amount.e8s(),
                    fee_e8s: fee.e8s(),
                    funding_credits_at_pin: funding.len(),
                };
                if to == &default_account {
                    if pending_default.replace(leg).is_some() {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "default_leg_while_pair_incomplete",
                            ),
                        );
                    }
                } else if to == &subaccount_one {
                    let Some(default) = pending_default.take() else {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "subaccount_one_leg_without_default",
                            ),
                        );
                    };
                    let expected_amount = required.get(&entry.id).copied();
                    let historical_unselected_job = expected_amount.is_none();
                    if expected_amount.is_some_and(|expected| expected != leg.amount_e8s) {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "subaccount_one_anchor_amount_mismatch",
                            ),
                        );
                    }
                    if default.tx_id >= leg.tx_id || default.amount_e8s == 0 || leg.amount_e8s == 0
                    {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "invalid_splitter_leg_pair",
                            ),
                        );
                    }
                    let Some(default_gross) = default.amount_e8s.checked_add(default.fee_e8s)
                    else {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(default.tx_id),
                                "default_gross_overflow",
                            ),
                        );
                    };
                    let Some(subaccount_one_gross) = leg.amount_e8s.checked_add(leg.fee_e8s) else {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "subaccount_one_gross_overflow",
                            ),
                        );
                    };
                    let Some(balance) = default_gross.checked_add(subaccount_one_gross) else {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "pinned_balance_overflow",
                            ),
                        );
                    };
                    let expected_default =
                        (u128::from(balance) * u128::from(splitter_number) / 100) as u64;
                    if default_gross != expected_default
                        || subaccount_one_gross != balance - expected_default
                    {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "split_percentage_mismatch",
                            ),
                        );
                    }
                    let mut discarded_sources = BTreeMap::new();
                    let mut discarded_ineligible = 0;
                    let (target_sources, target_ineligible) = if historical_unselected_job {
                        (&mut discarded_sources, &mut discarded_ineligible)
                    } else {
                        (&mut result.sources, &mut result.ineligible_e8s)
                    };
                    if let Err(class) = consume_job_funding(
                        balance,
                        leg.amount_e8s,
                        default.funding_credits_at_pin,
                        &mut funding,
                        target_sources,
                        target_ineligible,
                    ) {
                        return reward_history::HistoricalReconstruction::Malformed(
                            provenance_error(splitter_number, Some(entry.id), class),
                        );
                    }
                    if !historical_unselected_job {
                        required.remove(&entry.id);
                    }
                } else {
                    return reward_history::HistoricalReconstruction::Malformed(provenance_error(
                        splitter_number,
                        Some(entry.id),
                        "unexpected_transfer_destination",
                    ));
                }
            }
            IndexOperation::TransferFrom { from, .. } if from == &splitter_account => {
                return reward_history::HistoricalReconstruction::Malformed(provenance_error(
                    splitter_number,
                    Some(entry.id),
                    "unsupported_transfer_from_debit",
                ));
            }
            IndexOperation::Burn { from, .. } if from == &splitter_account => {
                return reward_history::HistoricalReconstruction::Malformed(provenance_error(
                    splitter_number,
                    Some(entry.id),
                    "unsupported_burn_debit",
                ));
            }
            IndexOperation::Approve { from, fee, .. }
                if from == &splitter_account && fee.e8s() > 0 =>
            {
                return reward_history::HistoricalReconstruction::Malformed(provenance_error(
                    splitter_number,
                    Some(entry.id),
                    "unsupported_approve_fee_debit",
                ));
            }
            _ => push_incoming_credit(&mut funding, &splitter_account, &entry),
        }
    }
    if let Some(default) = pending_default {
        return reward_history::HistoricalReconstruction::Malformed(provenance_error(
            splitter_number,
            Some(default.tx_id),
            "incomplete_splitter_pair",
        ));
    }
    if let Some((&missing, _)) = required.first_key_value() {
        if transactions
            .iter()
            .map(|entry| entry.id)
            .min()
            .is_some_and(|oldest| missing < oldest)
        {
            return reward_history::HistoricalReconstruction::NeedOlderHistory;
        }
        return reward_history::HistoricalReconstruction::Malformed(provenance_error(
            splitter_number,
            Some(missing),
            "subaccount_one_anchor_not_found",
        ));
    }
    reward_history::HistoricalReconstruction::Complete(result)
}

pub(super) struct SplitterHistoryCache {
    histories: BTreeMap<u8, reward_history::BackwardHistory>,
    counted_splitters: BTreeSet<u8>,
}

impl SplitterHistoryCache {
    pub(super) fn new() -> Self {
        Self {
            histories: BTreeMap::new(),
            counted_splitters: BTreeSet::new(),
        }
    }

    pub(super) async fn expand<I: reward_history::RewardHistoryClient>(
        &mut self,
        index: &I,
        relay: Principal,
        credits: &[SplitterFundingCredit],
    ) -> Result<ExpandedSplitterProvenance, String> {
        let referenced = credits
            .iter()
            .map(|credit| credit.splitter_number)
            .collect::<BTreeSet<_>>();
        if referenced
            .iter()
            .any(|number| logic::splitter_percentage(*number).is_none())
        {
            return Err("splitter_provenance_invalid_splitter".to_string());
        }
        let mut expanded = ExpandedSplitterProvenance::default();
        for splitter_number in referenced {
            let first_use = self.counted_splitters.insert(splitter_number);
            let history = self.histories.entry(splitter_number).or_insert_with(|| {
                reward_history::BackwardHistory::new(account_identifier_text(
                    relay,
                    Some(logic::relay_numbered_subaccount(splitter_number)),
                ))
            });
            let starting_len = history.transactions().len();
            let required = credits
                .iter()
                .filter(|credit| credit.splitter_number == splitter_number)
                .cloned()
                .collect::<Vec<_>>();
            let one = loop {
                if history.transactions().is_empty() && !history.exhausted() {
                    history
                        .extend(index)
                        .await
                        .map_err(|class| provenance_error(splitter_number, None, &class))?;
                }
                match reconstruct_splitter_history(
                    relay,
                    splitter_number,
                    history.authoritative_transactions(),
                    &required,
                    history.authoritative(),
                ) {
                    reward_history::HistoricalReconstruction::Complete(provenance) => {
                        break provenance
                    }
                    reward_history::HistoricalReconstruction::NeedOlderHistory => {
                        if history.exhausted() {
                            return Err(provenance_error(
                                splitter_number,
                                required.first().map(|credit| credit.tx_id),
                                "history_exhausted_before_provenance",
                            ));
                        }
                        history
                            .extend(index)
                            .await
                            .map_err(|class| provenance_error(splitter_number, None, &class))?;
                    }
                    reward_history::HistoricalReconstruction::Malformed(error) => {
                        return Err(error)
                    }
                }
            };
            expanded.scanned_transactions = expanded
                .scanned_transactions
                .checked_add(history.transactions().len() - starting_len)
                .ok_or_else(|| "splitter_history_count_overflow".to_string())?;
            expanded.splitters_scanned += usize::from(first_use);
            for (source, amount) in one.sources {
                add_checked(expanded.sources.entry(source).or_default(), amount)?;
            }
            add_checked(&mut expanded.ineligible_e8s, one.ineligible_e8s)?;
        }
        Ok(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use jupiter_ic_clients::index::{
        GetAccountIdentifierTransactionsResponse, IndexTimeStamp, IndexTransaction, Tokens,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn relay() -> Principal {
        Principal::from_slice(&[42])
    }

    fn transfer(
        id: u64,
        from: String,
        to: String,
        amount: u64,
        fee: u64,
    ) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    from,
                    to,
                    amount: Tokens::new(amount),
                    fee: Tokens::new(fee),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: id,
                }),
            },
        }
    }

    fn transfer_from(id: u64, from: String, to: String, amount: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::TransferFrom {
                    from,
                    to,
                    spender: "spender".to_string(),
                    amount: Tokens::new(amount),
                    fee: Tokens::new(10),
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: id,
                }),
            },
        }
    }

    fn mint(id: u64, to: String, amount: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Mint {
                    to,
                    amount: Tokens::new(amount),
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: id,
                }),
            },
        }
    }

    fn split_accounts(number: u8) -> (String, String, String) {
        (
            account_identifier_text(relay(), Some(logic::relay_numbered_subaccount(number))),
            account_identifier_text(relay(), None),
            account_identifier_text(relay(), Some(logic::relay_subaccount_one())),
        )
    }

    fn one_source_job(
        number: u8,
        source: [u8; 32],
        balance: u64,
        default_fee: u64,
        subaccount_one_fee: u64,
        first_id: u64,
    ) -> (Vec<IndexTransactionWithId>, SplitterFundingCredit) {
        let (splitter, default, subaccount_one) = split_accounts(number);
        let default_gross = u64::try_from(u128::from(balance) * u128::from(number) / 100).unwrap();
        let second_gross = balance - default_gross;
        let history = vec![
            transfer(first_id, hex::encode(source), splitter.clone(), balance, 10),
            transfer(
                first_id + 1,
                splitter.clone(),
                default,
                default_gross - default_fee,
                default_fee,
            ),
            transfer(
                first_id + 2,
                splitter,
                subaccount_one,
                second_gross - subaccount_one_fee,
                subaccount_one_fee,
            ),
        ];
        (
            history,
            SplitterFundingCredit {
                splitter_number: number,
                tx_id: first_id + 2,
                amount_e8s: second_gross - subaccount_one_fee,
            },
        )
    }

    fn valid_history() -> (Vec<IndexTransactionWithId>, SplitterFundingCredit) {
        let splitter = account_identifier_text(relay(), Some(logic::relay_numbered_subaccount(50)));
        let default = account_identifier_text(relay(), None);
        let subaccount_one = account_identifier_text(relay(), Some(logic::relay_subaccount_one()));
        (
            vec![
                transfer(1, hex::encode([1; 32]), splitter.clone(), 400, 10),
                transfer(2, hex::encode([2; 32]), splitter.clone(), 600, 10),
                transfer(3, splitter.clone(), default, 490, 10),
                transfer(4, splitter, subaccount_one, 490, 10),
            ],
            SplitterFundingCredit {
                splitter_number: 50,
                tx_id: 4,
                amount_e8s: 490,
            },
        )
    }

    #[test]
    fn cumulative_floor_allocation_conserves_exactly() {
        assert_eq!(proportional_allocations(&[4, 6], 5).unwrap(), vec![2, 3]);
        assert_eq!(
            proportional_allocations(&[1, 1, 1], 2).unwrap(),
            vec![0, 1, 1]
        );
    }

    #[test]
    fn stateless_exact_anchor_reconstructs_original_funders_repeatedly() {
        let (history, anchor) = valid_history();
        let first = reconstruct_splitter_history(
            relay(),
            50,
            &history,
            std::slice::from_ref(&anchor),
            true,
        )
        .unwrap_complete();
        let second = reconstruct_splitter_history(
            relay(),
            50,
            &history,
            std::slice::from_ref(&anchor),
            true,
        )
        .unwrap_complete();
        assert_eq!(first, second);
        assert_eq!(
            first.sources,
            BTreeMap::from([([1; 32], 196), ([2; 32], 294)])
        );
        assert_eq!(first.ineligible_e8s, 0);
    }

    #[test]
    fn malformed_anchor_and_splitter_shape_fail_closed() {
        let (mut history, anchor) = valid_history();
        let IndexOperation::Transfer { amount, .. } = &mut history[3].transaction.operation else {
            unreachable!()
        };
        *amount = Tokens::new(489);
        assert!(
            reconstruct_splitter_history(relay(), 50, &history, &[anchor], true)
                .unwrap_malformed()
                .contains("subaccount_one_anchor_amount_mismatch")
        );
    }

    #[test]
    fn every_intrinsic_splitter_percentage_reconstructs_exact_anchor() {
        for number in logic::SPLITTER_PERCENTAGES {
            let (history, anchor) = one_source_job(number, [number; 32], 10_000, 10, 20, 1);
            let result = reconstruct_splitter_history(
                relay(),
                number,
                &history,
                std::slice::from_ref(&anchor),
                true,
            )
            .unwrap_complete();
            assert_eq!(
                result.sources,
                BTreeMap::from([([number; 32], anchor.amount_e8s)])
            );
        }
    }

    #[test]
    fn repeated_transfer_from_and_mint_sources_aggregate_without_eligible_dilution() {
        let (splitter, default, subaccount_one) = split_accounts(50);
        let history = vec![
            transfer(1, hex::encode([1; 32]), splitter.clone(), 400, 10),
            transfer_from(2, hex::encode([1; 32]), splitter.clone(), 300),
            mint(3, splitter.clone(), 300),
            transfer(4, splitter.clone(), default, 490, 10),
            transfer(5, splitter, subaccount_one, 490, 10),
        ];
        let result = reconstruct_splitter_history(
            relay(),
            50,
            &history,
            &[SplitterFundingCredit {
                splitter_number: 50,
                tx_id: 5,
                amount_e8s: 490,
            }],
            true,
        )
        .unwrap_complete();
        assert_eq!(result.sources, BTreeMap::from([([1; 32], 343)]));
        assert_eq!(result.ineligible_e8s, 147);
    }

    #[test]
    fn post_pin_credit_is_excluded_and_carried_into_the_next_splitter_job() {
        let (splitter, default, subaccount_one) = split_accounts(50);
        let history = vec![
            transfer(1, hex::encode([1; 32]), splitter.clone(), 1_000, 10),
            transfer(2, splitter.clone(), default.clone(), 490, 10),
            transfer(3, hex::encode([2; 32]), splitter.clone(), 300, 10),
            transfer(4, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(5, hex::encode([2; 32]), splitter.clone(), 700, 10),
            transfer(6, splitter.clone(), default, 490, 10),
            transfer(7, splitter, subaccount_one, 490, 10),
        ];
        let first = reconstruct_splitter_history(
            relay(),
            50,
            &history,
            &[SplitterFundingCredit {
                splitter_number: 50,
                tx_id: 4,
                amount_e8s: 490,
            }],
            true,
        )
        .unwrap_complete();
        assert_eq!(first.sources, BTreeMap::from([([1; 32], 490)]));
        let second = reconstruct_splitter_history(
            relay(),
            50,
            &history,
            &[SplitterFundingCredit {
                splitter_number: 50,
                tx_id: 7,
                amount_e8s: 490,
            }],
            true,
        )
        .unwrap_complete();
        assert_eq!(second.sources, BTreeMap::from([([2; 32], 490)]));
    }

    #[test]
    fn independent_historical_leg_fees_and_cumulative_floor_are_exact() {
        let (history, anchor) = one_source_job(50, [1; 32], 10_000, 100, 200, 1);
        let result = reconstruct_splitter_history(
            relay(),
            50,
            &history,
            std::slice::from_ref(&anchor),
            true,
        )
        .unwrap_complete();
        assert_eq!(result.sources, BTreeMap::from([([1; 32], 4_800)]));
        assert_eq!(proportional_allocations(&[3, 2], 3).unwrap(), vec![1, 2]);
        assert_eq!(
            proportional_allocations(&[u64::MAX - 1, 1], u64::MAX - 1).unwrap(),
            vec![u64::MAX - 2, 1]
        );
    }

    #[test]
    fn whole_credit_under_and_overshoot_need_older_then_fail_at_genesis() {
        let (splitter, default, subaccount_one) = split_accounts(50);
        for credited in [999, 1_001] {
            let history = vec![
                transfer(1, hex::encode([1; 32]), splitter.clone(), credited, 10),
                transfer(2, splitter.clone(), default.clone(), 490, 10),
                transfer(3, splitter.clone(), subaccount_one.clone(), 490, 10),
            ];
            let required = [SplitterFundingCredit {
                splitter_number: 50,
                tx_id: 3,
                amount_e8s: 490,
            }];
            assert_eq!(
                reconstruct_splitter_history(relay(), 50, &history, &required, false),
                reward_history::HistoricalReconstruction::NeedOlderHistory
            );
            assert!(
                reconstruct_splitter_history(relay(), 50, &history, &required, true)
                    .unwrap_malformed()
                    .contains(if credited > 1_000 {
                        "funding_credit_exceeds_pinned_remainder"
                    } else {
                        "missing_funding_credit"
                    })
            );
        }
    }

    #[test]
    fn malformed_splitter_debits_pairs_memos_destinations_and_anchors_fail_closed() {
        let (history, anchor) = one_source_job(50, [1; 32], 1_000, 10, 10, 1);
        let (splitter, default, subaccount_one) = split_accounts(50);
        let mut cases = Vec::new();
        cases.push((
            vec![transfer_from(1, splitter.clone(), default.clone(), 1)],
            "unsupported_transfer_from_debit",
        ));
        cases.push((
            vec![IndexTransactionWithId {
                id: 1,
                transaction: IndexTransaction {
                    memo: 0,
                    icrc1_memo: None,
                    operation: IndexOperation::Burn {
                        from: splitter.clone(),
                        amount: Tokens::new(1),
                        spender: None,
                    },
                    created_at_time: None,
                    timestamp: Some(IndexTimeStamp { timestamp_nanos: 1 }),
                },
            }],
            "unsupported_burn_debit",
        ));
        cases.push((
            vec![IndexTransactionWithId {
                id: 1,
                transaction: IndexTransaction {
                    memo: 0,
                    icrc1_memo: None,
                    operation: IndexOperation::Approve {
                        from: splitter.clone(),
                        spender: default.clone(),
                        allowance: Tokens::new(1),
                        fee: Tokens::new(1),
                        expires_at: None,
                        expected_allowance: None,
                    },
                    created_at_time: None,
                    timestamp: Some(IndexTimeStamp { timestamp_nanos: 1 }),
                },
            }],
            "unsupported_approve_fee_debit",
        ));
        for (case, class) in cases {
            assert!(
                reconstruct_splitter_history(relay(), 50, &case, &[anchor.clone()], true)
                    .unwrap_malformed()
                    .contains(class)
            );
        }

        let mut wrong_memo = history.clone();
        wrong_memo[1].transaction.memo = 1;
        assert!(
            reconstruct_splitter_history(relay(), 50, &wrong_memo, &[anchor.clone()], true)
                .unwrap_malformed()
                .contains("unsupported_outgoing_memo")
        );

        let mut wrong_destination = history.clone();
        if let IndexOperation::Transfer { to, .. } = &mut wrong_destination[1].transaction.operation
        {
            *to = hex::encode([9; 32]);
        }
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &wrong_destination,
            &[anchor.clone()],
            true,
        )
        .unwrap_malformed()
        .contains("unexpected_transfer_destination"));

        let reversed = vec![
            history[0].clone(),
            transfer(2, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(3, splitter.clone(), default.clone(), 490, 10),
        ];
        assert!(
            reconstruct_splitter_history(relay(), 50, &reversed, &[anchor.clone()], true)
                .unwrap_malformed()
                .contains("subaccount_one_leg_without_default")
        );

        let incomplete = vec![history[0].clone(), history[1].clone()];
        assert!(
            reconstruct_splitter_history(relay(), 50, &incomplete, &[anchor.clone()], true)
                .unwrap_malformed()
                .contains("incomplete_splitter_pair")
        );

        let mut mismatch = history.clone();
        if let IndexOperation::Transfer { amount, .. } = &mut mismatch[1].transaction.operation {
            *amount = Tokens::new(491);
        }
        assert!(
            reconstruct_splitter_history(relay(), 50, &mismatch, &[anchor.clone()], true)
                .unwrap_malformed()
                .contains("split_percentage_mismatch")
        );

        let wrong_id = SplitterFundingCredit {
            tx_id: 99,
            ..anchor.clone()
        };
        assert!(
            reconstruct_splitter_history(relay(), 50, &history, &[wrong_id], true)
                .unwrap_malformed()
                .contains("subaccount_one_anchor_not_found")
        );
        let wrong_amount = SplitterFundingCredit {
            amount_e8s: anchor.amount_e8s - 1,
            ..anchor
        };
        assert!(
            reconstruct_splitter_history(relay(), 50, &history, &[wrong_amount], true)
                .unwrap_malformed()
                .contains("subaccount_one_anchor_amount_mismatch")
        );
    }

    struct MockHistory {
        pages: Mutex<VecDeque<Result<GetAccountIdentifierTransactionsResponse, String>>>,
        requests: Mutex<Vec<Option<u64>>>,
    }

    #[async_trait]
    impl reward_history::RewardHistoryClient for MockHistory {
        async fn get_transactions(
            &self,
            _account_identifier: String,
            start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, String> {
            self.requests.lock().unwrap().push(start);
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected_history_call".to_string()))
        }
    }

    fn irrelevant(id: u64) -> IndexTransactionWithId {
        mint(id, "unrelated".to_string(), 1)
    }

    fn pages(
        mut chronological: Vec<IndexTransactionWithId>,
    ) -> VecDeque<Result<GetAccountIdentifierTransactionsResponse, String>> {
        chronological.sort_by_key(|entry| entry.id);
        let oldest = chronological.first().unwrap().id;
        chronological.reverse();
        chronological
            .chunks(reward_history::ICP_HISTORY_PAGE_SIZE as usize)
            .map(|chunk| {
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: chunk.to_vec(),
                    oldest_tx_id: Some(oldest),
                })
            })
            .collect()
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn splitter_balance_proof_rejects_locally_complete_but_misbound_suffix() {
        const PINNED: u64 = 1_000;
        let (splitter, default, subaccount_one) = split_accounts(50);
        let recent = vec![
            transfer(7, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(6, splitter.clone(), default.clone(), 490, 10),
            transfer(5, hex::encode([3; 32]), splitter.clone(), PINNED, 0),
            transfer(4, splitter.clone(), subaccount_one, 490, 10),
            transfer(3, splitter.clone(), default, 490, 10),
            transfer(2, hex::encode([2; 32]), splitter.clone(), PINNED, 0),
        ];
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: PINNED,
                    transactions: recent,
                    oldest_tx_id: Some(1),
                }),
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: PINNED,
                    transactions: vec![transfer(1, hex::encode([1; 32]), splitter, PINNED, 0)],
                    oldest_tx_id: Some(1),
                }),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        let anchor = SplitterFundingCredit {
            splitter_number: 50,
            tx_id: 7,
            amount_e8s: 490,
        };
        let mut history = reward_history::BackwardHistory::new(split_accounts(50).0);
        block_on(history.extend(&index)).unwrap();
        assert!(!history.authoritative());
        assert_eq!(
            reconstruct_splitter_history(
                relay(),
                50,
                history.authoritative_transactions(),
                std::slice::from_ref(&anchor),
                history.authoritative(),
            ),
            reward_history::HistoricalReconstruction::NeedOlderHistory
        );

        block_on(history.extend(&index)).unwrap();
        let provenance = reconstruct_splitter_history(
            relay(),
            50,
            history.authoritative_transactions(),
            &[anchor],
            history.authoritative(),
        )
        .unwrap_complete();
        assert_eq!(provenance.sources, BTreeMap::from([([2; 32], 490)]));
    }

    #[test]
    fn recent_splitter_anchor_does_not_read_more_than_ten_thousand_older_transactions() {
        let mut history = (1..=12_000).map(irrelevant).collect::<Vec<_>>();
        let (sentinel, _) = one_source_job(50, [8; 32], 1_000, 10, 10, 11_500);
        let (target, anchor) = one_source_job(50, [9; 32], 1_000, 10, 10, 11_800);
        for entry in sentinel.into_iter().chain(target) {
            let index = usize::try_from(entry.id - 1).unwrap();
            history[index] = entry;
        }
        let index = MockHistory {
            pages: Mutex::new(pages(history)),
            requests: Mutex::new(Vec::new()),
        };
        let mut cache = SplitterHistoryCache::new();
        let result =
            block_on(cache.expand(&index, relay(), std::slice::from_ref(&anchor))).unwrap();
        assert_eq!(result.sources, BTreeMap::from([([9; 32], 490)]));
        assert_eq!(result.scanned_transactions, 1_000);
        assert_eq!(index.requests.lock().unwrap().len(), 1);
        let repeated = block_on(cache.expand(&index, relay(), &[anchor])).unwrap();
        assert_eq!(repeated.sources, result.sources);
        assert_eq!(repeated.scanned_transactions, 0);
        assert_eq!(index.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn splitter_scanner_extends_across_several_carried_jobs_and_pages() {
        let mut history = (1..=5_000).map(irrelevant).collect::<Vec<_>>();
        let (splitter, default, subaccount_one) = split_accounts(50);
        let replacements = vec![
            transfer(100, hex::encode([1; 32]), splitter.clone(), 1_000, 10),
            transfer(900, splitter.clone(), default.clone(), 490, 10),
            transfer(901, hex::encode([2; 32]), splitter.clone(), 1_000, 10),
            transfer(902, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(1_900, splitter.clone(), default.clone(), 490, 10),
            transfer(1_901, hex::encode([3; 32]), splitter.clone(), 1_000, 10),
            transfer(1_902, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(2_900, splitter.clone(), default.clone(), 490, 10),
            transfer(2_901, hex::encode([4; 32]), splitter.clone(), 1_000, 10),
            transfer(2_902, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(3_900, splitter.clone(), default.clone(), 490, 10),
            transfer(3_901, hex::encode([5; 32]), splitter.clone(), 1_000, 10),
            transfer(3_902, splitter.clone(), subaccount_one.clone(), 490, 10),
            transfer(4_600, splitter.clone(), default, 490, 10),
            transfer(4_602, splitter, subaccount_one, 490, 10),
        ];
        for entry in replacements {
            let index = usize::try_from(entry.id - 1).unwrap();
            history[index] = entry;
        }
        let anchor = SplitterFundingCredit {
            splitter_number: 50,
            tx_id: 4_602,
            amount_e8s: 490,
        };
        let index = MockHistory {
            pages: Mutex::new(pages(history)),
            requests: Mutex::new(Vec::new()),
        };
        let mut cache = SplitterHistoryCache::new();
        let result = block_on(cache.expand(&index, relay(), &[anchor])).unwrap();
        assert_eq!(result.sources, BTreeMap::from([([5; 32], 490)]));
        assert_eq!(index.requests.lock().unwrap().len(), 5);
    }
}
