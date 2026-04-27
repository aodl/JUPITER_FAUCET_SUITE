import { HttpAgent } from '@icp-sdk/core/agent';
import { Principal } from '@icp-sdk/core/principal';
import {
  FRONTEND_HINT,
  normalizeError,
  accountIdentifierHex,
  bytesToHex,
  uint8ArrayFromOptBytes,
  loadDashboardData,
  loadRegisteredCanisterSummaryPage,
  loadCanisterModuleHashes,
  loadTrackerData,
} from './dashboard-data.js';
import { createActor as createGovernanceActor } from '../declarations/nns_governance/index.js';
import { createNeuronDetailsController } from './neuron-details-controller.js';
import { setLink, setPaneValueText, setPaneValueTrustedHtml, setText } from './dom-helpers.js';
import { mergeRegisteredLandingData } from './registered-page-state.js';
import { escapeHtml, formatFolloweeLinks } from './followee-links.js';
import { readOpt } from './candid-opt.js';
import { buildCommitmentIndexFaultBannerText } from './historian-fault.js';

const FRONTEND_CONFIG = __JUPITER_FRONTEND_CONFIG__;
const DASH = '—';
const GOVERNANCE_CANISTER_ID = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
const JUPITER_NEURON_ID = 11614578985374291210n;
const TABLE_PAGE_SIZE = 6;
const JUPITER_STAKING_ACCOUNT_ADDRESS = 'rrkah-fqaaa-aaaaa-aaaaq-cai-h7evq5y.ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68';
const JUPITER_STAKING_ACCOUNT_EXPLORER_ACCOUNT_HEX = '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d';
const JUPITER_STAKING_ACCOUNT_OWNER = GOVERNANCE_CANISTER_ID;
const JUPITER_STAKING_ACCOUNT_SUBACCOUNT_HEX = 'ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68';
const TRACKER_REGISTRATION_URL = 'https://jupiter-faucet.com/#how-it-works';
const BLACKHOLE_CANISTER_ID = 'e3mmv-5qaaa-aaaah-aadma-cai';
const INLINE_TOOLTIP_CONTENT = {
  'blackhole-controller-help': `
    <div class="pane-fixed-tooltip-content">
      <p>Cycles balances are sampled periodically by historian.</p>
      <p>For ordinary canisters, cycles observability requires the canister to expose public status through the <a class="pane-external-link" href="https://github.com/ninegua/ic-blackhole" target="_blank" rel="noopener noreferrer">blackhole controller</a>. Newly registered canisters may show as pending until the next cycles sweep completes.</p>
    </div>`,
};

const SOURCE_PANE_MODULE_HASH_CACHE_TTL_MS = 60 * 60 * 1000;
let sourcePaneModuleHashesLoadedAt = 0;
let sourcePaneModuleHashesRequest = null;

const tableState = {
  registered: {
    page: 0,
    items: [],
    total: 0,
    pageSize: TABLE_PAGE_SIZE,
    loading: false,
    pendingPage: null,
    error: null,
    pages: new Map(),
  },
  commitments: { page: 0, items: [] },
  output: { page: 0, items: [] },
  rewards: { page: 0, items: [] },
};

const trackerState = {
  principalText: '',
  data: null,
  granularity: 'month',
  loading: false,
  error: null,
};


function isLocalHost() {
  const host = window.location.hostname;
  return host === '127.0.0.1' || host === 'localhost';
}

function formatPrincipal(value) {
  return value?.toText ? value.toText() : String(value || '');
}

function renderCanisterTrackerLink(value, { label = null, className = 'pane-canister-tracker-link pane-external-link mono' } = {}) {
  const principalText = formatPrincipal(value).trim();
  if (!principalText) return DASH;
  const display = label === null || label === undefined ? principalText : String(label);
  return `<a href="#metric-tracker" data-tracker-principal="${escapeHtml(principalText)}" class="${escapeHtml(className)}">${escapeHtml(display)}</a>`;
}

function renderCanisterDashboardLink(value, label = 'Open dashboard') {
  const principalText = formatPrincipal(value).trim();
  if (!principalText) return DASH;
  return `<a href="https://dashboard.internetcomputer.org/canister/${escapeHtml(principalText)}" target="_blank" rel="noopener noreferrer" class="pane-external-link mono">${escapeHtml(label)}</a>`;
}

function formatSourceController(value) {
  const principalText = formatPrincipal(value).trim();
  if (!principalText) return '';
  if (principalText === BLACKHOLE_CANISTER_ID) return renderCanisterTrackerLink(principalText, { label: 'blackhole' });
  return renderCanisterTrackerLink(principalText);
}

function formatIcpE8s(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const sign = asBigInt < 0n ? '-' : '';
  const absolute = asBigInt < 0n ? -asBigInt : asBigInt;
  const whole = absolute / 100_000_000n;
  const fraction = (absolute % 100_000_000n).toString().padStart(8, '0').replace(/0+$/, '');
  return fraction ? `${sign}${whole.toString()}.${fraction} ICP` : `${sign}${whole.toString()} ICP`;
}

function formatGroupedBigInt(value) {
  const text = value.toString();
  const sign = text.startsWith('-') ? '-' : '';
  const digits = sign ? text.slice(1) : text;
  return `${sign}${digits.replace(/\B(?=(\d{3})+(?!\d))/g, ',')}`;
}

function formatCycles(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  return `${formatGroupedBigInt(asBigInt)} cycles`;
}

function formatInteger(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  return formatGroupedBigInt(asBigInt);
}

function formatBytes(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let scaled = Number(asBigInt);
  let unitIndex = 0;
  while (scaled >= 1024 && unitIndex < units.length - 1) {
    scaled /= 1024;
    unitIndex += 1;
  }
  const digits = scaled >= 100 || unitIndex === 0 ? 0 : scaled >= 10 ? 1 : 2;
  return `${scaled.toFixed(digits)} ${units[unitIndex]}`;
}

function formatTimestampSeconds(value) {
  if (!value) return DASH;
  return new Date(Number(value) * 1000).toLocaleString('en-GB', {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatTimestampNanos(value) {
  if (!value) return DASH;
  const millis = (typeof value === 'bigint' ? value : BigInt(value)) / 1_000_000n;
  return new Date(Number(millis)).toLocaleString('en-GB', {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatAgeFromSeconds(value) {
  if (!value) return DASH;
  const nowSeconds = Math.floor(Date.now() / 1000);
  const diff = Math.max(0, nowSeconds - Number(value));
  const day = 24 * 60 * 60;
  const year = 365 * day;
  if (diff >= year) {
    const years = Math.floor(diff / year);
    const months = Math.floor((diff % year) / (30 * day));
    return months > 0 ? `${years}y ${months}mo` : `${years}y`;
  }
  if (diff >= 30 * day) return `${Math.floor(diff / (30 * day))}mo`;
  if (diff >= day) return `${Math.floor(diff / day)}d`;
  if (diff >= 60 * 60) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 60)}m`;
}

function numericValue(value, fallback = 0) {
  if (value === null || value === undefined) return fallback;
  return Number(value);
}

function setHidden(id, hidden) {
  const node = document.getElementById(id);
  if (!node) return;
  node.hidden = hidden;
}

function setStatusNote(id, value) {
  const node = document.getElementById(id);
  if (!node) return;
  node.textContent = value || '';
  node.hidden = !value;
}

function setMetricStatus(id, { value = null, loading = false, error = null } = {}) {
  const valueNode = document.getElementById(id);
  const statusNode = document.getElementById(`${id}-status`);
  if (valueNode) valueNode.textContent = value ?? '';
  if (!statusNode) return;
  statusNode.hidden = !loading && !error;
  statusNode.className = 'metric-status';
  statusNode.removeAttribute('title');
  statusNode.removeAttribute('aria-label');
  statusNode.textContent = '';
  if (loading) {
    statusNode.classList.add('metric-status--loading');
    statusNode.setAttribute('aria-label', 'Loading');
    return;
  }
  if (error) {
    statusNode.classList.add('metric-status--error');
    statusNode.textContent = '⚠';
    statusNode.title = error;
    statusNode.setAttribute('aria-label', error);
  }
}

function setMetricLoadingStates() {
  ['landing-current-stake','landing-total-output','landing-total-rewards','landing-registered-canisters','landing-qualifying-commitments'].forEach((id) => {
    setMetricStatus(id, { loading: true });
  });
  setHidden('landing-live-unavailable', true);
}

function nextRunLabel(status) {
  if (!status) return 'Next historian run unavailable.';
  const base = status.last_index_run_ts?.[0] ?? null;
  if (!base) return `Refreshes every ${formatInteger(status.index_interval_seconds)} seconds.`;
  const next = BigInt(base) + BigInt(status.index_interval_seconds);
  return `Next historian run approx. ${formatTimestampSeconds(next)}.`;
}

function renderLandingSummary(data) {
  setMetricStatus('landing-current-stake', data.stakeE8s === null ? { error: data.errors?.stake || 'Stake unavailable' } : { value: formatIcpE8s(data.stakeE8s) });
  setMetricStatus('landing-total-output', data.counts?.total_output_e8s === undefined || data.counts === null ? { error: data.errors?.counts || 'Total output unavailable' } : { value: formatIcpE8s(data.counts.total_output_e8s) });
  setMetricStatus('landing-total-rewards', data.counts?.total_rewards_e8s === undefined || data.counts === null ? { error: data.errors?.counts || 'Total rewards unavailable' } : { value: formatIcpE8s(data.counts.total_rewards_e8s) });
  setMetricStatus('landing-registered-canisters', data.counts?.registered_canister_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Target canisters unavailable' } : { value: formatInteger(data.counts.registered_canister_count) });
  setMetricStatus('landing-qualifying-commitments', data.counts?.qualifying_commitment_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Commitments unavailable' } : { value: formatInteger(data.counts.qualifying_commitment_count) });
  setHidden('landing-live-unavailable', true);
}

function renderLandingUnavailable(errorMessage = 'Live metrics unavailable') {
  setMetricStatus('landing-current-stake', { error: errorMessage });
  setMetricStatus('landing-total-output', { error: errorMessage });
  setMetricStatus('landing-total-rewards', { error: errorMessage });
  setMetricStatus('landing-registered-canisters', { error: errorMessage });
  setMetricStatus('landing-qualifying-commitments', { error: errorMessage });
  renderHistorianFaultBanner(null);
  setHidden('landing-live-unavailable', true);
}

function renderHistorianFaultBanner(data) {
  const banner = document.getElementById('historian-fault-banner');
  if (!banner) return;
  const text = buildCommitmentIndexFaultBannerText(data?.status, {
    formatTimestampSeconds,
    formatInteger,
  });
  if (!text) {
    banner.hidden = true;
    banner.textContent = '';
    return;
  }
  banner.textContent = text;
  banner.hidden = false;
}

function isJupiterStakingAccount(account) {
  if (!account) return true;
  const owner = formatPrincipal(account.owner);
  const subaccountHex = bytesToHex(uint8ArrayFromOptBytes(account.subaccount));
  return owner === JUPITER_STAKING_ACCOUNT_OWNER && subaccountHex === JUPITER_STAKING_ACCOUNT_SUBACCOUNT_HEX;
}

function stakingAccountDisplayAddress(account) {
  if (isJupiterStakingAccount(account)) return JUPITER_STAKING_ACCOUNT_ADDRESS;
  return accountIdentifierHex(account);
}

function stakingAccountExplorerAddress(account) {
  if (isJupiterStakingAccount(account)) return JUPITER_STAKING_ACCOUNT_EXPLORER_ACCOUNT_HEX;
  return accountIdentifierHex(account);
}

function renderHowItWorksAccount() {
  setCopyButton('copy-how-staking-account', () => JUPITER_STAKING_ACCOUNT_ADDRESS);
  setText('how-staking-account-address', JUPITER_STAKING_ACCOUNT_ADDRESS);
  const stakingAccountLink = document.getElementById('how-staking-account-link');
  if (stakingAccountLink) {
    stakingAccountLink.href = `https://dashboard.internetcomputer.org/account/${JUPITER_STAKING_ACCOUNT_EXPLORER_ACCOUNT_HEX}`;
    stakingAccountLink.title = JUPITER_STAKING_ACCOUNT_ADDRESS;
  }
}

function renderStakePane(data, neuron, { neuronLoading = false, neuronError = null, dataLoading = false } = {}) {
  const stakingAccount = data?.status?.staking_account;
  const stakingAddress = stakingAccountDisplayAddress(stakingAccount);
  const stakingExplorerAddress = stakingAccountExplorerAddress(stakingAccount);
  setLink('stake-pane-account-link', {
    href: stakingExplorerAddress ? `https://dashboard.internetcomputer.org/account/${stakingExplorerAddress}` : '',
    text: stakingAddress,
  });
  setLink('stake-neuron-id-link', {
    href: `https://dashboard.internetcomputer.org/neuron/${JUPITER_NEURON_ID.toString()}`,
    text: JUPITER_NEURON_ID.toString(),
  });
  setPaneValueText('stake-pane-balance', dataLoading
    ? { loading: true }
    : data?.stakeE8s === null
      ? { error: data?.errors?.stake || 'Stake unavailable' }
      : { value: formatIcpE8s(data?.stakeE8s) });

  if (neuron) {
    setPaneValueText('stake-neuron-age', { value: formatAgeFromSeconds(neuron.aging_since_timestamp_seconds) });
    setPaneValueText('stake-neuron-public', { value: 'Yes' });
    setPaneValueText('stake-neuron-created', { value: formatTimestampSeconds(neuron.created_timestamp_seconds) });
    setPaneValueText('stake-neuron-refresh', { value: formatTimestampSeconds(neuron.voting_power_refreshed_timestamp_seconds?.[0]) });
    setPaneValueTrustedHtml('stake-neuron-followees', { value: formatFolloweeLinks(neuron) });
    return;
  }

  const fallback = neuronError
    ? { loading: false, error: neuronError }
    : neuronLoading || !neuronState.loaded
      ? { loading: true }
      : { error: 'Public neuron details unavailable' };
  setPaneValueText('stake-neuron-age', fallback);
  setPaneValueText('stake-neuron-public', fallback);
  setPaneValueText('stake-neuron-created', fallback);
  setPaneValueText('stake-neuron-refresh', fallback);
  setPaneValueText('stake-neuron-followees', fallback);
}

function renderStakeNeuronStatus({ loading = false, error = null } = {}) {
  if (loading || error) {
    setStatusNote('stake-neuron-note', '');
    return;
  }
  setStatusNote('stake-neuron-note', '');
}

function renderPaneSubtitles(data) {
  const subtitle = nextRunLabel(data?.status);
  setText('registered-pane-subtitle', subtitle);
  setText('commitments-pane-subtitle', subtitle);
  setText('output-pane-subtitle', 'Historian tracks the aggregate; recent rows are queried live from the ICP index canister.');
  setText('rewards-pane-subtitle', 'Historian tracks the aggregate; recent rows are queried live from the ICP index canister.');

  const totalMemory = data?.status?.total_memory_bytes?.[0];
  const heapMemory = data?.status?.heap_memory_bytes?.[0];
  const stableMemory = data?.status?.stable_memory_bytes?.[0];
  const memoryNote = totalMemory === undefined || totalMemory === null
    ? ''
    : `Historian allocated memory: ${formatBytes(totalMemory)}`
      + (heapMemory !== undefined && stableMemory !== undefined
        ? ` (${formatBytes(heapMemory)} heap + ${formatBytes(stableMemory)} stable)`
        : '');
  setStatusNote('commitments-pane-memory-note', memoryNote);
}

function paneEmptyMessage(data, key, defaultText) {
  return data?.errors?.[key] ? `${defaultText} (${data.errors[key]})` : defaultText;
}

function setCopyButton(id, valueProvider) {
  const button = document.getElementById(id);
  if (!button || button.dataset.bound === 'true') return;
  button.dataset.bound = 'true';
  button.addEventListener('click', async () => {
    const value = valueProvider();
    if (!value || value === DASH) return;
    try {
      await navigator.clipboard.writeText(value);
      button.textContent = 'Copied';
      window.setTimeout(() => {
        button.textContent = 'Copy';
      }, 1200);
    } catch {
      button.textContent = 'Copy failed';
      window.setTimeout(() => {
        button.textContent = 'Copy';
      }, 1500);
    }
  });
}

function paginate(kind, items, renderRow, emptyMessage, colspan) {
  const state = tableState[kind];
  state.items = items || [];
  const totalPages = Math.max(1, Math.ceil(state.items.length / TABLE_PAGE_SIZE));
  if (state.page >= totalPages) state.page = totalPages - 1;
  const start = state.page * TABLE_PAGE_SIZE;
  const pageItems = state.items.slice(start, start + TABLE_PAGE_SIZE);
  const body = document.getElementById(`${kind}-pane-body`) || document.getElementById(`${kind}-transactions-body`);
  if (body) {
    body.innerHTML = pageItems.length
      ? pageItems.map(renderRow).join('')
      : `<tr><td colspan="${colspan}" class="empty-cell">${escapeHtml(emptyMessage)}</td></tr>`;
  }
  setText(`${kind}-page-info`, `Page ${state.page + 1} of ${totalPages}`);
  const prev = document.getElementById(`${kind}-prev-page`);
  const next = document.getElementById(`${kind}-next-page`);
  if (prev) prev.disabled = state.page === 0;
  if (next) next.disabled = state.page >= totalPages - 1;
}

function registeredPageCacheKey(page, pageSize) {
  return `${page}:${pageSize}`;
}

function cacheRegisteredPage(response) {
  if (!response) return;
  const state = tableState.registered;
  const page = numericValue(response.page, 0);
  const pageSize = Math.max(1, numericValue(response.page_size, TABLE_PAGE_SIZE));
  const items = Array.isArray(response.items) ? response.items : [];
  state.pages.set(registeredPageCacheKey(page, pageSize), response);
  state.page = page;
  state.pageSize = pageSize;
  state.total = numericValue(response.total, items.length);
  state.items = items;
  state.error = null;
}

function registeredTotalPages() {
  const state = tableState.registered;
  return Math.max(1, Math.ceil(state.total / Math.max(1, state.pageSize || TABLE_PAGE_SIZE)));
}

async function fetchRegisteredPage(page) {
  const state = tableState.registered;
  if (state.loading) return;
  const pageSize = Math.max(1, state.pageSize || TABLE_PAGE_SIZE);
  const cacheKey = registeredPageCacheKey(page, pageSize);
  const cached = state.pages.get(cacheKey);
  if (cached) {
    cacheRegisteredPage(cached);
    const current = window.__JUPITER_LANDING_DATA__ || {};
    window.__JUPITER_LANDING_DATA__ = mergeRegisteredLandingData(current, {
      registered: cached,
      registeredError: null,
    });
    renderRegisteredPane(window.__JUPITER_LANDING_DATA__ || null);
    return;
  }

  state.loading = true;
  state.pendingPage = page;
  state.error = null;
  renderRegisteredPane(window.__JUPITER_LANDING_DATA__ || null);
  try {
    const response = await loadRegisteredCanisterSummaryPage({
      historianCanisterId: FRONTEND_CONFIG?.historianCanisterId,
      host: window.location.origin,
      local: isLocalHost(),
      page,
      pageSize,
    });
    cacheRegisteredPage(response);
    const current = window.__JUPITER_LANDING_DATA__ || {};
    window.__JUPITER_LANDING_DATA__ = mergeRegisteredLandingData(current, {
      registered: response,
      registeredError: null,
    });
  } catch (error) {
    state.error = normalizeError(error);
    console.warn('Registered canister page failed', error);
    const current = window.__JUPITER_LANDING_DATA__ || {};
    window.__JUPITER_LANDING_DATA__ = mergeRegisteredLandingData(current, {
      registeredError: state.error,
    });
  } finally {
    state.loading = false;
    state.pendingPage = null;
    renderRegisteredPane(window.__JUPITER_LANDING_DATA__ || null);
  }
}


function renderCyclesHelpIcon() {
  return `
    <button
      class="pane-inline-tooltip-icon"
      type="button"
      data-tooltip-id="blackhole-controller-help"
      aria-label="Cycles observability help"
    >i</button>`;
}

function renderCyclesStatusCell(label = 'unavailable') {
  return `
    <span class="pane-inline-tooltip-fallback">
      <span>${escapeHtml(label)}</span>
      ${renderCyclesHelpIcon()}
    </span>`;
}

function variantNameFromValue(value) {
  if (!value || Array.isArray(value) || typeof value !== 'object') return '';
  return Object.keys(value)[0] || '';
}

function cyclesProbeStatusInfo(data) {
  const meta = data?.overview?.meta || {};
  const probeResult = optValue(meta.last_cycles_probe_result);
  const probeTs = optValue(meta.last_cycles_probe_ts);

  if (!probeTs && !probeResult) {
    return {
      label: 'pending first sweep',
      chartMessage: 'Cycles data pending first historian sweep.',
      note: 'Cycles data is pending: historian has registered this canister, but the first cycles sweep has not recorded a balance yet.',
    };
  }

  const resultName = variantNameFromValue(probeResult);
  if (resultName === 'Error') {
    const errorText = typeof probeResult.Error === 'string' ? probeResult.Error : 'unknown error';
    const when = formatTimestampSeconds(probeTs);
    return {
      label: 'probe failed',
      chartMessage: 'Cycles probe failed.',
      note: `Cycles data is unavailable because the last historian cycles probe failed${when !== DASH ? ` at ${when}` : ''}: ${errorText}`,
    };
  }

  if (resultName === 'NotAvailable') {
    const when = formatTimestampSeconds(probeTs);
    return {
      label: 'not available',
      chartMessage: 'Cycles data not available.',
      note: `Cycles data is unavailable because historian could not obtain a balance${when !== DASH ? ` during the last probe at ${when}` : ''}. Ordinary canisters must expose public status through the blackhole controller for cycles observability.`,
    };
  }

  const when = formatTimestampSeconds(probeTs);
  return {
    label: 'pending update',
    chartMessage: 'Cycles data pending update.',
    note: `Cycles data has not been recorded for this tracker view yet${when !== DASH ? `; the last probe was at ${when}` : ''}.`,
  };
}

function formatCommitmentTarget(item) {
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  if (canister) return renderCanisterTrackerLink(canister);
  return 'invalid target canister memo';
}

function commitmentOutcomeCategory(item) {
  const category = item?.outcome_category;
  if (category && !Array.isArray(category)) {
    if ('QualifyingCommitment' in category) return 'QualifyingCommitment';
    if ('UnderThresholdCommitment' in category) return 'UnderThresholdCommitment';
    if ('InvalidTargetMemo' in category) return 'InvalidTargetMemo';
  }
  if (item?.counts_toward_faucet) return 'QualifyingCommitment';
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  return canister ? 'UnderThresholdCommitment' : 'InvalidTargetMemo';
}

function formatCommitmentOutcome(item) {
  switch (commitmentOutcomeCategory(item)) {
    case 'QualifyingCommitment':
      return 'Qualifying';
    case 'UnderThresholdCommitment':
      return 'Under threshold';
    case 'InvalidTargetMemo':
      return 'Invalid target';
    default:
      return item?.counts_toward_faucet ? 'Qualifying' : 'Non-qualifying';
  }
}

function transactionHref(item) {
  const txIndex = item?.tx_id;
  if (txIndex !== undefined && txIndex !== null) {
    return `https://dashboard.internetcomputer.org/transaction/${encodeURIComponent(String(txIndex))}`;
  }
  return '';
}

function renderCommitmentTimestampCell(item) {
  const label = escapeHtml(formatTimestampNanos(item.timestamp_nanos?.[0]));
  const href = transactionHref(item);
  if (!href) return label;
  return `<a class="pane-external-link" href="${escapeHtml(href)}" target="_blank" rel="noopener noreferrer">${label}</a>`;
}


let inlineTooltipPopover = null;

function ensureInlineTooltipPopover() {
  if (inlineTooltipPopover) return inlineTooltipPopover;
  const popover = document.createElement('div');
  popover.className = 'pane-fixed-tooltip';
  popover.hidden = true;
  popover.innerHTML = `
    <div class="pane-fixed-tooltip-card" role="status" aria-live="polite">
      <div class="pane-fixed-tooltip-text" id="pane-fixed-tooltip-text"></div>
      <button class="pane-fixed-tooltip-close" type="button" aria-label="Close help">Close</button>
    </div>`;
  document.body.appendChild(popover);
  popover.addEventListener('pointerdown', (event) => {
    event.stopPropagation();
  });
  popover.addEventListener('click', (event) => {
    event.stopPropagation();
  });
  popover.querySelector('.pane-fixed-tooltip-close')?.addEventListener('click', (event) => {
    event.preventDefault();
    event.stopPropagation();
    popover.hidden = true;
  });
  inlineTooltipPopover = popover;
  return popover;
}

function showInlineTooltipPopover(content, { trustedHtml = false } = {}) {
  const popover = ensureInlineTooltipPopover();
  const textNode = popover.querySelector('#pane-fixed-tooltip-text');
  if (textNode) {
    if (trustedHtml) {
      textNode.innerHTML = content;
    } else {
      textNode.textContent = content;
    }
  }
  popover.hidden = false;
}


function sourcePaneModuleHashNodes() {
  return Array.from(document.querySelectorAll('[data-source-module-hash]'));
}

function sourcePaneControllerNodes() {
  return Array.from(document.querySelectorAll('[data-source-controllers]'));
}

function sourcePaneCanisterInfoNodes() {
  return [...sourcePaneModuleHashNodes(), ...sourcePaneControllerNodes()];
}

function sourcePaneModuleHashCacheKey() {
  if (!FRONTEND_CONFIG?.historianCanisterId) return null;
  return `jupiter-faucet:source-pane-canister-info:v3:${FRONTEND_CONFIG.historianCanisterId}`;
}

function normalizeSourcePaneInfo(infoByCanisterId, canisterId) {
  const entry = infoByCanisterId?.[canisterId];
  if (!entry) return { moduleHash: null, controllers: null };
  if (typeof entry === 'string') return { moduleHash: entry || null, controllers: null };
  if (typeof entry !== 'object' || Array.isArray(entry)) return { moduleHash: null, controllers: null };
  return {
    moduleHash: entry.moduleHash || entry.module_hash_hex || null,
    controllers: Array.isArray(entry.controllers) ? entry.controllers : null,
  };
}

function renderSourceControllers(controllers) {
  if (controllers === null || controllers === undefined) return 'Unavailable';
  if (!Array.isArray(controllers) || controllers.length === 0) return 'none';
  return controllers.map(formatSourceController).filter(Boolean).join(', ') || 'none';
}

function sourcePaneExpectedCanisterIds() {
  return Array.from(new Set(sourcePaneCanisterInfoNodes()
    .map((node) => node.getAttribute('data-source-module-hash') || node.getAttribute('data-source-controllers') || '')
    .filter(Boolean)));
}

function sourcePaneInfoHasCompleteControllerData(infoByCanisterId) {
  return sourcePaneExpectedCanisterIds().every((canisterId) => (
    Array.isArray(normalizeSourcePaneInfo(infoByCanisterId, canisterId).controllers)
  ));
}

function applySourcePaneModuleHashes(infoByCanisterId, { fallbackTitle = '' } = {}) {
  sourcePaneModuleHashNodes().forEach((node) => {
    const canisterId = node.getAttribute('data-source-module-hash') || '';
    const { moduleHash } = normalizeSourcePaneInfo(infoByCanisterId, canisterId);
    node.textContent = moduleHash || 'Unavailable';
    if (moduleHash) node.setAttribute('title', moduleHash);
    else if (fallbackTitle) node.setAttribute('title', fallbackTitle);
    else node.removeAttribute('title');
  });
  sourcePaneControllerNodes().forEach((node) => {
    const canisterId = node.getAttribute('data-source-controllers') || '';
    const { controllers } = normalizeSourcePaneInfo(infoByCanisterId, canisterId);
    node.innerHTML = renderSourceControllers(controllers);
    if (controllers === null && fallbackTitle) node.setAttribute('title', fallbackTitle);
    else node.removeAttribute('title');
  });
}

function readSourcePaneModuleHashCache() {
  const cacheKey = sourcePaneModuleHashCacheKey();
  if (!cacheKey) return null;
  try {
    const raw = window.localStorage.getItem(cacheKey);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return null;
    const cachedAt = Number(parsed.cachedAt || 0);
    if (!Number.isFinite(cachedAt) || cachedAt <= 0) return null;
    if ((Date.now() - cachedAt) > SOURCE_PANE_MODULE_HASH_CACHE_TTL_MS) return null;
    const infoByCanisterId = parsed.infoByCanisterId || parsed.hashByCanisterId;
    if (!infoByCanisterId || typeof infoByCanisterId !== 'object') return null;
    return { cachedAt, infoByCanisterId };
  } catch { return null; }
}

function writeSourcePaneModuleHashCache(infoByCanisterId) {
  const cacheKey = sourcePaneModuleHashCacheKey();
  if (!cacheKey || !sourcePaneInfoHasCompleteControllerData(infoByCanisterId)) return;
  try { window.localStorage.setItem(cacheKey, JSON.stringify({ cachedAt: Date.now(), infoByCanisterId })); }
  catch { /* Ignore storage failures. */ }
}

async function ensureSourcePaneModuleHashesLoaded() {
  const infoNodes = sourcePaneCanisterInfoNodes();
  if (infoNodes.length === 0 || !FRONTEND_CONFIG?.historianCanisterId) return;
  if (sourcePaneModuleHashesLoadedAt > 0 && (Date.now() - sourcePaneModuleHashesLoadedAt) <= SOURCE_PANE_MODULE_HASH_CACHE_TTL_MS) return;
  const cached = readSourcePaneModuleHashCache();
  if (cached) {
    applySourcePaneModuleHashes(cached.infoByCanisterId);
    sourcePaneModuleHashesLoadedAt = cached.cachedAt;
    return;
  }
  if (sourcePaneModuleHashesRequest) {
    await sourcePaneModuleHashesRequest;
    return;
  }
  sourcePaneModuleHashesRequest = (async () => {
    try {
      const infos = await loadCanisterModuleHashes({
        historianCanisterId: FRONTEND_CONFIG.historianCanisterId,
        host: window.location.origin,
        local: isLocalHost(),
      });
      const infoByCanisterId = Object.fromEntries(
        infos.map((item) => [formatPrincipal(item.canister_id), {
          moduleHash: readOpt(item.module_hash_hex) || null,
          controllers: readOpt(item.controllers)?.map(formatPrincipal) || null,
        }]),
      );
      applySourcePaneModuleHashes(infoByCanisterId);
      writeSourcePaneModuleHashCache(infoByCanisterId);
      sourcePaneModuleHashesLoadedAt = Date.now();
    } catch (error) {
      const reason = normalizeError(error);
      applySourcePaneModuleHashes({}, { fallbackTitle: reason });
    } finally {
      sourcePaneModuleHashesRequest = null;
    }
  })();
  await sourcePaneModuleHashesRequest;
}

function bindInlineTooltipFallbacks() {
  document.addEventListener('click', (event) => {
    const trigger = event.target instanceof Element
      ? event.target.closest('[data-tooltip-text], [data-tooltip-id]')
      : null;
    const popover = inlineTooltipPopover;
    if (trigger) {
      event.preventDefault();
      event.stopPropagation();
      const tooltipId = trigger.getAttribute('data-tooltip-id') || '';
      const trustedContent = tooltipId ? INLINE_TOOLTIP_CONTENT[tooltipId] : '';
      const fallbackText = trigger.getAttribute('data-tooltip-text') || '';
      const content = trustedContent || fallbackText;
      if (!content) return;
      showInlineTooltipPopover(content, { trustedHtml: Boolean(trustedContent) });
      return;
    }
    if (popover && !popover.hidden) {
      const inside = event.target instanceof Element && event.target.closest('.pane-fixed-tooltip-card');
      if (!inside) popover.hidden = true;
    }
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && inlineTooltipPopover && !inlineTooltipPopover.hidden) {
      inlineTooltipPopover.hidden = true;
    }
  });
}

function renderRegisteredPane(data) {
  const state = tableState.registered;
  if (data?.registered) {
    cacheRegisteredPage(data.registered);
    state.error = data?.errors?.registered || null;
  } else if (state.pages.size === 0) {
    state.items = [];
    state.total = 0;
    state.page = 0;
    state.pageSize = TABLE_PAGE_SIZE;
    state.error = data?.errors?.registered || null;
  }

  const page = state.loading && Number.isInteger(state.pendingPage) ? state.pendingPage : state.page;
  const totalPages = registeredTotalPages();
  const body = document.getElementById('registered-pane-body');
  const items = state.items || [];
  const emptyMessage = state.error
    ? `Target canisters unavailable (${state.error})`
    : paneEmptyMessage(data, 'registered', 'No target canisters indexed yet.');
  if (body) {
    body.innerHTML = items.length
      ? items.map((item) => `
        <tr>
          <td>${formatCommitmentTarget(item)}</td>
          <td>${escapeHtml(formatInteger(item.qualifying_commitment_count))}</td>
          <td>${escapeHtml(formatIcpE8s(item.total_qualifying_committed_e8s))}</td>
          <td>${item.latest_cycles?.[0] !== undefined && item.latest_cycles?.[0] !== null ? escapeHtml(formatCycles(item.latest_cycles[0])) : renderCyclesStatusCell(item.last_cycles_probe_ts?.[0] !== undefined && item.last_cycles_probe_ts?.[0] !== null ? 'unavailable' : 'pending')}</td>
        </tr>`).join('')
      : `<tr><td colspan="4" class="empty-cell">${escapeHtml(emptyMessage)}</td></tr>`;
  }

  const pageLabel = state.loading
    ? `Page ${page + 1} of ${totalPages} (loading…)`
    : `Page ${page + 1} of ${totalPages}`;
  setText('registered-page-info', state.error ? `${pageLabel} · ${state.error}` : pageLabel);
  const prev = document.getElementById('registered-prev-page');
  const next = document.getElementById('registered-next-page');
  if (prev) prev.disabled = state.loading || page === 0;
  if (next) next.disabled = state.loading || page >= totalPages - 1;
}

function renderCommitmentsPane(data) {
  const items = data?.recent?.items || [];
  paginate(
    'commitments',
    items,
    (item) => `
      <tr>
        <td>${renderCommitmentTimestampCell(item)}</td>
        <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
        <td>${formatCommitmentTarget(item)}</td>
        <td>${escapeHtml(formatCommitmentOutcome(item))}</td>
      </tr>`,
    paneEmptyMessage(data, 'recent', 'No commitments indexed yet.'),
    4,
  );
}

function renderRouteTransferTimestampCell(item) {
  const label = escapeHtml(formatTimestampNanos(item.timestamp_nanos?.[0]));
  const href = transactionHref(item);
  if (!href) return label;
  return `<a class="pane-external-link" href="${escapeHtml(href)}" target="_blank" rel="noopener noreferrer">${label}</a>`;
}

function renderRouteTransferRow(item) {
  return `
    <tr>
      <td>${renderRouteTransferTimestampCell(item)}</td>
      <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
      <td class="mono">${escapeHtml(formatInteger(item.tx_id))}</td>
    </tr>`;
}

function renderOutputPane(data) {
  const amount = data?.counts?.total_output_e8s;
  setPaneValueText('output-pane-total', amount === undefined || amount === null ? { error: data?.errors?.counts || 'Total output unavailable' } : { value: formatIcpE8s(amount) });
  const note = 'Total Output counts ICP routed from the disburser staging account to the faucet payout account. The table shows recent matching transfers fetched directly from the ICP index canister.';
  setText('output-pane-description', note);
  paginate(
    'output',
    data?.outputTransfers?.items || [],
    renderRouteTransferRow,
    paneEmptyMessage(data, 'outputTransfers', 'No output transfers found in the recent index window.'),
    3,
  );
}

function renderRewardsPane(data) {
  const amount = data?.counts?.total_rewards_e8s;
  setPaneValueText('rewards-pane-total', amount === undefined || amount === null ? { error: data?.errors?.counts || 'Total rewards unavailable' } : { value: formatIcpE8s(amount) });
  const note = 'Total Rewards counts ICP routed from the disburser staging account to the SNS rewards account. The table shows recent matching transfers fetched directly from the ICP index canister.';
  setText('rewards-pane-description', note);
  paginate(
    'rewards',
    data?.rewardsTransfers?.items || [],
    renderRouteTransferRow,
    paneEmptyMessage(data, 'rewardsTransfers', 'No rewards transfers found in the recent index window.'),
    3,
  );
}


function optValue(value) {
  return readOpt(value);
}

function timestampNanosToDate(value) {
  if (value === null || value === undefined) return null;
  const nanos = typeof value === 'bigint' ? value : BigInt(value);
  const millis = nanos / 1_000_000n;
  const asNumber = Number(millis);
  if (!Number.isFinite(asNumber)) return null;
  return new Date(asNumber);
}

function sourceNames(sources) {
  if (!Array.isArray(sources)) return [];
  return sources
    .map((source) => source && typeof source === 'object' && !Array.isArray(source) ? Object.keys(source)[0] : '')
    .filter(Boolean);
}

function trackerPeriod(date, granularity) {
  const year = date.getUTCFullYear();
  if (granularity === 'year') {
    return {
      key: `${year}`,
      label: `${year}`,
      startMs: Date.UTC(year, 0, 1),
    };
  }
  const month = date.getUTCMonth();
  return {
    key: `${year}-${String(month + 1).padStart(2, '0')}`,
    label: date.toLocaleString('en-GB', { month: 'short', year: 'numeric', timeZone: 'UTC' }),
    startMs: Date.UTC(year, month, 1),
  };
}

function emptyTrackerBucket(period) {
  return {
    ...period,
    commitmentAmountE8s: 0n,
    qualifyingCommitmentAmountE8s: 0n,
    commitmentCount: 0,
    qualifyingCommitmentCount: 0,
    observedCmcAmountE8s: 0n,
    observedCmcTransferCount: 0,
    cycles: null,
    cyclesTs: null,
  };
}

function itemAmountE8s(item) {
  return typeof item?.amount_e8s === 'bigint' ? item.amount_e8s : BigInt(item?.amount_e8s || 0);
}

function aggregateTrackerData(data, granularity) {
  const buckets = new Map();
  const ensureBucket = (period) => {
    const existing = buckets.get(period.key);
    if (existing) return existing;
    const created = emptyTrackerBucket(period);
    buckets.set(period.key, created);
    return created;
  };

  for (const item of data?.commitments?.items || []) {
    const timestamp = optValue(item.timestamp_nanos);
    const date = timestampNanosToDate(timestamp);
    if (!date) continue;
    const bucket = ensureBucket(trackerPeriod(date, granularity));
    const amount = itemAmountE8s(item);
    bucket.commitmentAmountE8s += amount;
    bucket.commitmentCount += 1;
    if (item.counts_toward_faucet) {
      bucket.qualifyingCommitmentAmountE8s += amount;
      bucket.qualifyingCommitmentCount += 1;
    }
  }

  for (const item of data?.cmcTransfers?.items || []) {
    const timestamp = optValue(item.timestamp_nanos);
    const date = timestampNanosToDate(timestamp);
    if (!date) continue;
    const bucket = ensureBucket(trackerPeriod(date, granularity));
    bucket.observedCmcAmountE8s += itemAmountE8s(item);
    bucket.observedCmcTransferCount += 1;
  }

  for (const item of data?.cycles?.items || []) {
    const date = timestampNanosToDate(item?.timestamp_nanos);
    if (!date || item?.cycles === undefined || item?.cycles === null) continue;
    const bucket = ensureBucket(trackerPeriod(date, granularity));
    const timestamp = typeof item.timestamp_nanos === 'bigint' ? item.timestamp_nanos : BigInt(item.timestamp_nanos);
    if (bucket.cyclesTs === null || timestamp >= bucket.cyclesTs) {
      bucket.cycles = typeof item.cycles === 'bigint' ? item.cycles : BigInt(item.cycles);
      bucket.cyclesTs = timestamp;
    }
  }

  return Array.from(buckets.values()).sort((left, right) => left.startMs - right.startMs);
}

function ratioBigInt(value, max) {
  if (!max || max <= 0n || value === null || value === undefined) return 0;
  const numerator = (typeof value === 'bigint' ? value : BigInt(value)) * 1_000_000n;
  return Math.max(0, Math.min(1, Number(numerator / max) / 1_000_000));
}

function sumE8s(items) {
  return (items || []).reduce((sum, item) => sum + itemAmountE8s(item), 0n);
}

function trackerMetricSummary(data) {
  const commitmentItems = data?.commitments?.items || [];
  const qualifyingCommitments = commitmentItems.filter((item) => item.counts_toward_faucet);
  const transferItems = data?.cmcTransfers?.items || [];
  const cyclesItems = data?.cycles?.items || [];
  const latestCycles = cyclesItems.length ? cyclesItems[cyclesItems.length - 1]?.cycles : null;
  return {
    commitmentCount: commitmentItems.length,
    qualifyingCommitmentCount: qualifyingCommitments.length,
    totalCommittedE8s: sumE8s(commitmentItems),
    qualifyingCommittedE8s: sumE8s(qualifyingCommitments),
    observedCmcTransferCount: transferItems.length,
    observedCmcE8s: sumE8s(transferItems),
    latestCycles,
  };
}

function pluralize(count, singular, plural = `${singular}s`) {
  return `${formatInteger(count)} ${count === 1 ? singular : plural}`;
}

function renderTrackerEmptyChart(message) {
  return `<div class="tracker-chart-empty">${escapeHtml(message)}</div>`;
}

function renderTrackerAmountBarChart({ buckets, amountKey, countKey, emptyMessage, ariaLabel, barClass = '', labelBuilder }) {
  const maxAmount = buckets.reduce((max, bucket) => bucket[amountKey] > max ? bucket[amountKey] : max, 0n);
  if (maxAmount <= 0n) {
    return renderTrackerEmptyChart(emptyMessage);
  }

  const width = 640;
  const height = 180;
  const padLeft = 44;
  const padRight = 18;
  const padTop = 18;
  const padBottom = 42;
  const chartWidth = width - padLeft - padRight;
  const chartHeight = height - padTop - padBottom;
  const slot = chartWidth / Math.max(1, buckets.length);
  const barWidth = Math.max(8, Math.min(44, slot * 0.58));
  const className = `tracker-chart-bar${barClass ? ` ${barClass}` : ''}`;
  const bars = buckets.map((bucket, index) => {
    const amount = bucket[amountKey] || 0n;
    const ratio = ratioBigInt(amount, maxAmount);
    const barHeight = Math.max(amount > 0n ? 2 : 0, ratio * chartHeight);
    const x = padLeft + (index * slot) + (slot - barWidth) / 2;
    const y = padTop + chartHeight - barHeight;
    const label = labelBuilder ? labelBuilder(bucket) : `${bucket.label}: ${formatIcpE8s(amount)} across ${pluralize(bucket[countKey] || 0, 'item')}`;
    return `<rect class="${className}" x="${x.toFixed(2)}" y="${y.toFixed(2)}" width="${barWidth.toFixed(2)}" height="${barHeight.toFixed(2)}" rx="4"><title>${escapeHtml(label)}</title></rect>`;
  }).join('');
  const ticks = buckets.map((bucket, index) => {
    if (buckets.length > 8 && index % Math.ceil(buckets.length / 8) !== 0 && index !== buckets.length - 1) return '';
    const x = padLeft + (index * slot) + slot / 2;
    return `<text class="tracker-chart-axis-label" x="${x.toFixed(2)}" y="${height - 14}" text-anchor="middle">${escapeHtml(bucket.label)}</text>`;
  }).join('');
  return `
    <svg class="tracker-chart-svg" viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(ariaLabel)}">
      <line class="tracker-chart-axis" x1="${padLeft}" y1="${padTop + chartHeight}" x2="${width - padRight}" y2="${padTop + chartHeight}"></line>
      <text class="tracker-chart-y-label" x="8" y="20">${escapeHtml(formatIcpE8s(maxAmount))}</text>
      ${bars}
      ${ticks}
    </svg>`;
}

function renderTrackerCommitmentsChart(buckets) {
  return renderTrackerAmountBarChart({
    buckets,
    amountKey: 'commitmentAmountE8s',
    countKey: 'commitmentCount',
    barClass: 'tracker-chart-bar--commitment',
    emptyMessage: 'No dated commitments are available for this beneficiary yet.',
    ariaLabel: `ICP commitments by ${trackerState.granularity}`,
    labelBuilder: (bucket) => `${bucket.label}: ${formatIcpE8s(bucket.commitmentAmountE8s)} across ${pluralize(bucket.commitmentCount, 'commitment')}; ${formatIcpE8s(bucket.qualifyingCommitmentAmountE8s)} qualifying across ${pluralize(bucket.qualifyingCommitmentCount, 'qualifying commitment')}`,
  });
}

function renderTrackerObservedCmcChart(buckets) {
  return renderTrackerAmountBarChart({
    buckets,
    amountKey: 'observedCmcAmountE8s',
    countKey: 'observedCmcTransferCount',
    barClass: 'tracker-chart-bar--observed-cmc',
    emptyMessage: 'No dated ICP transfers to the canister’s CMC top-up account are available yet.',
    ariaLabel: `Observed CMC top-up transfers by ${trackerState.granularity}`,
    labelBuilder: (bucket) => `${bucket.label}: ${formatIcpE8s(bucket.observedCmcAmountE8s)} across ${pluralize(bucket.observedCmcTransferCount, 'observed CMC transfer')}`,
  });
}

function renderTrackerCyclesChart(buckets, data) {
  const cycleBuckets = buckets.filter((bucket) => bucket.cycles !== null && bucket.cycles !== undefined);
  if (cycleBuckets.length === 0) {
    const status = cyclesProbeStatusInfo(data);
    return `<div class="tracker-chart-empty">${escapeHtml(status.chartMessage)} ${renderCyclesHelpIcon()}</div>`;
  }

  const maxCycles = cycleBuckets.reduce((max, bucket) => bucket.cycles > max ? bucket.cycles : max, 0n);
  const width = 640;
  const height = 180;
  const padLeft = 44;
  const padRight = 18;
  const padTop = 18;
  const padBottom = 42;
  const chartWidth = width - padLeft - padRight;
  const chartHeight = height - padTop - padBottom;
  const slot = chartWidth / Math.max(1, buckets.length);
  const pointFor = (bucket, index) => {
    const x = padLeft + (index * slot) + slot / 2;
    const ratio = ratioBigInt(bucket.cycles, maxCycles);
    const y = padTop + chartHeight - ratio * chartHeight;
    return { x, y };
  };
  const points = buckets
    .map((bucket, index) => bucket.cycles === null || bucket.cycles === undefined ? null : pointFor(bucket, index))
    .filter(Boolean);
  const polyline = points.map((point) => `${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(' ');
  const circles = buckets.map((bucket, index) => {
    if (bucket.cycles === null || bucket.cycles === undefined) return '';
    const point = pointFor(bucket, index);
    const label = `${bucket.label}: ${formatCycles(bucket.cycles)}`;
    return `<circle class="tracker-chart-point" cx="${point.x.toFixed(2)}" cy="${point.y.toFixed(2)}" r="4"><title>${escapeHtml(label)}</title></circle>`;
  }).join('');
  const ticks = buckets.map((bucket, index) => {
    if (buckets.length > 8 && index % Math.ceil(buckets.length / 8) !== 0 && index !== buckets.length - 1) return '';
    const x = padLeft + (index * slot) + slot / 2;
    return `<text class="tracker-chart-axis-label" x="${x.toFixed(2)}" y="${height - 14}" text-anchor="middle">${escapeHtml(bucket.label)}</text>`;
  }).join('');
  return `
    <svg class="tracker-chart-svg" viewBox="0 0 ${width} ${height}" role="img" aria-label="Cycles balance by ${escapeHtml(trackerState.granularity)}">
      <line class="tracker-chart-axis" x1="${padLeft}" y1="${padTop + chartHeight}" x2="${width - padRight}" y2="${padTop + chartHeight}"></line>
      <text class="tracker-chart-y-label" x="8" y="20">${escapeHtml(formatInteger(maxCycles))}</text>
      <polyline class="tracker-chart-line" points="${polyline}"></polyline>
      ${circles}
      ${ticks}
    </svg>`;
}

function renderTrackerCharts(data) {
  const wrapper = document.getElementById('tracker-chart-wrapper');
  if (!wrapper) return;
  const buckets = aggregateTrackerData(data, trackerState.granularity);
  if (buckets.length === 0) {
    wrapper.innerHTML = renderTrackerEmptyChart('No dated tracker data is available for this canister yet.');
    return;
  }

  wrapper.innerHTML = `
    <div class="tracker-chart-card">
      <div class="tracker-chart-header">
        <h3>ICP commitments</h3>
        <span>Memo-registered commitment history from historian.</span>
      </div>
      ${renderTrackerCommitmentsChart(buckets)}
    </div>
    <div class="tracker-chart-card">
      <div class="tracker-chart-header">
        <h3>Observed CMC top-ups</h3>
        <span>All transfers to the CMC deposit account; direct non-Jupiter top-ups may appear.</span>
      </div>
      ${renderTrackerObservedCmcChart(buckets)}
    </div>
    <div class="tracker-chart-card">
      <div class="tracker-chart-header">
        <h3>Cycles balance</h3>
        <span>Line uses a separate cycles scale.</span>
      </div>
      ${renderTrackerCyclesChart(buckets, data)}
    </div>`;
}

function renderTrackerRecognitionMessage(data, principalText) {
  const result = document.getElementById('tracker-result');
  if (!result) return;
  const detail = data?.isRecognized
    ? 'Historian recognises this principal, but not as a memo-registered commitment beneficiary.'
    : 'This principal is not a recognised commitment beneficiary.';
  result.innerHTML = `
    <div class="tracker-empty-state">
      <p>${escapeHtml(detail)}</p>
      <p>Register the canister for perpetual top-ups from the <a class="pane-external-link" href="${TRACKER_REGISTRATION_URL}" target="_blank" rel="noopener noreferrer">How it works guide</a>.</p>
      <p>${renderCanisterTrackerLink(principalText)}</p>
      <p>${renderCanisterDashboardLink(principalText)}</p>
    </div>`;
}

function renderTrackerData(data, principalText) {
  const result = document.getElementById('tracker-result');
  if (!result) return;
  if (!data?.isCommitmentBeneficiary) {
    renderTrackerRecognitionMessage(data, principalText);
    return;
  }

  const summary = trackerMetricSummary(data);
  const sources = sourceNames(data.overview?.sources).join(', ') || DASH;
  const firstSeen = formatTimestampSeconds(optValue(data.overview?.meta?.first_seen_ts));
  const lastCommitment = formatTimestampSeconds(optValue(data.overview?.meta?.last_commitment_ts));
  const cyclesStatus = cyclesProbeStatusInfo(data);
  const latestCyclesHtml = summary.latestCycles !== null && summary.latestCycles !== undefined
    ? escapeHtml(formatCycles(summary.latestCycles))
    : renderCyclesStatusCell(cyclesStatus.label);
  const commitmentError = data.errors?.commitments ? `<p class="pane-status-note tracker-status-note">Commitment history unavailable: ${escapeHtml(data.errors.commitments)}</p>` : '';
  const cyclesError = data.errors?.cycles ? `<p class="pane-status-note tracker-status-note">Cycles history unavailable: ${escapeHtml(data.errors.cycles)}</p>` : '';
  const cyclesStatusNote = summary.latestCycles === null || summary.latestCycles === undefined
    ? `<p class="pane-status-note tracker-status-note">${escapeHtml(cyclesStatus.note)}</p>`
    : '';
  const cmcError = data.errors?.cmcTransfers ? `<p class="pane-status-note tracker-status-note">Observed CMC top-up history unavailable: ${escapeHtml(data.errors.cmcTransfers)}</p>` : '';
  result.innerHTML = `
    <dl class="pane-detail-grid tracker-summary-grid">
      <div><dt>Canister</dt><dd class="pane-detail-value">${renderCanisterTrackerLink(principalText)}</dd></div>
      <div><dt>Dashboard</dt><dd class="pane-detail-value">${renderCanisterDashboardLink(principalText)}</dd></div>
      <div><dt>Sources</dt><dd class="pane-detail-value">${escapeHtml(sources)}</dd></div>
      <div><dt>First seen</dt><dd class="pane-detail-value">${escapeHtml(firstSeen)}</dd></div>
      <div><dt>Last commitment</dt><dd class="pane-detail-value">${escapeHtml(lastCommitment)}</dd></div>
      <div><dt>Commitments shown</dt><dd class="pane-detail-value">${escapeHtml(formatInteger(summary.commitmentCount))}</dd></div>
      <div><dt>Total commitments shown</dt><dd class="pane-detail-value">${escapeHtml(formatIcpE8s(summary.totalCommittedE8s))}</dd></div>
      <div><dt>Qualifying commitments shown</dt><dd class="pane-detail-value">${escapeHtml(`${formatInteger(summary.qualifyingCommitmentCount)} · ${formatIcpE8s(summary.qualifyingCommittedE8s)}`)}</dd></div>
      <div><dt>Observed CMC transfers shown</dt><dd class="pane-detail-value">${escapeHtml(formatInteger(summary.observedCmcTransferCount))}</dd></div>
      <div><dt>Observed ICP to CMC shown</dt><dd class="pane-detail-value">${escapeHtml(formatIcpE8s(summary.observedCmcE8s))}</dd></div>
      <div><dt>Latest cycles</dt><dd class="pane-detail-value">${latestCyclesHtml}</dd></div>
    </dl>
    <p class="pane-status-note tracker-status-note">Commitments are memo-registered ICP commitments associated with this beneficiary. Observed CMC top-ups are ICP transfers into the canister’s CMC top-up account and may include direct non-Jupiter top-ups.</p>
    ${commitmentError}
    ${cyclesStatusNote}
    ${cyclesError}
    ${cmcError}
    <div class="tracker-chart-wrapper" id="tracker-chart-wrapper"></div>`;
  renderTrackerCharts(data);
}

function setTrackerLoading(loading) {
  const submit = document.getElementById('tracker-submit');
  const input = document.getElementById('tracker-principal-input');
  if (submit) submit.disabled = loading;
  if (input) input.disabled = loading;
}

function setTrackerStatus(message = '', kind = '') {
  const status = document.getElementById('tracker-status');
  if (!status) return;
  status.textContent = message;
  status.hidden = !message;
  status.className = kind ? `pane-status-note tracker-status-note tracker-status-note--${kind}` : 'pane-status-note tracker-status-note';
}

function renderTrackerPrompt() {
  const result = document.getElementById('tracker-result');
  if (!result || result.innerHTML.trim()) return;
  result.innerHTML = `
    <div class="tracker-empty-state">
      <p>Paste a principal ID, usually a target canister ID, to inspect its memo-derived commitments and cycles tracking history.</p>
    </div>`;
}

function setTrackerGranularity(granularity) {
  trackerState.granularity = granularity === 'year' ? 'year' : 'month';
  document.querySelectorAll('[data-tracker-granularity]').forEach((button) => {
    const active = button.getAttribute('data-tracker-granularity') === trackerState.granularity;
    button.classList.toggle('is-active', active);
    button.setAttribute('aria-pressed', active ? 'true' : 'false');
  });
  if (trackerState.data?.isCommitmentBeneficiary) {
    renderTrackerData(trackerState.data, trackerState.principalText);
  }
}

async function submitTrackerPrincipal() {
  const input = document.getElementById('tracker-principal-input');
  const result = document.getElementById('tracker-result');
  const raw = input?.value?.trim() || '';
  trackerState.principalText = raw;
  trackerState.error = null;

  if (!raw) {
    setTrackerStatus('Paste a principal ID first.', 'error');
    input?.focus?.();
    return;
  }

  let principal;
  try {
    principal = Principal.fromText(raw);
  } catch {
    setTrackerStatus('Enter a valid principal ID.', 'error');
    input?.focus?.();
    return;
  }

  setTrackerLoading(true);
  setTrackerStatus('Loading tracker data…', 'loading');
  if (result) {
    result.innerHTML = '<div class="tracker-empty-state"><p>Loading…</p></div>';
  }

  try {
    const data = await loadTrackerData({
      historianCanisterId: FRONTEND_CONFIG?.historianCanisterId,
      host: window.location.origin,
      local: isLocalHost(),
      canisterId: principal,
    });
    trackerState.data = data;
    setTrackerStatus('', '');
    renderTrackerData(data, principal.toText());
  } catch (error) {
    trackerState.data = null;
    trackerState.error = normalizeError(error);
    setTrackerStatus(`Tracker unavailable: ${trackerState.error}`, 'error');
    if (result) {
      result.innerHTML = '<div class="tracker-empty-state"><p>Tracker data could not be loaded right now.</p></div>';
    }
  } finally {
    setTrackerLoading(false);
  }
}

function openTrackerPanelForLinkedPrincipal() {
  const trackerSection = document.querySelector('.nav-panel-section[data-panel="metric-tracker"]');
  const trackerAlreadyOpen = document.body.classList.contains('nav-panel-open')
    && trackerSection?.classList.contains('nav-panel-section--active');
  if (trackerAlreadyOpen) return;
  const trigger = document.querySelector('a[data-panel="metric-tracker"]');
  if (trigger) {
    trigger.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }));
    return;
  }
  if (window.location.hash !== '#metric-tracker') window.location.hash = '#metric-tracker';
}

function trackLinkedPrincipal(principalText) {
  const text = String(principalText || '').trim();
  if (!text) return;
  trackerState.principalText = text;
  const input = document.getElementById('tracker-principal-input');
  if (input) input.value = text;
  openTrackerPanelForLinkedPrincipal();
  window.setTimeout(() => {
    const refreshedInput = document.getElementById('tracker-principal-input');
    if (refreshedInput) refreshedInput.value = text;
    void submitTrackerPrincipal();
  }, 0);
}

function bindTrackerLinks() {
  if (document.documentElement.dataset.trackerLinksBound === 'true') return;
  document.documentElement.dataset.trackerLinksBound = 'true';
  document.addEventListener('click', (event) => {
    const trigger = event.target instanceof Element ? event.target.closest('[data-tracker-principal]') : null;
    if (!trigger) return;
    const principalText = trigger.getAttribute('data-tracker-principal') || '';
    if (!principalText) return;
    event.preventDefault();
    event.stopPropagation();
    trackLinkedPrincipal(principalText);
  }, true);
}

function bindTrackerPane() {
  const form = document.getElementById('tracker-form');
  if (form && form.dataset.bound !== 'true') {
    form.dataset.bound = 'true';
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      void submitTrackerPrincipal();
    });
  }
  document.querySelectorAll('[data-tracker-granularity]').forEach((button) => {
    if (button.dataset.bound === 'true') return;
    button.dataset.bound = 'true';
    button.addEventListener('click', () => {
      setTrackerGranularity(button.getAttribute('data-tracker-granularity') || 'month');
    });
  });
  setTrackerGranularity(trackerState.granularity);
  renderTrackerPrompt();
}

function bindPaginationButtons() {
  const registeredPrev = document.getElementById('registered-prev-page');
  const registeredNext = document.getElementById('registered-next-page');
  if (registeredPrev && registeredPrev.dataset.bound !== 'true') {
    registeredPrev.dataset.bound = 'true';
    registeredPrev.addEventListener('click', () => {
      const targetPage = Math.max(0, tableState.registered.page - 1);
      fetchRegisteredPage(targetPage);
    });
  }
  if (registeredNext && registeredNext.dataset.bound !== 'true') {
    registeredNext.dataset.bound = 'true';
    registeredNext.addEventListener('click', () => {
      const targetPage = Math.min(registeredTotalPages() - 1, tableState.registered.page + 1);
      fetchRegisteredPage(targetPage);
    });
  }

  [
    ['commitments', renderCommitmentsPane],
    ['output', renderOutputPane],
    ['rewards', renderRewardsPane],
  ].forEach(([kind, rerender]) => {
    const prev = document.getElementById(`${kind}-prev-page`);
    const next = document.getElementById(`${kind}-next-page`);
    if (prev && prev.dataset.bound !== 'true') {
      prev.dataset.bound = 'true';
      prev.addEventListener('click', () => {
        tableState[kind].page = Math.max(0, tableState[kind].page - 1);
        rerender(window.__JUPITER_LANDING_DATA__ || null);
      });
    }
    if (next && next.dataset.bound !== 'true') {
      next.dataset.bound = 'true';
      next.addEventListener('click', () => {
        tableState[kind].page += 1;
        rerender(window.__JUPITER_LANDING_DATA__ || null);
      });
    }
  });
}

function renderLandingPanes(data, neuron = null) {
  window.__JUPITER_LANDING_DATA__ = data;
  window.__JUPITER_NEURON_ERROR__ = neuronState.error;
  renderHowItWorksAccount();
  renderHistorianFaultBanner(data);
  renderStakePane(data, neuron);
  renderPaneSubtitles(data);
  renderRegisteredPane(data);
  renderCommitmentsPane(data);
  renderOutputPane(data);
  renderRewardsPane(data);
}

async function loadNeuronDetails({ host, local }) {
  const agent = await HttpAgent.create({
    host,
    verifyQuerySignatures: true,
  });
  if (local) {
    try {
      await agent.fetchRootKey();
    } catch {
      return null;
    }
  }
  const governance = createGovernanceActor(GOVERNANCE_CANISTER_ID, { agent });
  const response = await governance.list_neurons({
    neuron_ids: [JUPITER_NEURON_ID],
    include_neurons_readable_by_caller: false,
    include_empty_neurons_readable_by_caller: [],
    include_public_neurons_in_full_neurons: [true],
    page_number: [],
    page_size: [],
    neuron_subaccounts: [],
  });
  return readOpt(response.full_neurons);
}

const neuronDetailsController = createNeuronDetailsController({
  loadNeuronDetails: () => loadNeuronDetails({ host: window.location.origin, local: isLocalHost() }),
  renderStakePane,
  renderStakeNeuronStatus,
  normalizeError,
});

const neuronState = neuronDetailsController.state;

async function ensureNeuronDetailsLoaded(data) {
  await neuronDetailsController.ensureLoaded(data);
}

function bindNeuronDetailsLoader(data) {
  const load = () => {
    void ensureNeuronDetailsLoaded(data);
  };
  const maybeLoadFromLocation = () => {
    if (window.location.hash === '#metric-stake') {
      load();
    }
  };
  document.querySelectorAll('a[data-panel="metric-stake"]').forEach((trigger) => {
    if (trigger.dataset.neuronBound === 'true') return;
    trigger.dataset.neuronBound = 'true';
    trigger.addEventListener('click', load);
  });
  window.addEventListener('hashchange', maybeLoadFromLocation);
  maybeLoadFromLocation();
  window.setTimeout(maybeLoadFromLocation, 0);
}

async function initLandingPage() {
  bindPaginationButtons();
  setMetricLoadingStates();
  renderStakePane(null, null, { dataLoading: true });
  renderStakeNeuronStatus();
  try {
    const data = await loadDashboardData({
      historianCanisterId: FRONTEND_CONFIG?.historianCanisterId,
      host: window.location.origin,
      local: isLocalHost(),
      registeredPageSize: TABLE_PAGE_SIZE,
    });
    if (data.historianLikelyOutdated) {
      console.warn(`${FRONTEND_HINT} Redeploy jupiter_historian, then hard-refresh the frontend.`);
    }
    renderLandingSummary(data);
    renderLandingPanes(data, null);
    bindNeuronDetailsLoader(data);
  } catch (error) {
    console.warn('Landing metrics failed', error);
    renderLandingUnavailable(normalizeError?.(error) || 'Live metrics unavailable');
    renderLandingPanes(null, null);
  }
}

bindInlineTooltipFallbacks();
bindTrackerPane();
bindTrackerLinks();
document.addEventListener('navpanel:open', (event) => {
  if (event?.detail?.key === 'source') {
    void ensureSourcePaneModuleHashesLoaded();
  }
  if (event?.detail?.key === 'metric-tracker') {
    renderTrackerPrompt();
    window.setTimeout(() => document.getElementById('tracker-principal-input')?.focus?.(), 0);
  }
});
if (window.location.hash === '#source' || document.querySelector('.nav-panel-section--active[data-panel="source"]')) {
  void ensureSourcePaneModuleHashesLoaded();
}

if (document.getElementById('landing-live-summary')) {
  initLandingPage();
}
