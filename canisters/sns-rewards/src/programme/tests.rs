use super::{discovery::*, types::*};
use candid::Principal;

fn principal(value: u8) -> Principal {
    Principal::from_slice(&[value, 1])
}
fn config() -> ProgrammeConfig {
    ProgrammeConfig {
        version: 7,
        jup_root: principal(1),
        io_root: principal(2),
        icp_ledger: principal(3),
        icp_index: principal(4),
        staking_account: icrc_ledger_types::icrc1::account::Account {
            owner: principal(5),
            subaccount: Some([1; 32]),
        },
        canonical_relay: principal(6),
        protocol_governance_neuron: vec![8; 32],
        excluded_icp_accounts: vec![],
        excluded_jup_neurons: vec![vec![8; 32], vec![9; 32]],
        reserve_subaccount: None,
        reserve_transport_neuron: None,
        launch_timestamp_seconds: Some(100),
        patron_enabled: true,
        snapshot_max_age_seconds: 172800,
        automation: None,
    }
}
fn neuron() -> Neuron {
    Neuron {
        id: Some(crate::clients::NeuronId { id: vec![1; 32] }),
        permissions: vec![crate::clients::NeuronPermission {
            principal: Some(principal(7)),
            permission_type: vec![1, 2, 5, 9],
        }],
        cached_neuron_stake_e8s: 100,
        neuron_fees_e8s: 0,
        dissolve_state: Some(DissolveState::DissolveDelaySeconds(
            crate::economics::get().year_seconds,
        )),
        auto_stake_maturity: Some(true),
        staked_maturity_e8s_equivalent: None,
    }
}

#[test]
fn unrelated_voter_cannot_redirect_economic_ownership_or_compounding() {
    let mut n = neuron();
    n.permissions.push(crate::clients::NeuronPermission {
        principal: Some(principal(8)),
        permission_type: vec![1, 2, 3, 4, 6, 7, 8, 9, 10],
    });
    assert_eq!(economic_owner(&n), Some(principal(7)));
    assert!(compound_eligible(&n, principal(7), &config()));
    assert!(!compound_eligible(&n, principal(8), &config()));
    n.permissions[0].permission_type.retain(|code| *code != 5);
    assert_eq!(economic_owner(&n), None);
}

#[test]
fn native_preference_exact_duration_positive_stake_and_protocol_exclusion_are_independent() {
    let cfg = config();
    let mut n = neuron();
    assert!(compound_eligible(&n, principal(7), &cfg));
    n.auto_stake_maturity = Some(false);
    assert!(!compound_eligible(&n, principal(7), &cfg));
    n.auto_stake_maturity = None;
    assert!(!compound_eligible(&n, principal(7), &cfg));
    n.auto_stake_maturity = Some(true);
    n.dissolve_state = Some(DissolveState::WhenDissolvedTimestampSeconds(u64::MAX));
    assert!(!compound_eligible(&n, principal(7), &cfg));
    n.dissolve_state = Some(DissolveState::DissolveDelaySeconds(
        crate::economics::get().year_seconds + 1,
    ));
    assert!(!compound_eligible(&n, principal(7), &cfg));
    n.dissolve_state = neuron().dissolve_state;
    n.neuron_fees_e8s = n.cached_neuron_stake_e8s;
    assert!(!compound_eligible(&n, principal(7), &cfg));
    n.staked_maturity_e8s_equivalent = Some(1);
    assert!(compound_eligible(&n, principal(7), &cfg));
    n.id.as_mut().unwrap().id = vec![9; 32];
    assert!(!compound_eligible(&n, principal(7), &cfg));
}

#[test]
fn snapshots_fail_closed_for_missing_source_configuration_change_or_staleness() {
    let cfg = config();
    let mut snapshot = Snapshot {
        id: 1,
        config_version: 7,
        sources: vec![
            Source {
                root: principal(1),
                governance: principal(10),
                ledger: principal(11),
            },
            Source {
                root: principal(2),
                governance: principal(12),
                ledger: principal(13),
            },
        ],
        started_at_seconds: 1,
        completed_at_seconds: 2,
        slot: 0,
        owners: 1,
    };
    assert!(snapshot.valid_for(&cfg, 2));
    assert!(!snapshot.valid_for(&cfg, 1));
    assert!(!snapshot.valid_for(&cfg, 172803));
    snapshot.config_version += 1;
    assert!(!snapshot.valid_for(&cfg, 2));
    snapshot.config_version = 7;
    snapshot.sources.pop();
    assert!(!snapshot.valid_for(&cfg, 2));
}

#[test]
fn activation_rejects_unresolved_launch_role_collisions_and_unintegrated_combined_finance() {
    let mut cfg = config();
    assert!(cfg.validate().is_ok());
    cfg.launch_timestamp_seconds = None;
    assert!(cfg.validate().is_err());
    cfg.patron_enabled = false;
    cfg.automation = Some(AutomationConfig {
        proposer_neuron: vec![9; 32],
        targets: vec![],
        treasury_enabled: true,
        treasury_max_proposal_icp_e8s: 1_000_000_000,
        treasury_retain_icp_e8s: 0,
    });
    assert!(cfg.validate().is_ok());
    cfg.launch_timestamp_seconds = Some(100);
    cfg.patron_enabled = true;
    assert!(
        cfg.validate().is_err(),
        "combined treasury/patron activation must await mixed Relay provenance integration"
    );
    cfg.patron_enabled = false;
    cfg.reserve_transport_neuron = Some(vec![9; 32]);
    assert!(
        cfg.validate().is_err(),
        "reserve transport cannot be the operational proposer"
    );
}

#[test]
fn durable_error_evidence_is_bounded_utf8_safe_hashed_and_idempotent() {
    use sha2::{Digest, Sha256};
    let original = "💫".repeat(4000);
    let bounded = super::compact(&original);
    assert!(bounded.len() <= 2048);
    assert!(bounded.contains(&hex::encode(Sha256::digest(original.as_bytes()))));
    assert_eq!(super::compact(&bounded), bounded);
    assert_eq!(super::compact("BadFee actual=10"), "BadFee actual=10");
}

#[test]
fn reconfiguration_cannot_retarget_existing_capital_or_commitments() {
    let old = config();
    let mut next = old.clone();
    next.version += 1;
    next.io_root = principal(20);
    assert!(next.preserves_financial_bindings(&old));
    next.jup_root = principal(21);
    assert!(!next.preserves_financial_bindings(&old));
    next = old.clone();
    next.reserve_subaccount = Some([1; 32]);
    assert!(!next.preserves_financial_bindings(&old));
    next = old.clone();
    next.reserve_transport_neuron = Some(vec![1; 32]);
    assert!(!next.preserves_financial_bindings(&old));
    next = old.clone();
    next.staking_account.subaccount = Some([2; 32]);
    assert!(!next.preserves_financial_bindings(&old));
}

#[test]
fn production_activation_rejects_even_direct_only_patron_and_treasury_configs() {
    let mut cfg = config();
    assert!(cfg.validate().is_ok());
    assert!(cfg
        .validate_activation(false)
        .unwrap_err()
        .contains("provenance"));
    assert!(
        cfg.validate_activation(true).is_ok(),
        "explicit local direct-payment fixture"
    );
    cfg.patron_enabled = false;
    assert!(cfg.validate_activation(false).is_ok());
    cfg.automation = Some(AutomationConfig {
        proposer_neuron: vec![9; 32],
        targets: vec![],
        treasury_enabled: true,
        treasury_max_proposal_icp_e8s: 1_000_000_000,
        treasury_retain_icp_e8s: 0,
    });
    assert!(cfg.validate().is_ok());
    assert!(cfg.validate_activation(false).is_err());
}
