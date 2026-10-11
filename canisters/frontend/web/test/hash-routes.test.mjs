import test from 'node:test';
import assert from 'node:assert/strict';

import { trackerHashForMemo, trackerHashForPrincipal, trackerStateFromHash } from '../src/app/hash-routes.js';

test('trackerStateFromHash parses legacy principal hashes', () => {
  assert.deepEqual(trackerStateFromHash('#metric-tracker-aaaaa-aa'), {
    memo: 'aaaaa-aa',
    protocolCanister: '',
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
    range: 'month',
  });
});

test('memo tracker hashes preserve trailing suffix whitespace for default, subaccount, and neuron routes', () => {
  for (const memo of [
    '22255zqaaaaaaasqf6uqcai.A ',
    '22255zqaaaaaaasqf6uqcai:1.A ',
    '123456789.A ',
  ]) {
    assert.equal(trackerStateFromHash(trackerHashForMemo({ memo })).memo, memo);
  }
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
    range: 'all',
  });
  assert.deepEqual(trackerStateFromHash('#metric-tracker-aaaaa-aa?range=year'), {
    memo: 'aaaaa-aa',
    protocolCanister: '',
    range: 'year',
  });
  assert.equal(trackerStateFromHash('#metric-tracker?range=invalid').range, 'month');
});

test('trackerStateFromHash returns the current empty shape for empty and unrelated fragments', () => {
  const expected = { memo: '', protocolCanister: '', range: 'month' };
  assert.deepEqual(trackerStateFromHash(''), expected);
  assert.deepEqual(trackerStateFromHash('#elsewhere'), expected);
});
