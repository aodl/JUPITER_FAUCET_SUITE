use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

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
pub(crate) const MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S: u64 = 100_000_000;
pub(crate) const JUPITER_FAUCET_NEURON_ID: u64 = 11_614_578_985_374_291_210;
pub(crate) const ALL_CYCLES_BATCH_BELOW_FEE_EFFICIENT_THRESHOLD: &str =
    "all_cycles_batch_below_fee_efficient_threshold";
const BOOTSTRAP_CYCLES_PER_E8: u128 = 100_000;
const KNOWN_BLACKHOLE_CANISTERS: [&str; 2] =
    ["77deu-baaaa-aaaar-qb6za-cai", "e3mmv-5qaaa-aaaah-aadma-cai"];

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FaucetCommitmentPlan {
    pub source_account: Account,
    pub destination_account: Account,
    pub balance_start_e8s: u64,
    pub fee_e8s: u64,
    pub amount_e8s: u64,
    pub memo: Vec<u8>,
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

pub(crate) fn effective_managed_canisters(
    configured: &[Principal],
    self_id: Principal,
) -> Vec<Principal> {
    let mut set = BTreeSet::new();
    set.extend(configured.iter().copied());
    set.insert(self_id);
    set.into_iter().collect()
}

fn known_blackhole_canisters() -> &'static [Principal; 2] {
    static KNOWN_BLACKHOLE_CANISTERS_PARSED: OnceLock<[Principal; 2]> = OnceLock::new();
    KNOWN_BLACKHOLE_CANISTERS_PARSED.get_or_init(|| {
        KNOWN_BLACKHOLE_CANISTERS.map(|id| {
            Principal::from_text(id).expect("invalid hardcoded blackhole canister id")
        })
    })
}

pub(crate) fn is_known_blackhole_canister(canister_id: Principal) -> bool {
    known_blackhole_canisters().contains(&canister_id)
}

pub(crate) fn probe_canister_for(
    managed_canister_id: Principal,
    self_id: Principal,
    configured_blackhole_canister_id: Principal,
) -> Principal {
    if managed_canister_id == self_id {
        self_id
    } else if is_known_blackhole_canister(managed_canister_id) {
        managed_canister_id
    } else {
        configured_blackhole_canister_id
    }
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
    let mut topups = burn_plans(current, previous, relay_minted_since_previous, true);

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

pub(crate) fn build_spend_all_cycles_plan(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    relay_minted_since_previous: &BTreeMap<Principal, u128>,
    available_balance_e8s: u64,
    fee_e8s: u64,
) -> AllocationPlan {
    let mut topups = burn_plans(current, previous, relay_minted_since_previous, false);
    let positive_burn_count = topups.iter().filter(|plan| plan.burn_cycles > 0).count();
    let total_positive_burn = topups
        .iter()
        .filter(|plan| plan.burn_cycles > 0)
        .map(|plan| plan.burn_cycles)
        .sum::<u128>();

    if total_positive_burn == 0 {
        for plan in &mut topups {
            plan.skipped_reason = Some("no_positive_burn".to_string());
        }
        return AllocationPlan {
            topups,
            topup_phase_fully_funded: true,
            skipped_surplus_reason: Some("no_positive_burn".to_string()),
        };
    }

    let gross_shares = topups
        .iter()
        .map(|plan| {
            if plan.burn_cycles == 0 {
                0
            } else {
                (u128::from(available_balance_e8s).saturating_mul(plan.burn_cycles)
                    / total_positive_burn)
                    .min(u128::from(u64::MAX)) as u64
            }
        })
        .collect::<Vec<_>>();
    let fee_efficient_threshold = u128::from(fee_e8s).saturating_mul(2);
    let batch_is_fee_efficient = u128::from(available_balance_e8s)
        >= u128::from(fee_e8s).saturating_mul(positive_burn_count as u128)
        && topups
            .iter()
            .zip(gross_shares.iter())
            .filter(|(plan, _)| plan.burn_cycles > 0)
            .all(|(_, gross)| u128::from(*gross) >= fee_efficient_threshold);

    if !batch_is_fee_efficient {
        for (plan, gross) in topups.iter_mut().zip(gross_shares) {
            plan.gross_share_e8s = gross;
            plan.skipped_reason = Some(if plan.burn_cycles == 0 {
                "zero_burn".to_string()
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
        if plan.burn_cycles == 0 {
            plan.skipped_reason = Some("zero_burn".to_string());
            continue;
        }
        plan.gross_share_e8s = gross;
        plan.amount_e8s = gross - fee_e8s;
    }

    AllocationPlan {
        topups,
        topup_phase_fully_funded: true,
        skipped_surplus_reason: Some("no_raw_icp_recipients".to_string()),
    }
}

fn burn_plans(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    relay_minted_since_previous: &BTreeMap<Principal, u128>,
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
            BurnPlan {
                canister_id: *canister_id,
                previous_cycles,
                current_cycles: current_snapshot.cycles,
                relay_minted_cycles,
                burn_cycles,
                target_topup_cycles: if use_headroom_target {
                    target_topup_cycles(burn_cycles)
                } else {
                    burn_cycles
                },
                gross_share_e8s: 0,
                amount_e8s: 0,
                actual_minted_cycles: 0,
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
            Some("missing_conversion_estimate".to_string()),
        );
    }

    let surplus = allocate_equal_surplus_shares(recipients, available_balance_e8s, fee_e8s);
    let skipped_surplus_reason = if recipients.is_empty() {
        Some("no_surplus_recipients".to_string())
    } else if available_balance_e8s == 0 {
        Some("no_surplus".to_string())
    } else if surplus.iter().all(|plan| plan.amount_e8s == 0) {
        surplus
            .iter()
            .find_map(|plan| plan.skipped_reason.clone())
            .or_else(|| Some("gross_share_does_not_exceed_fee".to_string()))
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
        (0, Some("gross_share_does_not_exceed_fee".to_string()))
    } else {
        let candidate_amount = gross - fee_e8s;
        if candidate_amount < MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S {
            (0, Some("raw_icp_share_below_1_icp".to_string()))
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
    fn probe_canister_for_routes_known_blackholes_through_themselves() {
        let configured_blackhole = canister_b();
        let relay = canister_c();
        let fiduciary_blackhole = principal("77deu-baaaa-aaaar-qb6za-cai");
        let thirteen_node_blackhole = principal("e3mmv-5qaaa-aaaah-aadma-cai");

        assert_eq!(
            probe_canister_for(fiduciary_blackhole, relay, configured_blackhole),
            fiduciary_blackhole
        );
        assert_eq!(
            probe_canister_for(thirteen_node_blackhole, relay, configured_blackhole),
            thirteen_node_blackhole
        );
    }

    #[test]
    fn is_known_blackhole_canister_matches_only_known_blackholes() {
        assert!(is_known_blackhole_canister(principal(
            "77deu-baaaa-aaaar-qb6za-cai"
        )));
        assert!(is_known_blackhole_canister(principal(
            "e3mmv-5qaaa-aaaah-aadma-cai"
        )));
        assert!(!is_known_blackhole_canister(canister_a()));
    }

    #[test]
    fn probe_canister_for_routes_ordinary_managed_canister_through_configured_blackhole() {
        let configured_blackhole = canister_b();
        let relay = canister_c();

        assert_eq!(
            probe_canister_for(canister_a(), relay, configured_blackhole),
            configured_blackhole
        );
    }

    #[test]
    fn probe_canister_for_keeps_relay_self_on_direct_probe_path() {
        let configured_blackhole = canister_b();
        let relay = canister_c();

        assert_eq!(
            probe_canister_for(relay, relay, configured_blackhole),
            relay
        );
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
    fn allocation_without_raw_recipients_spends_all_available_icp_as_cycles() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 1_000, 10);

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some("no_raw_icp_recipients")
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

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 15, 10);

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
    fn allocation_without_raw_recipients_uses_burn_weighted_shares() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(700));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 1_200, 10);
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
    fn allocation_without_raw_recipients_blocks_partial_fast_burner_batch() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(100));
        current.insert(canister_b(), snapshot(900));
        previous.insert(canister_a(), snapshot(1_000));
        previous.insert(canister_b(), snapshot(1_000));

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 100, 10);
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

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 1_200, 10);
        let c = plan
            .topups
            .iter()
            .find(|sample| sample.canister_id == canister_c())
            .unwrap();

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some("no_raw_icp_recipients")
        );
        assert_eq!(c.gross_share_e8s, 0);
        assert_eq!(c.amount_e8s, 0);
        assert_eq!(c.skipped_reason.as_deref(), Some("zero_burn"));
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

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 10, 10);

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

        let plan = build_spend_all_cycles_plan(&current, &previous, &BTreeMap::new(), 1_000, 10);

        assert_eq!(
            plan.skipped_surplus_reason.as_deref(),
            Some("no_positive_burn")
        );
        assert!(plan.topups.iter().all(|sample| sample.amount_e8s == 0));
        assert!(plan
            .topups
            .iter()
            .all(|sample| { sample.skipped_reason.as_deref() == Some("no_positive_burn") }));
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

        assert_eq!(reason.as_deref(), Some("raw_icp_share_below_1_icp"));
        assert!(surplus.iter().all(|plan| plan.amount_e8s == 0));
        assert!(surplus
            .iter()
            .all(|plan| plan.skipped_reason.as_deref() == Some("raw_icp_share_below_1_icp")));
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

        assert_eq!(reason.as_deref(), Some("raw_icp_share_below_1_icp"));
        assert_eq!(surplus[0].amount_e8s, 0);
        assert_eq!(
            surplus[0].skipped_reason.as_deref(),
            Some("raw_icp_share_below_1_icp")
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
