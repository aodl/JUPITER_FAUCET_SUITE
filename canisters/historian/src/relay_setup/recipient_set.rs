use candid::Principal;

pub(crate) const MAX_SURPLUS_RECIPIENTS: usize = 5;
const MAX_STRUCTURAL_SURPLUS_RECIPIENTS: usize = u8::MAX as usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalSurplusRecipientSet(Vec<Principal>);

impl CanonicalSurplusRecipientSet {
    pub(crate) fn canonicalize(mut recipients: Vec<Principal>) -> Result<Self, String> {
        if recipients.is_empty() {
            return Err("at least one surplus recipient principal is required".to_string());
        }
        if recipients.len() > MAX_STRUCTURAL_SURPLUS_RECIPIENTS {
            return Err(format!(
                "at most {MAX_STRUCTURAL_SURPLUS_RECIPIENTS} surplus recipient principals can be canonicalized"
            ));
        }
        for recipient in &recipients {
            if *recipient == Principal::anonymous() {
                return Err("surplus recipient principal must not be anonymous".to_string());
            }
            if *recipient == Principal::management_canister() {
                return Err(
                    "surplus recipient principal must not be the management canister".to_string(),
                );
            }
        }
        recipients.sort_unstable_by(|left, right| left.as_slice().cmp(right.as_slice()));
        if recipients.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("duplicate surplus recipient principals are not allowed".to_string());
        }
        Ok(Self(recipients))
    }

    pub(crate) fn validate_for_new_setup(&self) -> Result<(), String> {
        if self.len() > MAX_SURPLUS_RECIPIENTS {
            return Err(format!(
                "at most {MAX_SURPLUS_RECIPIENTS} surplus recipient principals are allowed"
            ));
        }
        Ok(())
    }

    pub(crate) fn recipients(&self) -> &[Principal] {
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
        assert!(CanonicalSurplusRecipientSet::canonicalize(vec![Principal::anonymous()]).is_err());
        assert!(
            CanonicalSurplusRecipientSet::canonicalize(vec![Principal::management_canister()])
                .is_err()
        );
        assert!(
            CanonicalSurplusRecipientSet::canonicalize(vec![principal(1), principal(1)]).is_err()
        );

        let recipients =
            CanonicalSurplusRecipientSet::canonicalize(vec![principal(2), principal(1)]).unwrap();
        assert_eq!(recipients.recipients(), &[principal(1), principal(2)]);
        assert!(recipients.validate_for_new_setup().is_ok());

        let five =
            CanonicalSurplusRecipientSet::canonicalize((1..=5).map(principal).collect()).unwrap();
        assert!(five.validate_for_new_setup().is_ok());
        let six =
            CanonicalSurplusRecipientSet::canonicalize((1..=6).map(principal).collect()).unwrap();
        assert!(six.validate_for_new_setup().is_err());
    }
}
