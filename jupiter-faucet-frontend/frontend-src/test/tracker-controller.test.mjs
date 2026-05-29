import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@icp-sdk/core/principal';

import { createTrackerController } from '../src/app/tracker-controller.js';
import { simulatorHashForPrefill } from '../src/app/hash-routes.js';
import { JUPITER_RELAY_CANISTER_ID } from '../src/app/config.js';
import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';
import { defaultCanisterAccountIdentifier } from '../src/data/transfer-source-classification.js';

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

function rawTransfer(id, from, amountE8s, isMatchingMemo = false) {
  return {
    tx_id: BigInt(id),
    timestamp_nanos: [1_700_000_000_000_000_000n + BigInt(id)],
    amount_e8s: BigInt(amountE8s),
    from_account_identifier: from,
    is_matching_memo: isMatchingMemo,
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

    assert.equal(nodeMap.get('tracker-principal-input').value, 'jufzc-caaaa-aaaar-qb5da-cai');
    assert.equal(calls.length, 1);
    assert.equal(calls[0].historianCanisterId, 'hist-aa');
    assert.equal(calls[0].host, 'https://example.test');
    assert.equal(calls[0].local, true);
    assert.equal(calls[0].canisterId.toText(), 'jufzc-caaaa-aaaar-qb5da-cai');
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
    }]);
    assert.deepEqual(historyCalls.map((call) => call.hash), ['#metric-tracker?memo=22255zqaaaaaaasqf6uqcai.miner']);
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
      loadRawCanisterData: async () => ({
        status: { output_account: [faucetAccount] },
        transfers: { items: [
          rawTransfer(5, faucetAccountId, 500_000_000n, true),
          rawTransfer(4, faucetAccountId, 400_000_000n, false),
          rawTransfer(3, relayAccountId, 300_000_000n, false),
          rawTransfer(2, protocolAccountId, 200_000_000n, false),
          rawTransfer(1, otherAccountId, 100_000_000n, true),
        ] },
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
    assert.match(html, /Jupiter Faucet · other memo/);
    assert.match(html, /tracker-chart-bar--source-faucet-matching-memo/);
    assert.match(html, /tracker-chart-bar--source-faucet-other-memo/);
    assert.match(html, /Jupiter Relay/);
    assert.match(html, /Protocol canister/);
    assert.match(html, /Other/);
    assert.match(html, /Visible Jupiter Faucet transfers matching the outgoing memo: 1 · 5 ICP/);
    assert.match(html, /Jupiter Faucet · matching memo 5 ICP across 1 transfer/);
    assert.match(html, /Jupiter Faucet · other memo 4 ICP across 1 transfer/);
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
          rawTransfer(5, faucetAccountId, 500_000_000n, true),
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
    assert.match(html, /data-source-segment="faucet-matching-memo"/);
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
          rawTransfer(5, faucetAccountId, 500_000_000n, true),
          rawTransfer(4, faucetAccountId, 400_000_000n, false),
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
    assert.match(html, /Jupiter Faucet · other memo/);
    assert.match(html, /data-source-segment="faucet-matching-memo"/);
    assert.match(html, /data-source-segment="faucet-other-memo"/);
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
