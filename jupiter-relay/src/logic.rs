use std::collections::{BTreeMap, BTreeSet};

use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;

use crate::state::{
    CanisterBurnSample, Config, ConversionEstimate, CyclesSampleSource, CyclesSnapshot,
    SurplusRecipient, SurplusTarget, SurplusTransferSample,
};

pub(crate) const MEMO_TOP_UP_CANISTER_U64: u64 = 1_347_768_404;
pub(crate) const TOPUP_HEADROOM_NUMERATOR: u128 = 101;
pub(crate) const TOPUP_HEADROOM_DENOMINATOR: u128 = 100;
pub(crate) const CONVERSION_ESTIMATE_MAX_AGE_NANOS: u64 = 14 * 24 * 60 * 60 * 1_000_000_000;
const BOOTSTRAP_CYCLES_PER_E8: u128 = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BurnPlan {
    pub canister_id: Principal,
    pub previous_cycles: Option<u128>,
    pub current_cycles: u128,
    pub relay_minted_cycles: u128,
    pub burn_cycles: u128,
    pub target_topup_cycles: u128,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub actual_minted_cycles: u128,
    pub skipped_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedSurplusRecipient {
    pub target: SurplusTarget,
    pub account: Account,
    pub memo: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AllocationPlan {
    pub topups: Vec<BurnPlan>,
    pub topup_phase_fully_funded: bool,
    pub skipped_surplus_reason: Option<String>,
}

pub(crate) fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    let bytes = principal.as_slice();
    let mut out = [0u8; 32];
    out[0] = bytes.len() as u8;
    let len = bytes.len().min(31);
    out[1..1 + len].copy_from_slice(&bytes[..len]);
    out
}

pub(crate) fn cmc_deposit_account(cmc_id: Principal, canister_id: Principal) -> Account {
    Account {
        owner: cmc_id,
        subaccount: Some(principal_to_subaccount(canister_id)),
    }
}

pub(crate) fn default_account(self_id: Principal) -> Account {
    Account {
        owner: self_id,
        subaccount: None,
    }
}

pub(crate) fn compute_burn(previous: u128, relay_minted_cycles: u128, current: u128) -> u128 {
    previous
        .saturating_add(relay_minted_cycles)
        .saturating_sub(current)
}

pub(crate) fn target_topup_cycles(burn_cycles: u128) -> u128 {
    ceil_div(
        burn_cycles.saturating_mul(TOPUP_HEADROOM_NUMERATOR),
        TOPUP_HEADROOM_DENOMINATOR,
    )
}

fn ceil_div(numerator: u128, denominator: u128) -> u128 {
    if numerator == 0 {
        0
    } else {
        numerator.saturating_add(denominator - 1) / denominator
    }
}

pub(crate) fn topup_gross_e8s(
    target_cycles: u128,
    estimate: &ConversionEstimate,
    fee_e8s: u64,
) -> u64 {
    if target_cycles == 0 || estimate.cycles_per_e8 == 0 {
        return 0;
    }
    let amount = ceil_div(target_cycles, estimate.cycles_per_e8).min(u64::MAX as u128) as u64;
    amount.saturating_add(fee_e8s)
}

pub(crate) fn conversion_estimate_is_usable(estimate: &ConversionEstimate, now_nanos: u64) -> bool {
    estimate.cycles_per_e8 > 0
        && now_nanos.saturating_sub(estimate.timestamp_nanos) <= CONVERSION_ESTIMATE_MAX_AGE_NANOS
}

pub(crate) fn effective_managed_canisters(
    configured: &[Principal],
    self_id: Principal,
) -> Vec<Principal> {
    let mut set = BTreeSet::new();
    set.extend(configured.iter().copied());
    set.insert(self_id);
    set.into_iter().collect()
}

pub(crate) fn validate_config(cfg: &Config, self_id: Principal) -> Result<(), String> {
    validate_canister_principal("ledger_canister_id", cfg.ledger_canister_id)?;
    validate_canister_principal("cmc_canister_id", cfg.cmc_canister_id)?;
    validate_canister_principal("governance_canister_id", cfg.governance_canister_id)?;
    validate_canister_principal("blackhole_canister_id", cfg.blackhole_canister_id)?;

    if cfg.max_transfers_per_tick == Some(0) {
        return Err("max_transfers_per_tick must be greater than zero when set".to_string());
    }

    let mut seen = BTreeSet::new();
    for canister_id in &cfg.managed_canisters {
        validate_canister_principal("managed_canisters", *canister_id)?;
        if !seen.insert(*canister_id) {
            return Err(format!(
                "duplicate managed canister: {}",
                canister_id.to_text()
            ));
        }
    }

    let mut targets = BTreeSet::new();
    for recipient in &cfg.surplus_recipients {
        match recipient.target {
            SurplusTarget::Canister(canister_id) => {
                validate_canister_principal("surplus_recipients.target.Canister", canister_id)?;
            }
            SurplusTarget::Neuron(_) => {}
        }
        if !targets.insert(recipient.target.clone()) {
            return Err("duplicate surplus recipient target".to_string());
        }
    }

    if cfg.main_interval_seconds < 60 {
        return Err("main_interval_seconds must be at least 60 after clamping".to_string());
    }

    let _ = effective_managed_canisters(&cfg.managed_canisters, self_id);
    Ok(())
}

fn validate_canister_principal(name: &str, principal: Principal) -> Result<(), String> {
    if principal == Principal::anonymous() {
        return Err(format!("{name} must not be anonymous"));
    }
    if principal == Principal::management_canister() {
        return Err(format!("{name} must not be the management canister"));
    }
    Ok(())
}

pub(crate) fn build_allocation_plan(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    relay_minted_since_previous: &BTreeMap<Principal, u128>,
    available_balance_e8s: u64,
    fee_e8s: u64,
    conversion_estimate: Option<&ConversionEstimate>,
    now_nanos: u64,
) -> AllocationPlan {
    let mut topups = current
        .iter()
        .map(|(canister_id, current_snapshot)| {
            let previous_cycles = previous.get(canister_id).map(|sample| sample.cycles);
            let relay_minted_cycles = relay_minted_since_previous
                .get(canister_id)
                .copied()
                .unwrap_or(0);
            let burn_cycles = previous_cycles
                .map(|prev| compute_burn(prev, relay_minted_cycles, current_snapshot.cycles))
                .unwrap_or(0);
            let target_topup_cycles = target_topup_cycles(burn_cycles);
            BurnPlan {
                canister_id: *canister_id,
                previous_cycles,
                current_cycles: current_snapshot.cycles,
                relay_minted_cycles,
                burn_cycles,
                target_topup_cycles,
                gross_share_e8s: 0,
                amount_e8s: 0,
                actual_minted_cycles: 0,
                skipped_reason: None,
            }
        })
        .collect::<Vec<_>>();

    let usable_estimate =
        conversion_estimate.filter(|e| conversion_estimate_is_usable(e, now_nanos));
    let fallback_estimate;
    let estimate = if let Some(estimate) = usable_estimate {
        estimate
    } else {
        fallback_estimate = ConversionEstimate {
            cycles_per_e8: BOOTSTRAP_CYCLES_PER_E8,
            timestamp_nanos: now_nanos,
        };
        &fallback_estimate
    };

    let desired_gross = topups
        .iter_mut()
        .map(|plan| {
            let gross = topup_gross_e8s(plan.target_topup_cycles, estimate, fee_e8s);
            if gross == 0 {
                if plan.target_topup_cycles == 0 {
                    plan.skipped_reason = Some("zero_burn".to_string());
                }
                return 0;
            }
            if gross <= fee_e8s {
                plan.skipped_reason = Some("gross_share_does_not_exceed_fee".to_string());
                return 0;
            }
            gross
        })
        .collect::<Vec<_>>();
    let total_desired_gross = desired_gross
        .iter()
        .fold(0_u64, |acc, gross| acc.saturating_add(*gross));
    let topup_phase_fully_funded = total_desired_gross <= available_balance_e8s;

    if topup_phase_fully_funded {
        for (plan, gross) in topups.iter_mut().zip(desired_gross.iter().copied()) {
            if gross > 0 {
                plan.gross_share_e8s = gross;
                plan.amount_e8s = gross - fee_e8s;
            }
        }
    } else if total_desired_gross > 0 {
        for (plan, gross) in topups.iter_mut().zip(proportional_topup_gross_allocations(
            &desired_gross,
            total_desired_gross,
            available_balance_e8s,
        )) {
            if gross > fee_e8s {
                plan.gross_share_e8s = gross;
                plan.amount_e8s = gross - fee_e8s;
            } else if plan.skipped_reason.is_none() {
                plan.skipped_reason = Some("gross_share_does_not_exceed_fee".to_string());
            }
        }
    }

    if !topup_phase_fully_funded {
        for (plan, desired) in topups.iter_mut().zip(desired_gross) {
            if desired > 0 && plan.amount_e8s == 0 && plan.skipped_reason.is_none() {
                plan.skipped_reason = Some("insufficient_balance_for_topups".to_string());
            }
        }
    }

    AllocationPlan {
        topups,
        topup_phase_fully_funded,
        skipped_surplus_reason: if topup_phase_fully_funded {
            None
        } else {
            Some("insufficient_balance_for_topups".to_string())
        },
    }
}

fn proportional_topup_gross_allocations(
    desired_gross: &[u64],
    total_desired_gross: u64,
    available_balance_e8s: u64,
) -> Vec<u64> {
    let total = u128::from(total_desired_gross);
    let available = u128::from(available_balance_e8s);
    let mut allocations = desired_gross
        .iter()
        .map(|gross| {
            if *gross == 0 {
                return (0_u64, 0_u128);
            }
            let product = available.saturating_mul(u128::from(*gross));
            let share = (product / total).min(u128::from(*gross)) as u64;
            (share, product % total)
        })
        .collect::<Vec<_>>();
    let mut used = allocations
        .iter()
        .fold(0_u64, |acc, (share, _)| acc.saturating_add(*share));
    let mut order = allocations
        .iter()
        .enumerate()
        .map(|(index, (_, remainder))| (index, *remainder))
        .collect::<Vec<_>>();
    order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    while used < available_balance_e8s {
        let mut advanced = false;
        for (index, _) in &order {
            if used >= available_balance_e8s {
                break;
            }
            if allocations[*index].0 < desired_gross[*index] {
                allocations[*index].0 = allocations[*index].0.saturating_add(1);
                used = used.saturating_add(1);
                advanced = true;
            }
        }
        if !advanced {
            break;
        }
    }

    allocations.into_iter().map(|(share, _)| share).collect()
}

pub(crate) fn build_surplus_plan(
    recipients: &[ResolvedSurplusRecipient],
    available_balance_e8s: u64,
    fee_e8s: u64,
    conversion_estimate: Option<&ConversionEstimate>,
    now_nanos: u64,
) -> (Vec<SurplusTransferSample>, u64, Option<String>) {
    if conversion_estimate
        .filter(|estimate| conversion_estimate_is_usable(estimate, now_nanos))
        .is_none()
    {
        return (
            Vec::new(),
            0,
            Some("missing_conversion_estimate".to_string()),
        );
    }

    let surplus = allocate_equal_surplus_shares(recipients, available_balance_e8s, fee_e8s);
    let skipped_surplus_reason = if recipients.is_empty() {
        Some("no_surplus_recipients".to_string())
    } else if available_balance_e8s == 0 {
        Some("no_surplus".to_string())
    } else if surplus.iter().all(|plan| plan.amount_e8s == 0) {
        Some("gross_share_does_not_exceed_fee".to_string())
    } else {
        None
    };

    (surplus, available_balance_e8s, skipped_surplus_reason)
}

pub(crate) fn allocate_equal_surplus_shares(
    recipients: &[ResolvedSurplusRecipient],
    available_balance_e8s: u64,
    fee_e8s: u64,
) -> Vec<SurplusTransferSample> {
    if recipients.is_empty() {
        return Vec::new();
    }
    let gross = available_balance_e8s / recipients.len() as u64;
    recipients
        .iter()
        .map(|recipient| SurplusTransferSample {
            target: recipient.target.clone(),
            account: recipient.account,
            gross_share_e8s: gross,
            amount_e8s: if gross > fee_e8s { gross - fee_e8s } else { 0 },
            memo_len: recipient.memo.as_ref().map(|memo| memo.len() as u32),
            skipped_reason: if gross > fee_e8s {
                None
            } else {
                Some("gross_share_does_not_exceed_fee".to_string())
            },
        })
        .collect()
}

pub(crate) fn resolve_canister_surplus_recipient(
    recipient: &SurplusRecipient,
) -> Option<ResolvedSurplusRecipient> {
    match recipient.target {
        SurplusTarget::Canister(canister_id) => Some(ResolvedSurplusRecipient {
            target: recipient.target.clone(),
            account: Account {
                owner: canister_id,
                subaccount: None,
            },
            memo: recipient.memo.clone(),
        }),
        SurplusTarget::Neuron(_) => None,
    }
}

pub(crate) fn reject_duplicate_resolved_destinations(
    resolved: &[ResolvedSurplusRecipient],
) -> Result<(), String> {
    let mut accounts = BTreeSet::new();
    for recipient in resolved {
        if !accounts.insert(recipient.account) {
            return Err(format!(
                "duplicate resolved surplus destination: owner={} subaccount={:?}",
                recipient.account.owner.to_text(),
                recipient.account.subaccount
            ));
        }
    }
    Ok(())
}

pub(crate) fn sample_source_for(canister_id: Principal, self_id: Principal) -> CyclesSampleSource {
    if canister_id == self_id {
        CyclesSampleSource::SelfCanister
    } else {
        CyclesSampleSource::BlackholeStatus
    }
}

impl From<&BurnPlan> for CanisterBurnSample {
    fn from(value: &BurnPlan) -> Self {
        Self {
            canister_id: value.canister_id,
            previous_cycles: value.previous_cycles,
            current_cycles: value.current_cycles,
            relay_minted_cycles: value.relay_minted_cycles,
            burn_cycles: value.burn_cycles,
            target_topup_cycles: value.target_topup_cycles,
            gross_share_e8s: value.gross_share_e8s,
            amount_e8s: value.amount_e8s,
            actual_minted_cycles: value.actual_minted_cycles,
            skipped_reason: value.skipped_reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn canister_a() -> Principal {
        principal("22255-zqaaa-aaaas-qf6uq-cai")
    }

    fn canister_b() -> Principal {
        principal("qaa6y-5yaaa-aaaaa-aaafa-cai")
    }

    fn snapshot(cycles: u128) -> CyclesSnapshot {
        CyclesSnapshot {
            cycles,
            timestamp_nanos: 1,
            source: CyclesSampleSource::BlackholeStatus,
        }
    }

    fn config() -> Config {
        Config {
            managed_canisters: vec![canister_a()],
            ledger_canister_id: canister_b(),
            cmc_canister_id: canister_b(),
            governance_canister_id: canister_b(),
            blackhole_canister_id: canister_b(),
            main_interval_seconds: 60,
            max_transfers_per_tick: None,
            surplus_recipients: Vec::new(),
        }
    }

    #[test]
    fn principal_to_subaccount_encodes_length_and_bytes() {
        let p = canister_a();
        let sub = principal_to_subaccount(p);
        assert_eq!(sub[0] as usize, p.as_slice().len());
        assert_eq!(&sub[1..1 + p.as_slice().len()], p.as_slice());
    }

    #[test]
    fn config_validation_accepts_typed_surplus_targets_and_rejects_bad_values() {
        let self_id = canister_b();
        let mut cfg = config();
        cfg.surplus_recipients = vec![
            SurplusRecipient {
                target: SurplusTarget::Neuron(6_345_890_886_899_317_159),
                memo: None,
            },
            SurplusRecipient {
                target: SurplusTarget::Neuron(11_614_578_985_374_291_210),
                memo: Some(b"6345890886899317159".to_vec()),
            },
        ];
        assert!(validate_config(&cfg, self_id).is_ok());

        cfg.surplus_recipients.push(SurplusRecipient {
            target: SurplusTarget::Neuron(6_345_890_886_899_317_159),
            memo: None,
        });
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("duplicate surplus recipient target"));

        cfg.surplus_recipients = vec![SurplusRecipient {
            target: SurplusTarget::Canister(Principal::anonymous()),
            memo: None,
        }];
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("anonymous"));
    }

    #[test]
    fn burn_accounts_for_relay_minted_cycles() {
        assert_eq!(compute_burn(1_000, 105, 1_005), 100);
        assert_eq!(compute_burn(1_000, 105, 1_080), 25);
        assert_eq!(compute_burn(1_000, 105, 1_200), 0);
    }

    #[test]
    fn topup_headroom_uses_ceiling_arithmetic() {
        assert_eq!(target_topup_cycles(100_000_000_000), 101_000_000_000);
        assert_eq!(target_topup_cycles(1), 2);
        assert_eq!(target_topup_cycles(0), 0);
    }

    #[test]
    fn allocation_plans_topups_without_preplanning_surplus() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        let estimate = ConversionEstimate {
            cycles_per_e8: 10,
            timestamp_nanos: 1,
        };
        let plan = build_allocation_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            1_000,
            10,
            Some(&estimate),
            1,
        );

        assert_eq!(plan.topups[0].target_topup_cycles, 101);
        assert_eq!(plan.topups[0].gross_share_e8s, 21);
        assert_eq!(plan.topups[0].amount_e8s, 11);
        assert!(plan.topup_phase_fully_funded);
        assert!(plan.skipped_surplus_reason.is_none());
    }

    #[test]
    fn insufficient_topup_balance_disables_surplus() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        let estimate = ConversionEstimate {
            cycles_per_e8: 1,
            timestamp_nanos: 1,
        };

        let plan = build_allocation_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            50,
            10,
            Some(&estimate),
            1,
        );

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some("insufficient_balance_for_topups")
        );
        assert!(!plan.topup_phase_fully_funded);
    }

    #[test]
    fn missing_conversion_still_allows_bootstrap_topup_plan() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));

        let plan = build_allocation_plan(&current, &previous, &BTreeMap::new(), 1_000, 10, None, 1);

        assert!(plan.topup_phase_fully_funded);
        assert!(plan.topups[0].amount_e8s > 0);
        assert!(plan.skipped_surplus_reason.is_none());
    }

    #[test]
    fn surplus_plan_requires_usable_conversion() {
        let (surplus, e8s, reason) = build_surplus_plan(&[], 1_000, 10, None, 1);

        assert!(surplus.is_empty());
        assert_eq!(e8s, 0);
        assert_eq!(reason.as_deref(), Some("missing_conversion_estimate"));
    }

    #[test]
    fn surplus_plan_splits_equally_after_per_transfer_fees_and_preserves_memo_len() {
        let io_memo = b"6345890886899317159".to_vec();
        let recipients = vec![
            ResolvedSurplusRecipient {
                target: SurplusTarget::Neuron(6_345_890_886_899_317_159),
                account: Account {
                    owner: canister_a(),
                    subaccount: Some([1; 32]),
                },
                memo: None,
            },
            ResolvedSurplusRecipient {
                target: SurplusTarget::Neuron(11_614_578_985_374_291_210),
                account: Account {
                    owner: canister_a(),
                    subaccount: Some([2; 32]),
                },
                memo: Some(io_memo.clone()),
            },
        ];
        let estimate = ConversionEstimate {
            cycles_per_e8: 10,
            timestamp_nanos: 1,
        };

        let (surplus, e8s, reason) =
            build_surplus_plan(&recipients, 100_001, 10, Some(&estimate), 1);

        assert_eq!(e8s, 100_001);
        assert!(reason.is_none());
        assert_eq!(surplus.len(), 2);
        assert_eq!(surplus[0].gross_share_e8s, 50_000);
        assert_eq!(surplus[1].gross_share_e8s, 50_000);
        assert_eq!(surplus[0].amount_e8s, 49_990);
        assert_eq!(surplus[1].amount_e8s, 49_990);
        assert_eq!(surplus[0].memo_len, None);
        assert_eq!(surplus[1].memo_len, Some(io_memo.len() as u32));
    }

    #[test]
    fn underfunded_topups_are_allocated_proportionally() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(800));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));
        let estimate = ConversionEstimate {
            cycles_per_e8: 1,
            timestamp_nanos: 1,
        };

        let plan = build_allocation_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            180,
            10,
            Some(&estimate),
            1,
        );

        let a = plan
            .topups
            .iter()
            .find(|sample| sample.canister_id == canister_a())
            .unwrap();
        let b = plan
            .topups
            .iter()
            .find(|sample| sample.canister_id == canister_b())
            .unwrap();
        assert!(!plan.topup_phase_fully_funded);
        assert!(a.amount_e8s > 0);
        assert!(b.amount_e8s > 0);
        assert!(b.gross_share_e8s > a.gross_share_e8s);
    }
}
