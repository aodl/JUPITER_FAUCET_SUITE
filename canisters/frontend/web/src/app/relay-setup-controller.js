import { Principal } from '@icp-sdk/core/principal';
import { createActor as createLedgerActor } from '../../declarations/icp_ledger/index.js';
import { createHistorianClient, normalizeError } from './agent.js';
import { accountIdentifierHex, bytesToHex, readOptional } from '../data/dashboard-transforms.js';
import { escapeHtml } from '../followee-links.js';
import { DASH, formatIcpE8s, renderCanisterDashboardLink, renderCanisterTrackerLink } from './view-formatters.js';

const REFUND_ELIGIBLE_STATUSES = new Set([
  'BelowMinimum',
  'TargetNotObservable',
  'RefundAvailable',
  'FailedRetryable',
  'FailedTerminal',
]);

const TERMINAL_POLL_STATUSES = new Set([
  'Active',
  'TargetNotObservable',
  'FailedTerminal',
]);

const DEFAULT_POLL_INTERVAL_MS = 12_000;

function variantName(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return '';
  return Object.keys(value)[0] || '';
}

function statusText(value) {
  if (!value) return '';
  if (typeof value === 'string') return value;
  return variantName(value) || String(value);
}

function principalText(value) {
  const resolved = readOptional(value);
  if (!resolved) return '';
  return typeof resolved.toText === 'function' ? resolved.toText() : String(resolved);
}

function statusFromNotifyResult(result) {
  const kind = variantName(result);
  if (kind === 'TargetNotObservable') return 'TargetNotObservable';
  if (kind === 'BelowMinimum') return 'BelowMinimum';
  if (kind === 'InsufficientForCurrentRate') return 'InsufficientForCurrentRate';
  if (kind === 'Active') return 'Active';
  if (kind === 'SweptToExistingRelay') return 'SweptToExistingRelay';
  if (kind === 'SweepBelowDust') return 'SweepBelowDust';
  if (kind === 'Pending') return statusText(result.Pending?.job?.status) || 'Pending';
  if (kind === 'Failed') return statusText(result.Failed?.status) || 'Failed';
  return kind || '';
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

function renderRelayEntry(entry) {
  if (!entry) return '';
  const relayId = principalText(entry.relay_canister_id);
  const targetId = principalText(entry.target_canister_id);
  const kind = statusText(entry.kind) || 'Relay';
  const status = statusText(entry.status) || 'Unknown';
  return `
    <dl class="pane-detail-grid relay-setup-grid">
      <div><dt>Relay</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(relayId)}</dd></div>
      <div><dt>Dashboard</dt><dd class="pane-detail-value">${renderCanisterDashboardLink(relayId)}</dd></div>
      <div><dt>Target</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(targetId)}</dd></div>
      <div><dt>Registry kind</dt><dd class="pane-detail-value">${escapeHtml(kind)}</dd></div>
      <div><dt>Status</dt><dd class="pane-detail-value">${escapeHtml(status)}</dd></div>
    </dl>`;
}

function renderView({ view, balanceE8s = null, notifyResult = null }) {
  const existingRelay = readOptional(view?.existing_relay) || relayFromNotifyResult(notifyResult);
  const setupAccount = view?.setup_account;
  const subaccount = setupAccount?.subaccount?.[0] || [];
  const subaccountHex = bytesToHex(subaccount);
  const accountIdentifier = view?.setup_account_identifier || (setupAccount ? accountIdentifierHex(setupAccount) : '');
  const currentStatus = statusText(readOptional(view?.current_status)) || statusFromNotifyResult(notifyResult);
  const notifyStatus = statusFromNotifyResult(notifyResult);
  const status = notifyStatus || currentStatus || (existingRelay ? 'Active' : 'Not funded');
  const minimum = view?.minimum_e8s === undefined ? null : BigInt(view.minimum_e8s);
  const factoryEnabled = Boolean(view?.factory_enabled);

  setText('relay-setup-status', status);
  setText('relay-setup-minimum', minimum === null ? DASH : formatIcpE8s(minimum));
  setText('relay-setup-balance', balanceE8s === null || balanceE8s === undefined ? DASH : formatIcpE8s(balanceE8s));
  setText('relay-setup-subaccount', subaccountHex || DASH);
  setText('relay-setup-account-identifier', accountIdentifier || DASH);
  setText('relay-setup-warning', readOptional(view?.warning_text) || '');
  setText('relay-setup-factory', factoryEnabled ? 'Available' : 'Unavailable');
  setHtml('relay-setup-existing-relay', existingRelay ? renderRelayEntry(existingRelay) : '');
  setHidden('relay-setup-existing-relay', !existingRelay);

  const refundEligible = REFUND_ELIGIBLE_STATUSES.has(status);
  setHidden('relay-setup-refund', !refundEligible);
  setHidden('relay-setup-payment-details', Boolean(existingRelay));
}

export function createRelaySetupController({
  frontendConfig = {},
  isLocalHost = () => false,
  createHistorian = createHistorianClient,
  ledgerActorFactory = createLedgerActor,
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
    error: '',
    loaded: false,
    notifying: false,
    refunding: false,
    polling: false,
  };
  let pollHandle = null;
  let pollTargetText = '';

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
      setText('relay-setup-status', state.error);
      setHtml('relay-setup-existing-relay', '');
      setHidden('relay-setup-existing-relay', true);
      return;
    }
    if (!state.loaded) {
      setText('relay-setup-status', '');
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
      || statusText(readOptional(state.view?.current_status));
    return TERMINAL_POLL_STATUSES.has(status);
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
      const dust = BigInt(view.dust_e8s || 0);
      if (state.balanceE8s > dust && !state.notifying) {
        state.notifying = true;
        render();
        state.notifyResult = await historian.notify_relay_setup(state.target);
        state.notifying = false;
        state.view = await historian.get_relay_setup_view({ target_canister_id: state.target });
      }
      render();
      if (shouldStopForStatus()) {
        stopPolling();
      }
    } catch (error) {
      state.notifying = false;
      state.error = normalizeError(error);
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
    state.error = '';
    state.targetText = targetText;
    state.target = null;
    state.view = null;
    state.balanceE8s = null;
    state.notifyResult = null;
    state.loaded = false;
    stopPolling();
    render();

    let target;
    try {
      target = Principal.fromText(targetText);
    } catch {
      state.error = 'Enter a valid target canister ID.';
      input?.focus?.();
      render();
      return;
    }

    try {
      setText('relay-setup-status', 'Loading setup account...');
      const { agent, historian } = await historianBundle();
      const view = await historian.get_relay_setup_view({ target_canister_id: target });
      const ledger = await loadLedger({ agent, historian });
      const balance = await ledger.icrc1_balance_of(view.setup_account);
      state.target = target;
      state.view = view;
      state.balanceE8s = BigInt(balance || 0);
      state.loaded = true;

      const dust = BigInt(view.dust_e8s || 0);
      if (state.balanceE8s > dust) {
        state.notifying = true;
        render();
        state.notifyResult = await historian.notify_relay_setup(target);
        state.notifying = false;
        state.view = await historian.get_relay_setup_view({ target_canister_id: target });
      }
      render();
      startPolling();
    } catch (error) {
      state.notifying = false;
      state.error = normalizeError(error);
      stopPolling();
      render();
    }
  }

  async function requestRefund() {
    if (!state.target) return;
    try {
      state.refunding = true;
      setText('relay-setup-status', 'Requesting refund...');
      const { historian } = await historianBundle();
      const result = await historian.request_relay_setup_refund(state.target);
      const kind = variantName(result);
      state.notifyResult = kind ? { Failed: { status: kind, message: kind } } : state.notifyResult;
      state.view = await historian.get_relay_setup_view({ target_canister_id: state.target });
      state.refunding = false;
      setText('relay-setup-status', kind || 'Refund request complete');
      render();
      if (shouldStopForStatus()) {
        stopPolling();
      }
    } catch (error) {
      state.refunding = false;
      state.error = normalizeError(error);
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
    const refund = document.getElementById('relay-setup-refund');
    if (refund && refund.dataset.bound !== 'true') {
      refund.dataset.bound = 'true';
      refund.addEventListener('click', () => {
        void requestRefund();
      });
    }
  }

  return {
    state,
    bindPane,
    submitTarget,
    requestRefund,
    stopPolling,
    refreshBalanceAndMaybeNotify,
    render,
  };
}
