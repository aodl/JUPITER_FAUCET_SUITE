use icrc_ledger_types::icrc1::account::Account;

const SECS_PER_DAY: u64 = 86_400;
const SECS_PER_YEAR: u64 = 365 * SECS_PER_DAY;
const MAX_AGE_FOR_BONUS_SECS: u64 = 4 * SECS_PER_YEAR;

const BONUS_RECIPIENT_1_PCT: u64 = 95;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GrossSplit {
    pub base_e8s: u64,
    pub bonus_recipient_1_e8s: u64,
    pub bonus_recipient_2_e8s: u64,
}

fn age_multiplier_fraction(age_seconds: u64) -> (u128, u128) {
    // m = 1 + 0.25 * min(age,4y)/4y = 1 + min(age,4y) / 16y
    let den: u128 = (16 * SECS_PER_YEAR) as u128;
    let bonus_secs: u128 = age_seconds.min(MAX_AGE_FOR_BONUS_SECS) as u128;
    let num: u128 = den + bonus_secs;
    (num, den)
}

pub(crate) fn compute_gross_split(total_e8s: u64, age_seconds: u64) -> GrossSplit {
    if total_e8s == 0 {
        return GrossSplit {
            base_e8s: 0,
            bonus_recipient_1_e8s: 0,
            bonus_recipient_2_e8s: 0,
        };
    }

    let (num, den) = age_multiplier_fraction(age_seconds);
    let base = ((total_e8s as u128) * den / num) as u64;
    let bonus = total_e8s.saturating_sub(base);

    // 95/5, rounding toward recipient 1 side (ceil).
    let bonus1 = ((bonus as u128) * BONUS_RECIPIENT_1_PCT as u128).div_ceil(100);
    let bonus1 = bonus1 as u64;
    let bonus2 = bonus.saturating_sub(bonus1);

    GrossSplit {
        base_e8s: base,
        bonus_recipient_1_e8s: bonus1,
        bonus_recipient_2_e8s: bonus2,
    }
}

/// Memo layout (16 bytes): payout_id (8) + transfer_index (8)
fn build_memo(payout_id: u64, transfer_index: u64) -> [u8; 16] {
    let mut memo = [0u8; 16];
    memo[0..8].copy_from_slice(&payout_id.to_be_bytes());
    memo[8..16].copy_from_slice(&transfer_index.to_be_bytes());
    memo
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Planned {
    pub to: Account,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64, // gross - fee
    pub memo: [u8; 16],
    pub created_at_time_nanos: u64,
}

/// Single-pass planning:
/// - compute gross split on full staging balance
/// - for each destination:
///   if gross_share > fee => plan net transfer = gross_share - fee
///   else skip and leave it in staging
// Planning stays parameterized so tests can exercise payout math without canister state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_payout_transfers(
    payout_id: u64,
    created_at_base_nanos: u64,
    staging_balance_e8s: u64,
    fee_e8s: u64,
    age_seconds: u64,
    normal_to: &Account,
    bonus1_to: &Account,
    bonus2_to: &Account,
) -> (GrossSplit, Vec<Planned>) {
    let gross = compute_gross_split(staging_balance_e8s, age_seconds);

    let mut out: Vec<Planned> = Vec::with_capacity(3);
    let mut idx: u64 = 0;

    let mut push = |to: &Account, share: u64| {
        if share <= fee_e8s {
            return;
        }
        out.push(Planned {
            to: *to,
            gross_share_e8s: share,
            amount_e8s: share - fee_e8s,
            memo: build_memo(payout_id, idx),
            created_at_time_nanos: created_at_base_nanos.saturating_add(idx),
        });
        idx += 1;
    };

    push(normal_to, gross.base_e8s);
    push(bonus1_to, gross.bonus_recipient_1_e8s);
    push(bonus2_to, gross.bonus_recipient_2_e8s);

    (gross, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Principal;
    use std::collections::HashSet;

    fn acct() -> Account {
        Account {
            owner: Principal::anonymous(),
            subaccount: None,
        }
    }

    #[test]
    fn gross_split_age0_bonus0() {
        let g = compute_gross_split(1000, 0);
        assert_eq!(g.base_e8s, 1000);
        assert_eq!(g.bonus_recipient_1_e8s, 0);
        assert_eq!(g.bonus_recipient_2_e8s, 0);
    }

    #[test]
    fn gross_split_max_age_500_total() {
        // multiplier 1.25 => base=400, bonus=100 => 95/5 => 95/5
        let g = compute_gross_split(500, 4 * SECS_PER_YEAR);
        assert_eq!(g.base_e8s, 400);
        assert_eq!(g.bonus_recipient_1_e8s, 95);
        assert_eq!(g.bonus_recipient_2_e8s, 5);
        assert_eq!(
            g.base_e8s + g.bonus_recipient_1_e8s + g.bonus_recipient_2_e8s,
            500
        );
    }

    #[test]
    fn age_multiplier_clamps_at_four_years() {
        let (n4, d4) = age_multiplier_fraction(4 * SECS_PER_YEAR);
        let (n5, d5) = age_multiplier_fraction(5 * SECS_PER_YEAR);
        assert_eq!(n4, n5);
        assert_eq!(d4, d5);
    }

    #[test]
    fn base_is_monotone_decreasing_in_age_for_fixed_total() {
        let total = 1_000_000u64;
        let b0 = compute_gross_split(total, 0).base_e8s;
        let b2 = compute_gross_split(total, 2 * SECS_PER_YEAR).base_e8s;
        let b4 = compute_gross_split(total, 4 * SECS_PER_YEAR).base_e8s;
        assert!(b0 >= b2);
        assert!(b2 >= b4);
    }

    #[test]
    fn gross_split_bonus_rounding_invariants_small_totals() {
        // Use max bonus multiplier so bonus exists often.
        let age = 4 * SECS_PER_YEAR;
        for total in 1u64..200 {
            let g = compute_gross_split(total, age);
            assert_eq!(
                g.base_e8s + g.bonus_recipient_1_e8s + g.bonus_recipient_2_e8s,
                total
            );

            let bonus = total - g.base_e8s;
            let expected_bonus1 = ((bonus as u128) * BONUS_RECIPIENT_1_PCT as u128).div_ceil(100);
            assert_eq!(g.bonus_recipient_1_e8s, expected_bonus1 as u64);
            assert_eq!(g.bonus_recipient_2_e8s, bonus - g.bonus_recipient_1_e8s);
        }
    }

    #[test]
    fn plan_skips_shares_below_fee() {
        let a = acct();
        let fee = 10;
        let (gross, plan) = plan_payout_transfers(1, 1000, 100, fee, 0, &a, &a, &a);
        assert_eq!(gross.base_e8s, 100);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].amount_e8s, 90);
        assert_eq!(plan[0].gross_share_e8s, 100);
    }

    #[test]
    fn plan_respects_fee_threshold_and_cost_never_exceeds_balance() {
        let a = acct();
        let fee = 10_000;
        let balance = 1_000_000;
        let (_gross, plan) =
            plan_payout_transfers(99, 5_000, balance, fee, 4 * SECS_PER_YEAR, &a, &a, &a);

        // Each planned transfer must have gross_share > fee and amount = gross - fee
        for p in plan.iter() {
            assert!(p.gross_share_e8s > fee);
            assert_eq!(p.amount_e8s, p.gross_share_e8s - fee);
        }

        // Total debited from staging is sum(gross_share of planned transfers)
        let total_cost = plan.iter().map(|p| p.amount_e8s + fee).sum::<u64>();
        assert!(total_cost <= balance);
    }

    #[test]
    fn memo_unique_per_transfer_index() {
        let a = acct();
        let fee = 1;
        let (_gross, plan) =
            plan_payout_transfers(7, 1000, 1000, fee, 4 * SECS_PER_YEAR, &a, &a, &a);
        assert_eq!(plan.len(), 3);

        let mut set = HashSet::new();
        for p in plan {
            assert!(set.insert(p.memo.to_vec()));
        }
    }

    #[test]
    fn plan_empty_when_fee_ge_balance() {
        let a = acct();
        let fee = 100;
        let balance = 50;
        let (_gross, plan) = plan_payout_transfers(1, 1000, balance, fee, 0, &a, &a, &a);
        assert!(plan.is_empty());
    }

    #[test]
    fn gross_split_sums_for_various_ages_and_totals() {
        let ages = [
            0u64,
            SECS_PER_YEAR,
            2 * SECS_PER_YEAR,
            4 * SECS_PER_YEAR,
            10 * SECS_PER_YEAR, // should clamp at 4y
        ];

        let totals = [
            0u64,
            1,
            2,
            10,
            123,
            100_000_000,       // 1 ICP in e8s scale-ish
            1_000_000_000_000, // large
        ];

        for &age in &ages {
            for &total in &totals {
                let g = compute_gross_split(total, age);
                assert_eq!(
                    g.base_e8s + g.bonus_recipient_1_e8s + g.bonus_recipient_2_e8s,
                    total,
                    "sum invariant failed for total={total}, age={age}"
                );
                assert!(
                    g.base_e8s <= total,
                    "base should never exceed total for total={total}, age={age}"
                );
            }
        }
    }

    #[test]
    fn plan_memos_and_timestamps_are_stable_monotonic_transfer_sequence() {
        let a = acct();
        let payout_id = 42u64;
        let created_at = 1_000_000u64;
        let balance = 987_654_321u64;
        let fee = 10_000u64;
        let age = 2 * SECS_PER_YEAR;

        let (_gross, plan) =
            plan_payout_transfers(payout_id, created_at, balance, fee, age, &a, &a, &a);

        assert!(plan.len() <= 3);
        for (idx, transfer) in plan.iter().enumerate() {
            let expected_idx = idx as u64;
            assert_eq!(transfer.memo, build_memo(payout_id, expected_idx));
            assert_eq!(transfer.created_at_time_nanos, created_at + expected_idx);
            assert!(
                transfer.gross_share_e8s > fee,
                "planned transfer should always clear the ledger fee threshold"
            );
            assert_eq!(transfer.amount_e8s, transfer.gross_share_e8s - fee);
        }
    }

    #[test]
    fn memo_format_is_big_endian_and_stable() {
        // payout_id = 0x0102030405060708
        // index    = 0x0A0B0C0D0E0F1011
        let memo = build_memo(0x0102030405060708, 0x0A0B0C0D0E0F1011);

        assert_eq!(
            memo,
            [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
                0x10, 0x11
            ]
        );
    }

    #[test]
    fn base_is_stable_when_total_scales_with_age_multiplier() {
        // In NNS, the age bonus increases a neuron's voting power and therefore
        // increases the maturity earned per reward event. The "base" portion
        // corresponds to the no-age component and should remain stable when the
        // total scales proportionally with the age multiplier.
        //
        // Use a base_total divisible by 16 so that scaling by the multiplier
        // (1 + age/16y) stays exact for whole-year ages.
        let base_total = 1_000_000u64;

        let ages = [
            0u64,
            SECS_PER_YEAR,
            2 * SECS_PER_YEAR,
            3 * SECS_PER_YEAR,
            4 * SECS_PER_YEAR,
        ];

        let mut prev_bonus = 0u64;
        for &age in &ages {
            let (num, den) = age_multiplier_fraction(age);
            let total = ((base_total as u128) * num / den) as u64;

            let g = compute_gross_split(total, age);

            // Base should match the no-age baseline (exact for these inputs).
            assert_eq!(
                g.base_e8s, base_total,
                "base drifted at age={age} total={total}"
            );

            let bonus = g.bonus_recipient_1_e8s + g.bonus_recipient_2_e8s;
            assert!(
                bonus >= prev_bonus,
                "bonus should be monotone in age: prev={prev_bonus} now={bonus} age={age}"
            );
            prev_bonus = bonus;
        }
    }
}
