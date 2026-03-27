import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@dfinity/principal';

import {
  accountIdentifierHex,
  loadDashboardData,
  summaryMetricsUnavailable,
  REGISTERED_SUMMARY_PAGE_SIZE,
  RECENT_CONTRIBUTION_LIMIT,
  RECENT_BURN_LIMIT,
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
    last_index_run_ts: [123n],
    index_interval_seconds: 600n,
    last_completed_cycles_sweep_ts: [456n],
    cycles_interval_seconds: 3600n,
    ...overrides,
  };
}

function historianCounts(overrides = {}) {
  return {
    registered_canister_count: 2n,
    qualifying_contribution_count: 3n,
    icp_burned_e8s: 400_000_000n,
    sns_discovered_canister_count: 4n,
    ...overrides,
  };
}

function registeredResponse() {
  return {
    items: [
      {
        canister_id: principal('aaaaa-aa'),
        sources: [{ MemoContribution: null }],
        qualifying_contribution_count: 2n,
        total_qualifying_contributed_e8s: 300_000_000n,
        last_contribution_ts: [1000n],
        latest_cycles: [1234n],
        last_cycles_probe_ts: [1001n],
      },
    ],
    page: 0n,
    page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE),
    total: 1n,
  };
}

function recentResponse() {
  return {
    items: [
      {
        canister_id: principal('aaaaa-aa'),
        tx_id: 22n,
        timestamp_nanos: [1_710_000_000_000_000_000n],
        amount_e8s: 200_000_000n,
        counts_toward_faucet: true,
      },
    ],
  };
}

test('accountIdentifierHex stays stable for the staking-account derivation fixture', () => {
  assert.equal(
    accountIdentifierHex(stakingAccount()),
    '4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3',
  );
});

test('loadDashboardData uses the shared frontend actor query shapes and native ledger stake path', async () => {
  const calls = [];
  const historianActor = {
    async get_public_counts() {
      calls.push(['counts']);
      return historianCounts();
    },
    async get_public_status() {
      calls.push(['status']);
      return historianStatus();
    },
    async list_registered_canister_summaries(args) {
      calls.push(['registered', args]);
      return registeredResponse();
    },
    async list_recent_contributions(args) {
      calls.push(['recent', args]);
      return recentResponse();
    },
    async list_recent_burns() {
      calls.push(['burns', { limit: [RECENT_BURN_LIMIT] }]);
      return { items: [] };
    },
  };

  let accountBalanceArg = null;
  let ledgerActorCount = 0;
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor,
    ledgerActorFactory: (canisterId, options) => {
      ledgerActorCount += 1;
      assert.equal(canisterId, 'ryjl3-tyaaa-aaaaa-aaaba-cai');
      assert.deepEqual(options, { agent: { test: true } });
      return {
        async account_balance(arg) {
          accountBalanceArg = arg;
          return { e8s: 123_456_789n };
        },
        async icrc1_balance_of() {
          throw new Error('icrc1 fallback should not be used when native account_balance succeeds');
        },
      };
    },
  });

  assert.equal(ledgerActorCount, 1);
  assert.deepEqual(calls[0], ['counts']);
  assert.deepEqual(calls[1], ['status']);
  assert.deepEqual(calls[2], ['registered', {
    page: [0],
    page_size: [REGISTERED_SUMMARY_PAGE_SIZE],
    sort: [{ TotalQualifyingContributedDesc: null }],
  }]);
  assert.deepEqual(calls[3], ['recent', {
    limit: [RECENT_CONTRIBUTION_LIMIT],
    qualifying_only: [false],
  }]);
  assert.deepEqual(calls[4], ['burns', { limit: [RECENT_BURN_LIMIT] }]);
  assert.equal(Buffer.from(accountBalanceArg.account).toString('hex'), '4ac9d3098789752b0809a290b67ae21892c5bc83e686e701882aac9809398bb3');
  assert.equal(data.stakeE8s, 123_456_789n);
  assert.equal(data.counts.icp_burned_e8s, 400_000_000n);
  assert.equal(data.registered.items.length, 1);
  assert.equal(data.recent.items.length, 1);
  assert.equal(data.hasAnyFailure, false);
  assert.equal(summaryMetricsUnavailable(data), false);
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
      async list_recent_contributions() { return recentResponse(); },
      async list_recent_burns() { return { items: [] }; },
    },
    ledgerActorFactory: () => ({
      async account_balance() {
        throw new Error('account_balance unavailable in this fixture');
      },
      async icrc1_balance_of(account) {
        fallbackArg = account;
        return 88_000_000n;
      },
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
      async list_recent_contributions() { throw new Error('Method list_recent_contributions not found'); },
      async list_recent_burns() { throw new Error('Method list_recent_burns not found'); },
    },
    ledgerActorFactory: () => {
      throw new Error('ledger actor should not be created when historian status is unavailable');
    },
  });

  assert.equal(data.historianAllRejected, true);
  assert.equal(data.historianLikelyOutdated, true);
  assert.equal(data.stakeE8s, null);
  assert.equal(summaryMetricsUnavailable(data), true);
});

test('loadDashboardData preserves zero values as loaded metrics instead of treating them as unavailable', async () => {
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() { return historianCounts({ registered_canister_count: 0n, qualifying_contribution_count: 0n, icp_burned_e8s: 0n }); },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return { items: [], page: 0n, page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE), total: 0n }; },
      async list_recent_contributions() { return { items: [] }; },
      async list_recent_burns() { return { items: [] }; },
    },
    ledgerActorFactory: () => ({
      async account_balance() {
        return { e8s: 0n };
      },
      async icrc1_balance_of() {
        throw new Error('fallback should not be used');
      },
    }),
  });

  assert.equal(data.stakeE8s, 0n);
  assert.equal(data.counts.icp_burned_e8s, 0n);
  assert.equal(summaryMetricsUnavailable(data), false);
});


test('loadDashboardData can represent a registered-but-non-qualifying canister without inflating burned ICP', async () => {
  const target = principal('ryjl3-tyaaa-aaaaa-aaaba-cai');
  const data = await loadDashboardData({
    historianCanisterId: 'j5gs6-uiaaa-aaaar-qb5cq-cai',
    host: 'https://icp0.io',
    agent: { test: true },
    historianActor: {
      async get_public_counts() {
        return historianCounts({
          registered_canister_count: 1n,
          qualifying_contribution_count: 0n,
          icp_burned_e8s: 0n,
          sns_discovered_canister_count: 0n,
        });
      },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() {
        return {
          items: [{
            canister_id: target,
            sources: [{ MemoContribution: null }],
            qualifying_contribution_count: 0n,
            total_qualifying_contributed_e8s: 0n,
            last_contribution_ts: [2000n],
            latest_cycles: [],
            last_cycles_probe_ts: [],
          }],
          page: 0n,
          page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE),
          total: 1n,
        };
      },
      async list_recent_contributions() {
        return {
          items: [{
            canister_id: target,
            tx_id: 7n,
            timestamp_nanos: [1_710_000_000_000_000_001n],
            amount_e8s: 5_000_000n,
            counts_toward_faucet: false,
          }],
        };
      },
    },
    ledgerActorFactory: () => ({
      async account_balance() {
        return { e8s: 5_000_000n };
      },
      async icrc1_balance_of() {
        throw new Error('fallback should not be used');
      },
    }),
  });

  assert.equal(data.stakeE8s, 5_000_000n);
  assert.equal(data.counts.registered_canister_count, 1n);
  assert.equal(data.counts.qualifying_contribution_count, 0n);
  assert.equal(data.counts.icp_burned_e8s, 0n);
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
          qualifying_contribution_count: 0n,
          icp_burned_e8s: 0n,
          sns_discovered_canister_count: 3n,
        });
      },
      async get_public_status() { return historianStatus(); },
      async list_registered_canister_summaries() { return { items: [], page: 0n, page_size: BigInt(REGISTERED_SUMMARY_PAGE_SIZE), total: 0n }; },
      async list_recent_contributions() { return { items: [] }; },
      async list_recent_burns() { return { items: [] }; },
    },
    ledgerActorFactory: () => ({
      async account_balance() {
        return { e8s: 0n };
      },
      async icrc1_balance_of() {
        throw new Error('fallback should not be used');
      },
    }),
  });

  assert.equal(data.stakeE8s, 0n);
  assert.equal(data.counts.registered_canister_count, 0n);
  assert.equal(data.counts.qualifying_contribution_count, 0n);
  assert.equal(data.counts.icp_burned_e8s, 0n);
  assert.equal(data.counts.sns_discovered_canister_count, 3n);
  assert.equal(data.registered.total, 0n);
  assert.equal(summaryMetricsUnavailable(data), false);
});
