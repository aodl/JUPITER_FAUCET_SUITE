import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';

import {
  createRelaySetupController,
  duplicateRelayTargetIndexes,
  icrcAccountText,
  parseRelayTargetSet,
} from '../src/app/relay-setup-controller.js';
import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';

const TARGET_A = '22255-zqaaa-aaaas-qf6uq-cai';
const TARGET_B = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
const HISTORIAN = 'j5gs6-uiaaa-aaaar-qb5cq-cai';
const LEDGER = 'ryjl3-tyaaa-aaaaa-aaaba-cai';
const RELAY = 'br5f7-7uaaa-aaaaa-qaaca-cai';

class FakeElement {
  constructor(id) {
    this.id = id;
    this.dataset = {};
    this.listeners = new Map();
    this.attributes = new Map();
    this.textContent = '';
    this.innerHTML = '';
    this.value = '';
    this.hidden = false;
    this.disabled = false;
    this.focused = false;
    this.href = '';
    this.title = '';
    this.className = '';
    this.children = [];
    this.parentElement = null;
    this.tagName = 'div';
    this.classList = {
      toggle: (name, enabled) => {
        const classes = new Set(this.className.split(/\s+/u).filter(Boolean));
        if (enabled) classes.add(name);
        else classes.delete(name);
        this.className = [...classes].join(' ');
      },
    };
  }

  addEventListener(type, listener) { this.listeners.set(type, listener); }
  focus() { this.focused = true; }
  removeAttribute(name) { this.attributes.delete(name); this[name] = ''; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  append(...children) {
    children.forEach((child) => {
      child.parentElement = this;
      this.children.push(child);
    });
  }
  remove() {
    if (!this.parentElement) return;
    this.parentElement.children = this.parentElement.children.filter((child) => child !== this);
    this.parentElement = null;
  }
  matches(selector) {
    const dataAttribute = selector.match(/^\[data-([a-z0-9-]+)\]$/u)?.[1];
    if (!dataAttribute) return false;
    const key = dataAttribute.replace(/-([a-z])/gu, (_match, letter) => letter.toUpperCase());
    return key in this.dataset;
  }
  closest(selector) {
    for (let node = this; node; node = node.parentElement) {
      if (node.matches(selector)) return node;
    }
    return null;
  }
  querySelectorAll(selector) {
    const matches = [];
    const visit = (node) => {
      node.children.forEach((child) => {
        if (child.matches(selector)) matches.push(child);
        visit(child);
      });
    };
    visit(this);
    return matches;
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
}

const DOM_IDS = [
  'relay-setup-form', 'relay-setup-target-input', 'relay-setup-result',
  'relay-setup-summary', 'relay-setup-status', 'relay-setup-status-label',
  'relay-setup-factory', 'relay-setup-target-count', 'relay-setup-canonical-targets',
  'relay-setup-base-minimum', 'relay-setup-extra-count', 'relay-setup-extra-unit',
  'relay-setup-extra-total', 'relay-setup-minimum', 'relay-setup-requirement',
  'relay-setup-balance', 'relay-setup-icrc-account', 'relay-setup-account-identifier',
  'relay-setup-payment-details', 'relay-setup-create-panel', 'relay-setup-create',
  'relay-setup-existing-relay', 'copy-relay-setup-icrc-account',
  'copy-relay-setup-account-identifier', 'relay-setup-icrc-account-link',
  'relay-setup-account-identifier-link',
  'relay-setup-target-list', 'relay-setup-add-target', 'relay-setup-target-count-hint',
  'relay-setup-target-announcement', 'relay-setup-warning', 'relay-setup-submit',
];

async function withDom(run) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const nodes = new Map(DOM_IDS.map((id) => [id, new FakeElement(id)]));
  globalThis.document = {
    getElementById: (id) => nodes.get(id) || null,
    createElement: () => new FakeElement(''),
  };
  globalThis.window = {
    location: { origin: 'https://example.test' },
    setInterval: () => 1,
    clearInterval() {},
  };
  try { await run(nodes); } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
  }
}

function seedTargetRow(nodes, value = '') {
  const row = new FakeElement('');
  row.dataset.relayTargetRow = 'true';
  const label = new FakeElement('');
  label.dataset.relayTargetLabel = 'true';
  const controls = new FakeElement('');
  const input = new FakeElement('relay-setup-target-1');
  input.dataset.relayTargetInput = 'true';
  input.value = value;
  const remove = new FakeElement('');
  remove.dataset.relayTargetRemove = 'true';
  remove.hidden = true;
  const error = new FakeElement('relay-setup-target-error-1');
  error.dataset.relayTargetError = 'true';
  error.hidden = true;
  controls.append(input, remove);
  row.append(label, controls, error);
  nodes.get('relay-setup-target-list').append(row);
  return { row, label, input, remove, error };
}

function setupAccount() {
  return {
    owner: Principal.fromText(HISTORIAN),
    subaccount: [Array.from({ length: 32 }, (_, index) => index)],
  };
}

function viewFor({
  targets = [TARGET_A],
  account = setupAccount(),
  factoryAvailable = true,
  state = { NotFunded: null },
} = {}) {
  const targetCount = targets.length;
  const extraCount = targetCount - 1;
  return {
    canonical_target_canister_ids: targets.map((value) => Principal.fromText(value)),
    setup_key_identifier: 'ab'.repeat(32),
    setup_account: account ? [account] : [],
    setup_account_identifier: account ? [accountIdentifierHex(account)] : [],
    target_count: targetCount,
    singleton_nominal_minimum_e8s: 300_000_000n,
    extra_target_count: BigInt(extraCount),
    extra_target_unit_charge_e8s: 25_000_000n,
    total_extra_target_charge_e8s: BigInt(extraCount) * 25_000_000n,
    nominal_minimum_e8s: 300_000_000n + BigInt(extraCount) * 25_000_000n,
    factory_available: factoryAvailable,
    state,
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

function controllerHarness({
  view = viewFor(),
  balance = 400_000_000n,
  notify = { Active: { relay_canister_id: Principal.fromText(RELAY) } },
  getView,
  getBalance,
  notifyRelay,
  copyTextToClipboard = null,
} = {}) {
  const calls = { view: 0, balance: 0, notify: 0, intervals: 0, clears: 0, copied: [] };
  let currentView = view;
  let currentBalance = balance;
  const actor = {
    async get_relay_setup_view(args) {
      calls.view += 1;
      return getView ? getView(args, calls.view) : { Ok: currentView };
    },
    async get_public_status() {
      return { ledger_canister_id: Principal.fromText(LEDGER) };
    },
    async notify_relay_setup(args) {
      calls.notify += 1;
      return notifyRelay ? notifyRelay(args, calls.notify) : notify;
    },
  };
  const controller = createRelaySetupController({
    frontendConfig: { historianCanisterId: HISTORIAN },
    createHistorian: async () => ({ agent: {}, historian: actor }),
    ledgerActorFactory: () => ({
      async icrc1_balance_of(account) {
        calls.balance += 1;
        return getBalance ? getBalance(account, calls.balance) : currentBalance;
      },
    }),
    copyTextToClipboard: async (value) => {
      calls.copied.push(value);
      if (copyTextToClipboard) await copyTextToClipboard(value);
    },
    setIntervalFn: () => { calls.intervals += 1; return calls.intervals; },
    clearIntervalFn: () => { calls.clears += 1; },
  });
  return {
    controller,
    calls,
    actor,
    setView(value) { currentView = value; },
    setBalance(value) { currentBalance = value; },
  };
}

async function submit(nodes, harness, input = TARGET_A) {
  nodes.get('relay-setup-target-input').value = input;
  await harness.controller.submitTarget();
}

test('initial prompt is visible before target submission', async () => {
  await withDom(async (nodes) => {
    const { controller } = controllerHarness();
    controller.render();
    assert.equal(nodes.get('relay-setup-result').hidden, false);
    assert.equal(nodes.get('relay-setup-summary').hidden, true);
  });
});

test('empty input is rejected without an actor call', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness, '');
    assert.match(nodes.get('relay-setup-status').textContent, /at least one/i);
    assert.equal(harness.calls.view, 0);
  });
});

test('invalid principal is rejected without an actor call', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness, 'not-a-principal');
    assert.match(nodes.get('relay-setup-status').textContent, /invalid/i);
    assert.equal(harness.calls.view, 0);
  });
});

test('duplicate targets are rejected without an actor call', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness, `${TARGET_A}, ${TARGET_A}`);
    assert.match(nodes.get('relay-setup-status').textContent, /duplicate/i);
    assert.equal(harness.calls.view, 0);
  });
});

test('parser accepts twenty targets', () => {
  const text = Array.from({ length: 20 }, (_, index) => Principal.fromUint8Array(Uint8Array.of(index + 1)).toText()).join('\n');
  assert.equal(parseRelayTargetSet(text).length, 20);
});

test('parser rejects twenty-one targets', () => {
  const text = Array.from({ length: 21 }, (_, index) => Principal.fromUint8Array(Uint8Array.of(index + 1)).toText()).join('\n');
  assert.throws(() => parseRelayTargetSet(text), /no more than 20/i);
});

test('duplicate detection catches valid repeated canister IDs and ignores incomplete entries', () => {
  assert.deepEqual([...duplicateRelayTargetIndexes([TARGET_A, TARGET_A, '', 'not-yet-valid'])], [0, 1]);
  assert.deepEqual([...duplicateRelayTargetIndexes([TARGET_A, TARGET_B])], []);
});

test('repeatable target fields add, flag duplicates immediately, and remove cleanly', async () => {
  await withDom(async (nodes) => {
    const first = seedTargetRow(nodes);
    const harness = controllerHarness();
    harness.controller.bindPane();
    const list = nodes.get('relay-setup-target-list');

    assert.equal(nodes.get('relay-setup-submit').disabled, true);
    first.input.value = TARGET_A;
    list.listeners.get('input')({ target: first.input });
    assert.equal(nodes.get('relay-setup-submit').disabled, false);

    await nodes.get('relay-setup-add-target').listeners.get('click')();
    const inputs = list.querySelectorAll('[data-relay-target-input]');
    const removeButtons = list.querySelectorAll('[data-relay-target-remove]');
    assert.equal(inputs.length, 2);
    assert.equal(inputs[1].focused, true);
    assert.equal(removeButtons.every((button) => button.hidden === false), true);
    assert.equal(nodes.get('relay-setup-target-count-hint').textContent, '2 target canisters');
    assert.equal(nodes.get('relay-setup-submit').disabled, true);

    inputs[1].value = TARGET_A;
    list.listeners.get('input')({ target: inputs[1] });
    assert.equal(inputs[0].getAttribute('aria-invalid'), 'true');
    assert.equal(inputs[1].getAttribute('aria-invalid'), 'true');
    assert.equal(nodes.get('relay-setup-warning').hidden, false);
    assert.equal(nodes.get('relay-setup-submit').disabled, true);

    inputs[1].value = TARGET_B;
    list.listeners.get('input')({ target: inputs[1] });
    assert.equal(inputs[0].getAttribute('aria-invalid'), null);
    assert.equal(inputs[1].getAttribute('aria-invalid'), null);
    assert.equal(nodes.get('relay-setup-warning').hidden, true);
    assert.equal(nodes.get('relay-setup-submit').disabled, false);

    list.listeners.get('click')({ target: first.remove });
    const remainingInputs = list.querySelectorAll('[data-relay-target-input]');
    assert.equal(remainingInputs.length, 1);
    assert.equal(remainingInputs[0].value, TARGET_B);
    assert.equal(remainingInputs[0].focused, true);
    assert.equal(list.querySelector('[data-relay-target-remove]').hidden, true);
    assert.equal(nodes.get('relay-setup-target-count-hint').textContent, '1 target canister');
    assert.equal(nodes.get('relay-setup-submit').disabled, false);
    assert.match(nodes.get('relay-setup-target-announcement').textContent, /1 field remaining/);

    for (let index = 1; index < 20; index += 1) {
      await nodes.get('relay-setup-add-target').listeners.get('click')();
    }
    assert.equal(list.querySelectorAll('[data-relay-target-input]').length, 20);
    assert.equal(nodes.get('relay-setup-add-target').disabled, true);
    await nodes.get('relay-setup-add-target').listeners.get('click')();
    assert.equal(list.querySelectorAll('[data-relay-target-input]').length, 20);
  });
});

test('canonical backend ordering is displayed', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ view: viewFor({ targets: [TARGET_A, TARGET_B] }) });
    await submit(nodes, harness, `${TARGET_B}\n${TARGET_A}`);
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, `${TARGET_A}\n${TARGET_B}`);
  });
});

for (const [count, expected] of [[1, '3 ICP'], [2, '3.25 ICP'], [10, '5.25 ICP'], [20, '7.75 ICP']]) {
  test(`pricing for ${count} target${count === 1 ? '' : 's'} is displayed`, async () => {
    await withDom(async (nodes) => {
      const targets = Array.from({ length: count }, (_, index) => Principal.fromUint8Array(Uint8Array.of(index + 1)).toText());
      const harness = controllerHarness({ view: viewFor({ targets }), balance: 1_000_000_000n });
      await submit(nodes, harness, targets.join('\n'));
      assert.equal(nodes.get('relay-setup-minimum').textContent, expected);
      assert.equal(nodes.get('relay-setup-requirement').textContent, expected);
    });
  });
}

test('setup account links target the dashboard account page', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness);
    const identifier = accountIdentifierHex(setupAccount());
    assert.equal(nodes.get('relay-setup-icrc-account-link').href, `https://dashboard.internetcomputer.org/account/${identifier}`);
    assert.equal(nodes.get('relay-setup-icrc-account-link').title, icrcAccountText(setupAccount()));
    assert.equal(nodes.get('relay-setup-account-identifier-link').href, `https://dashboard.internetcomputer.org/account/${identifier}`);
    assert.equal(nodes.get('relay-setup-account-identifier-link').title, identifier);
  });
});

test('both setup account copy buttons copy their displayed values', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    harness.controller.bindPane();
    await submit(nodes, harness);
    await nodes.get('copy-relay-setup-icrc-account').listeners.get('click')();
    await nodes.get('copy-relay-setup-account-identifier').listeners.get('click')();
    assert.deepEqual(harness.calls.copied, [icrcAccountText(setupAccount()), accountIdentifierHex(setupAccount())]);
  });
});

test('initial insufficient balance keeps Create disabled and starts balance polling', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ balance: 299_999_999n });
    await submit(nodes, harness);
    assert.equal(nodes.get('relay-setup-create').disabled, true);
    assert.equal(harness.calls.intervals, 1);
  });
});

test('later balance polling enables Create without notifying automatically', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ balance: 299_999_999n });
    await submit(nodes, harness);
    harness.setBalance(300_000_000n);
    await harness.controller.refresh();
    assert.equal(nodes.get('relay-setup-create').disabled, false);
    assert.equal(harness.calls.notify, 0);
  });
});

test('explicit Create calls notify exactly once with only the target vector', async () => {
  await withDom(async (nodes) => {
    let args;
    const harness = controllerHarness({
      notifyRelay: async (value) => { args = value; return { InProgress: { phase: { CreateDispatched: null }, relay_canister_id: [] } }; },
      getView: async () => ({ Ok: viewFor() }),
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 1);
    assert.deepEqual(Object.keys(args), ['target_canister_ids']);
  });
});

test('authoritative live shortfall overrides a lower nominal query requirement', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      balance: 300_000_000n,
      notifyRelay: async (_args, call) => call === 1
        ? { BelowCurrentRequirement: { balance_e8s: 300_000_000n, required_e8s: 425_000_000n, shortfall_e8s: 125_000_000n } }
        : { InProgress: { phase: { Reserved: null }, relay_canister_id: [] } },
    });
    await submit(nodes, harness);
    assert.equal(nodes.get('relay-setup-create').disabled, false);
    await harness.controller.createRelay();
    assert.equal(nodes.get('relay-setup-requirement').textContent, '4.25 ICP');
    assert.equal(nodes.get('relay-setup-create').disabled, true);
    harness.setBalance(425_000_000n);
    await harness.controller.refresh();
    assert.equal(nodes.get('relay-setup-requirement').textContent, '4.25 ICP');
    assert.equal(nodes.get('relay-setup-create').disabled, false);
    await harness.controller.createRelay();
    assert.equal(harness.controller.state.requiredBalanceOverride, null);
  });
});

test('BelowMinimum renders balance, requirement, and shortfall while retaining controls', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ notify: { BelowMinimum: { balance_e8s: 200_000_000n, required_e8s: 300_000_000n, shortfall_e8s: 100_000_000n } } });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.match(nodes.get('relay-setup-status-label').textContent, /Balance 2 ICP; required 3 ICP; shortfall 1 ICP/);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
  });
});

test('Busy message survives the immediate setup-view refresh', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ notify: { Busy: null } });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.equal(nodes.get('relay-setup-status').textContent, 'Busy');
    assert.match(nodes.get('relay-setup-status-label').textContent, /maximum number of funded setups/);
  });
});

test('FailedPreSpend message survives refresh and leaves setup account visible', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ notify: { FailedPreSpend: { message: 'target probe failed' } } });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'target probe failed');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
  });
});

test('Active notify result renders the Relay and hides setup controls before query catch-up', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
  });
});

test('ManualRecoveryRequired notify result renders phase, Relay ID, and error and stops polling', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      notify: {
        ManualRecoveryRequired: {
          phase: { HandoffAttempted: null },
          relay_canister_id: [Principal.fromText(RELAY)],
          message: 'final state mismatch',
        },
      },
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.match(nodes.get('relay-setup-status-label').textContent, /HandoffAttempted/);
    assert.match(nodes.get('relay-setup-status-label').textContent, /br5f7/);
    assert.match(nodes.get('relay-setup-status-label').textContent, /final state mismatch/);
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
    assert.ok(harness.calls.clears >= 1);
  });
});

test('transient setup-view failure is rendered', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ getView: async () => { throw new Error('view unavailable'); } });
    await submit(nodes, harness);
    assert.match(nodes.get('relay-setup-status').textContent, /view unavailable/);
  });
});

test('transient ledger-balance failure is rendered', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ getBalance: async () => { throw new Error('balance unavailable'); } });
    await submit(nodes, harness);
    assert.match(nodes.get('relay-setup-status').textContent, /balance unavailable/);
  });
});

test('transient notify failure is rendered without hiding payment details', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ notifyRelay: async () => { throw new Error('notify unavailable'); } });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.match(nodes.get('relay-setup-status').textContent, /notify unavailable/);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
  });
});

test('stale notify completion cannot mutate a newer target-set view', async () => {
  await withDom(async (nodes) => {
    const pending = deferred();
    const harness = controllerHarness({
      getView: async (args) => ({ Ok: viewFor({ targets: args.target_canister_ids.map((value) => value.toText()) }) }),
      notifyRelay: async () => pending.promise,
    });
    await submit(nodes, harness, TARGET_A);
    const creatingA = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    await submit(nodes, harness, TARGET_B);
    pending.resolve({ Active: { relay_canister_id: Principal.fromText(RELAY) } });
    await creatingA;
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, TARGET_B);
    assert.equal(harness.controller.state.creating, false);
    assert.equal(nodes.get('relay-setup-create').disabled, false);
  });
});

test('InProgress notify result begins polling and displays its phase', async () => {
  await withDom(async (nodes) => {
    let inProgress = false;
    const state = { InProgress: { phase: { CreateDispatched: null }, relay_canister_id: [] } };
    const harness = controllerHarness({
      notifyRelay: async () => { inProgress = true; return state; },
      getView: async () => ({ Ok: inProgress ? viewFor({ account: null, state }) : viewFor() }),
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.ok(harness.calls.intervals >= 1);
    assert.match(nodes.get('relay-setup-status-label').textContent, /CreateDispatched/);
  });
});

test('polling stops when Active is reached', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness);
    harness.setView(viewFor({ account: null, state: { Active: { relay_canister_id: Principal.fromText(RELAY) } } }));
    await harness.controller.refresh();
    assert.ok(harness.calls.clears >= 1);
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
  });
});

test('polling stops and controls hide on ManualRecoveryRequired', async () => {
  await withDom(async (nodes) => {
    const manual = { ManualRecoveryRequired: { phase: { RelayFunded: null }, relay_canister_id: [Principal.fromText(RELAY)], message: 'handoff failed' } };
    const harness = controllerHarness();
    await submit(nodes, harness);
    harness.setView(viewFor({ account: null, state: manual }));
    await harness.controller.refresh();
    assert.ok(harness.calls.clears >= 1);
    assert.match(nodes.get('relay-setup-status-label').textContent, /handoff failed/);
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
  });
});

test('target input change stops polling and clears stale account details', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    harness.controller.bindPane();
    await submit(nodes, harness);
    nodes.get('relay-setup-target-input').value = TARGET_B;
    nodes.get('relay-setup-target-input').listeners.get('input')();
    assert.ok(harness.calls.clears >= 1);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(nodes.get('relay-setup-icrc-account-link').href, '');
  });
});

test('factory unavailable is explicit and exposes no creation controls', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ view: viewFor({ account: null, factoryAvailable: false }) });
    await submit(nodes, harness);
    assert.equal(nodes.get('relay-setup-factory').textContent, 'Unavailable');
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
  });
});

test('active exact set hides setup account and controls and renders Relay tracker link', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ view: viewFor({ account: null, state: { Active: { relay_canister_id: Principal.fromText(RELAY) } } }) });
    await submit(nodes, harness);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
  });
});

test('source and markup contain no payment proof, refund, quote, indicative, or automatic notify flow', () => {
  const source = readFileSync(new URL('../src/app/relay-setup-controller.js', import.meta.url), 'utf8');
  const markup = readFileSync(new URL('../../public/index.html', import.meta.url), 'utf8');
  for (const term of [
    `payment_${'block'}`,
    `request_relay_${'setup_refund'}`,
    `quote_relay_${'setup'}`,
    `list_relay_${'registrations'}`,
    `indicative_current_${'requirement'}`,
  ]) {
    assert.equal(source.includes(term), false);
    assert.equal(markup.includes(term), false);
  }
  assert.doesNotMatch(markup, /Indicative live requirement|payment block|refund button|late-payment sweep/i);
  assert.match(markup, /Create Relay/);
});

test('ICRC account rendering remains stable', () => {
  assert.match(icrcAccountText(setupAccount()), new RegExp(`^${HISTORIAN}-`));
});
