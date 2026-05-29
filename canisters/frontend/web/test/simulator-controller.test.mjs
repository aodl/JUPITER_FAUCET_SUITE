import test from 'node:test';
import assert from 'node:assert/strict';

import { createSimulatorController } from '../src/app/simulator-controller.js';
import { simulatorHashForPrefill } from '../src/app/hash-routes.js';

class FakeInput {
  constructor(id, value = '') {
    this.id = id;
    this.value = value;
    this.dataset = {};
    this.listeners = new Map();
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }
}

function makeNode(id, value = '') {
  return {
    id,
    value,
    textContent: '',
    innerHTML: '',
    hidden: false,
    className: '',
    title: '',
    dataset: {},
    listeners: new Map(),
    addEventListener(type, listener) {
      this.listeners.set(type, listener);
    },
    removeAttribute(name) {
      if (name === 'title') this.title = '';
    },
  };
}

async function withFakeSimulatorDom(nodes, fn, { hash = '#simulator', href = 'https://example.test/' } = {}) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const originalHistory = globalThis.history;
  const originalHtmlInputElement = globalThis.HTMLInputElement;
  const originalMouseEvent = globalThis.MouseEvent;
  const nodeMap = new Map(nodes.map((node) => [node.id, node]));
  const clickedPanels = [];
  let replacedHash = '';

  globalThis.HTMLInputElement = FakeInput;
  globalThis.MouseEvent = class {
    constructor(type) {
      this.type = type;
    }
  };
  globalThis.document = {
    body: {
      classList: {
        contains: () => false,
      },
    },
    getElementById(id) {
      return nodeMap.get(id) || null;
    },
    querySelector(selector) {
      if (selector === 'a[data-panel="simulator"]') {
        return {
          dispatchEvent(event) {
            clickedPanels.push(event.type);
          },
        };
      }
      return null;
    },
  };
  globalThis.window = {
    location: { hash, href },
    setTimeout(callback) {
      callback();
      return 1;
    },
  };
  globalThis.history = {
    replaceState(_state, _title, nextHash) {
      replacedHash = nextHash;
      window.location.hash = nextHash;
    },
  };

  try {
    await fn({ nodeMap, clickedPanels, replacedHash: () => replacedHash });
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
    globalThis.history = originalHistory;
    globalThis.HTMLInputElement = originalHtmlInputElement;
    globalThis.MouseEvent = originalMouseEvent;
  }
}

function simulatorNodes() {
  return [
    makeNode('commitment-simulator-form'),
    new FakeInput('simulator-icp-commitment', ''),
    new FakeInput('simulator-daily-burn', ''),
    new FakeInput('simulator-icp-price', ''),
    new FakeInput('simulator-apy', ''),
    makeNode('simulator-copy-url', 'Copy to URL'),
    makeNode('simulator-status'),
    makeNode('simulator-assumption-note'),
    makeNode('simulator-chart-wrapper'),
    makeNode('simulator-cycles-per-icp'),
    makeNode('simulator-icp-xdr-source'),
    makeNode('simulator-annual-topup-icp'),
    makeNode('simulator-annual-topup-cycles'),
    makeNode('simulator-annual-burn-cycles'),
    makeNode('simulator-age-bonus'),
    makeNode('simulator-effective-apy'),
    makeNode('simulator-year-end-balance'),
    makeNode('simulator-required-commitment'),
  ];
}

test('simulator controller sanitizes inputs without blocking intermediate empty edits', async () => {
  const nodes = simulatorNodes();
  await withFakeSimulatorDom(nodes, async ({ nodeMap }) => {
    const controller = createSimulatorController({
      copyTextToClipboard: async () => {},
      neuronId: 123n,
    });
    controller.bind();
    const form = nodeMap.get('commitment-simulator-form');
    const burn = nodeMap.get('simulator-daily-burn');

    burn.value = 'abc';
    form.listeners.get('input')({ target: burn });
    assert.equal(burn.value, '');

    burn.value = '0001.23456';
    form.listeners.get('input')({ target: burn });
    assert.equal(burn.value, '1.2345');
  });
});

test('simulator XDR prefill does not overwrite after the user edits price', async () => {
  const nodes = simulatorNodes();
  await withFakeSimulatorDom(nodes, async ({ nodeMap }) => {
    const controller = createSimulatorController({
      copyTextToClipboard: async () => {},
      neuronId: 123n,
    });
    controller.bind();
    const form = nodeMap.get('commitment-simulator-form');
    const price = nodeMap.get('simulator-icp-price');

    controller.applyIcpXdrRateFromStatus({ icp_xdr_rate: [{ rate: 42n, decimals: 1n }] });
    assert.equal(price.value, '4.2');

    price.value = '9.9';
    form.listeners.get('input')({ target: price });
    controller.applyIcpXdrRateFromStatus({ icp_xdr_rate: [{ rate: 11n, decimals: 1n }] });
    assert.equal(price.value, '9.9');
  });
});

test('simulator share button replaces the hash and copies the full URL', async () => {
  const nodes = simulatorNodes();
  const copied = [];
  await withFakeSimulatorDom(nodes, async ({ nodeMap, replacedHash }) => {
    const controller = createSimulatorController({
      copyTextToClipboard: async (value) => {
        copied.push(value);
      },
      neuronId: 123n,
    });
    controller.bind();
    nodeMap.get('simulator-icp-commitment').value = '12.3';
    nodeMap.get('simulator-daily-burn').value = '0.0042';
    nodeMap.get('simulator-icp-price').value = '5.5';
    nodeMap.get('simulator-apy').value = '8.1';

    await nodeMap.get('simulator-copy-url').listeners.get('click')();

    assert.equal(replacedHash(), '#simulator-burn=0.0042&commitment=12.3&price=5.5&apy=8.1');
    assert.equal(copied[0], 'https://example.test/#simulator-burn=0.0042&commitment=12.3&price=5.5&apy=8.1');
  });
});

test('simulator hash prefill opens the panel and applies all shared inputs', async () => {
  const nodes = simulatorNodes();
  const hash = simulatorHashForPrefill({
    dailyBurn: '0.12345',
    icpCommitment: '7.77',
    assumedIcpPrice: '8.88',
    annualApyPercent: '9.99',
  });

  await withFakeSimulatorDom(nodes, async ({ nodeMap, clickedPanels }) => {
    const controller = createSimulatorController({
      copyTextToClipboard: async () => {},
      neuronId: 123n,
    });
    controller.bind();
    assert.equal(controller.hydrateFromLocationHash(), true);

    assert.deepEqual(clickedPanels, ['click']);
    assert.equal(nodeMap.get('simulator-daily-burn').value, '0.1234');
    assert.equal(nodeMap.get('simulator-icp-commitment').value, '7.7');
    assert.equal(nodeMap.get('simulator-icp-price').value, '8.8');
    assert.equal(nodeMap.get('simulator-apy').value, '9.9');
  }, { hash });
});
