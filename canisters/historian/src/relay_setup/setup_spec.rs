use super::recipient_set::CanonicalSurplusRecipientSet;
use super::{CanonicalRelayTargetSet, RelaySetupKey};
use crate::state::Config;
use candid::Principal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalRelaySetup {
    targets: CanonicalRelayTargetSet,
    surplus_recipients: CanonicalSurplusRecipientSet,
}

impl CanonicalRelaySetup {
    pub(crate) fn canonicalize(
        target_canister_ids: Vec<Principal>,
        surplus_recipient_principals: Vec<Principal>,
    ) -> Result<Self, String> {
        Ok(Self {
            targets: CanonicalRelayTargetSet::canonicalize(target_canister_ids)?,
            surplus_recipients: CanonicalSurplusRecipientSet::canonicalize(
                surplus_recipient_principals,
            )?,
        })
    }

    pub(crate) fn validate_for_new_setup(
        &self,
        config: &Config,
        historian: Principal,
    ) -> Result<(), String> {
        self.targets.validate_for_new_setup(config, historian)?;
        self.surplus_recipients.validate_for_new_setup()
    }

    pub(crate) fn targets(&self) -> &[Principal] {
        self.targets.targets()
    }

    pub(crate) fn surplus_recipients(&self) -> &[Principal] {
        self.surplus_recipients.recipients()
    }

    pub(crate) fn target_count(&self) -> usize {
        self.targets.len()
    }

    pub(crate) fn surplus_recipient_count(&self) -> usize {
        self.surplus_recipients.len()
    }

    pub(crate) fn key(&self) -> RelaySetupKey {
        RelaySetupKey::from_canonical_configuration(self.targets(), self.surplus_recipients())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[0x7f, byte])
    }

    fn setup(targets: &[u8], recipients: &[u8]) -> CanonicalRelaySetup {
        CanonicalRelaySetup::canonicalize(
            targets.iter().copied().map(principal).collect(),
            recipients.iter().copied().map(principal).collect(),
        )
        .unwrap()
    }

    #[test]
    fn permutations_preserve_identity_and_category_changes_do_not() {
        assert_eq!(setup(&[1, 2], &[3, 4]).key(), setup(&[2, 1], &[3, 4]).key());
        assert_eq!(setup(&[1, 2], &[3, 4]).key(), setup(&[1, 2], &[4, 3]).key());
        assert_ne!(setup(&[1], &[2]).key(), setup(&[3], &[2]).key());
        assert_ne!(setup(&[1], &[2]).key(), setup(&[1], &[3]).key());
        assert_ne!(setup(&[1], &[2]).key(), setup(&[2], &[1]).key());
    }

    #[test]
    fn principal_may_be_target_and_recipient() {
        let setup = setup(&[1], &[1]);
        assert_eq!(setup.targets(), setup.surplus_recipients());
    }
}
