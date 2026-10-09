import { Principal } from '@icp-sdk/core/principal';

export const MAX_TARGET_CANISTER_MEMO_BYTES = 32;
export const MAX_NEURON_ID_MEMO_BYTES = 20;
export const EVENT_HORIZON_CANISTER_TEXT = 'eo6ei-gaaaa-aaaar-qchra-cai';

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

function numberedSubaccountFromBigInt(number) {
  if (number === 0n) return null;
  const subaccount = new Uint8Array(32);
  let remaining = number;
  for (let index = 31; index >= 24; index -= 1) {
    subaccount[index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  return subaccount;
}

export function parseNumberedSubaccountText(input, { allowEmpty = true } = {}) {
  const text = String(input ?? '').trim();
  if (!text) {
    return allowEmpty
      ? { ok: true, destinationSubaccount: null, destinationSubaccountNumberText: null, reason: '' }
      : { ok: false, destinationSubaccount: null, destinationSubaccountNumberText: null, reason: 'Destination subaccount number is required.' };
  }
  if (!/^[0-9]+$/.test(text)) {
    return { ok: false, destinationSubaccount: null, destinationSubaccountNumberText: null, reason: 'Destination subaccount must use ASCII decimal digits only.' };
  }
  const number = BigInt(text);
  if (number > MAX_U64) {
    return { ok: false, destinationSubaccount: null, destinationSubaccountNumberText: null, reason: 'Destination subaccount must fit in an unsigned 64-bit integer.' };
  }
  const canonical = number.toString();
  return {
    ok: true,
    destinationSubaccount: numberedSubaccountFromBigInt(number),
    destinationSubaccountNumberText: number === 0n ? null : canonical,
    reason: '',
  };
}

export function normalizeDestinationSubaccount(value) {
  if (value === null || value === undefined) return null;
  const bytes = value instanceof Uint8Array ? value : Uint8Array.from(value);
  if (bytes.length !== 32) throw new TypeError('Destination subaccount must contain exactly 32 bytes.');
  return bytes.every((byte) => byte === 0) ? null : Uint8Array.from(bytes);
}

export function numberedSubaccountNumberText(value) {
  const bytes = normalizeDestinationSubaccount(value);
  if (bytes === null) return null;
  if (bytes.slice(0, 24).some((byte) => byte !== 0)) {
    throw new TypeError('Destination subaccount is not a zero-padded unsigned 64-bit integer.');
  }
  let number = 0n;
  for (const byte of bytes.slice(24)) number = (number << 8n) | BigInt(byte);
  return number === 0n ? null : number.toString();
}

function parseRawDestinationText(text) {
  const destination = String(text ?? '').trim();
  const colonIndex = destination.indexOf(':');
  if (colonIndex < 0) {
    const parsed = parseDeclaredPrincipalText(destination);
    return parsed.principal
      ? { ...parsed, destinationSubaccount: null, destinationSubaccountNumberText: null }
      : parsed;
  }
  if (destination.indexOf(':', colonIndex + 1) >= 0 || /\s/.test(destination)) {
    return { principal: null, reason: 'Numbered raw ICP destinations must use owner:number with no internal whitespace.' };
  }
  const ownerText = destination.slice(0, colonIndex);
  const numberText = destination.slice(colonIndex + 1);
  const parsedOwner = parseDeclaredPrincipalText(ownerText);
  if (!parsedOwner.principal) return parsedOwner;
  const parsedNumber = parseNumberedSubaccountText(numberText, { allowEmpty: false });
  if (!parsedNumber.ok) return { principal: null, reason: parsedNumber.reason };
  return { ...parsedOwner, ...parsedNumber };
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

  if (memoText.startsWith('X.')) {
    const suffix = memoText.slice(2);
    if (!suffix) return { kind: 'invalid', reason: 'Event Horizon alias suffix is empty.' };
    const canisterId = Principal.fromText(EVENT_HORIZON_CANISTER_TEXT);
    return {
      kind: 'rawIcpCanister',
      canisterId,
      canisterText: EVENT_HORIZON_CANISTER_TEXT,
      destinationSubaccount: null,
      destinationSubaccountNumberText: null,
      outgoingMemoText: suffix,
      normalizedMemoText: memoText,
      declarationText: memoText,
    };
  }

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
    const parsed = parseRawDestinationText(leftTrimmed);
    if (!parsed.principal) return { kind: 'invalid', reason: parsed.reason };
    const normalizedDestination = parsed.destinationSubaccountNumberText === null
      ? parsed.canisterText
      : `${parsed.canisterText}:${parsed.destinationSubaccountNumberText}`;
    return {
      kind: 'rawIcpCanister',
      canisterId: parsed.principal,
      canisterText: parsed.canisterText,
      destinationSubaccount: parsed.destinationSubaccount,
      destinationSubaccountNumberText: parsed.destinationSubaccountNumberText,
      outgoingMemoText: right,
      normalizedMemoText: `${normalizedDestination}.${right}`,
      declarationText: memoText,
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
