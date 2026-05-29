import test from 'node:test';
import assert from 'node:assert/strict';

import { trackerHashForMemo, trackerHashForPrincipal, trackerStateFromHash } from '../src/app/hash-routes.js';

test('trackerStateFromHash parses legacy principal hashes', () => {
  assert.deepEqual(trackerStateFromHash('#metric-tracker-aaaaa-aa'), {
    memo: 'aaaaa-aa',
    protocolCanister: '',
    legacyPrincipal: 'aaaaa-aa',
  });
});

test('trackerHashForMemo and trackerStateFromHash preserve dotted memos and protocol canisters', () => {
  const hash = trackerHashForMemo({
    memo: '22255-zqaaa-aaaas-qf6uq-cai.a memo',
    protocolCanister: 'aaaaa-aa',
  });
  assert.equal(hash, '#metric-tracker?memo=22255-zqaaa-aaaas-qf6uq-cai.a+memo&protocol-canister=aaaaa-aa');
  assert.deepEqual(trackerStateFromHash(hash), {
    memo: '22255-zqaaa-aaaas-qf6uq-cai.a memo',
    protocolCanister: 'aaaaa-aa',
    legacyPrincipal: '',
  });
});

test('trackerHashForPrincipal remains backwards-compatible', () => {
  assert.equal(trackerHashForPrincipal('aaaaa-aa'), '#metric-tracker-aaaaa-aa');
});
