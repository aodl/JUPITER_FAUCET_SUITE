import { HttpAgent } from '@icp-sdk/core/agent';
import {
  FRONTEND_HINT,
  normalizeError,
  accountIdentifierHex,
  loadDashboardData,
} from './dashboard-data.js';
import { createActor as createGovernanceActor } from '../declarations/nns_governance/index.js';

const FRONTEND_CONFIG = __JUPITER_FRONTEND_CONFIG__;
const DASH = '—';
const GOVERNANCE_CANISTER_ID = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
const JUPITER_NEURON_ID = 11614578985374291210n;
const ALPHA_VOTE_NEURON_ID = '2947465672511369';
const TABLE_PAGE_SIZE = 6;
const JUPITER_STAKING_ACCOUNT_HEX = '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d';

const tableState = {
  registered: { page: 0, items: [] },
  commitments: { page: 0, items: [] },
  burn: { page: 0, items: [] },
};

const neuronState = {
  requested: false,
  loaded: false,
  value: null,
  error: null,
};

function isLocalHost() {
  const host = window.location.hostname;
  return host === '127.0.0.1' || host === 'localhost';
}

function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
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

function setHidden(id, hidden) {
  const node = document.getElementById(id);
  if (!node) return;
  node.hidden = hidden;
}

function setText(id, value) {
  const node = document.getElementById(id);
  if (node) node.textContent = value;
}

function setHtml(id, value) {
  const node = document.getElementById(id);
  if (node) node.innerHTML = value;
}

function setLink(id, { href, text, title = text } = {}) {
  const node = document.getElementById(id);
  if (!node) return;
  if (!href || !text) {
    node.removeAttribute('href');
    node.removeAttribute('title');
    return;
  }
  node.href = href;
  node.title = title;
  const valueNode = node.querySelector('span') || node;
  valueNode.textContent = text;
}

function setPaneValueStatus(id, { value = null, loading = false, error = null, html = false } = {}) {
  const valueNode = document.getElementById(id);
  const statusNode = document.getElementById(`${id}-status`);
  if (valueNode) {
    if (html) {
      valueNode.innerHTML = value ?? '';
    } else {
      valueNode.textContent = value ?? '';
    }
  }
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
  ['landing-current-stake','landing-icp-burned','landing-registered-canisters','landing-qualifying-contributions'].forEach((id) => {
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
  setMetricStatus('landing-icp-burned', data.counts?.icp_burned_e8s === undefined || data.counts === null ? { error: data.errors?.counts || 'Burned unavailable' } : { value: formatIcpE8s(data.counts.icp_burned_e8s) });
  setMetricStatus('landing-registered-canisters', data.counts?.registered_canister_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Canisters/principals unavailable' } : { value: formatInteger(data.counts.registered_canister_count) });
  setMetricStatus('landing-qualifying-contributions', data.counts?.qualifying_contribution_count === undefined || data.counts === null ? { error: data.errors?.counts || 'Commitments unavailable' } : { value: formatInteger(data.counts.qualifying_contribution_count) });
  setHidden('landing-live-unavailable', true);
}

function renderLandingUnavailable(errorMessage = 'Live metrics unavailable') {
  setMetricStatus('landing-current-stake', { error: errorMessage });
  setMetricStatus('landing-icp-burned', { error: errorMessage });
  setMetricStatus('landing-registered-canisters', { error: errorMessage });
  setMetricStatus('landing-qualifying-contributions', { error: errorMessage });
  setHidden('landing-live-unavailable', true);
}

function renderHowItWorksAccount() {
  setCopyButton('copy-how-staking-account', () => JUPITER_STAKING_ACCOUNT_HEX);
}

function formatFolloweeLinks(neuron) {
  const follows = [...new Set(neuron.followees.flatMap((entry) => entry[1].followees.map((followee) => followee.id.toString())))];
  if (!follows.length) return 'None';
  return follows.map((id) => {
    const label = id === ALPHA_VOTE_NEURON_ID ? 'αlpha-vote' : id;
    const title = id === ALPHA_VOTE_NEURON_ID ? `${label} (${id})` : id;
    return `<a class="pane-external-link mono" href="https://dashboard.internetcomputer.org/neuron/${escapeHtml(id)}" target="_blank" rel="noopener noreferrer" title="${escapeHtml(title)}">${escapeHtml(label)}</a>`;
  }).join(', ');
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
  setPaneValueStatus('stake-pane-balance', dataLoading
    ? { loading: true }
    : data?.stakeE8s === null
      ? { error: data?.errors?.stake || 'Stake unavailable' }
      : { value: formatIcpE8s(data?.stakeE8s) });

  if (neuron) {
    setPaneValueStatus('stake-neuron-age', { value: formatAgeFromSeconds(neuron.aging_since_timestamp_seconds) });
    setPaneValueStatus('stake-neuron-public', { value: 'Yes' });
    setPaneValueStatus('stake-neuron-created', { value: formatTimestampSeconds(neuron.created_timestamp_seconds) });
    setPaneValueStatus('stake-neuron-refresh', { value: formatTimestampSeconds(neuron.voting_power_refreshed_timestamp_seconds?.[0]) });
    setPaneValueStatus('stake-neuron-followees', { value: formatFolloweeLinks(neuron), html: true });
    return;
  }

  const fallback = neuronError
    ? { loading: false, error: neuronError }
    : neuronLoading || !neuronState.loaded
      ? { loading: true }
      : { error: 'Public neuron details unavailable' };
  setPaneValueStatus('stake-neuron-age', fallback);
  setPaneValueStatus('stake-neuron-public', fallback);
  setPaneValueStatus('stake-neuron-created', fallback);
  setPaneValueStatus('stake-neuron-refresh', fallback);
  setPaneValueStatus('stake-neuron-followees', fallback);
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
  setText('burned-pane-subtitle', subtitle);
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


function renderCyclesUnavailableCell() {
  return `
    <span class="pane-inline-tooltip">
      <span>unavailable</span>
      <button class="pane-inline-tooltip-button" type="button" aria-label="Cycles visibility help">i</button>
      <span class="pane-inline-tooltip-bubble">
        Needs a controlling <a class="pane-external-link" href="https://dashboard.internetcomputer.org/canister/e3mmv-5qaaa-aaaah-aadma-cai" target="_blank" rel="noopener noreferrer">blackhole canister</a> for <a class="pane-external-link" href="https://github.com/ninegua/ic-blackhole?tab=readme-ov-file#version-000" target="_blank" rel="noopener noreferrer">cycles visibility</a> (<code>canister_status</code>).
      </span>
    </span>`;
}

function formatCommitmentTarget(item) {
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  if (canister) {
    return escapeHtml(formatPrincipal(canister));
  }
  const memoText = Array.isArray(item?.memo_text) ? item.memo_text[0] : item?.memo_text;
  return escapeHtml(memoText || 'invalid principal');
}

function commitmentTransactionHref(item) {
  const txHash = Array.isArray(item?.tx_hash) ? item.tx_hash[0] : item?.tx_hash;
  const txIndex = item?.tx_id;
  if (txHash) {
    return `https://dashboard.internetcomputer.org/transaction/${encodeURIComponent(txHash)}?index=${encodeURIComponent(String(txIndex))}`;
  }
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

function renderRegisteredPane(data) {
  const items = data?.registered?.items || [];
  paginate(
    'registered',
    items,
    (item) => `
      <tr>
        <td class="mono">${formatCommitmentTarget(item)}</td>
        <td>${escapeHtml(formatInteger(item.qualifying_contribution_count))}</td>
        <td>${escapeHtml(formatIcpE8s(item.total_qualifying_contributed_e8s))}</td>
        <td>${item.latest_cycles?.[0] !== undefined && item.latest_cycles?.[0] !== null ? escapeHtml(formatCycles(item.latest_cycles[0])) : renderCyclesUnavailableCell()}</td>
      </tr>`,
    paneEmptyMessage(data, 'registered', 'No canisters/principals indexed yet.'),
    4,
  );
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
        <td>${item.counts_toward_faucet ? 'Yes' : 'No'}</td>
      </tr>`,
    paneEmptyMessage(data, 'recent', 'No commitments indexed yet.'),
    4,
  );
}

function renderBurnsPane(data) {
  const items = data?.burns?.items || [];
  paginate(
    'burn',
    items,
    (item) => `
      <tr>
        <td>${escapeHtml(formatTimestampNanos(item.timestamp_nanos?.[0]))}</td>
        <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
        <td>Burn</td>
        <td class="mono">${escapeHtml(formatInteger(item.tx_id))}</td>
      </tr>`,
    paneEmptyMessage(data, 'burns', 'No burn transactions indexed yet.'),
    4,
  );
}

function bindPaginationButtons() {
  [
    ['registered', renderRegisteredPane],
    ['commitments', renderCommitmentsPane],
    ['burn', renderBurnsPane],
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
  renderStakePane(data, neuron);
  renderPaneSubtitles(data);
  renderRegisteredPane(data);
  renderCommitmentsPane(data);
  renderBurnsPane(data);
}

async function loadNeuronDetails({ host, local }) {
  const agent = await HttpAgent.create({
    host,
    verifyQuerySignatures: false,
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
  return response.full_neurons?.[0] || null;
}

async function ensureNeuronDetailsLoaded(data) {
  if (neuronState.requested) return;
  neuronState.requested = true;
  renderStakePane(data, null, { neuronLoading: true });
  renderStakeNeuronStatus({ loading: true });
  try {
    const neuron = await loadNeuronDetails({ host: window.location.origin, local: isLocalHost() });
    neuronState.loaded = true;
    neuronState.value = neuron;
    neuronState.error = neuron ? null : 'Public neuron details unavailable';
    window.__JUPITER_NEURON_ERROR__ = neuronState.error;
    renderStakePane(data, neuron, { neuronError: neuronState.error });
    renderStakeNeuronStatus({ error: neuronState.error });
  } catch (error) {
    neuronState.error = normalizeError(error);
    window.__JUPITER_NEURON_ERROR__ = neuronState.error;
    renderStakePane(data, null, { neuronError: neuronState.error });
    renderStakeNeuronStatus({ error: neuronState.error });
    console.info('Public neuron details unavailable; core dashboard metrics load independently.', error);
  }
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

if (document.getElementById('landing-live-summary')) {
  initLandingPage();
}
