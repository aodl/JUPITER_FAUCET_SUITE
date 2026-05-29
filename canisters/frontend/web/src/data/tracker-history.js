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
  hasCanisterSource,
  principalToText,
  readOptional,
} from './dashboard-transforms.js';

const TRACKER_HISTORY_PAGE_SIZE = 100;
export const RAW_ICP_TRACKER_TRANSFER_LIMIT = 10_000;

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

async function loadRawIncomingTransfers({ historian, status = null, agent, indexActorFactory, account, memoText, historyLimit, onProgress = null }) {
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
    onProgress,
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
  const transfersPromise = statusPromise.then((status) => loadRawIncomingTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    account: { owner: canisterId, subaccount: [] },
    memoText: outgoingMemoText,
    historyLimit,
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
        source_filter: [{ MemoCommitment: null }],
      })
    : Promise.resolve({ items: [], truncated: false });
  const [statusResult, transfersResult, candidatesResult] = await Promise.allSettled([
    statusPromise,
    transfersPromise,
    candidatesPromise,
  ]);
  return {
    canisterId,
    status: fulfilledOrNull(statusResult),
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
