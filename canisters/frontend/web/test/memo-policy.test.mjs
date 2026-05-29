import test from 'node:test';
import assert from 'node:assert/strict';

import { parseJupiterMemo } from '../src/memo-policy.js';

const CANISTER = '22255-zqaaa-aaaas-qf6uq-cai';
const COMPACT = CANISTER.replaceAll('-', '');

test('parseJupiterMemo accepts plain and compact canister memos', () => {
  assert.equal(parseJupiterMemo(CANISTER).kind, 'cyclesTopUp');
  assert.equal(parseJupiterMemo(COMPACT).canisterText, CANISTER);
});

test('parseJupiterMemo classifies dotted canister and neuron memos', () => {
  const canister = parseJupiterMemo(`${COMPACT}.miner`);
  assert.equal(canister.kind, 'rawIcpCanister');
  assert.equal(canister.outgoingMemoText, 'miner');
  assert.equal(canister.normalizedMemoText, `${CANISTER}.miner`);

  const neuron = parseJupiterMemo('123456789.memo');
  assert.equal(neuron.kind, 'neuronStake');
  assert.equal(neuron.neuronId, 123456789n);
  assert.equal(neuron.outgoingMemoText, 'memo');
});

test('parseJupiterMemo preserves empty right-side raw memo text', () => {
  const canister = parseJupiterMemo(`${COMPACT}.`);
  assert.equal(canister.kind, 'rawIcpCanister');
  assert.equal(canister.outgoingMemoText, '');
  assert.equal(canister.normalizedMemoText, `${CANISTER}.`);

  const neuron = parseJupiterMemo('123456789.memo');
  assert.equal(neuron.kind, 'neuronStake');
  assert.equal(neuron.outgoingMemoText, 'memo');
});

test('parseJupiterMemo accepts plain non-zero numeric neuron IDs', () => {
  const parsed = parseJupiterMemo('11614578985374291210');
  assert.equal(parsed.kind, 'neuronStake');
  assert.equal(parsed.outgoingMemoText, null);
});

test('parseJupiterMemo enforces the Rust u64 neuron ID limit', () => {
  const max = parseJupiterMemo('18446744073709551615');
  assert.equal(max.kind, 'neuronStake');
  assert.equal(max.neuronId, 18_446_744_073_709_551_615n);

  assert.equal(parseJupiterMemo('18446744073709551616').kind, 'invalid');
  assert.equal(parseJupiterMemo('99999999999999999999').kind, 'invalid');
});

test('parseJupiterMemo rejects invalid memo inputs with UI-safe reasons', () => {
  for (const input of ['', '   ', 'abcé', `${CANISTER}.this-memo-is-too-long-for-policy`, '0', 'not-a-principal', 'aaaaa-aa', '2vxsx-fae', CANISTER.slice(0, -1)]) {
    assert.equal(parseJupiterMemo(input).kind, 'invalid', input);
    assert.equal(typeof parseJupiterMemo(input).reason, 'string');
  }
});

test('parseJupiterMemo preserves right-side raw memo text exactly', () => {
  const parsed = parseJupiterMemo(`${COMPACT}. abc `);
  assert.equal(parsed.kind, 'rawIcpCanister');
  assert.equal(parsed.outgoingMemoText, ' abc ');
});
