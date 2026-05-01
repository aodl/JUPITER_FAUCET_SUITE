export const E8S_PER_ICP = 100_000_000n;
export const CYCLES_PER_PRICE_UNIT = 1_000_000_000_000n;
export const TRILLION_CYCLES = 1_000_000_000_000n;
export const PROJECTION_WEEKS = 52;
export const PROJECTION_BUCKETS = PROJECTION_WEEKS;
export const DEFAULT_ANNUAL_APY_BASIS_POINTS = 700n;
export const SECONDS_PER_DAY = 24n * 60n * 60n;
export const AGE_BONUS_FULL_AGE_SECONDS = 4n * 365n * SECONDS_PER_DAY;
export const MAX_AGE_BONUS_BASIS_POINTS = 2_500n;

function cleanDecimalInput(value) {
  return String(value ?? '').trim().replace(/_/g, '').replace(/,/g, '');
}

export function parseDecimalToScaledBigInt(value, scale, { allowZero = true, maxFractionDigits = null, fieldName = 'value' } = {}) {
  const text = cleanDecimalInput(value);
  const scaleBigInt = typeof scale === 'bigint' ? scale : BigInt(scale);
  if (!text) throw new Error(`${fieldName} is required.`);
  if (!/^\d+(?:\.\d+)?$/.test(text)) throw new Error(`${fieldName} must be a non-negative decimal number.`);
  const [wholePart, rawFraction = ''] = text.split('.');
  if (maxFractionDigits !== null && rawFraction.length > maxFractionDigits) {
    throw new Error(`${fieldName} supports at most ${maxFractionDigits} decimal place${maxFractionDigits === 1 ? '' : 's'}.`);
  }
  const scaleDigits = scaleBigInt.toString().length - 1;
  if (scaleBigInt !== 10n ** BigInt(scaleDigits)) {
    throw new Error('scale must be a power of ten');
  }
  const fraction = rawFraction.padEnd(scaleDigits, '0').slice(0, scaleDigits);
  const parsed = BigInt(wholePart) * scaleBigInt + BigInt(fraction || '0');
  if (!allowZero && parsed === 0n) throw new Error(`${fieldName} must be greater than zero.`);
  return parsed;
}

export function ceilDiv(numerator, denominator) {
  if (denominator <= 0n) throw new Error('denominator must be positive');
  if (numerator <= 0n) return 0n;
  return (numerator + denominator - 1n) / denominator;
}

function divideRoundedTowardZero(value, denominator) {
  return value / denominator;
}

function nonNegativeBigInt(value, fieldName) {
  const parsed = typeof value === 'bigint' ? value : BigInt(value ?? 0);
  if (parsed < 0n) throw new Error(`${fieldName} must be non-negative.`);
  return parsed;
}

function emptyProjectionBucket(index) {
  return {
    key: `week-${index + 1}`,
    label: `W${index + 1}`,
    startMs: index,
    projectedTopupCycles: 0n,
    projectedTopupE8s: 0n,
    projectedBurnCycles: 0n,
    projectedBalanceCycles: 0n,
  };
}

function parseApyBasisPoints(raw) {
  if (raw.annualApyPercent !== undefined) {
    const tenthsOfPercent = parseDecimalToScaledBigInt(raw.annualApyPercent, 10n, {
      allowZero: true,
      maxFractionDigits: 1,
      fieldName: 'APY',
    });
    return tenthsOfPercent * 10n;
  }

  if (raw.annualBaseMaturityBasisPoints !== undefined) {
    return BigInt(raw.annualBaseMaturityBasisPoints);
  }

  return DEFAULT_ANNUAL_APY_BASIS_POINTS;
}

export function calculateAgeBonusBasisPointsFromAgeSeconds(ageSeconds) {
  const age = nonNegativeBigInt(ageSeconds, 'Neuron age');
  const clampedAge = age > AGE_BONUS_FULL_AGE_SECONDS ? AGE_BONUS_FULL_AGE_SECONDS : age;
  return (MAX_AGE_BONUS_BASIS_POINTS * clampedAge) / AGE_BONUS_FULL_AGE_SECONDS;
}

export function calculateAgeBonusBasisPointsFromAgingSince(agingSinceTimestampSeconds, nowTimestampSeconds) {
  if (agingSinceTimestampSeconds === null || agingSinceTimestampSeconds === undefined || !nowTimestampSeconds) return null;
  const agingSince = BigInt(agingSinceTimestampSeconds);
  const now = BigInt(nowTimestampSeconds);
  if (now <= agingSince) return 0n;
  return calculateAgeBonusBasisPointsFromAgeSeconds(now - agingSince);
}

export function calculateAgeBonusMaturityShareBasisPoints(ageBonusBasisPoints) {
  const ageBonus = nonNegativeBigInt(ageBonusBasisPoints, 'Age bonus');
  if (ageBonus === 0n) return 0n;
  return (ageBonus * 10_000n) / (10_000n + ageBonus);
}

export function calculateEffectiveApyBasisPoints(annualApyBasisPoints, ageBonusBasisPoints = 0n) {
  const apy = nonNegativeBigInt(annualApyBasisPoints, 'APY');
  const ageBonus = nonNegativeBigInt(ageBonusBasisPoints, 'Age bonus');
  if (apy === 0n || ageBonus === 0n) return apy;
  return (apy * 10_000n) / (10_000n + ageBonus);
}

export function normaliseSimulatorInputs(raw = {}) {
  const errors = [];
  let assumedIcpPriceUnits = 0n;
  let dailyBurnCycles = 0n;
  let icpCommitmentE8s = 0n;
  let annualApyBasisPoints = DEFAULT_ANNUAL_APY_BASIS_POINTS;
  let ageBonusBasisPoints = 0n;

  try {
    assumedIcpPriceUnits = parseDecimalToScaledBigInt(raw.assumedIcpPrice, 10_000n, {
      allowZero: false,
      maxFractionDigits: 1,
      fieldName: 'Assumed ICP/XDR price',
    });
  } catch (error) {
    errors.push(error.message);
  }

  try {
    dailyBurnCycles = parseDecimalToScaledBigInt(raw.dailyBurnTrillionCycles ?? raw.dailyBurnCycles, TRILLION_CYCLES, {
      allowZero: true,
      maxFractionDigits: 3,
      fieldName: 'Daily burn in T cycles',
    });
  } catch (error) {
    errors.push(error.message);
  }

  try {
    icpCommitmentE8s = parseDecimalToScaledBigInt(raw.icpCommitment, E8S_PER_ICP, {
      allowZero: false,
      maxFractionDigits: 1,
      fieldName: 'ICP commitment',
    });
    if (icpCommitmentE8s < E8S_PER_ICP) {
      errors.push('ICP commitment must be at least 1 ICP.');
    }
  } catch (error) {
    errors.push(error.message);
  }

  try {
    annualApyBasisPoints = parseApyBasisPoints(raw);
    if (annualApyBasisPoints < 0n) {
      errors.push('APY must be non-negative.');
    }
  } catch (error) {
    errors.push(error.message);
  }

  try {
    ageBonusBasisPoints = raw.ageBonusBasisPoints === undefined ? 0n : BigInt(raw.ageBonusBasisPoints);
    if (ageBonusBasisPoints < 0n) {
      errors.push('Age bonus must be non-negative.');
    }
  } catch (error) {
    errors.push(error.message);
  }

  if (errors.length > 0) return { ok: false, errors };

  const ageBonusMaturityShareBasisPoints = calculateAgeBonusMaturityShareBasisPoints(ageBonusBasisPoints);
  const effectiveApyBasisPoints = calculateEffectiveApyBasisPoints(annualApyBasisPoints, ageBonusBasisPoints);
  return {
    ok: true,
    value: {
      assumedIcpPriceUnits,
      dailyBurnCycles,
      icpCommitmentE8s,
      annualApyBasisPoints,
      ageBonusBasisPoints,
      ageBonusMaturityShareBasisPoints,
      effectiveApyBasisPoints,
    },
  };
}

export function buildSimulatorProjection(raw = {}) {
  const normalised = normaliseSimulatorInputs(raw);
  if (!normalised.ok) return normalised;

  const {
    assumedIcpPriceUnits,
    dailyBurnCycles,
    icpCommitmentE8s,
    annualApyBasisPoints,
    ageBonusBasisPoints,
    ageBonusMaturityShareBasisPoints,
    effectiveApyBasisPoints,
  } = normalised.value;

  const cyclesPerIcp = (CYCLES_PER_PRICE_UNIT * assumedIcpPriceUnits) / 10_000n;
  const annualTopupE8s = (icpCommitmentE8s * effectiveApyBasisPoints) / 10_000n;
  const annualTopupCycles = (annualTopupE8s * cyclesPerIcp) / E8S_PER_ICP;
  const annualBurnCycles = dailyBurnCycles * 365n;
  const requiredCommitmentE8s = (() => {
    if (annualBurnCycles === 0n) return E8S_PER_ICP;
    if (cyclesPerIcp <= 0n || effectiveApyBasisPoints <= 0n) return null;
    const requiredTopupE8s = ceilDiv(annualBurnCycles * E8S_PER_ICP, cyclesPerIcp);
    const requiredCommitment = ceilDiv(requiredTopupE8s * 10_000n, effectiveApyBasisPoints);
    return requiredCommitment < E8S_PER_ICP ? E8S_PER_ICP : requiredCommitment;
  })();

  const projectionWeeks = BigInt(PROJECTION_WEEKS);
  const weeklyTopupCycles = divideRoundedTowardZero(annualTopupCycles, projectionWeeks);
  const weeklyTopupE8s = divideRoundedTowardZero(annualTopupE8s, projectionWeeks);
  const weeklyBurnCycles = divideRoundedTowardZero(annualBurnCycles, projectionWeeks);
  const initialPayoutCycles = weeklyTopupCycles;

  const buckets = Array.from({ length: PROJECTION_WEEKS }, (_, index) => {
    const bucket = emptyProjectionBucket(index);
    const weekNumber = BigInt(index + 1);
    const previousWeek = BigInt(index);
    const cumulativeTopupCycles = (annualTopupCycles * weekNumber) / projectionWeeks;
    const cumulativeTopupE8s = (annualTopupE8s * weekNumber) / projectionWeeks;
    const cumulativeBurnCycles = (annualBurnCycles * weekNumber) / projectionWeeks;
    const previousTopupCycles = (annualTopupCycles * previousWeek) / projectionWeeks;
    const previousTopupE8s = (annualTopupE8s * previousWeek) / projectionWeeks;
    const previousBurnCycles = (annualBurnCycles * previousWeek) / projectionWeeks;
    bucket.projectedTopupCycles = cumulativeTopupCycles - previousTopupCycles;
    bucket.projectedTopupE8s = cumulativeTopupE8s - previousTopupE8s;
    bucket.projectedBurnCycles = cumulativeBurnCycles - previousBurnCycles;
    bucket.projectedBalanceCycles = initialPayoutCycles + cumulativeTopupCycles - cumulativeBurnCycles;
    return bucket;
  });

  const yearEndBalanceCycles = initialPayoutCycles + annualTopupCycles - annualBurnCycles;

  return {
    ok: true,
    inputs: normalised.value,
    buckets,
    summary: {
      cyclesPerIcp,
      annualApyBasisPoints,
      ageBonusBasisPoints,
      ageBonusMaturityShareBasisPoints,
      effectiveApyBasisPoints,
      annualTopupE8s,
      annualTopupCycles,
      annualBurnCycles,
      weeklyTopupE8s,
      weeklyTopupCycles,
      weeklyBurnCycles,
      initialPayoutCycles,
      yearEndBalanceCycles,
      requiredCommitmentE8s,
      isSustainableAtCurrentBurn: annualTopupCycles >= annualBurnCycles,
    },
  };
}
