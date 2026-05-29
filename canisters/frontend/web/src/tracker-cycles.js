const NANOS_PER_DAY = 86_400_000_000_000n;

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

export function estimateCyclesBurnedPerDay(data) {
  const samples = cycleSamplesForBurnEstimate(data);
  if (samples.length < 2) return null;

  const oldest = samples[0];
  const newest = samples[samples.length - 1];
  const elapsed = newest.timestampNanos - oldest.timestampNanos;
  if (elapsed <= 0n) return null;
  const burned = oldest.cycles > newest.cycles ? oldest.cycles - newest.cycles : 0n;
  return (burned * NANOS_PER_DAY) / elapsed;
}
