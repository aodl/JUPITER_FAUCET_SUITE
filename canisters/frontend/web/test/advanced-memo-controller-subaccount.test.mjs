import test from 'node:test';
import assert from 'node:assert/strict';

import { initAdvancedMemoBuilder } from '../src/app/advanced-memo-controller.js';

const OWNER_ONE = '22255-zqaaa-aaaas-qf6uq-cai';
const OWNER_TWO = 'u2qkp-aqaaa-aaaar-qb7ea-cai';
const OWNER_ONE_MEMO = '22255zqaaaaaaasqf6uqcai';
const OWNER_TWO_MEMO = 'u2qkpaqaaaaaaarqb7eacai';

class FakeElement extends EventTarget {
  constructor(id = '') {
    super();
    this.id = id;
    this.value = '';
    this.textContent = '';
    this.hidden = false;
    this.disabled = false;
    this.readOnly = false;
    this.className = '';
    this.dataset = {};
    this.children = [];
    this.attributes = new Map();
    this.isContentEditable = false;
  }

  contains(node) {
    return node === this || this.children.includes(node);
  }

  append(node) {
    this.children.push(node);
    this.textContent += node.textContent || '';
  }

  replaceChildren(...nodes) {
    this.children = [];
    this.textContent = '';
    for (const node of nodes) this.append(node);
  }

  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }

  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
}

function memoBuilderDom(initialHash) {
  const ids = [
    'advanced-memo-builder',
    'memo-builder-mode-fieldset',
    'memo-builder-canister-fields',
    'memo-builder-neuron-fields',
    'memo-builder-raw-subaccount-fields',
    'memo-builder-optional-fields',
    'memo-builder-canister',
    'memo-builder-neuron',
    'memo-builder-raw-subaccount',
    'memo-builder-optional',
    'memo-builder-optional-label',
    'memo-builder-title',
    'memo-builder-prefill-note',
    'memo-builder-safety-notice',
    'memo-builder-safety-target-kind',
    'memo-builder-safety-prescription-kind',
    'memo-builder-canister-dashboard-link',
    'memo-builder-url-context',
    'memo-builder-output',
    'memo-builder-copy',
    'memo-builder-tip-url',
    'copy-memo-builder-tip-url',
    'memo-builder-messages',
  ];
  const nodes = new Map(ids.map((id) => [id, new FakeElement(id)]));
  const builder = nodes.get('advanced-memo-builder');
  const radios = new Map(['rawIcp', 'neuron', 'cycles'].map((value) => [value, { value, checked: value === 'cycles' }]));
  builder.querySelector = (selector) => {
    const requested = selector.match(/value="([^"]+)"/)?.[1];
    if (requested) return radios.get(requested) || null;
    if (selector === 'input[name="memo-builder-mode"]:checked') {
      return [...radios.values()].find((radio) => radio.checked) || null;
    }
    return null;
  };
  for (const radio of radios.values()) {
    let checked = radio.checked;
    Object.defineProperty(radio, 'checked', {
      get: () => checked,
      set(value) {
        checked = Boolean(value);
        if (checked) {
          for (const other of radios.values()) {
            if (other !== radio) other.checked = false;
          }
        }
      },
    });
  }
  nodes.get('memo-builder-optional-label').textContent = 'Optional outgoing transfer memo';
  nodes.get('memo-builder-title').textContent = 'Memo Builder';
  nodes.get('memo-builder-copy').textContent = 'Copy memo';

  const fakeWindow = new EventTarget();
  fakeWindow.location = { hash: initialHash, href: `https://example.test/${initialHash}` };
  fakeWindow.setTimeout = (callback) => {
    callback();
    return 1;
  };
  const fakeDocument = new EventTarget();
  fakeDocument.getElementById = (id) => nodes.get(id) || null;
  fakeDocument.createElement = () => new FakeElement();
  fakeDocument.execCommand = () => true;

  return { nodes, builder, window: fakeWindow, document: fakeDocument };
}

async function withMemoBuilder(initialHash, run) {
  const originalWindow = globalThis.window;
  const originalDocument = globalThis.document;
  const env = memoBuilderDom(initialHash);
  globalThis.window = env.window;
  globalThis.document = env.document;
  try {
    initAdvancedMemoBuilder();
    await run({
      ...env,
      navigate(hash, eventType = 'hashchange') {
        env.window.location.hash = hash;
        env.window.location.href = `https://example.test/${hash}`;
        env.window.dispatchEvent(new Event(eventType));
      },
      renderFromBuilder(eventType = 'input') {
        env.builder.dispatchEvent(new Event(eventType));
      },
      renderFromDocument(eventType = 'navpanel:open') {
        env.document.dispatchEvent(new Event(eventType));
      },
    });
  } finally {
    globalThis.window = originalWindow;
    globalThis.document = originalDocument;
  }
}

function assertDefaultRoute(nodes, memoPrefix) {
  assert.equal(nodes.get('memo-builder-raw-subaccount').value, '');
  assert.equal(nodes.get('memo-builder-raw-subaccount').readOnly, false);
  assert.equal(nodes.get('memo-builder-output').value, `${memoPrefix}.`);
  assert.equal(nodes.get('memo-builder-copy').disabled, false);
}

test('same-owner owner-only prefill clears an explicit numbered destination', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}&subaccount=50`, ({ nodes, navigate }) => {
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}:50.`);
    navigate(`#memo-builder?canister=${OWNER_ONE}`);
    assertDefaultRoute(nodes, OWNER_ONE_MEMO);
  });
});

test('different-owner owner-only prefill changes owner and clears the numbered destination', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}&subaccount=50`, ({ nodes, navigate }) => {
    navigate(`#memo-builder?canister=${OWNER_TWO}`);
    assert.equal(nodes.get('memo-builder-canister').value, OWNER_TWO);
    assertDefaultRoute(nodes, OWNER_TWO_MEMO);
  });
});

test('owner-only prefill clears a manually entered numbered destination', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}`, ({ nodes, navigate, renderFromBuilder }) => {
    nodes.get('memo-builder-raw-subaccount').value = '50';
    renderFromBuilder();
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}:50.`);
    navigate(`#memo-builder?canister=${OWNER_TWO}`);
    assertDefaultRoute(nodes, OWNER_TWO_MEMO);
  });
});

test('popstate restores the owner-only default route from a numbered prefill', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}`, ({ nodes, navigate }) => {
    navigate(`#memo-builder?canister=${OWNER_ONE}&subaccount=50`);
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}:50.`);
    navigate(`#memo-builder?canister=${OWNER_ONE}`, 'popstate');
    assertDefaultRoute(nodes, OWNER_ONE_MEMO);
  });
});

test('unchanged owner-only fragment preserves manual number across repeated renders', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}`, ({ nodes, renderFromBuilder, renderFromDocument }) => {
    nodes.get('memo-builder-raw-subaccount').value = '50';
    renderFromBuilder('input');
    renderFromBuilder('change');
    renderFromDocument('navpanel:open');
    renderFromDocument('navpanel:pagechange');
    assert.equal(nodes.get('memo-builder-raw-subaccount').value, '50');
    assert.equal(nodes.get('memo-builder-raw-subaccount').readOnly, false);
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}:50.`);
  });
});

test('unchanged explicit numbered prefill preserves its value and read-only lock', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}&subaccount=50`, ({ nodes, renderFromBuilder, renderFromDocument }) => {
    renderFromBuilder('input');
    renderFromBuilder('change');
    renderFromDocument('navpanel:open');
    renderFromDocument('navpanel:pagechange');
    assert.equal(nodes.get('memo-builder-raw-subaccount').value, '50');
    assert.equal(nodes.get('memo-builder-raw-subaccount').readOnly, true);
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}:50.`);
  });
});

test('explicit zero after 50 selects the default route without retaining 50', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}&subaccount=50`, ({ nodes, navigate }) => {
    navigate(`#memo-builder?canister=${OWNER_ONE}&subaccount=0`);
    assert.equal(nodes.get('memo-builder-raw-subaccount').value, '0');
    assert.equal(nodes.get('memo-builder-raw-subaccount').readOnly, true);
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}.`);
    assert.equal(nodes.get('memo-builder-copy').disabled, false);
  });
});

test('explicit empty subaccount after 50 selects the default route without retaining 50', async () => {
  await withMemoBuilder(`#memo-builder?canister=${OWNER_ONE}&subaccount=50`, ({ nodes, navigate }) => {
    navigate(`#memo-builder?canister=${OWNER_ONE}&subaccount=`);
    assert.equal(nodes.get('memo-builder-raw-subaccount').value, '');
    assert.equal(nodes.get('memo-builder-raw-subaccount').readOnly, true);
    assert.equal(nodes.get('memo-builder-output').value, `${OWNER_ONE_MEMO}.`);
    assert.equal(nodes.get('memo-builder-copy').disabled, false);
  });
});
