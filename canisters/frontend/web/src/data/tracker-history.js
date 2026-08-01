import { createActor as createIndexActor } from '../../declarations/icp_index/index.js';
import { createActor as createHistorianActor } from '../../declarations/jupiter_historian/index.js';
import { createActor as createGovernanceActor } from '../../declarations/nns_governance/index.js';
import { createHistorianClient, normalizeError } from '../app/agent.js';
import { GOVERNANCE_CANISTER_ID } from '../app/config.js';
import { loadCanisterLogs } from './cycles.js';
import { loadCmcTopUpTransfersFromIndex, loadIncomingIcpTransfersFromIndex } from './index-transactions.js';
import { loadPublicNeuronStakingAccount } from './nns-neurons.js';
import {
  MAINNET_CMC_CANISTER_ID,
  fulfilledOrNull,
  hasCanisterTrackingReason,
  principalToText,
  readOptional,
} from './dashboard-transforms.js';

const TRACKER_HISTORY_PAGE_SIZE = 100;
export const RAW_ICP_TRACKER_TRANSFER_LIMIT = 10_000;
const TRACKER_PROGRESS_PAGE_KEYS = [
  'commitments',
  'cycles',
  'logs',
  'cmcTransfers',
  'relayInstances',
  'transfers',
  'candidates',
];

function progressPage(value = null, loading = false) {
  return {
    ...(value || {}),
    items: [...(value?.items || [])],
    loading,
  };
}

function trackerProgressSnapshot(progress) {
  const snapshot = {
    ...progress,
    errors: { ...(progress?.errors || {}) },
  };
  TRACKER_PROGRESS_PAGE_KEYS.forEach((key) => {
    if (progress?.[key]) snapshot[key] = progressPage(progress[key], Boolean(progress[key].loading));
  });
  return snapshot;
}

function notifyTrackerProgress(onProgress, progress) {
  if (typeof onProgress === 'function') onProgress(trackerProgressSnapshot(progress));
}

function trackProgressPromise(promise, {
  progress,
  key,
  onProgress,
  page = true,
  fallback = null,
  errorKey = null,
} = {}) {
  return promise.then(
    (value) => {
      progress[key] = page ? progressPage(value, false) : value;
      notifyTrackerProgress(onProgress, progress);
      return value;
    },
    (error) => {
      progress[key] = page ? progressPage(fallback, false) : fallback;
      if (errorKey) progress.errors[errorKey] = normalizeError(error);
      notifyTrackerProgress(onProgress, progress);
      throw error;
    },
  );
}

function positiveLimit(limit, fallback = TRACKER_HISTORY_PAGE_SIZE) {
  const value = Number(limit);
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : fallback;
}

function timestampNanos(value) {
  const timestamp = readOptional(value);
  if (timestamp === undefined || timestamp === null) return null;
  return typeof timestamp === 'bigint' ? timestamp : BigInt(timestamp);
}

function itemTimestampNanos(item) {
  return timestampNanos(item?.timestamp_nanos);
}

function isInsideTimestampCutoff(item, minTimestampNanos) {
  if (minTimestampNanos === null || minTimestampNanos === undefined) return true;
  const timestamp = itemTimestampNanos(item);
  return timestamp !== null && timestamp >= minTimestampNanos;
}

function pageCrossedTimestampCutoff(items, minTimestampNanos) {
  if (minTimestampNanos === null || minTimestampNanos === undefined) return false;
  const dated = (items || []).map(itemTimestampNanos).filter((timestamp) => timestamp !== null);
  if (dated.length === 0) return false;
  return dated.reduce((oldest, timestamp) => timestamp < oldest ? timestamp : oldest, dated[0]) < minTimestampNanos;
}

function buildGetCyclesHistoryArgs({ canisterId, startAfter = null, limit = TRACKER_HISTORY_PAGE_SIZE, descending = false } = {}) {
  return {
    canister_id: canisterId,
    start_after_ts: startAfter === null || startAfter === undefined ? [] : [typeof startAfter === 'bigint' ? startAfter : BigInt(startAfter)],
    limit: [limit],
    descending: [Boolean(descending)],
  };
}

async function loadTrackerCycles(historian, { canisterId, historyLimit, minTimestampNanos = null }) {
  const limit = positiveLimit(historyLimit);
  const items = [];
  let startAfter = null;
  let nextStartAfter = null;
  while (items.length < limit) {
    const page = await historian.get_cycles_history(buildGetCyclesHistoryArgs({
      canisterId,
      startAfter,
      limit: Math.min(TRACKER_HISTORY_PAGE_SIZE, limit - items.length),
      descending: true,
    }));
    const pageItems = page?.items || [];
    for (const item of pageItems) {
      if (isInsideTimestampCutoff(item, minTimestampNanos)) items.push(item);
      if (items.length >= limit) break;
    }
    nextStartAfter = readOptional(page?.next_start_after_ts);
    if (pageCrossedTimestampCutoff(pageItems, minTimestampNanos) || nextStartAfter === null || nextStartAfter === undefined) break;
    startAfter = nextStartAfter;
  }
  return {
    items: items.sort((left, right) => {
      const leftTs = typeof left.timestamp_nanos === 'bigint' ? left.timestamp_nanos : BigInt(left.timestamp_nanos);
      const rightTs = typeof right.timestamp_nanos === 'bigint' ? right.timestamp_nanos : BigInt(right.timestamp_nanos);
      return leftTs < rightTs ? -1 : leftTs > rightTs ? 1 : 0;
    }),
    next_start_after_ts: items.length >= limit && nextStartAfter !== null && nextStartAfter !== undefined ? [nextStartAfter] : [],
  };
}

function buildGetCommitmentHistoryArgs({ canisterId, startAfter = null, limit = TRACKER_HISTORY_PAGE_SIZE, descending = false } = {}) {
  return {
    canister_id: canisterId,
    start_after_tx_id: startAfter === null || startAfter === undefined ? [] : [typeof startAfter === 'bigint' ? startAfter : BigInt(startAfter)],
    limit: [limit],
    descending: [Boolean(descending)],
  };
}

function buildGetNeuronCommitmentHistoryArgs({ neuronId, startAfter = null, limit = TRACKER_HISTORY_PAGE_SIZE, descending = false } = {}) {
  return {
    neuron_id: typeof neuronId === 'bigint' ? neuronId : BigInt(neuronId),
    start_after_tx_id: startAfter === null || startAfter === undefined ? [] : [typeof startAfter === 'bigint' ? startAfter : BigInt(startAfter)],
    limit: [limit],
    descending: [Boolean(descending)],
  };
}

async function loadCommitmentHistoryPages({ fetchPage, historyLimit, minTimestampNanos = null }) {
  const limit = positiveLimit(historyLimit);
  const items = [];
  let startAfter = null;
  let nextStartAfter = null;
  while (items.length < limit) {
    const page = await fetchPage({
      startAfter,
      limit: Math.min(TRACKER_HISTORY_PAGE_SIZE, limit - items.length),
      descending: true,
    });
    const pageItems = page?.items || [];
    for (const item of pageItems) {
      if (isInsideTimestampCutoff(item, minTimestampNanos)) items.push(item);
      if (items.length >= limit) break;
    }
    nextStartAfter = readOptional(page?.next_start_after_tx_id);
    if (pageCrossedTimestampCutoff(pageItems, minTimestampNanos) || nextStartAfter === null || nextStartAfter === undefined) break;
    startAfter = nextStartAfter;
  }
  return {
    items: items.sort((left, right) => {
      const leftTx = typeof left.tx_id === 'bigint' ? left.tx_id : BigInt(left.tx_id);
      const rightTx = typeof right.tx_id === 'bigint' ? right.tx_id : BigInt(right.tx_id);
      return leftTx < rightTx ? -1 : leftTx > rightTx ? 1 : 0;
    }),
    next_start_after_tx_id: items.length >= limit && nextStartAfter !== null && nextStartAfter !== undefined ? [nextStartAfter] : [],
  };
}

async function loadTrackerCommitments(historian, { canisterId, historyLimit, minTimestampNanos = null }) {
  if (typeof historian?.get_commitment_history !== 'function') {
    throw new Error('Historian commitment history query is unavailable');
  }
  return loadCommitmentHistoryPages({
    historyLimit,
    minTimestampNanos,
    fetchPage: ({ startAfter, limit, descending }) => historian.get_commitment_history(buildGetCommitmentHistoryArgs({
      canisterId,
      startAfter,
      limit,
      descending,
    })),
  });
}

async function loadRawIcpCanisterCommitments(historian, { canisterId, historyLimit, minTimestampNanos = null }) {
  if (typeof historian?.get_raw_icp_commitment_history !== 'function') {
    throw new Error('Historian raw ICP commitment history query is unavailable');
  }
  return loadCommitmentHistoryPages({
    historyLimit,
    minTimestampNanos,
    fetchPage: ({ startAfter, limit, descending }) => historian.get_raw_icp_commitment_history(buildGetCommitmentHistoryArgs({
      canisterId,
      startAfter,
      limit,
      descending,
    })),
  });
}

async function loadNeuronCommitments(historian, { neuronId, historyLimit, minTimestampNanos = null }) {
  if (typeof historian?.get_neuron_commitment_history !== 'function') {
    throw new Error('Historian neuron commitment history query is unavailable');
  }
  return loadCommitmentHistoryPages({
    historyLimit,
    minTimestampNanos,
    fetchPage: ({ startAfter, limit, descending }) => historian.get_neuron_commitment_history(buildGetNeuronCommitmentHistoryArgs({
      neuronId,
      startAfter,
      limit,
      descending,
    })),
  });
}

async function loadTrackerCmcTransfers({ historian, status = null, agent, indexActorFactory, canisterId, cmcCanisterId, historyLimit, minTimestampNanos = null }) {
  const resolvedStatus = status || await historian.get_public_status();
  const indexCanisterId = principalToText(resolvedStatus?.index_canister_id);
  if (!indexCanisterId) {
    throw new Error('Historian status does not expose an ICP index canister ID');
  }
  const effectiveCmcCanisterId = cmcCanisterId || readOptional(resolvedStatus?.cmc_canister_id) || MAINNET_CMC_CANISTER_ID;
  const index = indexActorFactory(indexCanisterId, { agent });
  return loadCmcTopUpTransfersFromIndex({
    index,
    canisterId,
    cmcCanisterId: effectiveCmcCanisterId,
    limit: historyLimit,
    minTimestampNanos,
  });
}

async function loadRawIncomingTransfers({ historian, status = null, agent, indexActorFactory, account, memoText, historyLimit, minTimestampNanos = null, onProgress = null }) {
  const resolvedStatus = status || await historian.get_public_status();
  const indexCanisterId = principalToText(resolvedStatus?.index_canister_id);
  if (!indexCanisterId) {
    throw new Error('Historian status does not expose an ICP index canister ID');
  }
  const index = indexActorFactory(indexCanisterId, { agent });
  return loadIncomingIcpTransfersFromIndex({
    index,
    account,
    memoText,
    limit: historyLimit,
    minTimestampNanos,
    onProgress,
  });
}

async function loadRelayInstances(historian) {
  if (typeof historian?.list_canisters !== 'function') return { items: [] };
  try {
    const items = [];
    let startAfter = [];
    let previousCursor = '';
    while (true) {
      const page = await historian.list_canisters({
        start_after: startAfter,
        limit: [100],
        tracking_reason_filter: [{ RelayInstance: null }],
      });
      items.push(...(page?.items || []));
      const next = readOptional(page?.next_start_after);
      if (!next) break;
      const nextText = principalToText(next);
      if (!nextText || nextText === previousCursor) break;
      previousCursor = nextText;
      startAfter = [next];
    }
    return { items };
  } catch {
    return { items: [] };
  }
}

export async function loadTrackerData({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  indexActorFactory = createIndexActor,
  canisterLogsLoader = loadCanisterLogs,
  canisterId,
  cmcCanisterId = null,
  historyLimit = TRACKER_HISTORY_PAGE_SIZE,
  minTimestampNanos = null,
  onProgress = null,
} = {}) {
  if (!canisterId) {
    throw new Error('A canister ID is required');
  }

  const { agent: resolvedAgent, historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });

  const overview = await historian.get_canister_overview(canisterId);
  const overviewValue = readOptional(overview);
  const isCommitmentBeneficiary = hasCanisterTrackingReason(overviewValue?.tracking_reasons, 'MemoCommitment');

  if (!overviewValue) {
    return {
      canisterId,
      overview: overviewValue,
      isRecognized: false,
      isCommitmentBeneficiary,
      commitments: { items: [] },
      cycles: { items: [] },
      logs: { items: [] },
      cmcTransfers: { items: [] },
      errors: { commitments: null, cycles: null, logs: null, cmcTransfers: null },
    };
  }

  const progress = {
    canisterId,
    overview: overviewValue,
    status: null,
    relayInstances: progressPage(null, true),
    isRecognized: true,
    isCommitmentBeneficiary,
    commitments: progressPage(null, isCommitmentBeneficiary),
    cycles: progressPage(null, true),
    logs: progressPage(null, true),
    cmcTransfers: progressPage(null, true),
    errors: { commitments: null, cycles: null, logs: null, cmcTransfers: null },
  };
  notifyTrackerProgress(onProgress, progress);

  const commitmentsPromise = trackProgressPromise(isCommitmentBeneficiary
    ? loadTrackerCommitments(historian, {
        canisterId,
        historyLimit,
        minTimestampNanos,
      })
    : Promise.resolve({ items: [] }), {
    progress,
    key: 'commitments',
    onProgress,
    errorKey: 'commitments',
  });
  const cyclesPromise = trackProgressPromise(loadTrackerCycles(historian, {
    canisterId,
    historyLimit,
    minTimestampNanos,
  }), {
    progress,
    key: 'cycles',
    onProgress,
    errorKey: 'cycles',
  });
  const statusPromise = trackProgressPromise(historian.get_public_status(), {
    progress,
    key: 'status',
    onProgress,
    page: false,
  });
  const relayInstancesPromise = trackProgressPromise(loadRelayInstances(historian), {
    progress,
    key: 'relayInstances',
    onProgress,
  });
  const logsPromise = trackProgressPromise(canisterLogsLoader({
    agent: resolvedAgent,
    canisterId,
  }), {
    progress,
    key: 'logs',
    onProgress,
    errorKey: 'logs',
  });
  const cmcTransfersPromise = trackProgressPromise(statusPromise.then((status) => loadTrackerCmcTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    canisterId,
    cmcCanisterId,
    historyLimit,
    minTimestampNanos,
  })), {
    progress,
    key: 'cmcTransfers',
    onProgress,
    errorKey: 'cmcTransfers',
  });

  const [commitmentsResult, cyclesResult, statusResult, relayInstancesResult, logsResult, cmcTransfersResult] = await Promise.allSettled([
    commitmentsPromise,
    cyclesPromise,
    statusPromise,
    relayInstancesPromise,
    logsPromise,
    cmcTransfersPromise,
  ]);

  return {
    canisterId,
    overview: overviewValue,
    status: fulfilledOrNull(statusResult),
    relayInstances: fulfilledOrNull(relayInstancesResult) || { items: [] },
    isRecognized: true,
    isCommitmentBeneficiary,
    commitments: fulfilledOrNull(commitmentsResult) || { items: [] },
    cycles: fulfilledOrNull(cyclesResult) || { items: [] },
    logs: fulfilledOrNull(logsResult) || { items: [] },
    cmcTransfers: fulfilledOrNull(cmcTransfersResult) || { items: [] },
    errors: {
      commitments: commitmentsResult.status === 'rejected' ? normalizeError(commitmentsResult.reason) : null,
      cycles: cyclesResult.status === 'rejected' ? normalizeError(cyclesResult.reason) : null,
      logs: logsResult.status === 'rejected' ? normalizeError(logsResult.reason) : null,
      cmcTransfers: cmcTransfersResult.status === 'rejected' ? normalizeError(cmcTransfersResult.reason) : null,
    },
  };
}

export async function loadRawIcpCanisterTrackerData({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  indexActorFactory = createIndexActor,
  canisterId,
  outgoingMemoText = null,
  prefixLimit = 10,
  historyLimit = RAW_ICP_TRACKER_TRANSFER_LIMIT,
  minTimestampNanos = null,
  onProgress = null,
  onTransfersProgress = null,
} = {}) {
  if (!canisterId) throw new Error('A canister ID is required');
  const { agent: resolvedAgent, historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });
  const progressCallback = typeof onProgress === 'function' ? onProgress : onTransfersProgress;
  const progress = {
    canisterId,
    status: null,
    relayInstances: progressPage(null, true),
    commitments: progressPage(null, true),
    transfers: progressPage(null, true),
    candidates: progressPage(null, true),
    errors: { commitments: null, transfers: null, candidates: null },
  };
  notifyTrackerProgress(progressCallback, progress);

  const statusPromise = trackProgressPromise(historian.get_public_status(), {
    progress,
    key: 'status',
    onProgress: progressCallback,
    page: false,
  });
  const relayInstancesPromise = trackProgressPromise(loadRelayInstances(historian), {
    progress,
    key: 'relayInstances',
    onProgress: progressCallback,
  });
  const commitmentsPromise = trackProgressPromise(loadRawIcpCanisterCommitments(historian, {
    canisterId,
    historyLimit,
    minTimestampNanos,
  }), {
    progress,
    key: 'commitments',
    onProgress: progressCallback,
    errorKey: 'commitments',
  });
  const transfersPromise = trackProgressPromise(statusPromise.then((status) => loadRawIncomingTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    account: { owner: canisterId, subaccount: [] },
    memoText: outgoingMemoText,
    historyLimit,
    minTimestampNanos,
    onProgress: typeof progressCallback === 'function'
      ? (transfers) => {
          progress.transfers = progressPage(transfers, Boolean(transfers?.loading));
          notifyTrackerProgress(progressCallback, progress);
        }
      : null,
  })), {
    progress,
    key: 'transfers',
    onProgress: progressCallback,
    errorKey: 'transfers',
  });
  const prefix = String(outgoingMemoText || '');
  const candidatesPromise = trackProgressPromise(prefix.length >= 4 && typeof historian.find_canisters_by_memo_prefix === 'function'
    ? historian.find_canisters_by_memo_prefix({
        prefix,
        limit: [prefixLimit],
      })
    : Promise.resolve({ items: [], truncated: false }), {
    progress,
    key: 'candidates',
    onProgress: progressCallback,
    errorKey: 'candidates',
  });
  const [statusResult, relayInstancesResult, commitmentsResult, transfersResult, candidatesResult] = await Promise.allSettled([
    statusPromise,
    relayInstancesPromise,
    commitmentsPromise,
    transfersPromise,
    candidatesPromise,
  ]);
  return {
    canisterId,
    status: fulfilledOrNull(statusResult),
    relayInstances: fulfilledOrNull(relayInstancesResult) || { items: [] },
    commitments: fulfilledOrNull(commitmentsResult) || { items: [] },
    transfers: fulfilledOrNull(transfersResult) || { items: [] },
    candidates: fulfilledOrNull(candidatesResult) || { items: [], truncated: false },
    errors: {
      commitments: commitmentsResult.status === 'rejected' ? normalizeError(commitmentsResult.reason) : null,
      transfers: transfersResult.status === 'rejected' ? normalizeError(transfersResult.reason) : null,
      candidates: candidatesResult.status === 'rejected' ? normalizeError(candidatesResult.reason) : null,
    },
  };
}

export async function loadNeuronStakeTrackerData({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  indexActorFactory = createIndexActor,
  governanceActorFactory = createGovernanceActor,
  governanceCanisterId = GOVERNANCE_CANISTER_ID,
  neuronId,
  outgoingMemoText = null,
  historyLimit = RAW_ICP_TRACKER_TRANSFER_LIMIT,
  minTimestampNanos = null,
  onProgress = null,
  onTransfersProgress = null,
} = {}) {
  if (neuronId === null || neuronId === undefined) throw new Error('A neuron ID is required');
  const { agent: resolvedAgent, historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });
  const governance = governanceActorFactory(governanceCanisterId, { agent: resolvedAgent });
  const stakingAccount = await loadPublicNeuronStakingAccount({
    governance,
    neuronId,
    governanceCanisterId,
  });
  const progressCallback = typeof onProgress === 'function' ? onProgress : onTransfersProgress;
  const progress = {
    neuronId,
    stakingAccount,
    status: null,
    commitments: progressPage(null, true),
    transfers: progressPage(null, true),
    errors: { commitments: null, transfers: null },
  };
  notifyTrackerProgress(progressCallback, progress);

  const statusPromise = trackProgressPromise(historian.get_public_status(), {
    progress,
    key: 'status',
    onProgress: progressCallback,
    page: false,
  });
  const commitmentsPromise = trackProgressPromise(loadNeuronCommitments(historian, {
    neuronId,
    historyLimit,
    minTimestampNanos,
  }), {
    progress,
    key: 'commitments',
    onProgress: progressCallback,
    errorKey: 'commitments',
  });
  const transfersPromise = trackProgressPromise(statusPromise.then((status) => loadRawIncomingTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    account: stakingAccount,
    memoText: outgoingMemoText,
    historyLimit,
    minTimestampNanos,
    onProgress: typeof progressCallback === 'function'
      ? (transfers) => {
          progress.transfers = progressPage(transfers, Boolean(transfers?.loading));
          notifyTrackerProgress(progressCallback, progress);
        }
      : null,
  })), {
    progress,
    key: 'transfers',
    onProgress: progressCallback,
    errorKey: 'transfers',
  });
  const [statusResult, commitmentsResult, transfersResult] = await Promise.allSettled([
    statusPromise,
    commitmentsPromise,
    transfersPromise,
  ]);
  return {
    neuronId,
    stakingAccount,
    status: fulfilledOrNull(statusResult),
    commitments: fulfilledOrNull(commitmentsResult) || { items: [] },
    transfers: fulfilledOrNull(transfersResult) || { items: [] },
    errors: {
      commitments: commitmentsResult.status === 'rejected' ? normalizeError(commitmentsResult.reason) : null,
      transfers: transfersResult.status === 'rejected' ? normalizeError(transfersResult.reason) : null,
    },
  };
}
