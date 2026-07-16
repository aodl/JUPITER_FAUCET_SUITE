import test from 'node:test';
import assert from 'node:assert/strict';

import { createDashboardTablesController } from '../src/app/dashboard-tables-controller.js';

function withDashboardTableDom(nodes, fn) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  globalThis.document = {
    getElementById(id) {
      return nodes.get(id) || null;
    },
  };
  globalThis.window = { innerHeight: 900 };
  try {
    fn();
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
  }
}

function commitmentPaneNodes() {
  return new Map([
    ['commitments-pane-body', { innerHTML: '' }],
    ['commitments-raw-pane-body', { innerHTML: '' }],
    ['commitments-neurons-pane-body', { innerHTML: '' }],
    ['commitments-page-info', { textContent: '' }],
    ['commitments-raw-page-info', { textContent: '' }],
    ['commitments-neurons-page-info', { textContent: '' }],
    ['commitments-prev-page', { disabled: false }],
    ['commitments-next-page', { disabled: false }],
    ['commitments-raw-prev-page', { disabled: false }],
    ['commitments-raw-next-page', { disabled: false }],
    ['commitments-neurons-prev-page', { disabled: false }],
    ['commitments-neurons-next-page', { disabled: false }],
  ]);
}

function registeredPaneNodes() {
  return new Map([
    ['registered-pane-body', { innerHTML: '' }],
    ['registered-page-info', { textContent: '' }],
    ['registered-prev-page', { disabled: false }],
    ['registered-next-page', { disabled: false }],
  ]);
}

test('raw and neuron commitment rows link full declared memos to tracker', () => {
  const nodes = commitmentPaneNodes();
  const canister = '22255-zqaaa-aaaas-qf6uq-cai';
  const compactCanister = canister.replaceAll('-', '');
  const data = {
    recent: {
      items: [
        {
          canister_id: [canister],
          raw_icp_memo_text: ['vault'],
          tx_id: 6n,
          timestamp_nanos: [1_700_000_000_000_000_000n],
          amount_e8s: 200_000_000n,
        },
        {
          neuron_id: [123456789n],
          neuron_memo_text: ['donor'],
          tx_id: 7n,
          timestamp_nanos: [1_700_000_000_000_000_000n],
          amount_e8s: 100_000_000n,
        },
        {
          neuron_id: [123n],
          neuron_memo_text: [''],
          tx_id: 8n,
          timestamp_nanos: [1_700_000_000_000_000_000n],
          amount_e8s: 100_000_000n,
        },
      ],
    },
    errors: {},
  };

  withDashboardTableDom(nodes, () => {
    createDashboardTablesController({
      frontendConfig: {},
      isLocalHost: () => false,
      getLandingData: () => data,
    }).renderCommitmentsPane(data);
  });

  const rawHtml = nodes.get('commitments-raw-pane-body').innerHTML;
  assert.match(rawHtml, new RegExp(`href="#metric-tracker\\?memo=${compactCanister}\\.vault"`));
  assert.match(rawHtml, new RegExp(`data-tracker-memo="${compactCanister}\\.vault"`));
  assert.doesNotMatch(rawHtml, new RegExp(`${canister}\\.vault`));
  assert.doesNotMatch(rawHtml, />vault<\/td>/);

  const neuronHtml = nodes.get('commitments-neurons-pane-body').innerHTML;
  assert.match(neuronHtml, /href="#metric-tracker\?memo=123456789\.donor"/);
  assert.match(neuronHtml, /data-tracker-memo="123456789\.donor"/);
  assert.match(neuronHtml, /href="#metric-tracker\?memo=123\."/);
  assert.match(neuronHtml, /data-tracker-memo="123\."/);
  assert.doesNotMatch(neuronHtml, /href="#metric-tracker\?memo=123"/);
  assert.doesNotMatch(neuronHtml, /data-tracker-memo="123"/);
  assert.doesNotMatch(neuronHtml, /dashboard\.internetcomputer\.org\/neuron\/123456789/);
  assert.doesNotMatch(neuronHtml, />donor<\/td>/);
});

test('raw ICP declared memo links keep compact canister memo text within the memo limit', () => {
  const nodes = commitmentPaneNodes();
  const data = {
    recent: {
      items: [{
        canister_id: ['r5m5y-diaaa-aaaaa-qanaa-cai'],
        raw_icp_memo_text: ['2r3eo-5q'],
        tx_id: 8n,
        timestamp_nanos: [1_700_000_000_000_000_000n],
        amount_e8s: 100_000_000n,
      }],
    },
    errors: {},
  };

  withDashboardTableDom(nodes, () => {
    createDashboardTablesController({
      frontendConfig: {},
      isLocalHost: () => false,
      getLandingData: () => data,
    }).renderCommitmentsPane(data);
  });

  const html = nodes.get('commitments-raw-pane-body').innerHTML;
  assert.match(html, /r5m5ydiaaaaaaaaqanaacai\.2r3eo-5q/);
  assert.match(html, /data-tracker-memo="r5m5ydiaaaaaaaaqanaacai\.2r3eo-5q"/);
  assert.doesNotMatch(html, /r5m5y-diaaa-aaaaa-qanaa-cai\.2r3eo-5q/);
});

test('declared canister table uses memo-registered unavailable and empty wording', () => {
  const emptyNodes = registeredPaneNodes();
  withDashboardTableDom(emptyNodes, () => {
    createDashboardTablesController({
      frontendConfig: {},
      isLocalHost: () => false,
      getLandingData: () => ({}),
    }).renderRegisteredPane({ errors: {} });
  });

  assert.match(
    emptyNodes.get('registered-pane-body').innerHTML,
    /No memo-registered canisters indexed yet\./
  );
  assert.doesNotMatch(emptyNodes.get('registered-pane-body').innerHTML, /tracked canisters/i);

  const errorNodes = registeredPaneNodes();
  withDashboardTableDom(errorNodes, () => {
    createDashboardTablesController({
      frontendConfig: {},
      isLocalHost: () => false,
      getLandingData: () => ({}),
    }).renderRegisteredPane({ errors: { registered: 'index unavailable' } });
  });

  assert.match(
    errorNodes.get('registered-pane-body').innerHTML,
    /Declared canisters unavailable \(index unavailable\)/
  );
  assert.doesNotMatch(errorNodes.get('registered-pane-body').innerHTML, /Tracked canisters unavailable/);
});
