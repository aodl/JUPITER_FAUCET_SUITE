use crate::api::RelaySurplusRecipient;
use candid::Principal;
use std::cmp::Ordering;

pub(crate) const MAX_SURPLUS_RECIPIENTS: usize = 5;
const MAX_STRUCTURAL_SURPLUS_RECIPIENTS: usize = u8::MAX as usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalSurplusRecipientSet(Vec<RelaySurplusRecipient>);

impl CanonicalSurplusRecipientSet {
    pub(crate) fn canonicalize(mut recipients: Vec<RelaySurplusRecipient>) -> Result<Self, String> {
        if recipients.is_empty() {
            return Err("at least one surplus recipient is required".to_string());
        }
        if recipients.len() > MAX_STRUCTURAL_SURPLUS_RECIPIENTS {
            return Err(format!(
                "at most {MAX_STRUCTURAL_SURPLUS_RECIPIENTS} surplus recipients can be canonicalized"
            ));
        }
        for recipient in &recipients {
            match recipient {
                RelaySurplusRecipient::Principal(principal) => {
                    if *principal == Principal::anonymous() {
                        return Err("surplus recipient principal must not be anonymous".to_string());
                    }
                    if *principal == Principal::management_canister() {
                        return Err(
                            "surplus recipient principal must not be the management canister"
                                .to_string(),
                        );
                    }
                }
                RelaySurplusRecipient::Neuron(0) => {
                    return Err("surplus recipient neuron ID must be greater than zero".to_string());
                }
                RelaySurplusRecipient::Neuron(_) => {}
            }
        }
        recipients.sort_unstable_by(|left, right| match (left, right) {
            (RelaySurplusRecipient::Principal(left), RelaySurplusRecipient::Principal(right)) => {
                left.as_slice().cmp(right.as_slice())
            }
            (RelaySurplusRecipient::Principal(_), RelaySurplusRecipient::Neuron(_)) => {
                Ordering::Less
            }
            (RelaySurplusRecipient::Neuron(_), RelaySurplusRecipient::Principal(_)) => {
                Ordering::Greater
            }
            (RelaySurplusRecipient::Neuron(left), RelaySurplusRecipient::Neuron(right)) => {
                left.cmp(right)
            }
        });
        if recipients.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("duplicate surplus recipients are not allowed".to_string());
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

    #[test]
    fn validates_recipient_policy_and_canonical_order() {
        assert!(CanonicalSurplusRecipientSet::canonicalize(Vec::new()).is_err());
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Principal(Principal::anonymous())
        ])
        .is_err());
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Principal(Principal::management_canister())
        ])
        .is_err());
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Principal(principal(1)),
            RelaySurplusRecipient::Principal(principal(1))
        ])
        .is_err());
        assert!(
            CanonicalSurplusRecipientSet::canonicalize(vec![RelaySurplusRecipient::Neuron(0)])
                .is_err()
        );
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Neuron(42),
            RelaySurplusRecipient::Neuron(42)
        ])
        .is_err());

        let recipients = CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Neuron(u64::MAX),
            RelaySurplusRecipient::Principal(principal(2)),
            RelaySurplusRecipient::Neuron(1),
            RelaySurplusRecipient::Principal(principal(1)),
        ])
        .unwrap();
        assert_eq!(
            recipients.recipients(),
            &[
                RelaySurplusRecipient::Principal(principal(1)),
                RelaySurplusRecipient::Principal(principal(2)),
                RelaySurplusRecipient::Neuron(1),
                RelaySurplusRecipient::Neuron(u64::MAX),
            ]
        );
        assert!(recipients.validate_for_new_setup().is_ok());

        let five = CanonicalSurplusRecipientSet::canonicalize(vec![
            RelaySurplusRecipient::Principal(principal(1)),
            RelaySurplusRecipient::Principal(principal(2)),
            RelaySurplusRecipient::Principal(principal(3)),
            RelaySurplusRecipient::Neuron(1),
            RelaySurplusRecipient::Neuron(u64::MAX),
        ])
        .unwrap();
        assert!(five.validate_for_new_setup().is_ok());
        let six = CanonicalSurplusRecipientSet::canonicalize(
            (1..=6).map(RelaySurplusRecipient::Neuron).collect(),
        )
        .unwrap();
        assert!(six.validate_for_new_setup().is_err());
    }

    #[test]
    fn accepts_each_recipient_type_at_structural_u64_bounds() {
        let principal_only =
            CanonicalSurplusRecipientSet::canonicalize(vec![RelaySurplusRecipient::Principal(
                principal(1),
            )])
            .unwrap();
        assert_eq!(
            principal_only.recipients(),
            &[RelaySurplusRecipient::Principal(principal(1))]
        );

        for neuron_id in [1, u64::MAX] {
            let neuron_only =
                CanonicalSurplusRecipientSet::canonicalize(vec![RelaySurplusRecipient::Neuron(
                    neuron_id,
                )])
                .unwrap();
            assert_eq!(
                neuron_only.recipients(),
                &[RelaySurplusRecipient::Neuron(neuron_id)]
            );
        }
    }
}
