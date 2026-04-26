use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;

use crate::clients::index::{IndexOperation, IndexTimeStamp, IndexTransactionWithId};
use crate::state::{ActivePayoutJob, PendingNotification, Summary, TransferKind};

pub const MEMO_TOP_UP_CANISTER_U64: u64 = 1_347_768_404;
#[cfg(test)]
const MAX_TARGET_CANISTER_MEMO_BYTES: usize = jupiter_memo_policy::MAX_TARGET_CANISTER_MEMO_BYTES;

#[derive(Clone, Debug)]
pub struct Commitment {
    pub amount_e8s: u64,
    pub memo_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitmentValidity {
    IgnoreUnderThreshold,
    IgnoreBadMemo,
    Valid { beneficiary: Principal },
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitmentDecision {
    IgnoreUnderThreshold,
    IgnoreBadMemo,
    NoTransfer,
    Eligible { beneficiary: Principal, gross_share_e8s: u64, amount_e8s: u64 },
}

pub fn parse_beneficiary_from_memo(memo: &[u8]) -> Option<Principal> {
    jupiter_memo_policy::parse_target_canister_principal_from_memo(memo)
}

pub fn memo_bytes_from_index_tx(tx: &IndexTransactionWithId, staking_account_identifier: &str) -> Option<Commitment> {
    match &tx.transaction.operation {
        IndexOperation::Transfer { to, amount, .. } if to == staking_account_identifier => {
            let memo_bytes = tx
                .transaction
                .icrc1_memo
                .as_ref()
                .and_then(|icrc1_memo| (!icrc1_memo.is_empty()).then(|| icrc1_memo.clone()));
            Some(Commitment { amount_e8s: amount.e8s(), memo_bytes })
        }
        _ => None,
    }
}

pub fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    let bytes = principal.as_slice();
    let mut out = [0u8; 32];
    out[0] = bytes.len() as u8;
    let len = bytes.len().min(31);
    out[1..1 + len].copy_from_slice(&bytes[..len]);
    out
}

pub fn cmc_deposit_account(cmc_id: Principal, canister_id: Principal) -> Account {
    Account { owner: cmc_id, subaccount: Some(principal_to_subaccount(canister_id)) }
}

pub fn compute_raw_share_e8s(amount_e8s: u64, pot_start_e8s: u64, denom_e8s: u64) -> u64 {
    if pot_start_e8s == 0 || denom_e8s == 0 { return 0; }
    let raw = (amount_e8s as u128).saturating_mul(pot_start_e8s as u128).checked_div(denom_e8s as u128).unwrap_or(0);
    raw.min(u64::MAX as u128) as u64
}

pub fn classify_commitment(min_tx_e8s: u64, commitment: &Commitment) -> CommitmentValidity {
    if commitment.amount_e8s < min_tx_e8s { return CommitmentValidity::IgnoreUnderThreshold; }
    let memo = match commitment.memo_bytes.as_deref() { Some(m) if !m.is_empty() => m, _ => return CommitmentValidity::IgnoreBadMemo };
    let beneficiary = match parse_beneficiary_from_memo(memo) { Some(p) => p, None => return CommitmentValidity::IgnoreBadMemo };
    CommitmentValidity::Valid { beneficiary }
}

pub fn index_tx_timestamp_nanos(tx: &IndexTransactionWithId) -> Option<u64> {
    tx.transaction
        .timestamp
        .as_ref()
        .or(tx.transaction.created_at_time.as_ref())
        .map(|ts: &IndexTimeStamp| ts.timestamp_nanos)
}

pub fn conservative_effective_timestamp_nanos(tx_timestamp_nanos: u64, recognition_delay_seconds: u64) -> u64 {
    tx_timestamp_nanos.saturating_add(recognition_delay_seconds.saturating_mul(1_000_000_000))
}

pub fn compute_weighted_amount_e8s(
    raw_amount_e8s: u64,
    round_start_time_nanos: u64,
    round_end_time_nanos: u64,
    effective_timestamp_nanos: u64,
) -> u64 {
    if raw_amount_e8s == 0 || round_end_time_nanos <= round_start_time_nanos {
        return 0;
    }
    if effective_timestamp_nanos <= round_start_time_nanos {
        return raw_amount_e8s;
    }
    if effective_timestamp_nanos >= round_end_time_nanos {
        return 0;
    }
    let duration = (round_end_time_nanos - round_start_time_nanos) as u128;
    let participating = (round_end_time_nanos - effective_timestamp_nanos) as u128;
    let weighted = (raw_amount_e8s as u128).saturating_mul(participating).checked_div(duration).unwrap_or(0);
    weighted.min(u64::MAX as u128) as u64
}

pub fn commitment_amount_for_round_e8s(
    commitment: &Commitment,
    tx_id: u64,
    tx_timestamp_nanos: Option<u64>,
    round_start_latest_tx_id: Option<u64>,
    round_end_latest_tx_id: Option<u64>,
    round_start_time_nanos: u64,
    round_end_time_nanos: u64,
    recognition_delay_seconds: u64,
) -> Option<u64> {
    if round_end_latest_tx_id.map(|end| tx_id > end).unwrap_or(false) {
        return None;
    }
    // Legacy payout jobs and the one-off transition job do not yet have a meaningful
    // accumulation window. In those cases we must preserve the historical behavior and
    // treat in-range commitments as fully eligible for the round instead of letting a
    // zero-width window collapse their weighted amount to zero.
    if round_end_time_nanos <= round_start_time_nanos {
        return Some(commitment.amount_e8s);
    }
    if round_start_latest_tx_id.map(|start| tx_id <= start).unwrap_or(false) {
        return Some(commitment.amount_e8s);
    }
    let effective_timestamp_nanos = conservative_effective_timestamp_nanos(
        tx_timestamp_nanos.unwrap_or(round_end_time_nanos),
        recognition_delay_seconds,
    );
    Some(compute_weighted_amount_e8s(
        commitment.amount_e8s,
        round_start_time_nanos,
        round_end_time_nanos,
        effective_timestamp_nanos,
    ))
}

#[allow(dead_code)]
pub fn evaluate_commitment(pot_start_e8s: u64, denom_e8s: u64, fee_e8s: u64, min_tx_e8s: u64, commitment: &Commitment) -> CommitmentDecision {
    let beneficiary = match classify_commitment(min_tx_e8s, commitment) {
        CommitmentValidity::IgnoreUnderThreshold => return CommitmentDecision::IgnoreUnderThreshold,
        CommitmentValidity::IgnoreBadMemo => return CommitmentDecision::IgnoreBadMemo,
        CommitmentValidity::Valid { beneficiary } => beneficiary,
    };
    let gross_share_e8s = compute_raw_share_e8s(commitment.amount_e8s, pot_start_e8s, denom_e8s);
    if gross_share_e8s <= fee_e8s { return CommitmentDecision::NoTransfer; }
    CommitmentDecision::Eligible { beneficiary, gross_share_e8s, amount_e8s: gross_share_e8s.saturating_sub(fee_e8s) }
}

pub fn record_ledger_accepted_transfer(job: &mut ActivePayoutJob, pending: &PendingNotification) {
    job.gross_outflow_e8s = job.gross_outflow_e8s.saturating_add(pending.gross_share_e8s);
}

pub fn apply_notified_transfer(job: &mut ActivePayoutJob, pending: &PendingNotification) {
    match pending.kind {
        TransferKind::Beneficiary => {
            job.topped_up_count = job.topped_up_count.saturating_add(1);
            job.topped_up_sum_e8s = job.topped_up_sum_e8s.saturating_add(pending.amount_e8s);
            job.topped_up_min_e8s = Some(job.topped_up_min_e8s.map(|prev| prev.min(pending.amount_e8s)).unwrap_or(pending.amount_e8s));
            job.topped_up_max_e8s = Some(job.topped_up_max_e8s.map(|prev| prev.max(pending.amount_e8s)).unwrap_or(pending.amount_e8s));
        }
        TransferKind::RemainderToSelf => job.remainder_to_self_e8s = pending.amount_e8s,
    }
}

pub fn summary_from_job(job: &ActivePayoutJob) -> Summary {
    Summary {
        pot_start_e8s: job.pot_start_e8s,
        pot_remaining_e8s: job.pot_start_e8s.saturating_sub(job.gross_outflow_e8s),
        denom_staking_balance_e8s: job.denom_staking_balance_e8s,
        effective_denom_staking_balance_e8s: job.effective_denom_staking_balance_e8s,
        topped_up_count: job.topped_up_count,
        topped_up_sum_e8s: job.topped_up_sum_e8s,
        topped_up_min_e8s: job.topped_up_min_e8s,
        topped_up_max_e8s: job.topped_up_max_e8s,
        failed_topups: job.failed_topups,
        ambiguous_topups: job.ambiguous_topups,
        ignored_under_threshold: job.ignored_under_threshold,
        ignored_bad_memo: job.ignored_bad_memo,
        remainder_to_self_e8s: job.remainder_to_self_e8s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::index::{IndexOperation, IndexTransaction, IndexTransactionWithId, Tokens};
    use crate::state::{ActivePayoutJob, PendingNotification, TransferKind};

    fn principal(s: &str) -> Principal { Principal::from_text(s).unwrap() }
    fn target_canister() -> Principal { principal("22255-zqaaa-aaaas-qf6uq-cai") }

    #[test]
    fn classify_commitment_distinguishes_valid_threshold_and_bad_memo() {
        let beneficiary = target_canister();
        assert_eq!(
            classify_commitment(100, &Commitment { amount_e8s: 99, memo_bytes: Some(beneficiary.to_text().into_bytes()) }),
            CommitmentValidity::IgnoreUnderThreshold
        );
        assert_eq!(
            classify_commitment(100, &Commitment { amount_e8s: 100, memo_bytes: Some(b"bad".to_vec()) }),
            CommitmentValidity::IgnoreBadMemo
        );
        assert_eq!(
            classify_commitment(100, &Commitment { amount_e8s: 100, memo_bytes: Some(beneficiary.to_text().into_bytes()) }),
            CommitmentValidity::Valid { beneficiary }
        );
    }

    #[test]
    fn weighted_amount_is_full_partial_or_zero_based_on_effective_time() {
        assert_eq!(compute_weighted_amount_e8s(1_000, 0, 100, 0), 1_000);
        assert_eq!(compute_weighted_amount_e8s(1_000, 0, 100, 25), 750);
        assert_eq!(compute_weighted_amount_e8s(1_000, 0, 100, 100), 0);
        assert_eq!(compute_weighted_amount_e8s(1_000, 0, 100, 150), 0);
    }

    #[test]
    fn commitment_amount_for_round_uses_tx_id_boundaries_and_conservative_delay() {
        let commitment = Commitment { amount_e8s: 1_000, memo_bytes: Some(target_canister().to_text().into_bytes()) };
        let round_end_nanos = 100_000_000_000;
        assert_eq!(
            commitment_amount_for_round_e8s(&commitment, 10, Some(5_000_000_000), Some(20), Some(50), 0, round_end_nanos, 10),
            Some(1_000),
        );
        assert_eq!(
            commitment_amount_for_round_e8s(&commitment, 25, Some(20_000_000_000), Some(20), Some(50), 0, round_end_nanos, 10),
            Some(700),
        );
        assert_eq!(
            commitment_amount_for_round_e8s(&commitment, 55, Some(20_000_000_000), Some(20), Some(50), 0, round_end_nanos, 10),
            None,
        );
        assert_eq!(
            commitment_amount_for_round_e8s(&commitment, 30, None, Some(20), Some(50), 0, round_end_nanos, 10),
            Some(0),
        );
    }

    #[test]
    fn parser_accepts_target_canister_text_memo() {
        let p = target_canister();
        assert_eq!(parse_beneficiary_from_memo(p.to_text().as_bytes()), Some(p));
    }

    #[test]
    fn parser_rejects_oversize_self_authenticating_principal_text() {
        let p = Principal::from_text("33mql-r6bnm-7mzbp-gqvmp-iv6qr-5j3pw-tnwsf-f2az7-zppun-yb4lf-zae").unwrap();
        assert!(p.to_text().len() > MAX_TARGET_CANISTER_MEMO_BYTES);
        assert_eq!(parse_beneficiary_from_memo(p.to_text().as_bytes()), None);
    }

    #[test]
    fn parser_rejects_malformed_or_adversarial_memos() {
        let p = target_canister();
        assert_eq!(parse_beneficiary_from_memo(b"not-a-principal"), None);
        assert_eq!(parse_beneficiary_from_memo(b""), None);
        assert_eq!(parse_beneficiary_from_memo(&p.as_slice()[..p.as_slice().len().saturating_sub(1)]), None);
        assert_eq!(parse_beneficiary_from_memo(&vec![0xff; 64]), None);
    }

    #[test]
    fn parser_rejects_anonymous_and_management_canister_principals() {
        assert_eq!(parse_beneficiary_from_memo(Principal::anonymous().to_text().as_bytes()), None);
        assert_eq!(parse_beneficiary_from_memo(Principal::management_canister().to_text().as_bytes()), None);
    }

    #[test]
    fn parser_accepts_whitespace_padded_target_canister_text_memo() {
        let p = target_canister();
        let memo = format!("  {}\n", p.to_text());
        assert_eq!(parse_beneficiary_from_memo(memo.as_bytes()), Some(p));
    }

    #[test]
    fn parser_does_not_hardcode_a_cai_suffix() {
        let p = principal("uuc56-gyb");
        assert!(p.to_text().len() <= MAX_TARGET_CANISTER_MEMO_BYTES);
        assert_eq!(parse_beneficiary_from_memo(p.to_text().as_bytes()), Some(p));
    }

    #[test]
    fn empty_icrc1_memo_is_ignored() {
        let staking = "staking-account".to_string();
        let tx = IndexTransactionWithId {
            id: 7,
            transaction: IndexTransaction {
                memo: u64::from_be_bytes(*b"aaaaa-aa"),
                icrc1_memo: Some(vec![]),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "sender".to_string(),
                    amount: Tokens::new(123),
                    spender: None,
                },
                created_at_time: None,
                timestamp: None,
            },
        };
        let commitment = memo_bytes_from_index_tx(&tx, &staking).expect("matching transfer should be surfaced");
        assert_eq!(commitment.memo_bytes, None);
    }

    #[test]
    fn missing_icrc1_memo_does_not_consider_legacy_numeric_memo() {
        let staking = "staking-account".to_string();
        let tx = IndexTransactionWithId {
            id: 70,
            transaction: IndexTransaction {
                memo: u64::from_be_bytes(*b"aaaaa-aa"),
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "sender".to_string(),
                    amount: Tokens::new(123),
                    spender: None,
                },
                created_at_time: None,
                timestamp: None,
            },
        };
        let commitment = memo_bytes_from_index_tx(&tx, &staking).expect("matching transfer should be surfaced");
        assert_eq!(commitment.memo_bytes, None);
    }

    #[test]
    fn transfer_from_transactions_are_not_treated_as_commitments() {
        let staking = "staking-account".to_string();
        let tx = IndexTransactionWithId {
            id: 9,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(target_canister().to_text().into_bytes()),
                operation: IndexOperation::TransferFrom {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "sender".to_string(),
                    amount: Tokens::new(123),
                    spender: "spender".to_string(),
                },
                created_at_time: None,
                timestamp: None,
            },
        };
        assert!(memo_bytes_from_index_tx(&tx, &staking).is_none());
    }

    #[test]
    fn principal_subaccount_matches_documented_layout() {
        let p = principal("uuc56-gyb");
        let sub = principal_to_subaccount(p);
        assert_eq!(sub[0], p.as_slice().len() as u8);
        assert_eq!(&sub[1..1 + p.as_slice().len()], p.as_slice());
        assert!(sub[1 + p.as_slice().len()..].iter().all(|b| *b == 0));
    }

    #[test]
    fn commitment_below_threshold_is_ignored() {
        let valid = target_canister();
        let c = Commitment { amount_e8s: 99_999_999, memo_bytes: Some(valid.to_text().into_bytes()) };
        assert_eq!(evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &c), CommitmentDecision::IgnoreUnderThreshold);
    }

    #[test]
    fn commitment_exactly_at_threshold_is_included() {
        let valid = target_canister();
        let c = Commitment { amount_e8s: 100_000_000, memo_bytes: Some(valid.to_text().into_bytes()) };
        assert_eq!(evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &c), CommitmentDecision::Eligible { beneficiary: valid, gross_share_e8s: 50_000_000, amount_e8s: 49_990_000 });
    }

    #[test]
    fn commitment_above_threshold_is_included() {
        let valid = target_canister();
        let c = Commitment { amount_e8s: 400_000_000, memo_bytes: Some(valid.to_text().into_bytes()) };
        assert_eq!(evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &c), CommitmentDecision::Eligible { beneficiary: valid, gross_share_e8s: 200_000_000, amount_e8s: 199_990_000 });
    }

    #[test]
    fn share_calculation_uses_current_pot_and_denominator() {
        let valid = target_canister();
        let c = Commitment { amount_e8s: 250_000_000, memo_bytes: Some(valid.to_text().into_bytes()) };
        assert_eq!(evaluate_commitment(120_000_000, 600_000_000, 10_000, 100_000_000, &c), CommitmentDecision::Eligible { beneficiary: valid, gross_share_e8s: 50_000_000, amount_e8s: 49_990_000 });
    }

    #[test]
    fn evaluate_commitment_counts_bad_and_missing_memos() {
        let missing = Commitment { amount_e8s: 200_000_000, memo_bytes: None };
        let bad = Commitment { amount_e8s: 300_000_000, memo_bytes: Some(b"bad-memo".to_vec()) };
        assert_eq!(evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &missing), CommitmentDecision::IgnoreBadMemo);
        assert_eq!(evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &bad), CommitmentDecision::IgnoreBadMemo);
    }

    #[test]
    fn separate_commitments_for_same_beneficiary_remain_separate() {
        let beneficiary = target_canister();
        let first = Commitment { amount_e8s: 200_000_000, memo_bytes: Some(beneficiary.to_text().into_bytes()) };
        let second = Commitment { amount_e8s: 300_000_000, memo_bytes: Some(beneficiary.to_text().into_bytes()) };
        let first_eval = evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &first);
        let second_eval = evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &second);
        assert_eq!(first_eval, CommitmentDecision::Eligible { beneficiary, gross_share_e8s: 100_000_000, amount_e8s: 99_990_000 });
        assert_eq!(second_eval, CommitmentDecision::Eligible { beneficiary, gross_share_e8s: 150_000_000, amount_e8s: 149_990_000 });
    }

    #[test]
    fn distinct_beneficiaries_with_same_commitment_size_are_processed_independently() {
        let a = target_canister();
        let b = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let amount_e8s = 200_000_000;
        let eval_a = evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &Commitment { amount_e8s, memo_bytes: Some(a.to_text().into_bytes()) });
        let eval_b = evaluate_commitment(500_000_000, 1_000_000_000, 10_000, 100_000_000, &Commitment { amount_e8s, memo_bytes: Some(b.to_text().into_bytes()) });
        assert_eq!(eval_a, CommitmentDecision::Eligible { beneficiary: a, gross_share_e8s: 100_000_000, amount_e8s: 99_990_000 });
        assert_eq!(eval_b, CommitmentDecision::Eligible { beneficiary: b, gross_share_e8s: 100_000_000, amount_e8s: 99_990_000 });
    }

    #[test]
    fn rounding_behavior_is_deterministic() {
        assert_eq!(compute_raw_share_e8s(1, 1, 3), 0);
        assert_eq!(compute_raw_share_e8s(2, 5, 3), 3);
        assert_eq!(compute_raw_share_e8s(2, 5, 3), compute_raw_share_e8s(2, 5, 3));
        assert_eq!(compute_raw_share_e8s(333_333_333, 100_000_000, 1_000_000_000), 33_333_333);
    }

    #[test]
    fn proportional_pot_and_denominator_scaling_preserves_absolute_shares() {
        let commitment = 400_000_000_u64;
        let initial = compute_raw_share_e8s(commitment, 100_000_000, 1_000_000_000);
        let scaled = compute_raw_share_e8s(commitment, 150_000_000, 1_500_000_000);
        assert_eq!(initial, 40_000_000);
        assert_eq!(scaled, initial,
            "when added stake produces proportionally larger base maturity, later payout pots preserve the same absolute beneficiary share");
    }

    #[test]
    fn raw_share_math_shows_why_an_unweighted_live_denominator_would_skew_one_round() {
        let commitment = 400_000_000_u64;
        let steady_state = compute_raw_share_e8s(commitment, 100_000_000, 1_000_000_000);
        let skewed_round = compute_raw_share_e8s(commitment, 100_000_000, 1_500_000_000);
        let reequilibrated_round = compute_raw_share_e8s(commitment, 150_000_000, 1_500_000_000);

        assert_eq!(steady_state, 40_000_000);
        assert_eq!(skewed_round, 26_666_666,
            "if the staking-balance denominator grows before the payout pot catches up, that single round can temporarily underpay relative to steady state");
        assert_eq!(reequilibrated_round, steady_state,
            "once the payout pot scales with the larger stake base, the beneficiary re-equilibrates to the original absolute share");
    }

    #[test]
    fn no_transfer_when_share_rounds_below_fee() {
        let beneficiary = target_canister();
        let c = Commitment { amount_e8s: 100_000_000, memo_bytes: Some(beneficiary.to_text().into_bytes()) };
        assert_eq!(evaluate_commitment(10_000, 1_000_000_000, 10_000, 100_000_000, &c), CommitmentDecision::NoTransfer);
    }

    #[test]
    fn summary_tracks_separate_same_beneficiary_commitments_and_remainder_without_aggregation() {
        let a = target_canister();
        let self_id = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let mut job = ActivePayoutJob::new(1, 10_000, 40_0000_0000, 10_0000_0000, 123);
        let p1 = PendingNotification { kind: TransferKind::Beneficiary, beneficiary: a, gross_share_e8s: 4_0000_0000, amount_e8s: 3_9999_0000, block_index: 1, next_start: Some(10) };
        let p2 = PendingNotification { kind: TransferKind::Beneficiary, beneficiary: a, gross_share_e8s: 12_0000_0000, amount_e8s: 11_9999_0000, block_index: 2, next_start: Some(11) };
        let p3 = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: 24_0000_0000, amount_e8s: 23_9999_0000, block_index: 3, next_start: None };
        record_ledger_accepted_transfer(&mut job, &p1);
        apply_notified_transfer(&mut job, &p1);
        record_ledger_accepted_transfer(&mut job, &p2);
        apply_notified_transfer(&mut job, &p2);
        record_ledger_accepted_transfer(&mut job, &p3);
        apply_notified_transfer(&mut job, &p3);
        let summary = summary_from_job(&job);
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.remainder_to_self_e8s, 23_9999_0000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn summary_accounting_reconciles_pot_fees_and_remainder() {
        let beneficiary = target_canister();
        let mut job = ActivePayoutJob::new(9, 10_000, 100_000_000, 500_000_000, 77);
        let p1 = PendingNotification { kind: TransferKind::Beneficiary, beneficiary, gross_share_e8s: 40_000_000, amount_e8s: 39_990_000, block_index: 1, next_start: Some(1) };
        let p2 = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary, gross_share_e8s: 60_000_000, amount_e8s: 59_990_000, block_index: 2, next_start: None };
        record_ledger_accepted_transfer(&mut job, &p1);
        apply_notified_transfer(&mut job, &p1);
        record_ledger_accepted_transfer(&mut job, &p2);
        apply_notified_transfer(&mut job, &p2);
        let summary = summary_from_job(&job);
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 39_990_000);
        assert_eq!(summary.remainder_to_self_e8s, 59_990_000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn same_beneficiary_with_identical_memo_bytes_is_evaluated_as_distinct_commitments() {
        let beneficiary = target_canister();
        let memo = Some(beneficiary.to_text().into_bytes());
        let first = Commitment { amount_e8s: 125_000_000, memo_bytes: memo.clone() };
        let second = Commitment { amount_e8s: 125_000_000, memo_bytes: memo };
        let first_eval = evaluate_commitment(200_000_000, 500_000_000, 10_000, 100_000_000, &first);
        let second_eval = evaluate_commitment(200_000_000, 500_000_000, 10_000, 100_000_000, &second);
        assert_eq!(first_eval, CommitmentDecision::Eligible { beneficiary, gross_share_e8s: 50_000_000, amount_e8s: 49_990_000 });
        assert_eq!(second_eval, CommitmentDecision::Eligible { beneficiary, gross_share_e8s: 50_000_000, amount_e8s: 49_990_000 });
    }

    #[test]
    fn zero_pot_or_zero_denominator_never_produces_a_transfer() {
        let beneficiary = target_canister();
        let commitment = Commitment { amount_e8s: 100_000_000, memo_bytes: Some(beneficiary.to_text().into_bytes()) };
        assert_eq!(evaluate_commitment(0, 500_000_000, 10_000, 100_000_000, &commitment), CommitmentDecision::NoTransfer);
        assert_eq!(evaluate_commitment(50_000_000, 0, 10_000, 100_000_000, &commitment), CommitmentDecision::NoTransfer);
    }


    #[test]
    fn accepted_but_unnotified_transfer_still_reduces_remaining_pot() {
        let beneficiary = target_canister();
        let mut job = ActivePayoutJob::new(12, 10_000, 90_000_000, 200_000_000, 1);
        let pending = PendingNotification {
            kind: TransferKind::Beneficiary,
            beneficiary,
            gross_share_e8s: 30_000_000,
            amount_e8s: 29_990_000,
            block_index: 77,
            next_start: Some(5),
        };
        record_ledger_accepted_transfer(&mut job, &pending);
        let summary = summary_from_job(&job);
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 60_000_000);
    }


    #[test]
    fn pot_conservation_holds_across_mixed_outcomes() {
        fn lcg(seed: &mut u64) -> u64 {
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            *seed
        }

        let mut seed = 0x5eed_u64;
        for case in 0..64_u64 {
            let pot_start_e8s = 100_000_000 + (lcg(&mut seed) % 900_000_000);
            let denom_staking_balance_e8s = 1 + (lcg(&mut seed) % 1_000_000_000);
            let fee_e8s = 10_000;
            let mut job = ActivePayoutJob::new(case + 1, fee_e8s, pot_start_e8s, denom_staking_balance_e8s, case + 1);
            let self_id = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");

            let beneficiaries = [
                target_canister(),
                principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
                principal("r7inp-6aaaa-aaaaa-aaabq-cai"),
                principal("qjdve-lqaaa-aaaaa-aaaeq-cai"),
                principal("qoctq-giaaa-aaaaa-aaaea-cai"),
                principal("rdmx6-jaaaa-aaaaa-aaadq-cai"),
            ];
            let commitment_count = 1 + (lcg(&mut seed) % 6) as usize;
            for i in 0..commitment_count {
                let beneficiary = beneficiaries[i % beneficiaries.len()];
                let commitment = Commitment {
                    amount_e8s: 1 + (lcg(&mut seed) % 500_000_000),
                    memo_bytes: Some(beneficiary.to_text().into_bytes()),
                };
                if let CommitmentDecision::Eligible { beneficiary, gross_share_e8s, amount_e8s } =
                    evaluate_commitment(pot_start_e8s, denom_staking_balance_e8s, fee_e8s, 1, &commitment)
                {
                    if job.gross_outflow_e8s.saturating_add(gross_share_e8s) > job.pot_start_e8s {
                        job.failed_topups = job.failed_topups.saturating_add(1);
                        continue;
                    }
                    let pending = PendingNotification {
                        kind: TransferKind::Beneficiary,
                        beneficiary,
                        gross_share_e8s,
                        amount_e8s,
                        block_index: i as u64,
                        next_start: Some(i as u64),
                    };
                    record_ledger_accepted_transfer(&mut job, &pending);
                    if lcg(&mut seed) & 1 == 0 {
                        apply_notified_transfer(&mut job, &pending);
                    } else {
                        job.failed_topups = job.failed_topups.saturating_add(1);
                    }
                }
            }

            let remainder_gross_e8s = job.pot_start_e8s.saturating_sub(job.gross_outflow_e8s);
            if remainder_gross_e8s > fee_e8s {
                let remainder = PendingNotification {
                    kind: TransferKind::RemainderToSelf,
                    beneficiary: self_id,
                    gross_share_e8s: remainder_gross_e8s,
                    amount_e8s: remainder_gross_e8s.saturating_sub(fee_e8s),
                    block_index: 10_000 + case,
                    next_start: None,
                };
                record_ledger_accepted_transfer(&mut job, &remainder);
                apply_notified_transfer(&mut job, &remainder);
            }

            let summary = summary_from_job(&job);
            assert_eq!(summary.pot_start_e8s, pot_start_e8s);
            assert_eq!(job.gross_outflow_e8s.saturating_add(summary.pot_remaining_e8s), pot_start_e8s,
                "accepted gross outflow plus remaining pot should conserve the pot even across mixed notify outcomes");
            if summary.remainder_to_self_e8s > 0 {
                assert_eq!(summary.pot_remaining_e8s, 0, "once a remainder transfer is sent there should be no pot left in-state");
            }
            assert!(summary.pot_remaining_e8s <= fee_e8s || summary.remainder_to_self_e8s == 0,
                "only fee-sized dust may remain undistributed before a remainder transfer is recorded");
            assert!(summary.topped_up_sum_e8s <= job.gross_outflow_e8s,
                "notified beneficiary net payouts cannot exceed accepted gross outflow");
            assert!(summary.remainder_to_self_e8s <= pot_start_e8s);
        }
    }

    #[test]
    fn empty_completed_job_summary_keeps_full_pot_remaining_until_remainder_is_sent() {
        let job = ActivePayoutJob::new(11, 10_000, 70_000_000, 200_000_000, 1);
        let summary = summary_from_job(&job);
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.remainder_to_self_e8s, 0);
        assert_eq!(summary.pot_remaining_e8s, 70_000_000);
    }
}
