import { Principal } from '@icp-sdk/core/principal';
import {
  MAINNET_CMC_CANISTER_ID,
  RECENT_ROUTE_TRANSFER_LIMIT,
  accountIdentifierHex,
  compareBigIntDesc,
  readOptional,
} from './dashboard-transforms.js';

const RECENT_ROUTE_TRANSFER_PAGE_SIZE = 100;
const RECENT_ROUTE_TRANSFER_MAX_INDEX_PAGES = 10;
const RAW_ICP_TRANSFER_PAGE_SIZE = 100;

function routeTransferTimestampOpt(transaction) {
  const timestamp = readOptional(transaction?.timestamp) || readOptional(transaction?.created_at_time);
  return timestamp?.timestamp_nanos === undefined || timestamp?.timestamp_nanos === null
    ? []
    : [timestamp.timestamp_nanos];
}

function transactionTimestampNanos(transaction) {
  const timestamp = readOptional(transaction?.timestamp) || readOptional(transaction?.created_at_time);
  if (timestamp?.timestamp_nanos === undefined || timestamp?.timestamp_nanos === null) return null;
  return typeof timestamp.timestamp_nanos === 'bigint' ? timestamp.timestamp_nanos : BigInt(timestamp.timestamp_nanos);
}

function itemTimestampNanos(item) {
  const timestamp = readOptional(item?.timestamp_nanos);
  if (timestamp === undefined || timestamp === null) return null;
  return typeof timestamp === 'bigint' ? timestamp : BigInt(timestamp);
}

function isAtOrAfterTimestamp(item, minTimestampNanos) {
  if (minTimestampNanos === null || minTimestampNanos === undefined) return true;
  const timestamp = itemTimestampNanos(item);
  return timestamp !== null && timestamp >= minTimestampNanos;
}

function pageCrossedTimestampCutoff(transactions, minTimestampNanos) {
  if (minTimestampNanos === null || minTimestampNanos === undefined) return false;
  const dated = (transactions || [])
    .map((tx) => transactionTimestampNanos(tx?.transaction))
    .filter((timestamp) => timestamp !== null);
  if (dated.length === 0) return false;
  return dated.reduce((oldest, timestamp) => timestamp < oldest ? timestamp : oldest, dated[0]) < minTimestampNanos;
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

function decodeMemoText(memo) {
  if (memo === undefined || memo === null) return null;
  if (ArrayBuffer.isView(memo)) return decodeMemoText(Array.from(memo));
  if (!Array.isArray(memo)) return null;
  if (memo.length === 0) return '';
  if (!memo.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)) return null;
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from(memo));
  } catch {
    return null;
  }
}

function decodeIcrc1MemoText(memo) {
  if (memo === undefined || memo === null) return null;
  if (!Array.isArray(memo)) return decodeMemoText(memo);
  if (memo.length === 0) return null;
  return decodeMemoText(memo[0]);
}

function routeTransferFromIndexTransaction(tx, expectedFromAccountIdentifier, expectedToAccountIdentifier) {
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

function indexPageLimit(maxPages) {
  const count = Math.max(1, maxPages);
  if (!Number.isSafeInteger(count)) throw new Error('ICP index page count must be finite');
  return count;
}

function nextIndexPageStart(response, start, pageSize) {
  const transactions = response?.transactions;
  if (!Array.isArray(transactions) || transactions.length > pageSize) {
    throw new Error('ICP index returned a malformed page');
  }
  const blockId = (value) => {
    if (!(typeof value === 'bigint'
      || (typeof value === 'number' && Number.isSafeInteger(value))
      || (typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value)))) {
      throw new Error('ICP index returned a malformed page ID');
    }
    const id = BigInt(value);
    if (id < 0n || id > 18_446_744_073_709_551_615n) throw new Error('ICP index returned a malformed page ID');
    return id;
  };
  let previous = start;
  for (const [position, tx] of transactions.entries()) {
    const id = blockId(tx?.id);
    if (previous !== null && (id > previous || (position === 0 && id === previous))) {
      throw new Error('ICP index page did not advance in descending exclusive order');
    }
    previous = id;
  }
  const oldestValue = readOptional(response.oldest_tx_id);
  const oldest = oldestValue === null || oldestValue === undefined ? null : blockId(oldestValue);
  if (transactions.length === 0) {
    if (oldest !== null && (start === null || start > oldest)) {
      throw new Error('ICP index returned an incomplete empty page');
    }
    return null;
  }
  if (oldest !== null && previous < oldest) throw new Error('ICP index page precedes its oldest transaction');
  if (previous === 0n || previous === oldest || (oldest === null && transactions.length < pageSize)) return null;
  // The Index start is exclusive: subtracting one would skip an adjacent block.
  return previous;
}

async function getAccountIdentifierTransactions(index, accountIdentifier, start, maxResults) {
  if (!Number.isSafeInteger(maxResults) || maxResults <= 0) throw new Error('ICP index page size must be positive');
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
  const pageLimit = indexPageLimit(maxPages);

  for (let page = 0; page < pageLimit && items.length < limit; page += 1) {
    const response = await getAccountIdentifierTransactions(index, routeAccountIdentifier, start, pageSize);
    const nextStart = nextIndexPageStart(response, start, pageSize);
    const transactions = response.transactions;
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

    if (nextStart === null) break;
    start = nextStart;
  }

  return { items: normalizeRouteTransferItems(items, limit) };
}

function cmcDepositSubaccount(canisterId) {
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
  if (!canisterId) throw new Error('A declared canister ID is required');
  const owner = typeof cmcCanisterId === 'string' ? Principal.fromText(cmcCanisterId) : cmcCanisterId;
  return {
    owner,
    subaccount: [Array.from(cmcDepositSubaccount(canisterId))],
  };
}

function cmcTopUpTransferFromIndexTransaction(tx, expectedToAccountIdentifier) {
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
    timestamp_nanos: routeTransferTimestampOpt(tx.transaction),
    amount_e8s: amount,
    from_account_identifier: String(transfer.from || ''),
  };
}

function incomingIcpTransferFromIndexTransaction(tx, expectedToAccountIdentifier, sourceIds, memoText) {
  const transfer = transferOperation(tx?.transaction?.operation);
  if (!transfer) return null;

  const from = String(transfer.from || '').toLowerCase();
  const to = String(transfer.to || '').toLowerCase();
  if (to !== expectedToAccountIdentifier) return null;

  const amount = tokenE8s(transfer.amount);
  if (amount === null || tx?.id === undefined || tx?.id === null) return null;

  const icrc1MemoText = decodeIcrc1MemoText(tx?.transaction?.icrc1_memo);
  return {
    tx_id: tx.id,
    timestamp_nanos: routeTransferTimestampOpt(tx.transaction),
    amount_e8s: amount,
    from_account_identifier: String(transfer.from || ''),
    to_account_identifier: String(transfer.to || ''),
    icrc1_memo_text: icrc1MemoText,
    is_matching_source: sourceIds.size > 0 && sourceIds.has(from),
    is_matching_memo: memoText !== null && icrc1MemoText === memoText,
  };
}

export async function loadCmcTopUpTransfersFromIndex({
  index,
  canisterId,
  cmcCanisterId = MAINNET_CMC_CANISTER_ID,
  limit = RECENT_ROUTE_TRANSFER_LIMIT,
  pageSize = RECENT_ROUTE_TRANSFER_PAGE_SIZE,
  maxPages = Math.ceil(Math.max(1, limit) / Math.max(1, pageSize)),
  minTimestampNanos = null,
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
  const pageLimit = indexPageLimit(maxPages);

  for (let page = 0; page < pageLimit && items.length < limit; page += 1) {
    const response = await getAccountIdentifierTransactions(index, depositAccountIdentifier, start, pageSize);
    const nextStart = nextIndexPageStart(response, start, pageSize);
    const transactions = response.transactions;
    for (const tx of transactions) {
      const item = cmcTopUpTransferFromIndexTransaction(tx, depositAccountIdentifier);
      if (item && isAtOrAfterTimestamp(item, minTimestampNanos)) {
        const key = String(item.tx_id);
        if (!seen.has(key)) {
          seen.add(key);
          items.push(item);
        }
      }
      if (items.length >= limit) break;
    }

    if (pageCrossedTimestampCutoff(transactions, minTimestampNanos)) break;
    if (nextStart === null) break;
    start = nextStart;
  }

  return { items: normalizeRouteTransferItems(items, limit) };
}

export async function loadIncomingIcpTransfersFromIndex({
  index,
  account,
  limit = RECENT_ROUTE_TRANSFER_LIMIT,
  pageSize = RAW_ICP_TRANSFER_PAGE_SIZE,
  maxPages = Math.ceil(Math.max(1, limit) / Math.max(1, pageSize)),
  sourceAccountIdentifiers = [],
  memoText = null,
  minTimestampNanos = null,
  onProgress = null,
} = {}) {
  if (!index || typeof index.get_account_identifier_transactions !== 'function') {
    throw new Error('ICP index actor is unavailable');
  }
  if (!account) return { items: [] };

  const accountIdentifier = accountIdentifierHex(account).toLowerCase();
  const sourceIds = new Set((sourceAccountIdentifiers || []).map((value) => String(value || '').toLowerCase()).filter(Boolean));
  const exactMemo = memoText === null || memoText === undefined ? null : String(memoText);
  const items = [];
  const seen = new Set();
  let start = null;
  const pageLimit = indexPageLimit(maxPages);
  let truncated = false;
  const notifyProgress = async ({ loading, pagesLoaded }) => {
    if (typeof onProgress !== 'function') return;
    await onProgress({
      items: normalizeRouteTransferItems(items, limit),
      truncated,
      limit,
      page_size: pageSize,
      loading,
      pages_loaded: pagesLoaded,
    });
  };

  for (let page = 0; page < pageLimit && items.length < limit; page += 1) {
    const response = await getAccountIdentifierTransactions(index, accountIdentifier, start, pageSize);
    const nextStart = nextIndexPageStart(response, start, pageSize);
    const transactions = response.transactions;
    const reachedTimeWindowEnd = pageCrossedTimestampCutoff(transactions, minTimestampNanos);
    let additionalMatches = false;
    for (const tx of transactions) {
      const item = incomingIcpTransferFromIndexTransaction(tx, accountIdentifier, sourceIds, exactMemo);
      if (item && isAtOrAfterTimestamp(item, minTimestampNanos)) {
        const key = String(item.tx_id);
        if (!seen.has(key)) {
          seen.add(key);
          if (items.length < limit) items.push(item);
          else additionalMatches = true;
        }
      }
    }

    const moreHistory = nextStart !== null && !reachedTimeWindowEnd;
    truncated = additionalMatches || (moreHistory && (items.length >= limit || page + 1 >= pageLimit));
    const loading = moreHistory && !truncated;
    await notifyProgress({ loading, pagesLoaded: page + 1 });
    if (!loading) break;
    start = nextStart;
  }

  return { items: normalizeRouteTransferItems(items, limit), truncated, limit, page_size: pageSize, loading: false };
}
