import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@icp-sdk/core/principal';

import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';
import { loadIncomingIcpTransfersFromIndex } from '../src/data/index-transactions.js';

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
    ['3', [
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
  assert.deepEqual(calls.map((call) => call.start), [[], [3n]]);
  assert.deepEqual(progress, [
    { ids: [5n, 4n], loading: true, truncated: false, pagesLoaded: 1 },
    { ids: [5n, 4n, 3n], loading: false, truncated: true, pagesLoaded: 2 },
  ]);
});
