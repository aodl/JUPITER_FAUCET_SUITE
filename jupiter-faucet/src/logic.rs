use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;

use crate::clients::index::{IndexOperation, IndexTransactionWithId};
use crate::state::{ActivePayoutJob, PendingNotification, Summary, TransferKind};

pub const MEMO_TOP_UP_CANISTER_U64: u64 = 1_347_768_404;

#[derive(Clone, Debug)]
pub struct Contribution {
    pub amount_e8s: u64,
    pub memo_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContributionDecision {
    IgnoreUnderThreshold,
    IgnoreBadMemo,
    NoTransfer,
    Eligible { beneficiary: Principal, gross_share_e8s: u64, amount_e8s: u64 },
}

pub fn parse_beneficiary_from_memo(memo: &[u8]) -> Option<Principal> {
    std::str::from_utf8(memo).ok().and_then(|s| Principal::from_text(s.trim()).ok())
}

pub fn memo_bytes_from_index_tx(tx: &IndexTransactionWithId, staking_account_identifier: &str) -> Option<Contribution> {
    match &tx.transaction.operation {
        IndexOperation::Transfer { to, amount, .. } if to == staking_account_identifier => {
            let memo_bytes = if let Some(icrc1_memo) = &tx.transaction.icrc1_memo {
                if icrc1_memo.is_empty() { None } else { Some(icrc1_memo.clone()) }
            } else if tx.transaction.memo == 0 {
                None
            } else {
                Some(tx.transaction.memo.to_be_bytes().to_vec())
            };
            Some(Contribution { amount_e8s: amount.e8s(), memo_bytes })
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

pub fn evaluate_contribution(pot_start_e8s: u64, denom_e8s: u64, fee_e8s: u64, min_tx_e8s: u64, contribution: &Contribution) -> ContributionDecision {
    if contribution.amount_e8s < min_tx_e8s { return ContributionDecision::IgnoreUnderThreshold; }
    let memo = match contribution.memo_bytes.as_deref() { Some(m) if !m.is_empty() => m, _ => return ContributionDecision::IgnoreBadMemo };
    let beneficiary = match parse_beneficiary_from_memo(memo) { Some(p) => p, None => return ContributionDecision::IgnoreBadMemo };
    let gross_share_e8s = compute_raw_share_e8s(contribution.amount_e8s, pot_start_e8s, denom_e8s);
    if gross_share_e8s <= fee_e8s { return ContributionDecision::NoTransfer; }
    ContributionDecision::Eligible { beneficiary, gross_share_e8s, amount_e8s: gross_share_e8s.saturating_sub(fee_e8s) }
}

pub fn apply_notified_transfer(job: &mut ActivePayoutJob, pending: &PendingNotification) {
    job.pending_notification = None;
    job.next_start = pending.next_start;
    match pending.kind {
        TransferKind::Beneficiary => {
            job.beneficiary_gross_sent_sum_e8s = job.beneficiary_gross_sent_sum_e8s.saturating_add(pending.gross_share_e8s);
            job.topped_up_count = job.topped_up_count.saturating_add(1);
            job.topped_up_sum_e8s = job.topped_up_sum_e8s.saturating_add(pending.amount_e8s);
            job.topped_up_min_e8s = Some(job.topped_up_min_e8s.map(|prev| prev.min(pending.amount_e8s)).unwrap_or(pending.amount_e8s));
            job.topped_up_max_e8s = Some(job.topped_up_max_e8s.map(|prev| prev.max(pending.amount_e8s)).unwrap_or(pending.amount_e8s));
        }
        TransferKind::RemainderToSelf => job.remainder_to_self_e8s = pending.amount_e8s,
    }
}

pub fn summary_from_job(job: &ActivePayoutJob) -> Summary {
    let remainder_gross = if job.remainder_to_self_e8s == 0 { 0 } else { job.remainder_to_self_e8s.saturating_add(job.fee_e8s) };
    let gross_sent = job.beneficiary_gross_sent_sum_e8s.saturating_add(remainder_gross);
    Summary {
        pot_start_e8s: job.pot_start_e8s,
        pot_remaining_e8s: job.pot_start_e8s.saturating_sub(gross_sent),
        denom_staking_balance_e8s: job.denom_staking_balance_e8s,
        topped_up_count: job.topped_up_count,
        topped_up_sum_e8s: job.topped_up_sum_e8s,
        topped_up_min_e8s: job.topped_up_min_e8s,
        topped_up_max_e8s: job.topped_up_max_e8s,
        failed_topups: job.failed_topups,
        ignored_under_threshold: job.ignored_under_threshold,
        ignored_bad_memo: job.ignored_bad_memo,
        remainder_to_self_e8s: job.remainder_to_self_e8s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ActivePayoutJob, PendingNotification, TransferKind};

    fn principal(s: &str) -> Principal { Principal::from_text(s).unwrap() }

    #[test]
    fn parser_accepts_raw_principal_text_only() {
        let p = principal("aaaaa-aa");
        assert_eq!(parse_beneficiary_from_memo(p.to_text().as_bytes()), Some(p));
        assert_eq!(parse_beneficiary_from_memo(b"not-a-principal"), None);
        assert_eq!(parse_beneficiary_from_memo(b""), None);
    }

    #[test]
    fn principal_subaccount_matches_documented_layout() {
        let p = principal("aaaaa-aa");
        let sub = principal_to_subaccount(p);
        assert_eq!(sub[0], p.as_slice().len() as u8);
        assert_eq!(&sub[1..1 + p.as_slice().len()], p.as_slice());
        assert!(sub[1 + p.as_slice().len()..].iter().all(|b| *b == 0));
    }

    #[test]
    fn evaluate_contribution_preserves_streaming_semantics() {
        let valid = principal("aaaaa-aa");
        let c = Contribution { amount_e8s: 400_000_000, memo_bytes: Some(valid.to_text().into_bytes()) };
        assert_eq!(evaluate_contribution(500_000_000, 1_000_000_000, 10_000, 10_000_000, &c), ContributionDecision::Eligible { beneficiary: valid, gross_share_e8s: 200_000_000, amount_e8s: 199_990_000 });
    }

    #[test]
    fn evaluate_contribution_counts_bad_and_small_memos() {
        let valid = principal("aaaaa-aa");
        let small = Contribution { amount_e8s: 5_000_000, memo_bytes: Some(valid.to_text().into_bytes()) };
        let missing = Contribution { amount_e8s: 200_000_000, memo_bytes: None };
        let bad = Contribution { amount_e8s: 300_000_000, memo_bytes: Some(b"bad-memo".to_vec()) };
        assert_eq!(evaluate_contribution(500_000_000, 1_000_000_000, 10_000, 10_000_000, &small), ContributionDecision::IgnoreUnderThreshold);
        assert_eq!(evaluate_contribution(500_000_000, 1_000_000_000, 10_000, 10_000_000, &missing), ContributionDecision::IgnoreBadMemo);
        assert_eq!(evaluate_contribution(500_000_000, 1_000_000_000, 10_000, 10_000_000, &bad), ContributionDecision::IgnoreBadMemo);
    }

    #[test]
    fn summary_tracks_beneficiaries_and_remainder_without_buffering_transfers() {
        let a = principal("aaaaa-aa");
        let self_id = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let mut job = ActivePayoutJob::new(1, 10_000, 40_0000_0000, 10_0000_0000, 123);
        apply_notified_transfer(&mut job, &PendingNotification { kind: TransferKind::Beneficiary, beneficiary: a, gross_share_e8s: 4_0000_0000, amount_e8s: 3_9999_0000, block_index: 1, next_start: Some(10) });
        apply_notified_transfer(&mut job, &PendingNotification { kind: TransferKind::Beneficiary, beneficiary: a, gross_share_e8s: 12_0000_0000, amount_e8s: 11_9999_0000, block_index: 2, next_start: Some(11) });
        apply_notified_transfer(&mut job, &PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: 24_0000_0000, amount_e8s: 23_9999_0000, block_index: 3, next_start: None });
        let summary = summary_from_job(&job);
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.remainder_to_self_e8s, 23_9999_0000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }
}
