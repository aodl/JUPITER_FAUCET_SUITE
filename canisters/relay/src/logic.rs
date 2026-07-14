use std::collections::{BTreeMap, BTreeSet};

use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::account::principal_to_subaccount;

use crate::state::{
    CanisterBurnSample, Config, ConversionEstimate, CyclesSnapshot, SurplusRecipient,
    SurplusTarget, SurplusTransferSample, TargetProbeClassification,
};

pub(crate) const MEMO_TOP_UP_CANISTER_U64: u64 = 1_347_768_404;
pub(crate) const TOPUP_HEADROOM_NUMERATOR: u128 = 101;
pub(crate) const TOPUP_HEADROOM_DENOMINATOR: u128 = 100;
pub(crate) const CONVERSION_ESTIMATE_MAX_AGE_NANOS: u64 = 14 * 24 * 60 * 60 * 1_000_000_000;
pub(crate) const MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S: u64 = 100_000_000;
pub(crate) const JUPITER_FAUCET_NEURON_ID: u64 = 11_614_578_985_374_291_210;
pub(crate) const SKIP_REASON_ZERO_BURN: &str = "zero_burn";
pub(crate) const SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE: &str =
    "gross_share_does_not_exceed_fee";
pub(crate) const SKIP_REASON_INSUFFICIENT_BALANCE_FOR_TOPUPS: &str =
    "insufficient_balance_for_topups";
pub(crate) const SKIP_REASON_NO_POSITIVE_BURN: &str = "no_positive_burn";
pub(crate) const SKIP_REASON_NO_RAW_ICP_RECIPIENTS: &str = "no_raw_icp_recipients";
pub(crate) const SKIP_REASON_MISSING_CONVERSION_ESTIMATE: &str = "missing_conversion_estimate";
pub(crate) const SKIP_REASON_NO_SURPLUS_RECIPIENTS: &str = "no_surplus_recipients";
pub(crate) const SKIP_REASON_NO_SURPLUS: &str = "no_surplus";
pub(crate) const SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP: &str = "raw_icp_share_below_1_icp";
pub(crate) const SKIP_REASON_UNRECOVERED_CYCLE_DEFICIT: &str = "unrecovered_cycle_deficit";
pub(crate) const SKIP_REASON_TRANSIENT_PROBE_FAILURE: &str = "transient_probe_failure";
pub(crate) const SKIP_REASON_TARGET_UNAVAILABLE_AFTER_CONSECUTIVE_PROBE_FAILURES: &str =
    "target_unavailable_after_consecutive_probe_failures";
pub(crate) const TARGET_UNAVAILABLE_FAILURE_THRESHOLD: u32 = 3;
pub(crate) const ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD: &str =
    "all_cycles_batch_below_fee_efficient_threshold";
const BOOTSTRAP_CYCLES_PER_E8: u128 = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BurnPlan {
    pub canister_id: Principal,
    pub previous_cycles: Option<u128>,
    pub current_cycles: u128,
    pub relay_minted_cycles: u128,
    pub burn_cycles: u128,
    pub carried_deficit_cycles: u128,
    pub target_topup_cycles: u128,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub actual_minted_cycles: u128,
    pub remaining_deficit_cycles: u128,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FaucetCommitmentPlan {
    pub source_account: Account,
    pub destination_account: Account,
    pub balance_start_e8s: u64,
    pub fee_e8s: u64,
    pub amount_e8s: u64,
    pub memo: Vec<u8>,
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

pub(crate) fn relay_subaccount_one() -> [u8; 32] {
    let mut out = [0u8; 32];
    out[31] = 1;
    out
}

pub(crate) fn relay_subaccount_one_account(self_id: Principal) -> Account {
    Account {
        owner: self_id,
        subaccount: Some(relay_subaccount_one()),
    }
}

pub(crate) fn relay_faucet_commitment_memo(self_id: Principal) -> Result<Vec<u8>, String> {
    let memo = format!("{}.Relay", self_id.to_text().replace('-', ""));
    if !memo.is_ascii() {
        return Err("relay faucet commitment memo must be ASCII".to_string());
    }
    if memo.len() > 32 {
        return Err(format!(
            "relay faucet commitment memo is {} bytes; maximum is 32",
            memo.len()
        ));
    }
    Ok(memo.into_bytes())
}

pub(crate) fn build_faucet_commitment_plan(
    relay_id: Principal,
    governance_id: Principal,
    staking_subaccount: [u8; 32],
    balance_start_e8s: u64,
    fee_e8s: u64,
) -> Result<FaucetCommitmentPlan, &'static str> {
    if balance_start_e8s == 0 {
        return Err("subaccount_1_no_funds");
    }
    if balance_start_e8s <= fee_e8s {
        return Err("subaccount_1_below_1_icp_net");
    }
    let amount_e8s = balance_start_e8s - fee_e8s;
    if amount_e8s < MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S {
        return Err("subaccount_1_below_1_icp_net");
    }
    Ok(FaucetCommitmentPlan {
        source_account: relay_subaccount_one_account(relay_id),
        destination_account: Account {
            owner: governance_id,
            subaccount: Some(staking_subaccount),
        },
        balance_start_e8s,
        fee_e8s,
        amount_e8s,
        memo: relay_faucet_commitment_memo(relay_id).map_err(|_| "subaccount_1_memo_invalid")?,
    })
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

pub(crate) fn topup_net_e8s(target_cycles: u128, estimate: &ConversionEstimate) -> u64 {
    if target_cycles == 0 || estimate.cycles_per_e8 == 0 {
        return 0;
    }
    ceil_div(target_cycles, estimate.cycles_per_e8).min(u64::MAX as u128) as u64
}

pub(crate) fn conversion_estimate_is_usable(estimate: &ConversionEstimate, now_nanos: u64) -> bool {
    estimate.cycles_per_e8 > 0
        && now_nanos.saturating_sub(estimate.timestamp_nanos) <= CONVERSION_ESTIMATE_MAX_AGE_NANOS
}

#[cfg(test)]
pub(crate) fn conversion_estimate_from_icp_xdr_rate(
    rate: u64,
    decimals: u32,
    timestamp_seconds: u64,
) -> Result<ConversionEstimate, String> {
    let scale = 10_u128
        .checked_pow(decimals)
        .ok_or_else(|| format!("10^{decimals} overflows u128"))?;
    let numerator = u128::from(rate)
        .checked_mul(10_000)
        .ok_or_else(|| "rate * 10000 overflows u128".to_string())?;
    let cycles_per_e8 = numerator / scale;
    if cycles_per_e8 == 0 {
        return Err("ICP/XDR rate produced zero cycles per e8".to_string());
    }
    let timestamp_nanos = timestamp_seconds
        .checked_mul(1_000_000_000)
        .ok_or_else(|| "timestamp seconds to nanoseconds overflows u64".to_string())?;

    Ok(ConversionEstimate {
        cycles_per_e8,
        timestamp_nanos,
    })
}

pub(crate) fn conversion_estimate_from_cmc_rate(
    xdr_permyriad_per_icp: u64,
    timestamp_seconds: u64,
) -> Result<ConversionEstimate, String> {
    if xdr_permyriad_per_icp == 0 {
        return Err("CMC ICP/XDR conversion rate produced zero cycles per e8".to_string());
    }
    let timestamp_nanos = timestamp_seconds
        .checked_mul(1_000_000_000)
        .ok_or_else(|| "timestamp seconds to nanoseconds overflows u64".to_string())?;
    Ok(ConversionEstimate {
        cycles_per_e8: u128::from(xdr_permyriad_per_icp),
        timestamp_nanos,
    })
}

pub(crate) fn classify_target_probe(
    observed: bool,
    consecutive_failures: u32,
) -> TargetProbeClassification {
    if observed {
        TargetProbeClassification::Observable
    } else if consecutive_failures >= TARGET_UNAVAILABLE_FAILURE_THRESHOLD {
        TargetProbeClassification::UnavailableAfterConsecutiveFailures {
            consecutive_failures,
        }
    } else {
        TargetProbeClassification::TransientProbeFailure {
            consecutive_failures,
        }
    }
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_allocation_plan(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    relay_minted_since_previous: &BTreeMap<Principal, u128>,
    recovery_deficits: &BTreeMap<Principal, u128>,
    available_balance_e8s: u64,
    fee_e8s: u64,
    conversion_estimate: Option<&ConversionEstimate>,
    now_nanos: u64,
) -> AllocationPlan {
    let mut topups = burn_plans(
        current,
        previous,
        relay_minted_since_previous,
        recovery_deficits,
        true,
    );

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
            let net = topup_net_e8s(plan.target_topup_cycles, estimate);
            if net == 0 {
                if plan.target_topup_cycles == 0 {
                    plan.skipped_reason = Some(SKIP_REASON_ZERO_BURN.to_string());
                }
                return 0;
            }
            let gross = net.saturating_add(fee_e8s);
            if gross <= fee_e8s {
                plan.skipped_reason = Some(SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE.to_string());
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
                plan.skipped_reason = Some(SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE.to_string());
            }
        }
    }

    if !topup_phase_fully_funded {
        for (plan, desired) in topups.iter_mut().zip(desired_gross) {
            if desired > 0 && plan.amount_e8s == 0 && plan.skipped_reason.is_none() {
                plan.skipped_reason = Some(SKIP_REASON_INSUFFICIENT_BALANCE_FOR_TOPUPS.to_string());
            }
        }
    }

    AllocationPlan {
        topups,
        topup_phase_fully_funded,
        skipped_surplus_reason: if topup_phase_fully_funded {
            None
        } else {
            Some(SKIP_REASON_INSUFFICIENT_BALANCE_FOR_TOPUPS.to_string())
        },
    }
}

pub(crate) fn build_spend_all_cycles_plan(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    relay_minted_since_previous: &BTreeMap<Principal, u128>,
    recovery_deficits: &BTreeMap<Principal, u128>,
    available_balance_e8s: u64,
    fee_e8s: u64,
) -> AllocationPlan {
    let mut topups = burn_plans(
        current,
        previous,
        relay_minted_since_previous,
        recovery_deficits,
        false,
    );
    let positive_need_count = topups
        .iter()
        .filter(|plan| plan.target_topup_cycles > 0)
        .count();
    let total_positive_need = topups
        .iter()
        .filter(|plan| plan.target_topup_cycles > 0)
        .map(|plan| plan.target_topup_cycles)
        .sum::<u128>();

    if total_positive_need == 0 {
        for plan in &mut topups {
            plan.skipped_reason = Some(SKIP_REASON_NO_POSITIVE_BURN.to_string());
        }
        return AllocationPlan {
            topups,
            topup_phase_fully_funded: true,
            skipped_surplus_reason: Some(SKIP_REASON_NO_POSITIVE_BURN.to_string()),
        };
    }

    let gross_shares = topups
        .iter()
        .map(|plan| {
            if plan.target_topup_cycles == 0 {
                0
            } else {
                (u128::from(available_balance_e8s).saturating_mul(plan.target_topup_cycles)
                    / total_positive_need)
                    .min(u128::from(u64::MAX)) as u64
            }
        })
        .collect::<Vec<_>>();
    let fee_efficient_threshold = u128::from(fee_e8s).saturating_mul(2);
    let batch_is_fee_efficient = u128::from(available_balance_e8s)
        >= u128::from(fee_e8s).saturating_mul(positive_need_count as u128)
        && topups
            .iter()
            .zip(gross_shares.iter())
            .filter(|(plan, _)| plan.target_topup_cycles > 0)
            .all(|(_, gross)| u128::from(*gross) >= fee_efficient_threshold);

    if !batch_is_fee_efficient {
        for (plan, gross) in topups.iter_mut().zip(gross_shares) {
            plan.gross_share_e8s = gross;
            plan.skipped_reason = Some(if plan.target_topup_cycles == 0 {
                SKIP_REASON_ZERO_BURN.to_string()
            } else {
                ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD.to_string()
            });
        }
        return AllocationPlan {
            topups,
            topup_phase_fully_funded: true,
            skipped_surplus_reason: Some(
                ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD.to_string(),
            ),
        };
    }

    for (plan, gross) in topups.iter_mut().zip(gross_shares) {
        if plan.target_topup_cycles == 0 {
            plan.skipped_reason = Some(SKIP_REASON_ZERO_BURN.to_string());
            continue;
        }
        plan.gross_share_e8s = gross;
        plan.amount_e8s = gross - fee_e8s;
    }

    AllocationPlan {
        topups,
        topup_phase_fully_funded: true,
        skipped_surplus_reason: Some(SKIP_REASON_NO_RAW_ICP_RECIPIENTS.to_string()),
    }
}

fn burn_plans(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    relay_minted_since_previous: &BTreeMap<Principal, u128>,
    recovery_deficits: &BTreeMap<Principal, u128>,
    use_headroom_target: bool,
) -> Vec<BurnPlan> {
    current
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
            let carried_deficit_cycles = recovery_deficits.get(canister_id).copied().unwrap_or(0);
            let new_burn_target_cycles = if use_headroom_target {
                target_topup_cycles(burn_cycles)
            } else {
                burn_cycles
            };
            BurnPlan {
                canister_id: *canister_id,
                previous_cycles,
                current_cycles: current_snapshot.cycles,
                relay_minted_cycles,
                burn_cycles,
                carried_deficit_cycles,
                target_topup_cycles: carried_deficit_cycles.saturating_add(new_burn_target_cycles),
                gross_share_e8s: 0,
                amount_e8s: 0,
                actual_minted_cycles: 0,
                remaining_deficit_cycles: carried_deficit_cycles
                    .saturating_add(new_burn_target_cycles),
                skipped_reason: None,
            }
        })
        .collect()
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
            Some(SKIP_REASON_MISSING_CONVERSION_ESTIMATE.to_string()),
        );
    }

    let surplus = allocate_equal_surplus_shares(recipients, available_balance_e8s, fee_e8s);
    let skipped_surplus_reason = if recipients.is_empty() {
        Some(SKIP_REASON_NO_SURPLUS_RECIPIENTS.to_string())
    } else if available_balance_e8s == 0 {
        Some(SKIP_REASON_NO_SURPLUS.to_string())
    } else if surplus.iter().all(|plan| plan.amount_e8s == 0) {
        surplus
            .iter()
            .find_map(|plan| plan.skipped_reason.clone())
            .or_else(|| Some(SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE.to_string()))
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
    let (amount_e8s, skipped_reason) = if gross <= fee_e8s {
        (
            0,
            Some(SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE.to_string()),
        )
    } else {
        let candidate_amount = gross - fee_e8s;
        if candidate_amount < MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S {
            (0, Some(SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP.to_string()))
        } else {
            (candidate_amount, None)
        }
    };
    recipients
        .iter()
        .map(|recipient| SurplusTransferSample {
            target: recipient.target.clone(),
            account: recipient.account,
            gross_share_e8s: gross,
            amount_e8s,
            memo_len: recipient.memo.as_ref().map(|memo| memo.len() as u32),
            skipped_reason: skipped_reason.clone(),
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

impl From<&BurnPlan> for CanisterBurnSample {
    fn from(value: &BurnPlan) -> Self {
        Self {
            canister_id: value.canister_id,
            previous_cycles: value.previous_cycles,
            current_cycles: value.current_cycles,
            relay_minted_cycles: value.relay_minted_cycles,
            burn_cycles: value.burn_cycles,
            carried_deficit_cycles: value.carried_deficit_cycles,
            target_topup_cycles: value.target_topup_cycles,
            gross_share_e8s: value.gross_share_e8s,
            amount_e8s: value.amount_e8s,
            sent_topup_e8s: 0,
            actual_minted_cycles: value.actual_minted_cycles,
            remaining_deficit_cycles: value.remaining_deficit_cycles,
            skipped_reason: value.skipped_reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CyclesSampleSource;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn canister_a() -> Principal {
        principal("22255-zqaaa-aaaas-qf6uq-cai")
    }

    fn canister_b() -> Principal {
        principal("qaa6y-5yaaa-aaaaa-aaafa-cai")
    }

    fn canister_c() -> Principal {
        principal("rkp4c-7iaaa-aaaaa-aaaca-cai")
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
            cycles_probe_policy:
                jupiter_ic_clients::cycles_probe::CyclesProbePolicy::FixedBlackhole {
                    canister_id: canister_b(),
                },
            main_interval_seconds: 60,
            max_transfers_per_tick: None,
            surplus_recipients: Vec::new(),
        }
    }

    #[test]
    fn skip_reason_constants_preserve_external_strings() {
        assert_eq!(SKIP_REASON_ZERO_BURN, "zero_burn");
        assert_eq!(
            SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE,
            "gross_share_does_not_exceed_fee"
        );
        assert_eq!(
            SKIP_REASON_INSUFFICIENT_BALANCE_FOR_TOPUPS,
            "insufficient_balance_for_topups"
        );
        assert_eq!(SKIP_REASON_NO_POSITIVE_BURN, "no_positive_burn");
        assert_eq!(SKIP_REASON_NO_RAW_ICP_RECIPIENTS, "no_raw_icp_recipients");
        assert_eq!(
            SKIP_REASON_MISSING_CONVERSION_ESTIMATE,
            "missing_conversion_estimate"
        );
        assert_eq!(SKIP_REASON_NO_SURPLUS_RECIPIENTS, "no_surplus_recipients");
        assert_eq!(SKIP_REASON_NO_SURPLUS, "no_surplus");
        assert_eq!(
            SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP,
            "raw_icp_share_below_1_icp"
        );
        assert_eq!(
            SKIP_REASON_UNRECOVERED_CYCLE_DEFICIT,
            "unrecovered_cycle_deficit"
        );
        assert_eq!(
            ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD,
            "all_cycles_batch_below_fee_efficient_threshold"
        );
    }

    #[test]
    fn principal_to_subaccount_encodes_length_and_bytes() {
        let p = canister_a();
        let sub = principal_to_subaccount(p);
        assert_eq!(sub[0] as usize, p.as_slice().len());
        assert_eq!(&sub[1..1 + p.as_slice().len()], p.as_slice());
    }

    #[test]
    fn relay_subaccount_one_is_31_zero_bytes_plus_one() {
        let subaccount = relay_subaccount_one();
        assert_eq!(&subaccount[..31], [0u8; 31].as_slice());
        assert_eq!(subaccount[31], 1);
    }

    #[test]
    fn relay_faucet_commitment_memo_uses_compact_self_principal() {
        let relay = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        let memo = relay_faucet_commitment_memo(relay).unwrap();

        assert_eq!(memo, b"u2qkpaqaaaaaaarqb7eacai.Relay");
        assert!(memo.is_ascii());
        assert_eq!(memo.len(), 29);
        assert!(memo.len() <= 32);
    }

    #[test]
    fn subaccount_1_plan_skips_zero_balance() {
        let err =
            build_faucet_commitment_plan(canister_a(), canister_b(), [7u8; 32], 0, 10).unwrap_err();

        assert_eq!(err, "subaccount_1_no_funds");
    }

    #[test]
    fn subaccount_1_plan_skips_when_net_below_one_icp() {
        let err = build_faucet_commitment_plan(
            canister_a(),
            canister_b(),
            [7u8; 32],
            MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S + 10 - 1,
            10,
        )
        .unwrap_err();

        assert_eq!(err, "subaccount_1_below_1_icp_net");
    }

    #[test]
    fn subaccount_1_plan_allows_exactly_one_icp_net() {
        let plan = build_faucet_commitment_plan(
            canister_a(),
            canister_b(),
            [7u8; 32],
            MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S + 10,
            10,
        )
        .unwrap();

        assert_eq!(plan.amount_e8s, MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S);
        assert_eq!(plan.fee_e8s, 10);
        assert_eq!(
            plan.balance_start_e8s,
            MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S + 10
        );
        assert_eq!(
            plan.source_account,
            Account {
                owner: canister_a(),
                subaccount: Some(relay_subaccount_one()),
            }
        );
        assert_eq!(
            plan.destination_account,
            Account {
                owner: canister_b(),
                subaccount: Some([7u8; 32]),
            }
        );
    }

    #[test]
    fn subaccount_1_plan_sends_balance_minus_fee_when_above_threshold() {
        let plan = build_faucet_commitment_plan(
            canister_a(),
            canister_b(),
            [7u8; 32],
            250_000_000,
            10_000,
        )
        .unwrap();

        assert_eq!(plan.amount_e8s, 249_990_000);
    }

    #[test]
    fn config_validation_accepts_typed_surplus_targets_and_rejects_bad_values() {
        let self_id = canister_b();
        let mut cfg = config();
        cfg.surplus_recipients = vec![
            SurplusRecipient {
                target: SurplusTarget::Neuron(10_292_412_127_977_304_661),
                memo: None,
            },
            SurplusRecipient {
                target: SurplusTarget::Neuron(11_614_578_985_374_291_210),
                memo: Some(b"10292412127977304661".to_vec()),
            },
        ];
        assert!(validate_config(&cfg, self_id).is_ok());

        cfg.surplus_recipients.push(SurplusRecipient {
            target: SurplusTarget::Neuron(10_292_412_127_977_304_661),
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
    fn config_validation_rejects_duplicate_managed_canisters() {
        let self_id = canister_b();
        let duplicate = canister_a();
        let mut cfg = config();
        cfg.managed_canisters = vec![duplicate, duplicate];

        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("duplicate managed canister"));
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
    fn conversion_estimate_from_icp_xdr_rate_converts_to_cycles_per_e8() {
        let estimate =
            conversion_estimate_from_icp_xdr_rate(720_000_000, 8, 1_700_000_000).unwrap();

        assert_eq!(estimate.cycles_per_e8, 72_000);
        assert_eq!(estimate.timestamp_nanos, 1_700_000_000_000_000_000);
    }

    #[test]
    fn conversion_estimate_from_icp_xdr_rate_rejects_zero_cycles_per_e8() {
        let err = conversion_estimate_from_icp_xdr_rate(1, 8, 1).unwrap_err();

        assert!(err.contains("zero cycles per e8"));
    }

    #[test]
    fn conversion_estimate_from_icp_xdr_rate_rejects_scale_overflow() {
        let err = conversion_estimate_from_icp_xdr_rate(1, 129, 1).unwrap_err();

        assert!(err.contains("overflows"));
    }

    #[test]
    fn conversion_estimate_from_cmc_rate_treats_xdr_permyriad_as_cycles_per_e8() {
        let estimate = conversion_estimate_from_cmc_rate(72_345, 1_700_000_000).unwrap();

        assert_eq!(estimate.cycles_per_e8, 72_345);
        assert_eq!(estimate.timestamp_nanos, 1_700_000_000_000_000_000);
    }

    #[test]
    fn conversion_estimate_from_cmc_rate_rejects_zero_cycles_per_e8() {
        let err = conversion_estimate_from_cmc_rate(0, 1).unwrap_err();

        assert!(err.contains("zero cycles per e8"));
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
    fn capped_topup_plan_keeps_planned_amount_net_of_ledger_fee() {
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
            &BTreeMap::new(),
            1_000,
            10,
            Some(&estimate),
            1,
        );

        assert_eq!(plan.topups[0].target_topup_cycles, 101);
        assert_eq!(plan.topups[0].amount_e8s, 11);
        assert_eq!(plan.topups[0].gross_share_e8s, 21);
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
            &BTreeMap::new(),
            50,
            10,
            Some(&estimate),
            1,
        );

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(SKIP_REASON_INSUFFICIENT_BALANCE_FOR_TOPUPS)
        );
        assert!(!plan.topup_phase_fully_funded);
    }

    #[test]
    fn missing_conversion_still_allows_bootstrap_topup_plan() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));

        let plan = build_allocation_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            1_000,
            10,
            None,
            1,
        );

        assert!(plan.topup_phase_fully_funded);
        assert!(plan.topups[0].amount_e8s > 0);
        assert!(plan.skipped_surplus_reason.is_none());
    }

    #[test]
    fn raw_surplus_target_adds_carried_deficit_after_new_burn_headroom() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        let mut deficits = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        deficits.insert(canister_a(), 25);
        let estimate = ConversionEstimate {
            cycles_per_e8: 1,
            timestamp_nanos: 1,
        };

        let plan = build_allocation_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &deficits,
            1_000,
            10,
            Some(&estimate),
            1,
        );

        assert_eq!(plan.topups[0].burn_cycles, 100);
        assert_eq!(plan.topups[0].carried_deficit_cycles, 25);
        assert_eq!(plan.topups[0].target_topup_cycles, 126);
        assert_eq!(plan.topups[0].remaining_deficit_cycles, 126);
    }

    #[test]
    fn raw_surplus_target_does_not_apply_headroom_to_carried_deficit() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        let mut deficits = BTreeMap::new();
        current.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_a(), snapshot(1_000));
        deficits.insert(canister_a(), 25);
        let estimate = ConversionEstimate {
            cycles_per_e8: 1,
            timestamp_nanos: 1,
        };

        let plan = build_allocation_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &deficits,
            1_000,
            10,
            Some(&estimate),
            1,
        );

        assert_eq!(plan.topups[0].burn_cycles, 0);
        assert_eq!(plan.topups[0].target_topup_cycles, 25);
    }

    #[test]
    fn allocation_without_raw_recipients_spends_all_available_icp_as_cycles() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            1_000,
            10,
        );

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(SKIP_REASON_NO_RAW_ICP_RECIPIENTS)
        );
        assert!(plan.topup_phase_fully_funded);
        assert!(plan.topups.iter().all(|sample| sample.amount_e8s > 0));
        assert_eq!(
            plan.topups
                .iter()
                .map(|sample| sample.gross_share_e8s)
                .sum::<u64>(),
            1_000
        );
    }

    #[test]
    fn allocation_without_raw_recipients_retains_balance_without_fee_coverage() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            15,
            10,
        );

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
        assert!(plan.topups.iter().all(|sample| sample.amount_e8s == 0));
        assert!(plan
            .topups
            .iter()
            .all(|sample| sample.skipped_reason.as_deref()
                == Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)));
    }

    #[test]
    fn allocation_without_raw_recipients_does_not_require_conversion_estimate() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            10 * BOOTSTRAP_CYCLES_PER_E8 as u64,
            10,
        );

        assert_eq!(
            plan.topups[0].gross_share_e8s,
            10 * BOOTSTRAP_CYCLES_PER_E8 as u64
        );
        assert_eq!(
            plan.topups[0].amount_e8s,
            10 * BOOTSTRAP_CYCLES_PER_E8 as u64 - 10
        );
    }

    #[test]
    fn all_cycles_mode_zero_fresh_burn_with_carried_deficit_participates() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        let mut deficits = BTreeMap::new();
        current.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_a(), snapshot(1_000));
        deficits.insert(canister_a(), 100);

        let plan =
            build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), &deficits, 110, 10);

        assert_eq!(plan.topups[0].burn_cycles, 0);
        assert_eq!(plan.topups[0].carried_deficit_cycles, 100);
        assert_eq!(plan.topups[0].target_topup_cycles, 100);
        assert_eq!(plan.topups[0].gross_share_e8s, 110);
        assert_eq!(plan.topups[0].amount_e8s, 100);
        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(SKIP_REASON_NO_RAW_ICP_RECIPIENTS)
        );
    }

    #[test]
    fn allocation_without_raw_recipients_uses_burn_weighted_shares() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(700));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            1_200,
            10,
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

        assert_eq!(a.gross_share_e8s, 300);
        assert_eq!(b.gross_share_e8s, 900);
        assert_eq!(a.amount_e8s, 290);
        assert_eq!(b.amount_e8s, 890);
    }

    #[test]
    fn all_cycles_mode_allocation_weights_use_burn_plus_carried_deficit() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        let mut deficits = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(700));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));
        deficits.insert(canister_a(), 100);

        let plan =
            build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), &deficits, 500, 10);
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

        assert_eq!(a.target_topup_cycles, 200);
        assert_eq!(b.target_topup_cycles, 300);
        assert_eq!(a.gross_share_e8s, 200);
        assert_eq!(b.gross_share_e8s, 300);
    }

    #[test]
    fn all_cycles_fee_efficiency_gating_considers_carried_deficits() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        let mut deficits = BTreeMap::new();
        current.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_a(), snapshot(1_000));
        deficits.insert(canister_a(), 100);

        let plan =
            build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), &deficits, 15, 10);

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
        assert_eq!(
            plan.topups[0].skipped_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
    }

    #[test]
    fn allocation_without_raw_recipients_blocks_partial_fast_burner_batch() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(100));
        current.insert(canister_b(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            100,
            10,
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

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
        assert_eq!(a.gross_share_e8s, 90);
        assert_eq!(b.gross_share_e8s, 10);
        assert_eq!(a.amount_e8s, 0);
        assert_eq!(b.amount_e8s, 0);
        assert_eq!(
            a.skipped_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
        assert_eq!(
            b.skipped_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
    }

    #[test]
    fn allocation_without_raw_recipients_zero_burn_canisters_do_not_block_batch() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(700));
        current.insert(canister_c(), snapshot(1_000));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));
        previous.insert(canister_c(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            1_200,
            10,
        );
        let c = plan
            .topups
            .iter()
            .find(|sample| sample.canister_id == canister_c())
            .unwrap();

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(SKIP_REASON_NO_RAW_ICP_RECIPIENTS)
        );
        assert_eq!(c.gross_share_e8s, 0);
        assert_eq!(c.amount_e8s, 0);
        assert_eq!(c.skipped_reason.as_deref(), Some(SKIP_REASON_ZERO_BURN));
        assert!(plan
            .topups
            .iter()
            .filter(|sample| sample.burn_cycles > 0)
            .all(|sample| sample.amount_e8s > 0 && sample.skipped_reason.is_none()));
    }

    #[test]
    fn allocation_without_raw_recipients_retains_fee_inefficient_batch() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            10,
            10,
        );

        assert_eq!(plan.topups[0].gross_share_e8s, 10);
        assert_eq!(plan.topups[0].amount_e8s, 0);
        assert_eq!(
            plan.topups[0].skipped_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD)
        );
    }

    #[test]
    fn allocation_without_raw_recipients_and_without_positive_burn_retains_balance() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_a(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(
            &current,
            &previous,
            &BTreeMap::new(),
            &BTreeMap::new(),
            1_000,
            10,
        );

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some(SKIP_REASON_NO_POSITIVE_BURN)
        );
        assert!(plan.topups.iter().all(|sample| sample.amount_e8s == 0));
        assert!(plan.topups.iter().all(|sample| {
            sample.skipped_reason.as_deref() == Some(SKIP_REASON_NO_POSITIVE_BURN)
        }));
    }

    #[test]
    fn surplus_plan_requires_usable_conversion() {
        let (surplus, e8s, reason) = build_surplus_plan(&[], 1_000, 10, None, 1);

        assert!(surplus.is_empty());
        assert_eq!(e8s, 0);
        assert_eq!(
            reason.as_deref(),
            Some(SKIP_REASON_MISSING_CONVERSION_ESTIMATE)
        );
    }

    #[test]
    fn surplus_plan_splits_equally_after_per_transfer_fees_and_preserves_memo_len() {
        let io_memo = b"10292412127977304661".to_vec();
        let recipients = vec![
            ResolvedSurplusRecipient {
                target: SurplusTarget::Neuron(10_292_412_127_977_304_661),
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
            build_surplus_plan(&recipients, 200_000_021, 10, Some(&estimate), 1);

        assert_eq!(e8s, 200_000_021);
        assert!(reason.is_none());
        assert_eq!(surplus.len(), 2);
        assert_eq!(surplus[0].gross_share_e8s, 100_000_010);
        assert_eq!(surplus[1].gross_share_e8s, 100_000_010);
        assert_eq!(surplus[0].amount_e8s, 100_000_000);
        assert_eq!(surplus[1].amount_e8s, 100_000_000);
        assert_eq!(surplus[0].memo_len, None);
        assert_eq!(surplus[1].memo_len, Some(io_memo.len() as u32));
    }

    #[test]
    fn surplus_plan_suppresses_all_raw_icp_shares_below_one_icp_net() {
        let fee = 10;
        let recipients = vec![
            ResolvedSurplusRecipient {
                target: SurplusTarget::Canister(canister_a()),
                account: Account {
                    owner: canister_a(),
                    subaccount: None,
                },
                memo: None,
            },
            ResolvedSurplusRecipient {
                target: SurplusTarget::Neuron(11_614_578_985_374_291_210),
                account: Account {
                    owner: canister_b(),
                    subaccount: Some([2; 32]),
                },
                memo: None,
            },
        ];
        let estimate = ConversionEstimate {
            cycles_per_e8: 10,
            timestamp_nanos: 1,
        };

        let (surplus, _, reason) = build_surplus_plan(
            &recipients,
            2 * (MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S + fee) - 1,
            fee,
            Some(&estimate),
            1,
        );

        assert_eq!(
            reason.as_deref(),
            Some(SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP)
        );
        assert!(surplus.iter().all(|plan| plan.amount_e8s == 0));
        assert!(surplus
            .iter()
            .all(|plan| plan.skipped_reason.as_deref()
                == Some(SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP)));
    }

    #[test]
    fn surplus_plan_allows_exactly_one_icp_net_per_recipient() {
        let fee = 10;
        let recipients = vec![
            ResolvedSurplusRecipient {
                target: SurplusTarget::Canister(canister_a()),
                account: Account {
                    owner: canister_a(),
                    subaccount: None,
                },
                memo: None,
            },
            ResolvedSurplusRecipient {
                target: SurplusTarget::Neuron(11_614_578_985_374_291_210),
                account: Account {
                    owner: canister_b(),
                    subaccount: Some([2; 32]),
                },
                memo: None,
            },
        ];
        let estimate = ConversionEstimate {
            cycles_per_e8: 10,
            timestamp_nanos: 1,
        };

        let (surplus, _, reason) = build_surplus_plan(
            &recipients,
            2 * (MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S + fee),
            fee,
            Some(&estimate),
            1,
        );

        assert!(reason.is_none());
        assert_eq!(surplus.len(), 2);
        assert!(surplus
            .iter()
            .all(|plan| plan.amount_e8s == MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S));
        assert!(surplus.iter().all(|plan| plan.skipped_reason.is_none()));
    }

    #[test]
    fn surplus_plan_suppresses_single_raw_icp_share_below_one_icp_net() {
        let fee = 10;
        let recipients = vec![ResolvedSurplusRecipient {
            target: SurplusTarget::Canister(canister_a()),
            account: Account {
                owner: canister_a(),
                subaccount: None,
            },
            memo: None,
        }];
        let estimate = ConversionEstimate {
            cycles_per_e8: 10,
            timestamp_nanos: 1,
        };

        let (surplus, _, reason) = build_surplus_plan(
            &recipients,
            MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S + fee - 1,
            fee,
            Some(&estimate),
            1,
        );

        assert_eq!(
            reason.as_deref(),
            Some(SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP)
        );
        assert_eq!(surplus[0].amount_e8s, 0);
        assert_eq!(
            surplus[0].skipped_reason.as_deref(),
            Some(SKIP_REASON_RAW_ICP_SHARE_BELOW_1_ICP)
        );
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
