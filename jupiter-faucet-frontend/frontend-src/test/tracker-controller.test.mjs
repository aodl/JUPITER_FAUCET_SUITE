import test from 'node:test';
import assert from 'node:assert/strict';

import { createTrackerController } from '../src/app/tracker-controller.js';
import { simulatorHashForPrefill } from '../src/app/hash-routes.js';

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

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
}

async function withFakeTrackerDom(nodes, fn, { hash = '', rangeButtons = [] } = {}) {
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
        contains: () => false,
      },
    },
    getElementById(id) {
      return nodeMap.get(id) || null;
    },
    querySelector(selector) {
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
      sources: [{ Commitment: null }],
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

    assert.equal(nodeMap.get('tracker-principal-input').value, 'aaaaa-aa');
    assert.equal(calls.length, 1);
    assert.equal(calls[0].historianCanisterId, 'hist-aa');
    assert.equal(calls[0].host, 'https://example.test');
    assert.equal(calls[0].local, true);
    assert.equal(calls[0].canisterId.toText(), 'aaaaa-aa');
  }, { hash: '#metric-tracker-aaaaa-aa' });
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
    assert.equal(nodeMap.get('tracker-status').textContent, 'Enter a valid declared canister ID.');
    assert.equal(nodeMap.get('tracker-principal-input').focused, true);
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
    nodeMap.get('tracker-principal-input').value = 'aaaaa-aa';
    await controller.submitPrincipal();

    controller.setRange('month');

    assert.equal(controller.state.range, 'month');
    assert.match(nodeMap.get('tracker-result').innerHTML, /Latest Month/);
    assert.match(nodeMap.get('tracker-result').innerHTML, /Showing last month/);
    assert.deepEqual(monthButton.classList.toggled.at(-1), { name: 'is-active', active: true });
    assert.deepEqual(allButton.classList.toggled.at(-1), { name: 'is-active', active: false });
  }, { rangeButtons: [monthButton, allButton] });
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
    const trigger = new FakeElement({ 'data-tracker-principal': 'aaaaa-aa' });

    documentListeners.get('click')({
      target: trigger,
      preventDefault() {},
      stopPropagation() {},
    });
    await flushMicrotasks();

    assert.deepEqual(clickedPanels, ['click']);
    assert.equal(nodeMap.get('tracker-principal-input').value, 'aaaaa-aa');
    assert.deepEqual(calls, ['aaaaa-aa']);
  });
});
