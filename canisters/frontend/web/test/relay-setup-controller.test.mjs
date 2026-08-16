import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';

import {
  createRelaySetupController,
  duplicatePrincipalIndexes,
  duplicateRelayRecipientIndexes,
  duplicateRelayTargetIndexes,
  icrcAccountText,
  parseRelayRecipientSet,
  parseRelayNeuronId,
  parseRelayTargetSet,
} from '../src/app/relay-setup-controller.js';
import { accountIdentifierHex } from '../src/data/dashboard-transforms.js';

const TARGET_A = '22255-zqaaa-aaaas-qf6uq-cai';
const TARGET_B = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
const RECIPIENT_A = 'bg4sm-wzk';
const RECIPIENT_B = 'p27bn-tjl';
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
  'relay-setup-form', 'relay-setup-result',
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
  'relay-setup-recipient-list', 'relay-setup-add-recipient', 'relay-setup-recipient-count-hint',
  'relay-setup-recipient-announcement', 'relay-setup-recipient-count',
  'relay-setup-canonical-recipients', 'relay-setup-configuration-hash',
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

function seedRecipientRow(nodes, value = '') {
  const row = new FakeElement('');
  row.dataset.relayRecipientRow = 'true';
  const label = new FakeElement('');
  label.dataset.relayRecipientLabel = 'true';
  const controls = new FakeElement('');
  const type = new FakeElement('');
  type.dataset.relayRecipientType = 'true';
  type.value = 'Principal';
  const input = new FakeElement('relay-setup-recipient-1');
  input.dataset.relayRecipientInput = 'true';
  input.value = value;
  const remove = new FakeElement('');
  remove.dataset.relayRecipientRemove = 'true';
  remove.hidden = true;
  const error = new FakeElement('relay-setup-recipient-error-1');
  error.dataset.relayRecipientError = 'true';
  error.hidden = true;
  controls.append(type, input, remove);
  row.append(label, controls, error);
  nodes.get('relay-setup-recipient-list').append(row);
  return { row, label, type, input, remove, error };
}

function setupAccount() {
  return {
    owner: Principal.fromText(HISTORIAN),
    subaccount: [Array.from({ length: 32 }, (_, index) => index)],
  };
}

function viewFor({
  targets = [TARGET_A],
  recipients = [RECIPIENT_A],
  account = setupAccount(),
  factoryAvailable = true,
  state = { NotFunded: null },
} = {}) {
  const targetCount = targets.length;
  const extraCount = targetCount - 1;
  return {
    canonical_target_canister_ids: targets.map((value) => Principal.fromText(value)),
    canonical_surplus_recipients: recipients.map((value) => (
      typeof value === 'object'
        ? value
        : { Principal: Principal.fromText(value) }
    )),
    setup_key_identifier: 'ab'.repeat(32),
    setup_account: account ? [account] : [],
    setup_account_identifier: account ? [accountIdentifierHex(account)] : [],
    target_count: targetCount,
    surplus_recipient_count: recipients.length,
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
  loadNeuron,
  copyTextToClipboard = null,
} = {}) {
  const calls = { view: 0, balance: 0, notify: 0, governanceActors: 0, neurons: [], intervals: 0, clears: 0, copied: [] };
  let currentView = view;
  let currentBalance = balance;
  const actor = {
    async get_relay_configuration_view(args) {
      calls.view += 1;
      return getView ? getView(args, calls.view) : { Ok: currentView };
    },
    async get_public_status() {
      return { ledger_canister_id: Principal.fromText(LEDGER) };
    },
    async notify_relay_configuration(args) {
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
    governanceActorFactory: () => {
      calls.governanceActors += 1;
      return { list_neurons() {} };
    },
    publicNeuronLoader: async ({ neuronId }) => {
      calls.neurons.push(neuronId);
      if (loadNeuron) return loadNeuron(neuronId, calls.neurons.length);
      return { owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] };
    },
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

async function submit(nodes, harness, input = TARGET_A, recipients = RECIPIENT_A, recipientTypes = []) {
  const assignValues = (kind, text, seed) => {
    const list = nodes.get(`relay-setup-${kind}-list`);
    const selector = `[data-relay-${kind}-input]`;
    const values = String(text).split(/[\s,]+/u).filter(Boolean);
    if (values.length === 0) values.push('');
    while (list.querySelectorAll(selector).length < values.length) seed(nodes);
    list.querySelectorAll(selector).forEach((field, index) => { field.value = values[index] ?? ''; });
  };
  assignValues('target', input, seedTargetRow);
  assignValues('recipient', recipients, seedRecipientRow);
  nodes.get('relay-setup-recipient-list')
    .querySelectorAll('[data-relay-recipient-type]')
    .forEach((field, index) => { field.value = recipientTypes[index] || 'Principal'; });
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
    assert.match(nodes.get('relay-setup-status').textContent, /enter a target/i);
    assert.equal(harness.calls.view, 0);
  });
});

test('invalid principal is rejected without an actor call', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness, 'not-a-principal');
    assert.match(nodes.get('relay-setup-status').textContent, /valid target/i);
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

test('submission rejects an empty recipient list without an actor call', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness();
    await submit(nodes, harness, TARGET_A, '');
    assert.match(nodes.get('relay-setup-status').textContent, /surplus recipient/i);
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

test('recipient parser accepts one and five typed recipients and rejects zero or six', () => {
  const recipients = Array.from({ length: 6 }, (_, index) => Principal.fromUint8Array(Uint8Array.of(0x7f, index + 1)).toText());
  const typed = recipients.map((value) => ({ type: 'Principal', value }));
  assert.equal(parseRelayRecipientSet(typed.slice(0, 1)).length, 1);
  assert.equal(parseRelayRecipientSet(typed.slice(0, 5)).length, 5);
  assert.throws(() => parseRelayRecipientSet([]), /at least one/i);
  assert.throws(() => parseRelayRecipientSet(typed), /no more than 5/i);
});

test('recipient parser rejects malformed and duplicate principals', () => {
  assert.throws(() => parseRelayRecipientSet([{ type: 'Principal', value: 'not-a-principal' }]), /invalid recipient principal/i);
  assert.throws(() => parseRelayRecipientSet([
    { type: 'Principal', value: RECIPIENT_A },
    { type: 'Principal', value: RECIPIENT_A },
  ]), /duplicate/i);
  assert.deepEqual([...duplicatePrincipalIndexes([RECIPIENT_A, RECIPIENT_A])], [0, 1]);
});

test('neuron parser uses exact u64 BigInt syntax and typed duplicate detection', () => {
  assert.equal(parseRelayNeuronId('1'), 1n);
  assert.equal(parseRelayNeuronId('18446744073709551615'), 18446744073709551615n);
  for (const invalid of ['', '0', '-1', '+1', '1.5', '1e3', '1_000', 'abc', '18446744073709551616']) {
    assert.throws(() => parseRelayNeuronId(invalid));
  }
  assert.deepEqual([...duplicateRelayRecipientIndexes([
    { type: 'Neuron', value: '42' },
    { type: 'Neuron', value: '00042' },
    { type: 'Principal', value: RECIPIENT_A },
  ])], [0, 1]);
  assert.deepEqual([...duplicateRelayRecipientIndexes([
    { type: 'Neuron', value: '42' },
    { type: 'Principal', value: RECIPIENT_A },
  ])], []);
});

test('canonical duplicate neuron rows use typed-recipient wording', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const first = seedRecipientRow(nodes, '42');
    const second = seedRecipientRow(nodes, '00042');
    first.type.value = 'Neuron';
    second.type.value = 'Neuron';
    const harness = controllerHarness();
    harness.controller.bindPane();

    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: second.input });

    assert.equal(first.error.textContent, 'Duplicate surplus recipient. Each recipient must be unique.');
    assert.equal(second.error.textContent, 'Duplicate surplus recipient. Each recipient must be unique.');
    assert.doesNotMatch(first.error.textContent, /Duplicate principal/i);
    assert.equal(nodes.get('relay-setup-submit').disabled, true);
  });
});

test('duplicate detection catches valid repeated canister IDs and ignores incomplete entries', () => {
  assert.deepEqual([...duplicateRelayTargetIndexes([TARGET_A, TARGET_A, '', 'not-yet-valid'])], [0, 1]);
  assert.deepEqual([...duplicateRelayTargetIndexes([TARGET_A, TARGET_B])], []);
});

test('repeatable target fields add, flag duplicates immediately, and remove cleanly', async () => {
  await withDom(async (nodes) => {
    const first = seedTargetRow(nodes);
    seedRecipientRow(nodes, RECIPIENT_A);
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

test('repeatable recipient fields stop at five and retain one required row', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const first = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    const list = nodes.get('relay-setup-recipient-list');
    assert.equal(first.type.value, 'Principal');
    for (let index = 1; index < 5; index += 1) {
      nodes.get('relay-setup-add-recipient').listeners.get('click')();
    }
    assert.equal(list.querySelectorAll('[data-relay-recipient-input]').length, 5);
    assert.equal(
      list.querySelectorAll('[data-relay-recipient-type]').every((field) => field.value === 'Principal'),
      true,
    );
    assert.equal(nodes.get('relay-setup-add-recipient').disabled, true);
    nodes.get('relay-setup-add-recipient').listeners.get('click')();
    assert.equal(list.querySelectorAll('[data-relay-recipient-input]').length, 5);
    list.listeners.get('click')({ target: first.remove });
    assert.equal(list.querySelectorAll('[data-relay-recipient-input]').length, 4);
  });
});

test('recipient type controls are independent and switching type updates and invalidates the row', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const first = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    await submit(nodes, harness);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
    const second = seedRecipientRow(nodes, '42');

    first.type.value = 'Neuron';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: first.type });
    assert.equal(first.input.value, RECIPIENT_A);
    assert.equal(first.input.placeholder, 'Neuron ID');
    assert.equal(first.input.getAttribute('inputmode'), 'numeric');
    assert.equal(first.label.textContent, 'Recipient neuron ID 1');
    assert.equal(first.input.getAttribute('aria-invalid'), 'true');
    assert.equal(second.type.value, 'Principal');
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.ok(harness.calls.clears >= 1);

    first.type.value = 'Principal';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: first.type });
    assert.equal(first.input.placeholder, 'Principal');
    assert.equal(first.input.getAttribute('inputmode'), null);
  });
});

test('canonical backend ordering is displayed', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ view: viewFor({ targets: [TARGET_A, TARGET_B] }) });
    await submit(nodes, harness, `${TARGET_B}\n${TARGET_A}`);
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, `${TARGET_A}\n${TARGET_B}`);
  });
});

test('canonical recipient order and count are displayed', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ view: viewFor({ recipients: [RECIPIENT_A, RECIPIENT_B] }) });
    await submit(nodes, harness, TARGET_A, `${RECIPIENT_B}\n${RECIPIENT_A}`);
    assert.equal(nodes.get('relay-setup-recipient-count').textContent, '2');
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, `Principal: ${RECIPIENT_A}\nPrincipal: ${RECIPIENT_B}`);
    assert.equal(nodes.get('relay-setup-configuration-hash').textContent, 'ab'.repeat(32));
  });
});

test('same targets with a different recipient trigger a new view and setup account', async () => {
  await withDom(async (nodes) => {
    const accountA = setupAccount();
    const accountB = { ...setupAccount(), subaccount: [Array(32).fill(9)] };
    const harness = controllerHarness({
      getView: async (args) => ({
        Ok: viewFor({
          recipients: args.surplus_recipients.map((recipient) => recipient.Principal.toText()),
          account: args.surplus_recipients[0].Principal.toText() === RECIPIENT_A ? accountA : accountB,
        }),
      }),
    });
    await submit(nodes, harness, TARGET_A, RECIPIENT_A);
    const firstAccount = nodes.get('relay-setup-icrc-account').textContent;
    await submit(nodes, harness, TARGET_A, RECIPIENT_B);
    assert.equal(harness.calls.view, 2);
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, firstAccount);
  });
});

test('stale view for an earlier recipient set is ignored', async () => {
  await withDom(async (nodes) => {
    const pending = deferred();
    const harness = controllerHarness({
      getView: async (args, call) => call === 1
        ? pending.promise
        : { Ok: viewFor({ recipients: args.surplus_recipients.map((recipient) => recipient.Principal.toText()) }) },
    });
    const first = submit(nodes, harness, TARGET_A, RECIPIENT_A);
    while (harness.calls.view === 0) await Promise.resolve();
    await submit(nodes, harness, TARGET_A, RECIPIENT_B);
    pending.resolve({ Ok: viewFor({ recipients: [RECIPIENT_A] }) });
    await first;
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, `Principal: ${RECIPIENT_B}`);
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

test('view and Create calls carry both configuration vectors', async () => {
  await withDom(async (nodes) => {
    let viewArgs;
    let notifyArgs;
    const harness = controllerHarness({
      notifyRelay: async (value) => { notifyArgs = value; return { InProgress: { phase: { CreateDispatched: null }, relay_canister_id: [] } }; },
      getView: async (value) => { viewArgs = value; return { Ok: viewFor() }; },
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 1);
    assert.deepEqual(Object.keys(viewArgs).sort(), ['surplus_recipients', 'target_canister_ids']);
    assert.deepEqual(Object.keys(notifyArgs).sort(), ['surplus_recipients', 'target_canister_ids']);
    assert.equal(notifyArgs.surplus_recipients[0].Principal.toText(), RECIPIENT_A);
    assert.equal('surplus_recipient_principals' in notifyArgs, false);
    assert.equal(harness.calls.governanceActors, 0);
  });
});

test('neuron recipients serialize as bigint and preflight through NNS Governance', async () => {
  await withDom(async (nodes) => {
    let viewArgs;
    const harness = controllerHarness({
      getView: async (args) => {
        viewArgs = args;
        return { Ok: viewFor({ recipients: [{ Neuron: 42n }] }) };
      },
    });
    await submit(nodes, harness, TARGET_A, '42', ['Neuron']);
    assert.equal(harness.calls.governanceActors, 1);
    assert.deepEqual(harness.calls.neurons, [42n]);
    assert.deepEqual(viewArgs.surplus_recipients, [{ Neuron: 42n }]);
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, 'Neuron: 42');
  });
});

test('unverified neuron blocks Historian setup view without asserting a privacy cause', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      loadNeuron: async () => { throw new Error('not public'); },
    });
    await submit(nodes, harness, TARGET_A, '123', ['Neuron']);
    assert.equal(harness.calls.view, 0);
    assert.match(nodes.get('relay-setup-status').textContent, /Could not verify neuron 123 as publicly readable by NNS Governance/i);
    assert.doesNotMatch(nodes.get('relay-setup-status').textContent, /must be public/i);
  });
});

test('stale neuron preflight failure cannot cross a recipient type change', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, '42');
    recipient.type.value = 'Neuron';
    const pending = deferred();
    const harness = controllerHarness({ loadNeuron: async () => pending.promise });
    harness.controller.bindPane();
    const first = harness.controller.submitConfiguration();
    while (harness.calls.neurons.length === 0) await Promise.resolve();
    recipient.type.value = 'Principal';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: recipient.type });
    pending.reject(new Error('not public'));
    await first;
    assert.doesNotMatch(nodes.get('relay-setup-status').textContent, /public\/readable/i);
    assert.equal(harness.calls.view, 0);
  });
});

test('stale neuron preflight completion cannot request a view after a type change', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, '42');
    recipient.type.value = 'Neuron';
    const pending = deferred();
    const harness = controllerHarness({ loadNeuron: async () => pending.promise });
    harness.controller.bindPane();
    const first = harness.controller.submitConfiguration();
    while (harness.calls.neurons.length === 0) await Promise.resolve();
    recipient.type.value = 'Principal';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: recipient.type });
    pending.resolve({ owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] });
    await first;
    assert.equal(harness.calls.view, 0);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
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

test('stale notify completion cannot mutate a newer Relay configuration view', async () => {
  await withDom(async (nodes) => {
    const pending = deferred();
    const harness = controllerHarness({
      getView: async (args) => ({ Ok: viewFor({ targets: args.target_canister_ids.map((value) => value.toText()) }) }),
      notifyRelay: async () => pending.promise,
    });
    await submit(nodes, harness, TARGET_A);
    const creatingA = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    await submit(nodes, harness, TARGET_B, RECIPIENT_B);
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

test('recipient input change stops polling and clears stale account details', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    await submit(nodes, harness);
    recipient.input.value = RECIPIENT_B;
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.input });
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
  assert.match(source, /get_relay_configuration_view/);
  assert.match(source, /notify_relay_configuration/);
  assert.doesNotMatch(source, /get_relay_setup_view|notify_relay_setup/);
  assert.match(markup, /targets and typed recipients together determine the setup address/i);
  assert.match(markup, /No IO recipient is added automatically/i);
  assert.match(markup, /incorrectly selected Relay configuration are not automatically refundable/i);
  assert.doesNotMatch(markup, /surplus ICP will automatically be routed to the IO neuron/i);
});

test('ICRC account rendering remains stable', () => {
  assert.match(icrcAccountText(setupAccount()), new RegExp(`^${HISTORIAN}-`));
});
