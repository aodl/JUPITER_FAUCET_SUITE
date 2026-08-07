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
    by_principal
        .into_values()
        .max_by(|(left_principal, left), (right_principal, right)| {
            left.len()
                .cmp(&right.len())
                .then_with(|| right_principal.as_slice().cmp(left_principal.as_slice()))
        })
        .map(|(principal, _)| principal)
}

pub(crate) fn owner_for_neuron(neuron: &Neuron) -> Option<Principal> {
    effective_stake_is_positive(neuron).then(|| select_owner(&neuron.permissions))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn permission(principal: Option<Principal>, types: &[i32]) -> NeuronPermission {
        NeuronPermission {
            principal,
            permission_type: types.to_vec(),
        }
    }

    fn neuron(stake: u64, fees: u64, permissions: Vec<NeuronPermission>) -> Neuron {
        Neuron {
            id: None,
            permissions,
            cached_neuron_stake_e8s: stake,
            neuron_fees_e8s: fees,
        }
    }

    #[test]
    fn owner_selection_counts_distinct_permissions_across_entries() {
        let a = principal(2);
        let b = principal(3);
        let permissions = vec![
            permission(Some(a), &[1, 1, 2]),
            permission(Some(a), &[2, 3]),
            permission(Some(b), &[1, 2]),
        ];
        assert_eq!(select_owner(&permissions), Some(a));
    }

    #[test]
    fn owner_selection_uses_lexicographically_smallest_principal_on_tie() {
        let small = principal(1);
        let large = principal(2);
        let forward = vec![permission(Some(large), &[1]), permission(Some(small), &[7])];
        let reverse = forward.iter().cloned().rev().collect::<Vec<_>>();
        assert_eq!(select_owner(&forward), Some(small));
        assert_eq!(select_owner(&reverse), Some(small));
    }

    #[test]
    fn missing_principals_and_empty_permissions_produce_no_owner() {
        assert_eq!(select_owner(&[permission(None, &[1, 2])]), None);
        assert_eq!(select_owner(&[]), None);
    }

    #[test]
    fn effective_stake_rule_is_saturating_and_strictly_positive() {
        let owner = principal(1);
        assert_eq!(
            owner_for_neuron(&neuron(10, 10, vec![permission(Some(owner), &[1])])),
            None
        );
        assert_eq!(
            owner_for_neuron(&neuron(9, 10, vec![permission(Some(owner), &[1])])),
            None
        );
        assert_eq!(
            owner_for_neuron(&neuron(11, 10, vec![permission(Some(owner), &[1])])),
            Some(owner)
        );
    }
}
