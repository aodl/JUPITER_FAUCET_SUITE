import {
  accountIdentifierHex,
  dquorumStakingAccount,
  loadMemoRegisteredCanisterSummaryPage,
  normalizeError,
} from '../dashboard-data.js';
import { setPaneValueText, setText } from '../dom-helpers.js';
import { escapeHtml } from '../followee-links.js';
import { mergeRegisteredLandingData } from '../registered-page-state.js';
import { COMMITMENT_TABLE_PAGE_SIZE_ADJUSTMENT, calculateResponsiveTablePageSize } from './responsive-tables.js';
import {
  DASH,
  formatCycles,
  formatIcpE8s,
  formatInteger,
  formatTimestampNanos,
  renderCanisterTrackerLink,
  renderMemoTrackerLink,
  renderNeuronTrackerLink,
} from './view-formatters.js';

function numericValue(value, fallback = 0) {
  if (value === null || value === undefined) return fallback;
  return Number(value);
}

function paneEmptyMessage(data, key, defaultText) {
  return data?.errors?.[key] ? `${defaultText} (${data.errors[key]})` : defaultText;
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

function formatCommitmentTarget(item) {
  const neuronId = Array.isArray(item?.neuron_id) ? item.neuron_id[0] : item?.neuron_id;
  if (neuronId !== undefined && neuronId !== null) {
    return renderNeuronTrackerLink(neuronId);
  }
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  if (canister) return renderCanisterTrackerLink(canister);
  return 'invalid declared memo';
}

function rawIcpDeclaredMemo(item) {
  const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
  const right = Array.isArray(item?.raw_icp_memo_text) ? item.raw_icp_memo_text[0] : item?.raw_icp_memo_text;
  if (!canister || right === undefined || right === null) return '';
  const left = canister.toText ? canister.toText() : String(canister);
  return `${left.split('-').join('')}.${String(right)}`;
}

function neuronDeclaredMemo(item) {
  const neuronId = Array.isArray(item?.neuron_id) ? item.neuron_id[0] : item?.neuron_id;
  const right = Array.isArray(item?.neuron_memo_text) ? item.neuron_memo_text[0] : item?.neuron_memo_text;
  if (neuronId === undefined || neuronId === null) return '';
  return right === undefined || right === null
    ? String(neuronId)
    : `${String(neuronId)}.${String(right)}`;
}

function transactionHref(item) {
  const txIndex = item?.tx_id;
  if (txIndex !== undefined && txIndex !== null) {
    return `https://dashboard.internetcomputer.org/transaction/${encodeURIComponent(String(txIndex))}`;
  }
  return '';
}

function renderTimestampCell(item) {
  const label = escapeHtml(formatTimestampNanos(item.timestamp_nanos?.[0]));
  const href = transactionHref(item);
  if (!href) return label;
  return `<a class="pane-external-link" href="${escapeHtml(href)}" target="_blank" rel="noopener noreferrer">${label}</a>`;
}

function itemAmountE8s(item) {
  return typeof item?.amount_e8s === 'bigint' ? item.amount_e8s : BigInt(item?.amount_e8s || 0);
}

function sumRouteTransferE8s(items) {
  return (items || []).reduce((sum, item) => sum + itemAmountE8s(item), 0n);
}

function dashboardAccountLink(account, label, fallbackExplorerAccountId = '') {
  const text = escapeHtml(label);
  let explorerAccountId = fallbackExplorerAccountId;
  if (!explorerAccountId && account) {
    try {
      explorerAccountId = accountIdentifierHex(account);
    } catch {
      explorerAccountId = '';
    }
  }
  if (!explorerAccountId) return text;
  return `<a class="pane-external-link" href="https://dashboard.internetcomputer.org/account/${escapeHtml(explorerAccountId)}" target="_blank" rel="noopener noreferrer">${text}</a>`;
}

function renderRouteTransferRow(item) {
  return `
    <tr>
      <td>${renderTimestampCell(item)}</td>
      <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
      <td class="mono">${escapeHtml(formatInteger(item.tx_id))}</td>
    </tr>`;
}

export function createDashboardTablesController({
  frontendConfig,
  isLocalHost,
  getLandingData = () => window.__JUPITER_LANDING_DATA__ || null,
  setLandingData = (data) => {
    window.__JUPITER_LANDING_DATA__ = data;
  },
}) {
  let tablePageSize = calculateResponsiveTablePageSize();
  let tableResizeTimer = null;
  const tableState = {
    registered: {
      page: 0,
      items: [],
      total: 0,
      pageSize: tablePageSize,
      loading: false,
      pendingPage: null,
      error: null,
      pages: new Map(),
    },
    commitments: { page: 0, items: [] },
    'commitments-raw': { page: 0, items: [] },
    'commitments-neurons': { page: 0, items: [] },
    output: { page: 0, items: [] },
    rewards: { page: 0, items: [] },
    dquorum: { page: 0, items: [] },
  };

  const currentTablePageSize = () => tablePageSize;

  const currentPageSizeForTable = (kind) => {
    const adjustment = kind === 'commitments' || kind === 'commitments-raw' || kind === 'commitments-neurons'
      ? COMMITMENT_TABLE_PAGE_SIZE_ADJUSTMENT
      : 0;
    return Math.max(1, currentTablePageSize() + adjustment);
  };

  const paginate = (kind, items, renderRow, emptyMessage, colspan) => {
    const state = tableState[kind];
    const pageSize = currentPageSizeForTable(kind);
    state.items = items || [];
    const totalPages = Math.max(1, Math.ceil(state.items.length / pageSize));
    if (state.page >= totalPages) state.page = totalPages - 1;
    const start = state.page * pageSize;
    const pageItems = state.items.slice(start, start + pageSize);
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
  };

  const registeredPageCacheKey = (page, pageSize) => `${page}:${pageSize}`;

  const cacheRegisteredPage = (response) => {
    if (!response) return;
    const state = tableState.registered;
    const page = numericValue(response.page, 0);
    const pageSize = Math.max(1, numericValue(response.page_size, currentTablePageSize()));
    const items = Array.isArray(response.items) ? response.items : [];
    state.pages.set(registeredPageCacheKey(page, pageSize), response);
    state.page = page;
    state.pageSize = pageSize;
    state.total = numericValue(response.total, items.length);
    state.items = items;
    state.error = null;
  };

  const registeredTotalPages = () => {
    const state = tableState.registered;
    return Math.max(1, Math.ceil(state.total / Math.max(1, state.pageSize || currentTablePageSize())));
  };

  const renderRegisteredPane = (data) => {
    const state = tableState.registered;
    if (data?.registered) {
      cacheRegisteredPage(data.registered);
      state.error = data?.errors?.registered || null;
    } else if (state.pages.size === 0) {
      state.items = [];
      state.total = 0;
      state.page = 0;
      state.pageSize = currentTablePageSize();
      state.error = data?.errors?.registered || null;
    }

    const page = state.loading && Number.isInteger(state.pendingPage) ? state.pendingPage : state.page;
    const totalPages = registeredTotalPages();
    const body = document.getElementById('registered-pane-body');
    const items = state.items || [];
    const emptyMessage = state.error
      ? `Tracked canisters unavailable (${state.error})`
      : paneEmptyMessage(data, 'registered', 'No tracked canisters indexed yet.');
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
  };

  const fetchRegisteredPage = async (page) => {
    const state = tableState.registered;
    if (state.loading) return;
    const pageSize = Math.max(1, state.pageSize || currentTablePageSize());
    const cacheKey = registeredPageCacheKey(page, pageSize);
    const cached = state.pages.get(cacheKey);
    if (cached) {
      cacheRegisteredPage(cached);
      const current = getLandingData() || {};
      setLandingData(mergeRegisteredLandingData(current, {
        registered: cached,
        registeredError: null,
      }));
      renderRegisteredPane(getLandingData() || null);
      return;
    }

    state.loading = true;
    state.pendingPage = page;
    state.error = null;
    renderRegisteredPane(getLandingData() || null);
    try {
      const response = await loadMemoRegisteredCanisterSummaryPage({
        historianCanisterId: frontendConfig?.historianCanisterId,
        host: window.location.origin,
        local: isLocalHost(),
        page,
        pageSize,
      });
      cacheRegisteredPage(response);
      const current = getLandingData() || {};
      setLandingData(mergeRegisteredLandingData(current, {
        registered: response,
        registeredError: null,
      }));
    } catch (error) {
      state.error = normalizeError(error);
      console.warn('Memo registered canister page failed', error);
      const current = getLandingData() || {};
      setLandingData(mergeRegisteredLandingData(current, {
        registeredError: state.error,
      }));
    } finally {
      state.loading = false;
      state.pendingPage = null;
      renderRegisteredPane(getLandingData() || null);
    }
  };

  const renderCommitmentsPane = (data) => {
    const items = data?.recent?.items || [];
    const topUpItems = items.filter((item) => {
      const canister = Array.isArray(item?.canister_id) ? item.canister_id[0] : item?.canister_id;
      const rawMemo = Array.isArray(item?.raw_icp_memo_text) ? item.raw_icp_memo_text[0] : item?.raw_icp_memo_text;
      const neuronId = Array.isArray(item?.neuron_id) ? item.neuron_id[0] : item?.neuron_id;
      return canister && (rawMemo === undefined || rawMemo === null) && (neuronId === undefined || neuronId === null);
    });
    const rawItems = items.filter((item) => {
      const rawMemo = Array.isArray(item?.raw_icp_memo_text) ? item.raw_icp_memo_text[0] : item?.raw_icp_memo_text;
      return rawMemo !== undefined && rawMemo !== null;
    });
    const neuronItems = items.filter((item) => {
      const neuronId = Array.isArray(item?.neuron_id) ? item.neuron_id[0] : item?.neuron_id;
      return neuronId !== undefined && neuronId !== null;
    });
    paginate(
      'commitments',
      topUpItems,
      (item) => `
        <tr>
          <td>${renderTimestampCell(item)}</td>
          <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
          <td>${formatCommitmentTarget(item)}</td>
        </tr>`,
      paneEmptyMessage(data, 'recent', 'No declared canister commitments indexed yet.'),
      3,
    );
    paginate(
      'commitments-raw',
      rawItems,
      (item) => `
        <tr>
          <td>${renderTimestampCell(item)}</td>
          <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
          <td>${renderMemoTrackerLink(rawIcpDeclaredMemo(item))}</td>
        </tr>`,
      paneEmptyMessage(data, 'recent', 'No raw ICP commitments indexed yet.'),
      3,
    );
    paginate(
      'commitments-neurons',
      neuronItems,
      (item) => `
        <tr>
          <td>${renderTimestampCell(item)}</td>
          <td>${escapeHtml(formatIcpE8s(item.amount_e8s))}</td>
          <td>${renderMemoTrackerLink(neuronDeclaredMemo(item))}</td>
        </tr>`,
      paneEmptyMessage(data, 'recent', 'No declared neuron commitments indexed yet.'),
      3,
    );
  };

  const setRouteDescription = (id, data, routeLabel, destinationLabel, destinationAccount = null, { aggregate = true } = {}) => {
    const node = document.getElementById(id);
    if (!node) return;
    const stagingLink = dashboardAccountLink(data?.status?.output_source_account?.[0] || data?.status?.output_source_account, 'staging account');
    const destination = destinationAccount
      ? dashboardAccountLink(destinationAccount, destinationLabel)
      : escapeHtml(destinationLabel);
    node.innerHTML = `Jupiter neuron maturity is disbursed to the controlling canister's ${stagingLink}. ${escapeHtml(routeLabel)} counts ICP routed from that staging account to ${destination}. ${aggregate ? 'Historian tracks the aggregate; ' : ''}recent rows are fetched directly from the ICP index canister.`;
  };

  const renderOutputPane = (data) => {
    const amount = data?.counts?.total_output_e8s;
    setPaneValueText('output-pane-total', amount === undefined || amount === null ? { error: data?.errors?.counts || 'Total output unavailable' } : { value: formatIcpE8s(amount) });
    setRouteDescription('output-pane-description', data, 'Total Output', 'the faucet payout account', data?.status?.output_account?.[0] || data?.status?.output_account);
    paginate(
      'output',
      data?.outputTransfers?.items || [],
      renderRouteTransferRow,
      paneEmptyMessage(data, 'outputTransfers', 'No output transfers found in the recent index window.'),
      3,
    );
  };

  const renderRewardsPane = (data) => {
    const amount = data?.counts?.total_rewards_e8s;
    setPaneValueText('rewards-pane-total', amount === undefined || amount === null ? { error: data?.errors?.counts || 'Total rewards unavailable' } : { value: formatIcpE8s(amount) });
    setRouteDescription('rewards-pane-description', data, 'Total Rewards', 'the SNS rewards account', data?.status?.rewards_account?.[0] || data?.status?.rewards_account);
    paginate(
      'rewards',
      data?.rewardsTransfers?.items || [],
      renderRouteTransferRow,
      paneEmptyMessage(data, 'rewardsTransfers', 'No rewards transfers found in the recent index window.'),
      3,
    );
  };

  const renderDquorumPane = (data) => {
    const transfers = data?.dquorumTransfers?.items || [];
    const dquorumAccount = dquorumStakingAccount();
    setPaneValueText('dquorum-pane-total', data?.errors?.dquorumTransfers
      ? { error: data.errors.dquorumTransfers }
      : { value: formatIcpE8s(sumRouteTransferE8s(transfers)) });
    setRouteDescription('dquorum-pane-description', data, 'D-QUORUM Route', "D-QUORUM's staking account", dquorumAccount, { aggregate: false });
    paginate(
      'dquorum',
      transfers,
      renderRouteTransferRow,
      paneEmptyMessage(data, 'dquorumTransfers', 'No D-QUORUM route transfers found in the recent index window.'),
      3,
    );
  };

  const renderAll = (data) => {
    renderRegisteredPane(data);
    renderCommitmentsPane(data);
    renderOutputPane(data);
    renderRewardsPane(data);
    renderDquorumPane(data);
  };

  const bindPaginationButtons = () => {
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
      ['commitments-raw', renderCommitmentsPane],
      ['commitments-neurons', renderCommitmentsPane],
      ['output', renderOutputPane],
      ['rewards', renderRewardsPane],
      ['dquorum', renderDquorumPane],
    ].forEach(([kind, rerender]) => {
      const prev = document.getElementById(`${kind}-prev-page`);
      const next = document.getElementById(`${kind}-next-page`);
      if (prev && prev.dataset.bound !== 'true') {
        prev.dataset.bound = 'true';
        prev.addEventListener('click', () => {
          tableState[kind].page = Math.max(0, tableState[kind].page - 1);
          rerender(getLandingData() || null);
        });
      }
      if (next && next.dataset.bound !== 'true') {
        next.dataset.bound = 'true';
        next.addEventListener('click', () => {
          tableState[kind].page += 1;
          rerender(getLandingData() || null);
        });
      }
    });
  };

  const applyResponsiveTablePageSize = () => {
    const previousPageSize = currentTablePageSize();
    const nextPageSize = calculateResponsiveTablePageSize();
    if (nextPageSize === previousPageSize) return;

    tablePageSize = nextPageSize;
    ['commitments', 'commitments-raw', 'commitments-neurons', 'output', 'rewards', 'dquorum'].forEach((kind) => {
      const state = tableState[kind];
      const previousKindPageSize = kind === 'commitments' || kind === 'commitments-raw' || kind === 'commitments-neurons'
        ? Math.max(1, previousPageSize + COMMITMENT_TABLE_PAGE_SIZE_ADJUSTMENT)
        : previousPageSize;
      const nextKindPageSize = currentPageSizeForTable(kind);
      const firstVisibleItem = state.page * previousKindPageSize;
      state.page = Math.max(0, Math.floor(firstVisibleItem / nextKindPageSize));
    });

    const data = getLandingData() || null;
    renderCommitmentsPane(data);
    renderOutputPane(data);
    renderRewardsPane(data);
    renderDquorumPane(data);

    const registered = tableState.registered;
    const firstVisibleRegistered = registered.page * Math.max(1, registered.pageSize || previousPageSize);
    const targetRegisteredPage = Math.max(0, Math.floor(firstVisibleRegistered / nextPageSize));
    registered.pageSize = nextPageSize;
    registered.page = targetRegisteredPage;
    if (registered.pages.size > 0 || data?.registered) {
      void fetchRegisteredPage(targetRegisteredPage);
    } else {
      renderRegisteredPane(data);
    }
  };

  const bindResponsiveTablePageSize = () => {
    window.addEventListener('resize', () => {
      window.clearTimeout(tableResizeTimer);
      tableResizeTimer = window.setTimeout(applyResponsiveTablePageSize, 150);
    });
  };

  return {
    bindPaginationButtons,
    bindResponsiveTablePageSize,
    currentTablePageSize,
    renderAll,
    renderCommitmentsPane,
    renderDquorumPane,
    renderOutputPane,
    renderRegisteredPane,
    renderRewardsPane,
    state: tableState,
  };
}
