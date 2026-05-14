import { Principal } from '@icp-sdk/core/principal';

export const JUPITER_MEMO_MAX_BYTES = 32;
export const JUPITER_NEURON_ID_MAX_DIGITS = 20;
export const U64_MAX = 18446744073709551615n;

export function isAscii(text) {
  return /^[\x00-\x7F]*$/.test(String(text ?? ''));
}

export function sanitizeCanisterPrincipalText(text) {
  return String(text ?? '').trim().toLowerCase();
}

export function sanitizeNeuronIdText(text) {
  return String(text ?? '').trim();
}

function emptyResult({ truncatedOptionalMemo = '', errors = [], warnings = [] } = {}) {
  return {
    ok: false,
    output: '',
    usedBytes: 0,
    maxBytes: JUPITER_MEMO_MAX_BYTES,
    availableOptionalMemoBytes: 0,
    keptOptionalMemo: '',
    truncatedOptionalMemo,
    warnings,
    errors,
  };
}

function splitOptionalMemo(optionalMemoText, available) {
  const safeAvailable = Math.max(0, available);
  return {
    keptOptionalMemo: optionalMemoText.slice(0, safeAvailable),
    truncatedOptionalMemo: optionalMemoText.slice(safeAvailable),
  };
}

function principalTextWithGroupSeparators(text) {
  if (text.includes('-')) return text;
  return Array.from(text).reduce((out, ch, index) => (
    index > 0 && index % 5 === 0 ? `${out}-${ch}` : `${out}${ch}`
  ), '');
}

function parseDeclaredPrincipalText(text) {
  const trimmed = text.trim();
  if (!trimmed) return null;
  try {
    const principal = Principal.fromText(principalTextWithGroupSeparators(trimmed));
    const canonical = principal.toText();
    if (canonical === '2vxsx-fae' || canonical === 'aaaaa-aa') {
      return null;
    }
    return principal;
  } catch {
    return null;
  }
}

export function canonicalDeclaredPrincipalText(text) {
  return parseDeclaredPrincipalText(String(text ?? ''))?.toText() || '';
}

export function advancedMemoUrlPrefillState({ canister = null, neuron = null, requestedMode = '' } = {}) {
  const sanitizedCanister = canister === null ? '' : sanitizeCanisterPrincipalText(canister);
  const sanitizedNeuron = canister === null && neuron !== null ? sanitizeNeuronIdText(neuron) : '';
  const target = sanitizedCanister || sanitizedNeuron;
  const targetType = sanitizedCanister ? 'canister' : sanitizedNeuron ? 'neuron' : '';
  const mode = sanitizedCanister ? (requestedMode !== 'cycles' ? 'rawIcp' : 'cycles') : sanitizedNeuron ? 'neuron' : '';

  return {
    sanitizedCanister,
    sanitizedNeuron,
    target,
    targetType,
    displayTarget: targetType === 'canister' ? canonicalDeclaredPrincipalText(target) || target : target,
    mode,
    locksTarget: Boolean(target),
    hasEmptySuppliedTarget: !target && (canister !== null || neuron !== null),
  };
}

export function shouldApplyAdvancedMemoUrlTargetValue(currentFragment = '', lastAppliedPrefillFragment = '') {
  return currentFragment !== lastAppliedPrefillFragment;
}

export function advancedMemoValidationMessages(result, { lockedTargetText = '', lockedTargetType = '' } = {}) {
  const errors = result?.errors || [];
  const warnings = result?.warnings || [];
  const mappedErrors = lockedTargetType === 'canister' && lockedTargetText
    ? errors.map((error) => (
      error === 'Enter a valid non-anonymous declared canister ID.'
        ? `'${lockedTargetText}' is not a valid declared canister ID.`
        : error
    ))
    : errors;
  return [...mappedErrors, ...warnings];
}

export function isValidNeuronId(text) {
  if (!/^\d+$/.test(text)) return false;
  if (text.length > JUPITER_NEURON_ID_MAX_DIGITS) return false;
  const value = BigInt(text);
  return value > 0n && value <= U64_MAX;
}

export function buildAdvancedMemo({
  mode = 'cycles',
  canisterText = '',
  neuronIdText = '',
  optionalMemoText = '',
} = {}) {
  const targetText = mode === 'neuron' ? String(neuronIdText ?? '') : String(canisterText ?? '');
  const optional = String(optionalMemoText ?? '');
  const asciiInputs = [targetText, optional];
  if (!asciiInputs.every(isAscii)) {
    return emptyResult({
      truncatedOptionalMemo: optional,
      errors: ['Jupiter Faucet memos must be ASCII only.'],
    });
  }

  if (mode === 'neuron') {
    const neuronId = targetText.trim();
    const errors = [];
    if (!neuronId) {
      return emptyResult({ truncatedOptionalMemo: optional });
    }
    if (neuronId && !/^\d+$/.test(neuronId)) errors.push('Neuron IDs must use ASCII digits only.');
    if (/^\d+$/.test(neuronId) && !isValidNeuronId(neuronId)) {
      errors.push('Neuron ID must be a non-zero u64 value.');
    }
    if (errors.length > 0) {
      return emptyResult({ truncatedOptionalMemo: optional, errors });
    }

    const hasOptionalMemo = optional.length > 0;
    const prefix = hasOptionalMemo ? `${neuronId}.` : neuronId;
    const available = hasOptionalMemo ? JUPITER_MEMO_MAX_BYTES - prefix.length : 0;
    const { keptOptionalMemo, truncatedOptionalMemo } = hasOptionalMemo
      ? splitOptionalMemo(optional, available)
      : { keptOptionalMemo: '', truncatedOptionalMemo: '' };
    const output = `${prefix}${keptOptionalMemo}`;
    return {
      ok: true,
      output,
      usedBytes: output.length,
      maxBytes: JUPITER_MEMO_MAX_BYTES,
      availableOptionalMemoBytes: Math.max(0, available),
      keptOptionalMemo,
      truncatedOptionalMemo,
      warnings: [],
      errors: [],
    };
  }

  const principal = targetText.trim();
  const memoPrincipal = principal.replaceAll('-', '');
  if (!memoPrincipal) {
    return emptyResult({ truncatedOptionalMemo: optional });
  }
  if (principal.includes('.')) {
    return emptyResult({
      truncatedOptionalMemo: optional,
      errors: ['Declared canister ID text cannot contain a dot.'],
    });
  }

  if (mode === 'cycles' && principal.length > JUPITER_MEMO_MAX_BYTES) {
    return emptyResult({
      truncatedOptionalMemo: optional,
      errors: ['Declared canister ID exceeds the 32-byte ICP memo limit.'],
    });
  }

  if (!parseDeclaredPrincipalText(principal)) {
    return emptyResult({
      truncatedOptionalMemo: optional,
      errors: ['Enter a valid non-anonymous declared canister ID.'],
    });
  }

  if (mode === 'cycles') {
    return {
      ok: true,
      output: principal,
      usedBytes: principal.length,
      maxBytes: JUPITER_MEMO_MAX_BYTES,
      availableOptionalMemoBytes: 0,
      keptOptionalMemo: '',
      truncatedOptionalMemo: '',
      warnings: [],
      errors: [],
    };
  }

  const prefix = `${memoPrincipal}.`;
  const available = JUPITER_MEMO_MAX_BYTES - prefix.length;
  if (available < 0) {
    return emptyResult({
      truncatedOptionalMemo: optional,
      errors: ['Declared canister ID plus dot exceeds the 32-byte ICP memo limit.'],
    });
  }

  const { keptOptionalMemo, truncatedOptionalMemo } = splitOptionalMemo(optional, available);
  const output = `${prefix}${keptOptionalMemo}`;
  return {
    ok: true,
    output,
    usedBytes: output.length,
    maxBytes: JUPITER_MEMO_MAX_BYTES,
    availableOptionalMemoBytes: Math.max(0, available),
    keptOptionalMemo,
    truncatedOptionalMemo,
    warnings: [],
    errors: [],
  };
}
