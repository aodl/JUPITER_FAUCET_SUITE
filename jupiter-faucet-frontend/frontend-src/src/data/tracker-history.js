import { createActor as createIndexActor } from '../../declarations/icp_index/index.js';
import { createActor as createHistorianActor } from '../../declarations/jupiter_historian/index.js';
import { createHistorianClient, normalizeError } from '../app/agent.js';
import { loadCanisterLogs } from './cycles.js';
import { loadCmcTopUpTransfersFromIndex } from './index-transactions.js';
import {
  MAINNET_CMC_CANISTER_ID,
  fulfilledOrNull,
  hasCanisterSource,
  principalToText,
  readOptional,
} from './dashboard-transforms.js';

const TRACKER_HISTORY_PAGE_SIZE = 100;

function buildGetCyclesHistoryArgs({ canisterId, startAfter = null, limit = TRACKER_HISTORY_PAGE_SIZE, descending = false } = {}) {
  return {
    canister_id: canisterId,
    start_after_ts: startAfter === null || startAfter === undefined ? [] : [typeof startAfter === 'bigint' ? startAfter : BigInt(startAfter)],
    limit: [limit],
    descending: [Boolean(descending)],
  };
}

async function loadTrackerCycles(historian, args) {
  const page = await historian.get_cycles_history(args);
  return {
    ...page,
    items: [...(page?.items || [])].sort((left, right) => {
      const leftTs = typeof left.timestamp_nanos === 'bigint' ? left.timestamp_nanos : BigInt(left.timestamp_nanos);
      const rightTs = typeof right.timestamp_nanos === 'bigint' ? right.timestamp_nanos : BigInt(right.timestamp_nanos);
      return leftTs < rightTs ? -1 : leftTs > rightTs ? 1 : 0;
    }),
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

async function loadTrackerCommitments(historian, args) {
  if (typeof historian?.get_commitment_history !== 'function') {
    throw new Error('Historian commitment history query is unavailable');
  }
  return historian.get_commitment_history(args);
}

async function loadTrackerCmcTransfers({ historian, status = null, agent, indexActorFactory, canisterId, cmcCanisterId, historyLimit }) {
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
  });
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
  const isCommitmentBeneficiary = hasCanisterSource(overviewValue?.sources, 'MemoCommitment');

  if (!overviewValue || !isCommitmentBeneficiary) {
    return {
      canisterId,
      overview: overviewValue,
      isRecognized: Boolean(overviewValue),
      isCommitmentBeneficiary,
      commitments: { items: [] },
      cycles: { items: [] },
      logs: { items: [] },
      cmcTransfers: { items: [] },
      errors: { commitments: null, cycles: null, logs: null, cmcTransfers: null },
    };
  }

  const commitmentsPromise = loadTrackerCommitments(historian, buildGetCommitmentHistoryArgs({
    canisterId,
    limit: historyLimit,
    descending: false,
  }));
  const cyclesPromise = loadTrackerCycles(historian, buildGetCyclesHistoryArgs({
    canisterId,
    limit: historyLimit,
    descending: true,
  }));
  const statusPromise = historian.get_public_status();
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
  }));

  const [commitmentsResult, cyclesResult, statusResult, logsResult, cmcTransfersResult] = await Promise.allSettled([
    commitmentsPromise,
    cyclesPromise,
    statusPromise,
    logsPromise,
    cmcTransfersPromise,
  ]);

  return {
    canisterId,
    overview: overviewValue,
    status: fulfilledOrNull(statusResult),
    isRecognized: true,
    isCommitmentBeneficiary: true,
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
