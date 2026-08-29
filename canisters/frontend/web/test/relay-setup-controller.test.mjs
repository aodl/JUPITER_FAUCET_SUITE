import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';

import {
  createRelaySetupController,
  decodeRelayMemo,
  duplicatePrincipalIndexes,
  duplicateRelayRecipientIndexes,
  duplicateRelayTargetIndexes,
  icrcAccountText,
  parseRelayRecipientSet,
  parseRelayMemo,
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
  constructor(id, tagName = 'div') {
    this.id = id;
    this.tagName = tagName.toLowerCase();
    this.type = '';
    this._value = '';
    this.dataset = {};
    this.listeners = new Map();
    this.attributes = new Map();
    this.textContent = '';
    this.innerHTML = '';
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.focused = false;
    this.href = '';
    this.title = '';
    this.className = '';
    this.children = [];
    this.parentElement = null;
    this.classList = {
      toggle: (name, enabled) => {
        const classes = new Set(this.className.split(/\s+/u).filter(Boolean));
        if (enabled) classes.add(name);
        else classes.delete(name);
        this.className = [...classes].join(' ');
      },
    };
  }

  get value() { return this._value; }
  set value(value) {
    const text = String(value ?? '');
    this._value = this.tagName === 'input' && this.type === 'text'
      ? text.replace(/[\r\n]/gu, '')
      : text;
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
  'relay-setup-finalization-resume-note',
  'relay-setup-existing-relay', 'copy-relay-setup-icrc-account',
  'copy-relay-setup-account-identifier', 'relay-setup-icrc-account-link',
  'relay-setup-account-identifier-link',
  'relay-setup-target-list', 'relay-setup-add-target', 'relay-setup-target-count-hint',
  'relay-setup-target-announcement', 'relay-setup-warning', 'relay-setup-submit',
  'relay-setup-recipient-list', 'relay-setup-add-recipient', 'relay-setup-recipient-count-hint',
  'relay-setup-recipient-announcement', 'relay-setup-recipient-count',
  'relay-setup-canonical-recipients', 'relay-setup-configuration-hash',
  'relay-setup-mode-routing', 'relay-setup-mode-all-cycles',
  'relay-setup-recipient-editor', 'relay-setup-surplus-mode',
  'relay-setup-surplus-mode-summary',
];

async function withDom(run) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const nodes = new Map(DOM_IDS.map((id) => [id, new FakeElement(id)]));
  nodes.get('relay-setup-mode-routing').checked = true;
  globalThis.document = {
    getElementById: (id) => nodes.get(id) || null,
    createElement: (tagName) => new FakeElement('', tagName),
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
  const label = new FakeElement('', 'label');
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
  const label = new FakeElement('', 'label');
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
  const memoMode = new FakeElement('relay-setup-recipient-memo-mode-1');
  memoMode.dataset.relayRecipientMemoMode = 'true';
  memoMode.value = 'Text';
  const memoModeLabel = new FakeElement('', 'label');
  memoModeLabel.textContent = 'Memo format';
  memoModeLabel.dataset.relayRecipientMemoModeLabel = 'true';
  memoModeLabel.setAttribute('for', memoMode.id);
  const memoLabel = new FakeElement('', 'label');
  memoLabel.textContent = 'Memo';
  memoLabel.dataset.relayRecipientMemoLabel = 'true';
  const memoInput = new FakeElement('relay-setup-recipient-memo-1', 'input');
  memoInput.type = 'text';
  memoInput.dataset.relayRecipientMemoInput = 'true';
  memoLabel.setAttribute('for', memoInput.id);
  const memoCount = new FakeElement('relay-setup-recipient-memo-count-1');
  memoCount.dataset.relayRecipientMemoCount = 'true';
  const memoError = new FakeElement('relay-setup-recipient-memo-error-1');
  memoError.dataset.relayRecipientMemoError = 'true';
  memoError.hidden = true;
  const memoNotice = new FakeElement('relay-setup-recipient-memo-notice-1');
  memoNotice.dataset.relayRecipientMemoNotice = 'true';
  memoNotice.hidden = true;
  controls.append(type, input, remove);
  const memoControls = new FakeElement('');
  memoControls.className = 'relay-setup-memo-controls';
  memoControls.append(memoModeLabel, memoMode, memoLabel, memoInput, memoCount);
  row.append(label, controls, error, memoControls, memoError, memoNotice);
  nodes.get('relay-setup-recipient-list').append(row);
  return {
    row, label, type, input, remove, error, memoControls, memoModeLabel, memoMode,
    memoLabel, memoInput, memoCount, memoError, memoNotice,
  };
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
    canonical_surplus_recipients: recipients.map((value) => {
      if (typeof value !== 'object') {
        return { Principal: { principal: Principal.fromText(value), memo: [] } };
      }
      if ('Principal' in value && typeof value.Principal?.toText === 'function') {
        return { Principal: { principal: value.Principal, memo: [] } };
      }
      if ('Neuron' in value && typeof value.Neuron === 'bigint') {
        return { Neuron: { neuron_id: value.Neuron, memo: [] } };
      }
      return value;
    }),
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

async function flushMicrotasks(count = 8) {
  for (let index = 0; index < count; index += 1) await Promise.resolve();
}

function controllerHarness({
  view = viewFor(),
  balance = 400_000_000n,
  notify = { Active: { relay_canister_id: Principal.fromText(RELAY) } },
  getView,
  getBalance,
  notifyRelay,
  loadNeuron,
  createHistorianBundle,
  copyTextToClipboard = null,
} = {}) {
  const calls = {
    view: 0,
    balance: 0,
    notify: 0,
    historianBundles: 0,
    governanceActors: 0,
    neurons: [],
    intervals: 0,
    clears: 0,
    copied: [],
    notifyArgs: [],
  };
  let currentView = view;
  let currentBalance = balance;
  let intervalCallback = null;
  const actor = {
    async get_relay_configuration_view(args) {
      calls.view += 1;
      return getView ? getView(args, calls.view) : { Ok: currentView };
    },
    async get_public_status() {
      return { ledger_canister_id: Principal.fromText(LEDGER) };
    },
    notify_relay_configuration(args) {
      calls.notify += 1;
      calls.notifyArgs.push(args);
      return notifyRelay ? notifyRelay(args, calls.notify) : Promise.resolve(notify);
    },
  };
  const controller = createRelaySetupController({
    frontendConfig: { historianCanisterId: HISTORIAN },
    createHistorian: async () => {
      calls.historianBundles += 1;
      return createHistorianBundle
        ? createHistorianBundle({ agent: {}, historian: actor }, calls.historianBundles)
        : { agent: {}, historian: actor };
    },
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
    setIntervalFn: (callback) => {
      calls.intervals += 1;
      intervalCallback = callback;
      return calls.intervals;
    },
    clearIntervalFn: () => {
      calls.clears += 1;
      intervalCallback = null;
    },
  });
  return {
    controller,
    calls,
    actor,
    setView(value) { currentView = value; },
    setBalance(value) { currentBalance = value; },
    runPoll() { intervalCallback?.(); },
    hasScheduledPoll() { return intervalCallback !== null; },
  };
}

function assertCheckedConfigurationInvalidated(nodes, harness) {
  assert.equal(harness.controller.state.view, null);
  assert.equal(harness.controller.state.checkedConfigurationFingerprint, '');
  assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  assert.equal(nodes.get('relay-setup-existing-relay').innerHTML, '');
  assert.equal(nodes.get('relay-setup-create').disabled, true);
  assert.match(
    nodes.get('relay-setup-status').textContent,
    /(?:configuration|form) changed.*check.*again|current state could not be (?:confirmed|refreshed).*check.*again/i,
  );
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

test('recipient parser accepts zero, one, and five typed recipients and rejects six', () => {
  const recipients = Array.from({ length: 6 }, (_, index) => Principal.fromUint8Array(Uint8Array.of(0x7f, index + 1)).toText());
  const typed = recipients.map((value) => ({ type: 'Principal', value }));
  assert.equal(parseRelayRecipientSet(typed.slice(0, 1)).length, 1);
  assert.equal(parseRelayRecipientSet(typed.slice(0, 5)).length, 5);
  assert.deepEqual(parseRelayRecipientSet([]), []);
  assert.throws(() => parseRelayRecipientSet(typed), /no more than 5/i);
});

test('text memo parsing preserves exact UTF-8 bytes and enforces the 32-byte bound', () => {
  assert.deepEqual([...parseRelayMemo('Text', '')], []);
  assert.deepEqual([...parseRelayMemo('Text', ' a ')], [0x20, 0x61, 0x20]);
  assert.equal(parseRelayMemo('Text', 'a'.repeat(32)).length, 32);
  assert.throws(() => parseRelayMemo('Text', 'a'.repeat(33)), /33 bytes.*maximum is 32/i);
  assert.equal(parseRelayMemo('Text', 'é'.repeat(16)).length, 32);
  assert.throws(() => parseRelayMemo('Text', 'é'.repeat(17)), /34 bytes.*maximum is 32/i);
});

test('hex memo parsing supports arbitrary bytes and rejects malformed or overlong input', () => {
  assert.deepEqual([...parseRelayMemo('Hex', '')], []);
  assert.deepEqual([...parseRelayMemo('Hex', '0x00 ff 80')], [0x00, 0xff, 0x80]);
  assert.deepEqual([...parseRelayMemo('Hex', 'AA bb')], [0xaa, 0xbb]);
  assert.deepEqual([...parseRelayMemo('Hex', '00FF')], [...parseRelayMemo('Hex', '0x00 ff')]);
  assert.throws(() => parseRelayMemo('Hex', 'abc'), /complete byte pairs/i);
  assert.throws(() => parseRelayMemo('Hex', '00xz'), /non-hexadecimal/i);
  assert.equal(parseRelayMemo('Hex', 'ff'.repeat(32)).length, 32);
  assert.throws(() => parseRelayMemo('Hex', 'ff'.repeat(33)), /33 bytes.*maximum is 32/i);
});

test('memo decoding reports exact bytes before the 32-byte policy check', () => {
  assert.deepEqual([...decodeRelayMemo('Text', '')], []);
  assert.deepEqual([...decodeRelayMemo('Text', '  alpha  ')], [
    0x20, 0x20, 0x61, 0x6c, 0x70, 0x68, 0x61, 0x20, 0x20,
  ]);
  assert.equal(decodeRelayMemo('Text', 'é').length, 2);
  assert.equal(decodeRelayMemo('Text', 'a'.repeat(32)).length, 32);
  assert.equal(decodeRelayMemo('Text', 'a'.repeat(33)).length, 33);
  assert.equal(decodeRelayMemo('Hex', 'ff'.repeat(32)).length, 32);
  assert.equal(decodeRelayMemo('Hex', `0x${'ff '.repeat(33)}`).length, 33);
  assert.deepEqual([...decodeRelayMemo('Hex', '00 80 FF')], [0x00, 0x80, 0xff]);
});

test('recipient serialization carries exact memo bytes and duplicate identity ignores memo', () => {
  const principalEmpty = parseRelayRecipientSet([{
    type: 'Principal', value: RECIPIENT_A, memoMode: 'Text', memoValue: '',
  }]);
  assert.deepEqual(principalEmpty[0].Principal.memo, []);
  const principal = parseRelayRecipientSet([{
    type: 'Principal', value: RECIPIENT_A, memoMode: 'Hex', memoValue: '00ff',
  }]);
  assert.equal(principal[0].Principal.principal.toText(), RECIPIENT_A);
  assert.deepEqual(principal[0].Principal.memo, [0x00, 0xff]);
  const neuron = parseRelayRecipientSet([{
    type: 'Neuron', value: '42', memoMode: 'Text', memoValue: ' hi ',
  }]);
  assert.deepEqual(neuron, [{ Neuron: { neuron_id: 42n, memo: [0x20, 0x68, 0x69, 0x20] } }]);
  assert.deepEqual(parseRelayRecipientSet([{
    type: 'Neuron', value: '43', memoMode: 'Hex', memoValue: '00 80 ff',
  }]), [{ Neuron: { neuron_id: 43n, memo: [0x00, 0x80, 0xff] } }]);
  assert.deepEqual(parseRelayRecipientSet([]), []);
  assert.throws(() => parseRelayRecipientSet([
    { type: 'Principal', value: RECIPIENT_A, memoMode: 'Text', memoValue: 'A' },
    { type: 'Principal', value: RECIPIENT_A, memoMode: 'Text', memoValue: 'B' },
  ]), /duplicate/i);
  assert.throws(() => parseRelayRecipientSet([
    { type: 'Neuron', value: '42', memoMode: 'Hex', memoValue: '01' },
    { type: 'Neuron', value: '00042', memoMode: 'Hex', memoValue: '02' },
  ]), /duplicate/i);
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

test('repeatable recipient fields stop at five and remain individually removable', async () => {
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

test('added recipient rows contain visible memo labels in the intended grid order', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    harness.controller.addRecipientField();
    const rows = nodes.get('relay-setup-recipient-list')
      .querySelectorAll('[data-relay-recipient-row]');
    const row = rows[1];
    const memoModeLabel = row.querySelector('[data-relay-recipient-memo-mode-label]');
    const memoLabel = row.querySelector('[data-relay-recipient-memo-label]');
    assert.equal(memoModeLabel.tagName, 'label');
    assert.equal(memoModeLabel.textContent, 'Memo format');
    assert.equal(memoModeLabel.hidden, false);
    assert.equal(memoLabel.tagName, 'label');
    assert.equal(memoLabel.textContent, 'Memo');
    assert.equal(memoLabel.hidden, false);
    const memoControls = memoModeLabel.parentElement;
    assert.deepEqual(memoControls.children.map((child) => child.dataset), [
      { relayRecipientMemoModeLabel: 'true' },
      { relayRecipientMemoMode: 'true' },
      { relayRecipientMemoLabel: 'true' },
      { relayRecipientMemoInput: 'true' },
      { relayRecipientMemoCount: 'true' },
    ]);
  });
});

test('added recipient memo labels are associated with row-numbered controls', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    harness.controller.addRecipientField();
    const row = nodes.get('relay-setup-recipient-list')
      .querySelectorAll('[data-relay-recipient-row]')[1];
    const memoMode = row.querySelector('[data-relay-recipient-memo-mode]');
    const memoInput = row.querySelector('[data-relay-recipient-memo-input]');
    assert.equal(row.querySelector('[data-relay-recipient-memo-mode-label]').getAttribute('for'), 'relay-setup-recipient-memo-mode-2');
    assert.equal(memoMode.id, 'relay-setup-recipient-memo-mode-2');
    assert.equal(row.querySelector('[data-relay-recipient-memo-label]').getAttribute('for'), 'relay-setup-recipient-memo-2');
    assert.equal(memoInput.id, 'relay-setup-recipient-memo-2');
    assert.equal(row.querySelector('[data-relay-recipient-memo-count]').id, 'relay-setup-recipient-memo-count-2');
    assert.equal(row.querySelector('[data-relay-recipient-memo-error]').id, 'relay-setup-recipient-memo-error-2');
    assert.equal(row.querySelector('[data-relay-recipient-memo-notice]').id, 'relay-setup-recipient-memo-notice-2');
  });
});

test('recipient memo label associations and descriptions follow row renumbering', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const first = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    harness.controller.addRecipientField();
    const list = nodes.get('relay-setup-recipient-list');
    list.listeners.get('click')({ target: first.remove });
    const row = list.querySelectorAll('[data-relay-recipient-row]')[0];
    const memoModeLabel = row.querySelector('[data-relay-recipient-memo-mode-label]');
    const memoMode = row.querySelector('[data-relay-recipient-memo-mode]');
    const memoLabel = row.querySelector('[data-relay-recipient-memo-label]');
    const memoInput = row.querySelector('[data-relay-recipient-memo-input]');
    assert.equal(memoModeLabel.getAttribute('for'), 'relay-setup-recipient-memo-mode-1');
    assert.equal(memoMode.id, 'relay-setup-recipient-memo-mode-1');
    assert.equal(memoLabel.getAttribute('for'), 'relay-setup-recipient-memo-1');
    assert.equal(memoInput.id, 'relay-setup-recipient-memo-1');
    assert.equal(
      memoInput.getAttribute('aria-describedby'),
      'relay-setup-recipient-memo-count-1 relay-setup-recipient-memo-error-1 relay-setup-recipient-memo-notice-1',
    );
    assert.doesNotMatch(memoModeLabel.getAttribute('for'), /-2$/u);
    assert.doesNotMatch(memoLabel.getAttribute('for'), /-2$/u);
    assert.doesNotMatch(memoInput.getAttribute('aria-describedby'), /-2(?:\s|$)/u);
  });
});

test('five recipient rows have unique destination and memo-control IDs', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    while (nodes.get('relay-setup-recipient-list').querySelectorAll('[data-relay-recipient-row]').length < 5) {
      harness.controller.addRecipientField();
    }
    const selectors = [
      '[data-relay-recipient-input]',
      '[data-relay-recipient-memo-mode]',
      '[data-relay-recipient-memo-input]',
      '[data-relay-recipient-memo-count]',
      '[data-relay-recipient-memo-error]',
      '[data-relay-recipient-memo-notice]',
    ];
    const ids = selectors.flatMap((selector) => nodes.get('relay-setup-recipient-list')
      .querySelectorAll(selector)
      .filter((element) => element.id)
      .map((element) => element.id));
    assert.equal(ids.length, 30);
    assert.equal(new Set(ids).size, ids.length);
  });
});

test('static and added recipient rows expose the same memo field semantics', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    harness.controller.addRecipientField();
    const rows = nodes.get('relay-setup-recipient-list')
      .querySelectorAll('[data-relay-recipient-row]');
    for (const row of rows) {
      assert.equal(row.querySelector('[data-relay-recipient-memo-mode-label]').textContent, 'Memo format');
      assert.ok(row.querySelector('[data-relay-recipient-memo-mode]'));
      assert.equal(row.querySelector('[data-relay-recipient-memo-label]').textContent, 'Memo');
      assert.ok(row.querySelector('[data-relay-recipient-memo-input]'));
      assert.ok(row.querySelector('[data-relay-recipient-memo-count]'));
      assert.ok(row.querySelector('[data-relay-recipient-memo-error]'));
      assert.ok(row.querySelector('[data-relay-recipient-memo-notice]'));
    }
  });
});

test('removing the final recipient selects all-cycles mode and switching back creates a blank row', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    nodes.get('relay-setup-recipient-list').listeners.get('click')({ target: recipient.remove });
    assert.equal(nodes.get('relay-setup-recipient-list').querySelectorAll('[data-relay-recipient-row]').length, 0);
    assert.equal(nodes.get('relay-setup-mode-all-cycles').checked, true);
    assert.equal(nodes.get('relay-setup-recipient-editor').hidden, true);
    assert.equal(
      nodes.get('relay-setup-surplus-mode-summary').textContent,
      'All-cycles mode selected. No raw ICP surplus recipient will be configured.',
    );
    assert.equal(nodes.get('relay-setup-submit').disabled, false);

    const routing = nodes.get('relay-setup-mode-routing');
    routing.checked = true;
    nodes.get('relay-setup-mode-all-cycles').checked = false;
    routing.listeners.get('change')();
    assert.equal(nodes.get('relay-setup-recipient-list').querySelectorAll('[data-relay-recipient-row]').length, 1);
    assert.equal(nodes.get('relay-setup-recipient-editor').hidden, false);
    assert.equal(
      nodes.get('relay-setup-surplus-mode-summary').textContent,
      'Recipient routing selected. Configure one to five surplus recipients.',
    );
    assert.equal(nodes.get('relay-setup-submit').disabled, true);
  });
});

test('all-cycles mode requests and creates with an empty recipient vector and no Governance call', async () => {
  await withDom(async (nodes) => {
    let viewArgs;
    let notifyArgs;
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness({
      view: viewFor({ recipients: [] }),
      getView: async (args) => { viewArgs = args; return { Ok: viewFor({ recipients: [] }) }; },
      notifyRelay: async (args) => { notifyArgs = args; return { Active: { relay_canister_id: Principal.fromText(RELAY) } }; },
    });
    harness.controller.bindPane();
    nodes.get('relay-setup-mode-routing').checked = false;
    nodes.get('relay-setup-mode-all-cycles').checked = true;
    nodes.get('relay-setup-mode-all-cycles').listeners.get('change')();
    await harness.controller.submitConfiguration();
    assert.deepEqual(viewArgs.surplus_recipients, []);
    assert.equal(harness.calls.governanceActors, 0);
    assert.equal(nodes.get('relay-setup-surplus-mode').textContent, 'All-cycles mode');
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, 'None — all-cycles mode');
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, '—');
    await harness.controller.createRelay();
    assert.deepEqual(notifyArgs.surplus_recipients, []);
    assert.equal(harness.calls.notify, 1);
    assert.equal(harness.calls.governanceActors, 0);
  });
});

test('routing mode submits one or five recipients but rejects zero rows even by direct invocation', async () => {
  await withDom(async (nodes) => {
    let viewArgs;
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness({
      getView: async (args) => {
        viewArgs = args;
        return { Ok: viewFor({ recipients: args.surplus_recipients }) };
      },
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    assert.equal(viewArgs.surplus_recipients.length, 1);
    assert.equal(harness.calls.governanceActors, 0);

    const list = nodes.get('relay-setup-recipient-list');
    while (list.querySelectorAll('[data-relay-recipient-row]').length < 5) {
      nodes.get('relay-setup-add-recipient').listeners.get('click')();
    }
    list.querySelectorAll('[data-relay-recipient-input]').forEach((input, index) => {
      input.value = Principal.fromUint8Array(Uint8Array.of(0x71, index + 1)).toText();
    });
    await harness.controller.submitConfiguration();
    assert.equal(viewArgs.surplus_recipients.length, 5);

    list.querySelectorAll('[data-relay-recipient-row]').forEach((row) => row.remove());
    nodes.get('relay-setup-mode-routing').checked = true;
    nodes.get('relay-setup-mode-all-cycles').checked = false;
    const previousViewCalls = harness.calls.view;
    await harness.controller.submitConfiguration();
    assert.equal(harness.calls.view, previousViewCalls);
    assert.equal(
      nodes.get('relay-setup-status').textContent,
      'Add a surplus recipient or select all-cycles mode.',
    );
  });
});

test('eventless all-cycles-to-routing mutation invalidates the checked configuration', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness({ view: viewFor({ recipients: [] }) });
    harness.controller.bindPane();
    nodes.get('relay-setup-mode-routing').checked = false;
    nodes.get('relay-setup-mode-all-cycles').checked = true;
    nodes.get('relay-setup-mode-all-cycles').listeners.get('change')();
    await harness.controller.submitConfiguration();

    nodes.get('relay-setup-mode-routing').checked = true;
    nodes.get('relay-setup-mode-all-cycles').checked = false;
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless routing-to-all-cycles mutation invalidates the checked configuration', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    assert.equal(harness.controller.state.surplusRecipients.length, 1);
    const priorBalanceCalls = harness.calls.balance;
    nodes.get('relay-setup-mode-routing').checked = false;
    nodes.get('relay-setup-mode-all-cycles').checked = true;
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assert.equal(harness.calls.balance, priorBalanceCalls);
    assert.equal(harness.calls.governanceActors, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless target mutation invalidates the checked configuration before create', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    target.input.value = TARGET_B;
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless recipient destination mutation invalidates the checked configuration before create', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    recipient.input.value = RECIPIENT_B;
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless recipient type mutation invalidates the checked configuration before create', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    recipient.type.value = 'Neuron';
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless Text memo mutation invalidates the checked configuration before create', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoInput.value = 'checked text';
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    recipient.memoInput.value = 'changed text';
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless Hexadecimal memo mutation invalidates the checked configuration before create', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoMode.value = 'Hex';
    recipient.memoInput.value = '00ff';
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    recipient.memoInput.value = '00fe';
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless memo-mode mutation invalidates the checked configuration before create', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoInput.value = '61';
    const harness = controllerHarness();
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    recipient.memoMode.value = 'Hex';
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation during pre-notify actor acquisition prevents notify', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingBundle = deferred();
    const harness = controllerHarness({
      createHistorianBundle: async (bundle, call) => (call === 2 ? pendingBundle.promise : bundle),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const creating = harness.controller.createRelay();
    while (harness.calls.historianBundles < 2) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingBundle.resolve({ agent: {}, historian: harness.actor });
    await creating;
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation while notify is pending cannot render a stale Active result', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingNotify = deferred();
    const harness = controllerHarness({ notifyRelay: async () => pendingNotify.promise });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const creating = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingNotify.resolve({ Active: { relay_canister_id: Principal.fromText(RELAY) } });
    await creating;
    assert.equal(harness.calls.notify, 1);
    assert.equal(nodes.get('relay-setup-existing-relay').innerHTML, '');
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation while actor acquisition rejects clears the creating state', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingBundle = deferred();
    const harness = controllerHarness({
      createHistorianBundle: async (bundle, call) => (call === 2 ? pendingBundle.promise : bundle),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const creating = harness.controller.createRelay();
    while (harness.calls.historianBundles < 2) await Promise.resolve();
    assert.equal(harness.controller.state.creating, true);
    target.input.value = TARGET_B;
    pendingBundle.reject(new Error('actor unavailable'));
    await creating;
    assert.equal(harness.controller.state.creating, false);
    assert.equal(harness.calls.notify, 0);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation while notify rejects clears stale checked state conservatively', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingNotify = deferred();
    const harness = controllerHarness({ notifyRelay: async () => pendingNotify.promise });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const creating = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingNotify.reject(new Error('notification transport unavailable'));
    await creating;
    assert.equal(harness.controller.state.creating, false);
    assert.equal(harness.calls.notify, 1);
    assertCheckedConfigurationInvalidated(nodes, harness);
    assert.match(
      nodes.get('relay-setup-status').textContent,
      /request was being processed.*current state/i,
    );
  });
});

test('eventless mutation while setup view resolves clears the loading operation', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingView = deferred();
    const harness = controllerHarness({ getView: async () => pendingView.promise });
    harness.controller.bindPane();
    const checking = harness.controller.submitConfiguration();
    while (harness.calls.view === 0) await Promise.resolve();
    assert.equal(harness.controller.state.loading, true);
    target.input.value = TARGET_B;
    pendingView.resolve({ Ok: viewFor() });
    await checking;
    assert.equal(harness.controller.state.loading, false);
    assert.equal(harness.controller.state.requestFingerprint, '');
    assert.deepEqual(harness.controller.state.targets, []);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation while setup view rejects clears the loading operation', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingView = deferred();
    const harness = controllerHarness({ getView: async () => pendingView.promise });
    harness.controller.bindPane();
    const checking = harness.controller.submitConfiguration();
    while (harness.calls.view === 0) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingView.reject(new Error('setup view unavailable'));
    await checking;
    assert.equal(harness.controller.state.loading, false);
    assert.equal(harness.controller.state.requestFingerprint, '');
    assert.deepEqual(harness.controller.state.targets, []);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation during post-notify refresh resolution clears stale payment details', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingRefresh = deferred();
    const harness = controllerHarness({
      getView: async (_args, call) => (call === 1 ? { Ok: viewFor() } : pendingRefresh.promise),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const creating = harness.controller.createRelay();
    while (harness.calls.view < 2) await Promise.resolve();
    assert.equal(harness.controller.state.creating, false);
    target.input.value = TARGET_B;
    pendingRefresh.resolve({ Ok: viewFor() });
    await creating;
    assert.equal(harness.calls.notify, 1);
    assertCheckedConfigurationInvalidated(nodes, harness);
    assert.match(
      nodes.get('relay-setup-status').textContent,
      /request was being processed.*current state/i,
    );
  });
});

test('eventless mutation during post-notify refresh rejection clears stale payment details', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingRefresh = deferred();
    const harness = controllerHarness({
      getView: async (_args, call) => (call === 1 ? { Ok: viewFor() } : pendingRefresh.promise),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const creating = harness.controller.createRelay();
    while (harness.calls.view < 2) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingRefresh.reject(new Error('post-notify refresh unavailable'));
    await creating;
    assert.equal(harness.controller.state.creating, false);
    assert.equal(harness.calls.notify, 1);
    assertCheckedConfigurationInvalidated(nodes, harness);
    assert.match(
      nodes.get('relay-setup-status').textContent,
      /request was being processed.*current state/i,
    );
  });
});

test('event-driven invalidation during actor acquisition is not overwritten by rejection', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingBundle = deferred();
    const harness = controllerHarness({
      createHistorianBundle: async (bundle, call) => (call === 2 ? pendingBundle.promise : bundle),
      getView: async (args) => ({
        Ok: viewFor({ targets: args.target_canister_ids.map((value) => value.toText()) }),
      }),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const staleCreate = harness.controller.createRelay();
    while (harness.calls.historianBundles < 2) await Promise.resolve();
    target.input.value = TARGET_B;
    nodes.get('relay-setup-target-list').listeners.get('input')({ target: target.input });
    await harness.controller.submitConfiguration();
    pendingBundle.reject(new Error('stale actor failure'));
    await staleCreate;
    assert.equal(harness.calls.notify, 0);
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, TARGET_B);
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(harness.controller.state.error, '');
  });
});

test('event-driven invalidation during notify is not overwritten by rejection', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingNotify = deferred();
    const harness = controllerHarness({
      notifyRelay: async () => pendingNotify.promise,
      getView: async (args) => ({
        Ok: viewFor({ targets: args.target_canister_ids.map((value) => value.toText()) }),
      }),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const staleCreate = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    target.input.value = TARGET_B;
    nodes.get('relay-setup-target-list').listeners.get('input')({ target: target.input });
    await harness.controller.submitConfiguration();
    pendingNotify.reject(new Error('stale notify failure'));
    await staleCreate;
    assert.equal(harness.calls.notify, 1);
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, TARGET_B);
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(harness.controller.state.error, '');
    assert.equal(harness.controller.state.notifyResult, null);
  });
});

test('unchanged routing creation snapshots exact checked Text and Hexadecimal memo bytes', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const first = seedRecipientRow(nodes, RECIPIENT_A);
    first.memoInput.value = ' hi ';
    const second = seedRecipientRow(nodes, RECIPIENT_B);
    second.memoMode.value = 'Hex';
    second.memoInput.value = '00ff81';
    let notifyArgs;
    const harness = controllerHarness({
      getView: async (args) => ({ Ok: viewFor({ recipients: args.surplus_recipients }) }),
      notifyRelay: async (args) => {
        notifyArgs = args;
        return { InProgress: { phase: { CreateDispatched: null }, relay_canister_id: [] } };
      },
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    nodes.get('relay-setup-canonical-recipients').textContent = 'untrusted display text';
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 1);
    assert.equal(notifyArgs.surplus_recipients[0].Principal.principal.toText(), RECIPIENT_A);
    assert.deepEqual(notifyArgs.surplus_recipients[0].Principal.memo, [0x20, 0x68, 0x69, 0x20]);
    assert.equal(notifyArgs.surplus_recipients[1].Principal.principal.toText(), RECIPIENT_B);
    assert.deepEqual(notifyArgs.surplus_recipients[1].Principal.memo, [0x00, 0xff, 0x81]);
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
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, `Principal: ${RECIPIENT_A}\nMemo: none\nPrincipal: ${RECIPIENT_B}\nMemo: none`);
    assert.equal(nodes.get('relay-setup-configuration-hash').textContent, 'ab'.repeat(32));
  });
});

test('recipient memo controls serialize exact bytes, count bytes, and invalidate stale accounts', async () => {
  await withDom(async (nodes) => {
    let viewArgs;
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoMode.value = 'Hex';
    recipient.memoInput.value = '0x00 ff 80';
    const harness = controllerHarness({
      getView: async (args) => {
        viewArgs = args;
        return { Ok: viewFor({ recipients: args.surplus_recipients }) };
      },
    });
    harness.controller.bindPane();
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    assert.equal(recipient.memoCount.textContent, '3/32 bytes');
    await harness.controller.submitConfiguration();
    assert.deepEqual(viewArgs.surplus_recipients[0].Principal.memo, [0x00, 0xff, 0x80]);
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, '—');

    recipient.memoInput.value = '0x00 ff 81';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    recipient.memoMode.value = 'Text';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: recipient.memoMode });
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(recipient.memoMode.value, 'Hex');
    assert.equal(recipient.memoInput.value, '0x00 ff 81');
    assert.equal(recipient.memoError.textContent, '');
    assert.match(recipient.memoNotice.textContent, /cannot be represented as Text without changing them/i);
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), null);

    recipient.memoInput.value = '68 69';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    recipient.memoMode.value = 'Text';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoInput.value, 'hi');
    recipient.memoMode.value = 'Hex';
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoInput.value, '6869');
  });
});

test('memo counters distinguish syntax errors from actual 32-byte and 33-byte values', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness();
    harness.controller.bindPane();
    const list = nodes.get('relay-setup-recipient-list');

    recipient.memoInput.value = 'a'.repeat(32);
    list.listeners.get('input')({ target: recipient.memoInput });
    assert.equal(recipient.memoCount.textContent, '32/32 bytes');
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), null);

    recipient.memoInput.value = 'a'.repeat(33);
    list.listeners.get('input')({ target: recipient.memoInput });
    assert.equal(recipient.memoCount.textContent, '33/32 bytes');
    assert.equal(recipient.memoError.textContent, 'Memo is 33 bytes; maximum is 32 bytes.');
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), 'true');

    recipient.memoInput.value = '';
    list.listeners.get('input')({ target: recipient.memoInput });
    recipient.memoMode.value = 'Hex';
    list.listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoMode.value, 'Hex');
    recipient.memoInput.value = 'ff'.repeat(32);
    list.listeners.get('input')({ target: recipient.memoInput });
    assert.equal(recipient.memoCount.textContent, '32/32 bytes');
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), null);

    recipient.memoInput.value = 'ff'.repeat(33);
    list.listeners.get('input')({ target: recipient.memoInput });
    assert.equal(recipient.memoCount.textContent, '33/32 bytes');
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), 'true');

    recipient.memoInput.value = '0x0z';
    list.listeners.get('input')({ target: recipient.memoInput });
    assert.equal(recipient.memoCount.textContent, '—/32 bytes');
    assert.match(recipient.memoError.textContent, /non-hexadecimal/i);
  });
});

test('memo mode conversion accepts only an exact post-assignment byte round trip', async () => {
  await withDom(async (nodes) => {
    let lastViewArgs;
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoMode.value = 'Hex';
    recipient.memoInput.value = '0x68 69';
    const harness = controllerHarness({
      getView: async (args) => {
        lastViewArgs = args;
        return { Ok: viewFor({ recipients: args.surplus_recipients }) };
      },
    });
    harness.controller.bindPane();
    const list = nodes.get('relay-setup-recipient-list');

    recipient.memoMode.value = 'Text';
    list.listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoMode.value, 'Text');
    assert.equal(recipient.memoInput.value, 'hi');
    recipient.memoMode.value = 'Hex';
    list.listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoInput.value, '6869');

    recipient.memoInput.value = 'efbbbf61';
    list.listeners.get('input')({ target: recipient.memoInput });
    recipient.memoMode.value = 'Text';
    list.listeners.get('change')({ target: recipient.memoMode });
    if (recipient.memoMode.value === 'Text') {
      assert.deepEqual([...parseRelayMemo('Text', recipient.memoInput.value)], [0xef, 0xbb, 0xbf, 0x61]);
      recipient.memoMode.value = 'Hex';
      list.listeners.get('change')({ target: recipient.memoMode });
    }
    assert.equal(recipient.memoMode.value, 'Hex');
    assert.equal(recipient.memoInput.value, 'efbbbf61');

    recipient.memoInput.value = 'ff';
    list.listeners.get('input')({ target: recipient.memoInput });
    recipient.memoMode.value = 'Text';
    list.listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoMode.value, 'Hex');
    assert.equal(recipient.memoInput.value, 'ff');
    assert.equal(recipient.memoCount.textContent, '1/32 bytes');
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), null);
    assert.equal(nodes.get('relay-setup-submit').disabled, false);
    assert.match(recipient.memoNotice.textContent, /Hexadecimal mode was retained/i);

    recipient.memoInput.value = '0a';
    list.listeners.get('input')({ target: recipient.memoInput });
    recipient.memoMode.value = 'Text';
    list.listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoMode.value, 'Hex');
    assert.equal(recipient.memoInput.value, '0a');
    assert.deepEqual([...parseRelayMemo('Hex', recipient.memoInput.value)], [0x0a]);

    recipient.memoInput.value = '00ff81';
    list.listeners.get('input')({ target: recipient.memoInput });
    recipient.memoMode.value = 'Text';
    list.listeners.get('change')({ target: recipient.memoMode });
    assert.equal(recipient.memoMode.value, 'Hex');
    assert.equal(recipient.memoInput.value, '00ff81');
    assert.deepEqual([...parseRelayMemo('Hex', recipient.memoInput.value)], [0x00, 0xff, 0x81]);
    assert.equal(recipient.memoInput.getAttribute('aria-invalid'), null);
    assert.equal(nodes.get('relay-setup-submit').disabled, false);
    await harness.controller.submitConfiguration();
    assert.deepEqual(lastViewArgs.surplus_recipients[0].Principal.memo, [0x00, 0xff, 0x81]);
  });
});

test('canonical memo rendering always shows hex and only safe UTF-8 text', async () => {
  await withDom(async (nodes) => {
    const unsafeMemoBytes = [
      '\u202e', '\u2066', '\u2067', '\u2068', '\u2069', '\ufeff', '\u2028', '\u2029',
      '\u0001', '   ',
    ].map((text) => [...new TextEncoder().encode(text)]);
    unsafeMemoBytes.push([0xff, 0x00]);
    const safeTexts = ['alpha', '<script>', 'memo 123'];
    const harness = controllerHarness({
      view: viewFor({
        recipients: [
          ...unsafeMemoBytes.map((memo) => ({
            Principal: { principal: Principal.fromText(RECIPIENT_A), memo },
          })),
          ...safeTexts.map((text) => ({
            Principal: {
              principal: Principal.fromText(RECIPIENT_B),
              memo: [...new TextEncoder().encode(text)],
            },
          })),
        ],
      }),
    });
    harness.controller.state.view = viewFor({
      recipients: [
        ...unsafeMemoBytes.map((memo) => ({
          Principal: { principal: Principal.fromText(RECIPIENT_A), memo },
        })),
        ...safeTexts.map((text) => ({
          Principal: {
            principal: Principal.fromText(RECIPIENT_B),
            memo: [...new TextEncoder().encode(text)],
          },
        })),
      ],
    });
    harness.controller.render();
    const summary = nodes.get('relay-setup-canonical-recipients');
    for (const memo of unsafeMemoBytes) {
      const hex = Buffer.from(memo).toString('hex');
      assert.match(summary.textContent, new RegExp(`Memo hex: ${hex}`));
    }
    for (const text of safeTexts) {
      const hex = Buffer.from(new TextEncoder().encode(text)).toString('hex');
      assert.match(summary.textContent, new RegExp(`Memo hex: ${hex}`));
      assert.match(summary.textContent, new RegExp(`Memo text: ${text.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&')}`));
    }
    assert.equal((summary.textContent.match(/Memo text:/gu) || []).length, safeTexts.length);
    assert.equal(summary.innerHTML, '');
  });
});

test('memo-only changes replace the account solely from the authoritative Historian view', async () => {
  await withDom(async (nodes) => {
    const accountA = setupAccount();
    const accountB = { ...setupAccount(), subaccount: [Array(32).fill(9)] };
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const harness = controllerHarness({
      getView: async (args) => ({
        Ok: viewFor({
          recipients: args.surplus_recipients,
          account: args.surplus_recipients[0].Principal.memo[0] === 0x42 ? accountB : accountA,
        }),
      }),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const first = nodes.get('relay-setup-icrc-account').textContent;
    recipient.memoInput.value = 'B';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    await harness.controller.submitConfiguration();
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, first);
    assert.equal(harness.calls.view, 2);
  });
});

test('memo edits make pending setup, Governance, and notify responses stale', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const pendingView = deferred();
    const harness = controllerHarness({ getView: async () => pendingView.promise });
    harness.controller.bindPane();
    const submitPending = harness.controller.submitConfiguration();
    while (harness.calls.view === 0) await Promise.resolve();
    recipient.memoInput.value = 'changed';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    pendingView.resolve({ Ok: viewFor() });
    await submitPending;
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });

  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, '42');
    recipient.type.value = 'Neuron';
    const pendingGovernance = deferred();
    const harness = controllerHarness({ loadNeuron: async () => pendingGovernance.promise });
    harness.controller.bindPane();
    const submitPending = harness.controller.submitConfiguration();
    while (harness.calls.neurons.length === 0) await Promise.resolve();
    recipient.memoInput.value = 'changed';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    pendingGovernance.resolve({ owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] });
    await submitPending;
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });

  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    const pendingNotify = deferred();
    const harness = controllerHarness({ notifyRelay: async () => pendingNotify.promise });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const createPending = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    recipient.memoInput.value = 'changed';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.memoInput });
    pendingNotify.resolve({ Active: { relay_canister_id: Principal.fromText(RELAY) } });
    await createPending;
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(nodes.get('relay-setup-existing-relay').innerHTML, '');
  });
});

test('memo-mode interactions make pending setup, Governance, and notify responses stale', async () => {
  const switchMemoMode = (nodes, recipient, requested) => {
    recipient.memoMode.value = requested;
    nodes.get('relay-setup-recipient-list').listeners.get('change')({ target: recipient.memoMode });
  };

  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoInput.value = 'hi';
    const pendingView = deferred();
    const harness = controllerHarness({ getView: async () => pendingView.promise });
    harness.controller.bindPane();
    const submitPending = harness.controller.submitConfiguration();
    while (harness.calls.view === 0) await Promise.resolve();
    switchMemoMode(nodes, recipient, 'Hex');
    pendingView.resolve({ Ok: viewFor() });
    await submitPending;
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });

  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, '42');
    recipient.type.value = 'Neuron';
    recipient.memoInput.value = 'hi';
    const pendingGovernance = deferred();
    const harness = controllerHarness({ loadNeuron: async () => pendingGovernance.promise });
    harness.controller.bindPane();
    const submitPending = harness.controller.submitConfiguration();
    while (harness.calls.neurons.length === 0) await Promise.resolve();
    switchMemoMode(nodes, recipient, 'Hex');
    pendingGovernance.resolve({ owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] });
    await submitPending;
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });

  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, RECIPIENT_A);
    recipient.memoInput.value = 'hi';
    const pendingNotify = deferred();
    const harness = controllerHarness({ notifyRelay: async () => pendingNotify.promise });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    const createPending = harness.controller.createRelay();
    while (harness.calls.notify === 0) await Promise.resolve();
    switchMemoMode(nodes, recipient, 'Hex');
    pendingNotify.resolve({ Active: { relay_canister_id: Principal.fromText(RELAY) } });
    await createPending;
    assert.equal(nodes.get('relay-setup-existing-relay').innerHTML, '');
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });
});

test('same targets with a different recipient trigger a new view and setup account', async () => {
  await withDom(async (nodes) => {
    const accountA = setupAccount();
    const accountB = { ...setupAccount(), subaccount: [Array(32).fill(9)] };
    const harness = controllerHarness({
      getView: async (args) => ({
        Ok: viewFor({
          recipients: args.surplus_recipients.map((recipient) => recipient.Principal.principal.toText()),
          account: args.surplus_recipients[0].Principal.principal.toText() === RECIPIENT_A ? accountA : accountB,
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
        : { Ok: viewFor({ recipients: args.surplus_recipients.map((recipient) => recipient.Principal.principal.toText()) }) },
    });
    const first = submit(nodes, harness, TARGET_A, RECIPIENT_A);
    while (harness.calls.view === 0) await Promise.resolve();
    await submit(nodes, harness, TARGET_A, RECIPIENT_B);
    pending.resolve({ Ok: viewFor({ recipients: [RECIPIENT_A] }) });
    await first;
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, `Principal: ${RECIPIENT_B}\nMemo: none`);
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

test('eventless mutation while a polling refresh resolves stops stale polling', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingPoll = deferred();
    const harness = controllerHarness({
      balance: 299_999_999n,
      getView: async (_args, call) => (call === 1 ? { Ok: viewFor() } : pendingPoll.promise),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    assert.equal(harness.hasScheduledPoll(), true);
    harness.runPoll();
    while (harness.calls.view < 2) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingPoll.resolve({ Ok: viewFor() });
    await flushMicrotasks();
    assert.equal(harness.hasScheduledPoll(), false);
    assert.equal(harness.controller.state.loading, false);
    assertCheckedConfigurationInvalidated(nodes, harness);
  });
});

test('eventless mutation while a polling refresh rejects stops stale polling', async () => {
  await withDom(async (nodes) => {
    const target = seedTargetRow(nodes, TARGET_A);
    seedRecipientRow(nodes, RECIPIENT_A);
    const pendingPoll = deferred();
    const harness = controllerHarness({
      balance: 299_999_999n,
      getView: async (_args, call) => (call === 1 ? { Ok: viewFor() } : pendingPoll.promise),
    });
    harness.controller.bindPane();
    await harness.controller.submitConfiguration();
    assert.equal(harness.hasScheduledPoll(), true);
    harness.runPoll();
    while (harness.calls.view < 2) await Promise.resolve();
    target.input.value = TARGET_B;
    pendingPoll.reject(new Error('poll unavailable'));
    await flushMicrotasks();
    assert.equal(harness.hasScheduledPoll(), false);
    assert.equal(harness.controller.state.loading, false);
    assertCheckedConfigurationInvalidated(nodes, harness);
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
    assert.equal(notifyArgs.surplus_recipients[0].Principal.principal.toText(), RECIPIENT_A);
    assert.deepEqual(notifyArgs.surplus_recipients[0].Principal.memo, []);
    assert.equal('surplus_recipient_principals' in notifyArgs, false);
    assert.equal(harness.calls.governanceActors, 0);
  });
});

test('neuron recipients serialize as bigint and preflight through NNS Governance', async () => {
  await withDom(async (nodes) => {
    let viewArgs;
    const callOrder = [];
    const harness = controllerHarness({
      getView: async (args) => {
        callOrder.push('historian');
        viewArgs = args;
        return { Ok: viewFor({ recipients: [{ Neuron: 42n }] }) };
      },
      loadNeuron: async () => {
        callOrder.push('governance');
        return { owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] };
      },
    });
    await submit(nodes, harness, TARGET_A, '42', ['Neuron']);
    assert.equal(harness.calls.governanceActors, 1);
    assert.deepEqual(harness.calls.neurons, [42n]);
    assert.deepEqual(callOrder, ['historian', 'governance']);
    assert.deepEqual(viewArgs.surplus_recipients, [{ Neuron: { neuron_id: 42n, memo: [] } }]);
    assert.equal(nodes.get('relay-setup-canonical-recipients').textContent, 'Neuron: 42\nMemo: none');
  });
});

test('unverified neuron reads the Historian view but never exposes payment details', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      loadNeuron: async () => { throw new Error('not public'); },
    });
    await submit(nodes, harness, TARGET_A, '123', ['Neuron']);
    assert.equal(harness.calls.view, 1);
    assert.equal(harness.calls.balance, 0);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(harness.controller.state.view, null);
    assert.match(nodes.get('relay-setup-status').textContent, /Could not verify neuron 123 as publicly readable by NNS Governance/i);
    assert.doesNotMatch(nodes.get('relay-setup-status').textContent, /must be public/i);
  });
});

test('existing Active neuron configuration remains visible when the neuron loader would fail', async () => {
  await withDom(async (nodes) => {
    const active = { Active: { relay_canister_id: Principal.fromText(RELAY) } };
    const harness = controllerHarness({
      view: viewFor({ recipients: [{ Neuron: 42n }], state: active }),
      loadNeuron: async () => { throw new Error('governance unavailable'); },
    });
    await submit(nodes, harness, TARGET_A, '42', ['Neuron']);
    assert.equal(harness.calls.view, 1);
    assert.equal(harness.calls.governanceActors, 0);
    assert.deepEqual(harness.calls.neurons, []);
    assert.equal(harness.calls.balance, 0);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Active');
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
  });
});

test('existing InProgress neuron configuration skips Governance preflight', async () => {
  await withDom(async (nodes) => {
    const inProgress = { InProgress: { phase: { CreateDispatched: null }, relay_canister_id: [] } };
    const harness = controllerHarness({
      view: viewFor({ recipients: [{ Neuron: 42n }], state: inProgress }),
      loadNeuron: async () => { throw new Error('governance unavailable'); },
    });
    await submit(nodes, harness, TARGET_A, '42', ['Neuron']);
    assert.equal(harness.calls.view, 1);
    assert.equal(harness.calls.governanceActors, 0);
    assert.deepEqual(harness.calls.neurons, []);
    assert.equal(harness.calls.balance, 0);
    assert.match(nodes.get('relay-setup-status-label').textContent, /CreateDispatched/);
    assert.equal(harness.calls.intervals, 1);
  });
});

test('existing ManualRecoveryRequired neuron configuration skips Governance preflight', async () => {
  await withDom(async (nodes) => {
    const manualRecovery = {
      ManualRecoveryRequired: {
        phase: { RelayFunded: null },
        relay_canister_id: [Principal.fromText(RELAY)],
        message: 'operator investigation is required',
      },
    };
    const harness = controllerHarness({
      view: viewFor({
        recipients: [{ Neuron: 42n }],
        account: null,
        state: manualRecovery,
      }),
      loadNeuron: async () => { throw new Error('governance unavailable'); },
    });
    await submit(nodes, harness, TARGET_A, '42', ['Neuron']);
    assert.equal(harness.calls.view, 1);
    assert.equal(harness.calls.governanceActors, 0);
    assert.deepEqual(harness.calls.neurons, []);
    assert.equal(harness.calls.balance, 0);
    assert.match(nodes.get('relay-setup-status').textContent, /Manual\s*Recovery\s*Required/i);
    assert.match(nodes.get('relay-setup-status-label').textContent, /operator investigation is required/);
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(harness.calls.intervals, 0);
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
    assert.equal(harness.calls.view, 1);
    assert.equal(harness.calls.balance, 0);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });
});

test('stale neuron preflight completion cannot expose a view after a value change', async () => {
  await withDom(async (nodes) => {
    seedTargetRow(nodes, TARGET_A);
    const recipient = seedRecipientRow(nodes, '42');
    recipient.type.value = 'Neuron';
    const pending = deferred();
    const harness = controllerHarness({ loadNeuron: async () => pending.promise });
    harness.controller.bindPane();
    const first = harness.controller.submitConfiguration();
    while (harness.calls.neurons.length === 0) await Promise.resolve();
    recipient.input.value = '43';
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient.input });
    pending.resolve({ owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] });
    await first;
    assert.equal(harness.calls.view, 1);
    assert.equal(harness.calls.balance, 0);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
  });
});

test('polling a new neuron configuration never repeats Governance preflight', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({ balance: 299_999_999n });
    await submit(nodes, harness, TARGET_A, '42', ['Neuron']);
    assert.equal(harness.calls.governanceActors, 1);
    assert.deepEqual(harness.calls.neurons, [42n]);
    assert.equal(harness.calls.intervals, 1);
    await harness.controller.refresh();
    assert.equal(harness.calls.view, 2);
    assert.equal(harness.calls.governanceActors, 1);
    assert.deepEqual(harness.calls.neurons, [42n]);
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
    assert.equal(harness.calls.notify, 1);
    assert.equal(harness.controller.state.creating, false);
    assert.notEqual(harness.controller.state.checkedConfigurationFingerprint, '');
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
  });
});

test('ManualRecoveryRequired notify result renders phase, Relay ID, and error and stops polling', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      notify: {
        ManualRecoveryRequired: {
          phase: { FinalizationAttempted: null },
          relay_canister_id: [Principal.fromText(RELAY)],
          message: 'final state mismatch',
        },
      },
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.match(nodes.get('relay-setup-status-label').textContent, /FinalizationAttempted/);
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

test('synchronous pre-invocation notify failure keeps its concrete error', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      notifyRelay: () => { throw new Error('local notify encoding failed'); },
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assert.match(nodes.get('relay-setup-status').textContent, /local notify encoding failed/);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
    assert.notEqual(harness.controller.state.checkedConfigurationFingerprint, '');
    assert.equal(harness.calls.notify, 1);
  });
});

test('unchanged-form notify rejection requires a fresh authoritative check', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      notifyRelay: async () => { throw new Error('notification transport unavailable'); },
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assertCheckedConfigurationInvalidated(nodes, harness);
    assert.match(
      nodes.get('relay-setup-status').textContent,
      /may have been processed.*current state could not be confirmed.*check.*again/i,
    );
    assert.doesNotMatch(
      nodes.get('relay-setup-status').textContent,
      /notification transport unavailable/i,
    );
    assert.equal(harness.calls.notify, 1);
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 1);
  });
});

test('unchanged-form post-notify refresh rejection requires a fresh authoritative check', async () => {
  await withDom(async (nodes) => {
    const harness = controllerHarness({
      getView: async (_args, call) => {
        if (call === 1) return { Ok: viewFor() };
        throw new Error('post-notify refresh unavailable');
      },
    });
    await submit(nodes, harness);
    await harness.controller.createRelay();
    assertCheckedConfigurationInvalidated(nodes, harness);
    assert.match(
      nodes.get('relay-setup-status').textContent,
      /request returned.*current state could not be refreshed.*check.*again/i,
    );
    assert.doesNotMatch(
      nodes.get('relay-setup-status').textContent,
      /post-notify refresh unavailable/i,
    );
    assert.equal(harness.calls.notify, 1);
    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 1);
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
    assert.equal(harness.calls.notify, 1);
    assert.equal(harness.controller.state.creating, false);
    assert.notEqual(harness.controller.state.checkedConfigurationFingerprint, '');
    assert.ok(harness.calls.intervals >= 1);
    assert.match(nodes.get('relay-setup-status-label').textContent, /CreateDispatched/);
  });
});

test('funded finalization phases expose an explicit resume action without funding or factory availability', async () => {
  for (const phase of ['RelayFunded', 'FinalizationAttempted']) {
    await withDom(async (nodes) => {
      const inProgress = {
        InProgress: {
          phase: { [phase]: null },
          relay_canister_id: [Principal.fromText(RELAY)],
        },
      };
      const harness = controllerHarness({
        view: viewFor({ account: null, factoryAvailable: false, state: inProgress }),
      });
      await submit(nodes, harness);
      assert.equal(nodes.get('relay-setup-create-panel').hidden, false);
      assert.equal(nodes.get('relay-setup-create').textContent, 'Resume finalization');
      assert.equal(nodes.get('relay-setup-create').disabled, false);
      assert.equal(nodes.get('relay-setup-finalization-resume-note').hidden, false);
      assert.match(
        nodes.get('relay-setup-finalization-resume-note').textContent,
        /does not repeat ICP, CMC, creation, installation, or Relay-funding operations/i,
      );
      assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
      assert.equal(harness.calls.balance, 0);
      assert.ok(harness.hasScheduledPoll());
    });
  }
});

test('earlier in-progress phases never expose the finalization resume action', async () => {
  for (const phase of [
    'Reserved',
    'ProbingTargets',
    'CmcTransferPrepared',
    'CmcTransferAccepted',
    'CmcNotifySucceeded',
    'CreateDispatched',
    'ChildCreated',
    'CodeInstalled',
    'RelayFundingPrepared',
  ]) {
    await withDom(async (nodes) => {
      const inProgress = {
        InProgress: { phase: { [phase]: null }, relay_canister_id: [] },
      };
      const harness = controllerHarness({
        view: viewFor({ account: null, state: inProgress }),
      });
      await submit(nodes, harness);
      assert.equal(nodes.get('relay-setup-create-panel').hidden, true, phase);
      assert.equal(nodes.get('relay-setup-finalization-resume-note').hidden, true, phase);
      assert.equal(nodes.get('relay-setup-create').textContent, 'Create Relay', phase);
    });
  }
});

test('Resume finalization submits the canonical configuration once and polling remains query-only', async () => {
  await withDom(async (nodes) => {
    const inProgress = {
      InProgress: {
        phase: { FinalizationAttempted: null },
        relay_canister_id: [Principal.fromText(RELAY)],
      },
    };
    const pending = deferred();
    const harness = controllerHarness({
      view: viewFor({ account: null, factoryAvailable: false, state: inProgress }),
      notifyRelay: async () => pending.promise,
    });
    harness.controller.bindPane();
    await submit(nodes, harness);

    nodes.get('relay-setup-create').listeners.get('click')();
    while (harness.calls.notify === 0) await Promise.resolve();
    assert.equal(nodes.get('relay-setup-create').disabled, true);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Resuming finalization…');
    assert.deepEqual(harness.calls.notifyArgs, [{
      target_canister_ids: [Principal.fromText(TARGET_A)],
      surplus_recipients: [{
        Principal: { principal: Principal.fromText(RECIPIENT_A), memo: [] },
      }],
    }]);

    pending.resolve(inProgress);
    while (harness.calls.view < 2) await Promise.resolve();
    await flushMicrotasks();
    assert.equal(harness.calls.notify, 1);
    assert.equal(nodes.get('relay-setup-create').textContent, 'Resume finalization');
    assert.equal(nodes.get('relay-setup-create').disabled, false);
    assert.ok(harness.hasScheduledPoll());

    harness.runPoll();
    while (harness.calls.view < 3) await Promise.resolve();
    await flushMicrotasks();
    assert.equal(harness.calls.notify, 1);
    assert.ok(harness.calls.view >= 3);
  });
});

test('Active notification suppresses resume after a stale FinalizationAttempted query', async () => {
  await withDom(async (nodes) => {
    const staleInProgress = {
      InProgress: {
        phase: { FinalizationAttempted: null },
        relay_canister_id: [Principal.fromText(RELAY)],
      },
    };
    const harness = controllerHarness({
      view: viewFor({ account: null, state: staleInProgress }),
      notify: { Active: { relay_canister_id: Principal.fromText(RELAY) } },
    });
    harness.controller.bindPane();
    await submit(nodes, harness);

    await harness.controller.createRelay();

    assert.equal(harness.calls.notify, 1);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Active');
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
    assert.equal(nodes.get('relay-setup-existing-relay').hidden, false);
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
    assert.equal(nodes.get('relay-setup-finalization-resume-note').hidden, true);
    assert.equal(harness.hasScheduledPoll(), false);

    nodes.get('relay-setup-create').listeners.get('click')();
    await flushMicrotasks();
    assert.equal(harness.calls.notify, 1);
  });
});

test('ManualRecoveryRequired notification suppresses resume after a stale RelayFunded query', async () => {
  await withDom(async (nodes) => {
    const staleInProgress = {
      InProgress: {
        phase: { RelayFunded: null },
        relay_canister_id: [Principal.fromText(RELAY)],
      },
    };
    const harness = controllerHarness({
      view: viewFor({ account: null, state: staleInProgress }),
      notify: {
        ManualRecoveryRequired: {
          phase: { RelayFunded: null },
          relay_canister_id: [Principal.fromText(RELAY)],
          message: 'final state mismatch',
        },
      },
    });
    await submit(nodes, harness);

    await harness.controller.createRelay();

    assert.equal(harness.calls.notify, 1);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Manual recovery required');
    assert.match(nodes.get('relay-setup-status-label').textContent, /final state mismatch/);
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
    assert.equal(nodes.get('relay-setup-finalization-resume-note').hidden, true);
    assert.equal(harness.hasScheduledPoll(), false);

    await harness.controller.createRelay();
    assert.equal(harness.calls.notify, 1);
  });
});

test('form edits invalidate a resumable finalization before notification', async () => {
  await withDom(async (nodes) => {
    const inProgress = {
      InProgress: {
        phase: { RelayFunded: null },
        relay_canister_id: [Principal.fromText(RELAY)],
      },
    };
    const harness = controllerHarness({
      view: viewFor({ account: null, state: inProgress }),
    });
    harness.controller.bindPane();
    await submit(nodes, harness);
    const recipient = nodes.get('relay-setup-recipient-list')
      .querySelectorAll('[data-relay-recipient-input]')[0];
    recipient.value = RECIPIENT_B;
    nodes.get('relay-setup-recipient-list').listeners.get('input')({ target: recipient });
    nodes.get('relay-setup-create').listeners.get('click')();
    await flushMicrotasks();
    assert.equal(harness.calls.notify, 0);
    assert.equal(harness.controller.state.checkedConfigurationFingerprint, '');
    assert.equal(nodes.get('relay-setup-create-panel').hidden, true);
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
    const manual = { ManualRecoveryRequired: { phase: { RelayFunded: null }, relay_canister_id: [Principal.fromText(RELAY)], message: 'finalization failed' } };
    const harness = controllerHarness();
    await submit(nodes, harness);
    harness.setView(viewFor({ account: null, state: manual }));
    await harness.controller.refresh();
    assert.ok(harness.calls.clears >= 1);
    assert.match(nodes.get('relay-setup-status-label').textContent, /finalization failed/);
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
  assert.match(markup, /Targets, recipient destinations, and exact memo bytes determine the setup address/i);
  assert.match(markup, /No IO recipient is added automatically/i);
  assert.match(markup, /incorrectly selected Relay configuration are not automatically refundable/i);
  assert.doesNotMatch(markup, /surplus ICP will automatically be routed to the IO neuron/i);
});

test('ICRC account rendering remains stable', () => {
  assert.match(icrcAccountText(setupAccount()), new RegExp(`^${HISTORIAN}-`));
});
