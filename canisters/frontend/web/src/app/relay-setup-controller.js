import { Principal } from '@icp-sdk/core/principal';
import { createActor as createLedgerActor } from '../../declarations/icp_ledger/index.js';
import { createHistorianClient, normalizeError } from './agent.js';
import { accountIdentifierHex, bytesToHex, readOptional } from '../data/dashboard-transforms.js';
import { DASH, formatIcpE8s, renderCanisterTrackerLink } from './view-formatters.js';

const MAX_TARGETS = 20;
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

export function parseRelayTargetSet(text) {
  const tokens = String(text || '').split(/[\s,]+/u).map((value) => value.trim()).filter(Boolean);
  if (tokens.length === 0) throw new Error('Enter at least one target canister ID.');
  if (tokens.length > MAX_TARGETS) throw new Error(`Enter no more than ${MAX_TARGETS} target canister IDs.`);
  const principals = tokens.map((value) => {
    try {
      return Principal.fromText(value);
    } catch {
      throw new Error(`Invalid target canister ID: ${value}`);
    }
  });
  const normalized = principals.map((principal) => principal.toText());
  if (new Set(normalized).size !== normalized.length) {
    throw new Error('Duplicate target canisters are not allowed.');
  }
  return principals;
}

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
  copyTextToClipboard = null,
  hostProvider = () => window.location.origin,
  setIntervalFn = (callback, delay) => window.setInterval(callback, delay),
  clearIntervalFn = (handle) => window.clearInterval(handle),
  pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
} = {}) {
  const state = {
    inputText: '',
    targets: [],
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

  function stopPolling() {
    if (pollHandle !== null) clearIntervalFn(pollHandle);
    pollHandle = null;
  }

  function shouldPoll(view = state.view) {
    const kind = viewState(view);
    return kind === 'InProgress'
      || (kind === 'NotFunded' && Boolean(readOptional(view?.setup_account)));
  }

  function inputStillCurrent(expected, requestGeneration) {
    return generation === requestGeneration
      && String(document.getElementById('relay-setup-target-input')?.value || '').trim() === expected;
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
    const createButton = document.getElementById('relay-setup-create');
    const balance = state.balanceE8s === null ? 0n : state.balanceE8s;
    const effectiveRequirement = requiredBalance(view, state.requiredBalanceOverride);
    const canCreate = Boolean(view && account && !activeOrBlocked && !state.error
      && balance >= effectiveRequirement && !state.creating);

    setHidden('relay-setup-result', Boolean(view || state.error || state.loading));
    setHidden('relay-setup-summary', !view && !state.error && !state.loading);
    setText('relay-setup-status', state.error || (state.loading ? 'Checking target set…' : (state.creating ? 'Creating Relay…' : (notification?.status || kind || DASH))));
    setText('relay-setup-status-label', notification?.message || recoveryMessage || phase || DASH);
    setText('relay-setup-factory', view?.factory_available ? 'Available' : 'Unavailable');
    setText('relay-setup-target-count', view ? String(view.target_count) : DASH);
    setText('relay-setup-canonical-targets', view ? view.canonical_target_canister_ids.map(principalText).join('\n') : DASH);
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
    setHidden('relay-setup-create-panel', !account || activeOrBlocked);
    if (createButton) createButton.disabled = !canCreate;
    setHtml('relay-setup-existing-relay', displayedRelayId ? `<p>Relay: ${renderCanisterTrackerLink(displayedRelayId)}</p>` : '');
    setHidden('relay-setup-existing-relay', !displayedRelayId);
  }

  async function refresh({ expectedInput = state.inputText, requestGeneration = generation } = {}) {
    if (!state.targets.length) return;
    const { agent, historian } = await historianBundle();
    const result = await historian.get_relay_setup_view({ target_canister_ids: state.targets });
    if (!inputStillCurrent(expectedInput, requestGeneration)) return;
    const view = unwrapView(result);
    let balance = null;
    const account = readOptional(view.setup_account);
    if (account) {
      const ledger = await loadLedger({ agent, historian });
      balance = BigInt(await ledger.icrc1_balance_of(account));
      if (!inputStillCurrent(expectedInput, requestGeneration)) return;
    }
    state.view = view;
    state.balanceE8s = balance;
    state.error = '';
    state.loading = false;
    const kind = viewState(view);
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
    const expectedInput = state.inputText;
    const requestGeneration = generation;
    pollHandle = setIntervalFn(() => {
      void refresh({ expectedInput, requestGeneration }).catch((error) => {
        if (!inputStillCurrent(expectedInput, requestGeneration)) return;
        state.error = normalizeError(error);
        render();
      });
    }, pollIntervalMs);
  }

  async function submitTarget() {
    const input = document.getElementById('relay-setup-target-input');
    const inputText = String(input?.value || '').trim();
    generation += 1;
    const requestGeneration = generation;
    stopPolling();
    state.inputText = inputText;
    state.view = null;
    state.balanceE8s = null;
    state.creating = false;
    state.notifyResult = null;
    state.requiredBalanceOverride = null;
    state.error = '';
    state.loading = true;
    try {
      state.targets = parseRelayTargetSet(inputText);
      render();
      await refresh({ expectedInput: inputText, requestGeneration });
    } catch (error) {
      if (!inputStillCurrent(inputText, requestGeneration)) return;
      state.targets = [];
      state.error = normalizeError(error);
      state.loading = false;
      input?.focus?.();
      render();
    }
  }

  async function createRelay() {
    if (!state.targets.length || state.creating) return;
    const expectedInput = state.inputText;
    const requestGeneration = generation;
    if (state.requiredBalanceOverride !== null
      && state.balanceE8s !== null
      && state.balanceE8s >= state.requiredBalanceOverride) {
      state.requiredBalanceOverride = null;
    }
    state.creating = true;
    state.notifyResult = null;
    render();
    try {
      const { historian } = await historianBundle();
      const result = await historian.notify_relay_setup({ target_canister_ids: state.targets });
      if (!inputStillCurrent(expectedInput, requestGeneration)) return;
      state.notifyResult = result;
      state.creating = false;
      const notifyKind = variantName(result);
      if (notifyKind === 'BelowMinimum' || notifyKind === 'BelowCurrentRequirement') {
        state.requiredBalanceOverride = BigInt(result[notifyKind].required_e8s);
      } else if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') {
        state.requiredBalanceOverride = null;
      }
      await refresh({ expectedInput, requestGeneration });
      if (!inputStillCurrent(expectedInput, requestGeneration)) return;
      if (notifyKind === 'Active' || notifyKind === 'ManualRecoveryRequired') stopPolling();
    } catch (error) {
      if (!inputStillCurrent(expectedInput, requestGeneration)) return;
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
    const input = document.getElementById('relay-setup-target-input');
    if (input && input.dataset.bound !== 'true') {
      input.dataset.bound = 'true';
      input.addEventListener('input', () => {
        if (String(input.value || '').trim() === state.inputText) return;
        generation += 1;
        stopPolling();
        state.targets = [];
        state.view = null;
        state.balanceE8s = null;
        state.notifyResult = null;
        state.requiredBalanceOverride = null;
        state.error = '';
        state.loading = false;
        state.creating = false;
        render();
      });
    }
    const form = document.getElementById('relay-setup-form');
    if (form && form.dataset.bound !== 'true') {
      form.dataset.bound = 'true';
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        void submitTarget();
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

  return { state, bindPane, submitTarget, createRelay, refresh, stopPolling, teardown: stopPolling, render };
}
