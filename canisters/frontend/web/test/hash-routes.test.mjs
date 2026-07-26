import test from 'node:test';
import assert from 'node:assert/strict';

import { trackerHashForMemo, trackerHashForPrincipal, trackerStateFromHash } from '../src/app/hash-routes.js';

test('trackerStateFromHash parses legacy principal hashes', () => {
  assert.deepEqual(trackerStateFromHash('#metric-tracker-aaaaa-aa'), {
    memo: 'aaaaa-aa',
    protocolCanister: '',
    legacyPrincipal: 'aaaaa-aa',
    range: 'month',
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
    range: 'month',
  });
});

test('trackerHashForPrincipal remains backwards-compatible', () => {
  assert.equal(trackerHashForPrincipal('aaaaa-aa'), '#metric-tracker-aaaaa-aa');
});

test('tracker range route state is shareable for query and legacy hashes', () => {
  assert.equal(trackerHashForMemo({ range: 'year' }), '#metric-tracker?range=year');
  assert.equal(
    trackerHashForMemo({ memo: '22255-zqaaa-aaaas-qf6uq-cai', protocolCanister: 'aaaaa-aa', range: 'all' }),
    '#metric-tracker?memo=22255-zqaaa-aaaas-qf6uq-cai&protocol-canister=aaaaa-aa&range=all',
  );
  assert.equal(trackerHashForPrincipal('aaaaa-aa', { range: 'year' }), '#metric-tracker-aaaaa-aa?range=year');
  assert.deepEqual(trackerStateFromHash('#metric-tracker?range=all'), {
    memo: '',
    protocolCanister: '',
    legacyPrincipal: '',
    range: 'all',
  });
  assert.deepEqual(trackerStateFromHash('#metric-tracker-aaaaa-aa?range=year'), {
    memo: 'aaaaa-aa',
    protocolCanister: '',
    legacyPrincipal: 'aaaaa-aa',
    range: 'year',
  });
  assert.equal(trackerStateFromHash('#metric-tracker?range=invalid').range, 'month');
});
