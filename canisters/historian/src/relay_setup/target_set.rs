use super::RelaySetupKey;
use crate::state::Config;
use candid::Principal;

pub(crate) const MAX_RELAY_TARGETS: usize = 20;
const MAX_STRUCTURAL_RELAY_TARGETS: usize = u8::MAX as usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalRelayTargetSet(Vec<Principal>);

impl CanonicalRelayTargetSet {
    #[cfg(test)]
    pub(crate) fn new(
        targets: Vec<Principal>,
        config: &Config,
        historian: Principal,
    ) -> Result<Self, String> {
        let canonical = Self::canonicalize(targets)?;
        canonical.validate_for_new_setup(config, historian)?;
        Ok(canonical)
    }

    pub(crate) fn canonicalize(mut targets: Vec<Principal>) -> Result<Self, String> {
        if targets.is_empty() {
            return Err("at least one target canister is required".to_string());
        }
        if targets.len() > MAX_STRUCTURAL_RELAY_TARGETS {
            return Err(format!(
                "at most {MAX_STRUCTURAL_RELAY_TARGETS} target canisters can be canonicalized"
            ));
        }
        for target in &targets {
            if *target == Principal::anonymous() {
                return Err("target must not be anonymous".to_string());
            }
            if *target == Principal::management_canister() {
                return Err("target must not be the management canister".to_string());
            }
        }
        targets.sort_unstable_by(|left, right| left.as_slice().cmp(right.as_slice()));
        if targets.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("duplicate target canisters are not allowed".to_string());
        }
        Ok(Self(targets))
    }

    pub(crate) fn validate_for_new_setup(
        &self,
        config: &Config,
        historian: Principal,
    ) -> Result<(), String> {
        if self.len() > MAX_RELAY_TARGETS {
            return Err(format!(
                "at most {MAX_RELAY_TARGETS} target canisters are allowed"
            ));
        }
        for target in &self.0 {
            validate_target(*target, config, historian)?;
        }
        Ok(())
    }

    pub(crate) fn targets(&self) -> &[Principal] {
        &self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn key(&self) -> RelaySetupKey {
        RelaySetupKey::from_canonical_targets(&self.0)
    }
}

fn validate_target(target: Principal, config: &Config, historian: Principal) -> Result<(), String> {
    if target == Principal::anonymous() {
        return Err("target must not be anonymous".to_string());
    }
    if target == Principal::management_canister() {
        return Err("target must not be the management canister".to_string());
    }
    if target == historian {
        return Err("target must not be the historian canister".to_string());
    }
    if target == jupiter_ic_clients::constants::fiduciary_blackhole_canister_id()
        || target == config.ledger_canister_id
        || target == config.index_canister_id
        || Some(target) == config.cmc_canister_id
    {
        return Err("target must not be a configured protocol dependency".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use icrc_ledger_types::icrc1::account::Account;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn structural_principals(count: usize) -> Vec<Principal> {
        (0..count)
            .map(|index| Principal::from_slice(&[0x7f, (index >> 8) as u8, index as u8]))
            .collect()
    }

    fn config() -> Config {
        Config {
            staking_account: Account {
                owner: principal(30),
                subaccount: None,
            },
            output_source_account: Account {
                owner: principal(31),
                subaccount: None,
            },
            output_account: Account {
                owner: principal(32),
                subaccount: None,
            },
            rewards_account: Account {
                owner: principal(33),
                subaccount: None,
            },
            ledger_canister_id: principal(40),
            index_canister_id: principal(41),
            cmc_canister_id: Some(principal(42)),
            faucet_canister_id: Some(principal(43)),
            sns_wasm_canister_id: principal(44),
            xrc_canister_id: principal(45),
            enable_sns_tracking: false,
            scan_interval_seconds: 600,
            cycles_interval_seconds: 604_800,
            min_tx_e8s: 100_000_000,
            max_cycles_entries_per_canister: 100,
            max_commitment_entries_per_canister: 100,
            max_index_pages_per_tick: 10,
            max_canisters_per_cycles_tick: 25,
            relay_factory_enabled: true,
            relay_setup_min_e8s: 300_000_000,
            relay_initial_cycles: 2_000_000_000_000,
            relay_cycle_safety_margin_e8s: 5_000_000,
            relay_min_subaccount_one_seed_e8s: 100_020_000,
            self_service_relay_interval_seconds: 86_400,
            io_surplus_neuron_id: 1,
            canonical_relay_canister_id: Some(principal(46)),
            canonical_relay_targets: vec![principal(47)],
        }
    }

    #[test]
    fn validates_bounds_and_forbidden_principals() {
        let cfg = config();
        let historian = principal(50);
        assert!(CanonicalRelayTargetSet::new(Vec::new(), &cfg, historian).is_err());
        assert!(CanonicalRelayTargetSet::new(vec![principal(1)], &cfg, historian).is_ok());
        assert!(
            CanonicalRelayTargetSet::new((100..120).map(principal).collect(), &cfg, historian)
                .is_ok()
        );
        assert!(
            CanonicalRelayTargetSet::new((100..121).map(principal).collect(), &cfg, historian)
                .is_err()
        );
        for forbidden in [
            Principal::anonymous(),
            Principal::management_canister(),
            historian,
            cfg.ledger_canister_id,
            cfg.index_canister_id,
            cfg.cmc_canister_id.unwrap(),
        ] {
            assert!(CanonicalRelayTargetSet::new(vec![forbidden], &cfg, historian).is_err());
        }
    }

    #[test]
    fn structural_bounds_are_independent_of_self_service_policy() {
        let cfg = config();
        let historian = principal(50);
        let twenty = CanonicalRelayTargetSet::canonicalize(structural_principals(20)).unwrap();
        assert!(twenty.validate_for_new_setup(&cfg, historian).is_ok());

        let twenty_one = CanonicalRelayTargetSet::canonicalize(structural_principals(21)).unwrap();
        assert!(twenty_one.validate_for_new_setup(&cfg, historian).is_err());
        assert!(CanonicalRelayTargetSet::canonicalize(structural_principals(255)).is_ok());
        assert!(CanonicalRelayTargetSet::canonicalize(structural_principals(256)).is_err());
    }

    #[test]
    fn canonical_order_rejects_duplicates_and_drives_hash() {
        let cfg = config();
        let historian = principal(50);
        let ab = CanonicalRelayTargetSet::new(vec![principal(2), principal(1)], &cfg, historian)
            .unwrap();
        let ba = CanonicalRelayTargetSet::new(vec![principal(1), principal(2)], &cfg, historian)
            .unwrap();
        assert_eq!(ab.targets(), &[principal(1), principal(2)]);
        assert_eq!(ab.key(), ba.key());
        assert!(
            CanonicalRelayTargetSet::new(vec![principal(1), principal(1)], &cfg, historian)
                .is_err()
        );
        assert_ne!(
            ab.key(),
            CanonicalRelayTargetSet::new(vec![principal(1)], &cfg, historian)
                .unwrap()
                .key()
        );
    }
}
