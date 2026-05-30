import { HttpAgent } from '@icp-sdk/core/agent';
import {
  FRONTEND_HINT,
  normalizeError,
  loadDashboardData,
} from '../dashboard-data.js';
import { createActor as createGovernanceActor } from '../../declarations/nns_governance/index.js';
import { createNeuronDetailsController } from '../neuron-details-controller.js';
import { setText } from '../dom-helpers.js';
import { readOpt } from '../candid-opt.js';
import { buildCommitmentIndexFaultBannerText } from '../historian-fault.js';
import { initAdvancedMemoBuilder } from './advanced-memo-controller.js';
import { GOVERNANCE_CANISTER_ID, JUPITER_NEURON_ID } from './config.js';
import { createDashboardTablesController } from './dashboard-tables-controller.js';
import { SIMULATOR_HASH_PREFIX, simulatorHashForPrefill } from './hash-routes.js';
import { createSimulatorController } from './simulator-controller.js';
import { createSourcePaneController } from './source-pane-controller.js';
import { createStakePaneController } from './stake-pane-controller.js';
import { createTrackerController } from './tracker-controller.js';
import {
  DASH,
  formatBytes,
  formatIcpE8s,
  formatInteger,
  formatLocalTimestampSeconds,
  formatTimestampSeconds,
} from './view-formatters.js';

const FRONTEND_CONFIG = __JUPITER_FRONTEND_CONFIG__;
const INLINE_TOOLTIP_CONTENT = {
  'blackhole-controller-help': `
    <div class="pane-fixed-tooltip-content">
      <p>Cycles balances are sampled periodically by historian.</p>
      <p>For ordinary canisters, cycles observability requires the canister to expose public status through the <a class="pane-external-link" href="https://github.com/ninegua/ic-blackhole" target="_blank" rel="noopener noreferrer">blackhole controller</a>. Newly registered canisters may show as pending until the next cycles sweep completes.</p>
    </div>`,
};

function isLocalHost() {
  const host = window.location.hostname;
  return host === '127.0.0.1' || host === 'localhost';
}

const sourcePaneController = createSourcePaneController({
  frontendConfig: FRONTEND_CONFIG,
  isLocalHost,
});

const dashboardTablesController = createDashboardTablesController({
  frontendConfig: FRONTEND_CONFIG,
  isLocalHost,
});

const simulatorController = createSimulatorController({
  copyTextToClipboard,
  neuronId: JUPITER_NEURON_ID,
});

const trackerController = createTrackerController({
  frontendConfig: FRONTEND_CONFIG,
  isLocalHost,
  simulatorHashForPrefill,
  onSimulatorPrefillHash: () => {
    simulatorController.hydrateFromLocationHash();
  },
});

const stakePaneController = createStakePaneController({
  neuronId: JUPITER_NEURON_ID,
  simulatorController,
  setCopyButton,
  isNeuronLoaded: () => neuronState.loaded,
});

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
  return `Next historian run approx. ${formatLocalTimestampSeconds(next)}.`;
}

function renderLandingSummary(data) {
  setMetricStatus('landing-current-stake', data.stakeE8s === null ? { error: data.errors?.stake || 'Stake unavailable' } : { value: formatIcpE8s(data.stakeE8s) });
  setMetricStatus('landing-total-output', data.counts?.total_output_e8s === undefined || data.counts === null ? { error: data.errors?.counts || 'Total output unavailable' } : { value: formatIcpE8s(data.counts.total_output_e8s) });
  setMetricStatus('landing-total-rewards', data.counts?.total_rewards_e8s === undefined || data.counts === null ? { error: data.errors?.counts || 'Total rewards unavailable' } : { value: formatIcpE8s(data.counts.total_rewards_e8s) });
  setMetricStatus('landing-registered-canisters', data.counts?.registered_canister_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Declared canisters unavailable' } : { value: formatInteger(data.counts.registered_canister_count) });
  setMetricStatus('landing-qualifying-commitments', data.counts?.qualifying_commitment_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Patron commitments unavailable' } : { value: formatInteger(data.counts.qualifying_commitment_count) });
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

function renderPaneSubtitles(data) {
  const subtitle = nextRunLabel(data?.status);
  const registeredCount = data?.counts?.registered_canister_count;
  const optionalCountValue = (value) => (Array.isArray(value) ? value[0] : value);
  const rawIcpDeclaredCanisterCount = optionalCountValue(data?.counts?.raw_icp_declared_canister_count);
  const declaredNeuronCount = optionalCountValue(data?.counts?.declared_neuron_count);
  const commitmentsCanisterCount = registeredCount === undefined || registeredCount === null
    ? ''
    : `(${formatInteger(registeredCount)})`;
  const commitmentsRawCanisterCount = rawIcpDeclaredCanisterCount === undefined || rawIcpDeclaredCanisterCount === null
    ? ''
    : `(${formatInteger(rawIcpDeclaredCanisterCount)})`;
  const commitmentsNeuronCount = declaredNeuronCount === undefined || declaredNeuronCount === null
    ? ''
    : `(${formatInteger(declaredNeuronCount)})`;
  setText('landing-next-run', subtitle);
  setText('registered-pane-subtitle', subtitle);
  setText('commitments-pane-subtitle', subtitle);
  setText('commitments-canister-count', commitmentsCanisterCount);
  setText('commitments-raw-canister-count', commitmentsRawCanisterCount);
  setText('commitments-neuron-count', commitmentsNeuronCount);
  setText('output-pane-subtitle', 'Historian tracks the aggregate; recent rows are queried live from the ICP index canister.');
  setText('rewards-pane-subtitle', 'Historian tracks the aggregate; recent rows are queried live from the ICP index canister.');
  setText('dquorum-pane-subtitle', 'Recent rows are queried live from the ICP index canister.');

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

async function copyTextToClipboard(value) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }
  const textarea = document.createElement('textarea');
  textarea.value = value;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.left = '-9999px';
  document.body.appendChild(textarea);
  textarea.select();
  try {
    if (!document.execCommand('copy')) throw new Error('copy command failed');
  } finally {
    textarea.remove();
  }
}

function setCopyButton(id, valueProvider) {
  const button = document.getElementById(id);
  if (!button || button.dataset.bound === 'true') return;
  button.dataset.bound = 'true';
  button.addEventListener('click', async () => {
    const value = valueProvider();
    if (!value || value === DASH) return;
    try {
      await copyTextToClipboard(value);
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

function renderLandingPanes(data, neuron = null) {
  window.__JUPITER_LANDING_DATA__ = data;
  simulatorController.applyIcpXdrRateFromStatus(data?.status);
  window.__JUPITER_NEURON_ERROR__ = neuronState.error;
  stakePaneController.renderHowItWorksAccount();
  renderHistorianFaultBanner(data);
  stakePaneController.renderStakePane(data, neuron);
  renderPaneSubtitles(data);
  dashboardTablesController.renderAll(data);
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
  renderStakePane: stakePaneController.renderStakePane,
  renderStakeNeuronStatus: stakePaneController.renderStakeNeuronStatus,
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
    if (window.location.hash === '#metric-stake' || window.location.hash === '#simulator' || window.location.hash.startsWith(SIMULATOR_HASH_PREFIX)) {
      load();
    }
  };
  document.querySelectorAll('a[data-panel="metric-stake"], a[data-panel="simulator"]').forEach((trigger) => {
    if (trigger.dataset.neuronBound === 'true') return;
    trigger.dataset.neuronBound = 'true';
    trigger.addEventListener('click', load);
  });
  window.addEventListener('hashchange', maybeLoadFromLocation);
  maybeLoadFromLocation();
  window.setTimeout(maybeLoadFromLocation, 0);
}

async function initLandingPage() {
  dashboardTablesController.bindPaginationButtons();
  setMetricLoadingStates();
  stakePaneController.renderStakePane(null, null, { dataLoading: true });
  stakePaneController.renderStakeNeuronStatus();
  try {
    const data = await loadDashboardData({
      historianCanisterId: FRONTEND_CONFIG?.historianCanisterId,
      host: window.location.origin,
      local: isLocalHost(),
      registeredPageSize: dashboardTablesController.currentTablePageSize(),
    });
    if (data.historianLikelyOutdated) {
      console.warn(`${FRONTEND_HINT} Redeploy jupiter_historian, then hard-refresh the frontend.`);
    }
    renderLandingSummary(data);
    renderLandingPanes(data, null);
    bindNeuronDetailsLoader(data);
    void ensureNeuronDetailsLoaded(data);
  } catch (error) {
    console.warn('Landing metrics failed', error);
    renderLandingUnavailable(normalizeError?.(error) || 'Live metrics unavailable');
    renderLandingPanes(null, null);
  }
}

bindInlineTooltipFallbacks();
dashboardTablesController.bindResponsiveTablePageSize();
trackerController.bindPane();
initAdvancedMemoBuilder({ copyTextToClipboard });
simulatorController.bind();
trackerController.bindLinks();
trackerController.hydrateFromLocationHash({ submit: true });
simulatorController.hydrateFromLocationHash();
window.addEventListener('hashchange', () => {
  trackerController.hydrateFromLocationHash({ submit: true });
  simulatorController.hydrateFromLocationHash();
});
document.addEventListener('navpanel:open', (event) => {
  if (event?.detail?.key === 'source') {
    void sourcePaneController.ensureLoaded();
  }
  if (event?.detail?.key === 'simulator') {
    void ensureNeuronDetailsLoaded(window.__JUPITER_LANDING_DATA__ || null);
    simulatorController.hydrateFromLocationHash();
  }
  if (event?.detail?.key === 'metric-tracker') {
    const hydrated = trackerController.hydrateFromLocationHash({ submit: true });
    if (!hydrated) trackerController.renderPrompt();
    window.setTimeout(() => document.getElementById('tracker-principal-input')?.focus?.(), 0);
  }
});
if (window.location.hash === '#source' || document.querySelector('.nav-panel-section--active[data-panel="source"]')) {
  void sourcePaneController.ensureLoaded();
}

if (document.getElementById('landing-live-summary')) {
  initLandingPage();
}
