import { Principal } from '@icp-sdk/core/principal';
import { createActor as createLedgerActor } from '../../declarations/icp_ledger/index.js';
import { createHistorianClient, normalizeError } from './agent.js';
import { accountIdentifierHex, bytesToHex, readOptional } from '../data/dashboard-transforms.js';
import { escapeHtml } from '../followee-links.js';
import { DASH, formatIcpE8s, renderCanisterDashboardLink, renderCanisterTrackerLink } from './view-formatters.js';

const TERMINAL_POLL_STATUSES = new Set([
  'Active',
  'Refunded',
  'ManualRecoveryRequired',
  'TargetNotObservable',
]);

const DEFAULT_POLL_INTERVAL_MS = 12_000;
const FRONTEND_NOTIFY_MIN_E8S = 10_000n;
const RELAY_SETUP_PROMPT_TEXT = 'Enter a canister ID to check whether Jupiter can create a relay for it. This creates a relay, not an emergency top-up; if the canister is close to freezing, top it up directly first.';
const STATUS_LABELS = {
  NotFunded: 'Not funded',
  BelowMinimum: 'Below minimum',
  InsufficientForCurrentRate: 'Below current requirement',
  TargetNotObservable: 'Target not observable',
  PaymentNotAllowed: 'Payment not allowed',
  IndexNotReady: 'Index not ready',
  CreatingRelay: 'Creating relay',
  SweepingToExistingRelay: 'Sweeping to existing relay',
  RefundPending: 'Refund pending',
  FailedRetryable: 'Retryable failure',
  ManualRecoveryRequired: 'Manual recovery required',
};
const RECOVERY_STATUSES = new Set([
  'BelowMinimum',
  'InsufficientForCurrentRate',
  'TargetNotObservable',
  'FailedRetryable',
  'ManualRecoveryRequired',
  'Ambiguous',
  'Failed',
  'RefundPending',
  'Refunded',
  'Refunding',
]);
const PAYMENT_ACTIONABLE_RECOVERY_STATUSES = new Set([
  'BelowMinimum',
  'InsufficientForCurrentRate',
]);
const BASE32_ALPHABET = 'abcdefghijklmnopqrstuvwxyz234567';

function variantName(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return '';
  return Object.keys(value)[0] || '';
}

function statusText(value) {
  if (!value) return '';
  if (typeof value === 'string') return STATUS_LABELS[value] || value;
  const variant = variantName(value);
  return STATUS_LABELS[variant] || variant || String(value);
}

function statusKey(value) {
  if (!value) return '';
  if (typeof value === 'string') return value;
  return variantName(value);
}

function principalText(value) {
  const resolved = readOptional(value);
  if (!resolved) return '';
  return typeof resolved.toText === 'function' ? resolved.toText() : String(resolved);
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
  const size = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(size);
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
  if (bits > 0) {
    out += BASE32_ALPHABET[(value << (5 - bits)) & 31];
  }
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

function statusFromNotifyResult(result) {
  const kind = variantName(result);
  if (kind === 'TargetNotObservable') return 'TargetNotObservable';
  if (kind === 'BelowMinimum') return 'BelowMinimum';
  if (kind === 'InsufficientForCurrentRate') return 'InsufficientForCurrentRate';
  if (kind === 'Active') return 'Active';
  if (kind === 'SweptToExistingRelay') return 'SweptToExistingRelay';
  if (kind === 'SweepBelowDust') return 'SweepBelowDust';
  if (kind === 'Refunded') return 'Refunded';
  if (kind === 'RefundPending') return 'RefundPending';
  if (kind === 'Pending') return statusKey(result.Pending?.status) || 'Pending';
  if (kind === 'Failed') return statusKey(result.Failed?.status) || 'Failed';
  return kind || '';
}

function messageFromNotifyResult(result) {
  const kind = variantName(result);
  if (kind === 'Failed') return result.Failed?.message || '';
  if (kind === 'RefundPending') return result.RefundPending?.reason || '';
  if (kind === 'TargetNotObservable') return result.TargetNotObservable?.message || '';
  if (kind === 'BelowMinimum') {
    return `Current balance ${formatIcpE8s(result.BelowMinimum.current_balance_e8s)} is below required ${formatIcpE8s(result.BelowMinimum.minimum_e8s)}.`;
  }
  if (kind === 'InsufficientForCurrentRate') {
    return `Current balance ${formatIcpE8s(result.InsufficientForCurrentRate.current_balance_e8s)} is below current required ${formatIcpE8s(result.InsufficientForCurrentRate.required_e8s)}.`;
  }
  return '';
}

function relayFromNotifyResult(result) {
  const kind = variantName(result);
  if (kind === 'Active') return result.Active?.relay || null;
  if (kind === 'SweptToExistingRelay') return result.SweptToExistingRelay?.relay || null;
  if (kind === 'SweepBelowDust') return result.SweepBelowDust?.relay || null;
  return null;
}

function setText(id, value) {
  const node = document.getElementById(id);
  if (node) node.textContent = value || '';
}

function setHtml(id, value) {
  const node = document.getElementById(id);
  if (node) node.innerHTML = value || '';
}

function setHidden(id, hidden) {
  const node = document.getElementById(id);
  if (node) node.hidden = hidden;
}

function setAccountLink(id, { dashboardAccount = '', title = '' } = {}) {
  const node = document.getElementById(id);
  if (!node) return;
  if (!dashboardAccount) {
    node.removeAttribute?.('href');
    node.title = '';
    return;
  }
  node.href = `https://dashboard.internetcomputer.org/account/${dashboardAccount}`;
  node.title = title || dashboardAccount;
}

function renderRelaySetupPrompt(message = RELAY_SETUP_PROMPT_TEXT) {
  const result = document.getElementById('relay-setup-result');
  if (!result) return;
  setText('relay-setup-prompt-text', message);
  result.hidden = false;
}

function clearRelaySetupPrompt() {
  const result = document.getElementById('relay-setup-result');
  if (!result) return;
  result.hidden = true;
}

function clearRenderedView(status = '', { promptText = null } = {}) {
  setHidden('relay-setup-summary', !status);
  if (promptText !== null) {
    renderRelaySetupPrompt(promptText);
  } else if (status) {
    clearRelaySetupPrompt();
  } else {
    renderRelaySetupPrompt();
  }
  setText('relay-setup-status', status);
  setText('relay-setup-status-label', DASH);
  setText('relay-setup-minimum', DASH);
  setText('relay-setup-balance', DASH);
  setText('relay-setup-icrc-account', DASH);
  setText('relay-setup-account-identifier', DASH);
  setAccountLink('relay-setup-icrc-account-link');
  setAccountLink('relay-setup-account-identifier-link');
  setText('relay-setup-warning', '');
  setHidden('relay-setup-warning', true);
  setText('relay-setup-factory', DASH);
  setHtml('relay-setup-existing-relay', '');
  setHidden('relay-setup-existing-relay', true);
  setHtml('relay-setup-recovery-details', '');
  setHidden('relay-setup-recovery-details', true);
  setHidden('relay-setup-refund', true);
  setHidden('relay-setup-payment-details', true);
}

function formatOptionalIcp(value) {
  const resolved = readOptional(value);
  return resolved === null || resolved === undefined ? DASH : formatIcpE8s(resolved);
}

function formatOptionalCycles(value) {
  const resolved = readOptional(value);
  return resolved === null || resolved === undefined ? DASH : String(resolved);
}

function formatTransfer(record) {
  const resolved = readOptional(record);
  if (!resolved) return DASH;
  const block = readOptional(resolved.block_index);
  return `${formatIcpE8s(resolved.amount_e8s)} / block ${block ?? DASH} / completed ${Boolean(resolved.completed)}`;
}

function formatConversion(record, fallbackAmountE8s) {
  const resolved = readOptional(record);
  if (resolved) {
    const amount = formatIcpE8s(resolved.amount_e8s);
    const block = readOptional(resolved.block_index);
    return block === null || block === undefined ? amount : `${amount}, block ${block}`;
  }
  return formatOptionalIcp(fallbackAmountE8s);
}

function recoveryDiagnosticPayload(recoveryView) {
  if (!recoveryView) return null;
  const createAttempt = readOptional(recoveryView.relay_create_attempt);
  const rawRelayHash = readOptional(createAttempt?.raw_relay_wasm_hash_hex)
    || readOptional(recoveryView.relay_raw_wasm_hash_hex);
  const installPayloadHash = readOptional(createAttempt?.install_payload_hash_hex)
    || readOptional(recoveryView.relay_install_payload_hash_hex);
  return {
    target_canister_id: principalText(recoveryView.target_canister_id),
    status: statusText(recoveryView.status),
    last_error: readOptional(recoveryView.last_error),
    relay_canister_id: principalText(recoveryView.relay_canister_id) || null,
    cycle_conversion_e8s: readOptional(recoveryView.cycle_conversion_e8s)?.toString?.() || null,
    cycles_minted: readOptional(recoveryView.cycles_minted)?.toString?.() || null,
    relay_create_attach_cycles: createAttempt?.create_attach_cycles?.toString?.() || createAttempt?.initial_cycles?.toString?.() || null,
    configured_relay_create_attach_cycles: recoveryView.configured_relay_create_attach_cycles?.toString?.() || null,
    raw_relay_wasm_hash_hex: rawRelayHash,
    relay_install_payload_hash_hex: installPayloadHash,
    relay_onchain_module_hash_hex: readOptional(recoveryView.relay_onchain_module_hash_hex),
    relay_funding_e8s: readOptional(recoveryView.relay_funding_transfer)?.amount_e8s?.toString?.() || null,
    setup_amount_seen_e8s: recoveryView.setup_amount_seen_e8s?.toString?.() || null,
    setup_amount_processed_e8s: recoveryView.setup_amount_processed_e8s?.toString?.() || null,
    refund_transfer_count: recoveryView.refund_transfer_count?.toString?.() || '0',
  };
}

function renderRecoveryDetails(recoveryView) {
  if (!recoveryView) return '';
  const relayId = principalText(recoveryView.relay_canister_id) || DASH;
  const createAttempt = readOptional(recoveryView.relay_create_attempt);
  const relayRow = relayId === DASH
    ? ''
    : `<div><dt>Relay</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(relayId)}</dd></div>`;
  return `
    <dl class="pane-detail-grid relay-setup-grid">
      ${relayRow}
      <div><dt>CMC conversion</dt><dd class="pane-detail-value">${escapeHtml(formatConversion(recoveryView.cycle_transfer, recoveryView.cycle_conversion_e8s))}</dd></div>
      <div><dt>Cycles minted</dt><dd class="pane-detail-value">${escapeHtml(formatOptionalCycles(recoveryView.cycles_minted))}</dd></div>
      <div><dt>Cycles required for relay</dt><dd class="pane-detail-value">${escapeHtml(formatOptionalCycles(recoveryView.configured_relay_create_attach_cycles))}</dd></div>
      <div><dt>Cycles attached to create call</dt><dd class="pane-detail-value">${escapeHtml(formatOptionalCycles(createAttempt?.create_attach_cycles ?? createAttempt?.initial_cycles))}</dd></div>
      <div><dt>Relay funding</dt><dd class="pane-detail-value">${escapeHtml(formatTransfer(recoveryView.relay_funding_transfer))}</dd></div>
      <div><dt>Total received</dt><dd class="pane-detail-value">${escapeHtml(formatOptionalIcp(recoveryView.setup_amount_seen_e8s))}</dd></div>
      <div><dt>Amount converted</dt><dd class="pane-detail-value">${escapeHtml(formatOptionalIcp(recoveryView.setup_amount_processed_e8s))}</dd></div>
      <div><dt>Refund transfers</dt><dd class="pane-detail-value">${escapeHtml(String(recoveryView.refund_transfer_count ?? 0))}</dd></div>
    </dl>
    <div class="pane-inline-block">
      <div class="pane-inline-header"><strong>Operator diagnostic</strong><button class="pane-page-button" id="relay-setup-copy-diagnostic" type="button">Copy details</button></div>
    </div>`;
}

function renderRelayEntry(entry) {
  if (!entry) return '';
  const relayId = principalText(entry.relay_canister_id);
  const targetId = principalText(entry.target_canister_id);
  const kind = statusText(entry.kind) || 'Relay';
  return `
    <dl class="pane-detail-grid relay-setup-grid">
      <div><dt>Relay</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(relayId)}</dd></div>
      <div><dt>Dashboard</dt><dd class="pane-detail-value">${renderCanisterDashboardLink(relayId)}</dd></div>
      <div><dt>Target</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(targetId)}</dd></div>
      <div><dt>Registry kind</dt><dd class="pane-detail-value">${escapeHtml(kind)}</dd></div>
    </dl>`;
}

function renderView({
  view,
  balanceE8s = null,
  notifyResult = null,
  recoveryView = null,
  notifying = false,
}) {
  clearRelaySetupPrompt();
  setHidden('relay-setup-summary', false);
  const existingRelay = readOptional(view?.existing_relay) || relayFromNotifyResult(notifyResult);
  const setupAccount = view?.setup_account;
  const icrcAccount = setupAccount ? icrcAccountText(setupAccount) : '';
  const accountIdentifier = view?.setup_account_identifier || (setupAccount ? accountIdentifierHex(setupAccount) : '');
  const currentStatus = statusText(view?.status) || statusFromNotifyResult(notifyResult);
  const notifyStatus = statusFromNotifyResult(notifyResult);
  const status = statusText(notifyStatus) || currentStatus || (existingRelay ? 'Active' : 'Not funded');
  const requiredMinimum = readOptional(view?.current_required_e8s) ?? view?.minimum_e8s;
  const minimum = requiredMinimum === undefined ? null : BigInt(requiredMinimum);
  const factoryAvailable = Boolean(view?.factory_available);
  const paymentBlockedReason = readOptional(view?.payment_blocked_reason);
  const effectiveStatus = statusKey(recoveryView?.status) || notifyStatus || statusKey(view?.status);
  const statusRequiresRecovery = RECOVERY_STATUSES.has(effectiveStatus);
  const recoveryAllowsPayment = PAYMENT_ACTIONABLE_RECOVERY_STATUSES.has(effectiveStatus);
  const paymentDetailsHidden = Boolean(existingRelay)
    || view?.payment_allowed === false
    || (statusRequiresRecovery && !recoveryAllowsPayment);

  const recoveryError = readOptional(recoveryView?.last_error);
  const resultMessage = messageFromNotifyResult(notifyResult);
  const displayStatus = notifying
    ? 'Processing payment'
    : recoveryView
      ? (statusText(recoveryView.status) || status)
      : status;
  const notifyingMessage = notifying ? 'Notifying historian…' : '';
  setText('relay-setup-status', displayStatus);
  const pendingRecoveryMessage = statusRequiresRecovery && !recoveryAllowsPayment && !recoveryView && !resultMessage
    ? 'Loading recovery details…'
    : DASH;
  setText('relay-setup-status-label', recoveryError || resultMessage || notifyingMessage || pendingRecoveryMessage);
  setText('relay-setup-minimum', minimum === null ? DASH : formatIcpE8s(minimum));
  setText('relay-setup-balance', balanceE8s === null || balanceE8s === undefined ? DASH : formatIcpE8s(balanceE8s));
  setText('relay-setup-icrc-account', paymentDetailsHidden ? DASH : (icrcAccount || DASH));
  setText('relay-setup-account-identifier', paymentDetailsHidden ? DASH : (accountIdentifier || DASH));
  setAccountLink('relay-setup-icrc-account-link', paymentDetailsHidden ? {} : {
    dashboardAccount: accountIdentifier,
    title: icrcAccount,
  });
  setAccountLink('relay-setup-account-identifier-link', paymentDetailsHidden ? {} : {
    dashboardAccount: accountIdentifier,
    title: accountIdentifier,
  });
  setText('relay-setup-warning', '');
  setHidden('relay-setup-warning', true);
  setText('relay-setup-factory', factoryAvailable ? 'Available' : 'Unavailable');
  setHtml('relay-setup-existing-relay', existingRelay ? renderRelayEntry(existingRelay) : '');
  setHidden('relay-setup-existing-relay', !existingRelay);
  setHtml('relay-setup-recovery-details', recoveryView ? renderRecoveryDetails(recoveryView) : '');
  setHidden('relay-setup-recovery-details', !recoveryView);

  if (paymentBlockedReason && !notifyStatus) {
    setText('relay-setup-status', displayStatus);
    setText('relay-setup-status-label', paymentBlockedReason);
  }
  setHidden('relay-setup-refund', true);
  setHidden('relay-setup-payment-details', notifying || paymentDetailsHidden);
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
    targetText: '',
    target: null,
    view: null,
    balanceE8s: null,
    notifyResult: null,
    recoveryView: null,
    error: '',
    loaded: false,
    loading: false,
    notifying: false,
    polling: false,
  };
  let pollHandle = null;
  let pollTargetText = '';
  let requestGeneration = 0;

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
    if (!ledgerId) throw new Error('Historian did not return an ICP ledger canister ID');
    return ledgerActorFactory(ledgerId, { agent });
  }

  function render() {
    if (state.error) {
      clearRenderedView(state.error);
      return;
    }
    if (state.loading) {
      clearRenderedView('', { promptText: 'Checking relay setup…' });
      return;
    }
    if (!state.loaded) {
      clearRenderedView('');
      return;
    }
    renderView(state);
  }

  function stopPolling() {
    if (pollHandle !== null) {
      clearIntervalFn(pollHandle);
      pollHandle = null;
    }
    pollTargetText = '';
    state.polling = false;
  }

  function shouldStopForStatus() {
    const status = statusFromNotifyResult(state.notifyResult)
      || statusKey(state.view?.status);
    return TERMINAL_POLL_STATUSES.has(status);
  }

  function submittedTargetStillCurrent(targetText) {
    const input = document.getElementById('relay-setup-target-input');
    return state.targetText === targetText && String(input?.value || '').trim() === targetText;
  }

  function shouldFetchRecoveryForCurrentState() {
    const status = statusFromNotifyResult(state.notifyResult) || statusKey(state.view?.status);
    return RECOVERY_STATUSES.has(status) || variantName(state.notifyResult) === 'Failed';
  }

  async function loadRecoveryIfNeeded({ historian, target, targetText, generation }) {
    if (!shouldFetchRecoveryForCurrentState()) {
      state.recoveryView = null;
      return;
    }
    if (typeof historian.get_relay_setup_recovery_view !== 'function') return;
    const recoveryView = await historian.get_relay_setup_recovery_view({ target_canister_id: target });
    if (generation !== requestGeneration || !submittedTargetStillCurrent(targetText)) return;
    if (principalText(recoveryView?.target_canister_id) !== targetText) return;
    state.recoveryView = recoveryView;
  }

  async function refreshBalanceAndMaybeNotify(expectedTargetText) {
    if (!state.target || state.targetText !== expectedTargetText || shouldStopForStatus()) {
      stopPolling();
      return;
    }
    try {
      const { agent, historian } = await historianBundle();
      const view = await historian.get_relay_setup_view({ target_canister_id: state.target });
      if (state.targetText !== expectedTargetText) {
        stopPolling();
        return;
      }
      const ledger = await loadLedger({ agent, historian });
      const balance = await ledger.icrc1_balance_of(view.setup_account);
      if (state.targetText !== expectedTargetText) {
        stopPolling();
        return;
      }
      state.view = view;
      state.balanceE8s = BigInt(balance || 0);
      state.loaded = true;
      state.loading = false;
      if (state.balanceE8s > FRONTEND_NOTIFY_MIN_E8S && !state.notifying) {
        state.notifying = true;
        render();
        state.notifyResult = await historian.notify_relay_setup(state.target);
        state.notifying = false;
        state.view = await historian.get_relay_setup_view({ target_canister_id: state.target });
        await loadRecoveryIfNeeded({
          historian,
          target: state.target,
          targetText: expectedTargetText,
          generation: requestGeneration,
        });
      }
      await loadRecoveryIfNeeded({
        historian,
        target: state.target,
        targetText: expectedTargetText,
        generation: requestGeneration,
      });
      render();
      if (shouldStopForStatus()) {
        stopPolling();
      }
    } catch (error) {
      state.notifying = false;
      state.error = normalizeError(error);
      state.loading = false;
      stopPolling();
      render();
    }
  }

  function startPolling() {
    stopPolling();
    if (!state.target) return;
    pollTargetText = state.targetText;
    state.polling = true;
    pollHandle = setIntervalFn(() => {
      void refreshBalanceAndMaybeNotify(pollTargetText);
    }, pollIntervalMs);
    if (shouldStopForStatus()) {
      stopPolling();
    }
  }

  async function submitTarget() {
    const input = document.getElementById('relay-setup-target-input');
    const targetText = String(input?.value || '').trim();
    requestGeneration += 1;
    const generation = requestGeneration;
    state.error = '';
    state.targetText = targetText;
    state.target = null;
    state.view = null;
    state.balanceE8s = null;
    state.notifyResult = null;
    state.recoveryView = null;
    state.loaded = false;
    state.loading = true;
    state.notifying = false;
    stopPolling();
    render();

    let target;
    try {
      target = Principal.fromText(targetText);
    } catch {
      state.error = 'Enter a valid target canister ID.';
      state.loading = false;
      input?.focus?.();
      render();
      return;
    }

    try {
      const { agent, historian } = await historianBundle();
      if (!submittedTargetStillCurrent(targetText)) return;
      const view = await historian.get_relay_setup_view({ target_canister_id: target });
      if (!submittedTargetStillCurrent(targetText)) return;
      state.target = target;
      state.view = view;
      if (view?.payment_allowed === false) {
        state.loaded = true;
        state.loading = false;
        render();
        return;
      }
      const ledger = await loadLedger({ agent, historian });
      if (!submittedTargetStillCurrent(targetText)) return;
      const balance = await ledger.icrc1_balance_of(view.setup_account);
      if (!submittedTargetStillCurrent(targetText)) return;
      state.view = view;
      state.balanceE8s = BigInt(balance || 0);
      state.loaded = true;
      state.loading = false;

      const viewStatus = statusKey(view?.status);
      const terminalView = TERMINAL_POLL_STATUSES.has(viewStatus);
      if (!terminalView && state.balanceE8s > FRONTEND_NOTIFY_MIN_E8S) {
        state.notifying = true;
        render();
        state.notifyResult = await historian.notify_relay_setup(target);
        if (generation !== requestGeneration || !submittedTargetStillCurrent(targetText)) return;
        state.notifying = false;
        state.view = await historian.get_relay_setup_view({ target_canister_id: target });
        if (generation !== requestGeneration || !submittedTargetStillCurrent(targetText)) return;
      }
      await loadRecoveryIfNeeded({ historian, target, targetText, generation });
      render();
      startPolling();
    } catch (error) {
      if (!submittedTargetStillCurrent(targetText)) return;
      state.notifying = false;
      state.error = normalizeError(error);
      state.loading = false;
      stopPolling();
      render();
    }
  }

  function bindPane() {
    const form = document.getElementById('relay-setup-form');
    if (form && form.dataset.bound !== 'true') {
      form.dataset.bound = 'true';
      form.addEventListener('submit', (event) => {
        event.preventDefault();
        void submitTarget();
      });
    }
    const bindCopyValue = (buttonId, valueId) => {
      const button = document.getElementById(buttonId);
      if (!button || button.dataset.bound === 'true') return;
      button.dataset.bound = 'true';
      button.addEventListener('click', async () => {
        if (typeof copyTextToClipboard !== 'function') return;
        const value = document.getElementById(valueId)?.textContent || '';
        if (!value || value === DASH) return;
        const defaultText = button.textContent || 'Copy';
        try {
          await copyTextToClipboard(value);
          button.textContent = 'Copied';
          window.setTimeout(() => {
            button.textContent = defaultText;
          }, 1200);
        } catch {
          button.textContent = 'Copy failed';
          window.setTimeout(() => {
            button.textContent = defaultText;
          }, 1500);
        }
      });
    };
    bindCopyValue('copy-relay-setup-icrc-account', 'relay-setup-icrc-account');
    bindCopyValue('copy-relay-setup-account-identifier', 'relay-setup-account-identifier');
    const recoveryDetails = document.getElementById('relay-setup-recovery-details');
    if (recoveryDetails && recoveryDetails.dataset.copyBound !== 'true') {
      recoveryDetails.dataset.copyBound = 'true';
      recoveryDetails.addEventListener('click', async (event) => {
        if (event.target?.id !== 'relay-setup-copy-diagnostic') return;
        if (typeof copyTextToClipboard !== 'function') return;
        const value = recoveryDiagnosticPayload(state.recoveryView);
        if (!value) return;
        const button = event.target;
        const defaultText = button.textContent || 'Copy details';
        try {
          await copyTextToClipboard(JSON.stringify(value, null, 2));
          button.textContent = 'Copied';
          window.setTimeout(() => {
            button.textContent = defaultText;
          }, 1200);
        } catch {
          button.textContent = 'Copy failed';
          window.setTimeout(() => {
            button.textContent = defaultText;
          }, 1500);
        }
      });
    }
  }

  return {
    state,
    bindPane,
    submitTarget,
    stopPolling,
    refreshBalanceAndMaybeNotify,
    render,
  };
}
