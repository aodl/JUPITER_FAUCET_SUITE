//! Canonical ballot weights and reward shares from the pinned official Governance.
//! Time advances normally; no neuron age fields or official Wasms are patched.
use super::*;
use candid::Nat;
use sha2::{Digest, Sha256};
use std::time::Duration;
use support::calls::tick_n;

const YEAR: u64 = 31_557_600;
const HALF: u64 = YEAR / 2;
const E8: u64 = 100_000_000;
const FEE: u64 = 1_000_000;

fn now(env: &Env) -> u64 {
    env.pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000
}
fn advance(env: &Env, seconds: u64) {
    env.pic.advance_time(Duration::from_secs(seconds));
    tick_n(&env.pic, 20);
}
fn neuron(env: &Env, id: &[u8]) -> Result<sns::Neuron> {
    let response: sns::GetNeuronResponse = query_one(
        &env.pic,
        env.jup["governance"],
        env.a,
        "get_neuron",
        sns::GetNeuron {
            neuron_id: Some(sns::NeuronId { id: id.to_vec() }),
        },
    )?;
    match response.result {
        Some(sns::Result::Neuron(n)) => Ok(n),
        other => anyhow::bail!("native neuron {other:?}"),
    }
}
fn configure(env: &Env, caller: Principal, id: &[u8], operation: sns::Operation) -> Result<()> {
    env.manage(
        env.jup["governance"],
        caller,
        id,
        sns::Command::Configure(sns::Configure {
            operation: Some(operation),
        }),
    )?;
    Ok(())
}
fn deposit(env: &Env, id: &[u8], amount: u64) -> Result<()> {
    support::ledger::icrc1_transfer(
        &env.pic,
        env.jup["ledger"],
        env.rewards,
        TransferArg {
            from_subaccount: None,
            to: Account {
                owner: env.jup["governance"],
                subaccount: Some(id.try_into().unwrap()),
            },
            amount: amount.into(),
            fee: Some(FEE.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    Ok(())
}
fn create(env: &Env, caller: Principal, memo: u64, amount: u64, delay: u64) -> Result<Vec<u8>> {
    let id = Sha256::digest(
        [
            b"\x0cneuron-stake".as_slice(),
            caller.as_slice(),
            &memo.to_be_bytes(),
        ]
        .concat(),
    )
    .to_vec();
    deposit(env, &id, amount)?;
    let reply = env.manage(
        env.jup["governance"],
        caller,
        &[],
        sns::Command::ClaimOrRefresh(sns::ClaimOrRefresh {
            by: Some(sns::By::MemoAndController(sns::MemoAndController {
                memo,
                controller: Some(caller),
            })),
        }),
    )?;
    assert!(
        matches!(reply, sns::Command1::ClaimOrRefresh(r) if r.refreshed_neuron_id.as_ref().is_some_and(|n| n.id == id))
    );
    configure(
        env,
        caller,
        &id,
        sns::Operation::IncreaseDissolveDelay(sns::IncreaseDissolveDelay {
            additional_dissolve_delay_seconds: delay.try_into().unwrap(),
        }),
    )?;
    Ok(id)
}
fn proposal(env: &Env) -> Result<sns::ProposalData> {
    proposal_by(env, env.rewards, &env.jup_neurons["proposer"])
}
fn proposal_by(env: &Env, caller: Principal, neuron: &[u8]) -> Result<sns::ProposalData> {
    let reply = env.manage(
        env.jup["governance"],
        caller,
        neuron,
        sns::Command::MakeProposal(sns::Proposal {
            title: "Native economic regression".into(),
            summary: "Local canonical voting-power and participation evidence".into(),
            url: String::new(),
            action: Some(sns::Action::Motion(sns::Motion {
                motion_text: "No value transfer".into(),
            })),
        }),
    )?;
    let sns::Command1::MakeProposal(r) = reply else {
        anyhow::bail!("proposal reply")
    };
    get_proposal(env, r.proposal_id.unwrap().id)
}
fn get_proposal(env: &Env, id: u64) -> Result<sns::ProposalData> {
    let response: sns::GetProposalResponse = query_one(
        &env.pic,
        env.jup["governance"],
        env.a,
        "get_proposal",
        sns::GetProposal {
            proposal_id: Some(sns::ProposalId { id }),
        },
    )?;
    match response.result {
        Some(sns::Result1::Proposal(p)) => Ok(p),
        other => anyhow::bail!("native proposal {other:?}"),
    }
}
fn power(p: &sns::ProposalData, id: &[u8]) -> Option<u64> {
    p.ballots
        .iter()
        .find(|(key, _)| *key == hex::encode(id))
        .map(|(_, b)| b.voting_power)
}
fn vote(env: &Env, p: &sns::ProposalData, caller: Principal, id: &[u8], value: i32) -> Result<()> {
    env.manage(
        env.jup["governance"],
        caller,
        id,
        sns::Command::RegisterVote(sns::RegisterVote {
            proposal: p.id.clone(),
            vote: value,
        }),
    )?;
    Ok(())
}
// Integer oracle transcribed from the pinned official neuron.rs::voting_power.
// The assertion observes actual proposal ballots, not an application VP field.
fn expected(n: &sns::Neuron, at: u64) -> u64 {
    let stake = u128::from(
        n.cached_neuron_stake_e8s - n.neuron_fees_e8s
            + n.staked_maturity_e8s_equivalent.unwrap_or(0),
    );
    let delay = match n.dissolve_state {
        Some(sns::DissolveState::DissolveDelaySeconds(d)) => d,
        Some(sns::DissolveState::WhenDissolvedTimestampSeconds(t)) => t.saturating_sub(at),
        None => 0,
    }
    .min(YEAR);
    let age = match n.dissolve_state {
        Some(sns::DissolveState::DissolveDelaySeconds(d)) if d > 0 => at
            .saturating_sub(n.aging_since_timestamp_seconds)
            .min(4 * YEAR),
        _ => 0,
    };
    let duration_weight = stake + stake * u128::from(delay) / u128::from(YEAR);
    ((duration_weight + duration_weight * u128::from(age) * 2 / u128::from(4 * YEAR))
        * u128::from(n.voting_power_percentage_multiplier)
        / 100)
        .try_into()
        .unwrap()
}
fn shares(env: &Env, id: &[u8], end: u64) -> Result<Nat> {
    let n = neuron(env, id)?;
    assert_eq!(n.maturity_e8s_equivalent, 0);
    assert_eq!(n.staked_maturity_e8s_equivalent.unwrap_or(0), 0);
    let record = n
        .latest_reward_event_participation
        .context("native shares missing at zero inflation")?;
    assert_eq!(record.reward_event_end_timestamp_seconds, Some(end));
    record.reward_shares.context("native share amount")
}
fn settle(env: &Env, p: &sns::ProposalData) -> Result<sns::RewardEvent> {
    advance(env, 6 * 86400);
    let event: sns::RewardEvent = query_one(
        &env.pic,
        env.jup["governance"],
        env.a,
        "get_latest_reward_event",
        (),
    )?;
    assert_eq!(event.distributed_e8s_equivalent, 0);
    assert!(
        event
            .settled_proposals
            .iter()
            .any(|id| Some(id) == p.id.as_ref()),
        "{event:?}"
    );
    env.drive(12)?;
    Ok(event)
}

#[test]
#[ignore = "real official SNS age/delay ballots, Yes/No shares and zero issuance"]
fn canonical_age_delay_and_zero_inflation_participation() -> Result<()> {
    let env = Env::setup_with_economics(false, false, false, false, true)?;
    let old = &env.jup_neurons["a"];
    let peer = &env.jup_neurons["b"];
    let protocol = &env.jup_neurons["nnx_fixture"];
    let start = neuron(&env, old)?.aging_since_timestamp_seconds;
    let initial_supply: Nat =
        query_one(&env.pic, env.jup["ledger"], env.a, "icrc1_total_supply", ())?;
    assert_eq!(initial_supply, Nat::from(100_000_000_u64 * E8));
    let mut early_concentration = None;
    for (index, halves) in [0, 1, 2, 4, 6, 8, 10].into_iter().enumerate() {
        let target = start + halves * HALF;
        if now(&env) < target {
            advance(&env, target - now(&env));
        }
        let fresh_owner = Principal::self_authenticating(format!("fresh at half-year {halves}"));
        let fresh = create(&env, fresh_owner, 100 + index as u64, 1000 * E8, YEAR)?;
        let old_n = neuron(&env, old)?;
        let peer_n = neuron(&env, peer)?;
        let fresh_n = neuron(&env, &fresh)?;
        let protocol_n = neuron(&env, protocol)?;
        assert_eq!(protocol_n.cached_neuron_stake_e8s, 10_000_000 * E8);
        let p = proposal(&env)?;
        let at = p.proposal_creation_timestamp_seconds;
        for (id, n) in [
            (old.as_slice(), &old_n),
            (peer.as_slice(), &peer_n),
            (fresh.as_slice(), &fresh_n),
            (protocol.as_slice(), &protocol_n),
        ] {
            assert_eq!(
                power(&p, id),
                Some(expected(n, at)),
                "canonical weight at half-year {halves}: {n:?}"
            );
        }
        let old_power = power(&p, old).unwrap();
        let fresh_power = power(&p, &fresh).unwrap();
        let numerator = 4 + halves.min(8);
        assert!(
            (i128::from(old_power) * 4 - i128::from(fresh_power) * i128::from(numerator)).abs()
                < 1_000_000,
            "age factor at half-year {halves}: old={old_power}, fresh={fresh_power}"
        );
        let total = p
            .ballots
            .iter()
            .map(|(_, b)| u128::from(b.voting_power))
            .sum::<u128>();
        let concentration = u128::from(power(&p, protocol).unwrap()) * 1_000_000 / total;
        if halves == 0 {
            early_concentration = Some(concentration);
        }
        if halves == 8 {
            eprintln!(
                "native governance ppm: fresh={}, four-year={concentration}",
                early_concentration.unwrap()
            );
        }
        vote(&env, &p, env.a, old, 1)?;
        vote(&env, &p, env.b, peer, 2)?;
        vote(&env, &p, fresh_owner, &fresh, 2)?;
        vote(
            &env,
            &p,
            Principal::self_authenticating(b"NNX ROLE FIXTURE ONLY"),
            protocol,
            1,
        )?;
        // Changing preferences after a ballot does not rewrite its reward weight.
        if halves == 10 {
            configure(&env, env.a, old, sns::Operation::StartDissolving {})?;
            assert_eq!(neuron(&env, old)?.aging_since_timestamp_seconds, u64::MAX);
            assert_eq!(
                power(&get_proposal(&env, p.id.as_ref().unwrap().id)?, old),
                Some(old_power)
            );
        }
        let supply_before: Nat =
            query_one(&env.pic, env.jup["ledger"], env.a, "icrc1_total_supply", ())?;
        let event = settle(&env, &p)?;
        let end = event.end_timestamp_seconds.context("event end")?;
        let a = shares(&env, old, end)?;
        let b = shares(&env, peer, end)?;
        let f = shares(&env, &fresh, end)?;
        let excluded = shares(&env, protocol, end)?;
        assert_eq!(a, Nat::from(old_power));
        assert_eq!(
            a, b,
            "equal native age/delay factors cancel for equal participating stakes"
        );
        assert_eq!(
            b,
            Nat::from(power(&p, peer).unwrap()),
            "No votes count as Yes votes do"
        );
        assert_eq!(f, Nat::from(fresh_power));
        assert!(excluded > 0_u8);
        let participation: ParticipationState =
            query_one(&env.pic, env.rewards, env.a, "get_participation_status", ())?;
        let active = participation.active.context("published participation")?;
        assert_eq!(active.end_timestamp_seconds, end);
        assert_eq!(
            active.eligible_shares.0,
            &a.0 + &b.0 + &f.0,
            "exclude protocol shares without removing its governance power"
        );
        // This fixed inventory is a labelled arithmetic fixture, not an IO receipt.
        let inventory = 1_200_000_000_u64;
        for (owner, weight) in [(env.a, &a), (env.b, &b), (fresh_owner, &f)] {
            let quote: Option<Nat> = support::calls::update_bytes(
                &env.pic,
                env.rewards,
                env.a,
                "quote_gross_io_share",
                encode_args((inventory, owner))?,
            )?;
            assert_eq!(
                quote.unwrap().0,
                Nat::from(inventory).0 * &weight.0 / &active.eligible_shares.0
            );
        }
        let protocol_quote: Option<Nat> = support::calls::update_bytes(
            &env.pic,
            env.rewards,
            env.a,
            "quote_gross_io_share",
            encode_args((inventory, protocol_n.permissions[0].principal.unwrap()))?,
        )?;
        assert!(protocol_quote.is_none());
        let supply_after: Nat =
            query_one(&env.pic, env.jup["ledger"], env.a, "icrc1_total_supply", ())?;
        assert_eq!(
            supply_after, supply_before,
            "a zero purse cannot mint voting rewards"
        );
        eprintln!("native age half-years={halves}: old={old_power}, fresh={fresh_power}, shares={a}/{f}, distributed=0");
    }
    let six = create(&env, env.b, 201, 1000 * E8, HALF)?;
    let above = create(&env, env.b, 202, 1000 * E8, HALF + 1)?;
    let below = create(&env, env.b, 203, 1000 * E8, HALF - 1)?;
    let year = create(&env, env.b, 204, 1000 * E8, YEAR)?;
    let p = proposal(&env)?;
    for id in [&six, &above, &year, old] {
        assert_eq!(
            power(&p, id),
            Some(expected(
                &neuron(&env, id)?,
                p.proposal_creation_timestamp_seconds
            ))
        );
    }
    assert_eq!(power(&p, &below), None);
    assert!(vote(&env, &p, env.b, &below, 1).is_err());
    assert!(
        (i128::from(power(&p, &year).unwrap()) * 3 - i128::from(power(&p, &six).unwrap()) * 4)
            .abs()
            < 1_000_000
    );
    // A one-year dissolving neuron retains voting eligibility above six months,
    // with age zero. Below that boundary it cannot enter a new proposal ballot.
    assert!(matches!(
        neuron(&env, old)?.dissolve_state,
        Some(sns::DissolveState::WhenDissolvedTimestampSeconds(_))
    ));
    advance(&env, HALF + 1);
    let p = proposal(&env)?;
    assert_eq!(power(&p, old), None);
    configure(&env, env.a, old, sns::Operation::StopDissolving {})?;
    let stopped = neuron(&env, old)?;
    assert!(now(&env).saturating_sub(stopped.aging_since_timestamp_seconds) < 10);
    assert!(
        matches!(stopped.dissolve_state, Some(sns::DissolveState::DissolveDelaySeconds(d)) if d < HALF)
    );
    Ok(())
}

fn top_up(env: &Env, caller: Principal, id: &[u8], amount: u64) -> Result<()> {
    let before = neuron(env, id)?;
    deposit(env, id, amount)?;
    let from = now(env);
    env.manage(
        env.jup["governance"],
        caller,
        id,
        sns::Command::ClaimOrRefresh(sns::ClaimOrRefresh {
            by: Some(sns::By::NeuronId {}),
        }),
    )?;
    let to = now(env);
    let after = neuron(env, id)?;
    assert_eq!(
        after.cached_neuron_stake_e8s,
        before.cached_neuron_stake_e8s + amount
    );
    let expected_since = |at: u64| {
        at - u64::try_from(
            u128::from(at - before.aging_since_timestamp_seconds)
                * u128::from(before.cached_neuron_stake_e8s)
                / u128::from(after.cached_neuron_stake_e8s),
        )
        .unwrap()
    };
    assert!(
        (expected_since(from)..=expected_since(to)).contains(&after.aging_since_timestamp_seconds),
        "native weighted age: {before:?} -> {after:?}"
    );
    Ok(())
}

#[test]
#[ignore = "real official SNS top-up/refresh, capped age, split and genuine gift"]
fn native_stake_weighted_age_split_and_genuine_transfer() -> Result<()> {
    let env = Env::setup_with_economics(false, false, false, false, true)?;
    let small = create(&env, env.a, 300, E8, YEAR)?;
    advance(&env, 2 * YEAR);
    top_up(&env, env.a, &small, 999 * E8)?;
    assert!(now(&env) - neuron(&env, &small)?.aging_since_timestamp_seconds < YEAR / 100);
    top_up(&env, env.a, &env.jup_neurons["a"], 1000 * E8)?;
    let equal_age = now(&env) - neuron(&env, &env.jup_neurons["a"])?.aging_since_timestamp_seconds;
    assert!((YEAR..YEAR + 100).contains(&equal_age));
    advance(&env, 4 * YEAR);
    let capped = &env.jup_neurons["b"];
    top_up(&env, env.b, capped, 1000 * E8)?;
    let cap_age = now(&env) - neuron(&env, capped)?.aging_since_timestamp_seconds;
    assert!(
        (3 * YEAR..3 * YEAR + 100).contains(&cap_age),
        "stored age beyond the voting cap participates in native dilution"
    );
    advance(&env, HALF);
    top_up(&env, env.b, capped, 500 * E8)?;
    let before_refresh = neuron(&env, capped)?;
    env.manage(
        env.jup["governance"],
        env.b,
        capped,
        sns::Command::ClaimOrRefresh(sns::ClaimOrRefresh {
            by: Some(sns::By::NeuronId {}),
        }),
    )?;
    assert_eq!(
        neuron(&env, capped)?.aging_since_timestamp_seconds,
        before_refresh.aging_since_timestamp_seconds
    );
    let past = proposal(&env)?;
    vote(&env, &past, env.b, capped, 2)?;
    vote(
        &env,
        &past,
        Principal::self_authenticating(b"NNX ROLE FIXTURE ONLY"),
        &env.jup_neurons["nnx_fixture"],
        1,
    )?;
    settle(&env, &past)?;
    let before = neuron(&env, capped)?;
    assert!(before.latest_reward_event_participation.is_some());
    let split = env.manage(
        env.jup["governance"],
        env.b,
        capped,
        sns::Command::Split(sns::Split {
            memo: 301,
            amount_e8s: 100 * E8,
        }),
    )?;
    let sns::Command1::Split(split) = split else {
        anyhow::bail!("native split")
    };
    let child = split.created_neuron_id.unwrap().id;
    let after = neuron(&env, &child)?;
    assert_eq!(
        after.aging_since_timestamp_seconds,
        before.aging_since_timestamp_seconds
    );
    assert_eq!(after.cached_neuron_stake_e8s, 100 * E8 - FEE);
    assert!(
        after.latest_reward_event_participation.is_none(),
        "splitting cannot duplicate past participation"
    );
    let rights = after
        .permissions
        .iter()
        .find(|p| p.principal == Some(env.b))
        .unwrap()
        .permission_type
        .clone();
    env.manage(
        env.jup["governance"],
        env.b,
        &child,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.io_only),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: rights.clone(),
            }),
        }),
    )?;
    env.manage(
        env.jup["governance"],
        env.b,
        &child,
        sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
            principal_id: Some(env.b),
            permissions_to_remove: Some(sns::NeuronPermissionList {
                permissions: rights,
            }),
        }),
    )?;
    let gifted = neuron(&env, &child)?;
    assert_eq!(
        gifted.aging_since_timestamp_seconds,
        after.aging_since_timestamp_seconds
    );
    assert!(gifted
        .permissions
        .iter()
        .all(|p| p.principal != Some(env.b)));
    advance(&env, 86_401);
    env.drive(10)?;
    let context = env.relay_context()?;
    let owners: ResolveDefaultIcpAccountsResult = query_one(
        &env.pic,
        env.rewards,
        env.a,
        "resolve_default_icp_accounts",
        ResolveDefaultIcpAccountsArgs {
            snapshot_id: context.snapshot_id,
            account_identifiers: vec![
                jupiter_ic_clients::account_identifier::account_identifier_bytes(env.io_only, None)
                    .to_vec(),
            ],
        },
    )?;
    assert!(
        matches!(owners, ResolveDefaultIcpAccountsResult::Ok(v) if v == vec![Some(env.io_only)])
    );
    let p = proposal(&env)?;
    assert_eq!(
        power(&p, &child),
        Some(expected(&gifted, p.proposal_creation_timestamp_seconds))
    );
    vote(&env, &p, env.io_only, &child, 2)?;
    assert!(vote(&env, &p, env.b, &child, 1).is_err());
    Ok(())
}

#[test]
#[ignore = "official SNS protocol neurons manually vote with automation both enabled and absent"]
fn protocol_genesis_shares_are_excluded_without_automation_or_explicit_lists() -> Result<()> {
    let env = Env::setup_with_economics(false, false, false, false, true)?;
    env.drive(40)?;
    advance(&env, 1); // pass the official zero-vesting genesis timestamp boundary
    let reserve = &env.jup_neurons["reserve_transport"];
    let proposer = &env.jup_neurons["proposer"];
    let protocol = &env.jup_neurons["nnx_fixture"];
    let protocol_owner = neuron(&env, protocol)?.permissions[0].principal.unwrap();
    for (id, nonce) in [(reserve, 2_u64), (proposer, 3)] {
        let expected = Sha256::digest(
            [
                b"\x0cneuron-stake".as_slice(),
                env.rewards.as_slice(),
                &nonce.to_be_bytes(),
            ]
            .concat(),
        );
        assert_eq!(
            id.as_slice(),
            expected.as_slice(),
            "reviewed native genesis ID"
        );
    }
    configure(
        &env,
        env.rewards,
        reserve,
        sns::Operation::IncreaseDissolveDelay(sns::IncreaseDissolveDelay {
            additional_dissolve_delay_seconds: YEAR as u32,
        }),
    )?;
    // An ordinary neuron of the same principal must retain external shares.
    let ordinary = create(&env, env.rewards, 9, 100 * E8, YEAR)?;
    env.manage(
        env.jup["governance"],
        protocol_owner,
        protocol,
        sns::Command::AddNeuronPermissions(sns::AddNeuronPermissions {
            principal_id: Some(env.a),
            permissions_to_add: Some(sns::NeuronPermissionList {
                permissions: vec![sns::NeuronPermissionType::Vote as i32],
            }),
        }),
    )?;
    for automated in [false, true] {
        let mut config = env.status()?.config.unwrap();
        config.version += 1;
        config.patron_enabled = false;
        config.reserve_transport_neuron = None;
        config.excluded_jup_neurons = vec![protocol.clone()];
        config.automation = if automated {
            config.excluded_jup_neurons.push(proposer.clone());
            Some(AutomationConfig {
                proposer_neuron: proposer.clone(),
                targets: vec![],
                treasury_enabled: false,
                treasury_retain_icp_e8s: 0,
                treasury_max_proposal_icp_e8s: 0,
            })
        } else {
            None
        };
        config.validate().map_err(anyhow::Error::msg)?;
        for _ in 0..100 {
            env.pic.tick();
        }
        env.pic
            .upgrade_canister(
                env.rewards,
                env.rewards_wasm.clone(),
                encode_one(Some(jupiter_sns_rewards::UpgradeArgs {
                    reward_sns_root_canister_id: None,
                    programme: Some(config),
                }))?,
                Some(env.jup["root"]),
            )
            .map_err(|e| anyhow!("exclusion fixture configuration: {e:?}"))?;
        env.drive(12)?;
        let p = if automated {
            proposal(&env)? // manually submit with the normally narrowed proposer
        } else {
            proposal_by(&env, env.a, &env.jup_neurons["a"])?
        };
        if automated {
            vote(&env, &p, env.a, &env.jup_neurons["a"], 1)?;
        } else {
            vote(&env, &p, env.rewards, proposer, 1)?; // actual manual RegisterVote
        }
        vote(&env, &p, env.b, &env.jup_neurons["b"], 2)?;
        vote(&env, &p, env.rewards, reserve, 1)?;
        vote(&env, &p, env.rewards, &ordinary, 2)?;
        vote(&env, &p, protocol_owner, protocol, 1)?;
        let event = settle(&env, &p)?;
        let end = event.end_timestamp_seconds.unwrap();
        for id in [protocol, reserve, proposer] {
            assert!(
                shares(&env, id, end)? > 0_u8,
                "protocol voting power/participation is retained"
            );
            let recoverable = neuron(&env, id)?.permissions.iter().any(|p| {
                p.permission_type
                    .contains(&(sns::NeuronPermissionType::Disburse as i32))
            });
            assert_eq!(
                recoverable,
                !automated || id != proposer,
                "native automation retains its SubmitProposal-only narrowing"
            );
        }
        let a = shares(&env, &env.jup_neurons["a"], end)?;
        let b = shares(&env, &env.jup_neurons["b"], end)?;
        let ordinary_weight = shares(&env, &ordinary, end)?;
        let status: ParticipationState =
            query_one(&env.pic, env.rewards, env.a, "get_participation_status", ())?;
        let active = status.active.context("complete native event")?;
        assert_eq!(active.eligible_shares.0, &a.0 + &b.0 + &ordinary_weight.0);
        for (owner, weight) in [(env.a, &a), (env.b, &b), (env.rewards, &ordinary_weight)] {
            let quote: Option<Nat> = support::calls::update_bytes(
                &env.pic,
                env.rewards,
                env.a,
                "quote_gross_io_share",
                encode_args((1_000_000_u64, owner))?,
            )?;
            assert_eq!(
                quote.unwrap().0,
                Nat::from(1_000_000_u64).0 * &weight.0 / &active.eligible_shares.0
            );
        }
        let excluded: Option<Nat> = support::calls::update_bytes(
            &env.pic,
            env.rewards,
            env.a,
            "quote_gross_io_share",
            encode_args((1_000_000_u64, protocol_owner))?,
        )?;
        assert!(excluded.is_none());
        assert_eq!(event.distributed_e8s_equivalent, 0);
        // A same-configuration upgrade must not expose an aggregate collected
        // under a previous implementation's exclusion policy.
        for _ in 0..100 {
            env.pic.tick();
        }
        env.pic
            .upgrade_canister(
                env.rewards,
                env.rewards_wasm.clone(),
                encode_args(())?,
                Some(env.jup["root"]),
            )
            .map_err(|e| anyhow!("participation policy rebuild upgrade: {e:?}"))?;
        let reset: ParticipationState =
            query_one(&env.pic, env.rewards, env.a, "get_participation_status", ())?;
        assert!(reset.active.is_none() && reset.scan.is_none());
        let unavailable: Option<Nat> = support::calls::update_bytes(
            &env.pic,
            env.rewards,
            env.a,
            "quote_gross_io_share",
            encode_args((1_000_000_u64, env.a))?,
        )?;
        assert!(unavailable.is_none());
        env.drive(12)?;
        let rebuilt: ParticipationState =
            query_one(&env.pic, env.rewards, env.a, "get_participation_status", ())?;
        let rebuilt = rebuilt.active.context("recollected native event")?;
        assert_eq!(rebuilt.end_timestamp_seconds, active.end_timestamp_seconds);
        assert_eq!(rebuilt.eligible_shares, active.eligible_shares);
    }
    Ok(())
}
