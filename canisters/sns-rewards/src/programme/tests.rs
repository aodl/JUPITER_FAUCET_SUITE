use super::{discovery::*, types::*};
use candid::Principal;

fn principal(value: u8) -> Principal {
    Principal::from_slice(&[value, 1])
}
pub(super) fn config() -> ProgrammeConfig {
    ProgrammeConfig {
        version: 7,
        jup_root: principal(1),
        relay_factory: None,
        icp_ledger: jupiter_ic_clients::constants::icp_ledger_id(),
        icp_index: jupiter_ic_clients::constants::icp_index_id(),
        staking_account: icrc_ledger_types::icrc1::account::Account {
            owner: jupiter_ic_clients::constants::nns_governance_id(),
            subaccount: Some([1; 32]),
        },
        canonical_relay: principal(6),
        treasury_stake: None,
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
        permission_type: vec![3, 4, 10],
    });
    assert_eq!(economic_owner(&n), Some(principal(7)));
    assert!(compound_eligible(
        &n,
        principal(7),
        &config(),
        principal(42)
    ));
    assert!(!compound_eligible(
        &n,
        principal(8),
        &config(),
        principal(42)
    ));
    n.permissions[0].permission_type.retain(|code| *code != 5);
    assert_eq!(economic_owner(&n), None);
}

#[test]
fn native_preference_exact_duration_positive_stake_and_protocol_exclusion_are_independent() {
    let cfg = config();
    let mut n = neuron();
    assert!(compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.auto_stake_maturity = Some(false);
    assert!(!compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.auto_stake_maturity = None;
    assert!(!compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.auto_stake_maturity = Some(true);
    n.dissolve_state = Some(DissolveState::WhenDissolvedTimestampSeconds(u64::MAX));
    assert_eq!(
        economic_owner(&n),
        Some(principal(7)),
        "ordinary identity does not require a non-dissolving one-year commitment"
    );
    assert!(!compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.dissolve_state = Some(DissolveState::DissolveDelaySeconds(
        crate::economics::get().year_seconds + 1,
    ));
    assert!(!compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.dissolve_state = Some(DissolveState::DissolveDelaySeconds(
        crate::economics::get().year_seconds / 2,
    ));
    assert_eq!(economic_owner(&n), Some(principal(7)));
    assert!(
        !compound_eligible(&n, principal(7), &cfg, principal(42)),
        "native six-month voting eligibility cannot broaden direct 2x eligibility"
    );
    n.dissolve_state = neuron().dissolve_state;
    n.neuron_fees_e8s = n.cached_neuron_stake_e8s;
    assert!(!compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.staked_maturity_e8s_equivalent = Some(1);
    assert!(compound_eligible(&n, principal(7), &cfg, principal(42)));
    n.id.as_mut().unwrap().id = vec![9; 32];
    assert!(!compound_eligible(&n, principal(7), &cfg, principal(42)));
}

#[test]
fn snapshots_fail_closed_for_missing_source_configuration_change_or_staleness() {
    let cfg = config();
    let mut snapshot = Snapshot {
        id: 1,
        config_version: 7,
        sources: vec![Source {
            root: principal(1),
            governance: principal(10),
            ledger: principal(11),
        }],
        started_at_seconds: 1,
        completed_at_seconds: 2,
        slot: 0,
        owners: 1,
    };
    assert!(snapshot.valid_for(&cfg, 2));
    snapshot.sources.push(snapshot.sources[0].clone());
    assert!(!snapshot.matches_configuration(&cfg));
    assert!(
        !snapshot.valid_for(&cfg, 2),
        "old two-source metadata is never secure jUP recognition"
    );
    snapshot.sources.pop();
    assert!(!snapshot.valid_for(&cfg, 1));
    assert!(!snapshot.valid_for(&cfg, 172803));
    snapshot.config_version += 1;
    assert!(!snapshot.valid_for(&cfg, 2));
    snapshot.config_version = 7;
    snapshot.sources.pop();
    assert!(!snapshot.valid_for(&cfg, 2));
}

#[test]
fn activation_preserves_launch_and_role_checks_with_combined_finance() {
    let mut cfg = config();
    assert!(cfg.validate().is_ok());
    cfg.launch_timestamp_seconds = None;
    assert!(cfg.validate().is_err());
    cfg.patron_enabled = false;
    cfg.treasury_stake = Some(TreasuryStakeBinding {
        disburser: principal(20),
        neuron_id: 42,
    });
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
    assert!(cfg.validate().is_ok());
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
fn unsafe_low_id_candidate_cannot_poison_valid_neuron_or_win_compound_selection() {
    let mut unsafe_neuron = neuron();
    unsafe_neuron.id.as_mut().unwrap().id = vec![0; 32];
    unsafe_neuron
        .permissions
        .push(crate::clients::NeuronPermission {
            principal: Some(principal(8)),
            permission_type: vec![crate::generated::NeuronPermissionType::ManagePrincipals as i32],
        });
    assert_eq!(economic_owner(&unsafe_neuron), None);
    let safe = neuron();
    super::storage::clear(0);
    for candidate in [&unsafe_neuron, &safe] {
        if let Some(owner) = economic_owner(candidate) {
            super::storage::insert(
                0,
                owner,
                compound_eligible(candidate, owner, &config(), principal(42))
                    .then(|| candidate.id.as_ref().unwrap().id.clone()),
            );
        }
    }
    let owner = super::storage::lookup(
        0,
        jupiter_ic_clients::account_identifier::account_identifier_bytes(principal(7), None),
    )
    .unwrap();
    assert_eq!(owner.compound_neuron, safe.id.map(|n| n.id));
}

#[test]
fn treasury_requires_explicit_neuron_binding_and_cannot_retarget_it() {
    let mut cfg = config();
    cfg.automation = Some(AutomationConfig {
        proposer_neuron: vec![9; 32],
        targets: vec![],
        treasury_enabled: true,
        treasury_retain_icp_e8s: 0,
        treasury_max_proposal_icp_e8s: 1_000_000_000,
    });
    assert!(cfg.validate().is_err());
    cfg.treasury_stake = Some(TreasuryStakeBinding {
        disburser: principal(20),
        neuron_id: 42,
    });
    assert!(cfg.validate().is_ok());
    let mut changed = cfg.clone();
    changed.treasury_stake.as_mut().unwrap().neuron_id = 43;
    assert!(!changed.preserves_financial_bindings(&cfg));
    changed = cfg.clone();
    changed.staking_account.subaccount = None;
    assert!(changed.validate().is_err());
    changed = cfg;
    changed.treasury_stake.as_mut().unwrap().disburser = Principal::anonymous();
    assert!(changed.validate().is_err());
}

#[test]
fn protocol_id_exclusion_does_not_blacklist_the_same_principals_valid_neuron() {
    let mut protocol = neuron();
    protocol.id.as_mut().unwrap().id = vec![8; 32];
    let external = neuron();
    assert_eq!(economic_owner(&protocol), economic_owner(&external));
    assert!(!compound_eligible(
        &protocol,
        principal(7),
        &config(),
        principal(42)
    ));
    assert!(compound_eligible(
        &external,
        principal(7),
        &config(),
        principal(42)
    ));
}

#[test]
fn canonical_financial_identity_cannot_be_replaced_by_dependency_self_assertions() {
    let mut cfg = config();
    assert!(cfg.validate().is_ok());
    cfg.icp_index = principal(21);
    assert!(cfg.validate().is_err());
    cfg = config();
    cfg.icp_ledger = principal(22);
    assert!(cfg.validate().is_err());
    cfg = config();
    cfg.staking_account.owner = principal(23);
    assert!(cfg
        .validate()
        .unwrap_err()
        .contains("canonical NNS Governance"));
    cfg = config();
    cfg.staking_account.subaccount = None;
    assert!(cfg.validate().unwrap_err().contains("staking subaccount"));
}

#[test]
fn reviewed_genesis_ids_are_excluded_without_optional_configuration() {
    use sha2::{Digest, Sha256};
    let rewards = principal(42);
    let id = |memo: u64| {
        Sha256::digest(
            [
                b"\x0cneuron-stake".as_slice(),
                rewards.as_slice(),
                &memo.to_be_bytes(),
            ]
            .concat(),
        )
        .to_vec()
    };
    for automated in [false, true] {
        let mut cfg = config();
        cfg.excluded_jup_neurons = vec![cfg.protocol_governance_neuron.clone()];
        if automated {
            cfg.excluded_jup_neurons.push(id(3));
            cfg.automation = Some(AutomationConfig {
                proposer_neuron: id(3),
                targets: vec![],
                treasury_enabled: false,
                treasury_retain_icp_e8s: 0,
                treasury_max_proposal_icp_e8s: 0,
            });
        }
        assert!(cfg.validate().is_ok());
        assert!(cfg.reserve_transport_neuron.is_none());
        for excluded in [&cfg.protocol_governance_neuron, &id(2), &id(3)] {
            assert!(cfg.excludes_neuron(excluded, rewards));
            let mut n = neuron();
            n.id.as_mut().unwrap().id = excluded.clone();
            assert!(!compound_eligible(&n, principal(7), &cfg, rewards));
        }
        assert!(
            !cfg.excludes_neuron(&id(9), rewards),
            "same principal, ordinary neuron"
        );
        assert!(
            !cfg.excludes_neuron(&id(2), principal(43)),
            "a different canister has different native genesis IDs"
        );
    }
}

fn pending_relay_record(block: u64) -> PatronRecord {
    use icrc_ledger_types::icrc1::account::Account;
    let cfg = config();
    let snapshot = Snapshot {
        id: 1,
        config_version: cfg.version,
        sources: vec![Source {
            root: cfg.jup_root,
            governance: principal(10),
            ledger: principal(11),
        }],
        started_at_seconds: 1,
        completed_at_seconds: 2,
        slot: 0,
        owners: 0,
    };
    let reserve = Account {
        owner: principal(42),
        subaccount: None,
    };
    PatronRecord {
        contribution_block: block,
        snapshot_id: 1,
        ownership_snapshot: snapshot.clone(),
        recognition: None,
        route: PatronRoute::PendingRelay {
            relay: principal(43),
        },
        icp_ledger: cfg.icp_ledger,
        staking_account: cfg.staking_account,
        relay_factory: Some(principal(44)),
        source_spender: None,
        commitment_memo: b"canonical source proof".to_vec(),
        config_version: cfg.version,
        contributor_icp_account: vec![1; 32],
        commitment_timestamp_seconds: 10,
        qualifying_icp_e8s: 1_000_000_000,
        launch_timestamp_seconds: 100,
        compounded_neuron: None,
        ledger: principal(11),
        from: reserve,
        to: reserve,
        amount_e8s: 0,
        fee_e8s: 0,
        memo: vec![1],
        created_at_time_nanos: 10,
        outcome: PatronOutcome::AwaitingRelayMembership {
            lookup: None,
            reason: "Pending".into(),
        },
        refresh_result: None,
    }
}

#[test]
fn deferred_relay_journal_is_idempotent_bounded_and_does_not_rewind_history() {
    use super::{patron, storage};
    storage::clear_records_for_test();
    storage::mutate(|s| *s = Runtime::default());
    let mut record = pending_relay_record(100);
    let snapshot = record.ownership_snapshot.clone();
    storage::defer_relay(&record);
    storage::defer_relay(&record);
    assert_eq!(storage::read().relay_retries.unwrap().outstanding, 1);
    for block in 101..140 {
        let mut terminal = record.clone();
        terminal.contribution_block = block;
        terminal.outcome = PatronOutcome::Skipped {
            reason: "unrelated terminal record".into(),
        };
        storage::put(&terminal);
    }
    let page = storage::retry_page(None, 139, 32);
    assert_eq!(page.len(), 32);
    assert_eq!(page.last().unwrap().contribution_block, 131);
    assert_eq!(storage::retry_page(Some(131), 139, 32).len(), 8);
    assert!(storage::retry_page(Some(139), 139, 32).is_empty());
    storage::mutate(|s| {
        s.pending_block = Some(record.contribution_block);
        s.batch = Some(Batch {
            snapshot,
            end_block: 200,
            previous_end: None,
            cursor: Some(50),
            oldest: Some(1),
        });
        s.relay_retries.as_mut().unwrap().dispatching_block = Some(record.contribution_block);
    });
    record.outcome = PatronOutcome::Verified {
        block: 7_u64.into(),
        duplicate: false,
    };
    record.amount_e8s = 100;
    storage::put(&record);
    patron::finish(&record);
    let state = storage::read();
    assert_eq!(
        state.batch.unwrap().cursor,
        Some(50),
        "retry may not rewind primary history"
    );
    assert_eq!(state.relay_retries.unwrap().outstanding, 0);
    assert_eq!(state.verified_e8s, 100);
    patron::finish(&record);
    assert_eq!(
        storage::read().verified_e8s,
        100,
        "duplicate completion is not another payment"
    );
}

#[test]
fn a_finite_retry_pass_cannot_be_extended_by_continuous_new_pending_records() {
    use super::{patron, storage};
    storage::clear_records_for_test();
    storage::mutate(|s| *s = Runtime::default());
    storage::defer_relay(&pending_relay_record(100));
    for block in 101..140 {
        let mut terminal = pending_relay_record(block);
        terminal.outcome = PatronOutcome::Skipped {
            reason: "terminal".into(),
        };
        storage::put(&terminal);
    }
    assert_eq!(
        patron::select_relay_retry()
            .unwrap()
            .unwrap()
            .contribution_block,
        100
    );
    assert_eq!(storage::read().relay_retries.unwrap().end_block, Some(139));
    for new_block in 140..143 {
        storage::defer_relay(&pending_relay_record(new_block));
        assert!(patron::select_relay_retry().unwrap().is_none());
    }
    // The 32-record page, remaining seven terminals and end-of-pass check all
    // finish at the pinned bound despite a new pending opportunity on each tick.
    assert_eq!(storage::read().relay_retries.unwrap().end_block, None);
    storage::defer_relay(&pending_relay_record(143));
    assert_eq!(
        patron::select_relay_retry()
            .unwrap()
            .unwrap()
            .contribution_block,
        100,
        "the first opportunity must be revisited even with continuous appends"
    );
    assert_eq!(storage::read().relay_retries.unwrap().end_block, Some(143));
}
