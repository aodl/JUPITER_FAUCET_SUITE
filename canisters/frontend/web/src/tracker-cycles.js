const NANOS_PER_DAY = 86_400_000_000_000n;
const E8S_PER_ICP = 100_000_000n;
const CYCLES_PER_PRICE_UNIT = 1_000_000_000_000n;

export function cyclesFromLogText(text) {
  const match = String(text || '').match(/\bCycles:\s*([0-9][0-9_,]*)\b/);
  if (!match) return null;
  return BigInt(match[1].replace(/[,_]/g, ''));
}

function sortedHistorianCycleSamples(data) {
  return (data?.cycles?.items || [])
    .filter((item) => item?.timestamp_nanos !== undefined && item?.timestamp_nanos !== null && item?.cycles !== undefined && item?.cycles !== null)
    .map((item) => ({
      timestampNanos: typeof item.timestamp_nanos === 'bigint' ? item.timestamp_nanos : BigInt(item.timestamp_nanos),
      cycles: typeof item.cycles === 'bigint' ? item.cycles : BigInt(item.cycles),
      source: 'probe',
    }))
    .sort((left, right) => left.timestampNanos < right.timestampNanos ? -1 : left.timestampNanos > right.timestampNanos ? 1 : 0);
}

export function sortedLogCycleSamples(data) {
  return (data?.logs?.items || [])
    .map((item) => {
      const cycles = cyclesFromLogText(item?.text);
      if (cycles === null || item?.timestamp_nanos === undefined || item?.timestamp_nanos === null) return null;
      return {
        timestampNanos: typeof item.timestamp_nanos === 'bigint' ? item.timestamp_nanos : BigInt(item.timestamp_nanos),
        cycles,
        source: 'log',
      };
    })
    .filter(Boolean)
    .sort((left, right) => left.timestampNanos < right.timestampNanos ? -1 : left.timestampNanos > right.timestampNanos ? 1 : 0);
}

export function sortedCycleSamples(data) {
  const probeSamples = sortedHistorianCycleSamples(data);
  return probeSamples.length > 0 ? probeSamples : sortedLogCycleSamples(data);
}

export function cycleSamplesForBurnEstimate(data) {
  const primarySamples = sortedCycleSamples(data);
  if (primarySamples.length >= 2) return primarySamples;
  const fallbackSamples = primarySamples[0]?.source === 'probe'
    ? sortedLogCycleSamples(data)
    : sortedHistorianCycleSamples(data);
  return fallbackSamples.length >= 2 ? fallbackSamples : primarySamples;
}

function optValue(value) {
  return Array.isArray(value) ? value[0] : value;
}

function amountE8s(item) {
  const value = item?.amount_e8s;
  if (value === undefined || value === null) return null;
  return typeof value === 'bigint' ? value : BigInt(value);
}

function timestampNanos(item) {
  const value = optValue(item?.timestamp_nanos);
  if (value === undefined || value === null) return null;
  return typeof value === 'bigint' ? value : BigInt(value);
}

function cyclesPerIcpFromStatus(status) {
  const snapshot = optValue(status?.icp_xdr_rate);
  if (!snapshot?.rate || snapshot.decimals === undefined || snapshot.decimals === null) return null;
  const rate = typeof snapshot.rate === 'bigint' ? snapshot.rate : BigInt(snapshot.rate);
  const decimals = typeof snapshot.decimals === 'bigint' ? snapshot.decimals : BigInt(snapshot.decimals);
  if (rate <= 0n || decimals < 0n) return null;
  return (CYCLES_PER_PRICE_UNIT * rate) / (10n ** decimals);
}

function estimatedCmcTopUpCyclesBetween(data, startNanos, endNanos) {
  const cyclesPerIcp = cyclesPerIcpFromStatus(data?.status);
  if (!cyclesPerIcp) return null;
  let totalE8s = 0n;
  for (const item of data?.cmcTransfers?.items || []) {
    const timestamp = timestampNanos(item);
    if (timestamp === null || timestamp <= startNanos || timestamp > endNanos) continue;
    const amount = amountE8s(item);
    if (amount === null || amount <= 0n) continue;
    totalE8s += amount;
  }
  if (totalE8s <= 0n) return null;
  return (totalE8s * cyclesPerIcp) / E8S_PER_ICP;
}

function observedBalanceDropCycles(samples) {
  let burned = 0n;
  for (let index = 1; index < samples.length; index += 1) {
    const previous = samples[index - 1];
    const current = samples[index];
    if (previous.cycles > current.cycles) {
      burned += previous.cycles - current.cycles;
    }
  }
  return burned;
}

export function estimateCyclesBurnedPerDay(data) {
  const samples = cycleSamplesForBurnEstimate(data);
  if (samples.length < 2) return null;

  const oldest = samples[0];
  const newest = samples[samples.length - 1];
  const elapsed = newest.timestampNanos - oldest.timestampNanos;
  if (elapsed <= 0n) return null;
  const estimatedTopUpCycles = estimatedCmcTopUpCyclesBetween(data, oldest.timestampNanos, newest.timestampNanos);
  const topUpAdjustedBurn = estimatedTopUpCycles === null ? null : oldest.cycles + estimatedTopUpCycles - newest.cycles;
  const burned = topUpAdjustedBurn !== null && topUpAdjustedBurn > 0n
    ? topUpAdjustedBurn
    : observedBalanceDropCycles(samples);
  return (burned * NANOS_PER_DAY) / elapsed;
}
