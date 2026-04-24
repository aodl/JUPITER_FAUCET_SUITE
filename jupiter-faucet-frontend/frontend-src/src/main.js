import { HttpAgent } from '@icp-sdk/core/agent';
import {
  FRONTEND_HINT,
  normalizeError,
  accountIdentifierHex,
  loadDashboardData,
  loadRegisteredCanisterSummaryPage,
  loadCanisterModuleHashes,
} from './dashboard-data.js';
import { createActor as createGovernanceActor } from '../declarations/nns_governance/index.js';
import { createNeuronDetailsController } from './neuron-details-controller.js';
import { setLink, setPaneValueText, setPaneValueTrustedHtml, setText } from './dom-helpers.js';
import { mergeRegisteredLandingData } from './registered-page-state.js';
import { escapeHtml, formatFolloweeLinks } from './followee-links.js';
import { readOpt } from './candid-opt.js';
import { buildContributionIndexFaultBannerText } from './historian-fault.js';

const FRONTEND_CONFIG = __JUPITER_FRONTEND_CONFIG__;
const DASH = '—';
const GOVERNANCE_CANISTER_ID = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
const JUPITER_NEURON_ID = 11614578985374291210n;
const TABLE_PAGE_SIZE = 6;
const JUPITER_STAKING_ACCOUNT_HEX = '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d';
const INLINE_TOOLTIP_CONTENT = {
  'blackhole-controller-help': `
    <div class="pane-fixed-tooltip-content">
      <p><a class="pane-external-link" href="https://github.com/ninegua/ic-blackhole" target="_blank" rel="noopener noreferrer">Blackhole controller</a> required for cycles observability.</p>
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
};


function isLocalHost() {
  const host = window.location.hostname;
  return host === '127.0.0.1' || host === 'localhost';
}

function formatPrincipal(value) {
  return value?.toText ? value.toText() : String(value || '');
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
  ['landing-current-stake','landing-total-output','landing-total-rewards','landing-registered-canisters','landing-qualifying-contributions'].forEach((id) => {
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
  setMetricStatus('landing-qualifying-contributions', data.counts?.qualifying_contribution_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Commitments unavailable' } : { value: formatInteger(data.counts.qualifying_contribution_count) });
  setHidden('landing-live-unavailable', true);
}

function renderLandingUnavailable(errorMessage = 'Live metrics unavailable') {
  setMetricStatus('landing-current-stake', { error: errorMessage });
  setMetricStatus('landing-total-output', { error: errorMessage });
  setMetricStatus('landing-total-rewards', { error: errorMessage });
  setMetricStatus('landing-registered-canisters', { error: errorMessage });
  setMetricStatus('landing-qualifying-contributions', { error: errorMessage });
  renderHistorianFaultBanner(null);
  setHidden('landing-live-unavailable', true);
}

function renderHistorianFaultBanner(data) {
  const banner = document.getElementById('historian-fault-banner');
  if (!banner) return;
  const text = buildContributionIndexFaultBannerText(data?.status, {
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

function renderHowItWorksAccount() {
  setCopyButton('copy-how-staking-account', () => JUPITER_STAKING_ACCOUNT_HEX);
}

function renderStakePane(data, neuron, { neuronLoading = false, neuronError = null, dataLoading = false } = {}) {
  const stakingAddress = data?.status ? accountIdentifierHex(data.status.staking_account) : JUPITER_STAKING_ACCOUNT_HEX;
  setLink('stake-pane-account-link', {
    href: stakingAddress ? `https://dashboard.internetcomputer.org/account/${stakingAddress}` : '',
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
  setText('output-pane-subtitle', 'Historian tracks protocol-routed ICP from the disburser to the faucet payout account.');
  setText('rewards-pane-subtitle', 'Historian tracks protocol-routed ICP from the disburser to the SNS rewards account.');

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
  setStatusNote('output-pane-memory-note', memoryNote);
  setStatusNote('rewards-pane-memory-note', memoryNote);
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


function renderCyclesUnavailableCell() {
  return `
    <span class="pane-inline-tooltip-fallback">
      <span>unavailable</span>
      <button
        class="pane-inline-tooltip-icon"
        type="button"
        data-tooltip-id="blackhole-controller-help"
        aria-label="Cycles observability help"
      >i</button>
    </span>`;
}

function formatCommitmentTarget(item) {
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  if (canister) {
    return escapeHtml(formatPrincipal(canister));
  }
  return 'invalid target canister memo';
}

function commitmentOutcomeCategory(item) {
  const category = item?.outcome_category;
  if (category && !Array.isArray(category)) {
    if ('QualifyingContribution' in category) return 'QualifyingContribution';
    if ('UnderThresholdContribution' in category) return 'UnderThresholdContribution';
    if ('InvalidTargetMemo' in category) return 'InvalidTargetMemo';
  }
  if (item?.counts_toward_faucet) return 'QualifyingContribution';
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  return canister ? 'UnderThresholdContribution' : 'InvalidTargetMemo';
}

function formatCommitmentOutcome(item) {
  switch (commitmentOutcomeCategory(item)) {
    case 'QualifyingContribution':
      return 'Qualifying';
    case 'UnderThresholdContribution':
      return 'Under threshold';
    case 'InvalidTargetMemo':
      return 'Invalid target';
    default:
      return item?.counts_toward_faucet ? 'Qualifying' : 'Non-qualifying';
  }
}

function commitmentTransactionHref(item) {
  const txIndex = item?.tx_id;
  if (txIndex !== undefined && txIndex !== null) {
    return `https://dashboard.internetcomputer.org/transaction/${encodeURIComponent(String(txIndex))}`;
  }
  return '';
}

function renderCommitmentTimestampCell(item) {
  const label = escapeHtml(formatTimestampNanos(item.timestamp_nanos?.[0]));
  const href = commitmentTransactionHref(item);
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

function sourcePaneModuleHashCacheKey() {
  if (!FRONTEND_CONFIG?.historianCanisterId) return null;
  return `jupiter-faucet:source-pane-module-hashes:${FRONTEND_CONFIG.historianCanisterId}`;
}

function applySourcePaneModuleHashes(hashByCanisterId, { fallbackTitle = '' } = {}) {
  sourcePaneModuleHashNodes().forEach((node) => {
    const canisterId = node.getAttribute('data-source-module-hash') || '';
    const moduleHash = hashByCanisterId[canisterId] || null;
    node.textContent = moduleHash || 'Unavailable';
    if (moduleHash) {
      node.setAttribute('title', moduleHash);
    } else if (fallbackTitle) {
      node.setAttribute('title', fallbackTitle);
    } else {
      node.removeAttribute('title');
    }
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
    if (!parsed.hashByCanisterId || typeof parsed.hashByCanisterId !== 'object') return null;
    return { cachedAt, hashByCanisterId: parsed.hashByCanisterId };
  } catch {
    return null;
  }
}

function writeSourcePaneModuleHashCache(hashByCanisterId) {
  const cacheKey = sourcePaneModuleHashCacheKey();
  if (!cacheKey) return;
  try {
    window.localStorage.setItem(cacheKey, JSON.stringify({
      cachedAt: Date.now(),
      hashByCanisterId,
    }));
  } catch {
    // Ignore storage failures.
  }
}

async function ensureSourcePaneModuleHashesLoaded() {
  const hashNodes = sourcePaneModuleHashNodes();
  if (hashNodes.length === 0 || !FRONTEND_CONFIG?.historianCanisterId) return;
  if (sourcePaneModuleHashesLoadedAt > 0 && (Date.now() - sourcePaneModuleHashesLoadedAt) <= SOURCE_PANE_MODULE_HASH_CACHE_TTL_MS) {
    return;
  }

  const cached = readSourcePaneModuleHashCache();
  if (cached) {
    applySourcePaneModuleHashes(cached.hashByCanisterId);
    sourcePaneModuleHashesLoadedAt = cached.cachedAt;
    return;
  }

  if (sourcePaneModuleHashesRequest) {
    await sourcePaneModuleHashesRequest;
    return;
  }

  sourcePaneModuleHashesRequest = (async () => {
    try {
      const hashes = await loadCanisterModuleHashes({
        historianCanisterId: FRONTEND_CONFIG.historianCanisterId,
        host: window.location.origin,
        local: isLocalHost(),
      });
      const hashByCanisterId = Object.fromEntries(
        hashes.map((item) => [formatPrincipal(item.canister_id), readOpt(item.module_hash_hex) || null]),
      );
      applySourcePaneModuleHashes(hashByCanisterId);
      writeSourcePaneModuleHashCache(hashByCanisterId);
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
          <td class="mono">${formatCommitmentTarget(item)}</td>
          <td>${escapeHtml(formatInteger(item.qualifying_contribution_count))}</td>
          <td>${escapeHtml(formatIcpE8s(item.total_qualifying_contributed_e8s))}</td>
          <td>${item.latest_cycles?.[0] !== undefined && item.latest_cycles?.[0] !== null ? escapeHtml(formatCycles(item.latest_cycles[0])) : renderCyclesUnavailableCell()}</td>
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
        <td class="mono">${formatCommitmentTarget(item)}</td>
        <td>${escapeHtml(formatCommitmentOutcome(item))}</td>
      </tr>`,
    paneEmptyMessage(data, 'recent', 'No commitments indexed yet.'),
    4,
  );
}

function renderOutputPane(data) {
  const amount = data?.counts?.total_output_e8s;
  setPaneValueText('output-pane-total', amount === undefined || amount === null ? { error: data?.errors?.counts || 'Total output unavailable' } : { value: formatIcpE8s(amount) });
  const note = 'Total Output counts ICP routed from the disburser staging account to the faucet payout account. It does not attempt to measure downstream burn or spending.';
  setText('output-pane-description', note);
}

function renderRewardsPane(data) {
  const amount = data?.counts?.total_rewards_e8s;
  setPaneValueText('rewards-pane-total', amount === undefined || amount === null ? { error: data?.errors?.counts || 'Total rewards unavailable' } : { value: formatIcpE8s(amount) });
  const note = 'Total Rewards counts ICP routed from the disburser staging account to the SNS rewards account. It is a Jupiter routing metric rather than a downstream burn metric.';
  setText('rewards-pane-description', note);
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
document.addEventListener('navpanel:open', (event) => {
  if (event?.detail?.key === 'source') {
    void ensureSourcePaneModuleHashesLoaded();
  }
});
if (window.location.hash === '#source' || document.querySelector('.nav-panel-section--active[data-panel="source"]')) {
  void ensureSourcePaneModuleHashesLoaded();
}

if (document.getElementById('landing-live-summary')) {
  initLandingPage();
}
