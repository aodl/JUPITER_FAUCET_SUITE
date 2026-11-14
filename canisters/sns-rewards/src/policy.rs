use std::collections::{BTreeMap, BTreeSet};

use candid::Principal;

use crate::clients::{Neuron, NeuronPermission};

pub(crate) const SNS_OWNER_SCAN_INTERVAL_SECONDS: u64 = 86_400;
pub(crate) const SNS_NEURON_PAGE_SIZE: u32 = 100;
pub(crate) const SNS_OWNER_SCAN_MAX_PAGES: u32 = 10_000;
pub(crate) const MAX_ACCOUNT_LOOKUPS_PER_CALL: usize = 128;
pub(crate) const SCAN_LEASE_SECONDS: u64 = 30 * 60;

pub(crate) fn effective_stake_is_positive(neuron: &Neuron) -> bool {
    neuron
        .cached_neuron_stake_e8s
        .saturating_sub(neuron.neuron_fees_e8s)
        > 0
}

pub(crate) fn select_owner(permissions: &[NeuronPermission]) -> Option<Principal> {
    let mut by_principal: BTreeMap<Vec<u8>, (Principal, BTreeSet<i32>)> = BTreeMap::new();
    for permission in permissions {
        let Some(principal) = permission.principal else {
            continue;
        };
        by_principal
            .entry(principal.as_slice().to_vec())
            .or_insert_with(|| (principal, BTreeSet::new()))
            .1
            .extend(permission.permission_type.iter().copied());
    }
    use crate::generated::NeuronPermissionType as P;
    // A named beneficiary cannot consent on-chain to being assigned permissions.
    // Require recoverable stake and reject every competing economic authority.
    // SubmitProposal can incur rejection fees, but is voting-related under the
    // pinned framework (including grants made by ManageVotingPermission).
    let candidates: Vec<_> = by_principal
        .values()
        .filter(|(p, codes)| {
            *p != Principal::anonymous()
                && *p != Principal::management_canister()
                && codes.contains(&(P::Disburse as i32))
        })
        .map(|(p, _)| *p)
        .collect();
    let [beneficiary] = candidates.as_slice() else {
        return None;
    };
    for (principal, codes) in by_principal.values() {
        for code in codes {
            let permission = P::try_from(*code).ok()?;
            // The pinned protobuf explicitly defines Unspecified as unused.
            // It grants no authority, including when present in a native list.
            if permission == P::Unspecified {
                continue;
            }
            if principal != beneficiary
                && !matches!(
                    permission,
                    P::Vote | P::SubmitProposal | P::ManageVotingPermission
                )
            {
                return None;
            }
        }
    }
    Some(*beneficiary)
}

pub(crate) fn owner_for_neuron(neuron: &Neuron) -> Option<Principal> {
    effective_stake_is_positive(neuron).then(|| select_owner(&neuron.permissions))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::NeuronPermissionType as P;
    fn p(b: u8) -> Principal {
        Principal::from_slice(&[b])
    }
    fn entry(who: Principal, codes: &[P]) -> NeuronPermission {
        NeuronPermission {
            principal: Some(who),
            permission_type: codes.iter().map(|c| *c as i32).collect(),
        }
    }
    #[test]
    fn disburse_without_permission_manager_and_voting_delegation_are_safe() {
        let mut entries = vec![entry(p(1), &[P::Disburse])];
        assert_eq!(select_owner(&entries), Some(p(1)));
        entries.push(entry(
            p(2),
            &[P::Vote, P::SubmitProposal, P::ManageVotingPermission],
        ));
        entries.push(entry(p(1), &[P::ConfigureDissolveState, P::Disburse]));
        assert_eq!(select_owner(&entries), Some(p(1)));
    }
    #[test]
    fn every_competing_economic_authority_and_unknown_code_fails_closed() {
        for competing in [
            P::ConfigureDissolveState,
            P::ManagePrincipals,
            P::Disburse,
            P::Split,
            P::MergeMaturity,
            P::DisburseMaturity,
            P::StakeMaturity,
        ] {
            let entries = vec![entry(p(1), &[P::Disburse]), entry(p(2), &[competing])];
            assert_eq!(select_owner(&entries), None, "{competing:?}");
        }
        let mut entries = vec![entry(p(1), &[P::Disburse])];
        entries[0].permission_type.push(999);
        assert_eq!(select_owner(&entries), None);
    }
    #[test]
    fn fragmented_permissions_are_unioned_and_empty_entries_invent_nothing() {
        let entries = vec![
            entry(p(1), &[P::Disburse]),
            entry(p(1), &[P::ManagePrincipals]),
            entry(p(1), &[P::Disburse]),
            entry(p(2), &[]),
            entry(p(1), &[P::Unspecified]),
            entry(p(2), &[P::Unspecified]),
            NeuronPermission {
                principal: None,
                permission_type: vec![P::Disburse as i32],
            },
        ];
        assert_eq!(select_owner(&entries), Some(p(1)));
        assert_eq!(select_owner(&[]), None);
        assert_eq!(
            select_owner(&[entry(Principal::anonymous(), &[P::Disburse])]),
            None
        );
        assert_eq!(
            select_owner(&[entry(Principal::management_canister(), &[P::Disburse])]),
            None
        );
    }
    #[test]
    fn exhausted_stake_does_not_establish_identity() {
        let mut neuron = Neuron {
            id: None,
            cached_neuron_stake_e8s: 10,
            neuron_fees_e8s: 10,
            permissions: vec![entry(p(1), &[P::Disburse])],
        };
        assert_eq!(owner_for_neuron(&neuron), None);
        neuron.cached_neuron_stake_e8s += 1;
        assert_eq!(owner_for_neuron(&neuron), Some(p(1)));
    }
}
