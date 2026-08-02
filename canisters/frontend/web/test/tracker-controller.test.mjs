import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';

import { createTrackerController } from '../src/app/tracker-controller.js';
import { simulatorHashForPrefill } from '../src/app/hash-routes.js';
import { JUPITER_RELAY_CANISTER_ID } from '../src/app/config.js';
import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';
import { classifyTransferItem, defaultCanisterAccountIdentifier, relayInstanceSourceMap } from '../src/data/transfer-source-classification.js';

const metricsCss = readFileSync(new URL('../../public/metrics.css', import.meta.url), 'utf8');

class FakeElement {
  constructor(attrs = {}) {
    this.attrs = new Map(Object.entries(attrs));
    this.dataset = {};
    this.listeners = new Map();
    this.className = '';
    this.innerHTML = '';
    this.textContent = '';
    this.value = '';
    this.disabled = false;
    this.hidden = false;
    this.focused = false;
    this.classList = {
      toggled: [],
      contains: () => false,
      toggle: (name, active) => {
        this.classList.toggled.push({ name, active });
      },
    };
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  contains(node) {
    return node?.owner === this || node === this;
  }

  closest(selector) {
    if (selector === '[data-simulator-prefill]' && this.attrs.has('data-simulator-prefill')) return this;
    if (selector === '[data-tracker-range]' && this.attrs.has('data-tracker-range')) return this;
    if (selector === '[data-tracker-principal]' && this.attrs.has('data-tracker-principal')) return this;
    if (selector === '[data-tracker-memo]' && this.attrs.has('data-tracker-memo')) return this;
    return null;
  }

  getAttribute(name) {
    return this.attrs.get(name) || '';
  }

  setAttribute(name, value) {
    this.attrs.set(name, value);
  }

  focus() {
    this.focused = true;
  }
}

test('canonical Relay fallback classifies the source and attaches its Relay canister ID', () => {
  const item = classifyTransferItem({
    from_account_identifier: defaultCanisterAccountIdentifier(JUPITER_RELAY_CANISTER_ID),
  });

  assert.equal(item.source_category, 'relay');
  assert.equal(item.source_relay_canister_id, JUPITER_RELAY_CANISTER_ID);
  assert.equal(item.source_label, `Relay ${JUPITER_RELAY_CANISTER_ID.slice(0, 5)}…`);
});

test('generic RelayInstance classification attaches the tracked Relay ID', () => {
  const relay = 'br5f7-7uaaa-aaaaa-qaaca-cai';
  const relayAccountId = defaultCanisterAccountIdentifier(relay);
  const relaySourceMap = relayInstanceSourceMap([{
    canister_id: Principal.fromText(relay),
    tracking_reasons: [{ RelayInstance: null }],
  }]);

  const item = classifyTransferItem(
    { from_account_identifier: relayAccountId },
    { relayCanisterId: JUPITER_RELAY_CANISTER_ID, relaySourceMap },
  );

  assert.equal(item.source_category, 'relay');
  assert.equal(item.source_relay_canister_id, relay);
  assert.equal(item.source_label, 'Relay br5f7…');
});

test('unknown transfer sources remain other without fabricated Relay metadata', () => {
  const item = classifyTransferItem({ from_account_identifier: 'f'.repeat(64) });

  assert.equal(item.source_category, 'other');
  assert.equal(Object.hasOwn(item, 'source_relay_canister_id'), false);
  assert.equal(Object.hasOwn(item, 'source_label'), false);
});

test('Relay slot styles cover bars, linked legends, and fixed segment highlighting', () => {
  for (let slot = 1; slot <= 6; slot += 1) {
    assert.match(metricsCss, new RegExp(`\\.tracker-chart-bar--source-relay-${slot} \\{[\\s\\S]*?fill:`));
    assert.match(metricsCss, new RegExp(`\\.tracker-source-legend-item\\.tracker-chart-bar--source-relay-${slot} \\{[\\s\\S]*?color:`));
    assert.match(metricsCss, new RegExp(`data-source-segment="relay-instance-${slot}"`));
  }
  assert.match(metricsCss, /\.tracker-source-legend-item--link \{[\s\S]*?text-decoration: underline;/);
  assert.match(metricsCss, /\.tracker-source-legend-item--relay \{[\s\S]*?overflow-wrap: anywhere;/);
});

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
}

async function withFakeTrackerDom(nodes, fn, { hash = '', rangeButtons = [], trackerOpen = false } = {}) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const originalHistory = globalThis.history;
  const originalElement = globalThis.Element;
  const originalMouseEvent = globalThis.MouseEvent;
  const nodeMap = new Map(nodes.map((node) => [node.id, node]));
  const documentListeners = new Map();
  const clickedPanels = [];
  const historyCalls = [];

  globalThis.Element = FakeElement;
  globalThis.MouseEvent = class {
    constructor(type) {
      this.type = type;
    }
  };
  globalThis.document = {
    documentElement: { dataset: {} },
    body: {
      classList: {
        contains: (name) => trackerOpen && name === 'nav-panel-open',
      },
    },
    getElementById(id) {
      return nodeMap.get(id) || null;
    },
    querySelector(selector) {
      if (selector === '.nav-panel-section[data-panel="metric-tracker"]') {
        return trackerOpen ? {
          classList: {
            contains: (name) => name === 'nav-panel-section--active',
          },
        } : null;
      }
      if (selector === 'a[data-panel="metric-tracker"]') {
        return {
          dispatchEvent(event) {
            clickedPanels.push(event.type);
          },
        };
      }
      return null;
    },
    querySelectorAll(selector) {
      return selector === '[data-tracker-range]' ? rangeButtons : [];
    },
    addEventListener(type, listener) {
      documentListeners.set(type, listener);
    },
  };
  globalThis.window = {
    location: { hash, origin: 'https://example.test' },
    setTimeout(callback) {
      callback();
      return 1;
    },
  };
  globalThis.history = {
    replaceState(_state, _title, nextHash) {
      historyCalls.push({ type: 'replace', hash: nextHash });
      window.location.hash = nextHash;
    },
    pushState(_state, _title, nextHash) {
      historyCalls.push({ type: 'push', hash: nextHash });
      window.location.hash = nextHash;
    },
  };

  try {
    await fn({ nodeMap, documentListeners, clickedPanels, historyCalls });
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
    globalThis.history = originalHistory;
    globalThis.Element = originalElement;
    globalThis.MouseEvent = originalMouseEvent;
  }
}

function trackerNodes() {
  return [
    Object.assign(new FakeElement(), { id: 'tracker-form' }),
    Object.assign(new FakeElement(), { id: 'tracker-result' }),
    Object.assign(new FakeElement(), { id: 'tracker-status' }),
    Object.assign(new FakeElement(), { id: 'tracker-submit' }),
    Object.assign(new FakeElement(), { id: 'tracker-principal-input' }),
    Object.assign(new FakeElement(), { id: 'tracker-chart-wrapper' }),
  ];
}

function minimalTrackerData() {
  return {
    isCommitmentBeneficiary: true,
    isRecognized: true,
    overview: {
      tracking_reasons: [{ MemoCommitment: null }],
      meta: {
        first_seen_ts: [1n],
        last_commitment_ts: [1n],
      },
    },
    status: {
      index_interval_seconds: 60n,
      cycles_interval_seconds: 120n,
      last_index_run_ts: [1n],
      last_completed_cycles_sweep_ts: [1n],
    },
    commitments: {
      items: [{
        timestamp_nanos: 1_700_000_000_000_000_000n,
        amount_e8s: 200_000_000n,
        counts_toward_faucet: true,
      }],
    },
    cycles: { items: [] },
    cmcTransfers: { items: [] },
    logs: { items: [] },
    errors: {},
  };
}

test('tracker renders cycles-only data for recognized relay instances', async () => {
  const nodes = trackerNodes();
  const relay = 'u2qkp-aqaaa-aaaar-qb7ea-cai';
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => ({
        isCommitmentBeneficiary: false,
        isRecognized: true,
        overview: {
          tracking_reasons: [{ RelayInstance: null }],
          meta: {
            first_seen_ts: [123n],
            last_cycles_probe_ts: [456n],
          },
        },
        status: {
          cycles_interval_seconds: 3600n,
          last_completed_cycles_sweep_ts: [456n],
        },
        commitments: { items: [] },
        cycles: {
          items: [{
            timestamp_nanos: 1_700_000_000_000_000_000n,
            source: { BlackholeStatus: null },
            cycles: 4_200_000_000_000n,
          }],
        },
        cmcTransfers: { items: [] },
        logs: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = relay;

    await controller.submitPrincipal();

    const html = `${nodeMap.get('tracker-result').innerHTML}${nodeMap.get('tracker-chart-wrapper').innerHTML}`;
    assert.match(html, /Cycles balance/);
    assert.match(html, /RelayInstance/);
    assert.match(html, /4\.2000T cycles/);
    assert.doesNotMatch(html, /Total committed/);
    assert.doesNotMatch(html, /Qualifying commitments/);
    assert.doesNotMatch(html, /not a recognised commitment beneficiary/);
    assert.doesNotMatch(html, /Commitment history is updated/);
    assert.match(html, /Cycles balances are sampled by historian cycles sweeps/);
  });
});

function rawTransfer(id, from, amountE8s, isMatchingMemo = false, memoText = null) {
  return {
    tx_id: BigInt(id),
    timestamp_nanos: [1_700_000_000_000_000_000n + BigInt(id)],
    amount_e8s: BigInt(amountE8s),
    from_account_identifier: from,
    is_matching_memo: isMatchingMemo,
    icrc1_memo_text: memoText,
  };
}

function rawCommitment(id, amountE8s, timestampNanos = 1_700_000_000_000_000_000n + BigInt(id)) {
  return {
    tx_id: BigInt(id),
    timestamp_nanos: Array.isArray(timestampNanos) ? timestampNanos : [timestampNanos],
    amount_e8s: BigInt(amountE8s),
    counts_toward_faucet: true,
  };
}

function dayTimestampNanos(day) {
  return 1_700_000_000_000_000_000n + BigInt(day) * 86_400_000_000_000n;
}

function cmcTransfer(id, from, amountE8s, day = id) {
  return {
    tx_id: BigInt(id),
    timestamp_nanos: [dayTimestampNanos(day)],
    amount_e8s: BigInt(amountE8s),
    from_account_identifier: from,
  };
}

function relayInstance(relayCanisterId) {
  return {
    canister_id: Principal.fromText(relayCanisterId),
    tracking_reasons: [{ RelayInstance: null }],
  };
}

function trackerDataWithCmcTransfers({
  transfers,
  relayCanisterIds = [],
  faucetAccount = null,
} = {}) {
  const data = minimalTrackerData();
  return {
    ...data,
    status: {
      ...data.status,
      ...(faucetAccount ? { output_account: [faucetAccount] } : {}),
    },
    relayInstances: {
      items: relayCanisterIds.map((relayCanisterId) => relayInstance(relayCanisterId)),
    },
    cmcTransfers: { items: transfers || [] },
  };
}

function relayTrackerLinks(html) {
  return Array.from(String(html || '').matchAll(/<a[\s\S]*?data-tracker-memo="([^"]+\.)"[\s\S]*?<\/a>/g))
    .map((match) => ({ html: match[0], memo: match[1] }));
}

test('tracker hash hydration submits once for the same principal', async () => {
  const calls = [];
  const nodes = trackerNodes();
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => true,
      simulatorHashForPrefill,
      loadData: async (request) => {
        calls.push(request);
        return { isCommitmentBeneficiary: false, isRecognized: false };
      },
    });
    controller.bindPane();
    assert.equal(controller.hydrateFromLocationHash({ submit: true }), true);
    assert.equal(controller.hydrateFromLocationHash({ submit: true }), true);
    await flushMicrotasks();

    assert.equal(nodeMap.get('tracker-principal-input').value, 'jufzc-caaaa-aaaar-qb5da-cai');
    assert.equal(calls.length, 1);
    assert.equal(calls[0].historianCanisterId, 'hist-aa');
    assert.equal(calls[0].host, 'https://example.test');
    assert.equal(calls[0].local, true);
    assert.equal(calls[0].canisterId.toText(), 'jufzc-caaaa-aaaar-qb5da-cai');
    assert.equal(calls[0].historyLimit, 10_000);
    assert.equal(typeof calls[0].minTimestampNanos, 'bigint');
  }, { hash: '#metric-tracker-jufzc-caaaa-aaaar-qb5da-cai' });
});

test('tracker submit rejects invalid principals without loading data', async () => {
  const nodes = trackerNodes();
  let calls = 0;
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => {
        calls += 1;
      },
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'not a principal';

    await nodeMap.get('tracker-form').listeners.get('submit')({ preventDefault() {} });

    assert.equal(calls, 0);
    assert.equal(nodeMap.get('tracker-status').textContent, 'Memo target is not a valid canister principal or neuron ID.');
    assert.equal(nodeMap.get('tracker-principal-input').focused, true);
  });
});

test('tracker paints chart shells before the first data response', async () => {
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const cases = [
    {
      memo: canister,
      expectedHeadings: [/ICP commitments/, /Observed CMC top-ups/, /Cycles balance/],
      finalData: minimalTrackerData(),
      loaderKey: 'loadData',
    },
    {
      memo: `${canister.replaceAll('-', '')}.miner`,
      expectedHeadings: [/ICP commitments/, /Raw ICP canister memo/],
      finalData: {
        commitments: { items: [] },
        transfers: { items: [] },
        candidates: { items: [] },
        errors: {},
      },
      loaderKey: 'loadRawCanisterData',
    },
    {
      memo: '42.miner',
      expectedHeadings: [/ICP commitments/, /Raw ICP neuron memo/],
      finalData: {
        neuronId: 42n,
        stakingAccount: { owner: Principal.fromText('aaaaa-aa'), subaccount: [] },
        commitments: { items: [] },
        transfers: { items: [] },
        errors: {},
      },
      loaderKey: 'loadNeuronData',
    },
  ];

  for (const testCase of cases) {
    const nodes = trackerNodes();
    await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
      let resolveLoad;
      const pendingLoad = new Promise((resolve) => {
        resolveLoad = resolve;
      });
      const controller = createTrackerController({
        frontendConfig: {},
        isLocalHost: () => false,
        simulatorHashForPrefill,
        [testCase.loaderKey]: async () => pendingLoad,
      });
      controller.bindPane();
      nodeMap.get('tracker-principal-input').value = testCase.memo;

      const submission = controller.submitPrincipal();
      const loadingHtml = nodeMap.get('tracker-result').innerHTML;

      assert.equal(controller.state.loading, true);
      assert.match(loadingHtml, /tracker-chart-wrapper--loading/);
      assert.match(loadingHtml, /tracker-chart-loading-bars/);
      assert.match(loadingHtml, /Tracker charts are loading/);
      assert.doesNotMatch(loadingHtml, /tracker-empty-state"><p>Loading/);
      testCase.expectedHeadings.forEach((heading) => assert.match(loadingHtml, heading));

      resolveLoad(testCase.finalData);
      await submission;
      assert.equal(controller.state.loading, false);
    });
  }
});

test('plain memo tracker renders cumulative chart progress while sources load', async () => {
  const nodes = trackerNodes();
  let progressResultHtml = '';
  let progressChartHtml = '';
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const finalData = minimalTrackerData();
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async ({ onProgress }) => {
        onProgress({
          ...finalData,
          cycles: { items: [], loading: true },
          cmcTransfers: { items: [], loading: true },
          logs: { items: [], loading: true },
        });
        progressResultHtml = nodeMap.get('tracker-result').innerHTML;
        progressChartHtml = nodeMap.get('tracker-chart-wrapper').innerHTML;
        return finalData;
      },
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    assert.match(progressChartHtml, /tracker-chart-svg/);
    assert.match(progressChartHtml, /Loading observed CMC top-up history/);
    assert.match(progressChartHtml, /Loading cycles balance history/);
    assert.match(progressResultHtml, /Tracker charts are still loading/);
    assert.match(progressResultHtml, /Patron commitments shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
    assert.match(progressResultHtml, /Latest cycles shown<\/dt><dd class="pane-detail-value"><span class="tracker-summary-loading">Loading…<\/span>/);
    assert.match(progressResultHtml, /data-tracker-range="month"[^>]* disabled aria-disabled="true"/);

    const finalHtml = nodeMap.get('tracker-result').innerHTML;
    assert.doesNotMatch(finalHtml, /Tracker charts are still loading/);
    assert.doesNotMatch(finalHtml, /tracker-summary-loading/);
    assert.doesNotMatch(finalHtml, /data-tracker-range="month"[^>]* disabled/);
  });
});

test('tracker range buttons rerender loaded beneficiary data', async () => {
  const nodes = trackerNodes();
  const monthButton = new FakeElement({ 'data-tracker-range': 'month' });
  const allButton = new FakeElement({ 'data-tracker-range': 'all' });
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => minimalTrackerData(),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';
    await controller.submitPrincipal();

    controller.setRange('month');

    assert.equal(controller.state.range, 'month');
    assert.match(nodeMap.get('tracker-result').innerHTML, /Latest Month/);
    assert.match(nodeMap.get('tracker-result').innerHTML, /Showing last month/);
    assert.deepEqual(monthButton.classList.toggled.at(-1), { name: 'is-active', active: true });
    assert.deepEqual(allButton.classList.toggled.at(-1), { name: 'is-active', active: false });
  }, { rangeButtons: [monthButton, allButton] });
});

test('tracker commitment empty state distinguishes scoped ranges from all history', async () => {
  const nodes = trackerNodes();
  const trackerData = {
    ...minimalTrackerData(),
    cycles: {
      items: [{
        timestamp_nanos: 1_800_000_000_000_000_000n,
        source: { BlackholeStatus: null },
        cycles: 4_200_000_000_000n,
      }],
    },
  };
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => trackerData,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';
    await controller.submitPrincipal();

    assert.match(
      nodeMap.get('tracker-chart-wrapper').innerHTML,
      /No dated commitments are available within the selected time range\. Select All to view older loaded history\./,
    );
    assert.doesNotMatch(
      nodeMap.get('tracker-chart-wrapper').innerHTML,
      /No dated commitments are available for this beneficiary yet\./,
    );

    controller.setRange('all');
    await flushMicrotasks();

    assert.doesNotMatch(
      nodeMap.get('tracker-chart-wrapper').innerHTML,
      /No dated commitments are available within the selected time range\./,
    );
    assert.doesNotMatch(
      nodeMap.get('tracker-chart-wrapper').innerHTML,
      /No dated commitments are available for this beneficiary yet\./,
    );
    assert.match(nodeMap.get('tracker-chart-wrapper').innerHTML, /ICP commitments/);
  });
});

test('tracker initial scoped load uses the selected-range commitment empty state', async () => {
  const nodes = trackerNodes();
  const trackerData = {
    ...minimalTrackerData(),
    commitments: { items: [] },
    cycles: {
      items: [{
        timestamp_nanos: 1_800_000_000_000_000_000n,
        source: { BlackholeStatus: null },
        cycles: 4_200_000_000_000n,
      }],
    },
  };
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => trackerData,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';
    await controller.submitPrincipal();

    assert.equal(controller.state.range, 'month');
    assert.match(
      nodeMap.get('tracker-chart-wrapper').innerHTML,
      /No dated commitments are available within the selected time range\. Select All to view older loaded history\./,
    );
    assert.doesNotMatch(
      nodeMap.get('tracker-chart-wrapper').innerHTML,
      /No dated commitments are available for this beneficiary yet\./,
    );
  });
});

test('tracker range buttons replace the metric-tracker hash for shareable views', async () => {
  const nodes = trackerNodes();
  const yearButton = new FakeElement({ 'data-tracker-range': 'year' });
  await withFakeTrackerDom(nodes, async ({ nodeMap, historyCalls }) => {
    yearButton.owner = nodeMap.get('tracker-result');
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => minimalTrackerData(),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();
    nodeMap.get('tracker-result').listeners.get('click')({
      target: yearButton,
      preventDefault() {},
    });

    assert.equal(controller.state.range, 'year');
    assert.equal(window.location.hash, '#metric-tracker?memo=jufzc-caaaa-aaaar-qb5da-cai&range=year');
    assert.deepEqual(historyCalls.map((call) => call.hash), [
      '#metric-tracker?memo=jufzc-caaaa-aaaar-qb5da-cai&range=month',
      '#metric-tracker?memo=jufzc-caaaa-aaaar-qb5da-cai&range=year',
    ]);
  }, { hash: '#metric-tracker', rangeButtons: [yearButton] });
});

test('tracker range deep links hydrate the selected range before loading data', async () => {
  const nodes = trackerNodes();
  const requests = [];
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async (request) => {
        requests.push(request);
        return minimalTrackerData();
      },
    });
    controller.bindPane();

    assert.equal(controller.hydrateFromLocationHash({ submit: true }), true);
    await flushMicrotasks();

    assert.equal(controller.state.range, 'all');
    assert.equal(nodeMap.get('tracker-principal-input').value, 'jufzc-caaaa-aaaar-qb5da-cai');
    assert.equal(requests[0].minTimestampNanos, null);
  }, { hash: '#metric-tracker?memo=jufzc-caaaa-aaaar-qb5da-cai&range=all' });
});

test('tracker all range reloads raw ICP history with the large transfer limit', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const historyLimits = [];
  const cutoffs = [];

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async ({ historyLimit, minTimestampNanos }) => {
        historyLimits.push(historyLimit);
        cutoffs.push(minTimestampNanos);
        return {
          status: {},
          transfers: { items: [], limit: historyLimit },
          candidates: { items: [] },
          errors: {},
        };
      },
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();
    controller.setRange('all');
    await flushMicrotasks();

    assert.deepEqual(historyLimits, [10_000, 10_000]);
    assert.equal(typeof cutoffs[0], 'bigint');
    assert.equal(cutoffs[1], null);
    assert.equal(controller.state.range, 'all');
    assert.equal(controller.state.loadedRange, 'all');
  });
});

test('tracker year range reloads raw ICP history with a wider cutoff', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const historyLimits = [];
  const cutoffs = [];

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async ({ historyLimit, minTimestampNanos }) => {
        historyLimits.push(historyLimit);
        cutoffs.push(minTimestampNanos);
        return {
          status: {},
          transfers: { items: [], limit: historyLimit },
          candidates: { items: [] },
          errors: {},
        };
      },
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();
    controller.setRange('year');
    await flushMicrotasks();

    assert.deepEqual(historyLimits, [10_000, 10_000]);
    assert.equal(typeof cutoffs[0], 'bigint');
    assert.equal(typeof cutoffs[1], 'bigint');
    assert.ok(cutoffs[1] < cutoffs[0]);
    assert.equal(controller.state.range, 'year');
    assert.equal(controller.state.loadedRange, 'year');
  });
});

test('tracker simulator prefill links update hash and call simulator hook', async () => {
  const nodes = trackerNodes();
  const simulatorHashes = [];
  await withFakeTrackerDom(nodes, async ({ nodeMap, historyCalls }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      onSimulatorPrefillHash: (hash) => simulatorHashes.push(hash),
    });
    controller.bindPane();
    const link = new FakeElement({
      'data-simulator-prefill': 'true',
      href: '#simulator-burn=0.1000&commitment=10.0',
    });
    link.owner = nodeMap.get('tracker-result');

    nodeMap.get('tracker-result').listeners.get('click')({
      target: link,
      preventDefault() {},
    });

    assert.deepEqual(historyCalls, [{ type: 'push', hash: '#simulator-burn=0.1000&commitment=10.0' }]);
    assert.deepEqual(simulatorHashes, ['#simulator-burn=0.1000&commitment=10.0']);
  });
});

test('delegated tracker links open the tracker panel and submit linked principals', async () => {
  const nodes = trackerNodes();
  const calls = [];
  await withFakeTrackerDom(nodes, async ({ nodeMap, documentListeners, clickedPanels }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async (request) => {
        calls.push(request.canisterId.toText());
        return { isCommitmentBeneficiary: false, isRecognized: false };
      },
    });
    controller.bindPane();
    controller.bindLinks();
    const trigger = new FakeElement({ 'data-tracker-principal': 'jufzc-caaaa-aaaar-qb5da-cai' });

    documentListeners.get('click')({
      target: trigger,
      preventDefault() {},
      stopPropagation() {},
    });
    await flushMicrotasks();

    assert.deepEqual(clickedPanels, ['click']);
    assert.equal(nodeMap.get('tracker-principal-input').value, 'jufzc-caaaa-aaaar-qb5da-cai');
    assert.deepEqual(calls, ['jufzc-caaaa-aaaar-qb5da-cai']);
  });
});

test('delegated tracker memo links preserve compact dotted memo hashes', async () => {
  const nodes = trackerNodes();
  const calls = [];
  const compactMemo = '22255zqaaaaaaasqf6uqcai.miner';
  await withFakeTrackerDom(nodes, async ({ nodeMap, documentListeners, historyCalls }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async (request) => {
        calls.push({
          canisterId: request.canisterId.toText(),
          outgoingMemoText: request.outgoingMemoText,
          historyLimit: request.historyLimit,
          hasCutoff: typeof request.minTimestampNanos === 'bigint',
        });
        return {
          status: {},
          transfers: { items: [] },
          candidates: { items: [] },
          errors: {},
        };
      },
    });
    controller.bindPane();
    controller.bindLinks();
    const trigger = new FakeElement({ 'data-tracker-memo': compactMemo });

    documentListeners.get('click')({
      target: trigger,
      preventDefault() {},
      stopPropagation() {},
    });
    await flushMicrotasks();

    assert.equal(nodeMap.get('tracker-principal-input').value, compactMemo);
    assert.deepEqual(calls, [{
      canisterId: '22255-zqaaa-aaaas-qf6uq-cai',
      outgoingMemoText: 'miner',
      historyLimit: 10_000,
      hasCutoff: true,
    }]);
    assert.deepEqual(historyCalls.map((call) => call.hash), ['#metric-tracker?memo=22255zqaaaaaaasqf6uqcai.miner&range=month']);
  });
});

test('delegated tracker links preserve the previous tracker route in browser history', async () => {
  const nodes = trackerNodes();
  const previousMemo = 'jufzc-caaaa-aaaar-qb5da-cai';
  const linkedMemo = 'u2qkp-aqaaa-aaaar-qb7ea-cai.';
  const loads = [];
  await withFakeTrackerDom(nodes, async ({
    documentListeners,
    historyCalls,
    clickedPanels,
  }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async ({ canisterId }) => {
        loads.push(canisterId.toText());
        return minimalTrackerData();
      },
      loadRawCanisterData: async ({ canisterId, outgoingMemoText }) => {
        loads.push(`${canisterId.toText()}.${outgoingMemoText}`);
        return {
          status: {},
          transfers: { items: [] },
          candidates: { items: [] },
          errors: {},
        };
      },
    });
    controller.bindPane();
    controller.bindLinks();
    assert.equal(controller.hydrateFromLocationHash({ submit: true }), true);
    await flushMicrotasks();
    const trigger = new FakeElement({ 'data-tracker-memo': linkedMemo });

    documentListeners.get('click')({
      target: trigger,
      preventDefault() {},
      stopPropagation() {},
    });
    await flushMicrotasks();

    window.location.hash = `#metric-tracker?memo=${previousMemo}&range=month`;
    assert.equal(controller.hydrateFromLocationHash({ submit: true }), true);
    await flushMicrotasks();

    assert.deepEqual(clickedPanels, []);
    assert.deepEqual(historyCalls, [{
      type: 'push',
      hash: `#metric-tracker?memo=${linkedMemo}&range=month`,
    }]);
    assert.deepEqual(loads, [
      previousMemo,
      linkedMemo,
      previousMemo,
    ]);
  }, {
    hash: `#metric-tracker?memo=${previousMemo}&range=month`,
    trackerOpen: true,
  });
});

test('tracker hides observed CMC top-up card when no top-ups are loaded', async () => {
  const nodes = trackerNodes();
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => minimalTrackerData(),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-chart-wrapper').innerHTML;
    assert.match(html, /ICP commitments/);
    assert.match(html, /Cycles balance/);
    assert.doesNotMatch(html, /Observed CMC top-ups/);
    assert.doesNotMatch(html, /No dated ICP transfers to the canister’s CMC top-up account are available yet/);
  });
});

test('observed CMC chart renders two Relay instances as separately coloured tracker links', async () => {
  const nodes = trackerNodes();
  const target = 'jufzc-caaaa-aaaar-qb5da-cai';
  const relayA = 'u2qkp-aqaaa-aaaar-qb7ea-cai';
  const relayB = 'br5f7-7uaaa-aaaaa-qaaca-cai';
  const protocol = '22255-zqaaa-aaaas-qf6uq-cai';
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);
  const protocolAccountId = defaultCanisterAccountIdentifier(protocol);
  const otherAccountId = 'f'.repeat(64);
  const data = trackerDataWithCmcTransfers({
    faucetAccount,
    relayCanisterIds: [relayA, relayB],
    transfers: [
      cmcTransfer(10, faucetAccountId, 100_000_000n, 400),
      cmcTransfer(9, defaultCanisterAccountIdentifier(relayA), 200_000_000n, 400),
      cmcTransfer(8, defaultCanisterAccountIdentifier(relayB), 300_000_000n, 400),
      cmcTransfer(7, protocolAccountId, 400_000_000n, 400),
      cmcTransfer(6, otherAccountId, 500_000_000n, 400),
    ],
  });

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => data,
    });
    controller.bindPane();
    controller.state.protocolCanisterText = protocol;
    nodeMap.get('tracker-principal-input').value = target;

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-chart-wrapper').innerHTML;
    const links = relayTrackerLinks(html);
    assert.equal(links.length, 2);
    assert.deepEqual(links.map((link) => link.memo), [`${relayA}.`, `${relayB}.`]);
    links.forEach((link, index) => {
      const relayCanisterId = [relayA, relayB][index];
      assert.match(link.html, new RegExp(`</i>Jupiter Relay · ${relayCanisterId.slice(0, 5)}…\\s*</a>`));
      assert.match(link.html, new RegExp(`title="Jupiter Relay ${relayCanisterId.replaceAll('-', '\\-')}"`));
      assert.match(link.html, /&amp;range=month"/);
      assert.doesNotMatch(link.html, /protocol-canister/);
      assert.doesNotMatch(link.html, /tabindex=/);
    });
    assert.match(links[0].html, /tracker-chart-bar--source-relay-1/);
    assert.match(links[0].html, /data-source-segment="relay-instance-1"/);
    assert.match(links[0].html, new RegExp(`href="#metric-tracker\\?memo=${relayA.replaceAll('-', '\\-')}\\.&amp;range=month"`));
    assert.match(links[1].html, /tracker-chart-bar--source-relay-2/);
    assert.match(links[1].html, /data-source-segment="relay-instance-2"/);
    assert.match(html, new RegExp(`<rect class="tracker-chart-bar tracker-chart-bar--source-relay-1" data-source-segment="relay-instance-1"`));
    assert.match(html, new RegExp(`<rect class="tracker-chart-bar tracker-chart-bar--source-relay-2" data-source-segment="relay-instance-2"`));
    assert.match(html, new RegExp(`Relay ${relayA} 2 ICP across 1 observed CMC transfer`));
    assert.match(html, new RegExp(`Relay ${relayB} 3 ICP across 1 observed CMC transfer`));
    assert.match(html, /data-source-segment="faucet"/);
    assert.match(html, /Jupiter Faucet 1 ICP across 1 observed CMC transfer/);
    assert.match(html, /data-source-segment="protocol"/);
    assert.match(html, /Protocol canister 4 ICP across 1 observed CMC transfer/);
    assert.match(html, /data-source-segment="other"/);
    assert.match(html, /Other 5 ICP across 1 observed CMC transfer/);
    assert.match(html, /Relay canister IDs open the corresponding raw ICP tracker/);
  });
});

test('observed CMC chart deduplicates Relay links and aggregates daily transfers per Relay slot', async () => {
  const nodes = trackerNodes();
  const relay = 'br5f7-7uaaa-aaaaa-qaaca-cai';
  const relayAccountId = defaultCanisterAccountIdentifier(relay);
  const data = trackerDataWithCmcTransfers({
    relayCanisterIds: [relay],
    transfers: [
      cmcTransfer(3, relayAccountId, 100_000_000n, 400),
      cmcTransfer(2, relayAccountId, 200_000_000n, 400),
      cmcTransfer(1, relayAccountId, 400_000_000n, 399),
    ],
  });

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => data,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-chart-wrapper').innerHTML;
    assert.equal(relayTrackerLinks(html).filter((link) => link.memo === `${relay}.`).length, 1);
    assert.equal(new Set(Array.from(html.matchAll(/data-source-segment="(relay-instance-\d)"/g), (match) => match[1])).size, 1);
    assert.match(html, new RegExp(`Relay ${relay} 3 ICP across 2 observed CMC transfers`));
    assert.match(html, new RegExp(`Relay ${relay} 4 ICP across 1 observed CMC transfer`));
  });
});

test('observed CMC Relay slots stay stable while range visibility follows observed transfers', async () => {
  const nodes = trackerNodes();
  const relayA = 'u2qkp-aqaaa-aaaar-qb7ea-cai';
  const relayB = 'br5f7-7uaaa-aaaaa-qaaca-cai';
  const registeredWithoutTransfer = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
  const data = trackerDataWithCmcTransfers({
    relayCanisterIds: [relayA, relayB, registeredWithoutTransfer],
    transfers: [
      cmcTransfer(20, defaultCanisterAccountIdentifier(relayA), 200_000_000n, 400),
      cmcTransfer(10, defaultCanisterAccountIdentifier(relayB), 100_000_000n, 0),
    ],
  });

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => data,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    const monthHtml = nodeMap.get('tracker-chart-wrapper').innerHTML;
    const monthLinks = relayTrackerLinks(monthHtml);
    assert.deepEqual(monthLinks.map((link) => link.memo), [`${relayA}.`]);
    assert.match(monthLinks[0].html, /tracker-chart-bar--source-relay-1/);
    assert.match(monthLinks[0].html, /range=month/);
    assert.doesNotMatch(monthHtml, new RegExp(registeredWithoutTransfer));

    controller.setRange('all');
    await flushMicrotasks();

    const allHtml = nodeMap.get('tracker-chart-wrapper').innerHTML;
    const allLinks = relayTrackerLinks(allHtml);
    assert.deepEqual(allLinks.map((link) => link.memo), [`${relayA}.`, `${relayB}.`]);
    assert.match(allLinks[0].html, /tracker-chart-bar--source-relay-1/);
    assert.match(allLinks[1].html, /tracker-chart-bar--source-relay-2/);
    allLinks.forEach((link) => assert.match(link.html, /range=all/));
    assert.doesNotMatch(allHtml, new RegExp(registeredWithoutTransfer));
  });
});

test('observed CMC chart links the canonical Relay fallback without a registry entry', async () => {
  const nodes = trackerNodes();
  const data = trackerDataWithCmcTransfers({
    transfers: [
      cmcTransfer(
        1,
        defaultCanisterAccountIdentifier(JUPITER_RELAY_CANISTER_ID),
        200_000_000n,
        400,
      ),
    ],
  });

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => data,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    const links = relayTrackerLinks(nodeMap.get('tracker-chart-wrapper').innerHTML);
    assert.equal(links.length, 1);
    assert.equal(links[0].memo, `${JUPITER_RELAY_CANISTER_ID}.`);
    assert.match(links[0].html, new RegExp(`href="#metric-tracker\\?memo=${JUPITER_RELAY_CANISTER_ID.replaceAll('-', '\\-')}\\.&amp;range=month"`));
  });
});

test('observed CMC chart bounds explicit Relay segments and discloses visible overflow Relays', async () => {
  const nodes = trackerNodes();
  const relayCanisterIds = [
    'u2qkp-aqaaa-aaaar-qb7ea-cai',
    'rrkah-fqaaa-aaaaa-aaaaq-cai',
    'ryjl3-tyaaa-aaaaa-aaaba-cai',
    'r7inp-6aaaa-aaaaa-aaabq-cai',
    'rkp4c-7iaaa-aaaaa-aaaca-cai',
    'rno2w-sqaaa-aaaaa-aaacq-cai',
    'renrk-eyaaa-aaaaa-aaada-cai',
    'rdmx6-jaaaa-aaaaa-aaadq-cai',
  ];
  const data = trackerDataWithCmcTransfers({
    relayCanisterIds,
    transfers: relayCanisterIds.map((relayCanisterId, index) => cmcTransfer(
      100 - index,
      defaultCanisterAccountIdentifier(relayCanisterId),
      BigInt(index + 1) * 100_000_000n,
      index < 6 ? 400 : 0,
    )),
  });

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => data,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    const monthHtml = nodeMap.get('tracker-chart-wrapper').innerHTML;
    assert.equal(relayTrackerLinks(monthHtml).length, 6);
    assert.doesNotMatch(monthHtml, /tracker-relay-overflow-details/);
    relayCanisterIds.slice(6).forEach((relayCanisterId) => {
      assert.doesNotMatch(monthHtml, new RegExp(relayCanisterId));
    });

    controller.setRange('all');
    await flushMicrotasks();

    const html = nodeMap.get('tracker-chart-wrapper').innerHTML;
    const explicitSegmentKeys = new Set(
      Array.from(html.matchAll(/data-source-segment="(relay-instance-\d)"/g), (match) => match[1]),
    );
    assert.deepEqual(Array.from(explicitSegmentKeys).sort(), [
      'relay-instance-1',
      'relay-instance-2',
      'relay-instance-3',
      'relay-instance-4',
      'relay-instance-5',
      'relay-instance-6',
    ]);
    assert.match(html, /data-source-segment="relay"/);
    assert.match(html, /Other Relay instances/);
    assert.match(html, /Additional Relay canisters grouped in “Other Relay instances” \(2\)/);
    assert.match(html, /These Relay canister IDs share one grouped graph colour/);

    const links = relayTrackerLinks(html);
    assert.equal(links.length, relayCanisterIds.length);
    const overflowRelayCanisterIds = relayCanisterIds.slice(6);
    for (const relayCanisterId of overflowRelayCanisterIds) {
      const matchingLinks = links.filter((link) => link.memo === `${relayCanisterId}.`);
      assert.equal(matchingLinks.length, 1);
      assert.match(matchingLinks[0].html, /tracker-chart-bar--source-relay\b/);
      assert.match(matchingLinks[0].html, /data-source-segment="relay"/);
      assert.match(matchingLinks[0].html, /range=all/);
      assert.doesNotMatch(matchingLinks[0].html, /protocol-canister/);
    }
  });
});

test('observed CMC chart does not render Relay links or disclosure without visible Relay bars', async () => {
  const nodes = trackerNodes();
  const relayWithoutTransfer = 'br5f7-7uaaa-aaaaa-qaaca-cai';
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const data = trackerDataWithCmcTransfers({
    faucetAccount,
    relayCanisterIds: [relayWithoutTransfer],
    transfers: [
      cmcTransfer(1, accountIdentifierHex(faucetAccount), 100_000_000n, 400),
    ],
  });

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadData: async () => data,
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = 'jufzc-caaaa-aaaar-qb5da-cai';

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-chart-wrapper').innerHTML;
    assert.match(html, /Observed CMC top-ups/);
    assert.match(html, /data-source-segment="faucet"/);
    assert.equal(relayTrackerLinks(html).length, 0);
    assert.doesNotMatch(html, /tracker-relay-overflow-details/);
    assert.doesNotMatch(html, new RegExp(relayWithoutTransfer));
  });
});

test('raw ICP tracker splits Jupiter Faucet transfers by outgoing memo match', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const protocol = 'jufzc-caaaa-aaaar-qb5da-cai';
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);
  const relayAccountId = defaultCanisterAccountIdentifier(JUPITER_RELAY_CANISTER_ID);
  const protocolAccountId = defaultCanisterAccountIdentifier(protocol);
  const otherAccountId = 'f'.repeat(64);

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async ({ historyLimit }) => ({
        status: { output_account: [faucetAccount] },
        transfers: { items: [
          rawTransfer(5, faucetAccountId, 500_000_000n, true, 'miner'),
          rawTransfer(4, faucetAccountId, 400_000_000n, false, 'treasury'),
          rawTransfer(3, relayAccountId, 300_000_000n, false),
          rawTransfer(2, protocolAccountId, 200_000_000n, false),
          rawTransfer(1, otherAccountId, 100_000_000n, true),
        ], truncated: true, limit: historyLimit },
        candidates: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    controller.state.protocolCanisterText = protocol;
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Jupiter Faucet · matching memo/);
    assert.match(html, /Jupiter Faucet · treasury/);
    assert.doesNotMatch(html, /Jupiter Faucet · other memo/);
    assert.match(html, /tracker-chart-bar--source-faucet-memo-1/);
    assert.match(html, /tracker-chart-bar--source-faucet-memo-2/);
    assert.doesNotMatch(html, /tracker-chart-bar--source-faucet-other-memo/);
    assert.match(html, /Jupiter Relay/);
    assert.match(html, /Protocol canister/);
    assert.match(html, /Other/);
    assert.match(html, /Visible Jupiter Faucet transfers matching the outgoing memo: 1 · 5 ICP/);
    assert.match(html, /Jupiter Faucet · matching memo 5 ICP across 1 transfer/);
    assert.match(html, /Jupiter Faucet · treasury 4 ICP across 1 transfer/);
    assert.match(html, /Chart display is limited to the newest 10,000 incoming ICP transfers/);
  });
});

test('raw ICP canister and neuron trackers render commitment and incoming-transfer charts', async () => {
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);

  for (const mode of ['rawIcpCanister', 'neuronStake']) {
    const nodes = trackerNodes();
    await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
      const controller = createTrackerController({
        frontendConfig: {},
        isLocalHost: () => false,
        simulatorHashForPrefill,
        loadRawCanisterData: async () => ({
          status: { output_account: [faucetAccount] },
          commitments: { items: [rawCommitment(8, 800_000_000n)] },
          transfers: { items: [rawTransfer(5, faucetAccountId, 500_000_000n, true, 'miner')] },
          candidates: { items: [] },
          errors: {},
        }),
        loadNeuronData: async () => ({
          neuronId: 42n,
          stakingAccount: faucetAccount,
          status: { output_account: [faucetAccount] },
          commitments: { items: [rawCommitment(9, 900_000_000n)] },
          transfers: { items: [rawTransfer(6, faucetAccountId, 600_000_000n, true, 'miner')] },
          errors: {},
        }),
      });
      controller.bindPane();
      nodeMap.get('tracker-principal-input').value = mode === 'rawIcpCanister' ? `${compactCanister}.miner` : '42.miner';

      await controller.submitPrincipal();

      const html = nodeMap.get('tracker-result').innerHTML;
      assert.match(html, /<h3>ICP commitments<\/h3>/);
      assert.match(html, mode === 'rawIcpCanister' ? /Raw ICP canister memo/ : /Raw ICP neuron memo/);
      assert.match(html, /Commitments shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
      assert.match(html, /Incoming transfers shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
    });
  }
});

test('raw ICP tracker range uses the newest dated commitment or transfer as the shared anchor', async () => {
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);

  const scenarios = [
    {
      name: 'newer transfer',
      commitments: { items: [rawCommitment(1, 100_000_000n, dayTimestampNanos(0))] },
      transfers: { items: [rawTransfer(5, faucetAccountId, 500_000_000n, true)] },
      expectedCommitments: '0',
      expectedTransfers: '1',
      emptyMessage: /No retained qualifying commitments are loaded for last month/,
    },
    {
      name: 'newer commitment',
      commitments: { items: [rawCommitment(1, 100_000_000n, dayTimestampNanos(40))] },
      transfers: { items: [rawTransfer(5, faucetAccountId, 500_000_000n, true)] },
      expectedCommitments: '1',
      expectedTransfers: '0',
      emptyMessage: /No dated incoming ICP transfers are available in last month/,
    },
  ];
  scenarios[0].transfers.items[0].timestamp_nanos = [dayTimestampNanos(40)];
  scenarios[1].transfers.items[0].timestamp_nanos = [dayTimestampNanos(0)];

  for (const scenario of scenarios) {
    const nodes = trackerNodes();
    await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
      const controller = createTrackerController({
        frontendConfig: {},
        isLocalHost: () => false,
        simulatorHashForPrefill,
      });
      controller.bindPane();
      controller.state.viewMode = 'rawIcpCanister';
      controller.state.data = {
        status: { output_account: [faucetAccount] },
        commitments: scenario.commitments,
        transfers: scenario.transfers,
        candidates: { items: [] },
        errors: {},
      };
      controller.state.parsedMemo = {
        kind: 'rawIcpCanister',
        canisterText: canister,
        canisterId: Principal.fromText(canister),
        normalizedMemoText: canister,
        outgoingMemoText: null,
      };

      controller.setRange('month');

      const html = nodeMap.get('tracker-result').innerHTML;
      assert.match(
        html,
        new RegExp(`Commitments shown</dt><dd class="pane-detail-value">${scenario.expectedCommitments}</dd>`),
        scenario.name,
      );
      assert.match(
        html,
        new RegExp(`Incoming transfers shown</dt><dd class="pane-detail-value">${scenario.expectedTransfers}</dd>`),
        scenario.name,
      );
      assert.match(html, scenario.emptyMessage, scenario.name);
    });
  }
});

test('raw ICP commitment chart uses range-aware empty copy', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
    });
    controller.bindPane();
    controller.state.viewMode = 'rawIcpCanister';
    controller.state.data = {
      status: {},
      commitments: { items: [] },
      transfers: { items: [] },
      candidates: { items: [] },
      errors: {},
    };
    controller.state.parsedMemo = {
      kind: 'rawIcpCanister',
      canisterText: canister,
      canisterId: Principal.fromText(canister),
      normalizedMemoText: canister,
      outgoingMemoText: null,
    };

    controller.setRange('month');
    let html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /No retained qualifying commitments are loaded for last month/);
    assert.match(html, /loader may not have loaded older retained rows/);
    assert.doesNotMatch(html, /No retained target history exists/);

    controller.setRange('all');
    html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /No dated retained qualifying commitments are available for this target/);
  });
});

test('raw ICP commitment chart distinguishes undated retained commitments from no retained history', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
    });
    controller.bindPane();
    controller.state.viewMode = 'rawIcpCanister';
    controller.state.data = {
      status: {},
      commitments: { items: [rawCommitment(4, 400_000_000n, [])] },
      transfers: { items: [] },
      candidates: { items: [] },
      errors: {},
    };
    controller.state.parsedMemo = {
      kind: 'rawIcpCanister',
      canisterText: canister,
      canisterId: Principal.fromText(canister),
      normalizedMemoText: canister,
      outgoingMemoText: null,
    };

    controller.setRange('all');

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Commitments shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
    assert.match(html, /ICP committed shown<\/dt><dd class="pane-detail-value">4 ICP<\/dd>/);
    assert.match(html, /No dated retained qualifying commitments are available for this target/);
    assert.doesNotMatch(html, /No retained target history exists/);
  });
});

test('raw ICP tracker isolates commitment and transfer chart failures', async () => {
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);

  for (const failure of ['commitments', 'transfers']) {
    const nodes = trackerNodes();
    await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
      const controller = createTrackerController({
        frontendConfig: {},
        isLocalHost: () => false,
        simulatorHashForPrefill,
      });
      controller.bindPane();
      controller.state.viewMode = 'rawIcpCanister';
      controller.state.data = {
        status: { output_account: [faucetAccount] },
        commitments: { items: failure === 'commitments' ? [] : [rawCommitment(3, 300_000_000n)] },
        transfers: { items: failure === 'transfers' ? [] : [rawTransfer(5, faucetAccountId, 500_000_000n, true)] },
        candidates: { items: [] },
        errors: failure === 'commitments'
          ? { commitments: 'commitment query failed', transfers: null }
          : { commitments: null, transfers: 'index query failed' },
      };
      controller.state.parsedMemo = {
        kind: 'rawIcpCanister',
        canisterText: canister,
        canisterId: Principal.fromText(canister),
        normalizedMemoText: canister,
        outgoingMemoText: null,
      };

      controller.setRange('all');

      const html = nodeMap.get('tracker-result').innerHTML;
      if (failure === 'commitments') {
        assert.match(html, /Commitment history unavailable: commitment query failed/);
        assert.match(html, /Commitments shown<\/dt><dd class="pane-detail-value">—<\/dd>/);
        assert.match(html, /ICP committed shown<\/dt><dd class="pane-detail-value">—<\/dd>/);
        assert.match(html, /Incoming transfers shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
        assert.equal((html.match(/Commitment history unavailable/g) || []).length, 1);
      } else {
        assert.match(html, /Raw ICP transfer history unavailable: index query failed/);
        assert.match(html, /Commitments shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
      }
    });
  }
});

test('raw ICP commitment-only data renders no empty transfer-source legend', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
    });
    controller.bindPane();
    controller.state.viewMode = 'rawIcpCanister';
    controller.state.data = {
      status: {},
      commitments: { items: [rawCommitment(3, 300_000_000n)] },
      transfers: { items: [] },
      candidates: { items: [] },
      errors: {},
    };
    controller.state.parsedMemo = {
      kind: 'rawIcpCanister',
      canisterText: canister,
      canisterId: Principal.fromText(canister),
      normalizedMemoText: canister,
      outgoingMemoText: null,
    };

    controller.setRange('all');

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Commitments shown<\/dt><dd class="pane-detail-value">1<\/dd>/);
    assert.match(html, /Incoming transfers shown<\/dt><dd class="pane-detail-value">0<\/dd>/);
    assert.doesNotMatch(html, /tracker-source-legend/);
  });
});

test('raw ICP tracker explains target-wide commitment scope when an outgoing suffix is present', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async () => ({
        status: {},
        commitments: { items: [rawCommitment(3, 300_000_000n)] },
        transfers: { items: [] },
        candidates: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Retained qualifying commitment history is recorded for the destination target as a whole/);
    assert.match(html, /is not filtered by this outgoing memo/);
    assert.match(html, /The chart shows retained qualifying commitments in the selected range/);
  });
});

test('raw ICP tracker groups faucet memos after five distinct memos', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async () => ({
        status: { output_account: [faucetAccount] },
        transfers: { items: [
          rawTransfer(6, faucetAccountId, 600_000_000n, false, 'memo-f'),
          rawTransfer(5, faucetAccountId, 500_000_000n, true, 'miner'),
          rawTransfer(4, faucetAccountId, 400_000_000n, false, 'memo-d'),
          rawTransfer(3, faucetAccountId, 300_000_000n, false, 'memo-c'),
          rawTransfer(2, faucetAccountId, 200_000_000n, false, 'memo-b'),
          rawTransfer(1, faucetAccountId, 100_000_000n, false, 'memo-a'),
        ] },
        candidates: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Jupiter Faucet · matching memo/);
    assert.match(html, /Jupiter Faucet · memo-f/);
    assert.match(html, /Jupiter Faucet · memo-d/);
    assert.match(html, /Jupiter Faucet · memo-c/);
    assert.match(html, /Jupiter Faucet · memo-b/);
    assert.doesNotMatch(html, /Jupiter Faucet · memo-a/);
    assert.match(html, /Jupiter Faucet · other memo/);
    assert.match(html, /Jupiter Faucet · other memo 1 ICP across 1 transfer/);
  });
});

test('raw ICP tracker renders incoming transfers while index pages are still loading', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);
  let progressHtml = '';

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async ({ historyLimit, onProgress }) => {
        onProgress({
          status: { output_account: [faucetAccount] },
          transfers: {
            items: [rawTransfer(5, faucetAccountId, 500_000_000n, true, 'miner')],
            loading: true,
            truncated: false,
            limit: historyLimit,
            pages_loaded: 1,
          },
          candidates: { items: [], truncated: false, loading: true },
          errors: {},
        });
        progressHtml = nodeMap.get('tracker-result').innerHTML;
        return {
          status: { output_account: [faucetAccount] },
          transfers: {
            items: [
              rawTransfer(5, faucetAccountId, 500_000_000n, true, 'miner'),
              rawTransfer(4, faucetAccountId, 400_000_000n, false, 'treasury'),
            ],
            loading: false,
            truncated: false,
            limit: historyLimit,
          },
          candidates: { items: [], truncated: false },
          errors: {},
        };
      },
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();

    assert.match(progressHtml, /Chart still loading incoming ICP history/);
    assert.match(progressHtml, /1 transfers loaded across 1 index pages/);
    assert.match(progressHtml, /data-tracker-range="month"[^>]* disabled aria-disabled="true"/);
    assert.match(progressHtml, /Matching tracked canisters are still loading/);
    const finalHtml = nodeMap.get('tracker-result').innerHTML;
    assert.doesNotMatch(finalHtml, /Chart still loading incoming ICP history/);
    assert.match(finalHtml, /Incoming transfers shown<\/dt><dd class="pane-detail-value">2<\/dd>/);
  });
});

test('raw ICP tracker legend only includes sources with visible bars', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async () => ({
        status: { output_account: [faucetAccount] },
        transfers: { items: [
          rawTransfer(5, faucetAccountId, 500_000_000n, true, 'miner'),
        ] },
        candidates: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Jupiter Faucet · matching memo/);
    assert.match(html, /data-source-segment="faucet-memo-1"/);
    assert.doesNotMatch(html, /Jupiter Faucet · other memo/);
    assert.doesNotMatch(html, /Jupiter Relay/);
    assert.doesNotMatch(html, /Protocol canister/);
    assert.doesNotMatch(html, /data-source-segment="faucet-other-memo"/);
    assert.doesNotMatch(html, /data-source-segment="relay"/);
  });
});

test('raw ICP tracker renders revised candidate empty and heading copy', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async () => ({
        status: {},
        transfers: { items: [] },
        candidates: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.miner`;

    await controller.submitPrincipal();

    const emptyHtml = nodeMap.get('tracker-result').innerHTML;
    assert.match(emptyHtml, /If the right-hand side of the memo identifies another canister/);
    assert.match(emptyHtml, /committing 1 ICP with that canister&#39;s full ID in the memo/);
    assert.match(emptyHtml, /href="#how-it-works"[^>]*>How it Works<\/a>/);
    assert.doesNotMatch(emptyHtml, /No possible matching tracked canisters/);

    controller.state.data = {
      status: {},
      transfers: { items: [] },
      candidates: {
        items: [{
          canister_id: Principal.fromText(canister),
          total_qualifying_committed_e8s: 100_000_000n,
        }],
      },
      errors: {},
    };
    controller.state.loadedRange = 'all';
    controller.setRange('all');

    const candidateHtml = nodeMap.get('tracker-result').innerHTML;
    assert.match(candidateHtml, /Tracked canisters matching the memo&#39;s &#39;\.&#39; suffix/);
    assert.doesNotMatch(candidateHtml, /Possible matching tracked canisters/);
  });
});

test('raw ICP tracker treats an empty outgoing memo as present', async () => {
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);
  const relayAccountId = defaultCanisterAccountIdentifier(JUPITER_RELAY_CANISTER_ID);

  const nodes = trackerNodes();
  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
      loadRawCanisterData: async () => ({
        status: { output_account: [faucetAccount] },
        transfers: { items: [
          rawTransfer(5, faucetAccountId, 500_000_000n, true, ''),
          rawTransfer(4, faucetAccountId, 400_000_000n, false, 'treasury'),
          rawTransfer(3, relayAccountId, 300_000_000n, false),
        ] },
        candidates: { items: [] },
        errors: {},
      }),
    });
    controller.bindPane();
    nodeMap.get('tracker-principal-input').value = `${compactCanister}.`;

    await controller.submitPrincipal();

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Raw ICP canister memo/);
    assert.match(html, /Jupiter Faucet · matching memo/);
    assert.match(html, /Jupiter Faucet · treasury/);
    assert.doesNotMatch(html, /Jupiter Faucet · other memo/);
    assert.match(html, /data-source-segment="faucet-memo-1"/);
    assert.match(html, /data-source-segment="faucet-memo-2"/);
    assert.doesNotMatch(html, /data-source-segment="faucet-other-memo"/);
    assert.match(html, /Visible Jupiter Faucet transfers matching the outgoing memo: 1 · 5 ICP/);
    assert.match(html, /Jupiter Faucet · matching memo 5 ICP across 1 transfer/);
    assert.match(html, /<dt>Outgoing memo<\/dt><dd class="pane-detail-value mono"><\/dd>/);
    assert.match(html, /Prefix matching is skipped for short outgoing memos/);
    assert.doesNotMatch(html, /data-source-segment="faucet"/);
  });
});

test('raw ICP tracker uses generic source segments when outgoing memo is absent', async () => {
  const nodes = trackerNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const faucetAccount = { owner: Principal.fromText('aaaaa-aa'), subaccount: [] };
  const faucetAccountId = accountIdentifierHex(faucetAccount);
  const relayAccountId = defaultCanisterAccountIdentifier(JUPITER_RELAY_CANISTER_ID);

  await withFakeTrackerDom(nodes, async ({ nodeMap }) => {
    const controller = createTrackerController({
      frontendConfig: {},
      isLocalHost: () => false,
      simulatorHashForPrefill,
    });
    controller.bindPane();
    controller.state.viewMode = 'rawIcpCanister';
    controller.state.data = {
      status: { output_account: [faucetAccount] },
      transfers: { items: [
        rawTransfer(5, faucetAccountId, 500_000_000n, true),
        rawTransfer(4, faucetAccountId, 400_000_000n, false),
        rawTransfer(3, relayAccountId, 300_000_000n, false),
      ] },
      candidates: { items: [] },
      errors: {},
    };
    controller.state.parsedMemo = {
      kind: 'rawIcpCanister',
      canisterText: canister,
      canisterId: Principal.fromText(canister),
      normalizedMemoText: canister,
      outgoingMemoText: null,
    };

    controller.setRange('all');

    const html = nodeMap.get('tracker-result').innerHTML;
    assert.match(html, /Raw ICP canister memo/);
    assert.match(html, /Jupiter Faucet/);
    assert.match(html, /data-source-segment="faucet"/);
    assert.match(html, /Jupiter Faucet 9 ICP across 2 transfers/);
    assert.doesNotMatch(html, /matching memo/);
    assert.doesNotMatch(html, /other memo/);
    assert.doesNotMatch(html, /data-source-segment="faucet-matching-memo"/);
    assert.doesNotMatch(html, /data-source-segment="faucet-other-memo"/);
    assert.doesNotMatch(html, /Outgoing memo<\/dt>/);
  });
});
