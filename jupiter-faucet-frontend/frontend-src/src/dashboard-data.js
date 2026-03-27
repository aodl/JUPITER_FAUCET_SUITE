import { HttpAgent } from '@dfinity/agent';
import { sha224 } from '@noble/hashes/sha2.js';
import { createActor as createHistorianActor } from '../declarations/jupiter_historian/index.js';
import { createActor as createLedgerActor } from '../declarations/mock_icrc_ledger/index.js';

export const FRONTEND_HINT = 'Frontend expects the upgraded jupiter_historian canister with public live-metrics query methods.';
export const REGISTERED_SUMMARY_PAGE_SIZE = 100;
export const RECENT_CONTRIBUTION_LIMIT = 100;
export const RECENT_BURN_LIMIT = 100;

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
    data.counts?.icp_burned_e8s === undefined &&
    data.counts?.registered_canister_count === undefined &&
    data.counts?.qualifying_contribution_count === undefined
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

async function getOrCreateAgent({ host, local, agent }) {
  if (agent) return agent;
  const key = `${host}::${local ? 'local' : 'remote'}`;
  if (!agentPromises.has(key)) {
    agentPromises.set(key, (async () => {
      const httpAgent = new HttpAgent({ host });
      if (local) {
        try {
          await httpAgent.fetchRootKey();
        } catch (error) {
          console.warn('Failed to fetch local root key', error);
        }
      }
      return httpAgent;
    })());
  }
  return agentPromises.get(key);
}

export async function loadDashboardData({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
  ledgerActorFactory = createLedgerActor,
} = {}) {
  if (!historianActor && !historianCanisterId) {
    throw new Error('Historian canister ID is not configured for this build');
  }

  const resolvedAgent = await getOrCreateAgent({ host, local, agent });
  const historian = historianActor || historianActorFactory(historianCanisterId, { agent: resolvedAgent });

  const [countsResult, statusResult, registeredResult, recentResult, burnsResult] = await Promise.allSettled([
    historian.get_public_counts(),
    historian.get_public_status(),
    historian.list_registered_canister_summaries({
      page: [0],
      page_size: [REGISTERED_SUMMARY_PAGE_SIZE],
      sort: [{ TotalQualifyingContributedDesc: null }],
    }),
    historian.list_recent_contributions({
      limit: [RECENT_CONTRIBUTION_LIMIT],
      qualifying_only: [false],
    }),
    typeof historian.list_recent_burns === 'function'
      ? historian.list_recent_burns({ limit: [RECENT_BURN_LIMIT] })
      : Promise.resolve({ items: [] }),
  ]);

  let stakeResult = { status: 'rejected', reason: new Error('Stake unavailable') };
  if (statusResult.status === 'fulfilled') {
    const ledger = ledgerActorFactory(statusResult.value.ledger_canister_id.toText(), {
      agent: resolvedAgent,
    });
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

  const errors = {
    counts: countsResult.status === 'rejected' ? normalizeError(countsResult.reason) : null,
    status: statusResult.status === 'rejected' ? normalizeError(statusResult.reason) : null,
    registered: registeredResult.status === 'rejected' ? normalizeError(registeredResult.reason) : null,
    recent: recentResult.status === 'rejected' ? normalizeError(recentResult.reason) : null,
    burns: burnsResult.status === 'rejected' ? normalizeError(burnsResult.reason) : null,
    stake: stakeResult.status === 'rejected' ? normalizeError(stakeResult.reason) : null,
  };

  const historianFailures = [countsResult, statusResult, registeredResult, recentResult, burnsResult].filter((result) => result.status === 'rejected');
  const historianAllRejected = historianFailures.length === 5;
  const historianLikelyOutdated = historianAllRejected && historianFailures.every((result) => isMethodMissingError(result.reason));

  return {
    counts: countsResult.status === 'fulfilled' ? countsResult.value : null,
    status: statusResult.status === 'fulfilled' ? statusResult.value : null,
    registered: registeredResult.status === 'fulfilled' ? registeredResult.value : null,
    recent: recentResult.status === 'fulfilled' ? recentResult.value : null,
    burns: burnsResult.status === 'fulfilled' ? burnsResult.value : null,
    stakeE8s: stakeResult.status === 'fulfilled' ? stakeResult.value : null,
    hasAnyFailure:
      countsResult.status === 'rejected' ||
      statusResult.status === 'rejected' ||
      registeredResult.status === 'rejected' ||
      recentResult.status === 'rejected' ||
      burnsResult.status === 'rejected' ||
      stakeResult.status === 'rejected',
    errors,
    historianAllRejected,
    historianLikelyOutdated,
  };
}
