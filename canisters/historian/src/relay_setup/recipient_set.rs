use crate::api::RelaySurplusRecipient;
use candid::Principal;
use jupiter_memo_policy::MAX_RELAY_SURPLUS_MEMO_BYTES;
use std::cmp::Ordering;

pub(crate) const MAX_SURPLUS_RECIPIENTS: usize = 5;
const MAX_STRUCTURAL_SURPLUS_RECIPIENTS: usize = u8::MAX as usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalSurplusRecipientSet(Vec<RelaySurplusRecipient>);

fn destination_cmp(left: &RelaySurplusRecipient, right: &RelaySurplusRecipient) -> Ordering {
    match (left, right) {
        (
            RelaySurplusRecipient::Principal {
                principal: left, ..
            },
            RelaySurplusRecipient::Principal {
                principal: right, ..
            },
        ) => left.as_slice().cmp(right.as_slice()),
        (RelaySurplusRecipient::Principal { .. }, RelaySurplusRecipient::Neuron { .. }) => {
            Ordering::Less
        }
        (RelaySurplusRecipient::Neuron { .. }, RelaySurplusRecipient::Principal { .. }) => {
            Ordering::Greater
        }
        (
            RelaySurplusRecipient::Neuron {
                neuron_id: left, ..
            },
            RelaySurplusRecipient::Neuron {
                neuron_id: right, ..
            },
        ) => left.cmp(right),
    }
}

impl CanonicalSurplusRecipientSet {
    pub(crate) fn canonicalize(mut recipients: Vec<RelaySurplusRecipient>) -> Result<Self, String> {
        if recipients.len() > MAX_STRUCTURAL_SURPLUS_RECIPIENTS {
            return Err(format!(
                "at most {MAX_STRUCTURAL_SURPLUS_RECIPIENTS} surplus recipients can be canonicalized"
            ));
        }
        for recipient in &recipients {
            match recipient {
                RelaySurplusRecipient::Principal { principal, memo } => {
                    if *principal == Principal::anonymous() {
                        return Err("surplus recipient principal must not be anonymous".to_string());
                    }
                    if *principal == Principal::management_canister() {
                        return Err(
                            "surplus recipient principal must not be the management canister"
                                .to_string(),
                        );
                    }
                    if memo.len() > MAX_RELAY_SURPLUS_MEMO_BYTES {
                        return Err(format!(
                            "surplus recipient principal {} memo is {} bytes; maximum is {MAX_RELAY_SURPLUS_MEMO_BYTES}",
                            principal.to_text(),
                            memo.len()
                        ));
                    }
                }
                RelaySurplusRecipient::Neuron { neuron_id: 0, .. } => {
                    return Err("surplus recipient neuron ID must be greater than zero".to_string());
                }
                RelaySurplusRecipient::Neuron { neuron_id, memo } => {
                    if memo.len() > MAX_RELAY_SURPLUS_MEMO_BYTES {
                        return Err(format!(
                            "surplus recipient neuron {neuron_id} memo is {} bytes; maximum is {MAX_RELAY_SURPLUS_MEMO_BYTES}",
                            memo.len()
                        ));
                    }
                }
            }
        }
        recipients.sort_unstable_by(destination_cmp);
        if recipients
            .windows(2)
            .any(|pair| destination_cmp(&pair[0], &pair[1]) == Ordering::Equal)
        {
            return Err("duplicate surplus recipient destinations are not allowed".to_string());
        }
        Ok(Self(recipients))
    }

    pub(crate) fn validate_for_new_setup(&self) -> Result<(), String> {
        if self.len() > MAX_SURPLUS_RECIPIENTS {
            return Err(format!(
                "at most {MAX_SURPLUS_RECIPIENTS} surplus recipients are allowed"
            ));
        }
        Ok(())
    }

    pub(crate) fn recipients(&self) -> &[RelaySurplusRecipient] {
        &self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[0x7f, byte])
    }

    fn principal_recipient(byte: u8, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Principal {
            principal: principal(byte),
            memo,
        }
    }

    fn neuron_recipient(neuron_id: u64, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Neuron { neuron_id, memo }
    }

    #[test]
    fn validates_recipient_cardinality_and_destinations() {
        let zero = CanonicalSurplusRecipientSet::canonicalize(Vec::new()).unwrap();
        assert!(zero.validate_for_new_setup().is_ok());
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Principal {
                principal: Principal::anonymous(),
                memo: vec![],
            }
        ])
        .is_err());
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Principal {
                principal: Principal::management_canister(),
                memo: vec![],
            }
        ])
        .is_err());
        assert!(
            CanonicalSurplusRecipientSet::canonicalize(vec![neuron_recipient(0, vec![])]).is_err()
        );
        assert!(
            CanonicalSurplusRecipientSet::canonicalize(vec![principal_recipient(1, vec![])])
                .unwrap()
                .validate_for_new_setup()
                .is_ok()
        );
        assert!(
            CanonicalSurplusRecipientSet::canonicalize(vec![neuron_recipient(1, vec![])])
                .unwrap()
                .validate_for_new_setup()
                .is_ok()
        );

        let five = CanonicalSurplusRecipientSet::canonicalize(vec![
            principal_recipient(1, vec![]),
            principal_recipient(2, vec![]),
            principal_recipient(3, vec![]),
            neuron_recipient(1, vec![]),
            neuron_recipient(u64::MAX, vec![]),
        ])
        .unwrap();
        assert!(five.validate_for_new_setup().is_ok());
        let six = CanonicalSurplusRecipientSet::canonicalize(
            (1..=6).map(|id| neuron_recipient(id, vec![])).collect(),
        )
        .unwrap();
        assert!(six.validate_for_new_setup().is_err());
    }

    #[test]
    fn validates_exact_byte_memo_bounds_for_both_recipient_types() {
        for memo in [
            vec![],
            vec![0],
            vec![b'a'; MAX_RELAY_SURPLUS_MEMO_BYTES],
            vec![0x00, 0xff, 0x80],
        ] {
            assert!(
                CanonicalSurplusRecipientSet::canonicalize(vec![principal_recipient(
                    1,
                    memo.clone()
                )])
                .is_ok()
            );
            assert!(
                CanonicalSurplusRecipientSet::canonicalize(vec![neuron_recipient(1, memo)]).is_ok()
            );
        }
        for recipient in [
            principal_recipient(1, vec![0; MAX_RELAY_SURPLUS_MEMO_BYTES + 1]),
            neuron_recipient(1, vec![0; MAX_RELAY_SURPLUS_MEMO_BYTES + 1]),
        ] {
            let error = CanonicalSurplusRecipientSet::canonicalize(vec![recipient]).unwrap_err();
            assert!(error.contains("33 bytes"));
        }
    }

    #[test]
    fn duplicate_identity_ignores_memo_and_types_are_separate_namespaces() {
        for duplicates in [
            vec![
                principal_recipient(1, vec![]),
                principal_recipient(1, vec![]),
            ],
            vec![
                principal_recipient(1, vec![1]),
                principal_recipient(1, vec![2]),
            ],
            vec![neuron_recipient(42, vec![]), neuron_recipient(42, vec![])],
            vec![neuron_recipient(42, vec![1]), neuron_recipient(42, vec![2])],
        ] {
            assert!(CanonicalSurplusRecipientSet::canonicalize(duplicates).is_err());
        }
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            principal_recipient(1, vec![]),
            neuron_recipient(1, vec![]),
        ])
        .is_ok());
    }

    #[test]
    fn canonical_order_uses_destination_only_and_keeps_memos_attached() {
        let expected = vec![
            principal_recipient(1, vec![0xff]),
            principal_recipient(2, vec![0x00]),
            neuron_recipient(1, vec![0xaa]),
            neuron_recipient(u64::MAX, vec![0x55]),
        ];
        let mut reversed = expected.clone();
        reversed.reverse();
        let canonical = CanonicalSurplusRecipientSet::canonicalize(reversed).unwrap();
        assert_eq!(canonical.recipients(), expected);

        let memo_changed = CanonicalSurplusRecipientSet::canonicalize(vec![
            principal_recipient(2, vec![0xff]),
            principal_recipient(1, vec![0x00]),
        ])
        .unwrap();
        assert_eq!(
            memo_changed.recipients(),
            &[
                principal_recipient(1, vec![0x00]),
                principal_recipient(2, vec![0xff])
            ]
        );
    }
}
