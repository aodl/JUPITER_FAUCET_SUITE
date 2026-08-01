import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';

import {
  createRelaySetupController,
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
    this.textContent = '';
    this.innerHTML = '';
    this.value = '';
    this.hidden = false;
    this.disabled = false;
    this.focused = false;
  }

  addEventListener(type, listener) { this.listeners.set(type, listener); }
  focus() { this.focused = true; }
}

async function withDom(run) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const ids = [
    'relay-setup-form', 'relay-setup-target-input', 'relay-setup-result',
    'relay-setup-summary', 'relay-setup-status', 'relay-setup-status-label',
    'relay-setup-factory', 'relay-setup-target-count', 'relay-setup-canonical-targets',
    'relay-setup-base-minimum', 'relay-setup-extra-count', 'relay-setup-extra-unit',
    'relay-setup-extra-total', 'relay-setup-minimum', 'relay-setup-indicative',
    'relay-setup-balance', 'relay-setup-icrc-account', 'relay-setup-account-identifier',
    'relay-setup-payment-details', 'relay-setup-create-panel', 'relay-setup-create',
    'relay-setup-existing-relay', 'copy-relay-setup-icrc-account',
    'copy-relay-setup-account-identifier',
  ];
  const nodes = new Map(ids.map((id) => [id, new FakeElement(id)]));
  globalThis.document = { getElementById: (id) => nodes.get(id) || null };
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

function account() {
  return {
    owner: Principal.fromText(HISTORIAN),
    subaccount: [Array.from({ length: 32 }, (_, index) => index)],
  };
}

function availableView() {
  const setupAccount = account();
  return {
    canonical_target_canister_ids: [Principal.fromText(TARGET_A), Principal.fromText(TARGET_B)],
    setup_key_identifier: 'ab'.repeat(32),
    setup_account: [setupAccount],
    setup_account_identifier: [accountIdentifierHex(setupAccount)],
    target_count: 2,
    singleton_nominal_minimum_e8s: 300_000_000n,
    extra_target_count: 1n,
    extra_target_unit_charge_e8s: 25_000_000n,
    total_extra_target_charge_e8s: 25_000_000n,
    nominal_minimum_e8s: 325_000_000n,
    indicative_current_requirement_e8s: [340_000_000n],
    indicative_rate_timestamp_seconds: [1n],
    factory_available: true,
    state: { NotFunded: null },
  };
}

test('textarea parser accepts newline, comma, and whitespace separators', () => {
  for (const text of [`${TARGET_A}\n${TARGET_B}`, `${TARGET_A},${TARGET_B}`, `${TARGET_A} ${TARGET_B}`]) {
    assert.deepEqual(parseRelayTargetSet(text).map((value) => value.toText()), [TARGET_A, TARGET_B]);
  }
});

test('textarea parser rejects empty, invalid, duplicate, and 21-target input', () => {
  assert.throws(() => parseRelayTargetSet(''), /at least one/i);
  assert.throws(() => parseRelayTargetSet('not-a-principal'), /invalid/i);
  assert.throws(() => parseRelayTargetSet(`${TARGET_A}, ${TARGET_A}`), /duplicate/i);
  assert.throws(() => parseRelayTargetSet(Array.from({ length: 21 }, (_, index) => Principal.fromUint8Array(Uint8Array.of(index + 1)).toText()).join('\n')), /no more than 20/i);
});

test('check displays canonical pricing and balance without notifying automatically', async () => {
  await withDom(async (nodes) => {
    nodes.get('relay-setup-target-input').value = `${TARGET_B}\n${TARGET_A}`;
    let notifyCalls = 0;
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: HISTORIAN },
      createHistorian: async () => ({
        agent: {},
        historian: {
          get_relay_setup_view: async () => ({ Ok: availableView() }),
          get_public_status: async () => ({ ledger_canister_id: Principal.fromText(LEDGER) }),
          notify_relay_setup: async () => { notifyCalls += 1; return { Active: { relay_canister_id: Principal.fromText(RELAY) } }; },
        },
      }),
      ledgerActorFactory: () => ({ icrc1_balance_of: async () => 400_000_000n }),
    });
    await controller.submitTarget();
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, `${TARGET_A}\n${TARGET_B}`);
    assert.equal(nodes.get('relay-setup-target-count').textContent, '2');
    assert.equal(nodes.get('relay-setup-base-minimum').textContent, '3 ICP');
    assert.equal(nodes.get('relay-setup-extra-total').textContent, '0.25 ICP');
    assert.equal(nodes.get('relay-setup-minimum').textContent, '3.25 ICP');
    assert.equal(nodes.get('relay-setup-balance').textContent, '4 ICP');
    assert.equal(nodes.get('relay-setup-create').disabled, false);
    assert.equal(notifyCalls, 0);
  });
});

test('explicit Create Relay calls notify with only the target vector', async () => {
  await withDom(async (nodes) => {
    nodes.get('relay-setup-target-input').value = `${TARGET_A},${TARGET_B}`;
    const args = [];
    let active = false;
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: HISTORIAN },
      createHistorian: async () => ({
        agent: {},
        historian: {
          get_relay_setup_view: async () => active
            ? { Ok: { ...availableView(), setup_account: [], setup_account_identifier: [], state: { Active: { relay_canister_id: Principal.fromText(RELAY) } } } }
            : { Ok: availableView() },
          get_public_status: async () => ({ ledger_canister_id: Principal.fromText(LEDGER) }),
          notify_relay_setup: async (value) => { args.push(value); active = true; return { Active: { relay_canister_id: Principal.fromText(RELAY) } }; },
        },
      }),
      ledgerActorFactory: () => ({ icrc1_balance_of: async () => 400_000_000n }),
    });
    await controller.submitTarget();
    await controller.createRelay();
    assert.equal(args.length, 1);
    assert.deepEqual(Object.keys(args[0]), ['target_canister_ids']);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7/);
  });
});

test('late response for an old target set cannot overwrite a newer set', async () => {
  await withDom(async (nodes) => {
    const resolvers = [];
    const controller = createRelaySetupController({
      createHistorian: async () => ({
        agent: {},
        historian: {
          get_relay_setup_view: () => new Promise((resolve) => resolvers.push(resolve)),
          get_public_status: async () => ({ ledger_canister_id: Principal.fromText(LEDGER) }),
        },
      }),
      ledgerActorFactory: () => ({ icrc1_balance_of: async () => 0n }),
    });
    nodes.get('relay-setup-target-input').value = TARGET_A;
    const first = controller.submitTarget();
    while (resolvers.length < 1) await Promise.resolve();
    nodes.get('relay-setup-target-input').value = TARGET_B;
    const second = controller.submitTarget();
    while (resolvers.length < 2) await Promise.resolve();
    resolvers[1]({ Ok: { ...availableView(), canonical_target_canister_ids: [Principal.fromText(TARGET_B)], target_count: 1 } });
    await second;
    resolvers[0]({ Ok: { ...availableView(), canonical_target_canister_ids: [Principal.fromText(TARGET_A)], target_count: 1 } });
    await first;
    assert.equal(nodes.get('relay-setup-canonical-targets').textContent, TARGET_B);
  });
});

test('source and markup contain no payment proof, refund, quote, or automatic notify flow', () => {
  const source = readFileSync(new URL('../src/app/relay-setup-controller.js', import.meta.url), 'utf8');
  const markup = readFileSync(new URL('../../public/index.html', import.meta.url), 'utf8');
  const removedTerms = [
    `payment_${'block'}`,
    `request_relay_${'setup_refund'}`,
    `quote_relay_${'setup'}`,
    `list_relay_${'registrations'}`,
  ];
  for (const term of removedTerms) assert.equal(source.includes(term), false);
  assert.doesNotMatch(markup, /payment block|refund button|late-payment sweep/i);
  assert.match(markup, /Create Relay/);
  assert.match(markup, /not automatically refundable/);
});

test('ICRC account rendering remains stable', () => {
  assert.match(icrcAccountText(account()), new RegExp(`^${HISTORIAN}-`));
});
