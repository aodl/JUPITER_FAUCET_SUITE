import { Principal } from '@icp-sdk/core/principal';
import {
  normalizeError,
  loadTrackerData,
  loadRawIcpCanisterTrackerData,
  loadNeuronStakeTrackerData,
  RAW_ICP_TRACKER_TRANSFER_LIMIT,
} from '../dashboard-data.js';
import { readOpt } from '../candid-opt.js';
import { escapeHtml } from '../followee-links.js';
import { renderAmountBarChart, renderEmptyChart, renderLineChart, renderStackedAmountBarChart } from '../chart-rendering.js';
import { cycleSamplesForBurnEstimate, estimateCyclesBurnedPerDay, sortedCycleSamples } from '../tracker-cycles.js';
import { parseJupiterMemo } from '../memo-policy.js';
import { classifyTransferItem, relayRegistrySourceMap } from '../data/transfer-source-classification.js';
import { trackerHashForMemo, trackerHashForPrincipal, trackerStateFromHash } from './hash-routes.js';
import { formatDailyBurnInputFromCyclesPerDay, formatIcpCommitmentInputRoundedUp } from './simulator-controller.js';
import {
  DASH,
  formatCycles,
  formatDurationSeconds,
  formatIcpE8s,
  formatInteger,
  formatTimestampNanos,
  formatTimestampSeconds,
  formatTrillionCycles,
  renderCanisterDashboardLink,
  renderCanisterTrackerLink,
  renderNeuronDashboardLink,
} from './view-formatters.js';

const TRACKER_REGISTRATION_URL = '#how-it-works';
const TRACKER_RANGE_LABELS = {
  month: 'last month',
  year: 'last year',
  all: 'all currently loaded history',
};
const TRACKER_DEFAULT_RANGE = 'month';
const SOURCE_SEGMENTS = [
  { key: 'faucetAmountE8s', countKey: 'faucetTransferCount', className: 'tracker-chart-bar--source-faucet', label: 'Jupiter Faucet', legendKey: 'faucet' },
  { key: 'relayAmountE8s', countKey: 'relayTransferCount', className: 'tracker-chart-bar--source-relay', label: 'Jupiter Relay', legendKey: 'relay' },
  { key: 'protocolAmountE8s', countKey: 'protocolTransferCount', className: 'tracker-chart-bar--source-protocol', label: 'Protocol canister', legendKey: 'protocol' },
  { key: 'otherAmountE8s', countKey: 'otherTransferCount', className: 'tracker-chart-bar--source-other', label: 'Other', legendKey: 'other' },
];
const RAW_SOURCE_SEGMENTS = [
  { key: 'faucetMatchingMemoAmountE8s', countKey: 'faucetMatchingMemoTransferCount', className: 'tracker-chart-bar--source-faucet-matching-memo', label: 'Jupiter Faucet · matching memo', legendKey: 'faucet-matching-memo' },
  { key: 'faucetOtherMemoAmountE8s', countKey: 'faucetOtherMemoTransferCount', className: 'tracker-chart-bar--source-faucet-other-memo', label: 'Jupiter Faucet · other memo', legendKey: 'faucet-other-memo' },
  { key: 'relayAmountE8s', countKey: 'relayTransferCount', className: 'tracker-chart-bar--source-relay', label: 'Jupiter Relay', legendKey: 'relay' },
  { key: 'protocolAmountE8s', countKey: 'protocolTransferCount', className: 'tracker-chart-bar--source-protocol', label: 'Protocol canister', legendKey: 'protocol' },
  { key: 'otherAmountE8s', countKey: 'otherTransferCount', className: 'tracker-chart-bar--source-other', label: 'Other', legendKey: 'other' },
];

function optValue(value) {
  return readOpt(value);
}

function timestampNanosToDate(value) {
  if (value === null || value === undefined) return null;
  const nanos = typeof value === 'bigint' ? value : BigInt(value);
  const millis = nanos / 1_000_000n;
  const asNumber = Number(millis);
  if (!Number.isFinite(asNumber)) return null;
  return new Date(asNumber);
}

function trackingReasonNames(trackingReasons) {
  if (!Array.isArray(trackingReasons)) return [];
  return trackingReasons
    .map((reason) => reason && typeof reason === 'object' && !Array.isArray(reason) ? Object.keys(reason)[0] : '')
    .filter(Boolean);
}

function variantNameFromValue(value) {
  if (!value || Array.isArray(value) || typeof value !== 'object') return '';
  return Object.keys(value)[0] || '';
}

function itemAmountE8s(item) {
  return typeof item?.amount_e8s === 'bigint' ? item.amount_e8s : BigInt(item?.amount_e8s || 0);
}

function renderCyclesHelpIcon() {
  return `
    <button
      class="pane-inline-tooltip-icon"
      type="button"
      data-tooltip-id="blackhole-controller-help"
      aria-label="Cycles observability help"
    >i</button>`;
}

function renderCyclesStatusCell(label = 'unavailable') {
  return `
    <span class="pane-inline-tooltip-fallback">
      <span>${escapeHtml(label)}</span>
      ${renderCyclesHelpIcon()}
    </span>`;
}

function cyclesProbeStatusInfo(data) {
  const meta = data?.overview?.meta || {};
  const probeResult = optValue(meta.last_cycles_probe_result);
  const probeTs = optValue(meta.last_cycles_probe_ts);

  if (!probeTs && !probeResult) {
    return {
      label: 'pending first sweep',
      chartMessage: 'Cycles data pending first historian sweep.',
      note: 'Cycles data is pending: historian has registered this canister, but the first cycles sweep has not recorded a balance yet.',
    };
  }

  const resultName = variantNameFromValue(probeResult);
  if (resultName === 'Error') {
    const errorText = typeof probeResult.Error === 'string' ? probeResult.Error : 'unknown error';
    const when = formatTimestampSeconds(probeTs);
    return {
      kind: 'error',
      label: 'probe failed',
      chartMessage: 'Cycles probe failed.',
      note: `Cycles data is unavailable because the last historian cycles probe failed${when !== DASH ? ` at ${when}` : ''}: ${errorText}`,
    };
  }

  if (resultName === 'NotAvailable') {
    const when = formatTimestampSeconds(probeTs);
    return {
      kind: 'notAvailable',
      label: 'not available',
      chartMessage: 'Cycles data not available.',
      note: `Cycles data is unavailable because historian could not obtain a balance${when !== DASH ? ` during the last probe at ${when}` : ''}. Historian probes supported direct self, recognized blackhole, and SNS routes automatically.`,
    };
  }

  const when = formatTimestampSeconds(probeTs);
  return {
    kind: resultName === 'Ok' ? 'ok' : 'pending',
    label: 'pending update',
    chartMessage: 'Cycles data pending update.',
    note: `Cycles data has not been recorded for this tracker view yet${when !== DASH ? `; the last probe was at ${when}` : ''}.`,
  };
}

function trackerRangeCutoffMs(range, anchorMs = Date.now()) {
  if (range === 'all') return null;
  const dayMs = 24 * 60 * 60 * 1000;
  return anchorMs - (range === 'year' ? 365 : 30) * dayMs;
}

function trackerItemTimestampMs(item) {
  const timestamp = optValue(item?.timestamp_nanos);
  const date = timestampNanosToDate(timestamp);
  return date ? date.getTime() : null;
}

function trackerAllDatedItems(data) {
  return [
    ...(data?.commitments?.items || []),
    ...(data?.cycles?.items || []),
    ...(data?.cmcTransfers?.items || []),
    ...(data?.logs?.items || []),
  ];
}

function rawTransferTimestampMs(item) {
  const timestamp = optValue(item?.timestamp_nanos);
  const date = timestampNanosToDate(timestamp);
  return date ? date.getTime() : null;
}

function latestTrackerTimestampMs(data) {
  return trackerAllDatedItems(data).reduce((latest, item) => {
    const timestampMs = trackerItemTimestampMs(item);
    return timestampMs !== null && timestampMs > latest ? timestampMs : latest;
  }, 0);
}

function trackerHasAnyDatedItems(data) {
  return trackerAllDatedItems(data).some((item) => trackerItemTimestampMs(item) !== null);
}

function filterTrackerPageAfterCutoff(page, cutoffMs) {
  const items = page?.items || [];
  if (cutoffMs === null) return { ...(page || { items: [] }), items };
  return {
    ...(page || { items: [] }),
    items: items.filter((item) => {
      const timestampMs = trackerItemTimestampMs(item);
      return timestampMs !== null && timestampMs >= cutoffMs;
    }),
  };
}

function filterTrackerDataByRange(data, range) {
  if (!data || range === 'all') return data;
  const anchorMs = latestTrackerTimestampMs(data) || Date.now();
  const cutoffMs = trackerRangeCutoffMs(range, anchorMs);
  return {
    ...data,
    commitments: filterTrackerPageAfterCutoff(data.commitments, cutoffMs),
    cycles: filterTrackerPageAfterCutoff(data.cycles, cutoffMs),
    logs: filterTrackerPageAfterCutoff(data.logs, cutoffMs),
    cmcTransfers: filterTrackerPageAfterCutoff(data.cmcTransfers, cutoffMs),
  };
}

function trackerPeriod(date, range) {
  const year = date.getUTCFullYear();
  const month = date.getUTCMonth();
  const day = date.getUTCDate();
  return {
    key: `${year}-${String(month + 1).padStart(2, '0')}-${String(day).padStart(2, '0')}`,
    label: `${date.toLocaleString('en-GB', { day: '2-digit', month: 'short', year: range === 'month' ? undefined : '2-digit', timeZone: 'UTC' })} UTC`,
    startMs: Date.UTC(year, month, day),
    endMs: Date.UTC(year, month, day + 1),
  };
}

function nextTrackerPeriod(period, range) {
  const date = new Date(period.startMs);
  return trackerPeriod(new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() + 1)), range);
}

function trackerTimelineBounds(data, range) {
  const timestampMs = trackerAllDatedItems(data)
    .map(trackerItemTimestampMs)
    .filter((value) => value !== null);
  if (timestampMs.length === 0) return null;
  const maxMs = timestampMs.reduce((max, value) => value > max ? value : max, timestampMs[0]);
  const minMs = range === 'all'
    ? timestampMs.reduce((min, value) => value < min ? value : min, timestampMs[0])
    : trackerRangeCutoffMs(range, maxMs);
  if (minMs === null) return null;
  return {
    start: trackerPeriod(new Date(minMs), range),
    end: trackerPeriod(new Date(maxMs), range),
  };
}

function emptyTrackerBucket(period) {
  return {
    ...period,
    commitmentAmountE8s: 0n,
    qualifyingCommitmentAmountE8s: 0n,
    commitmentCount: 0,
    qualifyingCommitmentCount: 0,
    observedCmcAmountE8s: 0n,
    observedCmcTransferCount: 0,
    faucetAmountE8s: 0n,
    faucetTransferCount: 0,
    relayAmountE8s: 0n,
    relayTransferCount: 0,
    protocolAmountE8s: 0n,
    protocolTransferCount: 0,
    otherAmountE8s: 0n,
    otherTransferCount: 0,
    cycles: null,
    cyclesTs: null,
  };
}

function addSourceAmount(bucket, category, amount) {
  const prefix = category === 'faucet' || category === 'relay' || category === 'protocol' ? category : 'other';
  bucket[`${prefix}AmountE8s`] += amount;
  bucket[`${prefix}TransferCount`] += 1;
}

function aggregateTrackerData(data, range) {
  const buckets = new Map();
  const ensureBucket = (period) => {
    const existing = buckets.get(period.key);
    if (existing) return existing;
    const created = emptyTrackerBucket(period);
    buckets.set(period.key, created);
    return created;
  };

  for (const item of data?.commitments?.items || []) {
    const timestamp = optValue(item.timestamp_nanos);
    const date = timestampNanosToDate(timestamp);
    if (!date) continue;
    const bucket = ensureBucket(trackerPeriod(date, range));
    const amount = itemAmountE8s(item);
    bucket.commitmentAmountE8s += amount;
    bucket.commitmentCount += 1;
    if (item.counts_toward_faucet) {
      bucket.qualifyingCommitmentAmountE8s += amount;
      bucket.qualifyingCommitmentCount += 1;
    }
  }

  for (const item of data?.cmcTransfers?.items || []) {
    const timestamp = optValue(item.timestamp_nanos);
    const date = timestampNanosToDate(timestamp);
    if (!date) continue;
    const bucket = ensureBucket(trackerPeriod(date, range));
    bucket.observedCmcAmountE8s += itemAmountE8s(item);
    bucket.observedCmcTransferCount += 1;
    addSourceAmount(bucket, item.source_category, itemAmountE8s(item));
  }

  for (const item of data?.cycles?.items || []) {
    const date = timestampNanosToDate(item?.timestamp_nanos);
    if (!date || item?.cycles === undefined || item?.cycles === null) continue;
    const bucket = ensureBucket(trackerPeriod(date, range));
    const timestamp = typeof item.timestamp_nanos === 'bigint' ? item.timestamp_nanos : BigInt(item.timestamp_nanos);
    if (bucket.cyclesTs === null || timestamp >= bucket.cyclesTs) {
      bucket.cycles = typeof item.cycles === 'bigint' ? item.cycles : BigInt(item.cycles);
      bucket.cyclesTs = timestamp;
    }
  }

  const bounds = trackerTimelineBounds(data, range);
  if (bounds) {
    for (let period = bounds.start; period.startMs <= bounds.end.startMs; period = nextTrackerPeriod(period, range)) {
      ensureBucket(period);
    }
  }

  return Array.from(buckets.values()).sort((left, right) => left.startMs - right.startMs);
}

function rawTimelineBounds(items, range) {
  const timestamps = (items || []).map(rawTransferTimestampMs).filter((value) => value !== null);
  if (timestamps.length === 0) return null;
  const maxMs = Math.max(...timestamps);
  const minMs = range === 'all' ? Math.min(...timestamps) : trackerRangeCutoffMs(range, maxMs);
  if (minMs === null) return null;
  return { start: trackerPeriod(new Date(minMs), range), end: trackerPeriod(new Date(maxMs), range) };
}

function emptyRawBucket(period) {
  return {
    ...period,
    totalIcpE8s: 0n,
    totalTransferCount: 0,
    faucetAmountE8s: 0n,
    faucetTransferCount: 0,
    faucetMatchingMemoAmountE8s: 0n,
    faucetMatchingMemoTransferCount: 0,
    faucetOtherMemoAmountE8s: 0n,
    faucetOtherMemoTransferCount: 0,
    relayAmountE8s: 0n,
    relayTransferCount: 0,
    protocolAmountE8s: 0n,
    protocolTransferCount: 0,
    otherAmountE8s: 0n,
    otherTransferCount: 0,
  };
}

function rawTransferSourceSegment(item) {
  if (item?.source_category === 'faucet' && item?.is_matching_memo) return 'faucetMatchingMemo';
  if (item?.source_category === 'faucet') return 'faucetOtherMemo';
  if (item?.source_category === 'relay') return 'relay';
  if (item?.source_category === 'protocol') return 'protocol';
  return 'other';
}

function addRawSourceAmount(bucket, item, amount) {
  if (item?.source_category === 'faucet') {
    bucket.faucetAmountE8s += amount;
    bucket.faucetTransferCount += 1;
  }
  const prefix = rawTransferSourceSegment(item);
  bucket[`${prefix}AmountE8s`] += amount;
  bucket[`${prefix}TransferCount`] += 1;
}

function aggregateRawTransfers(items, range) {
  const buckets = new Map();
  const ensureBucket = (period) => {
    const existing = buckets.get(period.key);
    if (existing) return existing;
    const created = emptyRawBucket(period);
    buckets.set(period.key, created);
    return created;
  };
  for (const item of items || []) {
    const date = timestampNanosToDate(optValue(item.timestamp_nanos));
    if (!date) continue;
    const bucket = ensureBucket(trackerPeriod(date, range));
    const amount = itemAmountE8s(item);
    bucket.totalIcpE8s += amount;
    bucket.totalTransferCount += 1;
    addRawSourceAmount(bucket, item, amount);
  }
  const bounds = rawTimelineBounds(items, range);
  if (bounds) {
    for (let period = bounds.start; period.startMs <= bounds.end.startMs; period = nextTrackerPeriod(period, range)) {
      ensureBucket(period);
    }
  }
  return Array.from(buckets.values()).sort((left, right) => left.startMs - right.startMs);
}

function cycleSampleSourceLabel(samples) {
  return samples[0]?.source === 'log' ? 'canister logs' : 'historian cycle probes';
}

function formatTrillionCyclesPerDay(value) {
  if (value === null || value === undefined) return DASH;
  return `${formatTrillionCycles(value).replace(' cycles', '')} cycles/day`;
}

function sumE8s(items) {
  return (items || []).reduce((sum, item) => sum + itemAmountE8s(item), 0n);
}

function trackerMetricSummary(data) {
  const commitmentItems = data?.commitments?.items || [];
  const qualifyingCommitments = commitmentItems.filter((item) => item.counts_toward_faucet);
  const transferItems = data?.cmcTransfers?.items || [];
  const cyclesSamples = sortedCycleSamples(data);
  const latestCycles = cyclesSamples.length ? cyclesSamples[cyclesSamples.length - 1]?.cycles : null;
  return {
    commitmentCount: commitmentItems.length,
    qualifyingCommitmentCount: qualifyingCommitments.length,
    totalCommittedE8s: sumE8s(commitmentItems),
    qualifyingCommittedE8s: sumE8s(qualifyingCommitments),
    observedCmcTransferCount: transferItems.length,
    observedCmcE8s: sumE8s(transferItems),
    latestCycles,
  };
}

function classifyTrackerData(data, protocolCanisterId = null) {
  if (!data) return data;
  const relaySourceMap = relayRegistrySourceMap(data.relayRegistrations?.items || []);
  return {
    ...data,
    cmcTransfers: {
      ...(data.cmcTransfers || { items: [] }),
      items: (data.cmcTransfers?.items || []).map((item) => classifyTransferItem(item, {
        status: data.status,
        protocolCanisterId,
        relaySourceMap,
      })),
    },
  };
}

function rawSummary(items) {
  return (items || []).reduce((summary, item) => {
    const amount = itemAmountE8s(item);
    summary.totalTransferCount += 1;
    summary.totalIcpE8s += amount;
    if (item.source_category === 'faucet') {
      summary.faucetTransferCount += 1;
      summary.faucetIcpE8s += amount;
      if (item.is_matching_memo) {
        summary.faucetMatchingMemoTransferCount += 1;
        summary.faucetMatchingMemoIcpE8s += amount;
      }
    }
    return summary;
  }, {
    totalTransferCount: 0,
    totalIcpE8s: 0n,
    faucetTransferCount: 0,
    faucetIcpE8s: 0n,
    faucetMatchingMemoTransferCount: 0,
    faucetMatchingMemoIcpE8s: 0n,
  });
}

function renderTrackerLogs(data) {
  const logs = data?.logs?.items || [];
  const logsError = data?.errors?.logs;
  const body = logs.length === 0
    ? `<p class="tracker-log-empty">${escapeHtml(logsError ? `Canister logs unavailable: ${logsError}` : 'No canister logs are currently available for this principal.')}</p>`
    : `<pre class="tracker-log-output">${escapeHtml(logs.map((item) => {
        const idx = item?.idx === undefined || item?.idx === null ? '?' : item.idx.toString();
        const timestamp = formatTimestampNanos(item?.timestamp_nanos);
        return `[${idx}. ${timestamp}]: ${String(item?.text || '').trim()}`;
      }).join('\n'))}</pre>`;
  return `
    <details class="tracker-log-details">
      <summary>Canister logs</summary>
      ${body}
    </details>`;
}

function renderSimulatorPrefillLink({ dailyBurnCycles, commitmentE8s, simulatorHashForPrefill }) {
  const dailyBurnInput = formatDailyBurnInputFromCyclesPerDay(dailyBurnCycles);
  const commitmentInput = formatIcpCommitmentInputRoundedUp(commitmentE8s);
  if (!dailyBurnInput || !commitmentInput) return DASH;
  return `<a href="${escapeHtml(simulatorHashForPrefill({ dailyBurn: dailyBurnInput, icpCommitment: commitmentInput }))}" class="pane-external-link" data-simulator-prefill="true">Open simulator</a>`;
}

function renderCyclesProbeInfoNote(cyclesStatus, usingLogCycles) {
  if (cyclesStatus.kind !== 'error' && cyclesStatus.kind !== 'notAvailable') return '';
  const label = usingLogCycles
    ? 'Historian cycle probe unavailable; using canister log cycles.'
    : cyclesStatus.kind === 'error'
      ? 'Historian cycle probe failed.'
      : 'Historian cycle probe unavailable.';
  return `<p class="pane-status-note tracker-status-note tracker-status-note--info" title="${escapeHtml(cyclesStatus.note)}">${escapeHtml(label)}</p>`;
}

function pluralize(count, singular, plural = `${singular}s`) {
  return `${formatInteger(count)} ${count === 1 ? singular : plural}`;
}

function trackerRangeRank(range) {
  if (range === 'all') return 3;
  if (range === 'year') return 2;
  return 1;
}

function trackerRangeCutoffNanos(range, anchorMs = Date.now()) {
  const cutoffMs = trackerRangeCutoffMs(range, anchorMs);
  return cutoffMs === null ? null : BigInt(cutoffMs) * 1_000_000n;
}

export function createTrackerController({
  frontendConfig,
  isLocalHost,
  loadData = loadTrackerData,
  loadRawCanisterData = loadRawIcpCanisterTrackerData,
  loadNeuronData = loadNeuronStakeTrackerData,
  normalizeLoadError = normalizeError,
  simulatorHashForPrefill,
  onSimulatorPrefillHash = () => {},
}) {
  const state = {
    memoText: '',
    principalText: '',
    parsedMemo: null,
    protocolCanisterText: '',
    protocolCanisterId: null,
    viewMode: '',
    data: null,
    range: TRACKER_DEFAULT_RANGE,
    loadedRange: null,
    loading: false,
    error: null,
  };
  let lastHashSubmitMemo = '';

  const trackerRangeLabel = (range = state.range) => TRACKER_RANGE_LABELS[range] || TRACKER_RANGE_LABELS.month;

  const rawTransfersLoading = () => (state.viewMode === 'rawIcpCanister' || state.viewMode === 'neuronStake')
    && Boolean(state.data?.transfers?.loading);

  const renderTrackerRangeControls = () => {
    const disabled = rawTransfersLoading();
    const button = (range, label) => {
      const active = state.range === range;
      return `<button class="pane-page-button${active ? ' is-active' : ''}" type="button" data-tracker-range="${range}" aria-pressed="${active ? 'true' : 'false'}"${disabled ? ' disabled aria-disabled="true"' : ''}>${label}</button>`;
    };
    return `
      <div class="tracker-controls tracker-controls--charts" aria-label="Tracker chart range">
        ${button('month', 'Latest Month')}
        ${button('year', 'Latest Year')}
        ${button('all', 'All')}
      </div>`;
  };

  const trackerBucketDescription = () => 'daily buckets';

  const renderTrackerCadenceNote = (data) => {
    const status = data?.status;
    if (!status) {
      return '<p class="pane-status-note tracker-status-note">Cycles balances are sampled by historian cycles sweeps; cadence is unavailable because historian public status could not be loaded.</p>';
    }

    const indexCadence = formatDurationSeconds(status.index_interval_seconds);
    const lastIndex = formatTimestampSeconds(optValue(status.last_index_run_ts));
    const cyclesCadence = formatDurationSeconds(status.cycles_interval_seconds);
    const lastSweep = formatTimestampSeconds(optValue(status.last_completed_cycles_sweep_ts));
    const lastCanisterProbe = formatTimestampSeconds(optValue(data?.overview?.meta?.last_cycles_probe_ts));
    const indexText = indexCadence === DASH
      ? 'Commitment history is updated by historian ledger/index scans.'
      : `Commitment history is updated by historian ledger/index scans about every ${indexCadence}${lastIndex !== DASH ? `; last scan ${lastIndex}` : ''}.`;
    const cyclesText = cyclesCadence === DASH
      ? `Cycles balances are sampled by historian cycles sweeps${lastCanisterProbe !== DASH ? `; this canister was last probed ${lastCanisterProbe}` : ''}.`
      : `Cycles balances are sampled by historian cycles sweeps about every ${cyclesCadence}${lastSweep !== DASH ? `; last completed sweep ${lastSweep}` : ''}${lastCanisterProbe !== DASH ? `; this canister was last probed ${lastCanisterProbe}` : ''}.`;
    return `<p class="pane-status-note tracker-status-note">${escapeHtml(`${indexText} Observed CMC top-ups are queried from the ICP index when this pane loads. ${cyclesText}`)}</p>`;
  };

  const renderTrackerEmptyChart = (message) => renderEmptyChart(message);

  const trackerRangeEmptyMessage = ({ fullItems, rangeMessage, emptyMessage }) => {
    if (state.range !== 'all' && (fullItems || []).length > 0) {
      return `${rangeMessage} Select All to view older loaded history.`;
    }
    return emptyMessage;
  };

  const renderTrackerAmountBarChart = ({ buckets, amountKey, countKey, emptyMessage, ariaLabel, barClass = '', labelBuilder }) => renderAmountBarChart({
    buckets,
    amountKey,
    countKey,
    emptyMessage,
    ariaLabel,
    barClass,
    labelBuilder,
    valueFormatter: formatIcpE8s,
    xAxis: 'time',
    minBarWidth: 2,
    maxBarWidth: 28,
    allowMinBarOverflow: true,
  });

  const renderTrackerCommitmentsChart = (buckets, fullData = null) => renderTrackerAmountBarChart({
    buckets,
    amountKey: 'commitmentAmountE8s',
    countKey: 'commitmentCount',
    barClass: 'tracker-chart-bar--commitment',
    emptyMessage: trackerRangeEmptyMessage({
      fullItems: fullData?.commitments?.items,
      rangeMessage: `No dated commitments are available in ${trackerRangeLabel()}.`,
      emptyMessage: 'No dated commitments are available for this beneficiary yet.',
    }),
    ariaLabel: `ICP commitments in ${trackerRangeLabel()}`,
    labelBuilder: (bucket) => `${bucket.label}: ${formatIcpE8s(bucket.commitmentAmountE8s)} across ${pluralize(bucket.commitmentCount, 'commitment')}; ${formatIcpE8s(bucket.qualifyingCommitmentAmountE8s)} qualifying across ${pluralize(bucket.qualifyingCommitmentCount, 'qualifying commitment')}`,
  });

  const legendSegments = (segments, includeProtocol) => segments
    .filter((segment) => includeProtocol || segment.legendKey !== 'protocol')
    .map((segment) => `
      <span class="tracker-source-legend-item ${escapeHtml(segment.className)}" data-source-segment="${escapeHtml(segment.legendKey || segment.key)}" tabindex="0">
        <i></i>${escapeHtml(segment.label)}
      </span>`)
    .join('');

  const renderSourceLegend = ({ includeProtocol = false, segments = SOURCE_SEGMENTS } = {}) => `
    <div class="tracker-source-legend" aria-label="Transfer source legend">
      ${legendSegments(segments, includeProtocol)}
    </div>`;

  const activeSourceSegments = (segments, buckets) => {
    const rows = Array.isArray(buckets) ? buckets : [];
    return segments.filter((segment) => rows.some((bucket) => {
      const value = bucket?.[segment.key];
      return (typeof value === 'bigint' ? value : BigInt(value || 0)) > 0n;
    }));
  };

  const renderActiveSourceLegend = ({ includeProtocol = false, segments = SOURCE_SEGMENTS, buckets = [] } = {}) => {
    const activeSegments = activeSourceSegments(segments, buckets);
    if (activeSegments.length === 0) return '';
    return renderSourceLegend({ includeProtocol, segments: activeSegments });
  };

  const renderTrackerObservedCmcChart = (buckets, fullData = null) => renderStackedAmountBarChart({
    buckets,
    segments: activeSourceSegments(SOURCE_SEGMENTS, buckets),
    emptyMessage: trackerRangeEmptyMessage({
      fullItems: fullData?.cmcTransfers?.items,
      rangeMessage: `No dated ICP transfers to the canister’s CMC top-up account are available in ${trackerRangeLabel()}.`,
      emptyMessage: 'No dated ICP transfers to the canister’s CMC top-up account are available yet.',
    }),
    ariaLabel: `Observed CMC top-up transfers in ${trackerRangeLabel()}`,
    labelBuilder: (bucket, segment, amount, count) => `${bucket.label}: ${segment.label} ${formatIcpE8s(amount)} across ${pluralize(count, 'observed CMC transfer')}`,
    valueFormatter: formatIcpE8s,
    xAxis: 'time',
    minBarWidth: 2,
    maxBarWidth: 28,
    allowMinBarOverflow: true,
  });

  const trackerCyclesChartPoints = (data) => sortedCycleSamples(data).map((sample) => {
    const millis = Number(sample.timestampNanos / 1_000_000n);
    const date = new Date(millis);
    return {
      label: `${date.toLocaleString('en-GB', { day: '2-digit', month: 'short', year: 'numeric', timeZone: 'UTC' })} UTC`,
      timestampNanos: sample.timestampNanos,
      startMs: millis,
      endMs: millis + 1,
      cycles: sample.cycles,
    };
  });

  const trackerCyclesPointLabel = (point) => `${formatTimestampNanos(point.timestampNanos)}: ${formatCycles(point.cycles)}`;

  const renderTrackerCyclesChart = (data, timelineBuckets, fullData = data) => {
    const points = trackerCyclesChartPoints(data);
    if (points.length === 0) {
      if ((fullData?.cycles?.items || []).length > 0) {
        return renderTrackerEmptyChart(`No cycles samples are available in ${trackerRangeLabel()}.`);
      }
      const status = cyclesProbeStatusInfo(fullData);
      return `<div class="tracker-chart-empty">${escapeHtml(status.chartMessage)} ${renderCyclesHelpIcon()}</div>`;
    }

    return renderLineChart({
      buckets: points,
      valueKey: 'cycles',
      emptyMessage: `No cycles samples are available in ${trackerRangeLabel()}.`,
      ariaLabel: `Cycles balance in ${trackerRangeLabel()}`,
      valueFormatter: formatCycles,
      pointLabelBuilder: trackerCyclesPointLabel,
      xAxis: 'time',
      xDomainBuckets: timelineBuckets,
      xTickBuckets: timelineBuckets,
    });
  };

  const renderTrackerCharts = (data, fullData = data) => {
    const wrapper = document.getElementById('tracker-chart-wrapper');
    if (!wrapper) return;
    const buckets = aggregateTrackerData(data, state.range);
    const hasObservedCmcTopUps = (fullData?.cmcTransfers?.items || []).length > 0;
    if (buckets.length === 0) {
      const hasOlderLoadedData = state.range !== 'all' && trackerHasAnyDatedItems(fullData);
      const message = state.range === 'all'
        ? 'No dated tracker data is available for this canister yet.'
        : `No dated tracker data is available in ${trackerRangeLabel()}.${hasOlderLoadedData ? ' Select All to view older loaded history.' : ''}`;
      wrapper.innerHTML = renderTrackerEmptyChart(message);
      return;
    }

    wrapper.innerHTML = `
      <div class="tracker-chart-card">
        <div class="tracker-chart-header">
          <h3>ICP commitments</h3>
          <span>Memo-registered commitment history from historian.</span>
        </div>
        ${renderTrackerCommitmentsChart(buckets, fullData)}
      </div>
      ${hasObservedCmcTopUps ? `<div class="tracker-chart-card">
        <div class="tracker-chart-header">
          <h3>Observed CMC top-ups</h3>
          <span>Transfers to the CMC deposit account in the selected range, colour-coded by source; direct non-Jupiter top-ups may appear.</span>
        </div>
        ${renderActiveSourceLegend({ includeProtocol: Boolean(state.protocolCanisterText), buckets })}
        ${renderTrackerObservedCmcChart(buckets, fullData)}
      </div>` : ''}
      <div class="tracker-chart-card">
        <div class="tracker-chart-header">
          <h3>Cycles balance</h3>
        </div>
        ${renderTrackerCyclesChart(data, buckets, fullData)}
      </div>`;
  };

  const renderRecognitionMessage = (data, principalText) => {
    const result = document.getElementById('tracker-result');
    if (!result) return;
    const detail = data?.isRecognized
      ? 'Historian recognises this principal, but not as a memo-registered commitment beneficiary.'
      : 'This principal is not a recognised commitment beneficiary.';
    result.innerHTML = `
      <div class="tracker-empty-state">
        <p>${escapeHtml(detail)}</p>
        <p>Register the canister for perpetual top-ups from the <a class="pane-external-link" href="${TRACKER_REGISTRATION_URL}" data-panel="how-it-works">How it works guide</a>.</p>
        <p>${renderCanisterTrackerLink(principalText)}</p>
        <p>${renderCanisterDashboardLink(principalText)}</p>
      </div>`;
  };

  const renderData = (data, principalText) => {
    const result = document.getElementById('tracker-result');
    if (!result) return;
    const classifiedData = classifyTrackerData(data, state.protocolCanisterId);
    if (!classifiedData?.isCommitmentBeneficiary) {
      renderRecognitionMessage(classifiedData, principalText);
      return;
    }

    const visibleData = filterTrackerDataByRange(classifiedData, state.range);
    const summary = trackerMetricSummary(visibleData);
    const fullSummary = trackerMetricSummary(classifiedData);
    const cycleSamples = sortedCycleSamples(classifiedData);
    const rangeLabel = trackerRangeLabel();
    const trackingReasons = trackingReasonNames(classifiedData.overview?.tracking_reasons).join(', ') || DASH;
    const firstSeen = formatTimestampSeconds(optValue(classifiedData.overview?.meta?.first_seen_ts));
    const lastCommitment = formatTimestampSeconds(optValue(classifiedData.overview?.meta?.last_commitment_ts));
    const cyclesStatus = cyclesProbeStatusInfo(classifiedData);
    const usingLogCycles = cycleSamples[0]?.source === 'log';
    const latestCyclesHtml = summary.latestCycles !== null && summary.latestCycles !== undefined
      ? escapeHtml(formatCycles(summary.latestCycles))
      : renderCyclesStatusCell(cyclesStatus.label);
    const estimatedObservedCyclesBurnedPerDay = estimateCyclesBurnedPerDay(classifiedData);
    const estimatedCyclesBurnHtml = estimatedObservedCyclesBurnedPerDay === null
      ? null
      : escapeHtml(formatTrillionCyclesPerDay(estimatedObservedCyclesBurnedPerDay));
    const simulatorPrefillHtml = renderSimulatorPrefillLink({
      dailyBurnCycles: estimatedObservedCyclesBurnedPerDay,
      commitmentE8s: fullSummary.qualifyingCommittedE8s || fullSummary.totalCommittedE8s,
      simulatorHashForPrefill,
    });
    const burnEstimateSamples = cycleSamplesForBurnEstimate(classifiedData);
    const cycleSourceLabel = burnEstimateSamples.length > 0 ? cycleSampleSourceLabel(burnEstimateSamples) : 'available cycle data';
    const commitmentError = classifiedData.errors?.commitments ? `<p class="pane-status-note tracker-status-note">Commitment history unavailable: ${escapeHtml(classifiedData.errors.commitments)}</p>` : '';
    const cyclesError = classifiedData.errors?.cycles ? `<p class="pane-status-note tracker-status-note">Cycles history unavailable: ${escapeHtml(classifiedData.errors.cycles)}</p>` : '';
    const hasCyclesOutsideRange = (classifiedData?.cycles?.items || []).length > 0 && (visibleData?.cycles?.items || []).length === 0;
    const cyclesStatusNote = summary.latestCycles === null || summary.latestCycles === undefined
      ? `<p class="pane-status-note tracker-status-note">${escapeHtml(hasCyclesOutsideRange ? `No cycles samples are available in ${rangeLabel}.` : cyclesStatus.note)}</p>`
      : '';
    const cyclesProbeIssueNote = summary.latestCycles !== null && summary.latestCycles !== undefined
      ? renderCyclesProbeInfoNote(cyclesStatus, usingLogCycles)
      : '';
    const cmcError = classifiedData.errors?.cmcTransfers ? `<p class="pane-status-note tracker-status-note">Observed CMC top-up history unavailable: ${escapeHtml(classifiedData.errors.cmcTransfers)}</p>` : '';
    const protocolHtml = state.protocolCanisterText
      ? `<div><dt>Protocol canister</dt><dd class="pane-detail-value">${renderCanisterDashboardLink(state.protocolCanisterText)}</dd></div>`
      : '';
    result.innerHTML = `
      ${renderTrackerRangeControls()}
      <div class="tracker-chart-wrapper" id="tracker-chart-wrapper"></div>
      ${cyclesProbeIssueNote}
      ${renderTrackerLogs(data)}
      <dl class="pane-detail-grid tracker-summary-grid">
        <div><dt>Canister</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(principalText)}</dd></div>
        <div><dt>Dashboard</dt><dd class="pane-detail-value">${renderCanisterDashboardLink(principalText)}</dd></div>
        ${protocolHtml}
        <div><dt>Tracking reasons</dt><dd class="pane-detail-value">${escapeHtml(trackingReasons)}</dd></div>
        <div><dt>First seen</dt><dd class="pane-detail-value">${escapeHtml(firstSeen)}</dd></div>
        <div><dt>Last commitment</dt><dd class="pane-detail-value">${escapeHtml(lastCommitment)}</dd></div>
        <div><dt>Patron commitments shown</dt><dd class="pane-detail-value">${escapeHtml(formatInteger(summary.commitmentCount))}</dd></div>
        <div><dt>Total commitments shown</dt><dd class="pane-detail-value">${escapeHtml(formatIcpE8s(summary.totalCommittedE8s))}</dd></div>
        <div><dt>Qualifying commitments shown</dt><dd class="pane-detail-value">${escapeHtml(`${formatInteger(summary.qualifyingCommitmentCount)} · ${formatIcpE8s(summary.qualifyingCommittedE8s)}`)}</dd></div>
        <div><dt>Observed CMC transfers shown</dt><dd class="pane-detail-value">${escapeHtml(formatInteger(summary.observedCmcTransferCount))}</dd></div>
        <div><dt>Observed ICP to CMC shown</dt><dd class="pane-detail-value">${escapeHtml(formatIcpE8s(summary.observedCmcE8s))}</dd></div>
        <div><dt>Latest cycles shown</dt><dd class="pane-detail-value">${latestCyclesHtml}</dd></div>
        ${estimatedCyclesBurnHtml === null ? '' : `<div><dt>Estimated observed cycles burned/day</dt><dd class="pane-detail-value">${estimatedCyclesBurnHtml}</dd></div>`}
        ${estimatedCyclesBurnHtml === null ? '' : `<div><dt>Simulator prefill</dt><dd class="pane-detail-value">${simulatorPrefillHtml}</dd></div>`}
      </dl>
      <p class="pane-status-note tracker-status-note">Showing ${escapeHtml(rangeLabel)} using ${escapeHtml(trackerBucketDescription())}. Patron commitments are memo-registered ICP commitments associated with this beneficiary. Observed CMC top-ups are ICP transfers into the canister’s CMC top-up account and may include direct non-Jupiter top-ups.</p>
      ${estimatedCyclesBurnHtml === null ? '' : `<p class="pane-status-note tracker-status-note">Estimated observed cycles burn is calculated from loaded ${escapeHtml(cycleSourceLabel)} samples and observed CMC top-ups when a cached ICP/XDR rate is available; otherwise it falls back to downward balance changes.</p>`}
      ${renderTrackerCadenceNote(classifiedData)}
      ${commitmentError}
      ${cyclesStatusNote}
      ${cyclesError}
      ${cmcError}`;
    renderTrackerCharts(visibleData, classifiedData);
  };

  const setLoading = (loading) => {
    const submit = document.getElementById('tracker-submit');
    const input = document.getElementById('tracker-principal-input');
    if (submit) submit.disabled = loading;
    if (input) input.disabled = loading;
  };

  const setStatus = (message = '', kind = '') => {
    const status = document.getElementById('tracker-status');
    if (!status) return;
    status.textContent = message;
    status.hidden = !message;
    status.className = kind ? `pane-status-note tracker-status-note tracker-status-note--${kind}` : 'pane-status-note tracker-status-note';
  };

  const classifyRawTransfers = (data) => ({
    ...data,
    transfers: {
      ...(data?.transfers || { items: [] }),
      items: (data?.transfers?.items || []).map((item) => classifyTransferItem(item, {
        status: data?.status,
        protocolCanisterId: state.protocolCanisterId,
        relaySourceMap: relayRegistrySourceMap(data?.relayRegistrations?.items || []),
      })),
    },
  });

  const renderRawIcpChart = ({ buckets, emptyMessage, ariaLabel, segments = RAW_SOURCE_SEGMENTS }) => {
    if (buckets.length === 0) return renderEmptyChart(emptyMessage);
    const activeSegments = activeSourceSegments(segments, buckets);
    return renderStackedAmountBarChart({
      buckets,
      segments: activeSegments,
      emptyMessage,
      ariaLabel,
      labelBuilder: (bucket, segment, amount, count) => `${bucket.label}: ${segment.label} ${formatIcpE8s(amount)} across ${pluralize(count, 'transfer')}`,
      valueFormatter: formatIcpE8s,
      xAxis: 'time',
      minBarWidth: 2,
      maxBarWidth: 28,
      allowMinBarOverflow: true,
    });
  };

  const renderCandidateLinks = (data, parsed) => {
    const prefix = parsed.outgoingMemoText || '';
    if (prefix.length < 4) {
      return '<p class="pane-status-note tracker-status-note">Prefix matching is skipped for short outgoing memos; use at least four compact-principal characters for candidate search.</p>';
    }
    if (data?.candidates?.loading) {
      return '<p class="pane-status-note tracker-status-note">Matching tracked canisters are still loading.</p>';
    }
    const items = data?.candidates?.items || [];
    if (items.length === 0) {
      return '<p class="pane-status-note tracker-status-note">If the right-hand side of the memo identifies another canister then it is not yet known to Jupiter Faucet through a direct cycle top-up memo commitment. You can track that canister directly by committing 1 ICP with that canister&#39;s full ID in the memo (see <a class="pane-external-link" href="#how-it-works">How it Works</a>).</p>';
    }
    const links = items.map((item) => {
      const canisterText = item.canister_id.toText();
      const href = trackerHashForMemo({ memo: canisterText, protocolCanister: parsed.canisterText });
      return `<li><a class="pane-external-link mono" href="${escapeHtml(href)}" data-tracker-memo="${escapeHtml(canisterText)}" data-protocol-canister="${escapeHtml(parsed.canisterText)}">${escapeHtml(canisterText)}</a> <span>${escapeHtml(formatIcpE8s(item.total_qualifying_committed_e8s))} qualifying</span></li>`;
    }).join('');
    return `
      <div class="tracker-candidate-section">
        <h3>Tracked canisters matching the memo&#39;s &#39;.&#39; suffix</h3>
        <ul>${links}</ul>
        ${data?.candidates?.truncated ? '<p class="pane-status-note tracker-status-note">More possible matches exist than are shown.</p>' : ''}
      </div>`;
  };

  const renderRawIcpData = (data, parsed) => {
    const result = document.getElementById('tracker-result');
    if (!result) return;
    const classified = classifyRawTransfers(data);
    const latestRawMs = (classified.transfers?.items || [])
      .map(rawTransferTimestampMs)
      .filter((value) => value !== null)
      .reduce((latest, value) => value > latest ? value : latest, 0);
    const visibleItems = (classified.transfers?.items || []).filter((item) => {
      if (state.range === 'all') return true;
      const timestamp = rawTransferTimestampMs(item);
      return timestamp !== null && timestamp >= trackerRangeCutoffMs(state.range, latestRawMs || Date.now());
    });
    const summary = rawSummary(visibleItems);
    const isNeuron = parsed.kind === 'neuronStake';
    const title = isNeuron ? 'Raw ICP neuron memo' : 'Raw ICP canister memo';
    const target = isNeuron ? parsed.neuronId.toString() : parsed.canisterText;
    const targetHtml = isNeuron ? renderNeuronDashboardLink(target) : escapeHtml(target);
    const hasOutgoingMemo = parsed.outgoingMemoText !== null && parsed.outgoingMemoText !== undefined;
    const sourceSegments = hasOutgoingMemo ? RAW_SOURCE_SEGMENTS : SOURCE_SEGMENTS;
    const buckets = aggregateRawTransfers(visibleItems, state.range);
    const transferLoadingNote = classified.transfers?.loading
      ? `<p class="pane-status-note tracker-status-note tracker-status-note--loading tracker-chart-note">Chart still loading incoming ICP history… ${escapeHtml(formatInteger((classified.transfers.items || []).length))} transfers loaded${classified.transfers.pages_loaded ? ` across ${escapeHtml(formatInteger(classified.transfers.pages_loaded))} index pages` : ''}. The bars update as records arrive.</p>`
      : '';
    const matchingNote = !hasOutgoingMemo
      ? ''
      : `<p class="pane-status-note tracker-status-note">Visible Jupiter Faucet transfers matching the outgoing memo: ${escapeHtml(formatInteger(summary.faucetMatchingMemoTransferCount))} · ${escapeHtml(formatIcpE8s(summary.faucetMatchingMemoIcpE8s))}. If no transfers match, the top-up may not have been indexed yet, may be outside the loaded range, or may not have been paid through Jupiter Faucet yet.</p>`;
    const transferLimitNote = classified.transfers?.truncated
      ? `<p class="pane-status-note tracker-status-note tracker-status-note--info tracker-chart-note">Chart display is limited to the newest ${escapeHtml(formatInteger(classified.transfers.limit || (classified.transfers.items || []).length))} incoming ICP transfers loaded for this tracker view. Older transfers are omitted from the chart and summary.</p>`
      : '';
    result.innerHTML = `
      ${renderTrackerRangeControls()}
      <div class="tracker-chart-wrapper" id="tracker-chart-wrapper">
        <div class="tracker-chart-card">
          <div class="tracker-chart-header">
            <h3>${escapeHtml(title)}</h3>
            <span>Observed incoming ICP transfers into the ${isNeuron ? 'staking' : 'canister'} account, colour-coded by source.</span>
          </div>
          ${transferLoadingNote}
          ${transferLimitNote}
          ${renderActiveSourceLegend({ includeProtocol: Boolean(state.protocolCanisterText), segments: sourceSegments, buckets })}
          ${renderRawIcpChart({
            buckets,
            emptyMessage: classified.transfers?.loading
              ? 'Loading incoming ICP transfer history…'
              : `No dated incoming ICP transfers are available in ${trackerRangeLabel()}.`,
            ariaLabel: `${title} incoming transfers in ${trackerRangeLabel()}`,
            segments: sourceSegments,
          })}
        </div>
      </div>
      <dl class="pane-detail-grid tracker-summary-grid">
        <div><dt>Memo type</dt><dd class="pane-detail-value">${escapeHtml(title)}</dd></div>
        <div><dt>Tracked target</dt><dd class="pane-detail-value mono">${targetHtml}</dd></div>
        <div><dt>Incoming transfers shown</dt><dd class="pane-detail-value">${escapeHtml(formatInteger(summary.totalTransferCount))}</dd></div>
        <div><dt>Incoming ICP shown</dt><dd class="pane-detail-value">${escapeHtml(formatIcpE8s(summary.totalIcpE8s))}</dd></div>
        <div><dt>Jupiter Faucet inflow shown</dt><dd class="pane-detail-value">${escapeHtml(`${formatInteger(summary.faucetTransferCount)} · ${formatIcpE8s(summary.faucetIcpE8s)}`)}</dd></div>
        ${!hasOutgoingMemo ? '' : `<div><dt>Outgoing memo</dt><dd class="pane-detail-value mono">${escapeHtml(parsed.outgoingMemoText)}</dd></div>`}
      </dl>
      ${matchingNote}
      ${!isNeuron ? renderCandidateLinks(classified, parsed) : ''}
      ${classified.errors?.transfers ? `<p class="pane-status-note tracker-status-note">Raw ICP transfer history unavailable: ${escapeHtml(classified.errors.transfers)}</p>` : ''}
      ${classified.errors?.candidates ? `<p class="pane-status-note tracker-status-note">Possible matching canisters unavailable: ${escapeHtml(classified.errors.candidates)}</p>` : ''}`;
  };

  const renderPrompt = () => {
    const result = document.getElementById('tracker-result');
    if (!result || result.innerHTML.trim()) return;
    result.innerHTML = `
      <div class="tracker-empty-state">
        <p>Paste a memo to inspect Jupiter Faucet tracking history. Plain canister memos open cycle top-up tracking; dotted canister memos show raw ICP into the target canister account; numeric memos show public neuron staking-account inflows.</p>
      </div>`;
  };

  const setRange = (range) => {
    state.range = range === 'all' || range === 'year' ? range : 'month';
    document.querySelectorAll('[data-tracker-range]').forEach((button) => {
      const active = button.getAttribute('data-tracker-range') === state.range;
      button.classList.toggle('is-active', active);
      button.setAttribute('aria-pressed', active ? 'true' : 'false');
    });
    if (
      state.data
      && state.parsedMemo
      && !state.loading
      && state.loadedRange !== null
      && trackerRangeRank(state.range) > trackerRangeRank(state.loadedRange)
    ) {
      void submitMemo();
      return;
    }
    if (state.viewMode === 'cyclesTopUp' && state.data?.isCommitmentBeneficiary) {
      renderData(state.data, state.principalText);
    } else if ((state.viewMode === 'rawIcpCanister' || state.viewMode === 'neuronStake') && state.data && state.parsedMemo) {
      renderRawIcpData(state.data, state.parsedMemo);
    }
  };

  const replaceLocationHash = (memoText, protocolCanister = state.protocolCanisterText) => {
    const hash = trackerHashForMemo({ memo: memoText, protocolCanister });
    if (window.location.hash !== hash) {
      history.replaceState(null, '', hash);
    }
  };

  const parseProtocolCanister = (text) => {
    const trimmed = String(text || '').trim();
    if (!trimmed) return null;
    try {
      return Principal.fromText(trimmed);
    } catch {
      return null;
    }
  };

  const submitMemo = async () => {
    const input = document.getElementById('tracker-principal-input');
    const result = document.getElementById('tracker-result');
    const raw = input?.value || '';
    state.memoText = raw;
    state.error = null;

    const parsed = parseJupiterMemo(raw);
    state.parsedMemo = parsed;
    if (parsed.kind === 'invalid') {
      setStatus(parsed.reason, 'error');
      input?.focus?.();
      return;
    }

    state.viewMode = parsed.kind;
    state.principalText = parsed.canisterText || '';
    state.protocolCanisterId = parseProtocolCanister(state.protocolCanisterText);
    replaceLocationHash(raw);
    setLoading(true);
    setStatus('Loading tracker data…', 'loading');
    const historyLimit = RAW_ICP_TRACKER_TRANSFER_LIMIT;
    const minTimestampNanos = trackerRangeCutoffNanos(state.range);
    if (result) {
      result.innerHTML = '<div class="tracker-empty-state"><p>Loading…</p></div>';
    }

    const progressRequestId = `${Date.now()}-${Math.random()}`;
    state.progressRequestId = progressRequestId;
    try {
      const common = {
        historianCanisterId: frontendConfig?.historianCanisterId,
        host: window.location.origin,
        local: isLocalHost(),
      };
      const onTransfersProgress = (partialData) => {
        if (state.progressRequestId !== progressRequestId || state.viewMode !== parsed.kind) return;
        state.data = partialData;
        renderRawIcpData(partialData, parsed);
      };
      const data = parsed.kind === 'cyclesTopUp'
        ? await loadData({ ...common, canisterId: parsed.canisterId, historyLimit, minTimestampNanos })
        : parsed.kind === 'rawIcpCanister'
          ? await loadRawCanisterData({ ...common, canisterId: parsed.canisterId, outgoingMemoText: parsed.outgoingMemoText, historyLimit, minTimestampNanos, onTransfersProgress })
          : await loadNeuronData({ ...common, neuronId: parsed.neuronId, outgoingMemoText: parsed.outgoingMemoText, historyLimit, minTimestampNanos, onTransfersProgress });
      if (state.progressRequestId !== progressRequestId) return;
      state.data = data;
      state.loadedRange = state.range;
      setStatus('', '');
      if (parsed.kind === 'cyclesTopUp') renderData(data, parsed.canisterText);
      else renderRawIcpData(data, parsed);
    } catch (error) {
      if (state.progressRequestId !== progressRequestId) return;
      state.data = null;
      state.error = normalizeLoadError(error);
      setStatus(`Tracker unavailable: ${state.error}`, 'error');
      if (result) {
        result.innerHTML = '<div class="tracker-empty-state"><p>Tracker data could not be loaded right now.</p></div>';
      }
    } finally {
      if (state.progressRequestId === progressRequestId) {
        setLoading(false);
      }
    }
  };

  const submitPrincipal = submitMemo;

  const hydrateFromLocationHash = ({ submit = false } = {}) => {
    const route = trackerStateFromHash();
    const memoText = route.memo || route.legacyPrincipal;
    if (!memoText) return false;
    state.memoText = memoText;
    state.protocolCanisterText = route.protocolCanister || '';
    const input = document.getElementById('tracker-principal-input');
    if (input) input.value = memoText;
    if (submit && lastHashSubmitMemo !== `${memoText}|${state.protocolCanisterText}`) {
      lastHashSubmitMemo = `${memoText}|${state.protocolCanisterText}`;
      window.setTimeout(() => {
        const refreshedInput = document.getElementById('tracker-principal-input');
        if (refreshedInput) refreshedInput.value = memoText;
        void submitMemo();
      }, 0);
    }
    return true;
  };

  const openPanelForLinkedPrincipal = (principalText = '') => {
    const trackerSection = document.querySelector('.nav-panel-section[data-panel="metric-tracker"]');
    const trackerAlreadyOpen = document.body.classList.contains('nav-panel-open')
      && trackerSection?.classList.contains('nav-panel-section--active');
    if (trackerAlreadyOpen) {
      if (principalText) replaceLocationHash(principalText);
      return;
    }
    const trigger = document.querySelector('a[data-panel="metric-tracker"]');
    if (trigger) {
      trigger.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }));
      if (principalText) replaceLocationHash(principalText);
      return;
    }
    window.location.hash = principalText ? trackerHashForPrincipal(principalText) : '#metric-tracker';
  };

  const trackLinkedMemo = ({ memo, protocolCanister = '' } = {}) => {
    const text = String(memo || '').trim();
    if (!text) return;
    state.memoText = text;
    state.protocolCanisterText = String(protocolCanister || '').trim();
    const input = document.getElementById('tracker-principal-input');
    if (input) input.value = text;
    openPanelForLinkedPrincipal(text);
    window.setTimeout(() => {
      const refreshedInput = document.getElementById('tracker-principal-input');
      if (refreshedInput) refreshedInput.value = text;
      replaceLocationHash(text, state.protocolCanisterText);
      void submitMemo();
    }, 0);
  };

  const trackLinkedPrincipal = (principalText) => trackLinkedMemo({ memo: principalText });

  const bindLinks = () => {
    if (document.documentElement.dataset.trackerLinksBound === 'true') return;
    document.documentElement.dataset.trackerLinksBound = 'true';
    document.addEventListener('click', (event) => {
      const trigger = event.target instanceof Element ? event.target.closest('[data-tracker-principal]') : null;
      const memoTrigger = event.target instanceof Element ? event.target.closest('[data-tracker-memo]') : null;
      if (!trigger && !memoTrigger) return;
      if (memoTrigger) {
        const memo = memoTrigger.getAttribute('data-tracker-memo') || '';
        const protocolCanister = memoTrigger.getAttribute('data-protocol-canister') || '';
        if (!memo) return;
        event.preventDefault();
        event.stopPropagation();
        trackLinkedMemo({ memo, protocolCanister });
        return;
      }
      const principalText = trigger.getAttribute('data-tracker-principal') || '';
      if (!principalText) return;
      event.preventDefault();
      event.stopPropagation();
      trackLinkedPrincipal(principalText);
    }, true);
  };

  const bindPane = () => {
    const form = document.getElementById('tracker-form');
    if (form && form.dataset.bound !== 'true') {
      form.dataset.bound = 'true';
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        void submitPrincipal();
      });
    }
    const result = document.getElementById('tracker-result');
    if (result && result.dataset.rangeBound !== 'true') {
      result.dataset.rangeBound = 'true';
      result.addEventListener('click', (event) => {
        const simulatorLink = event.target instanceof Element ? event.target.closest('[data-simulator-prefill]') : null;
        if (simulatorLink && result.contains(simulatorLink)) {
          event.preventDefault();
          const href = simulatorLink.getAttribute('href') || '#simulator';
          if (window.location.hash !== href) {
            history.pushState(null, '', href);
          }
          onSimulatorPrefillHash(href);
          return;
        }
        const button = event.target instanceof Element ? event.target.closest('[data-tracker-range]') : null;
        if (!button || !result.contains(button)) return;
        event.preventDefault();
        if (button.disabled || button.getAttribute('aria-disabled') === 'true' || rawTransfersLoading()) return;
        setRange(button.getAttribute('data-tracker-range') || 'month');
      });
    }
    setRange(state.range);
    renderPrompt();
  };

  return {
    state,
    bindLinks,
    bindPane,
    hydrateFromLocationHash,
    renderPrompt,
    setRange,
    submitPrincipal,
    trackLinkedMemo,
    trackLinkedPrincipal,
  };
}
