use candid::Principal;

pub(crate) const INVALID_MEMO_PLACEHOLDER: &str = "invalid declared memo";
use std::collections::BTreeSet;

use crate::clients::index::{IndexOperation, IndexTransactionWithId};
use crate::state::{
    CanisterMeta, CanisterTrackingReason, CommitmentSample, CyclesProbeResult, CyclesSample,
    CyclesSampleSource,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndexedCommitmentTarget {
    CyclesTopUp {
        canister_id: Principal,
    },
    RawIcp {
        canister_id: Principal,
        memo_text: String,
    },
    NeuronStake {
        neuron_id: u64,
        memo_text: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexedCommitment {
    pub tx_id: u64,
    pub target: IndexedCommitmentTarget,
    pub amount_e8s: u64,
    pub timestamp_nanos: Option<u64>,
    pub counts_toward_faucet: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexedInvalidCommitment {
    pub tx_id: u64,
    pub amount_e8s: u64,
    pub timestamp_nanos: Option<u64>,
    pub memo_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndexedCommitmentEntry {
    Valid(IndexedCommitment),
    Invalid(IndexedInvalidCommitment),
}

pub(crate) type IndexMemoTransfer = (u64, Option<Vec<u8>>, u64, Option<u64>);

#[cfg(test)]
fn parse_target_canister_from_memo(bytes: &[u8]) -> Option<Principal> {
    jupiter_memo_policy::parse_target_canister_principal_from_memo(bytes)
}

pub(crate) fn memo_bytes_from_index_tx(
    tx: &IndexTransactionWithId,
    staking_account_id: &str,
) -> Option<IndexMemoTransfer> {
    match &tx.transaction.operation {
        IndexOperation::Transfer { to, amount, .. } if to == staking_account_id => {
            let memo = tx
                .transaction
                .icrc1_memo
                .as_ref()
                .and_then(|bytes| (!bytes.is_empty()).then(|| bytes.clone()));
            let ts = tx
                .transaction
                .timestamp
                .as_ref()
                .map(|ts| ts.timestamp_nanos)
                .or_else(|| {
                    tx.transaction
                        .created_at_time
                        .as_ref()
                        .map(|ts| ts.timestamp_nanos)
                });
            Some((tx.id, memo, amount.e8s(), ts))
        }
        _ => None,
    }
}

pub(crate) fn indexed_commitment_from_tx(
    tx: &IndexTransactionWithId,
    staking_account_id: &str,
    min_tx_e8s: u64,
) -> Option<IndexedCommitmentEntry> {
    let (tx_id, memo_opt, amount_e8s, timestamp_nanos) =
        memo_bytes_from_index_tx(tx, staking_account_id)?;
    let memo = memo_opt?;
    let target = match jupiter_memo_policy::parse_memo_directive(&memo) {
        Some(jupiter_memo_policy::MemoDirective::TopUp { canister_id }) => {
            Some(IndexedCommitmentTarget::CyclesTopUp { canister_id })
        }
        Some(jupiter_memo_policy::MemoDirective::RawIcp { canister_id, memo }) => {
            let memo_text = String::from_utf8_lossy(&memo).to_string();
            Some(IndexedCommitmentTarget::RawIcp {
                canister_id,
                memo_text,
            })
        }
        Some(jupiter_memo_policy::MemoDirective::NeuronStake { neuron_id, memo }) => {
            let memo_text = memo.map(|memo| String::from_utf8_lossy(&memo).to_string());
            Some(IndexedCommitmentTarget::NeuronStake {
                neuron_id,
                memo_text,
            })
        }
        None => None,
    };
    if let Some(target) = target {
        Some(IndexedCommitmentEntry::Valid(IndexedCommitment {
            tx_id,
            target,
            amount_e8s,
            timestamp_nanos,
            counts_toward_faucet: amount_e8s >= min_tx_e8s,
        }))
    } else {
        Some(IndexedCommitmentEntry::Invalid(IndexedInvalidCommitment {
            tx_id,
            amount_e8s,
            timestamp_nanos,
            memo_text: INVALID_MEMO_PLACEHOLDER.to_string(),
        }))
    }
}

pub(crate) fn merge_tracking_reasons(
    existing: Option<&BTreeSet<CanisterTrackingReason>>,
    add: CanisterTrackingReason,
) -> BTreeSet<CanisterTrackingReason> {
    let mut out = existing.cloned().unwrap_or_default();
    out.insert(add);
    out
}

pub(crate) fn push_commitment(
    history: &mut Vec<CommitmentSample>,
    sample: CommitmentSample,
    max_entries: u32,
) -> bool {
    if history
        .iter()
        .any(|existing| existing.tx_id == sample.tx_id)
    {
        return false;
    }
    history.push(sample);
    prune_vec(history, max_entries);
    true
}

pub(crate) fn push_cycles_sample(
    history: &mut Vec<CyclesSample>,
    sample: CyclesSample,
    max_entries: u32,
) -> bool {
    if history
        .last()
        .map(|existing| existing.timestamp_nanos == sample.timestamp_nanos)
        .unwrap_or(false)
    {
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

pub(crate) fn apply_cycles_probe_result(
    meta: &mut CanisterMeta,
    timestamp_nanos: u64,
    result: CyclesProbeResult,
) {
    meta.last_cycles_probe_ts = Some(timestamp_nanos / 1_000_000_000);
    meta.last_cycles_probe_result = Some(result);
}

pub(crate) fn apply_commitment_seen(
    meta: &mut CanisterMeta,
    timestamp_nanos: Option<u64>,
    now_secs: u64,
) {
    if meta.first_seen_ts.is_none() {
        meta.first_seen_ts = Some(
            timestamp_nanos
                .map(|ts| ts / 1_000_000_000)
                .unwrap_or(now_secs),
        );
    }
    meta.last_commitment_ts = Some(
        timestamp_nanos
            .map(|ts| ts / 1_000_000_000)
            .unwrap_or(now_secs),
    );
}

pub(crate) fn make_cycles_sample(
    timestamp_nanos: u64,
    cycles: u128,
    source: CyclesSampleSource,
) -> CyclesSample {
    CyclesSample {
        timestamp_nanos,
        cycles,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::index::{
        IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId, Tokens,
    };

    fn principal(s: &str) -> Principal {
        Principal::from_text(s).unwrap()
    }
    fn target_canister() -> Principal {
        principal("22255-zqaaa-aaaas-qf6uq-cai")
    }

    #[test]
    fn indexes_cycles_top_up_memo_as_target_canister() {
        let p = target_canister();
        assert_eq!(
            parse_target_canister_from_memo(p.to_text().as_bytes()),
            Some(p)
        );
    }

    #[test]
    fn indexes_declared_target_from_raw_icp_memo_directive() {
        let p = target_canister();
        let compact = p.to_text().replace('-', "");
        assert_eq!(
            parse_target_canister_from_memo(format!("{compact}.vault42").as_bytes()),
            Some(p)
        );
    }

    #[test]
    fn numeric_neuron_id_memos_are_not_indexed_as_canisters() {
        assert_eq!(
            parse_target_canister_from_memo(b"11614578985374291210"),
            None
        );
    }

    #[test]
    fn invalid_commitment_memo_is_not_indexed_as_target_canister() {
        assert_eq!(parse_target_canister_from_memo(b"not-a-principal"), None);
    }

    #[test]
    fn indexed_commitment_uses_icrc1_memo_and_threshold_flag() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
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
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: 99,
                }),
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Valid(c) => {
                assert_eq!(
                    c.target,
                    IndexedCommitmentTarget::CyclesTopUp {
                        canister_id: beneficiary
                    }
                );
                assert!(!c.counts_toward_faucet);
                assert_eq!(c.timestamp_nanos, Some(99));
            }
            IndexedCommitmentEntry::Invalid(_) => panic!("expected valid commitment"),
        }
    }

    #[test]
    fn missing_icrc1_memo_is_ignored_even_when_legacy_numeric_memo_is_set() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
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
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: 99,
                }),
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100);
        assert!(c.is_none());
    }

    #[test]
    fn invalid_memo_transfers_still_surface_without_transaction_hashes() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
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
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 123,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Invalid(c) => {
                assert_eq!(c.memo_text, INVALID_MEMO_PLACEHOLDER);
            }
            IndexedCommitmentEntry::Valid(_) => panic!("expected invalid commitment"),
        }
    }

    #[test]
    fn short_valid_principal_text_is_indexed_without_a_suffix_rule() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
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
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 124,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Valid(c) => {
                assert_eq!(
                    c.target,
                    IndexedCommitmentTarget::CyclesTopUp {
                        canister_id: Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").unwrap(),
                    }
                );
                assert!(c.counts_toward_faucet);
            }
            IndexedCommitmentEntry::Invalid(_) => panic!("expected valid commitment"),
        }
    }

    #[test]
    fn raw_icp_directive_is_indexed_with_declared_canister_and_right_memo_segment() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let canister = target_canister();
        let compact = canister.to_text().replace('-', "");
        let tx = IndexTransactionWithId {
            id: 4,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(format!("{compact}.vault42").into_bytes()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(100_000_000),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 125,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Valid(c) => {
                assert_eq!(
                    c.target,
                    IndexedCommitmentTarget::RawIcp {
                        canister_id: canister,
                        memo_text: "vault42".to_string(),
                    }
                );
                assert!(c.counts_toward_faucet);
            }
            IndexedCommitmentEntry::Invalid(_) => panic!("expected valid commitment"),
        }
    }

    #[test]
    fn neuron_id_directive_is_indexed_as_neuron_commitment() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 5,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(b"11614578985374291210".to_vec()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(100_000_000),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 126,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Valid(c) => {
                assert_eq!(
                    c.target,
                    IndexedCommitmentTarget::NeuronStake {
                        neuron_id: 11_614_578_985_374_291_210,
                        memo_text: None,
                    }
                );
                assert!(c.counts_toward_faucet);
            }
            IndexedCommitmentEntry::Invalid(_) => panic!("expected valid commitment"),
        }
    }

    #[test]
    fn dotted_neuron_id_directive_is_indexed_with_right_memo_segment() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 6,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(b"42.vault.memo".to_vec()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3".into(),
                    amount: Tokens::new(100_000_000),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 127,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Valid(c) => {
                assert_eq!(
                    c.target,
                    IndexedCommitmentTarget::NeuronStake {
                        neuron_id: 42,
                        memo_text: Some("vault.memo".to_string()),
                    }
                );
                assert!(c.counts_toward_faucet);
            }
            IndexedCommitmentEntry::Invalid(_) => panic!("expected valid commitment"),
        }
    }

    #[test]
    fn whitespace_only_non_empty_memo_surfaces_as_invalid() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 30,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(b"  \n\t".to_vec()),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "sender".into(),
                    amount: Tokens::new(50),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 123,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Invalid(c) => assert_eq!(c.memo_text, INVALID_MEMO_PLACEHOLDER),
            IndexedCommitmentEntry::Valid(_) => panic!("expected invalid commitment"),
        }
    }

    #[test]
    fn non_utf8_non_empty_memo_surfaces_as_invalid() {
        let staking =
            "22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d".to_string();
        let tx = IndexTransactionWithId {
            id: 31,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(vec![0xff, 0xfe, 0xfd]),
                operation: IndexOperation::Transfer {
                    to: staking.clone(),
                    fee: Tokens::new(10_000),
                    from: "sender".into(),
                    amount: Tokens::new(50),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 124,
                }),
                timestamp: None,
            },
        };
        let c = indexed_commitment_from_tx(&tx, &staking, 100).unwrap();
        match c {
            IndexedCommitmentEntry::Invalid(c) => assert_eq!(c.memo_text, INVALID_MEMO_PLACEHOLDER),
            IndexedCommitmentEntry::Valid(_) => panic!("expected invalid commitment"),
        }
    }

    #[test]
    fn push_cycles_dedupes_same_timestamp() {
        let mut history = vec![make_cycles_sample(
            10,
            100,
            CyclesSampleSource::SelfCanister,
        )];
        assert!(!push_cycles_sample(
            &mut history,
            make_cycles_sample(10, 200, CyclesSampleSource::BlackholeStatus),
            100
        ));
        assert_eq!(history.len(), 1);
        assert!(push_cycles_sample(
            &mut history,
            make_cycles_sample(11, 200, CyclesSampleSource::BlackholeStatus),
            100
        ));
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn push_commitment_dedupes_tx_and_prunes() {
        let mut history = vec![];
        assert!(push_commitment(
            &mut history,
            CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 10,
                counts_toward_faucet: true
            },
            2
        ));
        assert!(!push_commitment(
            &mut history,
            CommitmentSample {
                tx_id: 1,
                timestamp_nanos: Some(1),
                amount_e8s: 10,
                counts_toward_faucet: true
            },
            2
        ));
        assert!(push_commitment(
            &mut history,
            CommitmentSample {
                tx_id: 2,
                timestamp_nanos: Some(2),
                amount_e8s: 20,
                counts_toward_faucet: true
            },
            2
        ));
        assert!(push_commitment(
            &mut history,
            CommitmentSample {
                tx_id: 3,
                timestamp_nanos: Some(3),
                amount_e8s: 30,
                counts_toward_faucet: true
            },
            2
        ));
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].tx_id, 2);
        assert_eq!(history[1].tx_id, 3);
    }

    #[test]
    fn tracking_reason_merge_keeps_multiple_reasons() {
        let merged = merge_tracking_reasons(None, CanisterTrackingReason::MemoCommitment);
        let merged = merge_tracking_reasons(Some(&merged), CanisterTrackingReason::SnsDiscovery);
        assert!(merged.contains(&CanisterTrackingReason::MemoCommitment));
        assert!(merged.contains(&CanisterTrackingReason::SnsDiscovery));
    }
    #[test]
    fn transfer_from_transactions_do_not_count_as_staking_commitments() {
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
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 123,
                }),
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: 456,
                }),
            },
        };
        assert!(memo_bytes_from_index_tx(&tx, "staking-account").is_none());
        assert!(indexed_commitment_from_tx(&tx, "staking-account", 1).is_none());
    }
}
