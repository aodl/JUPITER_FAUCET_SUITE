import { Principal } from '@icp-sdk/core/principal';
import { createActor as createLedgerActor } from '../../declarations/icp_ledger/index.js';
import { createActor as createGovernanceActor } from '../../declarations/nns_governance/index.js';
import { createHistorianClient, normalizeError } from './agent.js';
import { GOVERNANCE_CANISTER_ID } from './config.js';
import { accountIdentifierHex, bytesToHex, readOptional } from '../data/dashboard-transforms.js';
import { loadPublicNeuronStakingAccount } from '../data/nns-neurons.js';
import { DASH, formatIcpE8s, renderCanisterTrackerLink } from './view-formatters.js';

const MAX_TARGETS = 20;
const MAX_RECIPIENTS = 5;
// Protocol parity: jupiter_memo_policy::MAX_RELAY_SURPLUS_MEMO_BYTES.
export const MAX_RELAY_SURPLUS_MEMO_BYTES = 32;
const MAX_U64 = 18_446_744_073_709_551_615n;
const DEFAULT_POLL_INTERVAL_MS = 12_000;
const BASE32_ALPHABET = 'abcdefghijklmnopqrstuvwxyz234567';

function variantName(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return '';
  return Object.keys(value)[0] || '';
}

function principalText(value) {
  const resolved = readOptional(value);
  if (!resolved) return '';
  return typeof resolved.toText === 'function' ? resolved.toText() : String(resolved);
}

export function equalBytes(left, right) {
  return left.length === right.length
    && left.every((byte, index) => byte === right[index]);
}

function relayMemoDisplay(value) {
  const bytes = Uint8Array.from(value || []);
  if (bytes.length === 0) return 'Memo: none';
  let text = '';
  try {
    text = new TextDecoder('utf-8', {
      fatal: true,
      ignoreBOM: true,
    }).decode(bytes);
  } catch {
    // Exact hexadecimal remains available for arbitrary binary memos.
  }
  const roundTrips = text
    && equalBytes(bytes, new TextEncoder().encode(text));
  const safe = roundTrips
    && /\S/u.test(text)
    && !/[\p{C}\p{Zl}\p{Zp}]/u.test(text);
  return `Memo hex: ${bytesToHex(bytes)}${safe ? `\nMemo text: ${text}` : ''}`;
}

function parsePrincipalList(text, {
  maximum,
  emptyMessage,
  maximumMessage,
  invalidLabel,
  duplicateMessage,
}) {
  const tokens = String(text || '').split(/[\s,]+/u).map((value) => value.trim()).filter(Boolean);
  if (tokens.length === 0) throw new Error(emptyMessage);
  if (tokens.length > maximum) throw new Error(maximumMessage);
  const principals = tokens.map((value) => {
    try {
      return Principal.fromText(value);
    } catch {
      throw new Error(`Invalid ${invalidLabel}: ${value}`);
    }
  });
  const normalized = principals.map((principal) => principal.toText());
  if (new Set(normalized).size !== normalized.length) {
    throw new Error(duplicateMessage);
  }
  return principals;
}

export function parseRelayTargetSet(text) {
  return parsePrincipalList(text, {
    maximum: MAX_TARGETS,
    emptyMessage: 'Enter at least one target canister ID.',
    maximumMessage: `Enter no more than ${MAX_TARGETS} target canister IDs.`,
    invalidLabel: 'target canister ID',
    duplicateMessage: 'Duplicate target canisters are not allowed.',
  });
}

export function parseRelayNeuronId(value) {
  const text = String(value ?? '').trim();
  if (!/^\d+$/u.test(text)) throw new Error(`Invalid recipient neuron ID: ${text || '(empty)'}`);
  const neuronId = BigInt(text);
  if (neuronId < 1n || neuronId > MAX_U64) {
    throw new Error(`Recipient neuron ID must be between 1 and ${MAX_U64}.`);
  }
  return neuronId;
}

export function decodeRelayMemo(mode, value) {
  const input = String(value ?? '');
  let bytes;
  if (mode === 'Hex') {
    const withoutPrefix = input.replace(/^\s*0x/iu, '');
    const compact = withoutPrefix.replace(/\s+/gu, '');
    if (!/^[0-9a-f]*$/iu.test(compact)) {
      throw new Error('Memo hexadecimal input contains a non-hexadecimal character.');
    }
    if (compact.length % 2 !== 0) {
      throw new Error('Memo hexadecimal input must contain complete byte pairs.');
    }
    bytes = Uint8Array.from(compact.match(/.{2}/gu) || [], (pair) => Number.parseInt(pair, 16));
  } else {
    bytes = new TextEncoder().encode(input);
  }
  return bytes;
}

export function parseRelayMemo(mode, value) {
  const bytes = decodeRelayMemo(mode, value);
  if (bytes.length > MAX_RELAY_SURPLUS_MEMO_BYTES) {
    throw new Error(`Memo is ${bytes.length} bytes; maximum is ${MAX_RELAY_SURPLUS_MEMO_BYTES} bytes.`);
  }
  return bytes;
}

function recipientDuplicateKey(recipient) {
  if (recipient.type === 'Neuron') return `Neuron:${parseRelayNeuronId(recipient.value)}`;
  return `Principal:${Principal.fromText(String(recipient.value || '').trim()).toText()}`;
}

export function duplicateRelayRecipientIndexes(recipients) {
  const canonicalIndexes = new Map();
  recipients.forEach((recipient, index) => {
    try {
      const canonical = recipientDuplicateKey(recipient);
      const indexes = canonicalIndexes.get(canonical) || [];
      indexes.push(index);
      canonicalIndexes.set(canonical, indexes);
    } catch {
      // Incomplete and malformed values are handled when the form is submitted.
    }
  });
  return new Set(
    [...canonicalIndexes.values()]
      .filter((indexes) => indexes.length > 1)
      .flat(),
  );
}

export function parseRelayRecipientSet(recipients) {
  if (!Array.isArray(recipients)) throw new Error('Surplus recipients must be a list.');
  if (recipients.length > MAX_RECIPIENTS) {
    throw new Error(`Enter no more than ${MAX_RECIPIENTS} surplus recipients.`);
  }
  const duplicates = duplicateRelayRecipientIndexes(recipients);
  if (duplicates.size > 0) throw new Error('Duplicate surplus recipients are not allowed.');
  return recipients.map(({ type, value, memoMode = 'Text', memoValue = '' }) => {
    const memo = Array.from(parseRelayMemo(memoMode, memoValue));
    if (type === 'Neuron') {
      return { Neuron: { neuron_id: parseRelayNeuronId(value), memo } };
    }
    const text = String(value || '').trim();
    try {
      return { Principal: { principal: Principal.fromText(text), memo } };
    } catch {
      throw new Error(`Invalid recipient principal: ${text || '(empty)'}`);
    }
  });
}

export function duplicatePrincipalIndexes(values) {
  const canonicalIndexes = new Map();
  values.forEach((rawValue, index) => {
    try {
      const canonical = Principal.fromText(String(rawValue || '').trim()).toText();
      const indexes = canonicalIndexes.get(canonical) || [];
      indexes.push(index);
      canonicalIndexes.set(canonical, indexes);
    } catch {
      // Incomplete and malformed principals are handled when the form is submitted.
    }
  });
  return new Set(
    [...canonicalIndexes.values()]
      .filter((indexes) => indexes.length > 1)
      .flat(),
  );
}

export const duplicateRelayTargetIndexes = duplicatePrincipalIndexes;

function crc32(bytes) {
  let value = 0xffffffff;
  for (const byte of bytes) {
    value = CRC32_TABLE[(value ^ byte) & 0xff] ^ (value >>> 8);
  }
  return (value ^ 0xffffffff) >>> 0;
}

const CRC32_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let index = 0; index < 256; index += 1) {
    let value = index;
    for (let bit = 0; bit < 8; bit += 1) {
      value = (value & 1) !== 0 ? (0xedb88320 ^ (value >>> 1)) : (value >>> 1);
    }
    table[index] = value >>> 0;
  }
  return table;
})();

function concatBytes(...parts) {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function base32NoPadding(bytes) {
  let bits = 0;
  let value = 0;
  let out = '';
  for (const byte of bytes) {
    value = (value << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      out += BASE32_ALPHABET[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) out += BASE32_ALPHABET[(value << (5 - bits)) & 31];
  return out;
}

export function icrcAccountText(account) {
  if (!account?.owner) return '';
  const owner = principalText(account.owner);
  const subaccount = account.subaccount?.[0] ? Uint8Array.from(account.subaccount[0]) : new Uint8Array(32);
  if (subaccount.every((byte) => byte === 0)) return owner;
  const checksum = crc32(concatBytes(account.owner.toUint8Array(), subaccount));
  const checksumBytes = new Uint8Array([
    (checksum >>> 24) & 0xff,
    (checksum >>> 16) & 0xff,
    (checksum >>> 8) & 0xff,
    checksum & 0xff,
  ]);
  return `${owner}-${base32NoPadding(checksumBytes)}.${bytesToHex(subaccount)}`;
}

function setText(id, value) {
  const node = document.getElementById(id);
  if (node) node.textContent = value ?? '';
}

function setHtml(id, value) {
  const node = document.getElementById(id);
  if (node) node.innerHTML = value || '';
}

function setHidden(id, hidden) {
  const node = document.getElementById(id);
  if (node) node.hidden = hidden;
}

function unwrapView(result) {
  if (result?.Ok) return result.Ok;
  if (result?.Err !== undefined) throw new Error(result.Err);
  throw new Error('Historian returned an invalid Relay setup view.');
}

function viewState(view) {
  return variantName(view?.state);
}

function requiredBalance(view, override = null) {
  return override === null || override === undefined
    ? BigInt(view?.nominal_minimum_e8s ?? 0)
    : BigInt(override);
}

function relayIdFromState(state) {
  const kind = variantName(state);
  return principalText(state?.[kind]?.relay_canister_id);
}

export function createRelaySetupController({
  frontendConfig = {},
  isLocalHost = () => false,
  createHistorian = createHistorianClient,
  ledgerActorFactory = createLedgerActor,
  governanceActorFactory = createGovernanceActor,
  publicNeuronLoader = loadPublicNeuronStakingAccount,
  copyTextToClipboard = null,
  hostProvider = () => window.location.origin,
  setIntervalFn = (callback, delay) => window.setInterval(callback, delay),
  clearIntervalFn = (handle) => window.clearInterval(handle),
  pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
} = {}) {
  const state = {
    requestFingerprint: '',
    checkedConfigurationFingerprint: '',
    targets: [],
    surplusRecipients: [],
    view: null,
    balanceE8s: null,
    notifyResult: null,
    error: '',
    loading: false,
    creating: false,
    requiredBalanceOverride: null,
  };
  let generation = 0;
  let pollHandle = null;
  const listSpecs = {
    target: {
      listId: 'relay-setup-target-list',
      addId: 'relay-setup-add-target',
      countId: 'relay-setup-target-count-hint',
      announcementId: 'relay-setup-target-announcement',
      rowSelector: '[data-relay-target-row]',
      inputSelector: '[data-relay-target-input]',
      labelSelector: '[data-relay-target-label]',
      removeSelector: '[data-relay-target-remove]',
      errorSelector: '[data-relay-target-error]',
      dataPrefix: 'relayTarget',
      idPrefix: 'relay-setup-target',
      hintId: 'relay-setup-target-hint',
      label: 'Target canister',
      placeholder: 'Canister ID',
      noun: 'target canister',
      maximum: MAX_TARGETS,
      parse: parseRelayTargetSet,
      duplicateError: 'Duplicate canister ID. Each target must be unique.',
    },
    recipient: {
      listId: 'relay-setup-recipient-list',
      addId: 'relay-setup-add-recipient',
      countId: 'relay-setup-recipient-count-hint',
      announcementId: 'relay-setup-recipient-announcement',
      rowSelector: '[data-relay-recipient-row]',
      inputSelector: '[data-relay-recipient-input]',
      labelSelector: '[data-relay-recipient-label]',
      removeSelector: '[data-relay-recipient-remove]',
      errorSelector: '[data-relay-recipient-error]',
      typeSelector: '[data-relay-recipient-type]',
      memoModeSelector: '[data-relay-recipient-memo-mode]',
      memoModeLabelSelector: '[data-relay-recipient-memo-mode-label]',
      memoInputSelector: '[data-relay-recipient-memo-input]',
      memoLabelSelector: '[data-relay-recipient-memo-label]',
      memoCountSelector: '[data-relay-recipient-memo-count]',
      memoErrorSelector: '[data-relay-recipient-memo-error]',
      memoNoticeSelector: '[data-relay-recipient-memo-notice]',
      dataPrefix: 'relayRecipient',
      idPrefix: 'relay-setup-recipient',
      hintId: 'relay-setup-recipient-hint',
      label: 'Surplus recipient',
      placeholder: 'Principal',
      noun: 'surplus recipient',
      maximum: MAX_RECIPIENTS,
      parse: parseRelayRecipientSet,
      duplicateError: 'Duplicate surplus recipient. Each recipient must be unique.',
    },
  };
  const nextFieldIds = { target: 2, recipient: 2 };

  function listNode(kind) {
    return document.getElementById(listSpecs[kind].listId);
  }

  function listInputs(kind) {
    const list = listNode(kind);
    return list?.querySelectorAll
      ? Array.from(list.querySelectorAll(listSpecs[kind].inputSelector))
      : [];
  }

  function listRows(kind) {
    const list = listNode(kind);
    return list?.querySelectorAll
      ? Array.from(list.querySelectorAll(listSpecs[kind].rowSelector))
      : [];
  }

  function listValues(kind) {
    return listInputs(kind).map((input) => String(input.value || '').trim());
  }

  function rawListValues(kind) {
    return listInputs(kind).map((input) => String(input.value || ''));
  }

  function recipientRowsValues({ trim = true } = {}) {
    return listRows('recipient').map((row) => {
      const value = String(row.querySelector?.(listSpecs.recipient.inputSelector)?.value || '');
      return {
        type: row.querySelector?.(listSpecs.recipient.typeSelector)?.value === 'Neuron'
          ? 'Neuron'
          : 'Principal',
        value: trim ? value.trim() : value,
        memoMode: row.querySelector?.(listSpecs.recipient.memoModeSelector)?.value === 'Hex'
          ? 'Hex'
          : 'Text',
        memoValue: String(row.querySelector?.(listSpecs.recipient.memoInputSelector)?.value || ''),
      };
    });
  }

  function recipientRoutingMode() {
    return document.getElementById('relay-setup-mode-all-cycles')?.checked !== true;
  }

  function configurationFingerprint() {
    return JSON.stringify({
      targets: rawListValues('target'),
      surplusMode: recipientRoutingMode() ? 'routing' : 'all-cycles',
      surplusRecipients: recipientRowsValues({ trim: false }),
    });
  }

  function setFieldError(kind, input, message = '') {
    const spec = listSpecs[kind];
    const row = input?.closest?.(spec.rowSelector);
    const error = row?.querySelector?.(spec.errorSelector);
    if (error) {
      error.textContent = message;
      error.hidden = !message;
    }
    row?.classList?.toggle?.('relay-setup-principal-row--error', Boolean(message));
    if (message) input?.setAttribute?.('aria-invalid', 'true');
    else input?.removeAttribute?.('aria-invalid');
  }

  function setMemoError(row, message = '') {
    const memoInput = row?.querySelector?.(listSpecs.recipient.memoInputSelector);
    const error = row?.querySelector?.(listSpecs.recipient.memoErrorSelector);
    if (error) {
      error.textContent = message;
      error.hidden = !message;
    }
    if (message) memoInput?.setAttribute?.('aria-invalid', 'true');
    else memoInput?.removeAttribute?.('aria-invalid');
  }

  function setMemoConversionNotice(row, message = '') {
    const notice = row?.querySelector?.(listSpecs.recipient.memoNoticeSelector);
    if (notice) {
      notice.textContent = message;
      notice.hidden = !message;
    }
    if (row?.dataset) {
      if (message) row.dataset.relayRecipientMemoConversionNotice = message;
      else delete row.dataset.relayRecipientMemoConversionNotice;
    }
  }

  function updateRecipientMemoState(row) {
    const mode = row?.querySelector?.(listSpecs.recipient.memoModeSelector)?.value === 'Hex'
      ? 'Hex'
      : 'Text';
    const input = row?.querySelector?.(listSpecs.recipient.memoInputSelector);
    const counter = row?.querySelector?.(listSpecs.recipient.memoCountSelector);
    let message = '';
    let count = null;
    try {
      count = decodeRelayMemo(mode, input?.value || '').length;
      if (count > MAX_RELAY_SURPLUS_MEMO_BYTES) {
        message = `Memo is ${count} bytes; maximum is ${MAX_RELAY_SURPLUS_MEMO_BYTES} bytes.`;
      }
    } catch (error) {
      message = error.message;
    }
    if (counter) {
      counter.textContent = `${count === null ? '\u2014' : count}/${MAX_RELAY_SURPLUS_MEMO_BYTES} bytes`;
    }
    setMemoError(row, message);
    return message;
  }

  function tryApplyRecipientMemoModeChange(row) {
    const selector = row?.querySelector?.(listSpecs.recipient.memoModeSelector);
    const input = row?.querySelector?.(listSpecs.recipient.memoInputSelector);
    if (!selector || !input) return false;
    const previous = row.dataset.relayRecipientMemoMode === 'Hex' ? 'Hex' : 'Text';
    const requested = selector.value === 'Hex' ? 'Hex' : 'Text';
    const previousValue = input.value;
    setMemoConversionNotice(row);
    if (previous === requested) return true;

    let originalBytes;
    try {
      originalBytes = parseRelayMemo(previous, previousValue);
    } catch {
      selector.value = previous;
      input.value = previousValue;
      updateRecipientMemoState(row);
      return false;
    }

    let candidateValue;
    try {
      if (previous === 'Text') {
        candidateValue = Array.from(originalBytes)
          .map((byte) => byte.toString(16).padStart(2, '0'))
          .join('');
      } else {
        candidateValue = new TextDecoder('utf-8', {
          fatal: true,
          ignoreBOM: true,
        }).decode(originalBytes);
      }
      input.value = candidateValue;
      const roundTrippedBytes = parseRelayMemo(requested, input.value);
      if (!equalBytes(originalBytes, roundTrippedBytes)) {
        throw new Error('The browser field changed the memo bytes.');
      }
    } catch {
      input.value = previousValue;
      selector.value = previous;
      setMemoConversionNotice(
        row,
        'These memo bytes cannot be represented as Text without changing them. Hexadecimal mode was retained.',
      );
      updateRecipientMemoState(row);
      return false;
    }
    row.dataset.relayRecipientMemoMode = requested;
    setMemoConversionNotice(row);
    updateRecipientMemoState(row);
    return true;
  }

  function validateVisibleList(kind, { includeIncomplete = false } = {}) {
    const spec = listSpecs[kind];
    const inputs = listInputs(kind);
    const values = listValues(kind);
    const recipientValues = kind === 'recipient' ? recipientRowsValues() : null;
    const duplicates = kind === 'recipient'
      ? duplicateRelayRecipientIndexes(recipientValues)
      : duplicatePrincipalIndexes(values);
    if (kind === 'recipient' && !recipientRoutingMode()) {
      inputs.forEach((input) => setFieldError(kind, input));
      listRows(kind).forEach((row) => setMemoError(row));
      return { valid: true, firstInvalidInput: null, firstError: '' };
    }
    const errors = values.map((value, index) => {
      if (duplicates.has(index)) return spec.duplicateError;
      if (!includeIncomplete) return '';
      if (!value) return `Enter a ${spec.noun} or remove this field.`;
      try {
        if (kind === 'recipient' && recipientValues[index].type === 'Neuron') {
          parseRelayNeuronId(value);
        } else {
          Principal.fromText(value);
        }
        return '';
      } catch {
        return `Enter a valid ${spec.noun}.`;
      }
    });
    inputs.forEach((input, index) => setFieldError(kind, input, errors[index]));
    const memoErrors = kind === 'recipient'
      ? listRows(kind).map(updateRecipientMemoState)
      : [];
    return {
      valid: errors.every((message) => !message) && memoErrors.every((message) => !message),
      firstInvalidInput: inputs[errors.findIndex(Boolean)]
        || listRows(kind)[memoErrors.findIndex(Boolean)]?.querySelector?.(listSpecs.recipient.memoInputSelector)
        || null,
      firstError: errors.find(Boolean) || memoErrors.find(Boolean) || '',
    };
  }

  function validateVisibleConfiguration({ includeIncomplete = false } = {}) {
    const target = validateVisibleList('target', { includeIncomplete });
    const recipient = validateVisibleList('recipient', { includeIncomplete });
    const duplicateTarget = duplicatePrincipalIndexes(listValues('target')).size > 0;
    const duplicateRecipient = recipientRoutingMode()
      && duplicateRelayRecipientIndexes(recipientRowsValues()).size > 0;
    const warning = document.getElementById('relay-setup-warning');
    if (warning) {
      warning.textContent = duplicateTarget
        ? 'Duplicate target canisters found. Change or remove one before checking the Relay configuration.'
        : (duplicateRecipient
          ? 'Duplicate surplus recipients found. Change or remove one before checking the Relay configuration.'
          : '');
      warning.hidden = !warning.textContent;
    }
    const hasEmptyFields = listInputs('target').length === 0
      || listValues('target').some((value) => !value)
      || (recipientRoutingMode() && (
        listInputs('recipient').length === 0 || listValues('recipient').some((value) => !value)
      ));
    const emptyFieldError = recipientRoutingMode() && listInputs('recipient').length === 0
      ? 'Add a surplus recipient or select all-cycles mode.'
      : '';
    const submitButton = document.getElementById('relay-setup-submit');
    if (submitButton) submitButton.disabled = !target.valid || !recipient.valid || hasEmptyFields;
    return {
      valid: target.valid && recipient.valid && !hasEmptyFields,
      firstInvalidInput: target.firstInvalidInput || recipient.firstInvalidInput,
      firstError: target.firstError || recipient.firstError || emptyFieldError
        || 'Complete every visible configuration field.',
    };
  }

  function announceListChange(kind, message) {
    setText(listSpecs[kind].announcementId, message);
  }

  function updateListRows(kind) {
    const spec = listSpecs[kind];
    const rows = listRows(kind);
    rows.forEach((row, index) => {
      const number = index + 1;
      const label = row.querySelector?.(spec.labelSelector);
      const removeButton = row.querySelector?.(spec.removeSelector);
      if (label) {
        const recipientType = kind === 'recipient'
          ? row.querySelector?.(spec.typeSelector)?.value
          : null;
        label.textContent = kind === 'recipient'
          ? `Recipient ${recipientType === 'Neuron' ? 'neuron ID' : 'principal'} ${number}`
          : `${spec.label} ${number}`;
      }
      if (kind === 'recipient') {
        const input = row.querySelector?.(spec.inputSelector);
        const error = row.querySelector?.(spec.errorSelector);
        const memoMode = row.querySelector?.(spec.memoModeSelector);
        const memoModeLabel = row.querySelector?.(spec.memoModeLabelSelector);
        const memoInput = row.querySelector?.(spec.memoInputSelector);
        const memoLabel = row.querySelector?.(spec.memoLabelSelector);
        const memoCount = row.querySelector?.(spec.memoCountSelector);
        const memoError = row.querySelector?.(spec.memoErrorSelector);
        const memoNotice = row.querySelector?.(spec.memoNoticeSelector);
        row.querySelector?.(spec.typeSelector)?.setAttribute?.(
          'aria-label',
          `Surplus recipient ${number} type`,
        );
        if (input) input.id = `relay-setup-recipient-${number}`;
        if (error) error.id = `relay-setup-recipient-error-${number}`;
        label?.setAttribute?.('for', `relay-setup-recipient-${number}`);
        input?.setAttribute?.(
          'aria-describedby',
          `relay-setup-recipient-hint relay-setup-recipient-error-${number}`,
        );
        memoMode?.setAttribute?.(
          'aria-label',
          `Surplus recipient ${number} memo format`,
        );
        if (memoMode) memoMode.id = `relay-setup-recipient-memo-mode-${number}`;
        memoModeLabel?.setAttribute?.('for', `relay-setup-recipient-memo-mode-${number}`);
        if (memoInput) memoInput.id = `relay-setup-recipient-memo-${number}`;
        memoLabel?.setAttribute?.('for', `relay-setup-recipient-memo-${number}`);
        if (memoCount) memoCount.id = `relay-setup-recipient-memo-count-${number}`;
        if (memoError) memoError.id = `relay-setup-recipient-memo-error-${number}`;
        if (memoNotice) memoNotice.id = `relay-setup-recipient-memo-notice-${number}`;
        memoInput?.setAttribute?.('aria-label', `Surplus recipient ${number} memo`);
        memoInput?.setAttribute?.(
          'aria-describedby',
          `relay-setup-recipient-memo-count-${number} relay-setup-recipient-memo-error-${number} relay-setup-recipient-memo-notice-${number}`,
        );
        if (!row.dataset.relayRecipientMemoMode) {
          row.dataset.relayRecipientMemoMode =
            row.querySelector?.(spec.memoModeSelector)?.value === 'Hex' ? 'Hex' : 'Text';
        }
        updateRecipientMemoState(row);
      }
      if (removeButton) {
        removeButton.hidden = kind === 'target' && rows.length === 1;
        removeButton.setAttribute?.('aria-label', `Remove ${spec.noun} ${number}`);
      }
    });
    const addButton = document.getElementById(spec.addId);
    if (addButton) addButton.disabled = rows.length >= spec.maximum;
    setText(
      spec.countId,
      `${rows.length} ${spec.noun}${rows.length === 1 ? '' : 's'}`,
    );
  }

  function applyRecipientType(row) {
    const spec = listSpecs.recipient;
    const type = row.querySelector?.(spec.typeSelector)?.value === 'Neuron' ? 'Neuron' : 'Principal';
    const input = row.querySelector?.(spec.inputSelector);
    if (input) {
      input.placeholder = type === 'Neuron' ? 'Neuron ID' : 'Principal';
      if (type === 'Neuron') input.setAttribute?.('inputmode', 'numeric');
      else input.removeAttribute?.('inputmode');
    }
  }

  function updateSurplusModePresentation() {
    const routing = recipientRoutingMode();
    setHidden('relay-setup-recipient-editor', !routing);
    setText(
      'relay-setup-surplus-mode-summary',
      routing
        ? 'Recipient routing selected. Configure one to five surplus recipients.'
        : 'All-cycles mode selected. No raw ICP surplus recipient will be configured.',
    );
  }

  function selectSurplusMode(routing) {
    const routingRadio = document.getElementById('relay-setup-mode-routing');
    const allCyclesRadio = document.getElementById('relay-setup-mode-all-cycles');
    if (routingRadio) routingRadio.checked = routing;
    if (allCyclesRadio) allCyclesRadio.checked = !routing;
    updateSurplusModePresentation();
  }

  function createListRow(kind) {
    const spec = listSpecs[kind];
    const rowId = nextFieldIds[kind];
    nextFieldIds[kind] += 1;
    const row = document.createElement('div');
    row.className = 'relay-setup-principal-row';
    row.dataset[`${spec.dataPrefix}Row`] = 'true';

    const label = document.createElement('label');
    label.className = 'relay-setup-principal-label';
    label.dataset[`${spec.dataPrefix}Label`] = 'true';
    label.htmlFor = `${spec.idPrefix}-${rowId}`;

    const controls = document.createElement('div');
    controls.className = 'relay-setup-principal-controls';

    if (kind === 'recipient') {
      const type = document.createElement('select');
      type.className = 'tracker-input relay-setup-recipient-type';
      type.dataset.relayRecipientType = 'true';
      type.setAttribute('aria-label', `Surplus recipient ${rowId} type`);
      const principalOption = document.createElement('option');
      principalOption.value = 'Principal';
      principalOption.textContent = 'Principal';
      const neuronOption = document.createElement('option');
      neuronOption.value = 'Neuron';
      neuronOption.textContent = 'Neuron ID';
      type.append(principalOption, neuronOption);
      type.value = 'Principal';
      controls.append(type);
    }

    const input = document.createElement('input');
    input.className = 'tracker-input mono relay-setup-principal-input';
    input.id = `${spec.idPrefix}-${rowId}`;
    input.type = 'text';
    input.autocomplete = 'off';
    input.autocapitalize = 'none';
    input.spellcheck = false;
    input.placeholder = spec.placeholder;
    input.dataset[`${spec.dataPrefix}Input`] = 'true';
    input.setAttribute('aria-describedby', `${spec.hintId} ${spec.idPrefix}-error-${rowId}`);

    const removeButton = document.createElement('button');
    removeButton.className = 'pane-page-button relay-setup-remove-principal';
    removeButton.type = 'button';
    removeButton.textContent = 'Remove';
    removeButton.dataset[`${spec.dataPrefix}Remove`] = 'true';

    const error = document.createElement('p');
    error.className = 'relay-setup-principal-error';
    error.id = `${spec.idPrefix}-error-${rowId}`;
    error.dataset[`${spec.dataPrefix}Error`] = 'true';
    error.hidden = true;

    controls.append(input, removeButton);
    row.append(label, controls, error);
    if (kind === 'recipient') {
      const memoControls = document.createElement('div');
      memoControls.className = 'relay-setup-memo-controls';
      const memoModeLabel = document.createElement('label');
      memoModeLabel.textContent = 'Memo format';
      memoModeLabel.dataset.relayRecipientMemoModeLabel = 'true';
      const memoMode = document.createElement('select');
      memoMode.className = 'tracker-input relay-setup-memo-mode';
      memoMode.dataset.relayRecipientMemoMode = 'true';
      const textOption = document.createElement('option');
      textOption.value = 'Text';
      textOption.textContent = 'Text';
      const hexOption = document.createElement('option');
      hexOption.value = 'Hex';
      hexOption.textContent = 'Hexadecimal';
      memoMode.append(textOption, hexOption);
      memoMode.value = 'Text';
      const memoLabel = document.createElement('label');
      memoLabel.textContent = 'Memo';
      memoLabel.dataset.relayRecipientMemoLabel = 'true';
      const memoInput = document.createElement('input');
      memoInput.className = 'tracker-input mono relay-setup-memo-input';
      memoInput.type = 'text';
      memoInput.autocomplete = 'off';
      memoInput.spellcheck = false;
      memoInput.dataset.relayRecipientMemoInput = 'true';
      const memoCount = document.createElement('span');
      memoCount.className = 'relay-setup-memo-count';
      memoCount.dataset.relayRecipientMemoCount = 'true';
      memoCount.setAttribute('role', 'status');
      memoCount.setAttribute('aria-live', 'polite');
      const memoError = document.createElement('p');
      memoError.className = 'relay-setup-principal-error';
      memoError.dataset.relayRecipientMemoError = 'true';
      memoError.hidden = true;
      const memoNotice = document.createElement('p');
      memoNotice.className = 'relay-setup-memo-notice';
      memoNotice.dataset.relayRecipientMemoNotice = 'true';
      memoNotice.setAttribute('role', 'status');
      memoNotice.setAttribute('aria-live', 'polite');
      memoNotice.hidden = true;
      memoControls.append(memoModeLabel, memoMode, memoLabel, memoInput, memoCount);
      row.append(memoControls, memoError, memoNotice);
      applyRecipientType(row);
      updateRecipientMemoState(row);
    }
    return row;
  }

  function invalidateCurrentConfiguration() {
    generation += 1;
    stopPolling();
    state.targets = [];
    state.surplusRecipients = [];
    state.requestFingerprint = '';
    state.checkedConfigurationFingerprint = '';
    state.view = null;
    state.balanceE8s = null;
    state.notifyResult = null;
    state.requiredBalanceOverride = null;
    state.error = '';
    state.loading = false;
    state.creating = false;
    render();
  }

  function handleVisibleConfigurationChange({ includeIncomplete = false } = {}) {
    const fingerprint = configurationFingerprint();
    const authoritativeFingerprint = state.checkedConfigurationFingerprint || state.requestFingerprint;
    if (fingerprint !== authoritativeFingerprint) invalidateCurrentConfiguration();
    validateVisibleConfiguration({ includeIncomplete });
  }

  function addListField(kind) {
    const spec = listSpecs[kind];
    const list = listNode(kind);
    const rows = listRows(kind);
    if (!list || rows.length >= spec.maximum) return;
    if (kind === 'recipient') selectSurplusMode(true);
    const row = createListRow(kind);
    list.append(row);
    updateListRows(kind);
    invalidateCurrentConfiguration();
    validateVisibleConfiguration();
    row.querySelector?.(spec.inputSelector)?.focus?.();
    announceListChange(kind, `${spec.label} ${rows.length + 1} added.`);
  }

  function removeListField(kind, button) {
    const spec = listSpecs[kind];
    const row = button?.closest?.(spec.rowSelector);
    if (!row) return;
    const rows = listRows(kind);
    const removedIndex = rows.indexOf(row);
    if (rows.length === 1 && kind === 'target') {
      const input = row.querySelector?.(spec.inputSelector);
      if (input) input.value = '';
    } else {
      row.remove?.();
    }
    if (kind === 'recipient' && listRows(kind).length === 0) selectSurplusMode(false);
    updateListRows(kind);
    invalidateCurrentConfiguration();
    validateVisibleConfiguration();
    const inputs = listInputs(kind);
    inputs[Math.min(Math.max(removedIndex, 0), inputs.length - 1)]?.focus?.();
    announceListChange(kind, `${spec.label} removed. ${inputs.length} field${inputs.length === 1 ? '' : 's'} remaining.`);
  }

  const addTargetField = () => addListField('target');
  const addRecipientField = () => addListField('recipient');
  const removeTargetField = (button) => removeListField('target', button);
  const removeRecipientField = (button) => removeListField('recipient', button);
  const validateVisibleTargetFields = (options) => validateVisibleList('target', options);

  function stopPolling() {
    if (pollHandle !== null) clearIntervalFn(pollHandle);
    pollHandle = null;
  }

  function shouldPoll(view = state.view) {
    const kind = viewState(view);
    return kind === 'InProgress'
      || (kind === 'NotFunded' && Boolean(readOptional(view?.setup_account)));
  }

  function isResumableFinalization(view = state.view) {
    const notifyKind = variantName(state.notifyResult);
    if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') return false;
    if (viewState(view) !== 'InProgress') return false;
    const phase = variantName(view?.state?.InProgress?.phase);
    return phase === 'RelayFunded' || phase === 'FinalizationAttempted';
  }

  function configurationStillCurrent(expected, requestGeneration) {
    return generation === requestGeneration
      && (state.requestFingerprint === expected
        || state.checkedConfigurationFingerprint === expected)
      && configurationFingerprint() === expected;
  }

  function ownsConfigurationOperation(expected, requestGeneration) {
    return generation === requestGeneration
      && (state.requestFingerprint === expected
        || state.checkedConfigurationFingerprint === expected);
  }

  function ensureConfigurationContinuationIsCurrent(
    expected,
    requestGeneration,
    message = 'The Relay configuration changed. Check it again.',
  ) {
    if (configurationStillCurrent(expected, requestGeneration)) return true;
    if (ownsConfigurationOperation(expected, requestGeneration)
      && configurationFingerprint() !== expected) {
      rejectStaleCheckedConfiguration(message);
    }
    return false;
  }

  function checkedConfigurationStillMatchesVisibleForm(expected = state.checkedConfigurationFingerprint) {
    return Boolean(
      expected
      && state.view
      && state.checkedConfigurationFingerprint === expected
      && configurationFingerprint() === expected,
    );
  }

  function rejectStaleCheckedConfiguration(message) {
    invalidateCurrentConfiguration();
    state.error = message;
    render();
  }

  async function historianBundle() {
    return createHistorian({
      historianCanisterId: frontendConfig?.historianCanisterId,
      host: hostProvider(),
      local: isLocalHost(),
    });
  }

  async function loadLedger({ agent, historian }) {
    const status = await historian.get_public_status();
    const ledgerId = principalText(status?.ledger_canister_id);
    if (!ledgerId) throw new Error('Historian did not return an ICP ledger canister ID.');
    return ledgerActorFactory(ledgerId, { agent });
  }

  function setAccountLink(linkId, value, accountIdentifier) {
    const link = document.getElementById(linkId);
    if (!link) return;
    if (!accountIdentifier) {
      if (typeof link.removeAttribute === 'function') link.removeAttribute('href');
      else link.href = '';
      link.title = '';
      return;
    }
    link.href = `https://dashboard.internetcomputer.org/account/${encodeURIComponent(accountIdentifier)}`;
    link.title = value || accountIdentifier;
  }

  function notifyPresentation() {
    const kind = variantName(state.notifyResult);
    const details = state.notifyResult?.[kind];
    switch (kind) {
      case 'BelowMinimum':
        return {
          status: 'Below minimum',
          message: `Balance ${formatIcpE8s(details.balance_e8s)}; required ${formatIcpE8s(details.required_e8s)}; shortfall ${formatIcpE8s(details.shortfall_e8s)}.`,
        };
      case 'BelowCurrentRequirement':
        return {
          status: 'Below current requirement',
          message: `Balance ${formatIcpE8s(details.balance_e8s)}; live required balance ${formatIcpE8s(details.required_e8s)}; shortfall ${formatIcpE8s(details.shortfall_e8s)}.`,
        };
      case 'Busy':
        return {
          status: 'Busy',
          message: 'Relay factory is processing the maximum number of funded setups. Try again shortly.',
        };
      case 'InProgress':
        return {
          status: 'In progress',
          message: variantName(details?.phase) || DASH,
        };
      case 'Active':
        return { status: 'Active', message: '' };
      case 'FailedPreSpend':
        return { status: 'Failed before spend', message: details?.message || DASH };
      case 'ManualRecoveryRequired': {
        const phase = variantName(details?.phase);
        const relayId = principalText(details?.relay_canister_id);
        return {
          status: 'Manual recovery required',
          message: [phase, relayId ? `Relay ${relayId}` : '', details?.message || ''].filter(Boolean).join(' · '),
        };
      }
      default:
        return null;
    }
  }

  function render() {
    const view = state.view;
    const kind = viewState(view);
    const stateDetails = view?.state?.[kind];
    const relayId = relayIdFromState(view?.state);
    const recoveryMessage = kind === 'ManualRecoveryRequired' ? stateDetails?.message : '';
    const phase = stateDetails?.phase ? variantName(stateDetails.phase) : '';
    const notifyKind = variantName(state.notifyResult);
    const notifyDetails = state.notifyResult?.[notifyKind];
    const notification = notifyPresentation();
    const account = readOptional(view?.setup_account);
    const accountText = account ? icrcAccountText(account) : '';
    const identifier = readOptional(view?.setup_account_identifier) || (account ? accountIdentifierHex(account) : '');
    const notifiedRelayId = ['Active', 'InProgress', 'ManualRecoveryRequired'].includes(notifyKind)
      ? principalText(notifyDetails?.relay_canister_id)
      : '';
    const displayedRelayId = relayId || notifiedRelayId;
    const activeOrBlocked = ['Active', 'InProgress', 'ManualRecoveryRequired'].includes(kind)
      || ['Active', 'InProgress', 'ManualRecoveryRequired'].includes(notifyKind);
    const resumableFinalization = isResumableFinalization(view);
    const createButton = document.getElementById('relay-setup-create');
    const balance = state.balanceE8s === null ? 0n : state.balanceE8s;
    const effectiveRequirement = requiredBalance(view, state.requiredBalanceOverride);
    const canCreate = Boolean(view && account && !activeOrBlocked && !state.error
      && balance >= effectiveRequirement && !state.creating);

    setHidden('relay-setup-result', Boolean(view || state.error || state.loading));
    setHidden('relay-setup-summary', !view && !state.error && !state.loading);
    setText('relay-setup-status', state.error || (state.loading ? 'Checking Relay configuration…' : (state.creating ? (resumableFinalization ? 'Resuming finalization…' : 'Creating Relay…') : (notification?.status || kind || DASH))));
    setText('relay-setup-status-label', notification?.message || recoveryMessage || phase || DASH);
    setText('relay-setup-factory', view?.factory_available ? 'Available' : 'Unavailable');
    setText('relay-setup-target-count', view ? String(view.target_count) : DASH);
    setText('relay-setup-canonical-targets', view ? view.canonical_target_canister_ids.map(principalText).join('\n') : DASH);
    setText('relay-setup-recipient-count', view ? String(view.surplus_recipient_count) : DASH);
    setText(
      'relay-setup-canonical-recipients',
      view
        ? (view.canonical_surplus_recipients.length === 0
          ? 'None \u2014 all-cycles mode'
          : view.canonical_surplus_recipients.map((recipient) => {
            if ('Neuron' in recipient) {
              return `Neuron: ${recipient.Neuron.neuron_id}\n${relayMemoDisplay(recipient.Neuron.memo)}`;
            }
            return `Principal: ${principalText(recipient.Principal.principal)}\n${relayMemoDisplay(recipient.Principal.memo)}`;
          }).join('\n'))
        : DASH,
    );
    setText(
      'relay-setup-surplus-mode',
      view ? (Number(view.surplus_recipient_count) === 0 ? 'All-cycles mode' : 'Recipient routing') : DASH,
    );
    setText('relay-setup-configuration-hash', view?.setup_key_identifier || DASH);
    setText('relay-setup-base-minimum', view ? formatIcpE8s(view.singleton_nominal_minimum_e8s) : DASH);
    setText('relay-setup-extra-count', view ? String(view.extra_target_count) : DASH);
    setText('relay-setup-extra-unit', view ? formatIcpE8s(view.extra_target_unit_charge_e8s) : DASH);
    setText('relay-setup-extra-total', view ? formatIcpE8s(view.total_extra_target_charge_e8s) : DASH);
    setText('relay-setup-minimum', view ? formatIcpE8s(view.nominal_minimum_e8s) : DASH);
    setText('relay-setup-requirement', view ? formatIcpE8s(effectiveRequirement) : DASH);
    setText('relay-setup-balance', state.balanceE8s === null ? DASH : formatIcpE8s(state.balanceE8s));
    setText('relay-setup-icrc-account', accountText || DASH);
    setText('relay-setup-account-identifier', identifier || DASH);
    setAccountLink('relay-setup-icrc-account-link', accountText, identifier);
    setAccountLink('relay-setup-account-identifier-link', identifier, identifier);
    setHidden('relay-setup-payment-details', !account || activeOrBlocked);
    setHidden('relay-setup-create-panel', resumableFinalization ? false : (!account || activeOrBlocked));
    setText(
      'relay-setup-finalization-resume-note',
      'Funding is complete. Resume performs only authoritative Relay status checks and, when the Relay is still in the exact Historian-controlled state, may retry the controller-removal settings update. It does not repeat ICP, CMC, creation, installation, or Relay-funding operations.',
    );
    setHidden('relay-setup-finalization-resume-note', !resumableFinalization);
    if (createButton) {
      createButton.textContent = resumableFinalization ? 'Resume finalization' : 'Create Relay';
      createButton.disabled = resumableFinalization ? state.creating : !canCreate;
    }
    setHtml('relay-setup-existing-relay', displayedRelayId ? `<p>Relay: ${renderCanisterTrackerLink(displayedRelayId)}</p>` : '');
    setHidden('relay-setup-existing-relay', !displayedRelayId);
  }

  function setupArgs() {
    return {
      target_canister_ids: [...state.targets],
      surplus_recipients: state.surplusRecipients.map((recipient) => {
        if ('Neuron' in recipient) {
          return { Neuron: { ...recipient.Neuron, memo: [...recipient.Neuron.memo] } };
        }
        return { Principal: { ...recipient.Principal, memo: [...recipient.Principal.memo] } };
      }),
    };
  }

  async function preflightNeuronRecipients(agent) {
    const neuronIds = [...new Set(state.surplusRecipients
      .filter((recipient) => 'Neuron' in recipient)
      .map((recipient) => recipient.Neuron.neuron_id))];
    if (neuronIds.length === 0) return;
    const governance = governanceActorFactory(GOVERNANCE_CANISTER_ID, { agent });
    for (const neuronId of neuronIds) {
      try {
        await publicNeuronLoader({ governance, neuronId });
      } catch {
        throw new Error(`Could not verify neuron ${neuronId} as publicly readable by NNS Governance. Check the neuron ID and try again.`);
      }
    }
  }

  async function refresh({
    expectedConfiguration = state.checkedConfigurationFingerprint,
    requestGeneration = generation,
    preflightNeurons = false,
    staleMessage = 'The Relay configuration changed. Check it again.',
  } = {}) {
    if (!state.targets.length) return;
    const { agent, historian } = await historianBundle();
    if (!ensureConfigurationContinuationIsCurrent(
      expectedConfiguration,
      requestGeneration,
      staleMessage,
    )) return;
    const result = await historian.get_relay_configuration_view(setupArgs());
    if (!ensureConfigurationContinuationIsCurrent(
      expectedConfiguration,
      requestGeneration,
      staleMessage,
    )) return;
    const view = unwrapView(result);
    const kind = viewState(view);
    const account = readOptional(view.setup_account);
    const isNewConfiguration = kind === 'NotFunded' && Boolean(account);
    if (preflightNeurons && isNewConfiguration) {
      await preflightNeuronRecipients(agent);
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        staleMessage,
      )) return;
    }
    let balance = null;
    if (isNewConfiguration) {
      const ledger = await loadLedger({ agent, historian });
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        staleMessage,
      )) return;
      balance = BigInt(await ledger.icrc1_balance_of(account));
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        staleMessage,
      )) return;
    }
    state.view = view;
    state.checkedConfigurationFingerprint = expectedConfiguration;
    state.requestFingerprint = '';
    state.balanceE8s = balance;
    state.error = '';
    state.loading = false;
    if (kind === 'Active' || kind === 'ManualRecoveryRequired') {
      state.requiredBalanceOverride = null;
    }
    render();
    if (shouldPoll(view)) {
      if (pollHandle === null) startPolling();
    } else {
      stopPolling();
    }
  }

  function startPolling() {
    stopPolling();
    if (!shouldPoll()) return;
    const expectedConfiguration = state.checkedConfigurationFingerprint;
    const requestGeneration = generation;
    pollHandle = setIntervalFn(() => {
      void refresh({ expectedConfiguration, requestGeneration }).catch((error) => {
        if (!ensureConfigurationContinuationIsCurrent(
          expectedConfiguration,
          requestGeneration,
        )) return;
        state.error = normalizeError(error);
        render();
      });
    }, pollIntervalMs);
  }

  async function submitConfiguration() {
    const fingerprint = configurationFingerprint();
    generation += 1;
    const requestGeneration = generation;
    stopPolling();
    state.requestFingerprint = fingerprint;
    state.checkedConfigurationFingerprint = '';
    state.view = null;
    state.balanceE8s = null;
    state.creating = false;
    state.notifyResult = null;
    state.requiredBalanceOverride = null;
    state.error = '';
    state.loading = true;
    try {
      const validation = validateVisibleConfiguration({ includeIncomplete: true });
      if (!validation.valid) {
        validation.firstInvalidInput?.focus?.();
        throw new Error(validation.firstError);
      }
      state.targets = parseRelayTargetSet(listValues('target').join('\n'));
      if (recipientRoutingMode()) {
        const recipientRows = recipientRowsValues();
        if (recipientRows.length === 0) {
          throw new Error('Add a surplus recipient or select all-cycles mode.');
        }
        state.surplusRecipients = parseRelayRecipientSet(recipientRows);
      } else {
        state.surplusRecipients = [];
      }
      render();
      await refresh({ expectedConfiguration: fingerprint, requestGeneration, preflightNeurons: true });
    } catch (error) {
      if (!ensureConfigurationContinuationIsCurrent(fingerprint, requestGeneration)) return;
      state.targets = [];
      state.surplusRecipients = [];
      state.requestFingerprint = '';
      state.checkedConfigurationFingerprint = '';
      state.error = normalizeError(error);
      state.loading = false;
      (listInputs('target')[0] || listInputs('recipient')[0])?.focus?.();
      render();
    }
  }

  async function createRelay() {
    if (state.creating) return;
    const resumableFinalization = isResumableFinalization();
    const currentKind = viewState(state.view);
    if (['Active', 'ManualRecoveryRequired'].includes(currentKind)
      || (currentKind === 'InProgress' && !resumableFinalization)) return;
    const expectedConfiguration = state.checkedConfigurationFingerprint;
    const requestGeneration = generation;
    if (!state.targets.length || !checkedConfigurationStillMatchesVisibleForm(expectedConfiguration)) {
      rejectStaleCheckedConfiguration(
        'The Relay configuration changed. Check it again before creating.',
      );
      return;
    }
    const routing = recipientRoutingMode();
    const targetCountMatches = Number(state.view.target_count) === state.targets.length;
    const recipientCount = Number(state.view.surplus_recipient_count);
    const recipientCountMatches = recipientCount === state.surplusRecipients.length;
    const modeMatches = routing
      ? state.surplusRecipients.length >= 1
        && state.surplusRecipients.length <= MAX_RECIPIENTS
        && recipientCount >= 1
      : state.surplusRecipients.length === 0 && recipientCount === 0;
    if (!targetCountMatches || !recipientCountMatches || !modeMatches) {
      rejectStaleCheckedConfiguration(
        'The surplus mode changed. Check the Relay configuration again before creating.',
      );
      return;
    }
    const notifyArgs = setupArgs();
    if (state.requiredBalanceOverride !== null
      && state.balanceE8s !== null
      && state.balanceE8s >= state.requiredBalanceOverride) {
      state.requiredBalanceOverride = null;
    }
    state.creating = true;
    state.notifyResult = null;
    render();
    const preNotificationStaleMessage = 'The Relay configuration changed. Check it again before creating.';
    const postNotificationStaleMessage = 'The form changed while the Relay request was being processed. Check the configuration again to see its current state.';
    let notificationStarted = false;
    let notificationResolved = false;
    try {
      const { historian } = await historianBundle();
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        preNotificationStaleMessage,
      )) return;
      const notifyPromise = historian.notify_relay_configuration(notifyArgs);
      notificationStarted = true;
      const result = await notifyPromise;
      notificationResolved = true;
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        postNotificationStaleMessage,
      )) return;
      state.notifyResult = result;
      state.creating = false;
      const notifyKind = variantName(result);
      if (notifyKind === 'BelowMinimum' || notifyKind === 'BelowCurrentRequirement') {
        state.requiredBalanceOverride = BigInt(result[notifyKind].required_e8s);
      } else if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') {
        state.requiredBalanceOverride = null;
      }
      await refresh({
        expectedConfiguration,
        requestGeneration,
        staleMessage: postNotificationStaleMessage,
      });
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        postNotificationStaleMessage,
      )) return;
      if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') stopPolling();
    } catch (error) {
      const staleMessage = notificationStarted
        ? postNotificationStaleMessage
        : preNotificationStaleMessage;
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        staleMessage,
      )) return;
      if (notificationResolved) {
        rejectStaleCheckedConfiguration(
          'The Relay request returned, but its current state could not be refreshed. Check the configuration again.',
        );
        return;
      }
      if (notificationStarted) {
        rejectStaleCheckedConfiguration(
          'The Relay request may have been processed, but its current state could not be confirmed. Check the configuration again.',
        );
        return;
      }
      state.creating = false;
      state.error = normalizeError(error);
      render();
    }
  }

  function bindCopy(buttonId, valueId) {
    const button = document.getElementById(buttonId);
    if (!button || button.dataset.bound === 'true') return;
    button.dataset.bound = 'true';
    button.addEventListener('click', async () => {
      const value = document.getElementById(valueId)?.textContent || '';
      if (typeof copyTextToClipboard === 'function' && value && value !== DASH) await copyTextToClipboard(value);
    });
  }

  function bindPane() {
    for (const kind of ['target', 'recipient']) {
      const spec = listSpecs[kind];
      const list = listNode(kind);
      if (list && list.dataset.bound !== 'true') {
        list.dataset.bound = 'true';
        list.addEventListener('input', (event) => {
          const destinationInput = event.target?.matches?.(spec.inputSelector);
          const memoInput = kind === 'recipient'
            && event.target?.matches?.(spec.memoInputSelector);
          if (!destinationInput && !memoInput) return;
          if (memoInput) {
            const row = event.target.closest?.(spec.rowSelector);
            setMemoConversionNotice(row);
          }
          handleVisibleConfigurationChange();
        });
        if (kind === 'recipient') {
          list.addEventListener('change', (event) => {
            const row = event.target.closest?.(spec.rowSelector);
            if (!row) return;
            if (event.target?.matches?.(spec.typeSelector)) applyRecipientType(row);
            else if (event.target?.matches?.(spec.memoModeSelector)) {
              // Every representation interaction invalidates authoritative async state,
              // including an exact conversion and a refused conversion.
              invalidateCurrentConfiguration();
              tryApplyRecipientMemoModeChange(row);
            }
            else return;
            updateListRows(kind);
            handleVisibleConfigurationChange({ includeIncomplete: true });
          });
          listRows(kind).forEach(applyRecipientType);
        }
        list.addEventListener('click', (event) => {
          const removeButton = event.target?.closest?.(spec.removeSelector);
          if (!removeButton) return;
          removeListField(kind, removeButton);
        });
        updateListRows(kind);
      }
      const addButton = document.getElementById(spec.addId);
      if (addButton && addButton.dataset.bound !== 'true') {
        addButton.dataset.bound = 'true';
        addButton.addEventListener('click', () => addListField(kind));
      }
    }
    for (const id of ['relay-setup-mode-routing', 'relay-setup-mode-all-cycles']) {
      const radio = document.getElementById(id);
      if (!radio || radio.dataset.bound === 'true') continue;
      radio.dataset.bound = 'true';
      radio.addEventListener('change', () => {
        if (id === 'relay-setup-mode-routing' && radio.checked && listRows('recipient').length === 0) {
          addListField('recipient');
        }
        updateSurplusModePresentation();
        handleVisibleConfigurationChange({ includeIncomplete: true });
      });
    }
    updateSurplusModePresentation();
    validateVisibleConfiguration();
    const form = document.getElementById('relay-setup-form');
    if (form && form.dataset.bound !== 'true') {
      form.dataset.bound = 'true';
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        void submitConfiguration();
      });
    }
    const createButton = document.getElementById('relay-setup-create');
    if (createButton && createButton.dataset.bound !== 'true') {
      createButton.dataset.bound = 'true';
      createButton.addEventListener('click', () => void createRelay());
    }
    bindCopy('copy-relay-setup-icrc-account', 'relay-setup-icrc-account');
    bindCopy('copy-relay-setup-account-identifier', 'relay-setup-account-identifier');
  }

  return {
    state,
    bindPane,
    submitTarget: submitConfiguration,
    submitConfiguration,
    createRelay,
    refresh,
    stopPolling,
    teardown: stopPolling,
    render,
    addTargetField,
    addRecipientField,
    removeTargetField,
    removeRecipientField,
    validateVisibleTargetFields,
    validateVisibleConfiguration,
  };
}
