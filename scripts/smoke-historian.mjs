#!/usr/bin/env node
import { HttpAgent } from '@dfinity/agent';
import { sha224 } from '@noble/hashes/sha2.js';
import { createActor as createHistorianActor } from '../jupiter-faucet-frontend/frontend-src/declarations/jupiter_historian/index.js';
import { createActor as createLedgerActor } from '../jupiter-faucet-frontend/frontend-src/declarations/mock_icrc_ledger/index.js';

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith('--')) continue;
    out[arg.slice(2)] = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[++i] : 'true';
  }
  return out;
}

function usage() {
  console.error('Usage: node scripts/smoke-historian.mjs --historian <canister-id> [--host https://icp0.io]');
  process.exit(1);
}

function uint8ArrayFromOptBytes(optBytes) {
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

function bytesToHex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function accountIdentifierBytes(account) {
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

function formatIcpE8s(value) {
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const whole = asBigInt / 100_000_000n;
  const fraction = (asBigInt % 100_000_000n).toString().padStart(8, '0').replace(/0+$/, '');
  return fraction ? `${whole}.${fraction} ICP` : `${whole} ICP`;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const host = args.host || 'https://icp0.io';
  const historianCanisterId = args.historian;
  if (!historianCanisterId) usage();

  const agent = new HttpAgent({ host });
  const historian = createHistorianActor(historianCanisterId, { agent });

  const [status, counts] = await Promise.all([
    historian.get_public_status(),
    historian.get_public_counts(),
  ]);

  const stakingAccountId = bytesToHex(accountIdentifierBytes(status.staking_account));
  const ledger = createLedgerActor(status.ledger_canister_id.toText(), { agent });

  let nativeBalance = null;
  let icrcBalance = null;
  let nativeError = null;
  let icrcError = null;

  try {
    const balance = await ledger.account_balance({ account: Array.from(accountIdentifierBytes(status.staking_account)) });
    nativeBalance = BigInt(balance.e8s);
  } catch (error) {
    nativeError = error?.message || String(error);
  }

  try {
    icrcBalance = BigInt(await ledger.icrc1_balance_of(status.staking_account));
  } catch (error) {
    icrcError = error?.message || String(error);
  }

  const output = {
    host,
    historianCanisterId,
    ledgerCanisterId: status.ledger_canister_id.toText(),
    stakingOwner: status.staking_account.owner.toText(),
    stakingSubaccountBytes: Array.isArray(status.staking_account.subaccount) && status.staking_account.subaccount[0] ? status.staking_account.subaccount[0] : [],
    stakingAccountIdentifier: stakingAccountId,
    counts: {
      registeredCanisters: Number(counts.registered_canister_count),
      qualifyingContributions: Number(counts.qualifying_contribution_count),
      icpBurnedE8s: counts.icp_burned_e8s.toString(),
      snsDiscoveredCanisters: Number(counts.sns_discovered_canister_count),
    },
    balances: {
      nativeLedgerE8s: nativeBalance?.toString() ?? null,
      nativeLedgerFormatted: nativeBalance !== null ? formatIcpE8s(nativeBalance) : null,
      nativeLedgerError: nativeError,
      icrc1E8s: icrcBalance?.toString() ?? null,
      icrc1Formatted: icrcBalance !== null ? formatIcpE8s(icrcBalance) : null,
      icrc1Error: icrcError,
    },
  };

  console.log(JSON.stringify(output, null, 2));

  if (Number(counts.registered_canister_count) === 0 && Number(counts.qualifying_contribution_count) === 0 && BigInt(counts.icp_burned_e8s) === 0n) {
    console.warn('warning: historian public counts are all zero; verify upgrade args, staking account, and index ingestion.');
  }
  if (nativeBalance !== null && nativeBalance > 0n && BigInt(counts.icp_burned_e8s) === 0n) {
    console.warn('warning: staking account has a non-zero ledger balance but historian qualifying contribution counts are zero.');
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
