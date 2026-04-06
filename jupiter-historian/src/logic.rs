use candid::Principal;

pub const INVALID_MEMO_PLACEHOLDER: &str = "invalid target canister memo";
pub const MAX_TARGET_CANISTER_MEMO_BYTES: usize = 32;
use std::collections::BTreeSet;

use crate::clients::index::{IndexOperation, IndexTransactionWithId};
use crate::state::{CanisterMeta, CanisterSource, ContributionSample, CyclesProbeResult, CyclesSample, CyclesSampleSource};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedContribution {
    pub tx_id: u64,
    pub beneficiary: Principal,
    pub amount_e8s: u64,
    pub timestamp_nanos: Option<u64>,
    pub counts_toward_faucet: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedInvalidContribution {
    pub tx_id: u64,
    pub amount_e8s: u64,
    pub timestamp_nanos: Option<u64>,
    pub memo_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexedContributionEntry {
    Valid(IndexedContribution),
    Invalid(IndexedInvalidContribution),
}


pub fn memo_text_from_bytes(bytes: &[u8]) -> Option<String> {
    let memo_text = std::str::from_utf8(bytes).ok()?.trim().to_string();
    if memo_text.is_empty() {
        return None;
    }
    Some(memo_text)
}

pub fn parse_target_canister_from_memo(bytes: &[u8]) -> Option<Principal> {
    if bytes.is_empty() || bytes.len() > MAX_TARGET_CANISTER_MEMO_BYTES || !bytes.is_ascii() {
        return None;
    }
    let memo_text = memo_text_from_bytes(bytes)?;
    if memo_text.len() > MAX_TARGET_CANISTER_MEMO_BYTES {
        return None;
    }
    let principal = Principal::from_text(&memo_text).ok()?;
    if principal == Principal::anonymous() || principal == Principal::management_canister() {
        return None;
    }
    Some(principal)
}

pub fn memo_bytes_from_index_tx(tx: &IndexTransactionWithId, staking_account_id: &str) -> Option<(u64, Option<Vec<u8>>, u64, Option<u64>)> {
    match &tx.transaction.operation {
        IndexOperation::Transfer { to, amount, .. } if to == staking_account_id => {
            let memo = tx
                .transaction
                .icrc1_memo
                .as_ref()
                .and_then(|bytes| (!bytes.is_empty()).then(|| bytes.clone()));
            let ts = tx.transaction.timestamp.as_ref().map(|ts| ts.timestamp_nanos)
                .or_else(|| tx.transaction.created_at_time.as_ref().map(|ts| ts.timestamp_nanos));
            Some((tx.id, memo, amount.e8s(), ts))
        }
        _ => None,
    }
}

pub fn indexed_contribution_from_tx(tx: &IndexTransactionWithId, staking_account_id: &str, min_tx_e8s: u64) -> Option<IndexedContributionEntry> {
    let (tx_id, memo_opt, amount_e8s, timestamp_nanos) = memo_bytes_from_index_tx(tx, staking_account_id)?;
    let memo = memo_opt?;
    memo_text_from_bytes(&memo)?;
    if let Some(beneficiary) = parse_target_canister_from_memo(&memo) {
        Some(IndexedContributionEntry::Valid(IndexedContribution {
            tx_id,
            beneficiary,
            amount_e8s,
            timestamp_nanos,
            counts_toward_faucet: amount_e8s >= min_tx_e8s,
        }))
    } else {
        Some(IndexedContributionEntry::Invalid(IndexedInvalidContribution {
            tx_id,
            amount_e8s,
            timestamp_nanos,
            memo_text: INVALID_MEMO_PLACEHOLDER.to_string(),
        }))
    }
}

pub fn merge_sources(existing: Option<&BTreeSet<CanisterSource>>, add: CanisterSource) -> BTreeSet<CanisterSource> {
    let mut out = existing.cloned().unwrap_or_default();
    out.insert(add);
    out
}

pub fn push_contribution(history: &mut Vec<ContributionSample>, sample: ContributionSample, max_entries: u32) -> bool {
    if history.iter().any(|existing| existing.tx_id == sample.tx_id) {
        return false;
    }
    history.push(sample);
    prune_vec(history, max_entries);
    true
}

pub fn push_cycles_sample(history: &mut Vec<CyclesSample>, sample: CyclesSample, max_entries: u32) -> bool {
    if history.last().map(|existing| existing.timestamp_nanos == sample.timestamp_nanos).unwrap_or(false) {
        return false;
    }
    history.push(sample);
    prune_vec(history, max_entries);
    true
}

fn prune_vec<T>(history: &mut Vec<T>, max_entries: u32) {
    let max_entries = max_entries as usize;
    if max_entries == 0 {
        history.clear();
        return;
    }
    if history.len() > max_entries {
        let excess = history.len() - max_entries;
        history.drain(0..excess);
    }
}

pub fn apply_cycles_probe_result(meta: &mut CanisterMeta, timestamp_nanos: u64, result: CyclesProbeResult) {
    meta.last_cycles_probe_ts = Some(timestamp_nanos / 1_000_000_000);
    meta.last_cycles_probe_result = Some(result);
}

pub fn apply_contribution_seen(meta: &mut CanisterMeta, timestamp_nanos: Option<u64>, now_secs: u64) {
    if meta.first_seen_ts.is_none() {
        meta.first_seen_ts = Some(timestamp_nanos.map(|ts| ts / 1_000_000_000).unwrap_or(now_secs));
    }
    meta.last_contribution_ts = Some(timestamp_nanos.map(|ts| ts / 1_000_000_000).unwrap_or(now_secs));
}

pub fn should_skip_blackhole_for_sources(sources: &BTreeSet<CanisterSource>) -> bool {
    sources.contains(&CanisterSource::SnsDiscovery)
}

pub fn make_cycles_sample(timestamp_nanos: u64, cycles: u128, source: CyclesSampleSource) -> CyclesSample {
    CyclesSample { timestamp_nanos, cycles, source }
}


pub fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    let bytes = principal.as_slice();
    let mut out = [0u8; 32];
    out[0] = bytes.len() as u8;
    let len = bytes.len().min(31);
    out[1..1 + len].copy_from_slice(&bytes[..len]);
    out
}

pub fn cmc_deposit_account(cmc_id: Principal, canister_id: Principal) -> icrc_ledger_types::icrc1::account::Account {
    icrc_ledger_types::icrc1::account::Account {
        owner: cmc_id,
        subaccount: Some(principal_to_subaccount(canister_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::index::{IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId, Tokens};

    fn principal(s: &str) -> Principal { Principal::from_text(s).unwrap() }
    fn target_canister() -> Principal { principal("22255-zqaaa-aaaas-qf6uq-cai") }

    #[test]
    fn parses_target_canister_memo() {
        let p = target_canister();
        assert_eq!(parse_target_canister_from_memo(p.to_text().as_bytes()), Some(p));
        assert_eq!(parse_target_canister_from_memo(format!("  {}\n", p.to_text()).as_bytes()), Some(p));
        assert_eq!(parse_target_canister_from_memo(b"bad"), None);
    }

    #[test]
    fn rejects_oversize_principal_text_memos_but_not_short_valid_principals() {
        let self_auth = Principal::from_text("33mql-r6bnm-7mzbp-gqvmp-iv6qr-5j3pw-tnwsf-f2az7-zppun-yb4lf-zae").unwrap();
        assert!(self_auth.to_text().len() > MAX_TARGET_CANISTER_MEMO_BYTES);
        assert_eq!(parse_target_canister_from_memo(self_auth.to_text().as_bytes()), None);
        let short = target_canister();
        assert!(short.to_text().len() <= MAX_TARGET_CANISTER_MEMO_BYTES);
        assert_eq!(parse_target_canister_from_memo(short.to_text().as_bytes()), Some(short));
    }

    #[test]
    fn rejects_anonymous_and_management_canister_principals() {
        assert_eq!(parse_target_canister_from_memo(Principal::anonymous().to_text().as_bytes()), None);
        assert_eq!(parse_target_canister_from_memo(Principal::management_canister().to_text().as_bytes()), None);
    }

    #[test]
    fn indexed_contribution_uses_icrc1_memo_and_threshold_flag() {
        let staking = "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let beneficiary = target_canister();
        let tx = IndexTransactionWithId {
            id: 1,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(beneficiary.to_text().into_bytes()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(50),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos: 99 }),
            },
        };
        let c = indexed_contribution_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedContributionEntry::Valid(c) => {
                assert_eq!(c.beneficiary, beneficiary);
                assert!(!c.counts_toward_faucet);
                assert_eq!(c.timestamp_nanos, Some(99));
            }
            IndexedContributionEntry::Invalid(_) => panic!("expected valid contribution"),
        }
    }


    #[test]
    fn missing_icrc1_memo_is_ignored_even_when_legacy_numeric_memo_is_set() {
        let staking = "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 21,
            transaction: IndexTransaction {
                memo: u64::from_be_bytes(*b"aaaaa-aa"),
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(50),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos: 99 }),
            },
        };
        let c = indexed_contribution_from_tx(&tx, &staking, 100);
        assert!(c.is_none());
    }

    #[test]
    fn invalid_memo_transfers_still_surface_without_transaction_hashes() {
        let staking = "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 2,
            transaction: IndexTransaction {
                memo: 7,
                icrc1_memo: Some(b"not-a-principal".to_vec()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(50),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp { timestamp_nanos: 123 }),
                timestamp: None,
            },
        };
        let c = indexed_contribution_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedContributionEntry::Invalid(c) => {
                assert_eq!(c.memo_text, INVALID_MEMO_PLACEHOLDER);
            }
            IndexedContributionEntry::Valid(_) => panic!("expected invalid contribution"),
        }
    }

    #[test]
    fn short_valid_principal_text_is_indexed_without_a_suffix_rule() {
        let staking = "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 3,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(b"qaa6y-5yaaa-aaaaa-aaafa-cai".to_vec()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(100_000_000),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp { timestamp_nanos: 124 }),
                timestamp: None,
            },
        };
        let c = indexed_contribution_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedContributionEntry::Valid(c) => {
                assert_eq!(c.beneficiary, Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").unwrap());
                assert!(c.counts_toward_faucet);
            }
            IndexedContributionEntry::Invalid(_) => panic!("expected valid contribution"),
        }
    }

    #[test]
    fn push_cycles_dedupes_same_timestamp() {
        let mut history = vec![make_cycles_sample(10, 100, CyclesSampleSource::SelfCanister)];
        assert!(!push_cycles_sample(&mut history, make_cycles_sample(10, 200, CyclesSampleSource::BlackholeStatus), 100));
        assert_eq!(history.len(), 1);
        assert!(push_cycles_sample(&mut history, make_cycles_sample(11, 200, CyclesSampleSource::BlackholeStatus), 100));
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn push_contribution_dedupes_tx_and_prunes() {
        let mut history = vec![];
        assert!(push_contribution(&mut history, ContributionSample { tx_id: 1, timestamp_nanos: Some(1), amount_e8s: 10, counts_toward_faucet: true }, 2));
        assert!(!push_contribution(&mut history, ContributionSample { tx_id: 1, timestamp_nanos: Some(1), amount_e8s: 10, counts_toward_faucet: true }, 2));
        assert!(push_contribution(&mut history, ContributionSample { tx_id: 2, timestamp_nanos: Some(2), amount_e8s: 20, counts_toward_faucet: true }, 2));
        assert!(push_contribution(&mut history, ContributionSample { tx_id: 3, timestamp_nanos: Some(3), amount_e8s: 30, counts_toward_faucet: true }, 2));
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].tx_id, 2);
        assert_eq!(history[1].tx_id, 3);
    }

    #[test]
    fn source_merge_and_blackhole_skip_behave() {
        let merged = merge_sources(None, CanisterSource::MemoContribution);
        let merged = merge_sources(Some(&merged), CanisterSource::SnsDiscovery);
        assert!(merged.contains(&CanisterSource::MemoContribution));
        assert!(should_skip_blackhole_for_sources(&merged));
    }
    #[test]
    fn transfer_from_transactions_do_not_count_as_staking_contributions() {
        let tx = IndexTransactionWithId {
            id: 42,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(target_canister().to_text().into_bytes()),
                operation: IndexOperation::TransferFrom {
                    to: "staking-account".to_string(),
                    fee: Tokens::new(10_000),
                    from: "from-account".to_string(),
                    amount: Tokens::new(1_000_000),
                    spender: "spender-account".to_string(),
                },
                created_at_time: Some(IndexTimeStamp { timestamp_nanos: 123 }),
                timestamp: Some(IndexTimeStamp { timestamp_nanos: 456 }),
            },
        };
        assert!(memo_bytes_from_index_tx(&tx, "staking-account").is_none());
        assert!(indexed_contribution_from_tx(&tx, "staking-account", 1).is_none());
    }


}
