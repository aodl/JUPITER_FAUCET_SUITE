import { HttpAgent } from '@icp-sdk/core/agent';
import { Principal } from '@icp-sdk/core/principal';
import { sha224 } from '@noble/hashes/sha2.js';
import { createActor as createHistorianActor } from '../declarations/jupiter_historian/index.js';
import { createActor as createLedgerActor } from '../declarations/icp_ledger/index.js';
import { createActor as createIndexActor } from '../declarations/icp_index/index.js';

export const FRONTEND_HINT = 'Frontend expects the upgraded jupiter_historian canister with the public dashboard query methods.';
export const REGISTERED_SUMMARY_PAGE_SIZE = 100;
export const RECENT_COMMITMENT_LIMIT = 100;
export const RECENT_ROUTE_TRANSFER_LIMIT = 100;
export const RECENT_ROUTE_TRANSFER_PAGE_SIZE = 100;
export const RECENT_ROUTE_TRANSFER_MAX_INDEX_PAGES = 10;
export const TRACKER_HISTORY_PAGE_SIZE = 100;
export const MAINNET_CMC_CANISTER_ID = 'rkp4c-7iaaa-aaaaa-aaaca-cai';

const agentPromises = new Map();

export function normalizeError(error) {
  if (!error) return 'Unknown error';
  if (typeof error === 'string') return error;
  return error.message || String(error);
}

export function isMethodMissingError(error) {
  const text = normalizeError(error).toLowerCase();
  return text.includes('method') && (text.includes('not found') || text.includes('not part of the service'));
}

export function summaryMetricsUnavailable(data) {
  return (
    data.stakeE8s === null &&
    data.counts?.total_output_e8s === undefined &&
    data.counts?.total_rewards_e8s === undefined &&
    data.counts?.registered_canister_count === undefined &&
    data.counts?.qualifying_commitment_count === undefined
  );
}

export function uint8ArrayFromOptBytes(optBytes) {
  if (!Array.isArray(optBytes) || optBytes.length === 0 || !optBytes[0]) {
    return new Uint8Array(32);
  }
  return Uint8Array.from(optBytes[0]);
}

function concatBytes(...parts) {
  const size = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(size);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

const CRC32_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i += 1) {
    let value = i;
    for (let bit = 0; bit < 8; bit += 1) {
      value = (value & 1) !== 0 ? (0xedb88320 ^ (value >>> 1)) : (value >>> 1);
    }
    table[i] = value >>> 0;
  }
  return table;
})();

function crc32(bytes) {
  let value = 0xffffffff;
  for (const byte of bytes) {
    value = CRC32_TABLE[(value ^ byte) & 0xff] ^ (value >>> 8);
  }
  return (value ^ 0xffffffff) >>> 0;
}

export function bytesToHex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function accountIdentifierBytes(account) {
  const domainSeparator = new TextEncoder().encode('\x0Aaccount-id');
  const ownerBytes = account.owner.toUint8Array();
  const subaccount = uint8ArrayFromOptBytes(account.subaccount);
  const hash = sha224(concatBytes(domainSeparator, ownerBytes, subaccount));
  const checksum = crc32(hash);
  const checksumBytes = new Uint8Array([
    (checksum >>> 24) & 0xff,
    (checksum >>> 16) & 0xff,
    (checksum >>> 8) & 0xff,
    checksum & 0xff,
  ]);
  return concatBytes(checksumBytes, hash);
}

export function accountIdentifierHex(account) {
  return bytesToHex(accountIdentifierBytes(account));
}

function buildRegisteredCanisterSummariesRequest({ page = 0, pageSize = REGISTERED_SUMMARY_PAGE_SIZE } = {}) {
  return {
    page: [page],
    page_size: [pageSize],
  };
}

function readOptional(value) {
  if (Array.isArray(value)) {
    return value.length > 0 ? value[0] : null;
  }
  return value ?? null;
}

function principalToText(value) {
  const principal = readOptional(value);
  if (!principal) return '';
  return typeof principal.toText === 'function' ? principal.toText() : String(principal);
}

function compareBigIntDesc(left, right) {
  const a = typeof left === 'bigint' ? left : BigInt(left);
  const b = typeof right === 'bigint' ? right : BigInt(right);
  if (a === b) return 0;
  return a > b ? -1 : 1;
}

function routeTransferTimestampOpt(transaction) {
  const timestamp = readOptional(transaction?.timestamp) || readOptional(transaction?.created_at_time);
  return timestamp?.timestamp_nanos === undefined || timestamp?.timestamp_nanos === null
    ? []
    : [timestamp.timestamp_nanos];
}

function tokenE8s(tokens) {
  if (tokens?.e8s === undefined || tokens?.e8s === null) return null;
  return tokens.e8s;
}

function transferOperation(operation) {
  if (!operation || Array.isArray(operation)) return null;
  if (Object.prototype.hasOwnProperty.call(operation, 'Transfer')) return operation.Transfer;
  if (Object.prototype.hasOwnProperty.call(operation, 'TransferFrom')) return operation.TransferFrom;
  return null;
}

export function routeTransferFromIndexTransaction(tx, expectedFromAccountIdentifier, expectedToAccountIdentifier) {
  const transfer = transferOperation(tx?.transaction?.operation);
  if (!transfer) return null;

  const from = String(transfer.from || '').toLowerCase();
  const to = String(transfer.to || '').toLowerCase();
  if (from !== expectedFromAccountIdentifier || to !== expectedToAccountIdentifier) {
    return null;
  }

  const amount = tokenE8s(transfer.amount);
  if (amount === null || tx?.id === undefined || tx?.id === null) return null;

  return {
    tx_id: tx.id,
    timestamp_nanos: routeTransferTimestampOpt(tx.transaction),
    amount_e8s: amount,
  };
}

function normalizeRouteTransferItems(items, limit) {
  const seen = new Set();
  const unique = [];
  for (const item of items) {
    const key = String(item.tx_id);
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(item);
  }
  unique.sort((a, b) => compareBigIntDesc(a.tx_id, b.tx_id));
  return unique.slice(0, limit);
}

async function getAccountIdentifierTransactions(index, accountIdentifier, start, maxResults) {
  const result = await index.get_account_identifier_transactions({
    account_identifier: accountIdentifier,
    start: start === null || start === undefined ? [] : [typeof start === 'bigint' ? start : BigInt(start)],
    max_results: BigInt(maxResults),
  });
  if (result && Object.prototype.hasOwnProperty.call(result, 'Ok')) return result.Ok;
  if (result && Object.prototype.hasOwnProperty.call(result, 'Err')) {
    throw new Error(result.Err?.message || 'ICP index returned an error');
  }
  throw new Error('ICP index returned an unexpected response');
}

export async function loadRecentRouteTransfersFromIndex({
  index,
  outputSourceAccount,
  routeAccount,
  limit = RECENT_ROUTE_TRANSFER_LIMIT,
  pageSize = RECENT_ROUTE_TRANSFER_PAGE_SIZE,
  maxPages = RECENT_ROUTE_TRANSFER_MAX_INDEX_PAGES,
} = {}) {
  if (!index || typeof index.get_account_identifier_transactions !== 'function') {
    throw new Error('ICP index actor is unavailable');
  }
  if (!outputSourceAccount || !routeAccount) {
    return { items: [] };
  }

  const sourceAccountIdentifier = accountIdentifierHex(outputSourceAccount).toLowerCase();
  const routeAccountIdentifier = accountIdentifierHex(routeAccount).toLowerCase();
  const items = [];
  const seen = new Set();
  let start = null;

  for (let page = 0; page < Math.max(1, maxPages) && items.length < limit; page += 1) {
    const response = await getAccountIdentifierTransactions(index, routeAccountIdentifier, start, pageSize);
    const transactions = response?.transactions || [];
    for (const tx of transactions) {
      const item = routeTransferFromIndexTransaction(tx, sourceAccountIdentifier, routeAccountIdentifier);
      if (item) {
        const key = String(item.tx_id);
        if (!seen.has(key)) {
          seen.add(key);
          items.push(item);
        }
      }
      if (items.length >= limit) break;
    }

    if (transactions.length < pageSize) break;
    const lastId = transactions[transactions.length - 1]?.id;
    if (lastId === undefined || lastId === null) break;

    const lastIdBigInt = typeof lastId === 'bigint' ? lastId : BigInt(lastId);
    if (lastIdBigInt === 0n) break;

    const nextStart = lastIdBigInt - 1n;
    if (start !== null && nextStart >= BigInt(start)) break;
    start = nextStart;
  }

  return { items: normalizeRouteTransferItems(items, limit) };
}

export function resetAgentCacheForTests() {
  agentPromises.clear();
}

async function getOrCreateAgent({ host, local, agent }) {
  if (agent) return agent;
  const key = `${host}::${local ? 'local' : 'remote'}`;
  if (!agentPromises.has(key)) {
    const agentPromise = (async () => {
      const httpAgent = await HttpAgent.create({
        host,
        verifyQuerySignatures: true,
      });
      if (local) {
        try {
          await httpAgent.fetchRootKey();
        } catch (error) {
          console.warn('Failed to fetch local root key', error);
        }
      }
      return httpAgent;
    })();
    agentPromise.catch(() => {
      if (agentPromises.get(key) === agentPromise) {
        agentPromises.delete(key);
      }
    });
    agentPromises.set(key, agentPromise);
  }
  return agentPromises.get(key);
}

async function createHistorianClient({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
} = {}) {
  if (!historianActor && !historianCanisterId) {
    throw new Error('Historian canister ID is not configured for this build');
  }

  const resolvedAgent = await getOrCreateAgent({ host, local, agent });

  try {
    return {
      agent: resolvedAgent,
      historian: historianActor || historianActorFactory(historianCanisterId, { agent: resolvedAgent }),
    };
  } catch (error) {
    throw new Error(normalizeError(error));
  }
}


function buildGetCyclesHistoryArgs({ canisterId, startAfter = null, limit = TRACKER_HISTORY_PAGE_SIZE, descending = false } = {}) {
  return {
    canister_id: canisterId,
    start_after_ts: startAfter === null || startAfter === undefined ? [] : [typeof startAfter === 'bigint' ? startAfter : BigInt(startAfter)],
    limit: [limit],
    descending: [Boolean(descending)],
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

function variantName(value) {
  if (!value || Array.isArray(value) || typeof value !== 'object') return '';
  return Object.keys(value)[0] || '';
}

export function hasCanisterSource(sources, sourceName) {
  return Array.isArray(sources) && sources.some((source) => variantName(source) === sourceName);
}

function fulfilledOrNull(result) {
  return result.status === 'fulfilled' ? result.value : null;
}

async function loadTrackerCommitments(historian, args) {
  if (typeof historian?.get_commitment_history !== 'function') {
    throw new Error('Historian commitment history query is unavailable');
  }
  return historian.get_commitment_history(args);
}


export function cmcDepositSubaccount(canisterId) {
  const principalBytes = canisterId.toUint8Array();
  if (principalBytes.length > 31) {
    throw new Error('Principal is too long for a CMC top-up subaccount');
  }
  const subaccount = new Uint8Array(32);
  subaccount[0] = principalBytes.length;
  subaccount.set(principalBytes, 1);
  return subaccount;
}

export function cmcDepositAccount({ canisterId, cmcCanisterId = MAINNET_CMC_CANISTER_ID } = {}) {
  if (!canisterId) throw new Error('A target canister principal is required');
  const owner = typeof cmcCanisterId === 'string' ? Principal.fromText(cmcCanisterId) : cmcCanisterId;
  return {
    owner,
    subaccount: [Array.from(cmcDepositSubaccount(canisterId))],
  };
}

function cmcTopUpTransferTimestampOpt(transaction) {
  const timestamp = readOptional(transaction?.timestamp) || readOptional(transaction?.created_at_time);
  return timestamp?.timestamp_nanos === undefined || timestamp?.timestamp_nanos === null
    ? []
    : [timestamp.timestamp_nanos];
}

export function cmcTopUpTransferFromIndexTransaction(tx, expectedToAccountIdentifier) {
  const transfer = transferOperation(tx?.transaction?.operation);
  if (!transfer) return null;

  const to = String(transfer.to || '').toLowerCase();
  if (to !== expectedToAccountIdentifier) {
    return null;
  }

  const amount = tokenE8s(transfer.amount);
  if (amount === null || tx?.id === undefined || tx?.id === null) return null;

  return {
    tx_id: tx.id,
    timestamp_nanos: cmcTopUpTransferTimestampOpt(tx.transaction),
    amount_e8s: amount,
    from_account_identifier: String(transfer.from || ''),
  };
}

export async function loadCmcTopUpTransfersFromIndex({
  index,
  canisterId,
  cmcCanisterId = MAINNET_CMC_CANISTER_ID,
  limit = RECENT_ROUTE_TRANSFER_LIMIT,
  pageSize = RECENT_ROUTE_TRANSFER_PAGE_SIZE,
  maxPages = RECENT_ROUTE_TRANSFER_MAX_INDEX_PAGES,
} = {}) {
  if (!index || typeof index.get_account_identifier_transactions !== 'function') {
    throw new Error('ICP index actor is unavailable');
  }
  if (!canisterId) {
    return { items: [] };
  }

  const depositAccountIdentifier = accountIdentifierHex(cmcDepositAccount({ canisterId, cmcCanisterId })).toLowerCase();
  const items = [];
  const seen = new Set();
  let start = null;

  for (let page = 0; page < Math.max(1, maxPages) && items.length < limit; page += 1) {
    const response = await getAccountIdentifierTransactions(index, depositAccountIdentifier, start, pageSize);
    const transactions = response?.transactions || [];
    for (const tx of transactions) {
      const item = cmcTopUpTransferFromIndexTransaction(tx, depositAccountIdentifier);
      if (item) {
        const key = String(item.tx_id);
        if (!seen.has(key)) {
          seen.add(key);
          items.push(item);
        }
      }
      if (items.length >= limit) break;
    }

    if (transactions.length < pageSize) break;
    const lastId = transactions[transactions.length - 1]?.id;
    if (lastId === undefined || lastId === null) break;

    const lastIdBigInt = typeof lastId === 'bigint' ? lastId : BigInt(lastId);
    if (lastIdBigInt === 0n) break;

    const nextStart = lastIdBigInt - 1n;
    if (start !== null && nextStart >= BigInt(start)) break;
    start = nextStart;
  }

  return { items: normalizeRouteTransferItems(items, limit) };
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
  canisterId,
  cmcCanisterId = null,
  historyLimit = TRACKER_HISTORY_PAGE_SIZE,
} = {}) {
  if (!canisterId) {
    throw new Error('A principal ID is required');
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
      cmcTransfers: { items: [] },
      errors: { commitments: null, cycles: null, cmcTransfers: null },
    };
  }

  const commitmentsPromise = loadTrackerCommitments(historian, buildGetCommitmentHistoryArgs({
    canisterId,
    limit: historyLimit,
    descending: false,
  }));
  const cyclesPromise = historian.get_cycles_history(buildGetCyclesHistoryArgs({
    canisterId,
    limit: historyLimit,
    descending: false,
  }));
  const statusPromise = historian.get_public_status();
  const cmcTransfersPromise = statusPromise.then((status) => loadTrackerCmcTransfers({
    historian,
    status,
    agent: resolvedAgent,
    indexActorFactory,
    canisterId,
    cmcCanisterId,
    historyLimit,
  }));

  const [commitmentsResult, cyclesResult, statusResult, cmcTransfersResult] = await Promise.allSettled([
    commitmentsPromise,
    cyclesPromise,
    statusPromise,
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
    cmcTransfers: fulfilledOrNull(cmcTransfersResult) || { items: [] },
    errors: {
      commitments: commitmentsResult.status === 'rejected' ? normalizeError(commitmentsResult.reason) : null,
      cycles: cyclesResult.status === 'rejected' ? normalizeError(cyclesResult.reason) : null,
      cmcTransfers: cmcTransfersResult.status === 'rejected' ? normalizeError(cmcTransfersResult.reason) : null,
    },
  };
}

export async function loadRegisteredCanisterSummaryPage({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  page = 0,
  pageSize = REGISTERED_SUMMARY_PAGE_SIZE,
} = {}) {
  const { historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });
  return historian.list_registered_canister_summaries(
    buildRegisteredCanisterSummariesRequest({ page, pageSize }),
  );
}


export async function loadCanisterModuleHashes({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
} = {}) {
  const { historian } = await createHistorianClient({
    historianCanisterId,
    host,
    local,
    agent,
    historianActor,
    historianActorFactory,
  });
  return historian.get_canister_module_hashes();
}

async function loadRouteTransferTables({ status, agent, indexActorFactory }) {
  const indexCanisterId = principalToText(status?.index_canister_id);
  const outputSourceAccount = readOptional(status?.output_source_account);
  const outputAccount = readOptional(status?.output_account);
  const rewardsAccount = readOptional(status?.rewards_account);

  if (!indexCanisterId || !outputSourceAccount || !outputAccount || !rewardsAccount) {
    return {
      outputTransfersResult: { status: 'fulfilled', value: { items: [] } },
      rewardsTransfersResult: { status: 'fulfilled', value: { items: [] } },
    };
  }

  const index = indexActorFactory(indexCanisterId, { agent });
  const [outputTransfersResult, rewardsTransfersResult] = await Promise.allSettled([
    loadRecentRouteTransfersFromIndex({
      index,
      outputSourceAccount,
      routeAccount: outputAccount,
    }),
    loadRecentRouteTransfersFromIndex({
      index,
      outputSourceAccount,
      routeAccount: rewardsAccount,
    }),
  ]);

  return { outputTransfersResult, rewardsTransfersResult };
}

export async function loadDashboardData({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  ledgerActorFactory = createLedgerActor,
  indexActorFactory = createIndexActor,
  registeredPage = 0,
  registeredPageSize = REGISTERED_SUMMARY_PAGE_SIZE,
} = {}) {
  let resolvedAgent;
  let historian;
  try {
    ({ agent: resolvedAgent, historian } = await createHistorianClient({
      historianCanisterId,
      host,
      local,
      agent,
      historianActor,
      historianActorFactory,
    }));
  } catch (error) {
    const reason = normalizeError(error);
    return {
      counts: null,
      status: null,
      registered: null,
      recent: null,
      outputTransfers: null,
      rewardsTransfers: null,
      stakeE8s: null,
      hasAnyFailure: true,
      errors: {
        counts: reason,
        status: reason,
        registered: reason,
        recent: reason,
        outputTransfers: reason,
        rewardsTransfers: reason,
        stake: 'Stake unavailable',
      },
      historianAllRejected: true,
      historianLikelyOutdated: isMethodMissingError(error),
    };
  }

  const [countsResult, statusResult, registeredResult, recentResult] = await Promise.allSettled([
    historian.get_public_counts(),
    historian.get_public_status(),
    historian.list_registered_canister_summaries(
      buildRegisteredCanisterSummariesRequest({
        page: registeredPage,
        pageSize: registeredPageSize,
      }),
    ),
    historian.list_recent_commitments({
      limit: [RECENT_COMMITMENT_LIMIT],
      qualifying_only: [false],
    }),
  ]);

  let stakeResult = { status: 'rejected', reason: new Error('Stake unavailable') };
  let outputTransfersResult = { status: 'fulfilled', value: { items: [] } };
  let rewardsTransfersResult = { status: 'fulfilled', value: { items: [] } };

  if (statusResult.status === 'fulfilled') {
    let ledger;
    try {
      ledger = ledgerActorFactory(statusResult.value.ledger_canister_id.toText(), {
        agent: resolvedAgent,
      });
    } catch (error) {
      ledger = null;
      stakeResult = { status: 'rejected', reason: error };
    }
    if (ledger) {
      const stakingAccount = statusResult.value.staking_account;
      const stakeAccountId = accountIdentifierBytes(stakingAccount);
      stakeResult = await ledger
        .account_balance({ account: stakeAccountId })
        .then((value) => ({ status: 'fulfilled', value: value.e8s }))
        .catch(async (nativeReason) => {
          try {
            const fallbackValue = await ledger.icrc1_balance_of(stakingAccount);
            return { status: 'fulfilled', value: fallbackValue };
          } catch (icrcReason) {
            return {
              status: 'rejected',
              reason: new Error(`Stake unavailable via native ledger account_balance (${normalizeError(nativeReason)}) and icrc1_balance_of (${normalizeError(icrcReason)})`),
            };
          }
        });
    }

    try {
      ({ outputTransfersResult, rewardsTransfersResult } = await loadRouteTransferTables({
        status: statusResult.value,
        agent: resolvedAgent,
        indexActorFactory,
      }));
    } catch (error) {
      outputTransfersResult = { status: 'rejected', reason: error };
      rewardsTransfersResult = { status: 'rejected', reason: error };
    }
  }

  const countsValue = countsResult.status === 'fulfilled' ? countsResult.value : null;

  const errors = {
    counts: countsResult.status === 'rejected' ? normalizeError(countsResult.reason) : null,
    status: statusResult.status === 'rejected' ? normalizeError(statusResult.reason) : null,
    registered: registeredResult.status === 'rejected' ? normalizeError(registeredResult.reason) : null,
    recent: recentResult.status === 'rejected' ? normalizeError(recentResult.reason) : null,
    outputTransfers: outputTransfersResult.status === 'rejected' ? normalizeError(outputTransfersResult.reason) : null,
    rewardsTransfers: rewardsTransfersResult.status === 'rejected' ? normalizeError(rewardsTransfersResult.reason) : null,
    stake: stakeResult.status === 'rejected' ? normalizeError(stakeResult.reason) : null,
  };

  const historianFailures = [countsResult, statusResult, registeredResult, recentResult].filter((result) => result.status === 'rejected');
  const historianAllRejected = historianFailures.length === 4;
  const historianLikelyOutdated = historianAllRejected && historianFailures.every((result) => isMethodMissingError(result.reason));

  return {
    counts: countsValue,
    status: statusResult.status === 'fulfilled' ? statusResult.value : null,
    registered: registeredResult.status === 'fulfilled' ? registeredResult.value : null,
    recent: recentResult.status === 'fulfilled' ? recentResult.value : null,
    outputTransfers: outputTransfersResult.status === 'fulfilled' ? outputTransfersResult.value : null,
    rewardsTransfers: rewardsTransfersResult.status === 'fulfilled' ? rewardsTransfersResult.value : null,
    stakeE8s: stakeResult.status === 'fulfilled' ? stakeResult.value : null,
    hasAnyFailure:
      countsResult.status === 'rejected' ||
      statusResult.status === 'rejected' ||
      registeredResult.status === 'rejected' ||
      recentResult.status === 'rejected' ||
      stakeResult.status === 'rejected',
    errors,
    historianAllRejected,
    historianLikelyOutdated,
  };
}
