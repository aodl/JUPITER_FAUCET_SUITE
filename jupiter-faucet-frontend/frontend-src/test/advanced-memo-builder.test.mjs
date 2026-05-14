import test from 'node:test';
import assert from 'node:assert/strict';

import {
  JUPITER_MEMO_MAX_BYTES,
  advancedMemoUrlPrefillState,
  advancedMemoValidationMessages,
  buildAdvancedMemo,
  canonicalDeclaredPrincipalText,
  isValidNeuronId,
  sanitizeCanisterPrincipalText,
  sanitizeNeuronIdText,
  shouldApplyAdvancedMemoUrlTargetValue,
} from '../src/advanced-memo-builder.js';

const COMPACT_CANISTER = '22255zqaaaaaaasqf6uqcai';
const CANONICAL_CANISTER = '22255-zqaaa-aaaas-qf6uq-cai';
const SHORT_CANISTER = '2ibo7-dia';
const COMPACT_SHORT_CANISTER = '2ibo7dia';

test('target input sanitizers trim without rewriting target identity', () => {
  assert.equal(sanitizeCanisterPrincipalText(' 22255-ZQAAA.aaaa_asqf6-uqcai-1089 '), '22255-zqaaa.aaaa_asqf6-uqcai-1089');
  assert.equal(sanitizeNeuronIdText(' neuron 42.7 '), 'neuron 42.7');
});

test('empty target fields do not show semantic validation errors', () => {
  const canister = buildAdvancedMemo({ mode: 'cycles', canisterText: '' });
  const neuron = buildAdvancedMemo({ mode: 'neuron', neuronIdText: '' });

  assert.equal(canister.ok, false);
  assert.equal(canister.output, '');
  assert.deepEqual(canister.errors, []);
  assert.equal(neuron.ok, false);
  assert.equal(neuron.output, '');
  assert.deepEqual(neuron.errors, []);
});

test('cycles top-up memo uses the plain declared canister ID', () => {
  const result = buildAdvancedMemo({
    mode: 'cycles',
    canisterText: COMPACT_CANISTER,
    optionalMemoText: 'ignored',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, COMPACT_CANISTER);
  assert.equal(result.usedBytes, COMPACT_CANISTER.length);
  assert.equal(result.availableOptionalMemoBytes, 0);
  assert.equal(result.truncatedOptionalMemo, '');
  assert.deepEqual(result.warnings, []);
});

test('cycles top-up memo preserves canister hyphens in the generated memo', () => {
  const result = buildAdvancedMemo({
    mode: 'cycles',
    canisterText: CANONICAL_CANISTER,
    optionalMemoText: 'ignored',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, CANONICAL_CANISTER);
  assert.equal(result.usedBytes, result.output.length);
  assert.match(result.output, /-/);
});

test('malformed canister text is invalid and not copyable', () => {
  const result = buildAdvancedMemo({
    mode: 'cycles',
    canisterText: 'abc',
  });

  assert.equal(result.ok, false);
  assert.equal(result.output, '');
  assert.match(result.errors.join(' '), /valid non-anonymous declared canister ID/);
});

test('cycles top-up memo rejects declared canister ID text over the 32-byte limit', () => {
  const result = buildAdvancedMemo({
    mode: 'cycles',
    canisterText: 'abcdefghijklmnopqrstuvwxyzabcdefg',
  });

  assert.equal(result.ok, false);
  assert.equal(result.output, '');
  assert.match(result.errors.join(' '), /exceeds the 32-byte ICP memo limit/);
});

test('principal display canonicalizes valid compact canister targets only', () => {
  assert.equal(canonicalDeclaredPrincipalText(COMPACT_CANISTER), CANONICAL_CANISTER);
  assert.equal(canonicalDeclaredPrincipalText(CANONICAL_CANISTER), CANONICAL_CANISTER);
  assert.equal(canonicalDeclaredPrincipalText('abc'), '');
  assert.equal(canonicalDeclaredPrincipalText('2vxsx-fae'), '');
  assert.equal(canonicalDeclaredPrincipalText('aaaaa-aa'), '');
});

test('URL prefill state does not lock empty supplied targets', () => {
  for (const args of [{ canister: '' }, { neuron: '' }]) {
    const state = advancedMemoUrlPrefillState(args);
    assert.equal(state.target, '');
    assert.equal(state.displayTarget, '');
    assert.equal(state.mode, '');
    assert.equal(state.locksTarget, false);
    assert.equal(state.hasEmptySuppliedTarget, true);
  }
});

test('URL prefill state treats empty canister with requested mode as manually editable', () => {
  const state = advancedMemoUrlPrefillState({ canister: '', requestedMode: 'rawIcp' });

  assert.equal(state.target, '');
  assert.equal(state.displayTarget, '');
  assert.equal(state.mode, '');
  assert.equal(state.locksTarget, false);
  assert.equal(state.hasEmptySuppliedTarget, true);
});

test('URL target values apply once per fragment even when empty target has a label', () => {
  const fragment = '#how-it-works:3?canister=&label=Foo';

  assert.equal(shouldApplyAdvancedMemoUrlTargetValue(fragment, ''), true);
  assert.equal(shouldApplyAdvancedMemoUrlTargetValue(fragment, fragment), false);
});

test('validation messages name invalid locked URL canister targets', () => {
  const result = buildAdvancedMemo({ mode: 'rawIcp', canisterText: 'abc' });

  assert.deepEqual(
    advancedMemoValidationMessages(result, {
      lockedTargetText: 'abc',
      lockedTargetType: 'canister',
    }),
    ["'abc' is not a valid declared canister ID."],
  );
  assert.deepEqual(
    advancedMemoValidationMessages(result, {
      lockedTargetText: '',
      lockedTargetType: '',
    }),
    ['Enter a valid non-anonymous declared canister ID.'],
  );
});

test('URL prefill state locks non-empty targets and canonicalizes canister display', () => {
  const compactCanister = advancedMemoUrlPrefillState({
    canister: COMPACT_CANISTER,
    requestedMode: 'rawIcp',
  });
  assert.equal(compactCanister.target, COMPACT_CANISTER);
  assert.equal(compactCanister.displayTarget, CANONICAL_CANISTER);
  assert.equal(compactCanister.mode, 'rawIcp');
  assert.equal(compactCanister.locksTarget, true);

  const invalidCanister = advancedMemoUrlPrefillState({ canister: 'abc' });
  assert.equal(invalidCanister.target, 'abc');
  assert.equal(invalidCanister.displayTarget, 'abc');
  assert.equal(invalidCanister.mode, 'rawIcp');
  assert.equal(invalidCanister.locksTarget, true);

  const overflowNeuron = advancedMemoUrlPrefillState({ neuron: '18446744073709551616' });
  assert.equal(overflowNeuron.target, '18446744073709551616');
  assert.equal(overflowNeuron.displayTarget, '18446744073709551616');
  assert.equal(overflowNeuron.mode, 'neuron');
  assert.equal(overflowNeuron.locksTarget, true);
});

test('raw ICP canister compact memo reaches exactly 32 bytes with 8 optional characters', () => {
  const result = buildAdvancedMemo({
    mode: 'rawIcp',
    canisterText: COMPACT_CANISTER,
    optionalMemoText: '12345678',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, `${COMPACT_CANISTER}.12345678`);
  assert.equal(result.output.length, JUPITER_MEMO_MAX_BYTES);
  assert.equal(result.usedBytes, JUPITER_MEMO_MAX_BYTES);
  assert.equal(result.truncatedOptionalMemo, '');
});

test('raw ICP canister compact memo truncates optional overflow beyond the parser edge case', () => {
  const result = buildAdvancedMemo({
    mode: 'rawIcp',
    canisterText: COMPACT_CANISTER,
    optionalMemoText: '123456789',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, `${COMPACT_CANISTER}.12345678`);
  assert.equal(result.output.endsWith('12345678'), true);
  assert.equal(result.truncatedOptionalMemo, '9');
  assert.deepEqual(result.warnings, []);
});

test('raw ICP canister memo strips canister hyphens only from the generated memo', () => {
  const canisterText = sanitizeCanisterPrincipalText(CANONICAL_CANISTER);
  const result = buildAdvancedMemo({
    mode: 'rawIcp',
    canisterText,
    optionalMemoText: '',
  });

  assert.equal(result.ok, true);
  assert.equal(canisterText, CANONICAL_CANISTER);
  assert.equal(result.output, `${COMPACT_CANISTER}.`);
  assert.doesNotMatch(result.warnings.join(' '), /Remove hyphens/);
});

test('neuron bare memo omits separator when optional memo is empty', () => {
  const result = buildAdvancedMemo({
    mode: 'neuron',
    neuronIdText: '42',
    optionalMemoText: '',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, '42');
});

test('neuron memo includes separator and outgoing memo text', () => {
  const result = buildAdvancedMemo({
    mode: 'neuron',
    neuronIdText: '42',
    optionalMemoText: 'vault.memo',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, '42.vault.memo');
});

test('neuron optional memo truncates to the 32-byte limit', () => {
  const result = buildAdvancedMemo({
    mode: 'neuron',
    neuronIdText: '42',
    optionalMemoText: '123456789012345678901234567890',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, '42.12345678901234567890123456789');
  assert.equal(result.output.length, JUPITER_MEMO_MAX_BYTES);
  assert.equal(result.truncatedOptionalMemo, '0');
});

test('non-ASCII input is invalid and not copyable', () => {
  const result = buildAdvancedMemo({
    mode: 'cycles',
    canisterText: COMPACT_CANISTER,
    optionalMemoText: 'mémö',
  });

  assert.equal(result.ok, false);
  assert.equal(result.output, '');
  assert.match(result.errors.join(' '), /ASCII only/);
});

test('invalid neuron IDs are rejected', () => {
  for (const neuronIdText of ['0', '0000', 'abc', '18446744073709551616', '99999999999999999999', '123456789012345678901']) {
    const result = buildAdvancedMemo({
      mode: 'neuron',
      neuronIdText,
      optionalMemoText: '',
    });
    assert.equal(result.ok, false, `${neuronIdText} should be invalid`);
    assert.equal(result.output, '');
  }
});

test('neuron ID validation matches non-zero u64 parser bounds', () => {
  assert.equal(isValidNeuronId('18446744073709551615'), true);

  for (const neuronIdText of ['0', '0000', '18446744073709551616', '99999999999999999999', '123456789012345678901']) {
    assert.equal(isValidNeuronId(neuronIdText), false, `${neuronIdText} should be invalid`);
  }
});

test('frontend memo builder mirrors jupiter-memo-policy parser corpus', () => {
  const validCases = [
    {
      label: 'canonical valid canister ID',
      args: { mode: 'cycles', canisterText: CANONICAL_CANISTER },
      output: CANONICAL_CANISTER,
    },
    {
      label: 'compact valid canister ID',
      args: { mode: 'cycles', canisterText: COMPACT_CANISTER },
      output: COMPACT_CANISTER,
    },
    {
      label: 'short parser-accepted principal',
      args: { mode: 'cycles', canisterText: SHORT_CANISTER },
      output: SHORT_CANISTER,
    },
    {
      label: 'compact short parser-accepted principal',
      args: { mode: 'cycles', canisterText: COMPACT_SHORT_CANISTER },
      output: COMPACT_SHORT_CANISTER,
    },
    {
      label: 'raw ICP memo exactly 32 bytes',
      args: { mode: 'rawIcp', canisterText: COMPACT_CANISTER, optionalMemoText: '12345678' },
      output: `${COMPACT_CANISTER}.12345678`,
    },
    {
      label: 'raw ICP with empty right-hand memo segment',
      args: { mode: 'rawIcp', canisterText: COMPACT_CANISTER, optionalMemoText: '' },
      output: `${COMPACT_CANISTER}.`,
    },
    {
      label: 'raw ICP preserving right-hand memo text after first dot',
      args: { mode: 'rawIcp', canisterText: COMPACT_CANISTER, optionalMemoText: 'swap.7' },
      output: `${COMPACT_CANISTER}.swap.7`,
    },
    {
      label: 'neuron ID without optional memo',
      args: { mode: 'neuron', neuronIdText: '42', optionalMemoText: '' },
      output: '42',
    },
    {
      label: 'neuron ID with optional memo',
      args: { mode: 'neuron', neuronIdText: '42', optionalMemoText: 'vault.memo' },
      output: '42.vault.memo',
    },
    {
      label: 'neuron ID at u64::MAX',
      args: { mode: 'neuron', neuronIdText: '18446744073709551615', optionalMemoText: '' },
      output: '18446744073709551615',
    },
  ];

  for (const { label, args, output } of validCases) {
    const result = buildAdvancedMemo(args);
    assert.equal(result.ok, true, label);
    assert.equal(result.output, output, label);
    assert.equal(result.output.length <= JUPITER_MEMO_MAX_BYTES, true, label);
    assert.deepEqual(result.errors, [], label);
  }

  const invalidCases = [
    { label: 'malformed declared canister ID', args: { mode: 'cycles', canisterText: 'abc' } },
    { label: 'anonymous principal', args: { mode: 'cycles', canisterText: '2vxsx-fae' } },
    { label: 'management canister', args: { mode: 'cycles', canisterText: 'aaaaa-aa' } },
    { label: 'cycles memo over 32 bytes', args: { mode: 'cycles', canisterText: '33mql-r6bnm-7mzbp-gqvmp-iv6qr-5j3pw-tnwsf-f2az7-zppun-yb4lf-zae' } },
    { label: 'neuron ID over u64::MAX', args: { mode: 'neuron', neuronIdText: '18446744073709551616' } },
    { label: 'zero neuron ID', args: { mode: 'neuron', neuronIdText: '0' } },
    { label: 'non-ASCII input', args: { mode: 'cycles', canisterText: `${COMPACT_CANISTER}é` } },
  ];

  for (const { label, args } of invalidCases) {
    const result = buildAdvancedMemo(args);
    assert.equal(result.ok, false, label);
    assert.equal(result.output, '', label);
  }
});

test('raw ICP optional memo overflow is visibly truncated to a parser-accepted memo', () => {
  const result = buildAdvancedMemo({
    mode: 'rawIcp',
    canisterText: COMPACT_CANISTER,
    optionalMemoText: '123456789',
  });

  assert.equal(result.ok, true);
  assert.equal(result.output, `${COMPACT_CANISTER}.12345678`);
  assert.equal(result.output.length, JUPITER_MEMO_MAX_BYTES);
  assert.equal(result.truncatedOptionalMemo, '9');
});
