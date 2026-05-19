import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createDashboardTablesController } from '../src/app/dashboard-tables-controller.js';
import { simulatorPrefillFromHash } from '../src/app/hash-routes.js';
import { clampSimulatorInputValue } from '../src/app/simulator-controller.js';
import { createSourcePaneController } from '../src/app/source-pane-controller.js';
import {
  renderCanisterDashboardLink,
  renderCanisterTrackerLink,
  formatSourceController,
} from '../src/app/view-formatters.js';
import { setPaneValueTrustedHtml } from '../src/dom-helpers.js';
import { formatFolloweeLinks } from '../src/followee-links.js';

const XSS_PAYLOADS = [
  '<img src=x onerror=alert(1)>',
  '"><svg/onload=alert(1)>',
  'javascript:alert(1)',
  '</a><script>alert(1)</script>',
  '<a onclick="alert(1)">x</a>',
  '<math href="javascript:alert(1)">',
  '<iframe srcdoc="<script>alert(1)</script>">',
  'x" autofocus onfocus="alert(1)',
];

function assertNoExecutableHtml(html) {
  assert.doesNotMatch(html, /<script\b/i);
  assert.doesNotMatch(html, /<img\b/i);
  assert.doesNotMatch(html, /<svg\b/i);
  assert.doesNotMatch(html, /<iframe\b/i);
  assert.doesNotMatch(html, /\s(?:href|src|srcdoc)\s*=\s*(['"])\s*javascript:/i);
  assert.equal(hasUnsafeTagAttribute(html), false);
}

function hasUnsafeTagAttribute(html) {
  for (const tag of html.matchAll(/<\s*[a-z][^>]*>/gi)) {
    let quote = null;
    let outsideQuotes = '';
    for (const char of tag[0]) {
      if (quote) {
        if (char === quote) quote = null;
      } else if (char === '"' || char === "'") {
        quote = char;
      } else {
        outsideQuotes += char;
      }
    }
    if (/\son[a-z0-9_-]+\s*=/i.test(outsideQuotes)) return true;
    if (/\b(?:href|src|srcdoc)\s*=\s*javascript:/i.test(outsideQuotes)) return true;
  }
  return false;
}

function listJsFiles(root) {
  return readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) return listJsFiles(path);
    return path.endsWith('.js') ? [path] : [];
  });
}

function makeSourceNode(attributeName, canisterId) {
  const attrs = new Map([[attributeName, canisterId]]);
  return {
    textContent: '',
    innerHTML: '',
    title: '',
    hasAttribute(name) {
      return attrs.has(name);
    },
    getAttribute(name) {
      return attrs.get(name) || null;
    },
    setAttribute(name, value) {
      if (name === 'title') this.title = value;
      attrs.set(name, value);
    },
    removeAttribute(name) {
      if (name === 'title') this.title = '';
      attrs.delete(name);
    },
  };
}

async function withSourcePaneBrowser({ nodes, storageEntries = [] }, fn) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const storage = new Map(storageEntries);
  globalThis.document = {
    querySelectorAll(selector) {
      if (selector === '[data-source-module-hash]') return nodes.filter((node) => node.hasAttribute('data-source-module-hash'));
      if (selector === '[data-source-controllers]') return nodes.filter((node) => node.hasAttribute('data-source-controllers'));
      if (selector === '[data-source-memory]') return [];
      return [];
    },
  };
  globalThis.window = {
    location: { origin: 'https://example.test' },
    localStorage: {
      getItem(key) {
        return storage.get(key) || null;
      },
      setItem(key, value) {
        storage.set(key, value);
      },
    },
  };
  try {
    await fn();
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
  }
}

function makeStatusNode() {
  return {
    hidden: false,
    className: '',
    classList: { add() {} },
    textContent: '',
    title: '',
    ariaLabel: '',
    removeAttribute(name) {
      if (name === 'title') this.title = '';
      if (name === 'aria-label') this.ariaLabel = '';
    },
    setAttribute(name, value) {
      if (name === 'aria-label') this.ariaLabel = value;
    },
  };
}

function withDocumentGetElementById(nodes, fn) {
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

test('trusted followee HTML remains escaped before it reaches setPaneValueTrustedHtml', () => {
  for (const payload of XSS_PAYLOADS) {
    const valueNode = { textContent: '', innerHTML: '' };
    const statusNode = makeStatusNode();
    const neuron = {
      followees: [[0, { followees: [{ id: { toString: () => payload } }] }]],
    };
    const trustedFolloweeHtml = formatFolloweeLinks(neuron);

    withDocumentGetElementById(new Map([
      ['stake-neuron-followees', valueNode],
      ['stake-neuron-followees-status', statusNode],
    ]), () => {
      setPaneValueTrustedHtml('stake-neuron-followees', {
        value: trustedFolloweeHtml,
        error: payload,
      });
    });

    assertNoExecutableHtml(valueNode.innerHTML);
    assert.equal(statusNode.title, payload);
    assert.equal(statusNode.ariaLabel, payload);
  }
});

test('principal and controller renderers escape untrusted labels and links', () => {
  for (const payload of XSS_PAYLOADS) {
    const trackerHtml = renderCanisterTrackerLink(payload, { label: payload, className: payload });
    const dashboardHtml = renderCanisterDashboardLink(payload, payload);
    const controllerHtml = formatSourceController(payload);
    assertNoExecutableHtml(trackerHtml);
    assertNoExecutableHtml(dashboardHtml);
    assertNoExecutableHtml(controllerHtml);
  }
});

test('source pane cached localStorage data cannot inject controller HTML', async () => {
  for (const payload of XSS_PAYLOADS) {
    const controllers = makeSourceNode('data-source-controllers', 'aaaaa-aa');
    const cacheKey = 'jupiter-faucet:source-pane-canister-info:v4:hist-aa';
    const cached = {
      cachedAt: Date.now(),
      infoByCanisterId: {
        'aaaaa-aa': {
          moduleHash: payload,
          controllers: [payload],
        },
      },
    };

    await withSourcePaneBrowser({
      nodes: [controllers],
      storageEntries: [[cacheKey, JSON.stringify(cached)]],
    }, async () => {
      const controller = createSourcePaneController({
        frontendConfig: { historianCanisterId: 'hist-aa' },
        isLocalHost: () => false,
        loadCanisterInfo: async () => {
          throw new Error('loader should not run');
        },
      });
      await controller.ensureLoaded();
    });

    assertNoExecutableHtml(controllers.innerHTML);
  }
});

test('historian-provided memo text and table errors are escaped before table innerHTML writes', () => {
  for (const payload of XSS_PAYLOADS) {
    const bodies = new Map([
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
    const data = {
      recent: {
        items: [
          {
            canister_id: [payload],
            raw_icp_memo_text: [payload],
            tx_id: payload,
            timestamp_nanos: [1_700_000_000_000_000_000n],
            amount_e8s: 100_000_000n,
          },
          {
            neuron_id: [payload],
            neuron_memo_text: [payload],
            tx_id: payload,
            timestamp_nanos: [1_700_000_000_000_000_000n],
            amount_e8s: 100_000_000n,
          },
        ],
      },
      errors: {
        recent: payload,
      },
    };

    withDocumentGetElementById(bodies, () => {
      createDashboardTablesController({
        frontendConfig: {},
        isLocalHost: () => false,
        getLandingData: () => data,
      }).renderCommitmentsPane(data);
    });

    assertNoExecutableHtml(bodies.get('commitments-pane-body').innerHTML);
    assertNoExecutableHtml(bodies.get('commitments-raw-pane-body').innerHTML);
    assertNoExecutableHtml(bodies.get('commitments-neurons-pane-body').innerHTML);
    assert.equal(bodies.get('commitments-page-info').textContent, 'Page 1 of 1');
  }
});

test('hash route simulator prefill payloads are clamped before entering simulator inputs', () => {
  for (const payload of XSS_PAYLOADS) {
    const prefill = simulatorPrefillFromHash(`#simulator-burn=${encodeURIComponent(payload)}&commitment=${encodeURIComponent(payload)}&price=${encodeURIComponent(payload)}&apy=${encodeURIComponent(payload)}`);
    assert.equal(clampSimulatorInputValue(prefill.dailyBurn, { min: 0, fractionDigits: 4 }), '1');
    assert.equal(clampSimulatorInputValue(prefill.icpCommitment, { min: 1, fractionDigits: 1 }), '1');
    assert.equal(clampSimulatorInputValue(prefill.assumedIcpPrice, { min: 0.1, fractionDigits: 1 }), '1');
    assert.equal(clampSimulatorInputValue(prefill.annualApyPercent, { min: 0, fractionDigits: 1 }), '1');
  }
});

test('raw HTML sinks stay inventoried for XSS review coverage', () => {
  const srcRoot = fileURLToPath(new URL('../src', import.meta.url));
  const sinkPattern = /\.innerHTML\s*=|\.insertAdjacentHTML\s*\(|setPaneValueTrustedHtml\s*\(/;
  const sinks = listJsFiles(srcRoot).flatMap((path) => readFileSync(path, 'utf8')
    .split('\n')
    .filter((line) => sinkPattern.test(line))
    .map((line) => `${relative(srcRoot, path)}:${line.trim()}`))
    .sort();

  assert.deepEqual(sinks, [
    "app/bootstrap.js:popover.innerHTML = `",
    'app/bootstrap.js:textNode.innerHTML = content;',
    'app/dashboard-tables-controller.js:body.innerHTML = items.length',
    'app/dashboard-tables-controller.js:body.innerHTML = pageItems.length',
    'app/dashboard-tables-controller.js:node.innerHTML = `Jupiter neuron maturity is disbursed to the controlling canister\'s ${stagingLink}. ${escapeHtml(routeLabel)} counts ICP routed from that staging account to ${destination}. ${aggregate ? \'Historian tracks the aggregate; \' : \'\'}recent rows are fetched directly from the ICP index canister.`;',
    'app/simulator-controller.js:assumption.innerHTML = `Projection uses the configured APY. Exact APY depends on numerous factors — consult the <a class="pane-external-link" href="${dashboardHref}" target="_blank" rel="noopener noreferrer">dashboard</a> for the current annualised rewards estimate. ${ageBonusCopy} ${rateCopy} It assumes 1T cycles per ICP/XDR price unit and a weekly-cadence one-year projection.`;',
    "app/simulator-controller.js:if (wrapper) wrapper.innerHTML = renderEmptyChart('Enter valid simulator inputs to render the projection.');",
    'app/simulator-controller.js:wrapper.innerHTML = `',
    'app/source-pane-controller.js:node.innerHTML = renderSourceControllers(controllers);',
    'app/stake-pane-controller.js:setPaneValueTrustedHtml(\'stake-neuron-followees\', { value: formatFolloweeLinks(neuron) });',
    "app/tracker-controller.js:result.innerHTML = '<div class=\"tracker-empty-state\"><p>Loading…</p></div>';",
    "app/tracker-controller.js:result.innerHTML = '<div class=\"tracker-empty-state\"><p>Tracker data could not be loaded right now.</p></div>';",
    'app/tracker-controller.js:result.innerHTML = `',
    'app/tracker-controller.js:result.innerHTML = `',
    'app/tracker-controller.js:result.innerHTML = `',
    'app/tracker-controller.js:wrapper.innerHTML = `',
    'app/tracker-controller.js:wrapper.innerHTML = renderTrackerEmptyChart(message);',
    'dom-helpers.js:export function setPaneValueTrustedHtml(id, { value = null, loading = false, error = null } = {}) {',
    "dom-helpers.js:if (valueNode) valueNode.innerHTML = value ?? '';",
  ]);
});
