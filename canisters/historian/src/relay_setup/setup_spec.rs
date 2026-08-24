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

    fn principal_recipient(byte: u8, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Principal {
            principal: principal(byte),
            memo,
        }
    }

    fn neuron_recipient(neuron_id: u64, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Neuron { neuron_id, memo }
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
        let recipients = || {
            vec![
                principal_recipient(3, vec![1]),
                principal_recipient(4, vec![2]),
                neuron_recipient(2, vec![3]),
            ]
        };
        assert_eq!(
            setup(&[1, 2], recipients()).key(),
            setup(&[2, 1], recipients()).key()
        );
        assert_eq!(
            setup(&[1, 2], recipients()).key(),
            setup(&[1, 2], recipients().into_iter().rev().collect()).key()
        );
        assert_ne!(
            setup(&[1], vec![principal_recipient(2, vec![1])]).key(),
            setup(&[3], vec![principal_recipient(2, vec![1])]).key()
        );
        assert_ne!(
            setup(&[1], vec![principal_recipient(2, vec![1])]).key(),
            setup(&[1], vec![principal_recipient(3, vec![1])]).key()
        );
        assert_ne!(
            setup(&[1], vec![neuron_recipient(2, vec![1])]).key(),
            setup(&[1], vec![neuron_recipient(3, vec![1])]).key()
        );
        assert_ne!(
            setup(&[1], vec![principal_recipient(2, vec![1])]).key(),
            setup(&[1], vec![neuron_recipient(2, vec![1])]).key()
        );
    }

    #[test]
    fn memo_bytes_and_recipient_count_change_identity() {
        let empty_memo = setup(&[1], vec![principal_recipient(2, vec![])]);
        let memoed = setup(&[1], vec![principal_recipient(2, vec![0])]);
        let memo_changed = setup(&[1], vec![principal_recipient(2, vec![1])]);
        let all_cycles = setup(&[1], vec![]);
        assert_ne!(empty_memo.key(), memoed.key());
        assert_ne!(memoed.key(), memo_changed.key());
        assert_ne!(all_cycles.key(), empty_memo.key());
    }

    #[test]
    fn principal_may_be_target_and_recipient() {
        let setup = setup(&[1], vec![principal_recipient(1, vec![])]);
        assert_eq!(setup.targets(), &[principal(1)]);
        assert_eq!(
            setup.surplus_recipients(),
            &[principal_recipient(1, vec![])]
        );
    }
}
