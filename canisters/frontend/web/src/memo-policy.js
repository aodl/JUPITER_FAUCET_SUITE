import { Principal } from '@icp-sdk/core/principal';

export const MAX_TARGET_CANISTER_MEMO_BYTES = 32;
export const MAX_NEURON_ID_MEMO_BYTES = 20;

const ANONYMOUS_PRINCIPAL_TEXT = '2vxsx-fae';
const MANAGEMENT_PRINCIPAL_TEXT = 'aaaaa-aa';
const MAX_U64 = 18_446_744_073_709_551_615n;

function byteLength(text) {
  return new TextEncoder().encode(text).length;
}

function hasOnlyAscii(text) {
  return /^[\x00-\x7f]*$/.test(text);
}

function principalTextWithGroupSeparators(text) {
  if (text.includes('-')) return text;
  return Array.from(text).reduce((out, ch, index) => `${out}${index > 0 && index % 5 === 0 ? '-' : ''}${ch}`, '');
}

function parseDeclaredPrincipalText(text) {
  const trimmed = String(text || '').trim();
  if (!trimmed) return { principal: null, reason: 'Memo target is empty.' };
  if (byteLength(trimmed) > MAX_TARGET_CANISTER_MEMO_BYTES) {
    return { principal: null, reason: 'Memo target is longer than 32 bytes.' };
  }
  let principal;
  try {
    principal = Principal.fromText(principalTextWithGroupSeparators(trimmed));
  } catch {
    return { principal: null, reason: 'Memo target is not a valid canister principal or neuron ID.' };
  }
  const principalText = principal.toText();
  if (principalText === ANONYMOUS_PRINCIPAL_TEXT) {
    return { principal: null, reason: 'Anonymous principal cannot be used as a Jupiter Faucet memo target.' };
  }
  if (principalText === MANAGEMENT_PRINCIPAL_TEXT) {
    return { principal: null, reason: 'The management canister principal cannot be used as a Jupiter Faucet memo target.' };
  }
  return { principal, canisterText: principalText, reason: '' };
}

function parseNeuronIdText(text) {
  if (!text || byteLength(text) > MAX_NEURON_ID_MEMO_BYTES || !/^[0-9]+$/.test(text)) return null;
  const neuronId = BigInt(text);
  return neuronId === 0n || neuronId > MAX_U64 ? null : neuronId;
}

export function parseJupiterMemo(input) {
  const memoText = String(input ?? '');
  if (memoText.length === 0) return { kind: 'invalid', reason: 'Paste a memo first.' };
  if (!hasOnlyAscii(memoText)) return { kind: 'invalid', reason: 'Memo must contain ASCII characters only.' };
  if (byteLength(memoText) > MAX_TARGET_CANISTER_MEMO_BYTES) {
    return { kind: 'invalid', reason: 'Memo must be no more than 32 bytes.' };
  }
  const trimmed = memoText.trim();
  if (!trimmed) return { kind: 'invalid', reason: 'Paste a non-empty memo first.' };

  const dotIndex = memoText.indexOf('.');
  if (dotIndex >= 0) {
    const left = memoText.slice(0, dotIndex);
    const right = memoText.slice(dotIndex + 1);
    const leftTrimmed = left.trim();
    const neuronId = parseNeuronIdText(leftTrimmed);
    if (neuronId !== null) {
      return {
        kind: 'neuronStake',
        neuronId,
        outgoingMemoText: right,
        normalizedMemoText: `${leftTrimmed}.${right}`,
      };
    }
    const parsed = parseDeclaredPrincipalText(leftTrimmed);
    if (!parsed.principal) return { kind: 'invalid', reason: parsed.reason };
    return {
      kind: 'rawIcpCanister',
      canisterId: parsed.principal,
      canisterText: parsed.canisterText,
      outgoingMemoText: right,
      normalizedMemoText: `${parsed.canisterText}.${right}`,
    };
  }

  const neuronId = parseNeuronIdText(trimmed);
  if (neuronId !== null) {
    return {
      kind: 'neuronStake',
      neuronId,
      outgoingMemoText: null,
      normalizedMemoText: trimmed,
    };
  }

  if (/^[0-9]+$/.test(trimmed)) {
    return { kind: 'invalid', reason: 'Neuron memo must be a non-zero decimal neuron ID no longer than 20 bytes.' };
  }

  const parsed = parseDeclaredPrincipalText(memoText);
  if (!parsed.principal) return { kind: 'invalid', reason: parsed.reason };
  return {
    kind: 'cyclesTopUp',
    canisterId: parsed.principal,
    canisterText: parsed.canisterText,
    normalizedMemoText: parsed.canisterText,
  };
}
