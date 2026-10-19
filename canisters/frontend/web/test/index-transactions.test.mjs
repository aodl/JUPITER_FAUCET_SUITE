import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@icp-sdk/core/principal';

import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';
import {
  cmcDepositAccount,
  loadCmcTopUpTransfersFromIndex,
  loadIncomingIcpTransfersFromIndex,
  loadRecentRouteTransfersFromIndex,
} from '../src/data/index-transactions.js';

function transferTx(id, { from, to, amount = 10n, memo } = {}) {
  const tx = {
    id: BigInt(id),
    transaction: {
      operation: { Transfer: { from, to, amount: { e8s: amount } } },
      timestamp: [{ timestamp_nanos: BigInt(id) * 1_000_000n }],
    },
  };
  if (memo !== undefined) {
    tx.transaction.icrc1_memo = memo;
  }
  return tx;
}

test('loadIncomingIcpTransfersFromIndex filters incoming transfers and marks source and memo matches', async () => {
  const account = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const to = accountIdentifierHex(account);
  const source = 'a'.repeat(64);
  const other = 'b'.repeat(64);
  const index = {
    async get_account_identifier_transactions(args) {
      assert.equal(args.account_identifier, to);
      return { Ok: { transactions: [
        transferTx(3, { from: source, to, memo: [Uint8Array.from([109, 101, 109, 111])] }),
        transferTx(2, { from: other, to: 'c'.repeat(64), memo: [[109, 101, 109, 111]] }),
        transferTx(1, { from: other, to, memo: [[0xff]] }),
      ] } };
    },
  };

  const result = await loadIncomingIcpTransfersFromIndex({
    index,
    account,
    sourceAccountIdentifiers: [source],
    memoText: 'memo',
  });
  assert.equal(result.items.length, 2);
  assert.equal(result.items[0].from_account_identifier, source);
  assert.equal(result.items[0].is_matching_source, true);
  assert.equal(result.items[0].is_matching_memo, true);
  assert.equal(result.items[1].icrc1_memo_text, null);
});

test('loadIncomingIcpTransfersFromIndex distinguishes missing, empty, matching, and invalid memos', async () => {
  const account = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const to = accountIdentifierHex(account);
  const source = 'a'.repeat(64);
  const index = {
    async get_account_identifier_transactions() {
      return { Ok: { transactions: [
        transferTx(5, { from: source, to }),
        transferTx(4, { from: source, to, memo: [[]] }),
        transferTx(3, { from: source, to, memo: [[109, 101, 109, 111]] }),
        transferTx(2, { from: source, to, memo: [[0xff]] }),
      ] } };
    },
  };

  const emptyResult = await loadIncomingIcpTransfersFromIndex({
    index,
    account,
    sourceAccountIdentifiers: [source],
    memoText: '',
  });
  assert.deepEqual(emptyResult.items.map((item) => item.icrc1_memo_text), [null, '', 'memo', null]);
  assert.deepEqual(emptyResult.items.map((item) => item.is_matching_memo), [false, true, false, false]);

  const memoResult = await loadIncomingIcpTransfersFromIndex({
    index,
    account,
    sourceAccountIdentifiers: [source],
    memoText: 'memo',
  });
  assert.deepEqual(memoResult.items.map((item) => item.is_matching_memo), [false, false, true, false]);
});

test('loadIncomingIcpTransfersFromIndex decodes Candid optional Uint8Array memos for tracker matching', async () => {
  const account = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const to = accountIdentifierHex(account);
  const source = 'a'.repeat(64);
  const index = {
    async get_account_identifier_transactions() {
      return { Ok: { transactions: [
        transferTx(3, { from: source, to, memo: [Uint8Array.from([74, 117, 112, 105, 116, 101, 114])] }),
        transferTx(2, { from: source, to, memo: [Uint8Array.from([116, 101, 115, 116])] }),
        transferTx(1, { from: source, to, memo: [[0xff]] }),
      ] } };
    },
  };

  const result = await loadIncomingIcpTransfersFromIndex({
    index,
    account,
    sourceAccountIdentifiers: [source],
    memoText: 'Jupiter',
  });

  assert.deepEqual(result.items.map((item) => item.icrc1_memo_text), ['Jupiter', 'test', null]);
  assert.deepEqual(result.items.map((item) => item.is_matching_memo), [true, false, false]);
});

test('loadIncomingIcpTransfersFromIndex paginates until the requested transfer limit', async () => {
  const account = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const to = accountIdentifierHex(account);
  const source = 'a'.repeat(64);
  const calls = [];
  const progress = [];
  const pages = new Map([
    ['none', [
      transferTx(5, { from: source, to }),
      transferTx(4, { from: source, to }),
    ]],
    ['4', [
      transferTx(3, { from: source, to }),
      transferTx(2, { from: source, to }),
    ]],
  ]);
  const index = {
    async get_account_identifier_transactions(args) {
      calls.push(args);
      const key = args.start.length === 0 ? 'none' : args.start[0].toString();
      return { Ok: { transactions: pages.get(key) || [] } };
    },
  };

  const result = await loadIncomingIcpTransfersFromIndex({
    index,
    account,
    limit: 3,
    pageSize: 2,
    onProgress: (page) => progress.push({
      ids: page.items.map((item) => item.tx_id),
      loading: page.loading,
      truncated: page.truncated,
      pagesLoaded: page.pages_loaded,
    }),
  });

  assert.deepEqual(result.items.map((item) => item.tx_id), [5n, 4n, 3n]);
  assert.equal(result.truncated, true);
  assert.equal(result.limit, 3);
  assert.equal(result.page_size, 2);
  assert.equal(calls.length, 2);
  assert.deepEqual(calls.map((call) => call.start), [[], [4n]]);
  assert.deepEqual(progress, [
    { ids: [5n, 4n], loading: true, truncated: false, pagesLoaded: 1 },
    { ids: [5n, 4n, 3n], loading: false, truncated: true, pagesLoaded: 2 },
  ]);
});

test('loadIncomingIcpTransfersFromIndex paginates until a timestamp cutoff is covered', async () => {
  const account = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const to = accountIdentifierHex(account);
  const source = 'a'.repeat(64);
  const calls = [];
  const progress = [];
  const pages = new Map([
    ['none', [
      transferTx(5, { from: source, to }),
      transferTx(4, { from: source, to }),
    ]],
    ['4', [
      transferTx(3, { from: source, to }),
      transferTx(2, { from: source, to }),
    ]],
    ['2', [
      transferTx(1, { from: source, to }),
    ]],
  ]);
  const index = {
    async get_account_identifier_transactions(args) {
      calls.push(args);
      const key = args.start.length === 0 ? 'none' : args.start[0].toString();
      return { Ok: { transactions: pages.get(key) || [] } };
    },
  };

  const result = await loadIncomingIcpTransfersFromIndex({
    index,
    account,
    limit: 10,
    pageSize: 2,
    minTimestampNanos: 2_500_000n,
    onProgress: (page) => progress.push({
      ids: page.items.map((item) => item.tx_id),
      loading: page.loading,
      truncated: page.truncated,
      pagesLoaded: page.pages_loaded,
    }),
  });

  assert.deepEqual(result.items.map((item) => item.tx_id), [5n, 4n, 3n]);
  assert.equal(result.truncated, false);
  assert.equal(calls.length, 2);
  assert.deepEqual(calls.map((call) => call.start), [[], [4n]]);
  assert.deepEqual(progress, [
    { ids: [5n, 4n], loading: true, truncated: false, pagesLoaded: 1 },
    { ids: [5n, 4n, 3n], loading: false, truncated: false, pagesLoaded: 2 },
  ]);
});

const sourceAccount = { owner: Principal.fromText('uccpi-cqaaa-aaaar-qby3q-cai'), subaccount: [] };
const recipientAccount = { owner: Principal.fromText('acjuz-liaaa-aaaar-qb4qq-cai'), subaccount: [] };
const sourceId = accountIdentifierHex(sourceAccount);
const loaders = [
  ['route', loadRecentRouteTransfersFromIndex,
    { outputSourceAccount: sourceAccount, routeAccount: recipientAccount }, accountIdentifierHex(recipientAccount)],
  ['CMC', loadCmcTopUpTransfersFromIndex,
    { canisterId: recipientAccount.owner }, accountIdentifierHex(cmcDepositAccount({ canisterId: recipientAccount.owner }))],
  ['incoming', loadIncomingIcpTransfersFromIndex,
    { account: recipientAccount }, accountIdentifierHex(recipientAccount)],
];

function exclusiveIndex(transactions, { cap = Infinity } = {}) {
  const calls = [];
  return {
    calls,
    async get_account_identifier_transactions(args) {
      calls.push(args);
      const cursor = args.start[0];
      return { Ok: {
        oldest_tx_id: transactions.length ? [transactions.at(-1).id] : [],
        transactions: transactions.filter((tx) => cursor === undefined || tx.id < cursor)
          .slice(0, Math.min(Number(args.max_results), cap)),
      } };
    },
  };
}

for (const [name, load, options, destination] of loaders) {
  for (const ids of [[5, 4, 3, 2, 1], [1000, 511, 510, 2, 0]]) {
    test(`${name} loader preserves every exclusive page boundary: ${ids}`, async () => {
      const index = exclusiveIndex(ids.map((id) => transferTx(id, { from: sourceId, to: destination })));
      const result = await load({ ...options, index, pageSize: 2, maxPages: 10, limit: 20 });
      assert.deepEqual(result.items.map((item) => item.tx_id), ids.map(BigInt));
      assert.deepEqual(index.calls.map((args) => args.start), [[], [BigInt(ids[1])], [BigInt(ids[3])]]);
      if (name === 'incoming') assert.equal(result.truncated, false);
    });
  }

  test(`${name} loader follows short capped pages until the known oldest transaction`, async () => {
    const ids = [5, 4, 3, 2, 1];
    const index = exclusiveIndex(ids.map((id) => transferTx(id, { from: sourceId, to: destination })), { cap: 1 });
    const result = await load({ ...options, index, pageSize: 2, maxPages: 10, limit: 20 });
    assert.deepEqual(result.items.map((item) => item.tx_id), ids.map(BigInt));
    assert.equal(index.calls.length, 5);
  });

  test(`${name} loader respects display and page limits without losing adjacent matches`, async () => {
    const index = exclusiveIndex([5, 4, 3, 2, 1].map((id) => transferTx(id, { from: sourceId, to: destination })));
    const result = await load({ ...options, index, pageSize: 2, maxPages: 2, limit: 3 });
    assert.deepEqual(result.items.map((item) => item.tx_id), [5n, 4n, 3n]);
    assert.equal(index.calls.length, 2);
    if (name === 'incoming') assert.equal(result.truncated, true);
  });

  for (const malformed of [[5, 6], [5, -1], [5, 'invalid'], [5, null]]) {
    test(`${name} loader rejects malformed page IDs ${malformed}`, async () => {
      const transactions = malformed.map((id) => id === null
        ? { transaction: {} }
        : { ...transferTx(5, { from: sourceId, to: destination }), id });
      const index = { async get_account_identifier_transactions() { return { Ok: { transactions } }; } };
      await assert.rejects(load({ ...options, index, pageSize: 2, maxPages: 3, limit: 20 }), /ICP index.*page/i);
    });
  }

  test(`${name} loader rejects a repeated page before claiming complete history`, async () => {
    let calls = 0;
    const index = {
      async get_account_identifier_transactions() {
        calls += 1;
        return { Ok: { transactions: [5, 4].map((id) => transferTx(id, { from: sourceId, to: destination })) } };
      },
    };
    await assert.rejects(load({ ...options, index, pageSize: 2, maxPages: 10, limit: 20 }), /ICP index.*page/i);
    assert.equal(calls, 2);
  });
}

test('incoming completion and progress distinguish real caps from an exhausted exact limit', async () => {
  for (const ids of [[1, 0], []]) {
    const index = exclusiveIndex(ids.map((id) => transferTx(id, { from: sourceId, to: accountIdentifierHex(recipientAccount) })));
    const progress = [];
    const result = await loadIncomingIcpTransfersFromIndex({
      index, account: recipientAccount, limit: 2, pageSize: 2, maxPages: 1,
      onProgress: (page) => progress.push(page),
    });
    assert.equal(result.truncated, false);
    assert.equal(progress.at(-1).loading, false);
    assert.equal(progress.at(-1).truncated, false);
  }
  const index = exclusiveIndex([5, 4, 3].map((id) => transferTx(id, { from: sourceId, to: accountIdentifierHex(recipientAccount) })));
  const progress = [];
  const result = await loadIncomingIcpTransfersFromIndex({
    index, account: recipientAccount, limit: 10, pageSize: 2, maxPages: 1,
    onProgress: (page) => progress.push(page),
  });
  assert.equal(result.truncated, true);
  assert.equal(progress.at(-1).loading, false);
  assert.equal(index.calls.length, 1);
});

for (const [name, load, options, destination] of loaders.filter(([name]) => name !== 'route')) {
  test(`${name} loader applies the time cutoff across exclusive page boundaries`, async () => {
    const index = exclusiveIndex([5, 4, 3, 2, 1].map((id) => transferTx(id, { from: sourceId, to: destination })));
    const result = await load({ ...options, index, pageSize: 2, maxPages: 10, limit: 20, minTimestampNanos: 2_500_000n });
    assert.deepEqual(result.items.map((item) => item.tx_id), [5n, 4n, 3n]);
    assert.equal(index.calls.length, 2);
    if (name === 'incoming') assert.equal(result.truncated, false);
  });
}

test('an unbounded page count is rejected before an Index call', async () => {
  const index = exclusiveIndex([]);
  await assert.rejects(loadIncomingIcpTransfersFromIndex({ index, account: recipientAccount, maxPages: Infinity }), /page count/);
  assert.equal(index.calls.length, 0);
});
