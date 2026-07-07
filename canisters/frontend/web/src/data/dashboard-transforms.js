import { Principal } from '@icp-sdk/core/principal';
import { sha224, sha256 } from '@noble/hashes/sha2.js';
import {
  DQUORUM_STAKING_ACCOUNT_SUBACCOUNT_HEX,
  GOVERNANCE_CANISTER_ID,
  MAINNET_CMC_CANISTER_ID,
} from '../app/config.js';

export const FRONTEND_HINT = 'Frontend expects the upgraded jupiter_historian canister with the public dashboard query methods.';
export const REGISTERED_SUMMARY_PAGE_SIZE = 100;
export const RECENT_COMMITMENT_LIMIT = 100;
export const RECENT_ROUTE_TRANSFER_LIMIT = 100;
export { MAINNET_CMC_CANISTER_ID };

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

export function hexToBytes(hex) {
  const text = String(hex || '').trim();
  if (text.length % 2 !== 0 || /[^0-9a-f]/i.test(text)) {
    throw new Error('Invalid hex byte string');
  }
  const bytes = new Uint8Array(text.length / 2);
  for (let index = 0; index < text.length; index += 2) {
    bytes[index / 2] = Number.parseInt(text.slice(index, index + 2), 16);
  }
  return bytes;
}

export function dquorumStakingAccount(governanceCanisterId = GOVERNANCE_CANISTER_ID) {
  const owner = typeof governanceCanisterId === 'string' ? Principal.fromText(governanceCanisterId) : governanceCanisterId;
  return {
    owner,
    subaccount: [Array.from(hexToBytes(DQUORUM_STAKING_ACCOUNT_SUBACCOUNT_HEX))],
  };
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

export function relaySetupSubaccount(targetPrincipal) {
  const target = typeof targetPrincipal === 'string' ? Principal.fromText(targetPrincipal) : targetPrincipal;
  const domain = new TextEncoder().encode('jupiter-relay-setup-v1');
  return sha256(concatBytes(domain, target.toUint8Array()));
}

export function relaySetupAccount({ historianCanisterId, targetCanisterId }) {
  const owner = typeof historianCanisterId === 'string' ? Principal.fromText(historianCanisterId) : historianCanisterId;
  return {
    owner,
    subaccount: [Array.from(relaySetupSubaccount(targetCanisterId))],
  };
}

export function buildRegisteredCanisterSummariesRequest({ page = 0, pageSize = REGISTERED_SUMMARY_PAGE_SIZE } = {}) {
  return {
    page: [page],
    page_size: [pageSize],
  };
}

export function readOptional(value) {
  if (Array.isArray(value)) {
    return value.length > 0 ? value[0] : null;
  }
  return value ?? null;
}

export function principalToText(value) {
  const principal = readOptional(value);
  if (!principal) return '';
  return typeof principal.toText === 'function' ? principal.toText() : String(principal);
}

export function compareBigIntDesc(left, right) {
  const a = typeof left === 'bigint' ? left : BigInt(left);
  const b = typeof right === 'bigint' ? right : BigInt(right);
  if (a === b) return 0;
  return a > b ? -1 : 1;
}

export function variantName(value) {
  if (!value || Array.isArray(value) || typeof value !== 'object') return '';
  return Object.keys(value)[0] || '';
}

export function hasCanisterSource(sources, sourceName) {
  return Array.isArray(sources) && sources.some((source) => variantName(source) === sourceName);
}

export function fulfilledOrNull(result) {
  return result.status === 'fulfilled' ? result.value : null;
}
