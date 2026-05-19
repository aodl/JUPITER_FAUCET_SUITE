use std::collections::{BTreeMap, BTreeSet};

use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;

use crate::state::{
    CanisterBurnSample, Config, CyclesSampleSource, CyclesSnapshot, RawIcpRecipient,
};

pub(crate) const MEMO_TOP_UP_CANISTER_U64: u64 = 1_347_768_404;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BurnPlan {
    pub canister_id: Principal,
    pub previous_cycles: Option<u128>,
    pub current_cycles: u128,
    pub burn_cycles: u128,
    pub weight: u128,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub skipped_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawIcpPlan {
    pub recipient: RawIcpRecipient,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub retain_self: bool,
    pub skipped_reason: Option<String>,
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

pub(crate) fn compute_burn(previous: u128, current: u128) -> u128 {
    previous.saturating_sub(current)
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

    if let Some(raw) = &cfg.raw_icp_mode {
        if raw.recipients.is_empty() {
            return Err("raw_icp_mode.recipients must not be empty".to_string());
        }
        let mut accounts = BTreeSet::new();
        for recipient in &raw.recipients {
            validate_account_owner(
                "raw_icp_mode.recipients.account.owner",
                recipient.account.owner,
            )?;
            if !accounts.insert(recipient.account) {
                return Err(format!(
                    "duplicate raw ICP recipient account: owner={} subaccount={:?}",
                    recipient.account.owner.to_text(),
                    recipient.account.subaccount
                ));
            }
        }
    }

    if cfg.main_interval_seconds < 60 {
        return Err("main_interval_seconds must be at least 60 after clamping".to_string());
    }

    let _ = effective_managed_canisters(&cfg.managed_canisters, self_id);
    Ok(())
}

fn validate_canister_principal(name: &str, principal: Principal) -> Result<(), String> {
    validate_account_owner(name, principal)
}

fn validate_account_owner(name: &str, principal: Principal) -> Result<(), String> {
    if principal == Principal::anonymous() {
        return Err(format!("{name} must not be anonymous"));
    }
    if principal == Principal::management_canister() {
        return Err(format!("{name} must not be the management canister"));
    }
    Ok(())
}

pub(crate) fn build_burn_plan(
    current: &BTreeMap<Principal, CyclesSnapshot>,
    previous: &BTreeMap<Principal, CyclesSnapshot>,
    available_balance_e8s: u64,
    fee_e8s: u64,
) -> Vec<BurnPlan> {
    let mut base = current
        .iter()
        .map(|(canister_id, current_snapshot)| {
            let previous_cycles = previous.get(canister_id).map(|sample| sample.cycles);
            let burn_cycles = previous_cycles
                .map(|prev| compute_burn(prev, current_snapshot.cycles))
                .unwrap_or(0);
            BurnPlan {
                canister_id: *canister_id,
                previous_cycles,
                current_cycles: current_snapshot.cycles,
                burn_cycles,
                weight: 0,
                gross_share_e8s: 0,
                amount_e8s: 0,
                skipped_reason: None,
            }
        })
        .collect::<Vec<_>>();
    let total_burn: u128 = base.iter().map(|p| p.burn_cycles).sum();
    if total_burn > 0 {
        for plan in &mut base {
            plan.weight = plan.burn_cycles;
        }
    } else {
        for plan in &mut base {
            plan.weight = 1;
        }
    }
    allocate_weighted_gross_shares(&mut base, available_balance_e8s, fee_e8s);
    base
}

pub(crate) fn allocate_weighted_gross_shares(
    plans: &mut [BurnPlan],
    available_balance_e8s: u64,
    fee_e8s: u64,
) {
    let total_weight: u128 = plans.iter().map(|p| p.weight).sum();
    if total_weight == 0 {
        return;
    }
    for plan in plans {
        let gross = (available_balance_e8s as u128)
            .saturating_mul(plan.weight)
            .checked_div(total_weight)
            .unwrap_or(0)
            .min(u64::MAX as u128) as u64;
        plan.gross_share_e8s = gross;
        if gross == 0 {
            plan.skipped_reason = Some("zero gross share".to_string());
        } else if gross <= fee_e8s {
            plan.skipped_reason = Some("gross share does not exceed ledger fee".to_string());
        } else {
            plan.amount_e8s = gross - fee_e8s;
        }
    }
}

pub(crate) fn allocate_equal_raw_icp_shares(
    recipients: &[RawIcpRecipient],
    default_account: Account,
    available_balance_e8s: u64,
    fee_e8s: u64,
) -> Vec<RawIcpPlan> {
    if recipients.is_empty() {
        return Vec::new();
    }
    let gross = available_balance_e8s / recipients.len() as u64;
    recipients
        .iter()
        .cloned()
        .map(|recipient| classify_raw_icp_candidate(recipient, default_account, gross, fee_e8s))
        .collect()
}

pub(crate) fn classify_raw_icp_candidate(
    recipient: RawIcpRecipient,
    default_account: Account,
    gross_share_e8s: u64,
    fee_e8s: u64,
) -> RawIcpPlan {
    if is_self_default_account(&recipient.account, default_account.owner) {
        return RawIcpPlan {
            recipient,
            gross_share_e8s,
            amount_e8s: 0,
            retain_self: true,
            skipped_reason: Some("self default account retained".to_string()),
        };
    }
    if gross_share_e8s <= fee_e8s {
        return RawIcpPlan {
            recipient,
            gross_share_e8s,
            amount_e8s: 0,
            retain_self: false,
            skipped_reason: Some("gross share does not exceed ledger fee".to_string()),
        };
    }
    RawIcpPlan {
        recipient,
        gross_share_e8s,
        amount_e8s: gross_share_e8s - fee_e8s,
        retain_self: false,
        skipped_reason: None,
    }
}

pub(crate) fn is_self_default_account(account: &Account, self_id: Principal) -> bool {
    *account == default_account(self_id)
}

pub(crate) fn raw_mode_active(
    min_cycles: Option<u128>,
    threshold: u128,
    configured: bool,
    probe_failed: bool,
) -> bool {
    configured && !probe_failed && min_cycles.map(|cycles| cycles > threshold).unwrap_or(false)
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
            burn_cycles: value.burn_cycles,
            weight: value.weight,
            gross_share_e8s: value.gross_share_e8s,
            amount_e8s: value.amount_e8s,
            skipped_reason: value.skipped_reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{RawIcpModeConfig, RawIcpRecipient};

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
            blackhole_canister_id: canister_b(),
            main_interval_seconds: 60,
            max_transfers_per_tick: None,
            raw_icp_mode: None,
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
    fn effective_set_includes_self_and_deduplicates() {
        let self_id = canister_b();
        let effective = effective_managed_canisters(&[canister_a(), canister_a()], self_id);
        assert_eq!(effective.len(), 2);
        assert!(effective.contains(&canister_a()));
        assert!(effective.contains(&self_id));
    }

    #[test]
    fn config_validation_rejects_bad_principals_and_duplicate_managed() {
        let self_id = canister_b();
        let mut cfg = config();
        cfg.managed_canisters = vec![Principal::anonymous()];
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("anonymous"));
        cfg.managed_canisters = vec![Principal::management_canister()];
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("management"));
        cfg.managed_canisters = vec![canister_a(), canister_a()];
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("duplicate"));
    }

    #[test]
    fn config_validation_rejects_zero_max_transfers_per_tick() {
        let self_id = canister_b();
        let mut cfg = config();
        cfg.max_transfers_per_tick = Some(0);
        assert_eq!(
            validate_config(&cfg, self_id).unwrap_err(),
            "max_transfers_per_tick must be greater than zero when set"
        );
    }

    #[test]
    fn raw_mode_validation_allows_self_default_but_rejects_duplicates() {
        let self_id = canister_b();
        let mut cfg = config();
        cfg.raw_icp_mode = Some(RawIcpModeConfig {
            min_cycles_threshold: 0,
            recipients: vec![RawIcpRecipient {
                account: default_account(self_id),
                memo: None,
            }],
        });
        assert!(validate_config(&cfg, self_id).is_ok());
        cfg.raw_icp_mode
            .as_mut()
            .unwrap()
            .recipients
            .push(RawIcpRecipient {
                account: default_account(self_id),
                memo: None,
            });
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("duplicate"));
    }

    #[test]
    fn raw_mode_validation_rejects_empty_recipient_list() {
        let self_id = canister_b();
        let mut cfg = config();
        cfg.raw_icp_mode = Some(RawIcpModeConfig {
            min_cycles_threshold: 0,
            recipients: Vec::new(),
        });
        assert!(validate_config(&cfg, self_id)
            .unwrap_err()
            .contains("must not be empty"));
    }

    #[test]
    fn relay_non_default_subaccount_is_not_self_default() {
        let self_id = canister_a();
        let mut subaccount = [0_u8; 32];
        subaccount[31] = 1;
        let account = Account {
            owner: self_id,
            subaccount: Some(subaccount),
        };
        assert!(!is_self_default_account(&account, self_id));
    }

    #[test]
    fn burn_saturates_at_zero() {
        assert_eq!(compute_burn(10_000, 7_000), 3_000);
        assert_eq!(compute_burn(7_000, 10_000), 0);
    }

    #[test]
    fn weighted_shares_follow_burns_and_skip_idle_when_positive_burn_exists() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(900));
        current.insert(canister_b(), snapshot(400));
        previous.insert(canister_a(), snapshot(1000));
        previous.insert(canister_b(), snapshot(1000));
        let plans = build_burn_plan(&current, &previous, 1_000, 10);
        let a = plans
            .iter()
            .find(|plan| plan.canister_id == canister_a())
            .unwrap();
        let b = plans
            .iter()
            .find(|plan| plan.canister_id == canister_b())
            .unwrap();
        assert_eq!(a.gross_share_e8s, 142);
        assert_eq!(b.gross_share_e8s, 857);
        assert!(plans.iter().map(|p| p.gross_share_e8s).sum::<u64>() <= 1_000);
    }

    #[test]
    fn positive_burn_distribution_uses_relative_burn_weights() {
        let a = canister_a();
        let b = canister_b();
        let c = principal("rkp4c-7iaaa-aaaaa-aaaca-cai");
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(a, snapshot(900));
        current.insert(b, snapshot(700));
        current.insert(c, snapshot(400));
        previous.insert(a, snapshot(1000));
        previous.insert(b, snapshot(1000));
        previous.insert(c, snapshot(1000));

        let plans = build_burn_plan(&current, &previous, 10_000, 10);
        let by_id = plans
            .iter()
            .map(|plan| (plan.canister_id, plan.gross_share_e8s))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(by_id[&a], 1_000);
        assert_eq!(by_id[&b], 3_000);
        assert_eq!(by_id[&c], 6_000);
    }

    #[test]
    fn zero_burn_splits_equally() {
        let mut current = BTreeMap::new();
        let previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(1000));
        current.insert(canister_b(), snapshot(1000));
        let plans = build_burn_plan(&current, &previous, 1_001, 10);
        assert_eq!(plans[0].weight, 1);
        assert_eq!(plans[1].weight, 1);
        assert_eq!(plans[0].gross_share_e8s, 500);
        assert_eq!(plans[1].gross_share_e8s, 500);
    }

    #[test]
    fn positive_burn_gives_idle_canister_zero_share() {
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        current.insert(canister_a(), snapshot(1000));
        current.insert(canister_b(), snapshot(900));
        previous.insert(canister_a(), snapshot(1000));
        previous.insert(canister_b(), snapshot(1000));
        let plans = build_burn_plan(&current, &previous, 1_000, 10);
        let a = plans
            .iter()
            .find(|plan| plan.canister_id == canister_a())
            .unwrap();
        let b = plans
            .iter()
            .find(|plan| plan.canister_id == canister_b())
            .unwrap();
        assert_eq!(a.gross_share_e8s, 0);
        assert_eq!(b.gross_share_e8s, 1_000);
    }

    #[test]
    fn gross_share_at_or_below_fee_is_skipped_and_dust_retained_by_flooring() {
        let mut current = BTreeMap::new();
        current.insert(canister_a(), snapshot(1000));
        current.insert(canister_b(), snapshot(1000));
        let plans = build_burn_plan(&current, &BTreeMap::new(), 21, 10);

        assert_eq!(plans[0].gross_share_e8s, 10);
        assert_eq!(plans[0].amount_e8s, 0);
        assert!(plans[0]
            .skipped_reason
            .as_ref()
            .unwrap()
            .contains("ledger fee"));
        assert_eq!(plans.iter().map(|p| p.gross_share_e8s).sum::<u64>(), 20);
        assert!(plans.iter().map(|p| p.gross_share_e8s).sum::<u64>() <= 21);
    }

    #[test]
    fn raw_icp_activation_is_strictly_above_threshold() {
        assert!(!raw_mode_active(Some(100), 100, true, false));
        assert!(raw_mode_active(Some(101), 100, true, false));
        assert!(!raw_mode_active(Some(101), 100, false, false));
        assert!(!raw_mode_active(Some(101), 100, true, true));
    }

    #[test]
    fn raw_icp_equal_split_retains_self_default_and_charges_fee_per_transfer() {
        let self_id = canister_a();
        let mut sub = [0u8; 32];
        sub[31] = 7;
        let recipients = vec![
            RawIcpRecipient {
                account: default_account(self_id),
                memo: None,
            },
            RawIcpRecipient {
                account: Account {
                    owner: self_id,
                    subaccount: Some(sub),
                },
                memo: Some(vec![1, 2, 3]),
            },
        ];
        let plans = allocate_equal_raw_icp_shares(&recipients, default_account(self_id), 1_000, 10);
        assert!(plans[0].retain_self);
        assert_eq!(plans[0].amount_e8s, 0);
        assert_eq!(plans[1].gross_share_e8s, 500);
        assert_eq!(plans[1].amount_e8s, 490);
        assert_eq!(plans[1].recipient.memo, Some(vec![1, 2, 3]));
    }
}
