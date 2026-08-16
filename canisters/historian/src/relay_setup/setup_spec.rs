use super::recipient_set::CanonicalSurplusRecipientSet;
use super::{CanonicalRelayTargetSet, RelaySetupKey};
use crate::api::RelaySurplusRecipient;
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
        surplus_recipients: Vec<RelaySurplusRecipient>,
    ) -> Result<Self, String> {
        Ok(Self {
            targets: CanonicalRelayTargetSet::canonicalize(target_canister_ids)?,
            surplus_recipients: CanonicalSurplusRecipientSet::canonicalize(surplus_recipients)?,
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

    pub(crate) fn surplus_recipients(&self) -> &[RelaySurplusRecipient] {
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

    fn setup(targets: &[u8], recipients: Vec<RelaySurplusRecipient>) -> CanonicalRelaySetup {
        CanonicalRelaySetup::canonicalize(
            targets.iter().copied().map(principal).collect(),
            recipients,
        )
        .unwrap()
    }

    #[test]
    fn permutations_preserve_identity_and_category_changes_do_not() {
        let principals = || {
            vec![
                RelaySurplusRecipient::Principal(principal(3)),
                RelaySurplusRecipient::Principal(principal(4)),
            ]
        };
        assert_eq!(
            setup(&[1, 2], principals()).key(),
            setup(&[2, 1], principals()).key()
        );
        assert_eq!(
            setup(&[1, 2], principals()).key(),
            setup(&[1, 2], principals().into_iter().rev().collect()).key()
        );
        assert_eq!(
            setup(
                &[1],
                vec![
                    RelaySurplusRecipient::Neuron(2),
                    RelaySurplusRecipient::Neuron(1)
                ]
            )
            .key(),
            setup(
                &[1],
                vec![
                    RelaySurplusRecipient::Neuron(1),
                    RelaySurplusRecipient::Neuron(2)
                ]
            )
            .key()
        );
        assert_eq!(
            setup(
                &[1],
                vec![
                    RelaySurplusRecipient::Neuron(2),
                    RelaySurplusRecipient::Principal(principal(3))
                ]
            )
            .key(),
            setup(
                &[1],
                vec![
                    RelaySurplusRecipient::Principal(principal(3)),
                    RelaySurplusRecipient::Neuron(2)
                ]
            )
            .key()
        );
        assert_ne!(
            setup(&[1], vec![RelaySurplusRecipient::Principal(principal(2))]).key(),
            setup(&[3], vec![RelaySurplusRecipient::Principal(principal(2))]).key()
        );
        assert_ne!(
            setup(&[1], vec![RelaySurplusRecipient::Principal(principal(2))]).key(),
            setup(&[1], vec![RelaySurplusRecipient::Principal(principal(3))]).key()
        );
        assert_ne!(
            setup(&[1], vec![RelaySurplusRecipient::Neuron(2)]).key(),
            setup(&[1], vec![RelaySurplusRecipient::Neuron(3)]).key()
        );
        assert_ne!(
            setup(&[1], vec![RelaySurplusRecipient::Principal(principal(2))]).key(),
            setup(&[1], vec![RelaySurplusRecipient::Neuron(2)]).key()
        );
    }

    #[test]
    fn principal_may_be_target_and_recipient() {
        let setup = setup(&[1], vec![RelaySurplusRecipient::Principal(principal(1))]);
        assert_eq!(setup.targets(), &[principal(1)]);
        assert_eq!(
            setup.surplus_recipients(),
            &[RelaySurplusRecipient::Principal(principal(1))]
        );
    }
}
