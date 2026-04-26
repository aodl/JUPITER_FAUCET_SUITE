import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@icp-sdk/core/principal';
import { HttpAgent } from '@icp-sdk/core/agent';

import {
  accountIdentifierHex,
  loadDashboardData,
  loadRecentRouteTransfersFromIndex,
  loadRegisteredCanisterSummaryPage,
  loadTrackerData,
  loadCmcTopUpTransfersFromIndex,
  cmcDepositAccount,
  hasCanisterSource,
  resetAgentCacheForTests,
  summaryMetricsUnavailable,
  REGISTERED_SUMMARY_PAGE_SIZE,
  RECENT_COMMITMENT_LIMIT,
  RECENT_ROUTE_TRANSFER_LIMIT,
} from '../src/dashboard-data.js';

function principal(text) {
  return Principal.fromText(text);
}

function stakingAccount() {
  return {
    owner: principal('2vxsx-fae'),
    subaccount: [Array.from({ length: 32 }, () => 9)],
  };
}

function historianStatus(overrides = {}) {
  return {
    staking_account: stakingAccount(),
    ledger_canister_id: principal('ryjl3-tyaaa-aaaaa-aaaba-cai'),
    faucet_canister_id: principal('acjuz-liaaa-aaaar-qb4qq-cai'),
    last_index_run_ts: [123n],
    index_interval_seconds: 600n,
    last_completed_cycles_sweep_ts: [456n],
    cycles_interval_seconds: 3600n,
    heap_memory_bytes: [8_388_608n],
    stable_memory_bytes: [0n],
    total_memory_bytes: [8_388_608n],
    ...overrides,
  };
}

function historianCounts(overrides = {}) {
  return {
    registered_canister_count: 2n,
    qualifying_commitment_count: 3n,
    sns_discovered_canister_count: 4n,
    total_output_e8s: 400_000_000n,
    total_rewards_e8s: 50_000_000n,
    ...overrides,
  };
}

function registeredResponse() {
  return {
    items: [{
      canister_id: principal('aaaaa-aa'),
      sources: [{ MemoCommitment: null }],
      qualifying_commitment_count: 2n,
      total_qualifying_committed_e8s: 300_000_000n,
      last_commitment_ts: [1000n],
      latest_cycles: [1234n],
      last_cycles_probe_ts: [1001n],
    }],
    page: 0n,
    page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE),
    total: 1n,
  };
}

function recentResponse() {
  return {
    items: [{
      canister_id: principal('aaaaa-aa'),
      tx_id: 22n,
      timestamp_nanos: [1_710_000_000_000_000_000n],
      amount_e8s: 200_000_000n,
      counts_toward_faucet: true,
      outcome_category: { QualifyingCommitment: null },
    }],
  };
}

test('accountIdentifierHex stays stable for the staking-account derivation fixture', () => {
  assert.equal(
    accountIdentifierHex(stakingAccount()),
    '4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3',
  );
});

test('loadDashboardData uses historian counts and status plus the native ledger stake path', async () => {
  const calls = [];
  let accountBalanceArg = null;
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { calls.push(['counts']); return historianCounts(); },
      async get_public_status() { calls.push(['status']); return historianStatus(); },
      async list_registered_canister_summaries(args) { calls.push(['registered', args]); return registeredResponse(); },
      async list_recent_commitments(args) { calls.push(['recent', args]); return recentResponse(); },
    },
    ledgerActorFactory: (canisterId, options) => {
      assert.equal(canisterId, 'ryjl3-tyaaa-aaaaa-aaaba-cai');
      assert.deepEqual(options, { agent: { test: true } });
      return {
        async account_balance(arg) { accountBalanceArg = arg; return { e8s: 123_456_789n }; },
        async icrc1_balance_of() { throw new Error('fallback should not be used'); },
      };
    },
  });

  assert.deepEqual(calls[0], ['counts']);
  assert.deepEqual(calls[1], ['status']);
  assert.deepEqual(calls[2], ['registered', { page: [0], page_size: [REGISTERED_SUMMARY_PAGE_SIZE] }]);
  assert.deepEqual(calls[3], ['recent', { limit: [RECENT_COMMITMENT_LIMIT], qualifying_only: [false] }]);
  assert.equal(Buffer.from(accountBalanceArg.account).toString('hex'), '4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3');
  assert.equal(data.stakeE8s, 123_456_789n);
  assert.equal(data.counts.total_output_e8s, 400_000_000n);
  assert.equal(data.counts.total_rewards_e8s, 50_000_000n);
  assert.equal(data.registered.items.length, 1);
  assert.equal(data.recent.items.length, 1);
  assert.equal(data.hasAnyFailure, false);
  assert.equal(summaryMetricsUnavailable(data), false);
});

function routeAccount(ownerText) {
  return { owner: principal(ownerText), subaccount: [] };
}

function routeIndexTransaction({ id, from, to, amountE8s, timestampNanos = 1_710_000_000_000_000_000n, operationKind = 'Transfer' }) {
  const transfer = {
    from,
    to,
    amount: { e8s: BigInt(amountE8s) },
    fee: { e8s: 10_000n },
  };
  if (operationKind === 'Transfer') {
    transfer.spender = [];
  } else {
    transfer.spender = from;
  }
  return {
    id: BigInt(id),
    transaction: {
      memo: 0n,
      icrc1_memo: [],
      created_at_time: [],
      timestamp: [{ timestamp_nanos: timestampNanos }],
      operation: { [operationKind]: transfer },
    },
  };
}

test('loadDashboardData skips recent route transfer lookups when route config is missing', async () => {
  let indexActorConstructed = false;

  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 1n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
    indexActorFactory: () => {
      indexActorConstructed = true;
      throw new Error('index actor should not be constructed without full route config');
    },
  });

  assert.equal(indexActorConstructed, false);
  assert.deepEqual(data.outputTransfers.items, []);
  assert.deepEqual(data.rewardsTransfers.items, []);
  assert.equal(data.errors.outputTransfers, null);
  assert.equal(data.errors.rewardsTransfers, null);
});

test('loadDashboardData queries the ICP index directly for recent output and reward transfers', async () => {
  const source = routeAccount('uccpi-cqaaa-aaaar-qby3q-cai');
  const output = routeAccount('acjuz-liaaa-aaaar-qb4qq-cai');
  const rewards = routeAccount('alk7f-5aaaa-aaaar-qb4ra-cai');
  const sourceId = accountIdentifierHex(source);
  const outputId = accountIdentifierHex(output);
  const rewardsId = accountIdentifierHex(rewards);
  const indexCalls = [];

  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() {
        return historianStatus({
          output_source_account: [source],
          output_account: [output],
          rewards_account: [rewards],
          index_canister_id: [principal('qhbym-qaaaa-aaaaa-aaafq-cai')],
        });
      },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 1n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
    indexActorFactory: (canisterId, options) => {
      assert.equal(canisterId, 'qhbym-qaaaa-aaaaa-aaafq-cai');
      assert.deepEqual(options, { agent: { test: true } });
      return {
        async get_account_identifier_transactions(args) {
          indexCalls.push(args);
          if (args.account_identifier === outputId) {
            return { Ok: { balance: 0n, oldest_tx_id: [40n], transactions: [
              {
                id: 44n,
                transaction: {
                  memo: 0n,
                  icrc1_memo: [],
                  created_at_time: [],
                  timestamp: [{ timestamp_nanos: 1_710_000_000_000_000_044n }],
                  operation: { Transfer: { from: sourceId, to: outputId, amount: { e8s: 111_000_000n }, fee: { e8s: 10_000n }, spender: [] } },
                },
              },
              {
                id: 43n,
                transaction: {
                  memo: 0n,
                  icrc1_memo: [],
                  created_at_time: [],
                  timestamp: [{ timestamp_nanos: 1_710_000_000_000_000_043n }],
                  operation: { Transfer: { from: 'third-party', to: outputId, amount: { e8s: 999_000_000n }, fee: { e8s: 10_000n }, spender: [] } },
                },
              },
            ] } };
          }
          return { Ok: { balance: 0n, oldest_tx_id: [50n], transactions: [{
            id: 55n,
            transaction: {
              memo: 0n,
              icrc1_memo: [],
              created_at_time: [{ timestamp_nanos: 1_710_000_000_000_000_055n }],
              timestamp: [],
              operation: { TransferFrom: { from: sourceId, to: rewardsId, amount: { e8s: 22_000_000n }, fee: { e8s: 10_000n }, spender: sourceId } },
            },
          }] } };
        },
      };
    },
  });

  assert.equal(indexCalls.length, 2);
  assert.deepEqual(indexCalls.map((call) => call.account_identifier).sort(), [outputId, rewardsId].sort());
  assert.equal(indexCalls[0].max_results, BigInt(RECENT_ROUTE_TRANSFER_LIMIT));
  assert.equal(data.outputTransfers.items.length, 1);
  assert.equal(data.outputTransfers.items[0].tx_id, 44n);
  assert.equal(data.outputTransfers.items[0].amount_e8s, 111_000_000n);
  assert.equal(data.rewardsTransfers.items.length, 1);
  assert.equal(data.rewardsTransfers.items[0].tx_id, 55n);
  assert.equal(data.rewardsTransfers.items[0].amount_e8s, 22_000_000n);
  assert.equal(data.errors.outputTransfers, null);
  assert.equal(data.errors.rewardsTransfers, null);
});


test('loadDashboardData keeps route transfer lookup failures out of the global failure flag', async () => {
  const source = routeAccount('uccpi-cqaaa-aaaar-qby3q-cai');
  const output = routeAccount('acjuz-liaaa-aaaar-qb4qq-cai');
  const rewards = routeAccount('alk7f-5aaaa-aaaar-qb4ra-cai');

  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() {
        return historianStatus({
          output_source_account: [source],
          output_account: [output],
          rewards_account: [rewards],
          index_canister_id: [principal('qhbym-qaaaa-aaaaa-aaafq-cai')],
        });
      },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 1n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
    indexActorFactory: () => ({
      async get_account_identifier_transactions() {
        throw new Error('index temporarily unavailable');
      },
    }),
  });

  assert.equal(data.hasAnyFailure, false);
  assert.equal(data.errors.outputTransfers, 'index temporarily unavailable');
  assert.equal(data.errors.rewardsTransfers, 'index temporarily unavailable');
  assert.equal(data.outputTransfers, null);
  assert.equal(data.rewardsTransfers, null);
});

test('loadRecentRouteTransfersFromIndex de-duplicates rows before applying the limit and advances the index cursor', async () => {
  const source = routeAccount('uccpi-cqaaa-aaaar-qby3q-cai');
  const output = routeAccount('acjuz-liaaa-aaaar-qb4qq-cai');
  const sourceId = accountIdentifierHex(source);
  const outputId = accountIdentifierHex(output);
  const calls = [];
  const pages = [
    [
      routeIndexTransaction({ id: 10n, from: sourceId, to: outputId, amountE8s: 100_000_000n }),
      routeIndexTransaction({ id: 10n, from: sourceId, to: outputId, amountE8s: 100_000_000n }),
    ],
    [
      routeIndexTransaction({ id: 9n, from: sourceId, to: outputId, amountE8s: 200_000_000n }),
      routeIndexTransaction({ id: 8n, from: 'third-party', to: outputId, amountE8s: 300_000_000n }),
    ],
  ];

  const data = await loadRecentRouteTransfersFromIndex({
    outputSourceAccount: source,
    routeAccount: output,
    limit: 2,
    pageSize: 2,
    maxPages: 3,
    index: {
      async get_account_identifier_transactions(args) {
        calls.push(args);
        return { Ok: { balance: 0n, oldest_tx_id: [], transactions: pages[calls.length - 1] || [] } };
      },
    },
  });

  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0].start, []);
  assert.deepEqual(calls[1].start, [9n]);
  assert.deepEqual(data.items.map((item) => item.tx_id), [10n, 9n]);
  assert.deepEqual(data.items.map((item) => item.amount_e8s), [100_000_000n, 200_000_000n]);
});

test('loadDashboardData requests only the configured registered canister summary page', async () => {
  const registeredCalls = [];
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    registeredPage: 2,
    registeredPageSize: 6,
    historianActor: {
      async get_public_counts() { return historianCounts({ registered_canister_count: 18n }); },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries(args) {
        registeredCalls.push(args);
        return { items: registeredResponse().items, page: 2n, page_size: 6n, total: 18n };
      },
      async list_recent_commitments() { return { items: [] }; },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 1n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
  });

  assert.deepEqual(registeredCalls, [{ page: [2], page_size: [6] }]);
  assert.equal(data.registered.page, 2n);
  assert.equal(data.registered.page_size, 6n);
  assert.equal(data.registered.total, 18n);
});

test('loadRegisteredCanisterSummaryPage uses the shared frontend query shape for server-side pagination', async () => {
  const calls = [];
  const response = await loadRegisteredCanisterSummaryPage({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async list_registered_canister_summaries(args) {
        calls.push(args);
        return { items: registeredResponse().items, page: 3n, page_size: 6n, total: 17n };
      },
    },
    page: 3,
    pageSize: 6,
  });

  assert.deepEqual(calls, [{ page: [3], page_size: [6] }]);
  assert.equal(response.page, 3n);
  assert.equal(response.page_size, 6n);
  assert.equal(response.total, 17n);
});

test('loadDashboardData falls back to icrc1_balance_of when native ledger account_balance fails', async () => {
  let fallbackArg = null;
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => ({
      async account_balance() { throw new Error('account_balance unavailable in this fixture'); },
      async icrc1_balance_of(account) { fallbackArg = account; return 88_000_000n; },
    }),
  });

  assert.equal(fallbackArg.owner.toText(), stakingAccount().owner.toText());
  assert.deepEqual(fallbackArg.subaccount, stakingAccount().subaccount);
  assert.equal(data.stakeE8s, 88_000_000n);
  assert.equal(data.errors.stake, null);
});

test('loadDashboardData flags an outdated historian interface when every public query method is missing', async () => {
  const methodMissing = new Error('Method get_public_counts is not part of the service');
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { throw methodMissing; },
      async get_public_status() { throw new Error('Method get_public_status not found'); },
      async list_registered_canister_summaries() { throw new Error('Method list_registered_canister_summaries is not part of the service'); },
      async list_recent_commitments() { throw new Error('Method list_recent_commitments not found'); },
    },
    ledgerActorFactory: () => { throw new Error('ledger actor should not be created'); },
  });

  assert.equal(data.historianAllRejected, true);
  assert.equal(data.historianLikelyOutdated, true);
  assert.equal(data.stakeE8s, null);
  assert.equal(summaryMetricsUnavailable(data), true);
});

test('loadDashboardData enables query signature verification when it creates an agent', async () => {
  resetAgentCacheForTests();
  const originalCreate = HttpAgent.create;
  const seen = [];
  try {
    HttpAgent.create = async (options) => {
      seen.push(options);
      return { async fetchRootKey() {} };
    };

    const data = await loadDashboardData({
      historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
      host: 'https://icp0.io',
      historianActorFactory: (_canisterId, { agent }) => ({
        async get_public_counts() { return historianCounts(); },
        async get_public_status() { return historianStatus(); },
        async list_registered_canister_summaries() { return registeredResponse(); },
        async list_recent_commitments() { return recentResponse(); },
      }),
      ledgerActorFactory: () => ({ async account_balance() { return { e8s: 1n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
    });

    assert.equal(data.stakeE8s, 1n);
    assert.equal(seen.length, 1);
    assert.equal(seen[0].host, 'https://icp0.io');
    assert.equal(seen[0].verifyQuerySignatures, true);
  } finally {
    HttpAgent.create = originalCreate;
    resetAgentCacheForTests();
  }
});



test('loadDashboardData evicts failed agent initialization from cache so the next attempt can retry', async () => {
  resetAgentCacheForTests();
  const originalCreate = HttpAgent.create;
  let attempts = 0;
  try {
    HttpAgent.create = async () => {
      attempts += 1;
      if (attempts === 1) {
        throw new Error('transient agent creation failure');
      }
      return { async fetchRootKey() {} };
    };

    const failed = await loadDashboardData({
      historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
      host: 'https://icp0.io',
      historianActorFactory: () => ({
        async get_public_counts() { return historianCounts(); },
        async get_public_status() { return historianStatus(); },
        async list_registered_canister_summaries() { return registeredResponse(); },
        async list_recent_commitments() { return recentResponse(); },
      }),
      ledgerActorFactory: () => ({ async account_balance() { return { e8s: 1n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
    });

    assert.equal(failed.historianAllRejected, true);
    assert.match(failed.errors.counts, /transient agent creation failure/);

    const data = await loadDashboardData({
      historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
      host: 'https://icp0.io',
      historianActorFactory: () => ({
        async get_public_counts() { return historianCounts(); },
        async get_public_status() { return historianStatus(); },
        async list_registered_canister_summaries() { return registeredResponse(); },
        async list_recent_commitments() { return recentResponse(); },
      }),
      ledgerActorFactory: () => ({ async account_balance() { return { e8s: 2n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
    });

    assert.equal(attempts, 2);
    assert.equal(data.stakeE8s, 2n);
  } finally {
    HttpAgent.create = originalCreate;
    resetAgentCacheForTests();
  }
});

test('loadDashboardData preserves zero values as loaded metrics instead of treating them as unavailable', async () => {
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() {
        return historianCounts({
          registered_canister_count: 0n,
          qualifying_commitment_count: 0n,
          sns_discovered_canister_count: 0n,
          total_output_e8s: 0n,
          total_rewards_e8s: 0n,
        });
      },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return { items: [], page: 0n, page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE), total: 0n }; },
      async list_recent_commitments() { return { items: [] }; },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 0n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
  });

  assert.equal(data.stakeE8s, 0n);
  assert.equal(data.counts.total_output_e8s, 0n);
  assert.equal(data.counts.total_rewards_e8s, 0n);
  assert.equal(summaryMetricsUnavailable(data), false);
});

test('loadDashboardData can represent a registered-but-non-qualifying canister without inflating protocol totals', async () => {
  const target = principal('ryjl3-tyaaa-aaaaa-aaaba-cai');
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() {
        return historianCounts({
          registered_canister_count: 1n,
          qualifying_commitment_count: 0n,
          sns_discovered_canister_count: 0n,
          total_output_e8s: 0n,
          total_rewards_e8s: 0n,
        });
      },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() {
        return {
          items: [{
            canister_id: target,
            sources: [{ MemoCommitment: null }],
            qualifying_commitment_count: 0n,
            total_qualifying_committed_e8s: 0n,
            last_commitment_ts: [2000n],
            latest_cycles: [],
            last_cycles_probe_ts: [],
          }],
          page: 0n,
          page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE),
          total: 1n,
        };
      },
      async list_recent_commitments() {
        return {
          items: [{
            canister_id: target,
            tx_id: 7n,
            timestamp_nanos: [1_710_000_000_000_000_001n],
            amount_e8s: 5_000_000n,
            counts_toward_faucet: false,
            outcome_category: { UnderThresholdCommitment: null },
          }],
        };
      },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 5_000_000n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
  });

  assert.equal(data.stakeE8s, 5_000_000n);
  assert.equal(data.counts.registered_canister_count, 1n);
  assert.equal(data.counts.qualifying_commitment_count, 0n);
  assert.equal(data.counts.total_output_e8s, 0n);
  assert.equal(data.counts.total_rewards_e8s, 0n);
  assert.equal(data.registered.items[0].canister_id.toText(), target.toText());
  assert.equal(data.recent.items[0].counts_toward_faucet, false);
  assert.equal(summaryMetricsUnavailable(data), false);
});

test('loadDashboardData keeps SNS-only discovery out of registered frontend totals', async () => {
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() {
        return historianCounts({
          registered_canister_count: 0n,
          qualifying_commitment_count: 0n,
          sns_discovered_canister_count: 3n,
          total_output_e8s: 0n,
          total_rewards_e8s: 0n,
        });
      },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return { items: [], page: 0n, page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE), total: 0n }; },
      async list_recent_commitments() { return { items: [] }; },
    },
    ledgerActorFactory: () => ({ async account_balance() { return { e8s: 0n }; }, async icrc1_balance_of() { throw new Error('fallback should not be used'); } }),
  });

  assert.equal(data.stakeE8s, 0n);
  assert.equal(data.counts.registered_canister_count, 0n);
  assert.equal(data.counts.qualifying_commitment_count, 0n);
  assert.equal(data.counts.total_output_e8s, 0n);
  assert.equal(data.counts.total_rewards_e8s, 0n);
  assert.equal(data.counts.sns_discovered_canister_count, 3n);
  assert.equal(data.registered.total, 0n);
  assert.equal(summaryMetricsUnavailable(data), false);
});

test('loadDashboardData returns structured failures when historian actor construction throws synchronously', async () => {
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActorFactory: () => {
      throw new Error('historian actor construction failed');
    },
  });

  assert.equal(data.counts, null);
  assert.equal(data.status, null);
  assert.equal(data.registered, null);
  assert.equal(data.recent, null);
  assert.equal(data.stakeE8s, null);
  assert.equal(data.hasAnyFailure, true);
  assert.equal(data.errors.counts, 'historian actor construction failed');
  assert.equal(data.errors.status, 'historian actor construction failed');
  assert.equal(data.errors.registered, 'historian actor construction failed');
  assert.equal(data.errors.recent, 'historian actor construction failed');
  assert.equal(data.errors.stake, 'Stake unavailable');
  assert.equal(data.historianAllRejected, true);
  assert.equal(data.historianLikelyOutdated, false);
});

test('loadDashboardData preserves partial dashboard data when get_public_status fails', async () => {
  let ledgerCreated = false;
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() { throw new Error('status temporarily unavailable'); },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => {
      ledgerCreated = true;
      throw new Error('ledger actor should not be created when status failed');
    },
  });

  assert.equal(ledgerCreated, false);
  assert.deepEqual(data.counts, historianCounts());
  assert.deepEqual(data.registered, registeredResponse());
  assert.deepEqual(data.recent, recentResponse());
  assert.equal(data.status, null);
  assert.equal(data.stakeE8s, null);
  assert.equal(data.errors.status, 'status temporarily unavailable');
  assert.equal(data.errors.stake, 'Stake unavailable');
});

test('loadDashboardData surfaces both native and icrc stake failures in one normalized message', async () => {
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => ({
      async account_balance() { throw new Error('native path unavailable'); },
      async icrc1_balance_of() { throw new Error('icrc fallback unavailable'); },
    }),
  });

  assert.equal(
    data.errors.stake,
    'Stake unavailable via native ledger account_balance (native path unavailable) and icrc1_balance_of (icrc fallback unavailable)',
  );
  assert.equal(data.stakeE8s, null);
  assert.equal(data.hasAnyFailure, true);
});

test('loadDashboardData preserves historian data when ledger actor construction throws synchronously', async () => {
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts(); },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return registeredResponse(); },
      async list_recent_commitments() { return recentResponse(); },
    },
    ledgerActorFactory: () => { throw new Error('ledger actor construction failed'); },
  });

  assert.deepEqual(data.counts, historianCounts());
  assert.deepEqual(data.status, historianStatus());
  assert.deepEqual(data.registered, registeredResponse());
  assert.deepEqual(data.recent, recentResponse());
  assert.equal(data.stakeE8s, null);
  assert.equal(data.errors.stake, 'ledger actor construction failed');
  assert.equal(data.hasAnyFailure, true);
});

test('hasCanisterSource detects candid variant-style source values', () => {
  assert.equal(hasCanisterSource([{ MemoCommitment: null }], 'MemoCommitment'), true);
  assert.equal(hasCanisterSource([{ SnsDiscovery: null }], 'MemoCommitment'), false);
  assert.equal(hasCanisterSource([], 'MemoCommitment'), false);
});

test('loadTrackerData returns unrecognised state without history queries', async () => {
  const target = principal('ryjl3-tyaaa-aaaaa-aaaba-cai');
  let commitmentHistoryCalled = false;
  const data = await loadTrackerData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    canisterId: target,
    historianActor: {
      async get_canister_overview(canisterId) {
        assert.equal(canisterId.toText(), target.toText());
        return [];
      },
      async get_commitment_history() {
        commitmentHistoryCalled = true;
        throw new Error('should not query commitment history for an unrecognised canister');
      },
      async get_cycles_history() {
        throw new Error('should not query cycles history for an unrecognised canister');
      },
    },
  });

  assert.equal(commitmentHistoryCalled, false);
  assert.equal(data.isRecognized, false);
  assert.equal(data.isCommitmentBeneficiary, false);
  assert.deepEqual(data.commitments.items, []);
  assert.deepEqual(data.cycles.items, []);
});

test('loadTrackerData loads commitment, observed CMC top-up, and cycles histories for memo-registered beneficiaries', async () => {
  const target = principal('ryjl3-tyaaa-aaaaa-aaaba-cai');
  const calls = [];
  const data = await loadTrackerData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    canisterId: target,
    historyLimit: 12,
    historianActor: {
      async get_canister_overview(canisterId) {
        calls.push(['overview', canisterId.toText()]);
        return [{
          canister_id: target,
          sources: [{ MemoCommitment: null }],
          meta: {
            first_seen_ts: [100n],
            last_commitment_ts: [200n],
            last_cycles_probe_ts: [300n],
            last_cycles_probe_result: [{ Ok: { BlackholeStatus: null } }],
          },
          cycles_points: 1,
          commitment_points: 1,
        }];
      },
      async get_commitment_history(args) {
        calls.push(['commitments', args]);
        return {
          items: [{
            tx_id: 22n,
            timestamp_nanos: [1_710_000_000_000_000_000n],
            amount_e8s: 200_000_000n,
            counts_toward_faucet: true,
          }],
          next_start_after_tx_id: [],
        };
      },
      async get_cycles_history(args) {
        calls.push(['cycles', args]);
        return {
          items: [{
            timestamp_nanos: 1_710_010_000_000_000_000n,
            cycles: 1_000_000_000_000n,
            source: { BlackholeStatus: null },
          }],
          next_start_after_ts: [],
        };
      },
      async get_public_status() {
        calls.push(['status']);
        return historianStatus({
          index_canister_id: [principal('qhbym-qaaaa-aaaaa-aaafq-cai')],
        });
      },
    },
    indexActorFactory: (canisterId, options) => {
      assert.equal(canisterId, 'qhbym-qaaaa-aaaaa-aaafq-cai');
      assert.deepEqual(options, { agent: { test: true } });
      const depositId = accountIdentifierHex(cmcDepositAccount({ canisterId: target }));
      return {
        async get_account_identifier_transactions(args) {
          calls.push(['index', args]);
          assert.equal(args.account_identifier, depositId);
          return { Ok: { balance: 0n, oldest_tx_id: [66n], transactions: [{
            id: 66n,
            transaction: {
              memo: 0n,
              icrc1_memo: [],
              created_at_time: [],
              timestamp: [{ timestamp_nanos: 1_710_020_000_000_000_000n }],
              operation: { Transfer: { from: 'faucet-account', to: depositId, amount: { e8s: 123_000_000n }, fee: { e8s: 10_000n }, spender: [] } },
            },
          }] } };
        },
      };
    },
  });

  assert.equal(data.isRecognized, true);
  assert.equal(data.isCommitmentBeneficiary, true);
  assert.equal(data.commitments.items.length, 1);
  assert.equal(data.cycles.items.length, 1);
  assert.equal(data.cmcTransfers.items.length, 1);
  assert.equal(data.cmcTransfers.items[0].amount_e8s, 123_000_000n);
  assert.deepEqual(calls[0], ['overview', target.toText()]);
  assert.equal(calls[1][1].limit[0], 12);
  assert.equal(calls[1][1].descending[0], false);
  assert.equal(calls[1][1].canister_id.toText(), target.toText());
  assert.equal(calls[2][1].limit[0], 12);
  assert.equal(calls[2][1].descending[0], false);
  assert.equal(calls[2][1].canister_id.toText(), target.toText());
});

test('loadTrackerData treats SNS-only canisters as not commitment beneficiaries', async () => {
  const target = principal('ryjl3-tyaaa-aaaaa-aaaba-cai');
  const data = await loadTrackerData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    canisterId: target,
    historianActor: {
      async get_canister_overview() {
        return [{
          canister_id: target,
          sources: [{ SnsDiscovery: null }],
          meta: {
            first_seen_ts: [100n],
            last_commitment_ts: [],
            last_cycles_probe_ts: [300n],
            last_cycles_probe_result: [{ Ok: { SnsRootSummary: null } }],
          },
          cycles_points: 1,
          commitment_points: 0,
        }];
      },
      async get_commitment_history() {
        throw new Error('SNS-only canisters should not query commitment history');
      },
      async get_cycles_history() {
        throw new Error('SNS-only canisters should not query cycles history');
      },
    },
  });

  assert.equal(data.isRecognized, true);
  assert.equal(data.isCommitmentBeneficiary, false);
  assert.deepEqual(data.commitments.items, []);
  assert.deepEqual(data.cycles.items, []);
});

test('loadCmcTopUpTransfersFromIndex builds the CMC deposit account and filters matching transfers', async () => {
  const target = principal('ryjl3-tyaaa-aaaaa-aaaba-cai');
  const depositId = accountIdentifierHex(cmcDepositAccount({ canisterId: target }));
  const calls = [];
  const result = await loadCmcTopUpTransfersFromIndex({
    canisterId: target,
    index: {
      async get_account_identifier_transactions(args) {
        calls.push(args);
        return { Ok: { balance: 0n, oldest_tx_id: [10n], transactions: [
          {
            id: 11n,
            transaction: {
              memo: 0n,
              icrc1_memo: [],
              created_at_time: [],
              timestamp: [{ timestamp_nanos: 1_710_000_000_000_000_011n }],
              operation: { Transfer: { from: 'faucet-account', to: depositId, amount: { e8s: 250_000_000n }, fee: { e8s: 10_000n }, spender: [] } },
            },
          },
          {
            id: 10n,
            transaction: {
              memo: 0n,
              icrc1_memo: [],
              created_at_time: [],
              timestamp: [{ timestamp_nanos: 1_710_000_000_000_000_010n }],
              operation: { Transfer: { from: 'someone-else', to: 'not-the-deposit-account', amount: { e8s: 500_000_000n }, fee: { e8s: 10_000n }, spender: [] } },
            },
          },
        ] } };
      },
    },
    limit: 10,
    pageSize: 10,
  });

  assert.equal(calls.length, 1);
  assert.equal(calls[0].account_identifier, depositId);
  assert.equal(result.items.length, 1);
  assert.equal(result.items[0].tx_id, 11n);
  assert.equal(result.items[0].amount_e8s, 250_000_000n);
});
