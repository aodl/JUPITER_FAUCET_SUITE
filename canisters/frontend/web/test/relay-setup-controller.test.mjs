import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@icp-sdk/core/principal';

import { createRelaySetupController } from '../src/app/relay-setup-controller.js';
import { accountIdentifierHex, bytesToHex, relaySetupSubaccount } from '../src/data/dashboard-transforms.js';

class FakeElement {
  constructor(id = '') {
    this.id = id;
    this.dataset = {};
    this.listeners = new Map();
    this.textContent = '';
    this.innerHTML = '';
    this.value = '';
    this.hidden = false;
    this.focused = false;
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  focus() {
    this.focused = true;
  }
}

async function withRelaySetupDom(fn) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const ids = [
    'relay-setup-form',
    'relay-setup-target-input',
    'relay-setup-status',
    'relay-setup-factory',
    'relay-setup-minimum',
    'relay-setup-balance',
    'relay-setup-subaccount',
    'relay-setup-account-identifier',
    'relay-setup-warning',
    'relay-setup-existing-relay',
    'relay-setup-payment-details',
    'relay-setup-refund',
  ];
  const nodes = new Map(ids.map((id) => [id, new FakeElement(id)]));

  globalThis.document = {
    getElementById(id) {
      return nodes.get(id) || null;
    },
  };
  globalThis.window = {
    location: { origin: 'https://example.test' },
    setInterval() {
      return 1;
    },
    clearInterval() {},
  };

  try {
    await fn(nodes);
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
  }
}

function setupView({
  target,
  historian,
  existingRelay = [],
  status = { NotFunded: null },
  minimum = 200_000_000n,
  paymentAllowed = true,
  paymentBlockedReason = [],
  factoryAvailable = true,
} = {}) {
  const setupAccount = {
    owner: Principal.fromText(historian),
    subaccount: [Array.from(relaySetupSubaccount(target))],
  };
  return {
    target_canister_id: Principal.fromText(target),
    setup_account: setupAccount,
    setup_account_identifier: accountIdentifierHex(setupAccount),
    minimum_e8s: minimum,
    payment_allowed: paymentAllowed,
    payment_blocked_reason: paymentBlockedReason,
    existing_relay: existingRelay,
    status,
    factory_available: factoryAvailable,
    relay_wasm_hash_hex: ['a'.repeat(64)],
    warning_text: ['Blackhole visibility warning'],
  };
}

test('relay setup rejects invalid target without loading actors', async () => {
  await withRelaySetupDom(async (nodes) => {
    let actorLoads = 0;
    const controller = createRelaySetupController({
      createHistorian: async () => {
        actorLoads += 1;
        throw new Error('not expected');
      },
    });
    controller.bindPane();
    nodes.get('relay-setup-target-input').value = 'not a principal';

    await nodes.get('relay-setup-form').listeners.get('submit')({ preventDefault() {} });

    assert.equal(actorLoads, 0);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Enter a valid target canister ID.');
    assert.equal(nodes.get('relay-setup-target-input').focused, true);
  });
});

test('relay setup renders setup account and does not notify below dust', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const view = setupView({ target, historian: historianId });
  const calls = [];

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      isLocalHost: () => true,
      createHistorian: async (args) => {
        calls.push(['createHistorian', args]);
        return {
          agent: { test: true },
          historian: {
            async get_relay_setup_view(request) {
              calls.push(['view', request.target_canister_id.toText()]);
              return view;
            },
            async get_public_status() {
              calls.push(['status']);
              return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
            },
            async notify_relay_setup() {
              calls.push(['notify']);
              throw new Error('notify should not be called below dust');
            },
          },
        };
      },
      ledgerActorFactory: (ledgerId, options) => {
        calls.push(['ledger', ledgerId, options]);
        return {
          async icrc1_balance_of(account) {
            calls.push(['balance', bytesToHex(account.subaccount[0])]);
            return 1n;
          },
        };
      },
      hostProvider: () => 'https://example.test',
    });
    controller.bindPane();
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-status').textContent, 'Not funded');
    assert.equal(nodes.get('relay-setup-minimum').textContent, '2 ICP');
    assert.equal(nodes.get('relay-setup-balance').textContent, '0.00000001 ICP');
    assert.equal(nodes.get('relay-setup-factory').textContent, 'Available');
    assert.equal(nodes.get('relay-setup-warning').textContent, 'Blackhole visibility warning');
    assert.equal(nodes.get('relay-setup-subaccount').textContent, bytesToHex(relaySetupSubaccount(target)));
    assert.equal(nodes.get('relay-setup-existing-relay').hidden, true);
    assert.equal(nodes.get('relay-setup-refund').hidden, true);
    assert.equal(calls.some((call) => call[0] === 'notify'), false);
  });
});

test('relay setup notifies above dust and renders active relay', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const relay = Principal.fromText('br5f7-7uaaa-aaaaa-qaaca-cai');
  const activeRelay = {
    relay_canister_id: relay,
    target_canister_id: Principal.fromText(target),
    kind: { SelfService: null },
    relay_wasm_hash_hex: [],
    created_at_ts: [],
  };
  const calls = [];

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            calls.push(['view']);
            return setupView({
              target,
              historian: historianId,
              existingRelay: calls.some((call) => call[0] === 'notify') ? [activeRelay] : [],
            });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { Active: { relay: activeRelay } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 200_010_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), [['notify', target]]);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Active');
    assert.equal(nodes.get('relay-setup-existing-relay').hidden, false);
    assert.match(nodes.get('relay-setup-existing-relay').innerHTML, /br5f7-7uaaa-aaaaa-qaaca-cai/);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
  });
});

test('relay setup has no refund button and renders automatic refunded result', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const calls = [];

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            return setupView({
              target,
              historian: historianId,
              status: { Refunded: null },
            });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { Refunded: { blocks: [7n] } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 1n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();
    assert.equal(nodes.get('relay-setup-refund').hidden, true);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Refunded');
    assert.deepEqual(calls, []);
  });
});

test('relay setup hides payment details but still notifies funded blocked target for auto-refund', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const calls = [];

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            return setupView({
              target,
              historian: historianId,
              paymentAllowed: false,
              paymentBlockedReason: ['target must not be a configured protocol dependency'],
            });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { RefundPending: { reason: 'refund pending index catch-up' } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 1_000_000_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.deepEqual(calls, [['notify', target]]);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Refund pending');
  });
});

test('relay setup polling refreshes later balance and notifies above dust', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const calls = [];
  const intervals = [];

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            calls.push(['view']);
            return setupView({ target, historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { BelowMinimum: { minimum_e8s: 200_000_000n, current_balance_e8s: 20_000n } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          const balanceCalls = calls.filter((call) => call[0] === 'balance').length;
          calls.push(['balance']);
          return balanceCalls === 0 ? 0n : 20_000n;
        },
      }),
      setIntervalFn: (callback) => {
        intervals.push(callback);
        return intervals.length;
      },
      clearIntervalFn: () => {},
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();
    assert.equal(nodes.get('relay-setup-balance').textContent, '0 ICP');
    assert.equal(calls.some((call) => call[0] === 'notify'), false);

    assert.equal(intervals.length, 1);
    await controller.refreshBalanceAndMaybeNotify(target);

    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), [['notify', target]]);
    assert.equal(nodes.get('relay-setup-balance').textContent, '0.0002 ICP');
  });
});

test('relay setup polling cancels old target after target change', async () => {
  const targetA = '22255-zqaaa-aaaas-qf6uq-cai';
  const targetB = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const intervals = [];
  const calls = [];

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view(request) {
            calls.push(['view', request.target_canister_id.toText()]);
            return setupView({ target: request.target_canister_id.toText(), historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { BelowMinimum: { minimum_e8s: 200_000_000n, current_balance_e8s: 20_000n } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 0n;
        },
      }),
      setIntervalFn: (callback) => {
        intervals.push(callback);
        return intervals.length;
      },
      clearIntervalFn: () => {},
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = targetA;
    await controller.submitTarget();
    nodes.get('relay-setup-target-input').value = targetB;
    await controller.submitTarget();

    assert.equal(intervals.length, 2);
    await controller.refreshBalanceAndMaybeNotify(targetA);

    assert.equal(calls.some((call) => call[0] === 'notify' && call[1] === targetA), false);
    assert.equal(controller.state.targetText, targetB);
  });
});

test('relay setup polling stops after active result', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  let cleared = 0;

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            return setupView({ target, historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            return { Active: { relay: null } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 20_000n;
        },
      }),
      setIntervalFn: () => 7,
      clearIntervalFn: () => {
        cleared += 1;
      },
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;
    await controller.submitTarget();

    assert.equal(controller.state.polling, false);
    assert.equal(cleared, 1);
  });
});

test('relay setup polling stops after manual recovery required result', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  let cleared = 0;

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            return setupView({ target, historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            return { Failed: { status: { ManualRecoveryRequired: null }, message: 'manual intervention required' } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 20_000n;
        },
      }),
      setIntervalFn: () => 7,
      clearIntervalFn: () => {
        cleared += 1;
      },
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;
    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-status').textContent, 'Manual recovery required');
    assert.equal(controller.state.polling, false);
    assert.equal(cleared, 1);
  });
});
