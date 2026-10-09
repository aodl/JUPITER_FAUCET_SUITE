import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import {
  normalizeDestinationSubaccount,
  numberedSubaccountNumberText,
  parseJupiterMemo,
  parseNumberedSubaccountText,
} from '../src/memo-policy.js';

const CANISTER = '22255-zqaaa-aaaas-qf6uq-cai';
const COMPACT = CANISTER.replaceAll('-', '');
const MEMO_POLICY_CASES = JSON.parse(readFileSync(
  new URL('../../../../crates/memo-policy/fixtures/memo-policy-cases.json', import.meta.url),
  'utf8',
));

function bytesHex(bytes) {
  return bytes === null ? null : Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

test('parseJupiterMemo matches the canonical shared fixture corpus', () => {
  for (const fixture of MEMO_POLICY_CASES) {
    const memo = fixture.memo_hex
      ? new TextDecoder().decode(Uint8Array.from(fixture.memo_hex.match(/../g).map((byte) => Number.parseInt(byte, 16))))
      : fixture.memo_text;
    const parsed = parseJupiterMemo(memo);
    const expected = fixture.expected_directive;
    if (expected === null) {
      assert.equal(parsed.kind, 'invalid', fixture.name);
      continue;
    }
    if (expected.kind === 'top_up') {
      assert.equal(parsed.kind, 'cyclesTopUp', fixture.name);
      assert.equal(parsed.canisterId.toText(), expected.canister_id, fixture.name);
    } else if (expected.kind === 'raw_icp') {
      assert.equal(parsed.kind, 'rawIcpCanister', fixture.name);
      assert.equal(parsed.canisterId.toText(), expected.canister_id, fixture.name);
      assert.equal(parsed.outgoingMemoText, expected.memo_text, fixture.name);
      assert.equal(bytesHex(parsed.destinationSubaccount), expected.destination_subaccount_hex ?? null, fixture.name);
    } else {
      assert.equal(parsed.kind, 'neuronStake', fixture.name);
      assert.equal(parsed.neuronId, BigInt(expected.neuron_id), fixture.name);
      assert.equal(parsed.outgoingMemoText, expected.memo_text, fixture.name);
    }
  }
});

test('numbered subaccount helpers use strict zero-padded big-endian u64 values', () => {
  for (const numberText of ['0', '0000', '1', '50', '255', '256', '9007199254740993', '18446744073709551615']) {
    const parsed = parseNumberedSubaccountText(numberText);
    assert.equal(parsed.ok, true, numberText);
    const expected = BigInt(numberText) === 0n ? null : BigInt(numberText).toString();
    assert.equal(parsed.destinationSubaccountNumberText, expected);
    assert.equal(numberedSubaccountNumberText(parsed.destinationSubaccount), expected);
  }
  assert.equal(parseNumberedSubaccountText('18446744073709551616').ok, false);
  assert.throws(() => normalizeDestinationSubaccount(new Uint8Array(31)), /32 bytes/);
  const unsupported = new Uint8Array(32);
  unsupported[0] = 1;
  assert.throws(() => numberedSubaccountNumberText(unsupported), /zero-padded/);
});

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

test('parseJupiterMemo resolves the exact Event Horizon X alias without expanding display text', () => {
  const memo = 'X.uccpicqaaaaaaarqby3qcai.n0:1';
  const parsed = parseJupiterMemo(memo);
  assert.equal(parsed.kind, 'rawIcpCanister');
  assert.equal(parsed.canisterId.toText(), 'eo6ei-gaaaa-aaaar-qchra-cai');
  assert.equal(parsed.outgoingMemoText, 'uccpicqaaaaaaarqby3qcai.n0:1');
  assert.equal(parsed.normalizedMemoText, memo);
  assert.equal(parseJupiterMemo('X.').kind, 'invalid');
  assert.equal(parseJupiterMemo('x.foo').kind, 'invalid');
});
