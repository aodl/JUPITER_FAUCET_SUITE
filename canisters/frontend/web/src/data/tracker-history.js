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

async function loadTrackerCommitments(historian, { canisterId, historyLimit, minTimestampNanos = null }) {
  if (typeof historian?.get_commitment_history !== 'function') {
    throw new Error('Historian commitment history query is unavailable');
  }
  const limit = positiveLimit(historyLimit);
  const items = [];
  let startAfter = null;
  let nextStartAfter = null;
  while (items.length < limit) {
    const page = await historian.get_commitment_history(buildGetCommitmentHistoryArgs({
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

async function loadRelayRegistrations(historian) {
  if (typeof historian?.list_relay_registrations !== 'function') return { items: [] };
  try {
    const items = [];
    let startAfter = [];
    for (let pageIndex = 0; pageIndex < 50; pageIndex += 1) {
      const page = await historian.list_relay_registrations({
        start_after: startAfter,
        limit: [200],
      });
      items.push(...(page?.items || []));
      const next = readOptional(page?.next_start_after);
      if (!next) break;
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

  const commitmentsPromise = isCommitmentBeneficiary
    ? loadTrackerCommitments(historian, {
        canisterId,
        historyLimit,
        minTimestampNanos,
      })
    : Promise.resolve({ items: [] });
  const cyclesPromise = loadTrackerCycles(historian, {
    canisterId,
    historyLimit,
    minTimestampNanos,
  });
  const statusPromise = historian.get_public_status();
  const relayRegistrationsPromise = loadRelayRegistrations(historian);
  const logsPromise = canisterLogsLoader({
    agent: resolvedAgent,
    canisterId,
  });
  const cmcTransfersPromise = statusPromise.then((status) => loadTrackerCmcTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    canisterId,
    cmcCanisterId,
    historyLimit,
    minTimestampNanos,
  }));

  const [commitmentsResult, cyclesResult, statusResult, relayRegistrationsResult, logsResult, cmcTransfersResult] = await Promise.allSettled([
    commitmentsPromise,
    cyclesPromise,
    statusPromise,
    relayRegistrationsPromise,
    logsPromise,
    cmcTransfersPromise,
  ]);

  return {
    canisterId,
    overview: overviewValue,
    status: fulfilledOrNull(statusResult),
    relayRegistrations: fulfilledOrNull(relayRegistrationsResult) || { items: [] },
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
  const statusPromise = historian.get_public_status();
  const relayRegistrationsPromise = loadRelayRegistrations(historian);
  const transfersPromise = statusPromise.then((status) => loadRawIncomingTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    account: { owner: canisterId, subaccount: [] },
    memoText: outgoingMemoText,
    historyLimit,
    minTimestampNanos,
    onProgress: typeof onTransfersProgress === 'function'
      ? (transfers) => onTransfersProgress({
          canisterId,
          status,
          transfers,
          candidates: { items: [], truncated: false, loading: true },
          errors: { transfers: null, candidates: null },
        })
      : null,
  }));
  const prefix = String(outgoingMemoText || '');
  const candidatesPromise = prefix.length >= 4 && typeof historian.find_canisters_by_memo_prefix === 'function'
    ? historian.find_canisters_by_memo_prefix({
        prefix,
        limit: [prefixLimit],
      })
    : Promise.resolve({ items: [], truncated: false });
  const [statusResult, relayRegistrationsResult, transfersResult, candidatesResult] = await Promise.allSettled([
    statusPromise,
    relayRegistrationsPromise,
    transfersPromise,
    candidatesPromise,
  ]);
  return {
    canisterId,
    status: fulfilledOrNull(statusResult),
    relayRegistrations: fulfilledOrNull(relayRegistrationsResult) || { items: [] },
    transfers: fulfilledOrNull(transfersResult) || { items: [] },
    candidates: fulfilledOrNull(candidatesResult) || { items: [], truncated: false },
    errors: {
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
  const status = await historian.get_public_status();
  const transfers = await loadRawIncomingTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    account: stakingAccount,
    memoText: outgoingMemoText,
    historyLimit,
    minTimestampNanos,
    onProgress: typeof onTransfersProgress === 'function'
      ? (progressTransfers) => onTransfersProgress({
          neuronId,
          stakingAccount,
          status,
          transfers: progressTransfers,
          errors: { transfers: null },
        })
      : null,
  });
  return {
    neuronId,
    stakingAccount,
    status,
    transfers,
    errors: { transfers: null },
  };
}
