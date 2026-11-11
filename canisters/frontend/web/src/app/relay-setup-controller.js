import { Principal } from '@icp-sdk/core/principal';
import { createActor as createLedgerActor } from '../../declarations/icp_ledger/index.js';
import { createActor as createGovernanceActor } from '../../declarations/nns_governance/index.js';
import { createHistorianClient, normalizeError } from './agent.js';
import { GOVERNANCE_CANISTER_ID, ICP_LEDGER_CANISTER_ID, JUPITER_STAKING_ACCOUNT } from './config.js';
import {
  accountIdentifierHex,
  bytesToHex,
  icrcAccountText,
  readOptional,
} from '../data/dashboard-transforms.js';
import {
  deriveRelayFundingDestinations,
  relayEndowmentMemo,
} from '../data/relay-funding.js';
import { loadPublicNeuronStakingAccount } from '../data/nns-neurons.js';
import { DASH, formatIcpE8s, renderCanisterTrackerLink } from './view-formatters.js';

const MAX_TARGETS = 20;
const MAX_RECIPIENTS = 5;
// Protocol parity: jupiter_memo_policy::MAX_RELAY_SURPLUS_MEMO_BYTES.
export const MAX_RELAY_SURPLUS_MEMO_BYTES = 32;
const MAX_U64 = 18_446_744_073_709_551_615n;
const DEFAULT_POLL_INTERVAL_MS = 12_000;
export { icrcAccountText } from '../data/dashboard-transforms.js';

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

function escapeHtml(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function fundingCopyButton(index, field, label) {
  return `<button class="pane-copy-button" type="button" data-relay-funding-copy="${field}" data-relay-funding-index="${index}">${label}</button>`;
}

// Keep the protocol-derived catalogue; present only the selected destination.
export function renderRelayFundingCards(destinations, index = 0) {
  const destination = destinations[index];
  if (!destination) return '';
  const title = destination.subaccountNumber === null ? 'Operating funds'
    : destination.subaccountNumber === 1 ? 'Endowment via Relay staging'
      : `${destination.subaccountNumber}% operating / ${100 - destination.subaccountNumber}% endowment`;
  const explorer = `https://dashboard.internetcomputer.org/account/${encodeURIComponent(destination.legacyAccountIdentifier)}`;
  return `<article class="relay-funding-card">
    <h4>${escapeHtml(title)}</h4>
    <p class="pane-status-note">Complete payment account</p>
    <div class="relay-funding-value"><code class="mono">${escapeHtml(destination.icrcAccountText)}</code>${fundingCopyButton(index, 'icrcAccountText', 'Copy account')}</div>
    <details class="relay-funding-details"><summary>Other wallet formats</summary><dl>
      <div><dt>Owner</dt><dd class="mono">${escapeHtml(destination.owner.toText())} ${fundingCopyButton(index, 'ownerText', 'Copy owner')}</dd></div>
      <div><dt>Full 32-byte subaccount</dt><dd class="mono relay-funding-value">${escapeHtml(destination.subaccountHex)} ${fundingCopyButton(index, 'subaccountHex', 'Copy subaccount')}</dd></div>
      <div><dt>Legacy ICP account identifier</dt><dd class="mono relay-funding-value"><a class="pane-external-link" href="${explorer}" target="_blank" rel="noopener noreferrer">${escapeHtml(destination.legacyAccountIdentifier)}</a> ${fundingCopyButton(index, 'legacyAccountIdentifier', 'Copy identifier')}</dd></div>
    </dl></details>
    ${destination.subaccountNumber !== null && destination.subaccountNumber !== 1 ? `<p class="pane-status-note">The account suffix is hexadecimal: .${destination.subaccountNumber.toString(16)} means subaccount ${destination.subaccountNumber}, selecting ${destination.subaccountNumber}% operating funds.</p>` : ''}
  </article>`;
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
    activeRelayId: '',
    fundingDestinations: [],
    reviewing: false,
  };
  let creationRequestInFlight = false;
  let fundingOutcome = 'operating';
  let fundingSelection = 0;
  let generation = 0;
  let refreshSequence = 0;
  let pollInFlight = false;
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
    return document.getElementById('relay-setup-mode-routing')?.checked === true;
  }

  function configurationFingerprint() {
    return JSON.stringify({
      historian: frontendConfig.historianCanisterId,
      network: frontendConfig.network,
      ledger: frontendConfig.ledgerCanisterId,
      host: hostProvider(),
      local: isLocalHost(),
      targets: rawListValues('target'),
      surplusMode: recipientRoutingMode() ? 'routing'
        : document.getElementById('relay-setup-mode-all-cycles')?.checked ? 'all-cycles' : '',
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
    if (message) {
      const disclosure = row?.querySelector?.('[data-relay-recipient-memo]');
      if (disclosure) disclosure.open = true;
      memoInput?.setAttribute?.('aria-invalid', 'true');
    }
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
      counter.textContent = `${count === null ? '-' : count}/${MAX_RELAY_SURPLUS_MEMO_BYTES} bytes`;
    }
    setMemoError(row, message);
    const summary = row?.querySelector?.('[data-relay-recipient-memo-summary]');
    if (summary) summary.textContent = message ? 'Edit memo - check its bytes'
      : count ? `Memo added · ${count} bytes - edit` : 'Optional surplus recipient memo';
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
      if (!value) return includeIncomplete ? `Enter a ${spec.noun} or remove this field.` : '';
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
          : (!state.reviewing ? state.error : ''));
      warning.hidden = !warning.textContent;
    }
    setHidden('relay-setup-error-focus', !warning?.textContent
      || !(target.firstInvalidInput || recipient.firstInvalidInput));
    const modeChosen = recipientRoutingMode()
      || document.getElementById('relay-setup-mode-all-cycles')?.checked === true;
    const hasEmptyFields = !modeChosen || listInputs('target').length === 0
      || listValues('target').some((value) => !value)
      || (recipientRoutingMode() && (
        listInputs('recipient').length === 0 || listValues('recipient').some((value) => !value)
      ));
    const emptyFieldError = recipientRoutingMode() && listInputs('recipient').length === 0
      ? 'Add a surplus recipient or select all-cycles mode.'
      : '';
    const submitButton = document.getElementById('relay-setup-submit');
    if (submitButton) submitButton.disabled = state.loading || state.creating
      || !target.valid || !recipient.valid || hasEmptyFields;
    setText('relay-setup-submit-reason', !modeChosen ? 'Choose how funding should be used.'
      : hasEmptyFields ? 'Complete the canister and recipient fields.'
        : !target.valid || !recipient.valid ? 'Check the highlighted fields.'
          : state.loading ? 'Checking your configuration…' : 'Ready to review.');
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
          : `${number === 1 && rows.length === 1 ? 'Canister ID' : `Canister ID ${number}`}`;
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
    setHidden('relay-setup-equal-split', !routing || listRows('recipient').length < 2);
    setText(
      'relay-setup-surplus-mode-summary',
      routing
        ? 'Cycles first, then surplus payments selected.'
        : document.getElementById('relay-setup-mode-all-cycles')?.checked
          ? 'All funding for cycles selected.' : 'Choose a funding policy.',
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
      principalOption.textContent = 'Principal default ICP account';
      const neuronOption = document.createElement('option');
      neuronOption.value = 'Neuron';
      neuronOption.textContent = 'Public NNS neuron ID';
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
      memoLabel.textContent = 'Surplus recipient memo';
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
      const disclosure = document.createElement('details');
      disclosure.className = 'relay-setup-memo';
      disclosure.dataset.relayRecipientMemo = 'true';
      const summary = document.createElement('summary');
      summary.dataset.relayRecipientMemoSummary = 'true';
      summary.textContent = 'Optional surplus recipient memo';
      const removeMemo = document.createElement('button');
      removeMemo.type = 'button';
      removeMemo.className = 'pane-page-button';
      removeMemo.dataset.relayRecipientMemoRemove = 'true';
      removeMemo.textContent = 'Remove memo';
      disclosure.append(summary, memoControls, memoError, memoNotice, removeMemo);
      row.append(disclosure);
      applyRecipientType(row);
      updateRecipientMemoState(row);
    }
    return row;
  }

  function invalidateCurrentConfiguration() {
    generation += 1;
    refreshSequence += 1;
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
    state.activeRelayId = '';
    state.fundingDestinations = [];
    fundingSelection += 1;
    setText('relay-setup-copy-status', '');
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
    updateSurplusModePresentation();
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
    // Removing a recipient never silently changes the immutable funding policy.
    updateListRows(kind);
    updateSurplusModePresentation();
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

  function recoveryState(view = state.view) {
    if (variantName(state.notifyResult) === 'Active' || viewState(view) === 'Active') return null;
    return readOptional(view?.recovery);
  }

  function recoveryQuote(view = state.view) {
    const recovery = recoveryState(view);
    return recovery?.Ready || recovery?.FundingRequired || null;
  }

  function shouldPoll(view = state.view) {
    if (viewState(view) === 'Active' || variantName(state.notifyResult) === 'Active') return false;
    if (variantName(recoveryState(view)) === 'Unsafe') return false;
    if (recoveryQuote(view)) return true;
    if (variantName(recoveryState(view)) === 'CheckPending' && variantName(state.notifyResult) === 'InProgress') return true;
    if (['Active', 'ManualRecoveryRequired'].includes(variantName(state.notifyResult))) return false;
    const kind = viewState(view);
    return kind === 'InProgress'
      || (kind === 'NotFunded' && Boolean(readOptional(view?.setup_account)));
  }

  function isResumableSetup(view = state.view) {
    const assessment = recoveryState(view);
    if (assessment) return Boolean(assessment.Ready);
    const notifyKind = variantName(state.notifyResult);
    if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') return false;
    if (viewState(view) !== 'InProgress') return false;
    const phase = variantName(view?.state?.InProgress?.phase);
    return ['CmcTransferAccepted', 'CmcNotifySucceeded', 'RelayFunded', 'FinalizationAttempted'].includes(phase);
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

  function loadLedger({ agent }) {
    const local = frontendConfig.network === 'local' && isLocalHost();
    const configured = local ? frontendConfig.ledgerCanisterId : ICP_LEDGER_CANISTER_ID;
    if (!configured) throw new Error('Local ICP Ledger is not configured for this build.');
    const ledgerId = Principal.fromText(configured).toText();
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
          status: 'Setup temporarily busy',
          message: 'Your request could not start right now. No new payment is needed. Wait a little before trying again.',
        };
      case 'InProgress':
        return {
          status: 'In progress',
          message: 'Your setup is being processed. Checking status never sends another payment.',
        };
      case 'Active':
        return { status: 'Relay created', message: '' };
      case 'FailedPreSpend':
        return { status: 'Failed before spend', message: details?.message || DASH };
      case 'ManualRecoveryRequired': {
        const relayId = principalText(details?.relay_canister_id);
        return {
          status: 'Manual recovery required',
          message: [relayId ? `Relay ${relayId}` : '', details?.message || '', 'Keep this exact configuration for support. Do not pay again or restart creation.'].filter(Boolean).join(' · '),
        };
      }
      default:
        return null;
    }
  }

  function render() {
    if (state.checkedConfigurationFingerprint
      && configurationFingerprint() !== state.checkedConfigurationFingerprint) {
      invalidateCurrentConfiguration();
      return;
    }
    const operationLocked = creationRequestInFlight || state.loading;
    // Editing can discard a non-spending check through the generation fence.
    // A submitted creation/continuation stays locked through its follow-up read.
    for (const id of ['relay-setup-edit', 'relay-setup-another']) {
      const button = document.getElementById(id);
      if (button) button.disabled = creationRequestInFlight;
    }
    for (const id of ['copy-relay-setup-icrc-account', 'copy-relay-setup-account-identifier']) {
      const button = document.getElementById(id);
      if (button) button.disabled = operationLocked;
    }
    const view = state.view;
    const kind = viewState(view);
    const stateDetails = view?.state?.[kind];
    const relayId = relayIdFromState(view?.state);
    const recovery = recoveryState(view);
    const quote = recoveryQuote(view);
    const recoveryKind = variantName(recovery);
    const recoveryBlocksSpending = ['Unsafe', 'CheckPending'].includes(recoveryKind);
    const manualWarning = 'Keep this exact configuration for support. Do not pay again or restart creation.';
    const recoveryNotice = quote
      ? (BigInt(quote.additional_e8s) > 0n
        ? `Additional setup funding needed.${quote.relay_canister_id.length ? ' Your Relay has already been created.' : ''} Add ${formatIcpE8s(quote.additional_e8s)} to the setup account below, then continue setup. This amount must arrive in the account; your wallet may charge a separate sending fee.`
        : 'No additional setup payment is needed. Continue setup to check and finish the remaining steps.')
      : recoveryKind === 'CheckPending' ? (pollHandle !== null
        ? "Your setup is still being checked. We'll check again automatically. Do not send another payment until the check succeeds."
        : "We couldn't check your setup right now. Select Re-check this setup to try again. Do not send another payment until the check succeeds.")
        : recoveryKind === 'Unsafe' ? `We couldn't confirm that it is safe to continue this setup. ${manualWarning}` : '';
    const recoveryMessage = kind === 'ManualRecoveryRequired'
      ? `${stateDetails?.message || ''} ${manualWarning}` : '';
    const phase = stateDetails?.phase ? variantName(stateDetails.phase) : '';
    const notifyKind = variantName(state.notifyResult);
    const notifyDetails = state.notifyResult?.[notifyKind];
    const notification = notifyPresentation();
    const account = quote?.setup_account || readOptional(view?.setup_account);
    const accountText = account ? icrcAccountText(account) : '';
    const identifier = account ? accountIdentifierHex(account) : '';
    const notifiedRelayId = ['Active', 'InProgress', 'ManualRecoveryRequired'].includes(notifyKind)
      ? principalText(notifyDetails?.relay_canister_id)
      : '';
    const displayedRelayId = relayId || notifiedRelayId;
    const activeRelayId = kind === 'Active'
      ? relayId
      : notifyKind === 'Active' ? notifiedRelayId : '';
    const activeChanged = state.activeRelayId !== activeRelayId;
    if (activeChanged) {
      fundingSelection += 1;
      state.fundingDestinations = activeRelayId ? deriveRelayFundingDestinations(activeRelayId) : [];
    }
    state.activeRelayId = activeRelayId;
    const activeOrBlocked = ['Active', 'InProgress', 'ManualRecoveryRequired'].includes(kind)
      || ['Active', 'InProgress', 'ManualRecoveryRequired'].includes(notifyKind);
    const resumableSetup = isResumableSetup(view);
    const resumesCreation = ['CmcTransferAccepted', 'CmcNotifySucceeded'].includes(phase);
    const resumeLabel = quote ? 'Continue setup' : resumesCreation ? 'Resume creation' : 'Resume finalization';
    const createButton = document.getElementById('relay-setup-create');
    const balance = state.balanceE8s === null ? 0n : state.balanceE8s;
    const effectiveRequirement = quote ? BigInt(quote.required_e8s) : requiredBalance(view, state.requiredBalanceOverride);
    const canCreate = Boolean(view && account && !activeOrBlocked && !recoveryBlocksSpending && !state.error
      && balance >= effectiveRequirement && !operationLocked);
    const fundingNotice = ['BelowMinimum', 'BelowCurrentRequirement'].includes(notifyKind);
    const notificationStatus = fundingNotice
      ? (canCreate ? 'Ready to create' : 'Waiting for funding')
      : resumableSetup ? 'Ready to resume' : notification?.status;
    const status = {
      NotFunded: !view?.factory_available ? 'Creation unavailable' : canCreate ? 'Ready to create' : 'Waiting for funding',
      InProgress: resumableSetup ? 'Ready to resume' : 'Creation in progress',
      ManualRecoveryRequired: 'Manual recovery required',
      Active: 'Active',
    }[kind] || DASH;

    setHidden('relay-setup-operation-notice', !creationRequestInFlight);
    const reviewStage = !activeRelayId && state.reviewing;
    setHidden('relay-setup-form', state.reviewing || Boolean(activeRelayId));
    setHidden('relay-setup-intro', state.reviewing || Boolean(activeRelayId));
    setHidden('relay-setup-review', !reviewStage);
    setText('relay-setup-title', activeRelayId ? 'Relay' : state.reviewing ? 'Review your setup' : 'Create a relay');
    setHidden('relay-setup-recheck', (!state.error && !recovery) || Boolean(activeRelayId));
    const recheckButton = document.getElementById('relay-setup-recheck');
    if (recheckButton) recheckButton.disabled = operationLocked;
    setHidden('relay-setup-summary', !view);
    // Instructions and controls share the same operation lock. Assessments are
    // non-spending; continuation may spend and stays locked through its next read.
    const operationMessage = state.creating
      ? 'Continuing your Relay setup. No action is needed right now. Do not send another setup payment.'
      : 'Checking your Relay setup. No action is needed right now. Do not send another setup payment.';
    setText('relay-setup-status', activeRelayId ? 'Active'
      : operationLocked ? (state.creating ? 'Processing setup…' : 'Checking setup…')
        : state.error ? state.error
          : quote ? (BigInt(quote.additional_e8s) > 0n ? 'Additional setup funding needed' : 'Ready to continue setup')
            : recoveryKind === 'CheckPending' ? 'Setup check temporarily unavailable'
              : recoveryKind === 'Unsafe' ? 'Setup needs review' : notificationStatus || status);
    setText('relay-setup-status-label', activeRelayId ? '' : operationLocked ? operationMessage
      : state.error || recoveryNotice || (resumableSetup ? `No additional setup payment is needed. Select ${resumeLabel} to continue.` : '') || (fundingNotice ? (canCreate
        ? 'Your setup balance is sufficient. Select Create relay to continue.'
        : `Last creation check: ${notification.message}`) : notification?.message) || recoveryMessage
      || (kind === 'InProgress' ? (resumableSetup
        ? `No additional setup payment is needed. Select ${resumeLabel} to continue.`
        : 'Your setup is being processed. Do not send another setup deposit.') : ''));
    setHidden('relay-setup-reviewed-recipients', Number(view?.surplus_recipient_count || 0) === 0);
    setText('relay-setup-target-count', view ? String(view.target_count) : DASH);
    setText('relay-setup-canonical-targets', view ? view.canonical_target_canister_ids.map(principalText).join('\n') : DASH);
    setText('relay-setup-recipient-count', view ? String(view.surplus_recipient_count) : DASH);
    setText(
      'relay-setup-canonical-recipients',
      view
        ? (view.canonical_surplus_recipients.length === 0
          ? 'None - all funding for cycles'
          : view.canonical_surplus_recipients.map((recipient) => {
            if ('Neuron' in recipient) {
              return `Public NNS neuron: ${recipient.Neuron.neuron_id}\n${relayMemoDisplay(recipient.Neuron.memo)}`;
            }
            return `Principal default ICP account: ${principalText(recipient.Principal.principal)}\n${relayMemoDisplay(recipient.Principal.memo)}`;
          }).join('\n'))
        : DASH,
    );
    setText(
      'relay-setup-surplus-mode',
      view ? (Number(view.surplus_recipient_count) === 0 ? 'All funding for cycles' : 'Cycles first, then surplus payments') : DASH,
    );
    setText('relay-setup-base-minimum', view ? formatIcpE8s(view.singleton_nominal_minimum_e8s) : DASH);
    setText('relay-setup-extra-count', view ? String(view.extra_target_count) : DASH);
    setText('relay-setup-extra-unit', view ? formatIcpE8s(view.extra_target_unit_charge_e8s) : DASH);
    setText('relay-setup-extra-total', view ? formatIcpE8s(view.total_extra_target_charge_e8s) : DASH);
    setText('relay-setup-minimum-label', quote ? 'Remaining setup balance needed' : 'Setup minimum');
    setText('relay-setup-minimum', quote ? formatIcpE8s(quote.required_e8s) : view ? formatIcpE8s(view.nominal_minimum_e8s) : DASH);
    setHidden('relay-setup-original-pricing', Boolean(quote));
    setText('relay-setup-requirement', view ? formatIcpE8s(effectiveRequirement) : DASH);
    setText('relay-setup-shortfall', quote ? formatIcpE8s(quote.additional_e8s)
      : state.balanceE8s === null ? DASH
        : formatIcpE8s(effectiveRequirement > balance ? effectiveRequirement - balance : 0n));
    setHidden('relay-setup-live-requirement', Boolean(quote) || !view || effectiveRequirement <= BigInt(view.nominal_minimum_e8s));
    setText('relay-setup-live-requirement', `Current live requirement: ${formatIcpE8s(effectiveRequirement)}. This is higher than the nominal minimum.`);
    setText('relay-setup-balance', state.balanceE8s === null ? DASH : formatIcpE8s(state.balanceE8s));
    setText('relay-setup-icrc-account', accountText || DASH);
    setText('relay-setup-account-identifier', identifier || DASH);
    setAccountLink('relay-setup-icrc-account-link', accountText, identifier);
    setAccountLink('relay-setup-account-identifier-link', identifier, identifier);
    setHidden('relay-setup-payment-details', operationLocked || recoveryBlocksSpending || !account || (activeOrBlocked && !quote) || Boolean(state.error));
    setHidden('relay-setup-create-panel', recoveryBlocksSpending || Boolean(state.error) || (quote || resumableSetup ? false : (!account || activeOrBlocked)));
    setText(
      'relay-setup-finalization-resume-note',
      resumesCreation
        ? 'Your setup payment is recorded. Resuming creation checks its result and finishes setup when confirmed. No additional setup payment is needed.'
        : 'Setup funding is complete. Resume finalization checks your Relay and finishes activation. No additional setup payment is needed.',
    );
    setHidden('relay-setup-finalization-resume-note', operationLocked || !resumableSetup || Boolean(quote));
    if (createButton) {
      createButton.textContent = quote ? 'Continue setup' : resumableSetup ? resumeLabel : 'Create relay';
      createButton.disabled = state.error || operationLocked ? true
        : quote ? state.creating || BigInt(quote.additional_e8s) > 0n
          : resumableSetup ? state.creating : !canCreate;
    }
    setHtml('relay-setup-existing-relay', displayedRelayId ? `<p>Relay: ${renderCanisterTrackerLink(displayedRelayId)}</p>` : '');
    setHidden('relay-setup-existing-relay', !displayedRelayId);
    setText('relay-funding-owner', activeRelayId || DASH);
    setText('relay-funding-config-summary', view ? `${view.target_count} canister${Number(view.target_count) === 1 ? '' : 's'} · ${Number(view.surplus_recipient_count) ? 'Cycles first, then surplus payments' : 'All funding for cycles'}` : '');
    setText('relay-funding-configuration', view ? `${view.canonical_target_canister_ids.map(principalText).join('\n')}\n\n${document.getElementById('relay-setup-canonical-recipients')?.textContent || ''}` : '');
    setHtml('relay-funding-activity', activeRelayId ? renderCanisterTrackerLink(activeRelayId, { label: 'View Relay activity' }) : '');
    setText('relay-funding-copy-status', '');
    setText('relay-funding-endowment-memo', activeRelayId ? relayEndowmentMemo(activeRelayId) : '');
    setText('relay-funding-staking-account', activeRelayId ? JUPITER_STAKING_ACCOUNT.address : '');
    setHidden('relay-funding-section', !activeRelayId);
    if (activeChanged && activeRelayId) document.getElementById('relay-funding-title')?.focus?.();
    renderFundingChoice();
    validateVisibleConfiguration();
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
    const sequence = ++refreshSequence;
    state.loading = true;
    render();
    try {
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
      if (sequence !== refreshSequence) return;
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
      let balance = recoveryQuote(view) ? BigInt(recoveryQuote(view).balance_e8s) : null;
      if (isNewConfiguration) {
        const ledger = loadLedger({ agent });
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
      if (sequence !== refreshSequence) return;
      // An immutable authenticated Active mapping must not regress to a funding
      // account when an older snapshot or continuation completes later.
      if (viewState(state.view) === 'Active' && kind !== 'Active') return;
      state.view = view;
      state.checkedConfigurationFingerprint = expectedConfiguration;
      state.requestFingerprint = '';
      state.balanceE8s = balance;
      state.error = '';
      state.loading = false;
      if (kind === 'Active' || kind === 'ManualRecoveryRequired') {
        state.requiredBalanceOverride = null;
      }
      if (shouldPoll(view)) {
        if (pollHandle === null) startPolling();
      } else {
        stopPolling();
      }
    } finally {
      if (sequence === refreshSequence && generation === requestGeneration) {
        state.loading = false;
        if (ensureConfigurationContinuationIsCurrent(expectedConfiguration, requestGeneration, staleMessage)) render();
      }
    }
  }

  function startPolling() {
    stopPolling();
    if (!shouldPoll()) return;
    const expectedConfiguration = state.checkedConfigurationFingerprint;
    const requestGeneration = generation;
    pollHandle = setIntervalFn(() => {
      if (pollInFlight || creationRequestInFlight || state.loading) return;
      pollInFlight = true;
      const sequence = refreshSequence + 1;
      void refresh({ expectedConfiguration, requestGeneration }).catch((error) => {
        if (sequence !== refreshSequence) return;
        if (!ensureConfigurationContinuationIsCurrent(
          expectedConfiguration,
          requestGeneration,
        )) return;
        state.error = normalizeError(error);
        state.view = null;
        stopPolling();
        render();
      }).finally(() => { pollInFlight = false; });
    }, pollIntervalMs);
  }

  async function submitConfiguration() {
    const fingerprint = configurationFingerprint();
    const previousRequirement = fingerprint === state.checkedConfigurationFingerprint
      ? state.requiredBalanceOverride : null;
    generation += 1;
    const requestGeneration = generation;
    stopPolling();
    state.requestFingerprint = fingerprint;
    state.checkedConfigurationFingerprint = '';
    state.view = null;
    state.balanceE8s = null;
    state.creating = false;
    state.notifyResult = null;
    state.requiredBalanceOverride = previousRequirement;
    state.error = '';
    state.loading = true;
    setText('relay-setup-copy-status', '');
    try {
      const validation = validateVisibleConfiguration({ includeIncomplete: true });
      if (!validation.valid) {
        const disclosure = validation.firstInvalidInput?.closest?.('[data-relay-recipient-memo]');
        if (disclosure) disclosure.open = true;
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
      state.reviewing = true;
      setHidden('relay-setup-edit-notice', true);
      render();
      document.getElementById('relay-setup-review-heading')?.focus?.();
      await refresh({ expectedConfiguration: fingerprint, requestGeneration, preflightNeurons: true });
    } catch (error) {
      if (!ensureConfigurationContinuationIsCurrent(fingerprint, requestGeneration)) return;
      state.targets = [];
      state.surplusRecipients = [];
      state.requestFingerprint = '';
      state.checkedConfigurationFingerprint = '';
      state.error = normalizeError(error);
      state.loading = false;
      if (!state.reviewing) {
        const validation = validateVisibleConfiguration({ includeIncomplete: true });
        (validation.firstInvalidInput || document.getElementById('relay-setup-mode-all-cycles') || listInputs('target')[0])?.focus?.();
        setText('relay-setup-warning', state.error);
        setHidden('relay-setup-warning', false);
      }
      render();
    }
  }

  async function createRelay() {
    const assessment = recoveryState();
    if (assessment && !assessment.Ready) return;
    if (creationRequestInFlight || state.loading || state.creating || state.activeRelayId
      || (variantName(state.notifyResult) === 'ManualRecoveryRequired' && !recoveryState()?.Ready)) return;
    const resumableSetup = isResumableSetup();
    if (variantName(state.notifyResult) === 'InProgress' && !resumableSetup) return;
    const currentKind = viewState(state.view);
    if ((['Active', 'ManualRecoveryRequired'].includes(currentKind) && !recoveryState()?.Ready)
      || (currentKind === 'InProgress' && !resumableSetup)) return;
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
    if (!resumableSetup && (state.error || state.balanceE8s === null
      || state.balanceE8s < requiredBalance(state.view, state.requiredBalanceOverride))) return;
    const notifyArgs = setupArgs();
    if (state.requiredBalanceOverride !== null
      && state.balanceE8s !== null
      && state.balanceE8s >= state.requiredBalanceOverride) {
      state.requiredBalanceOverride = null;
    }
    creationRequestInFlight = true;
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
      if (state.view) state.view = { ...state.view, recovery: [] };
      state.creating = false;
      const notifyKind = variantName(result);
      if (notifyKind === 'BelowMinimum' || notifyKind === 'BelowCurrentRequirement') {
        state.requiredBalanceOverride = BigInt(result[notifyKind].required_e8s);
      } else if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') {
        state.requiredBalanceOverride = null;
      }
      render();
      if (notifyKind === 'Active' || (notifyKind === 'ManualRecoveryRequired' && !recoveryQuote())) stopPolling();
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
      if (notifyKind === 'Active' || (notifyKind === 'ManualRecoveryRequired' && !recoveryQuote())) stopPolling();
    } catch (error) {
      const staleMessage = notificationStarted
        ? postNotificationStaleMessage
        : preNotificationStaleMessage;
      if (!ensureConfigurationContinuationIsCurrent(
        expectedConfiguration,
        requestGeneration,
        staleMessage,
      )) return;
      if (notificationResolved && variantName(state.notifyResult) === 'Active') {
        state.creating = false;
        stopPolling();
        render();
        return;
      }
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
    } finally {
      creationRequestInFlight = false;
      render();
    }
  }

  function bindCopy(buttonId, valueId) {
    const button = document.getElementById(buttonId);
    if (!button || button.dataset.bound === 'true') return;
    button.dataset.bound = 'true';
    button.addEventListener('click', async () => {
      if (!checkedConfigurationStillMatchesVisibleForm()) {
        rejectStaleCheckedConfiguration('The Relay configuration changed. Check it again.');
        return;
      }
      if (creationRequestInFlight || state.loading || ['Unsafe', 'CheckPending'].includes(variantName(recoveryState()))
        || state.error || (viewState(state.view) !== 'NotFunded' && !recoveryQuote())
        || variantName(state.notifyResult) === 'Active') return;
      const expected = state.checkedConfigurationFingerprint;
      const requestGeneration = generation;
      const value = document.getElementById(valueId)?.textContent || '';
      if (!value || value === DASH) return;
      let message = 'Account copied.';
      try {
        if (typeof copyTextToClipboard !== 'function') throw new Error('Clipboard unavailable');
        await copyTextToClipboard(value);
      } catch {
        message = 'Copy failed. Select and copy the complete account manually.';
      }
      if (configurationStillCurrent(expected, requestGeneration)
        && document.getElementById(valueId)?.textContent === value) {
        setText('relay-setup-copy-status', message);
      }
    });
  }

  function selectedFundingIndex() {
    if (fundingOutcome === 'future') return 1;
    if (fundingOutcome !== 'split') return 0;
    const percentage = Number(document.getElementById('relay-funding-percentage')?.value || 50);
    return state.fundingDestinations.findIndex((destination) => destination.subaccountNumber === percentage);
  }

  function renderFundingChoice() {
    const future = fundingOutcome === 'future';
    const split = fundingOutcome === 'split';
    const staging = document.getElementById('relay-funding-staging-alternative')?.open === true;
    setHidden('relay-funding-operating-note', future || split);
    setHidden('relay-funding-direct-endowment', !future || staging);
    setHidden('relay-funding-future-flow', !future);
    setHidden('relay-funding-split-controls', !split);
    setHidden('relay-funding-destinations', future && !staging);
    setHtml('relay-funding-destinations', state.activeRelayId
      ? renderRelayFundingCards(state.fundingDestinations, selectedFundingIndex()) : '');
    setText('relay-funding-split-note', 'Pay this complete account without a routing memo. Fees reduce both shares; the endowment share may wait for more deposits.');
  }

  function editConfiguration() {
    if (creationRequestInFlight) return;
    state.reviewing = false;
    invalidateCurrentConfiguration();
    setText('relay-setup-edit-notice', 'Any deposits remain with the exact previous setup. Editing does not move or refund them. Re-check the original configuration to resume it.');
    setHidden('relay-setup-edit-notice', false);
    listInputs('target')[0]?.focus?.();
  }

  function configureAnotherRelay() {
    if (creationRequestInFlight) return;
    state.reviewing = false;
    invalidateCurrentConfiguration();
    for (const kind of ['target', 'recipient']) {
      listNode(kind)?.replaceChildren(createListRow(kind));
      updateListRows(kind);
    }
    for (const id of ['relay-setup-mode-routing', 'relay-setup-mode-all-cycles']) {
      const input = document.getElementById(id);
      if (input) input.checked = false;
    }
    fundingOutcome = 'operating';
    for (const outcome of ['operating', 'future', 'split']) {
      const input = document.getElementById(`relay-funding-${outcome}`);
      if (input) input.checked = outcome === 'operating';
    }
    const percentage = document.getElementById('relay-funding-percentage');
    if (percentage) percentage.value = '50';
    document.querySelectorAll('#nav-panel-relay-setup details').forEach((node) => { node.open = false; });
    setHidden('relay-funding-options', true);
    document.getElementById('relay-funding-add')?.setAttribute('aria-expanded', 'false');
    setHidden('relay-setup-edit-notice', true);
    updateSurplusModePresentation();
    render();
    listInputs('target')[0]?.focus?.();
  }

  function bindEndowmentCopy(buttonId, valueId) {
    document.getElementById(buttonId)?.addEventListener('click', async () => {
      if (!checkedConfigurationStillMatchesVisibleForm() || !state.activeRelayId
        || fundingOutcome !== 'future'
        || document.getElementById('relay-funding-staging-alternative')?.open) return;
      const activeRelayId = state.activeRelayId;
      const selection = fundingSelection;
      const value = valueId === 'relay-funding-endowment-memo'
        ? relayEndowmentMemo(activeRelayId) : JUPITER_STAKING_ACCOUNT.address;
      let message = 'Copied.';
      try {
        if (typeof copyTextToClipboard !== 'function') throw new Error('Clipboard unavailable');
        await copyTextToClipboard(value);
      } catch {
        message = 'Copy failed. Select and copy the value manually.';
      }
      if (state.activeRelayId === activeRelayId && selection === fundingSelection
        && document.getElementById(valueId)?.textContent === value) {
        setText('relay-funding-copy-status', message);
      }
    });
  }

  function bindFundingCopies() {
    const container = document.getElementById('relay-funding-destinations');
    if (!container || container.dataset.bound === 'true') return;
    container.dataset.bound = 'true';
    container.addEventListener('click', async (event) => {
      const button = event.target?.closest?.('[data-relay-funding-copy]');
      if (!button) return;
      if (!checkedConfigurationStillMatchesVisibleForm()) {
        rejectStaleCheckedConfiguration('The Relay configuration changed. Check it again.');
        return;
      }
      const index = Number.parseInt(button.dataset.relayFundingIndex || '', 10);
      const field = button.dataset.relayFundingCopy;
      if (index !== selectedFundingIndex()
        || (fundingOutcome === 'future'
          && !document.getElementById('relay-funding-staging-alternative')?.open)) return;
      const selection = fundingSelection;
      const destination = state.fundingDestinations[index];
      const value = destination?.[field];
      if (!state.activeRelayId || typeof value !== 'string' || !value) return;
      const activeRelayId = state.activeRelayId;
      try {
        if (typeof copyTextToClipboard !== 'function') {
          throw new Error('Clipboard access is unavailable.');
        }
        await copyTextToClipboard(value);
        if (
          fundingSelection !== selection
          || state.activeRelayId !== activeRelayId
          || state.fundingDestinations[index] !== destination
          || destination[field] !== value
        ) return;
        setText('relay-funding-copy-status', 'Copied.');
      } catch {
        if (
          fundingSelection !== selection
          || state.activeRelayId !== activeRelayId
          || state.fundingDestinations[index] !== destination
          || destination[field] !== value
        ) return;
        setText('relay-funding-copy-status', 'Copy failed. Select and copy the value manually.');
      }
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
          const memoRemove = kind === 'recipient' && event.target?.closest?.('[data-relay-recipient-memo-remove]');
          if (memoRemove) {
            const row = memoRemove.closest(spec.rowSelector);
            const input = row?.querySelector?.(spec.memoInputSelector);
            if (input) input.value = '';
            setMemoConversionNotice(row);
            handleVisibleConfigurationChange();
            return;
          }
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
        if (!creationRequestInFlight && !state.loading) void submitConfiguration();
      });
    }
    const createButton = document.getElementById('relay-setup-create');
    if (createButton && createButton.dataset.bound !== 'true') {
      createButton.dataset.bound = 'true';
      createButton.addEventListener('click', () => void createRelay());
    }
    bindCopy('copy-relay-setup-icrc-account', 'relay-setup-icrc-account');
    bindCopy('copy-relay-setup-account-identifier', 'relay-setup-account-identifier');
    bindFundingCopies();
    bindEndowmentCopy('relay-funding-copy-endowment-memo', 'relay-funding-endowment-memo');
    bindEndowmentCopy('relay-funding-copy-staking-account', 'relay-funding-staking-account');
    document.getElementById('relay-setup-edit')?.addEventListener('click', editConfiguration);
    document.getElementById('relay-setup-another')?.addEventListener('click', configureAnotherRelay);
    document.getElementById('relay-setup-recheck')?.addEventListener('click', () => {
      if (!creationRequestInFlight && !state.loading) void submitConfiguration();
    });
    document.getElementById('relay-setup-error-focus')?.addEventListener('click', () => {
      const { firstInvalidInput } = validateVisibleConfiguration({ includeIncomplete: true });
      const disclosure = firstInvalidInput?.closest?.('[data-relay-recipient-memo]');
      if (disclosure) disclosure.open = true;
      firstInvalidInput?.focus?.();
    });
    document.getElementById('relay-funding-add')?.addEventListener('click', () => {
      const panel = document.getElementById('relay-funding-options');
      const button = document.getElementById('relay-funding-add');
      if (panel) { panel.hidden = !panel.hidden; button?.setAttribute('aria-expanded', String(!panel.hidden)); }
    });
    for (const outcome of ['operating', 'future', 'split']) {
      document.getElementById(`relay-funding-${outcome}`)?.addEventListener('change', () => {
        fundingOutcome = outcome;
        fundingSelection += 1;
        setText('relay-funding-copy-status', '');
        renderFundingChoice();
      });
    }
    for (const [id, event] of [['relay-funding-percentage', 'change'], ['relay-funding-staging-alternative', 'toggle']]) {
      document.getElementById(id)?.addEventListener(event, () => {
        fundingSelection += 1;
        setText('relay-funding-copy-status', '');
        renderFundingChoice();
      });
    }
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
