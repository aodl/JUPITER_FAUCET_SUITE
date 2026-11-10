//! Reserve-funded patron amounts, independent of processing time and Ledger fees.
use candid::Nat;

const DAY: u64 = 86_400;
// floor(2^(-1/1461) * 2^96), independently generated with 110-digit Decimal.
const DAILY_FACTOR_Q96: u128 = 79_190_582_943_866_507_388_228_089_184;

/// ICP and jUP both have eight decimals. Apply the multiplier before the only
/// token floor. Each Q96 multiplication rounds down. At most 1460 daily factors
/// contribute <1461/2^96 relative error; for any u64 ICP input the absolute
/// error is <0.000001 jUP base units. Consequently the integer result can be at
/// most one base unit below the exact real formula, and is never above it.
/// Whole halvings (including before launch) are exact. No floating point, time
/// subtraction overflow, linear-in-age work, or platform-dependent libm.
pub(crate) fn reward_e8s(
    qualifying_icp_e8s: u64,
    commitment_timestamp_seconds: u64,
    launch_timestamp_seconds: u64,
    compounded: bool,
) -> u64 {
    let days = commitment_timestamp_seconds.saturating_sub(launch_timestamp_seconds) / DAY;
    let halving_days = crate::economics::get().halving_days;
    let halves = days / halving_days;
    if halves >= 64 {
        return 0;
    }
    let mut exponent = days % halving_days;
    let mut factor = Nat::from(DAILY_FACTOR_Q96).0;
    let mut rate = Nat::from(1_u8).0 << 96_usize;
    while exponent != 0 {
        if exponent & 1 == 1 {
            rate = (rate * &factor) >> 96_usize;
        }
        factor = (&factor * &factor) >> 96_usize;
        exponent >>= 1;
    }
    let numerator = Nat::from(qualifying_icp_e8s).0 * u8::from(compounded).saturating_add(1) * rate;
    let amount = (numerator / crate::economics::get().patron_icp_per_jup) >> (96 + halves as usize);
    u64::try_from(amount).expect("reward is at most one fifth of the u64 ICP input")
}

pub(crate) fn reserve_cost(amount: u64, actual_fee: u64) -> Option<u64> {
    amount.checked_add(actual_fee)
}

#[cfg(test)]
mod tests {
    use super::*;
    const HALVING_DAYS: u64 = 1461;

    #[test]
    fn launch_days_halvings_and_multiplier_precede_floor() {
        let launch = 2_000_000;
        assert_eq!(reward_e8s(1_000_000_000, 0, launch, false), 100_000_000);
        assert_eq!(
            reward_e8s(1_000_000_000, launch + DAY - 1, launch, true),
            200_000_000
        );
        assert!(reward_e8s(1_000_000_000, launch + DAY, launch, false) < 100_000_000);
        for halves in 0..60 {
            assert_eq!(
                reward_e8s(
                    1_000_000_000,
                    launch + halves * HALVING_DAYS * DAY,
                    launch,
                    false
                ),
                100_000_000 >> halves
            );
        }
        assert_eq!(reward_e8s(5, launch, launch, false), 0);
        assert_eq!(reward_e8s(5, launch, launch, true), 1);
        assert_eq!(reward_e8s(10, launch, launch, false), 1);
        assert_eq!(reward_e8s(u64::MAX, u64::MAX, 0, true), 0);
    }

    #[test]
    fn high_precision_decimal_oracle_covers_every_fractional_day() {
        // This oracle uses decimal exp/ln, independent of the integer power
        // implementation. A zero-test or unavailable Python is a failure.
        let output = std::process::Command::new("python3")
            .args([
                "-c",
                r#"
from decimal import Decimal, localcontext
with localcontext() as c:
 c.prec=110
 for day in range(1462):
  rate=Decimal(2)**(-Decimal(day)/1461)
  for amount in [5,10,1000000000,18446744073709551615]:
   for multiplier in [1,2]:
    print(day,amount,multiplier,int(Decimal(amount)*multiplier*rate/10))
"#,
            ])
            .output()
            .expect("run independent Python decimal oracle");
        assert!(output.status.success(), "{:?}", output);
        let text = String::from_utf8(output.stdout).unwrap();
        let mut checked = 0;
        for line in text.lines() {
            let values: Vec<u64> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            let actual = reward_e8s(values[1], values[0] * DAY, 0, values[2] == 2);
            assert!(
                actual <= values[3] && values[3] - actual <= 1,
                "oracle {line}: {actual}"
            );
            checked += 1;
        }
        assert_eq!(checked, 1462 * 4 * 2);
    }

    #[test]
    fn fees_are_sponsored_and_tiny_payment_amplification_is_explicit() {
        let fee = 10_000_000;
        assert_eq!(
            reserve_cost(reward_e8s(1_000_000_000, 0, 0, false), fee),
            Some(110_000_000)
        );
        assert_eq!(
            reserve_cost(reward_e8s(1_000_000_000, 0, 0, true), fee),
            Some(210_000_000)
        );
        // Each commitment still satisfies Faucet's actual one-ICP minimum.
        // Far into the programme, each nonzero reward can be a single base unit.
        let late = 23 * HALVING_DAYS * DAY;
        let one = reserve_cost(reward_e8s(10_000_000_000_000_000, late, 0, false), fee).unwrap();
        let split = reserve_cost(reward_e8s(100_000_000, late, 0, false), fee).unwrap() as u128
            * 100_000_000;
        assert_eq!(split, 1_000_000_100_000_000);
        assert!(split > u128::from(one) * 7_000_000);
        assert_eq!(reserve_cost(u64::MAX, fee), None);
    }
}
