#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = process.cwd();

const privateExportChecks = [
  ['canisters/frontend/web/src/advanced-memo-builder.js', [
    'JUPITER_NEURON_ID_MAX_DIGITS',
    'U64_MAX',
    'isAscii',
  ]],
  ['canisters/frontend/web/src/chart-rendering.js', [
    'toBigIntValue',
    'ratioBigInt',
  ]],
  ['canisters/frontend/web/src/data/index-transactions.js', [
    'RECENT_ROUTE_TRANSFER_PAGE_SIZE',
    'RECENT_ROUTE_TRANSFER_MAX_INDEX_PAGES',
    'routeTransferFromIndexTransaction',
    'cmcDepositSubaccount',
    'cmcTopUpTransferFromIndexTransaction',
  ]],
  ['canisters/frontend/web/src/dashboard-data.js', [
    'isMethodMissingError',
    'MAINNET_CMC_CANISTER_ID',
    'MAINNET_GOVERNANCE_CANISTER_ID',
    'DQUORUM_STAKING_ACCOUNT_SUBACCOUNT_HEX',
    'hexToBytes',
    'accountIdentifierBytes',
    'routeTransferFromIndexTransaction',
    'cmcDepositSubaccount',
    'cmcTopUpTransferFromIndexTransaction',
    'RECENT_ROUTE_TRANSFER_PAGE_SIZE',
    'RECENT_ROUTE_TRANSFER_MAX_INDEX_PAGES',
    'TRACKER_HISTORY_PAGE_SIZE',
  ]],
  ['canisters/frontend/web/src/projection-simulator.js', [
    'SECONDS_PER_DAY',
    'ceilDiv',
    'normaliseSimulatorInputs',
  ]],
  ['canisters/frontend/web/src/tracker-cycles.js', [
    'sortedHistorianCycleSamples',
  ]],
];

const failures = [];

for (const [relativePath, symbols] of privateExportChecks) {
  const filePath = path.join(repoRoot, relativePath);
  const source = await readFile(filePath, 'utf8');
  for (const symbol of symbols) {
    const exportedDeclaration = new RegExp(String.raw`export\s+(?:const|let|var|function|class)\s+${symbol}\b`);
    const exportedList = new RegExp(String.raw`export\s*\{[^}]*\b${symbol}\b[^}]*\}`, 's');
    if (exportedDeclaration.test(source) || exportedList.test(source)) {
      failures.push(`${relativePath}: ${symbol} should stay module-private`);
    }
  }
}

if (failures.length > 0) {
  console.error(failures.join('\n'));
  process.exit(1);
}

console.log('Frontend internal export check passed');
