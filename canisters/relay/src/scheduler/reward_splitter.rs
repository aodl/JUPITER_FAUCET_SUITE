use std::collections::{BTreeMap, BTreeSet, VecDeque};

use candid::Principal;
use jupiter_ic_clients::account_identifier::{account_identifier_bytes, account_identifier_text};
use jupiter_ic_clients::index::{IndexOperation, IndexTransactionWithId};

use super::reward_history;
use crate::logic;
use crate::reward_state::RewardHistoryBoundary;

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
    pub boundary_updates: BTreeMap<u8, RewardHistoryBoundary>,
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
    splitter_number: u8,
    anchor_tx_id: u64,
    balance_e8s: u64,
    subaccount_one_amount_e8s: u64,
    funding: &mut VecDeque<UpstreamCredit>,
    sources: &mut BTreeMap<[u8; 32], u64>,
    ineligible_e8s: &mut u64,
) -> Result<(), String> {
    let mut remaining = balance_e8s;
    let mut consumed = Vec::new();
    while remaining > 0 {
        let credit = funding.pop_front().ok_or_else(|| {
            provenance_error(
                splitter_number,
                Some(anchor_tx_id),
                "missing_funding_credit",
            )
        })?;
        if credit.amount_e8s > remaining {
            return Err(provenance_error(
                splitter_number,
                Some(credit.tx_id),
                "funding_credit_exceeds_pinned_remainder",
            ));
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
    .map_err(|class| provenance_error(splitter_number, Some(anchor_tx_id), &class))?;
    for (credit, allocated) in consumed.into_iter().zip(allocations) {
        if let Some(source) = credit.source {
            add_checked(sources.entry(source).or_default(), allocated)?;
        } else {
            add_checked(ineligible_e8s, allocated)?;
        }
    }
    Ok(())
}

pub(super) fn reconstruct_splitter_history(
    relay: Principal,
    splitter_number: u8,
    transactions: &[IndexTransactionWithId],
    prior_boundary: RewardHistoryBoundary,
    required_credits: &[SplitterFundingCredit],
) -> Result<ExpandedSplitterProvenance, String> {
    if logic::splitter_percentage(splitter_number).is_none()
        || required_credits
            .iter()
            .any(|credit| credit.splitter_number != splitter_number)
    {
        return Err(provenance_error(
            splitter_number,
            None,
            "invalid_splitter_number",
        ));
    }
    let mut required = BTreeMap::<u64, u64>::new();
    for credit in required_credits {
        if required.insert(credit.tx_id, credit.amount_e8s).is_some() {
            return Err(provenance_error(
                splitter_number,
                Some(credit.tx_id),
                "duplicate_subaccount_one_anchor",
            ));
        }
    }
    let Some(last_anchor) = required.keys().next_back().copied() else {
        return Ok(ExpandedSplitterProvenance::default());
    };
    if prior_boundary
        .processed_through_tx_id
        .is_some_and(|cursor| last_anchor <= cursor)
    {
        return Err(provenance_error(
            splitter_number,
            Some(last_anchor),
            "anchor_not_after_cursor",
        ));
    }

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
        if let Some(cursor) = prior_boundary.processed_through_tx_id {
            if entry.id == cursor {
                continue;
            }
            if entry.id < cursor {
                if prior_boundary
                    .carried_credit_start_tx_id
                    .is_some_and(|carried| entry.id >= carried)
                {
                    // Historical overlap restores funding only. Its outgoing legs were
                    // already adjudicated with the prior second-leg cursor.
                    push_incoming_credit(&mut funding, &splitter_account, &entry);
                }
                continue;
            }
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
                    return Err(provenance_error(
                        splitter_number,
                        Some(entry.id),
                        "unsupported_outgoing_memo",
                    ));
                }
                let leg = OutgoingLeg {
                    tx_id: entry.id,
                    amount_e8s: amount.e8s(),
                    fee_e8s: fee.e8s(),
                };
                if to == &default_account {
                    if pending_default.replace(leg).is_some() {
                        return Err(provenance_error(
                            splitter_number,
                            Some(entry.id),
                            "default_leg_while_pair_incomplete",
                        ));
                    }
                } else if to == &subaccount_one {
                    let default = pending_default.take().ok_or_else(|| {
                        provenance_error(
                            splitter_number,
                            Some(entry.id),
                            "subaccount_one_leg_without_default",
                        )
                    })?;
                    let expected_amount = required.remove(&entry.id);
                    let bootstrapping_past_pre_fix_job = expected_amount.is_none()
                        && prior_boundary.processed_through_tx_id.is_none();
                    if expected_amount.is_none() && !bootstrapping_past_pre_fix_job {
                        return Err(provenance_error(
                            splitter_number,
                            Some(entry.id),
                            "unconsumed_subaccount_one_leg",
                        ));
                    }
                    if expected_amount.is_some_and(|expected| expected != leg.amount_e8s) {
                        return Err(provenance_error(
                            splitter_number,
                            Some(entry.id),
                            "subaccount_one_anchor_amount_mismatch",
                        ));
                    }
                    if default.tx_id >= leg.tx_id || default.amount_e8s == 0 || leg.amount_e8s == 0
                    {
                        return Err(provenance_error(
                            splitter_number,
                            Some(entry.id),
                            "invalid_splitter_leg_pair",
                        ));
                    }
                    let default_gross = default
                        .amount_e8s
                        .checked_add(default.fee_e8s)
                        .ok_or_else(|| {
                            provenance_error(
                                splitter_number,
                                Some(default.tx_id),
                                "default_gross_overflow",
                            )
                        })?;
                    let subaccount_one_gross =
                        leg.amount_e8s.checked_add(leg.fee_e8s).ok_or_else(|| {
                            provenance_error(
                                splitter_number,
                                Some(entry.id),
                                "subaccount_one_gross_overflow",
                            )
                        })?;
                    let balance =
                        default_gross
                            .checked_add(subaccount_one_gross)
                            .ok_or_else(|| {
                                provenance_error(
                                    splitter_number,
                                    Some(entry.id),
                                    "pinned_balance_overflow",
                                )
                            })?;
                    let expected_default =
                        (u128::from(balance) * u128::from(splitter_number) / 100) as u64;
                    if default_gross != expected_default
                        || subaccount_one_gross != balance - expected_default
                    {
                        return Err(provenance_error(
                            splitter_number,
                            Some(entry.id),
                            "split_percentage_mismatch",
                        ));
                    }
                    if bootstrapping_past_pre_fix_job {
                        let mut discarded_sources = BTreeMap::new();
                        let mut discarded_ineligible = 0;
                        consume_job_funding(
                            splitter_number,
                            entry.id,
                            balance,
                            leg.amount_e8s,
                            &mut funding,
                            &mut discarded_sources,
                            &mut discarded_ineligible,
                        )?;
                    } else {
                        consume_job_funding(
                            splitter_number,
                            entry.id,
                            balance,
                            leg.amount_e8s,
                            &mut funding,
                            &mut result.sources,
                            &mut result.ineligible_e8s,
                        )?;
                    }
                } else {
                    return Err(provenance_error(
                        splitter_number,
                        Some(entry.id),
                        "unexpected_transfer_destination",
                    ));
                }
            }
            IndexOperation::TransferFrom { from, .. } if from == &splitter_account => {
                return Err(provenance_error(
                    splitter_number,
                    Some(entry.id),
                    "unsupported_transfer_from_debit",
                ));
            }
            IndexOperation::Burn { from, .. } if from == &splitter_account => {
                return Err(provenance_error(
                    splitter_number,
                    Some(entry.id),
                    "unsupported_burn_debit",
                ));
            }
            IndexOperation::Approve { from, fee, .. }
                if from == &splitter_account && fee.e8s() > 0 =>
            {
                return Err(provenance_error(
                    splitter_number,
                    Some(entry.id),
                    "unsupported_approve_fee_debit",
                ));
            }
            _ => push_incoming_credit(&mut funding, &splitter_account, &entry),
        }
    }
    if let Some(default) = pending_default {
        return Err(provenance_error(
            splitter_number,
            Some(default.tx_id),
            "incomplete_splitter_pair",
        ));
    }
    if let Some((&missing, _)) = required.first_key_value() {
        return Err(provenance_error(
            splitter_number,
            Some(missing),
            "subaccount_one_anchor_not_found",
        ));
    }
    result.boundary_updates.insert(
        splitter_number,
        RewardHistoryBoundary {
            processed_through_tx_id: Some(last_anchor),
            carried_credit_start_tx_id: funding
                .front()
                .and_then(|credit| (credit.tx_id < last_anchor).then_some(credit.tx_id)),
        },
    );
    Ok(result)
}

pub(super) async fn expand_splitter_provenance(
    index_id: Principal,
    relay: Principal,
    credits: &[SplitterFundingCredit],
    boundaries: &BTreeMap<u8, RewardHistoryBoundary>,
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
        let prior = boundaries
            .get(&splitter_number)
            .copied()
            .unwrap_or_default();
        let account_identifier = account_identifier_text(
            relay,
            Some(logic::relay_numbered_subaccount(splitter_number)),
        );
        let history = reward_history::scan_history(
            index_id,
            account_identifier,
            prior.processed_through_tx_id,
            prior.carried_credit_start_tx_id,
        )
        .await
        .map_err(|class| provenance_error(splitter_number, None, &class))?;
        let one = reconstruct_splitter_history(
            relay,
            splitter_number,
            &history,
            prior,
            &credits
                .iter()
                .filter(|credit| credit.splitter_number == splitter_number)
                .cloned()
                .collect::<Vec<_>>(),
        )?;
        expanded.scanned_transactions = expanded
            .scanned_transactions
            .checked_add(history.len())
            .ok_or_else(|| "splitter_history_count_overflow".to_string())?;
        expanded.splitters_scanned += 1;
        for (source, amount) in one.sources {
            add_checked(expanded.sources.entry(source).or_default(), amount)?;
        }
        add_checked(&mut expanded.ineligible_e8s, one.ineligible_e8s)?;
        expanded.boundary_updates.extend(one.boundary_updates);
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_ic_clients::index::{IndexTimeStamp, IndexTransaction, Tokens};

    fn relay() -> Principal {
        Principal::from_slice(&[42])
    }

    fn source(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn tx(id: u64, operation: IndexOperation, memo: Option<Vec<u8>>) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: memo,
                operation,
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: id,
                }),
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: id,
                }),
            },
        }
    }

    fn transfer(
        id: u64,
        from: String,
        to: String,
        amount: u64,
        fee: u64,
    ) -> IndexTransactionWithId {
        tx(
            id,
            IndexOperation::Transfer {
                from,
                to,
                amount: Tokens::new(amount),
                fee: Tokens::new(fee),
                spender: None,
            },
            None,
        )
    }

    fn transfer_from(id: u64, from: String, to: String, amount: u64) -> IndexTransactionWithId {
        tx(
            id,
            IndexOperation::TransferFrom {
                from,
                to,
                amount: Tokens::new(amount),
                fee: Tokens::new(10),
                spender: source(99),
            },
            None,
        )
    }

    fn split_accounts(number: u8) -> (String, String, String) {
        (
            account_identifier_text(relay(), Some(logic::relay_numbered_subaccount(number))),
            account_identifier_text(relay(), None),
            account_identifier_text(relay(), Some(logic::relay_subaccount_one())),
        )
    }

    fn valid_history(
        number: u8,
        credits: &[(u64, String, u64)],
        balance: u64,
        fee: u64,
        default_tx: u64,
        second_tx: u64,
    ) -> (Vec<IndexTransactionWithId>, SplitterFundingCredit) {
        let (splitter, default, subaccount_one) = split_accounts(number);
        let default_gross = (u128::from(balance) * u128::from(number) / 100) as u64;
        let second_gross = balance - default_gross;
        let mut history = credits
            .iter()
            .map(|(id, from, amount)| transfer(*id, from.clone(), splitter.clone(), *amount, 10))
            .collect::<Vec<_>>();
        history.push(transfer(
            default_tx,
            splitter.clone(),
            default,
            default_gross - fee,
            fee,
        ));
        history.push(transfer(
            second_tx,
            splitter,
            subaccount_one,
            second_gross - fee,
            fee,
        ));
        (
            history,
            SplitterFundingCredit {
                splitter_number: number,
                tx_id: second_tx,
                amount_e8s: second_gross - fee,
            },
        )
    }

    #[test]
    fn cumulative_floor_allocation_is_exact_deterministic_and_wide() {
        assert_eq!(proportional_allocations(&[100], 50).unwrap(), vec![50]);
        assert_eq!(
            proportional_allocations(&[50, 50], 49).unwrap(),
            vec![24, 25]
        );
        assert_eq!(proportional_allocations(&[3, 2], 3).unwrap(), vec![1, 2]);
        assert_eq!(
            proportional_allocations(&[1, 1, 1], 2).unwrap(),
            vec![0, 1, 1]
        );
        assert_eq!(
            proportional_allocations(&[u64::MAX - 1, 1], u64::MAX - 1).unwrap(),
            vec![u64::MAX - 2, 1]
        );
        for credits in [
            vec![1],
            vec![1, 2],
            vec![3, 5, 8],
            vec![10, 10, 10, 10],
            vec![u32::MAX as u64, u32::MAX as u64],
        ] {
            let balance = credits.iter().sum::<u64>();
            for total in [0, 1, balance / 3, balance] {
                let allocations = proportional_allocations(&credits, total).unwrap();
                assert_eq!(allocations.iter().sum::<u64>(), total);
                assert!(allocations
                    .iter()
                    .zip(&credits)
                    .all(|(allocated, credit)| allocated <= credit));
                assert_eq!(
                    proportional_allocations(&credits, total).unwrap(),
                    allocations
                );
            }
        }
    }

    #[test]
    fn all_nine_intrinsic_accounts_reconstruct_exact_net_second_leg() {
        let accounts = intrinsic_splitter_accounts(relay());
        assert_eq!(accounts.len(), 9);
        for number in logic::SPLITTER_PERCENTAGES {
            let (history, anchor) = valid_history(
                number,
                &[(1, source(number), 1_000_000)],
                1_000_000,
                10_000,
                2,
                3,
            );
            let result = reconstruct_splitter_history(
                relay(),
                number,
                &history,
                RewardHistoryBoundary::default(),
                std::slice::from_ref(&anchor),
            )
            .unwrap();
            assert_eq!(result.sources[&[number; 32]], anchor.amount_e8s);
            assert_eq!(result.ineligible_e8s, 0);
            assert_eq!(
                result.boundary_updates[&number].processed_through_tx_id,
                Some(3)
            );
            assert_eq!(
                accounts[&account_identifier_bytes(
                    relay(),
                    Some(logic::relay_numbered_subaccount(number))
                )],
                number
            );
        }
    }

    #[test]
    fn multiple_repeated_transfer_from_and_mint_sources_allocate_and_aggregate() {
        let number = 50;
        let (splitter, default, subaccount_one) = split_accounts(number);
        let mut history = vec![
            transfer(1, source(1), splitter.clone(), 300, 10),
            transfer_from(2, source(2), splitter.clone(), 200),
            transfer(3, source(1), splitter.clone(), 100, 10),
            tx(
                4,
                IndexOperation::Mint {
                    to: splitter.clone(),
                    amount: Tokens::new(400),
                },
                None,
            ),
        ];
        history.push(transfer(5, splitter.clone(), default, 490, 10));
        history.push(transfer(6, splitter, subaccount_one, 490, 10));
        let result = reconstruct_splitter_history(
            relay(),
            number,
            &history,
            RewardHistoryBoundary::default(),
            &[SplitterFundingCredit {
                splitter_number: number,
                tx_id: 6,
                amount_e8s: 490,
            }],
        )
        .unwrap();
        assert_eq!(result.sources[&[1; 32]], 196);
        assert_eq!(result.sources[&[2; 32]], 98);
        assert_eq!(result.ineligible_e8s, 196);
        assert_eq!(
            result.sources.values().sum::<u64>() + result.ineligible_e8s,
            490
        );
    }

    #[test]
    fn deposits_after_pin_are_carried_across_historical_outgoing_overlap() {
        for deposit_between_legs in [false, true] {
            let number = 50;
            let (splitter, default, subaccount_one) = split_accounts(number);
            let (default_id, carry_id) = if deposit_between_legs { (2, 3) } else { (3, 2) };
            let history = vec![
                transfer(1, source(1), splitter.clone(), 1_000, 10),
                transfer(carry_id, source(2), splitter.clone(), 30, 10),
                transfer(default_id, splitter.clone(), default.clone(), 490, 10),
                transfer(4, splitter.clone(), subaccount_one.clone(), 490, 10),
            ];
            let first = reconstruct_splitter_history(
                relay(),
                number,
                &history,
                RewardHistoryBoundary::default(),
                &[SplitterFundingCredit {
                    splitter_number: number,
                    tx_id: 4,
                    amount_e8s: 490,
                }],
            )
            .unwrap();
            assert_eq!(first.sources, BTreeMap::from([([1; 32], 490)]));
            assert_eq!(
                first.boundary_updates[&number].carried_credit_start_tx_id,
                Some(carry_id)
            );

            let next_history = vec![
                history
                    .iter()
                    .find(|entry| entry.id == carry_id)
                    .unwrap()
                    .clone(),
                history
                    .iter()
                    .find(|entry| entry.id == default_id)
                    .unwrap()
                    .clone(),
                transfer(5, splitter.clone(), default.clone(), 14, 1),
                transfer(6, splitter.clone(), subaccount_one.clone(), 14, 1),
            ];
            let second = reconstruct_splitter_history(
                relay(),
                number,
                &next_history,
                first.boundary_updates[&number],
                &[SplitterFundingCredit {
                    splitter_number: number,
                    tx_id: 6,
                    amount_e8s: 14,
                }],
            )
            .unwrap();
            assert_eq!(second.sources, BTreeMap::from([([2; 32], 14)]));
            assert_eq!(
                second.boundary_updates[&number],
                RewardHistoryBoundary {
                    processed_through_tx_id: Some(6),
                    carried_credit_start_tx_id: None,
                }
            );
        }
    }

    #[test]
    fn empty_v1_migration_boundary_skips_valid_pre_fix_jobs_without_re_attribution() {
        let number = 50;
        let (splitter, default, subaccount_one) = split_accounts(number);
        let history = vec![
            transfer(1, source(1), splitter.clone(), 1_000, 10),
            transfer(2, splitter.clone(), default.clone(), 490, 10),
            transfer(3, splitter.clone(), subaccount_one.clone(), 480, 20),
            transfer(4, source(2), splitter.clone(), 30, 10),
            transfer(5, splitter.clone(), default, 14, 1),
            transfer(6, splitter, subaccount_one, 14, 1),
        ];
        let result = reconstruct_splitter_history(
            relay(),
            number,
            &history,
            RewardHistoryBoundary::default(),
            &[SplitterFundingCredit {
                splitter_number: number,
                tx_id: 6,
                amount_e8s: 14,
            }],
        )
        .unwrap();
        assert_eq!(result.sources, BTreeMap::from([([2; 32], 14)]));
        assert!(!result.sources.contains_key(&[1; 32]));
        assert_eq!(
            result.boundary_updates[&number],
            RewardHistoryBoundary {
                processed_through_tx_id: Some(6),
                carried_credit_start_tx_id: None,
            }
        );
    }

    #[test]
    fn unequal_historical_leg_fees_reconstruct_each_gross_and_preserve_second_leg_net() {
        let number = 30;
        let balance = 500_000_007;
        let default_fee = 10_000;
        let subaccount_one_fee = 20_000;
        let default_gross = (u128::from(balance) * u128::from(number) / 100) as u64;
        let subaccount_one_gross = balance - default_gross;
        let default_amount = default_gross - default_fee;
        let subaccount_one_amount = subaccount_one_gross - subaccount_one_fee;
        let (splitter, default, subaccount_one) = split_accounts(number);
        let history = vec![
            transfer(1, source(1), splitter.clone(), balance, default_fee),
            transfer(2, splitter.clone(), default, default_amount, default_fee),
            transfer(
                3,
                splitter,
                subaccount_one,
                subaccount_one_amount,
                subaccount_one_fee,
            ),
        ];
        let result = reconstruct_splitter_history(
            relay(),
            number,
            &history,
            RewardHistoryBoundary::default(),
            &[SplitterFundingCredit {
                splitter_number: number,
                tx_id: 3,
                amount_e8s: subaccount_one_amount,
            }],
        )
        .unwrap();

        assert_eq!(
            result.sources,
            BTreeMap::from([([1; 32], subaccount_one_amount)])
        );
        assert_eq!(result.ineligible_e8s, 0);
        assert_eq!(
            result.sources.values().sum::<u64>() + result.ineligible_e8s,
            subaccount_one_amount
        );
        assert_eq!(
            result.boundary_updates[&number],
            RewardHistoryBoundary {
                processed_through_tx_id: Some(3),
                carried_credit_start_tx_id: None,
            }
        );
    }

    #[test]
    fn exact_whole_credit_fifo_rejects_missing_and_overshooting_funding() {
        let (history, anchor) = valid_history(50, &[(1, source(1), 1_001)], 1_000, 10, 2, 3);
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &history,
            RewardHistoryBoundary::default(),
            &[anchor]
        )
        .unwrap_err()
        .contains("funding_credit_exceeds_pinned_remainder"));

        let (history, anchor) = valid_history(50, &[], 1_000, 10, 2, 3);
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &history,
            RewardHistoryBoundary::default(),
            &[anchor]
        )
        .unwrap_err()
        .contains("missing_funding_credit"));
    }

    #[test]
    fn malformed_and_unsupported_splitter_debits_fail_closed() {
        let number = 50;
        let (splitter, default, subaccount_one) = split_accounts(number);
        let anchor = SplitterFundingCredit {
            splitter_number: number,
            tx_id: 3,
            amount_e8s: 490,
        };
        let base_credit = transfer(1, source(1), splitter.clone(), 1_000, 10);
        let cases = [
            (
                transfer(2, splitter.clone(), source(9), 490, 10),
                "unexpected_transfer_destination",
            ),
            (
                transfer_from(2, splitter.clone(), default.clone(), 490),
                "unsupported_transfer_from_debit",
            ),
            (
                tx(
                    2,
                    IndexOperation::Burn {
                        from: splitter.clone(),
                        amount: Tokens::new(10),
                        spender: None,
                    },
                    None,
                ),
                "unsupported_burn_debit",
            ),
            (
                tx(
                    2,
                    IndexOperation::Approve {
                        from: splitter.clone(),
                        fee: Tokens::new(10),
                        allowance: Tokens::new(1),
                        expires_at: None,
                        spender: source(9),
                        expected_allowance: None,
                    },
                    None,
                ),
                "unsupported_approve_fee_debit",
            ),
        ];
        for (bad, reason) in cases {
            let error = reconstruct_splitter_history(
                relay(),
                number,
                &[base_credit.clone(), bad],
                RewardHistoryBoundary::default(),
                std::slice::from_ref(&anchor),
            )
            .unwrap_err();
            assert!(error.contains(reason), "{error}");
        }

        let second_first = vec![
            base_credit.clone(),
            transfer(3, splitter.clone(), subaccount_one.clone(), 490, 10),
        ];
        assert!(reconstruct_splitter_history(
            relay(),
            number,
            &second_first,
            RewardHistoryBoundary::default(),
            std::slice::from_ref(&anchor)
        )
        .unwrap_err()
        .contains("subaccount_one_leg_without_default"));

        let two_defaults = vec![
            base_credit,
            transfer(2, splitter.clone(), default.clone(), 490, 10),
            transfer(3, splitter.clone(), default, 490, 10),
        ];
        assert!(reconstruct_splitter_history(
            relay(),
            number,
            &two_defaults,
            RewardHistoryBoundary::default(),
            std::slice::from_ref(&anchor)
        )
        .unwrap_err()
        .contains("default_leg_while_pair_incomplete"));
    }

    #[test]
    fn percentage_anchor_id_anchor_amount_and_memo_are_strict() {
        let (history, anchor) = valid_history(50, &[(1, source(1), 1_000)], 1_000, 10, 2, 3);
        let mut wrong_percentage = history.clone();
        if let IndexOperation::Transfer { amount, .. } =
            &mut wrong_percentage[1].transaction.operation
        {
            *amount = Tokens::new(480);
        }
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &wrong_percentage,
            RewardHistoryBoundary::default(),
            std::slice::from_ref(&anchor)
        )
        .unwrap_err()
        .contains("split_percentage_mismatch"));

        let mut memo = history.clone();
        memo[1].transaction.icrc1_memo = Some(vec![1]);
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &memo,
            RewardHistoryBoundary::default(),
            std::slice::from_ref(&anchor)
        )
        .unwrap_err()
        .contains("unsupported_outgoing_memo"));

        let wrong_amount = SplitterFundingCredit {
            amount_e8s: anchor.amount_e8s - 1,
            ..anchor.clone()
        };
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &history,
            RewardHistoryBoundary::default(),
            &[wrong_amount]
        )
        .unwrap_err()
        .contains("subaccount_one_anchor_amount_mismatch"));

        let wrong_id = SplitterFundingCredit { tx_id: 9, ..anchor };
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &history,
            RewardHistoryBoundary::default(),
            &[wrong_id]
        )
        .unwrap_err()
        .contains("subaccount_one_anchor_not_found"));
    }

    #[test]
    fn valid_second_leg_fee_repin_succeeds_but_fee_only_gross_mutation_fails() {
        let (history, anchor) = valid_history(50, &[(1, source(1), 1_000)], 1_000, 10, 2, 3);

        let mut repinned = history.clone();
        if let IndexOperation::Transfer { amount, fee, .. } = &mut repinned[2].transaction.operation
        {
            *amount = Tokens::new(480);
            *fee = Tokens::new(20);
        }
        let repinned_anchor = SplitterFundingCredit {
            amount_e8s: 480,
            ..anchor.clone()
        };
        let result = reconstruct_splitter_history(
            relay(),
            50,
            &repinned,
            RewardHistoryBoundary::default(),
            &[repinned_anchor],
        )
        .unwrap();
        assert_eq!(result.sources, BTreeMap::from([([1; 32], 480)]));
        assert_eq!(result.ineligible_e8s, 0);
        assert_eq!(
            result.boundary_updates[&50].processed_through_tx_id,
            Some(3)
        );

        let mut invalid_gross = history;
        if let IndexOperation::Transfer { fee, .. } = &mut invalid_gross[2].transaction.operation {
            *fee = Tokens::new(20);
        }
        assert!(reconstruct_splitter_history(
            relay(),
            50,
            &invalid_gross,
            RewardHistoryBoundary::default(),
            std::slice::from_ref(&anchor),
        )
        .unwrap_err()
        .contains("split_percentage_mismatch"));
    }
}
