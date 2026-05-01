import test from 'node:test';
import assert from 'node:assert/strict';

import {
  AGE_BONUS_FULL_AGE_SECONDS,
  CYCLES_PER_PRICE_UNIT,
  DEFAULT_ANNUAL_APY_BASIS_POINTS,
  E8S_PER_ICP,
  MAX_AGE_BONUS_BASIS_POINTS,
  PROJECTION_BUCKETS,
  PROJECTION_WEEKS,
  TRILLION_CYCLES,
  buildSimulatorProjection,
  calculateAgeBonusBasisPointsFromAgeSeconds,
  calculateAgeBonusBasisPointsFromAgingSince,
  calculateAgeBonusMaturityShareBasisPoints,
  calculateEffectiveApyBasisPoints,
  parseDecimalToScaledBigInt,
} from '../src/projection-simulator.js';

const WEEKS_PER_YEAR = 52n;

function okProjection(inputs) {
  const projection = buildSimulatorProjection(inputs);
  assert.equal(projection.ok, true, projection.errors?.join(' '));
  return projection;
}

function annualTopupCyclesForCommitmentE8s({ commitmentE8s, assumedIcpPrice = '7.2', annualApyPercent = '7.0', ageBonusBasisPoints = 0n }) {
  const priceUnits = parseDecimalToScaledBigInt(assumedIcpPrice, 10_000n, {
    allowZero: false,
    maxFractionDigits: 1,
    fieldName: 'Assumed ICP/XDR price',
  });
  const tenthsOfPercent = parseDecimalToScaledBigInt(annualApyPercent, 10n, {
    allowZero: true,
    maxFractionDigits: 1,
    fieldName: 'APY',
  });
  const configuredApyBasisPoints = tenthsOfPercent * 10n;
  const effectiveApyBasisPoints = calculateEffectiveApyBasisPoints(configuredApyBasisPoints, ageBonusBasisPoints);
  const cyclesPerIcp = (CYCLES_PER_PRICE_UNIT * priceUnits) / 10_000n;
  const annualTopupE8s = (commitmentE8s * effectiveApyBasisPoints) / 10_000n;
  return (annualTopupE8s * cyclesPerIcp) / E8S_PER_ICP;
}

function sumBuckets(projection, key) {
  return projection.buckets.reduce((sum, bucket) => sum + bucket[key], 0n);
}

test('default projection uses 7.0% configured APY, 0.001T daily burn, and the 1 ICP commitment floor', () => {
  const projection = okProjection({
    assumedIcpPrice: '10.0',
    dailyBurnTrillionCycles: '0.001',
    icpCommitment: '100.0',
    annualApyPercent: '7.0',
    ageBonusBasisPoints: 0n,
  });

  assert.equal(DEFAULT_ANNUAL_APY_BASIS_POINTS, 700n);
  assert.equal(projection.buckets.length, PROJECTION_BUCKETS);
  assert.equal(projection.inputs.annualApyBasisPoints, 700n);
  assert.equal(projection.inputs.effectiveApyBasisPoints, 700n);
  assert.equal(projection.inputs.ageBonusBasisPoints, 0n);
  assert.equal(projection.inputs.ageBonusMaturityShareBasisPoints, 0n);
  assert.equal(projection.inputs.dailyBurnCycles, TRILLION_CYCLES / 1000n);
  assert.equal(projection.summary.cyclesPerIcp, 10n * CYCLES_PER_PRICE_UNIT);
  assert.equal(projection.summary.annualTopupE8s, 7n * E8S_PER_ICP);
  assert.equal(projection.summary.annualTopupCycles, 70n * TRILLION_CYCLES);
  assert.equal(projection.summary.annualBurnCycles, 365_000_000_000n);
  assert.equal(PROJECTION_WEEKS, 52);
  assert.equal(projection.summary.initialPayoutCycles, 1_346_153_846_153n);
  assert.equal(projection.summary.yearEndBalanceCycles, 70_981_153_846_153n);
  assert.equal('requiredStartingBufferCycles' in projection.summary, false);
  assert.equal(projection.summary.requiredCommitmentE8s, E8S_PER_ICP);
  assert.equal(projection.summary.isSustainableAtCurrentBurn, true);
});

test('age bonus calculation matches the NNS four-year ramp and the maturity component share', () => {
  assert.equal(calculateAgeBonusBasisPointsFromAgeSeconds(0n), 0n);
  assert.equal(calculateAgeBonusBasisPointsFromAgeSeconds(AGE_BONUS_FULL_AGE_SECONDS / 2n), 1_250n);
  assert.equal(calculateAgeBonusBasisPointsFromAgeSeconds(AGE_BONUS_FULL_AGE_SECONDS), MAX_AGE_BONUS_BASIS_POINTS);
  assert.equal(calculateAgeBonusBasisPointsFromAgeSeconds(AGE_BONUS_FULL_AGE_SECONDS * 2n), MAX_AGE_BONUS_BASIS_POINTS);
  assert.equal(calculateAgeBonusBasisPointsFromAgingSince(1_000n, 1_000n), 0n);
  assert.equal(calculateAgeBonusBasisPointsFromAgingSince(1_000n, 1_000n + AGE_BONUS_FULL_AGE_SECONDS), MAX_AGE_BONUS_BASIS_POINTS);
  assert.equal(calculateAgeBonusMaturityShareBasisPoints(0n), 0n);
  assert.equal(calculateAgeBonusMaturityShareBasisPoints(1_250n), 1_111n);
  assert.equal(calculateAgeBonusMaturityShareBasisPoints(2_500n), 2_000n);
});

test('simulator discounts configured APY by the current age-bonus component before projecting top-ups', () => {
  const noAgeBonus = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.001', icpCommitment: '100.0', annualApyPercent: '7.0', ageBonusBasisPoints: 0n });
  const fullAgeBonus = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.001', icpCommitment: '100.0', annualApyPercent: '7.0', ageBonusBasisPoints: 2_500n });

  assert.equal(fullAgeBonus.summary.ageBonusMaturityShareBasisPoints, 2_000n);
  assert.equal(fullAgeBonus.summary.effectiveApyBasisPoints, 560n);
  assert.equal(fullAgeBonus.summary.annualTopupE8s, 560_000_000n);
  assert.equal(fullAgeBonus.summary.annualTopupCycles, 56n * TRILLION_CYCLES);
  assert.equal(fullAgeBonus.summary.annualTopupCycles * 5n, noAgeBonus.summary.annualTopupCycles * 4n);
});

test('weekly projection buckets reconcile exactly while rounding drift stays bounded', () => {
  const projection = okProjection({
    assumedIcpPrice: '7.2',
    dailyBurnTrillionCycles: '0.001',
    icpCommitment: '42.5',
    annualApyPercent: '7.0',
    ageBonusBasisPoints: 1_250n,
  });

  assert.equal(projection.buckets.length, PROJECTION_WEEKS);
  assert.equal(projection.buckets[0].label, 'W1');
  assert.equal(projection.buckets.at(-1).label, 'W52');

  const assertSpreadAtMostOne = (key) => {
    const values = projection.buckets.map((bucket) => bucket[key]);
    const min = values.reduce((left, right) => left < right ? left : right);
    const max = values.reduce((left, right) => left > right ? left : right);
    assert.ok(max - min <= 1n, `${key} weekly rounding drift should be at most 1 base unit`);
  };

  assertSpreadAtMostOne('projectedTopupCycles');
  assertSpreadAtMostOne('projectedTopupE8s');
  assertSpreadAtMostOne('projectedBurnCycles');
  assert.equal(sumBuckets(projection, 'projectedTopupCycles'), projection.summary.annualTopupCycles);
  assert.equal(sumBuckets(projection, 'projectedTopupE8s'), projection.summary.annualTopupE8s);
  assert.equal(sumBuckets(projection, 'projectedBurnCycles'), projection.summary.annualBurnCycles);
  assert.equal(projection.buckets.at(-1).projectedBalanceCycles, projection.summary.yearEndBalanceCycles);
});

test('balance projection starts with a day-one payout and then changes linearly with the annual net rate', () => {
  const overfunded = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.001', icpCommitment: '100.0', annualApyPercent: '7.0', ageBonusBasisPoints: 0n });
  const underfunded = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.100', icpCommitment: '20.0', annualApyPercent: '7.0', ageBonusBasisPoints: 0n });

  for (const projection of [overfunded, underfunded]) {
    const annualNet = projection.summary.annualTopupCycles - projection.summary.annualBurnCycles;
    let previous = null;
    projection.buckets.forEach((bucket, index) => {
      const week = BigInt(index + 1);
      const expected = projection.summary.initialPayoutCycles
        + (projection.summary.annualTopupCycles * week) / WEEKS_PER_YEAR
        - (projection.summary.annualBurnCycles * week) / WEEKS_PER_YEAR;
      assert.equal(bucket.projectedBalanceCycles, expected);
      if (previous !== null) {
        if (annualNet >= 0n) assert.ok(bucket.projectedBalanceCycles >= previous);
        else assert.ok(bucket.projectedBalanceCycles <= previous);
      }
      previous = bucket.projectedBalanceCycles;
    });
    assert.equal(projection.buckets.at(-1).projectedBalanceCycles, projection.summary.initialPayoutCycles + annualNet);
  }
});

test('break-even annual funding leaves the balance line flat at the single projected payout amount', () => {
  const projection = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.001', icpCommitment: '1.0', annualApyPercent: '7.0', ageBonusBasisPoints: 0n });

  assert.ok(projection.summary.annualTopupCycles >= projection.summary.annualBurnCycles);
  assert.ok(projection.summary.initialPayoutCycles > 0n);
  assert.ok(projection.buckets.every((bucket) => bucket.projectedBalanceCycles >= projection.summary.initialPayoutCycles));
  assert.equal('requiredStartingBufferCycles' in projection.summary, false);
});

test('break-even commitment is the minimal e8s amount that covers the annual burn after age-bonus discount', () => {
  const input = { assumedIcpPrice: '7.2', dailyBurnTrillionCycles: '0.100', icpCommitment: '1.0', annualApyPercent: '7.0', ageBonusBasisPoints: 1_250n };
  const baseline = okProjection(input);
  const required = baseline.summary.requiredCommitmentE8s;
  assert.ok(required >= E8S_PER_ICP);

  assert.ok(annualTopupCyclesForCommitmentE8s({ commitmentE8s: required, ageBonusBasisPoints: 1_250n }) >= baseline.summary.annualBurnCycles);
  assert.ok(annualTopupCyclesForCommitmentE8s({ commitmentE8s: required - 1n, ageBonusBasisPoints: 1_250n }) < baseline.summary.annualBurnCycles);
});

test('higher assumed ICP/XDR price lowers the required commitment for the same burn', () => {
  const lowPrice = okProjection({ assumedIcpPrice: '5.0', dailyBurnTrillionCycles: '0.100', icpCommitment: '1.0', annualApyPercent: '7.0' });
  const highPrice = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.100', icpCommitment: '1.0', annualApyPercent: '7.0' });

  assert.ok(lowPrice.summary.requiredCommitmentE8s >= highPrice.summary.requiredCommitmentE8s * 2n - 1n);
  assert.ok(lowPrice.summary.requiredCommitmentE8s <= highPrice.summary.requiredCommitmentE8s * 2n);
});

test('higher configured APY lowers the required commitment and increases annual top-ups', () => {
  const sevenPercent = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.100', icpCommitment: '100.0', annualApyPercent: '7.0' });
  const threePointFivePercent = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.100', icpCommitment: '100.0', annualApyPercent: '3.5' });
  const zeroPercent = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.100', icpCommitment: '100.0', annualApyPercent: '0.0' });

  assert.equal(threePointFivePercent.summary.annualTopupE8s * 2n, sevenPercent.summary.annualTopupE8s);
  assert.equal(threePointFivePercent.summary.annualTopupCycles * 2n, sevenPercent.summary.annualTopupCycles);
  assert.ok(threePointFivePercent.summary.requiredCommitmentE8s >= sevenPercent.summary.requiredCommitmentE8s * 2n - 1n);
  assert.ok(threePointFivePercent.summary.requiredCommitmentE8s <= sevenPercent.summary.requiredCommitmentE8s * 2n);
  assert.equal(zeroPercent.summary.annualTopupCycles, 0n);
  assert.equal(zeroPercent.summary.initialPayoutCycles, 0n);
  assert.equal(zeroPercent.summary.requiredCommitmentE8s, null);
  assert.equal(zeroPercent.summary.isSustainableAtCurrentBurn, false);
});

test('zero burn and a valid minimum commitment produce an overfunded projection with a 1 ICP required commitment', () => {
  const projection = okProjection({ assumedIcpPrice: '10.0', dailyBurnTrillionCycles: '0.000', icpCommitment: '1.0', annualApyPercent: '7.0' });

  assert.equal(projection.summary.annualBurnCycles, 0n);
  assert.ok(projection.summary.annualTopupCycles > 0n);
  assert.ok(projection.summary.initialPayoutCycles > 0n);
  assert.equal(projection.summary.requiredCommitmentE8s, E8S_PER_ICP);
  assert.equal(projection.summary.isSustainableAtCurrentBurn, true);
});

test('user-facing simulator inputs allow one decimal except daily burn which allows three decimals', () => {
  const projection = okProjection({ assumedIcpPrice: '10.1', dailyBurnTrillionCycles: '1.123', icpCommitment: '2.3', annualApyPercent: '7.4', ageBonusBasisPoints: 500n });

  assert.equal(projection.inputs.assumedIcpPriceUnits, 101_000n);
  assert.equal(projection.inputs.dailyBurnCycles, 1_123n * TRILLION_CYCLES / 1000n);
  assert.equal(projection.inputs.icpCommitmentE8s, 230_000_000n);
  assert.equal(projection.inputs.annualApyBasisPoints, 740n);
  assert.equal(projection.inputs.ageBonusBasisPoints, 500n);
});

test('decimal parser accepts separators but rejects invalid precision, negative values, and zero when disallowed', () => {
  assert.equal(parseDecimalToScaledBigInt('1.25', 100n), 125n);
  assert.equal(parseDecimalToScaledBigInt('1,234.50', 100n), 123450n);
  assert.equal(parseDecimalToScaledBigInt('1_234.50', 100n), 123450n);
  assert.throws(() => parseDecimalToScaledBigInt('-1', 100n), /non-negative decimal/);
  assert.throws(() => parseDecimalToScaledBigInt('1.234', 100n, { maxFractionDigits: 2 }), /at most 2 decimal places/);
  assert.throws(() => parseDecimalToScaledBigInt('0', 100n, { allowZero: false, fieldName: 'Test field' }), /Test field must be greater than zero/);
});

test('normalisation reports all invalid user-facing simulator inputs together', () => {
  const projection = buildSimulatorProjection({
    assumedIcpPrice: '0',
    dailyBurnTrillionCycles: '1.1111',
    icpCommitment: '0.5',
    annualApyPercent: '7.11',
    ageBonusBasisPoints: -1n,
  });

  assert.equal(projection.ok, false);
  assert.match(projection.errors.join(' '), /Assumed ICP\/XDR price must be greater than zero/);
  assert.match(projection.errors.join(' '), /Daily burn in T cycles supports at most 3 decimal places/);
  assert.match(projection.errors.join(' '), /ICP commitment must be at least 1 ICP/);
  assert.match(projection.errors.join(' '), /APY supports at most 1 decimal place/);
  assert.match(projection.errors.join(' '), /Age bonus must be non-negative/);
});
