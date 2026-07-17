import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';

import {
  createRelaySetupController,
  icrcAccountText,
} from '../src/app/relay-setup-controller.js';
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
    this.href = '';
    this.title = '';
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  removeAttribute(name) {
    if (name === 'href') this.href = '';
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
    'relay-setup-result',
    'relay-setup-prompt-text',
    'relay-setup-summary',
    'relay-setup-status',
    'relay-setup-status-label',
    'relay-setup-factory',
    'relay-setup-minimum',
    'relay-setup-balance',
    'relay-setup-icrc-account-link',
    'relay-setup-icrc-account',
    'copy-relay-setup-icrc-account',
    'relay-setup-account-identifier-link',
    'relay-setup-account-identifier',
    'copy-relay-setup-account-identifier',
    'relay-setup-warning',
    'relay-setup-existing-relay',
    'relay-setup-recovery-details',
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
    setTimeout(callback) {
      callback();
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
  minimum = 300_000_000n,
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
    current_required_e8s: [],
    nominal_minimum_e8s: 300_000_000n,
    payment_allowed: paymentAllowed,
    payment_blocked_reason: paymentBlockedReason,
    existing_relay: existingRelay,
    status,
    factory_available: factoryAvailable,
    warning_text: [],
  };
}

function recoveryView({ target, message }) {
  return {
    target_canister_id: Principal.fromText(target),
    status: { ManualRecoveryRequired: null },
    last_error: [message],
    relay_canister_id: [],
    setup_account_identifier: 'legacy-account',
    setup_amount_seen_e8s: 300_000_000n,
    setup_amount_processed_e8s: 0n,
    cycle_conversion_e8s: [94_950_000n],
    cycles_minted: [1_000_000_000_000n],
    configured_relay_create_attach_cycles: 2_000_000_000_000n,
    cycle_transfer: [{
      kind: { CmcConversion: null },
      from_account_identifier: 'from',
      to_account_identifier: 'to',
      amount_e8s: 94_950_000n,
      fee_e8s: 10_000n,
      created_at_time_nanos: 1n,
      block_index: [37_414_364n],
      completed: true,
    }],
    relay_funding_transfer: [],
    existing_relay_sweep_transfer: [],
    refund_transfer_count: 0,
    relay_create_attempt: [{
      target_canister_id: Principal.fromText(target),
      created_at_ts: 99n,
      initial_cycles: 1_000_000_000_000n,
      create_attach_cycles: 1_000_000_000_000n,
    }],
    created_at_ts: 1n,
    updated_at_ts: 2n,
  };
}

test('relay setup renders full ICRC payment account fixture', () => {
  const account = {
    owner: Principal.fromText('j5gs6-uiaaa-aaaar-qb5cq-cai'),
    subaccount: [[
      0x6e, 0xb0, 0x38, 0xb9, 0x7c, 0x48, 0xa7, 0x47,
      0xe0, 0x9a, 0x06, 0xfd, 0xfc, 0xba, 0x97, 0xee,
      0x05, 0x1b, 0x34, 0xc6, 0x25, 0xce, 0x15, 0xac,
      0x8e, 0x94, 0x66, 0xe8, 0x2d, 0x1b, 0x3d, 0xac,
    ]],
  };

  assert.equal(
    icrcAccountText(account),
    'j5gs6-uiaaa-aaaar-qb5cq-cai-ij5xrtq.6eb038b97c48a747e09a06fdfcba97ee051b34c625ce15ac8e9466e82d1b3dac',
  );
});

test('relay setup payment markup focuses on useful account identifiers', () => {
  const html = readFileSync(new URL('../../public/index.html', import.meta.url), 'utf8');
  assert.match(html, /ICRC payment account/);
  assert.match(html, /Alternative account identifier/);
  assert.match(html, /id="copy-relay-setup-icrc-account"[^>]*>Copy<\/button>/);
  assert.match(html, /id="relay-setup-icrc-account-link"/);
  assert.match(html, /id="copy-relay-setup-account-identifier"[^>]*>Copy<\/button>/);
  assert.match(html, /id="relay-setup-account-identifier-link"/);
  assert.match(html, /placeholder="Canister ID"/);
  assert.match(html, /id="relay-setup-warning" hidden/);
  assert.match(html, /id="relay-setup-result"[\s\S]*tracker-empty-state[\s\S]*id="relay-setup-prompt-text"[\s\S]*Enter a canister ID to check/);
  assert.match(html, /id="relay-setup-summary" hidden/);
  assert.match(html, /id="relay-setup-payment-details" hidden/);
  assert.doesNotMatch(html, /Raw setup subaccount/);
  assert.doesNotMatch(html, /Legacy account identifier/);
});

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

test('relay setup renders prompt before a target is checked', async () => {
  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController();

    controller.render();

    assert.equal(nodes.get('relay-setup-summary').hidden, true);
    assert.equal(nodes.get('relay-setup-result').hidden, false);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
  });
});

test('relay setup displays payment details for a new unfunded target without notifying', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const view = setupView({ target, historian: historianId });
  const calls = [];
  const copiedValues = [];

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
            async notify_relay_setup(arg) {
              calls.push(['notify', arg.toText()]);
              return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 1n } };
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
      copyTextToClipboard: async (value) => {
        copiedValues.push(value);
      },
      hostProvider: () => 'https://example.test',
    });
    controller.bindPane();
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-result').hidden, true);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Not funded');
    assert.equal(nodes.get('relay-setup-minimum').textContent, '3 ICP');
    assert.equal(nodes.get('relay-setup-balance').textContent, '0.00000001 ICP');
    assert.equal(nodes.get('relay-setup-factory').textContent, 'Available');
    assert.equal(nodes.get('relay-setup-warning').textContent, '');
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, icrcAccountText(view.setup_account));
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, view.setup_account_identifier);
    assert.equal(
      nodes.get('relay-setup-icrc-account-link').href,
      `https://dashboard.internetcomputer.org/account/${view.setup_account_identifier}`,
    );
    assert.equal(
      nodes.get('relay-setup-account-identifier-link').href,
      `https://dashboard.internetcomputer.org/account/${view.setup_account_identifier}`,
    );
    await nodes.get('copy-relay-setup-icrc-account').listeners.get('click')();
    await nodes.get('copy-relay-setup-account-identifier').listeners.get('click')();
    assert.deepEqual(copiedValues, [
      icrcAccountText(view.setup_account),
      view.setup_account_identifier,
    ]);
    assert.equal(nodes.get('relay-setup-existing-relay').hidden, true);
    assert.equal(nodes.get('relay-setup-refund').hidden, true);
    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), []);
  });
});

test('relay setup hides payment details when backend reports funded target recovery failure', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const message = 'no supported cycles-observation route could read the target balance';
  const calls = [];

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
            calls.push(['status']);
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            calls.push(['notify']);
            return { Failed: { status: { ManualRecoveryRequired: null }, message } };
          },
        },
      }),
      ledgerActorFactory: () => {
        calls.push(['ledger']);
        return {
          async icrc1_balance_of() {
            calls.push(['balance']);
            return 300_000_000n;
          },
        };
      },
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;
    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-status').textContent, 'Manual recovery required');
    assert.equal(nodes.get('relay-setup-status-label').textContent, message);
    assert.equal(nodes.get('relay-setup-warning').hidden, true);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, '—');
    assert.equal(controller.state.polling, false);
    assert.deepEqual(calls, [['view'], ['status'], ['ledger'], ['balance'], ['notify'], ['view']]);
  });
});

test('relay setup displays current requirement after insufficient-rate notify result', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const view = setupView({ target, historian: historianId, minimum: 300_000_000n });

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            return view;
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            return {
              InsufficientForCurrentRate: {
                required_e8s: 425_000_000n,
                current_balance_e8s: 300_000_000n,
              },
            };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 300_000_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-status').textContent, 'Below current requirement');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'Current balance 3 ICP is below current required 4.25 ICP.');
    assert.equal(nodes.get('relay-setup-minimum').textContent, '3 ICP');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, view.setup_account_identifier);
  });
});

test('relay setup transient notify rejection displays error without payment details', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';

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
            throw new Error('temporary historian rejection');
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 300_000_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-status').textContent, 'temporary historian rejection');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, '—');
  });
});

test('relay setup clears stale details while loading a new target', async () => {
  const targetA = '22255-zqaaa-aaaas-qf6uq-cai';
  const targetB = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  let resolveTargetB;
  const targetBView = new Promise((resolve) => {
    resolveTargetB = resolve;
  });

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view(request) {
            const target = request.target_canister_id.toText();
            if (target === targetB) await targetBView;
            return setupView({ target, historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 1n } };
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

    nodes.get('relay-setup-target-input').value = targetA;
    await controller.submitTarget();
    assert.equal(nodes.get('relay-setup-payment-details').hidden, false);
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, '—');

    nodes.get('relay-setup-target-input').value = targetB;
    const submitB = controller.submitTarget();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(nodes.get('relay-setup-summary').hidden, true);
    assert.equal(nodes.get('relay-setup-result').hidden, false);
    assert.equal(nodes.get('relay-setup-prompt-text').textContent, 'Checking relay setup…');
    assert.equal(nodes.get('relay-setup-warning').hidden, true);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, '—');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);

    resolveTargetB();
    await submitB;
    assert.equal(nodes.get('relay-setup-status').textContent, 'Not funded');
    assert.notEqual(nodes.get('relay-setup-icrc-account').textContent, '—');
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

test('relay setup existing active relay sweeps only when balance is above threshold', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const relay = Principal.fromText('br5f7-7uaaa-aaaaa-qaaca-cai');
  const activeRelay = {
    relay_canister_id: relay,
    target_canister_id: Principal.fromText(target),
    kind: { SelfService: null },
    created_at_ts: [],
  };
  const calls = [];
  let balance = 1n;

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            calls.push(['view']);
            return setupView({ target, historian: historianId, existingRelay: [activeRelay] });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { SweptToExistingRelay: { relay: activeRelay, amount_e8s: balance, block_index: 99n } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          calls.push(['balance']);
          return balance;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();
    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), []);
    assert.equal(nodes.get('relay-setup-existing-relay').hidden, false);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);

    calls.length = 0;
    balance = 20_000n;
    await controller.submitTarget();
    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), [['notify', target]]);
    assert.equal(nodes.get('relay-setup-status').textContent, 'SweptToExistingRelay');
  });
});

test('relay setup shows processing state instead of not funded after payment is detected', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  let resolveNotify;
  const notifyStarted = new Promise((resolve) => {
    resolveNotify = resolve;
  });

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
            await notifyStarted;
            return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 20_000n } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 300_000_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    const submit = controller.submitTarget();
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(nodes.get('relay-setup-status').textContent, 'Processing payment');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'Notifying historian…');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);

    resolveNotify();
    await submit;
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

test('relay setup renders payment-blocked view without notifying', async () => {
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
            throw new Error('notify should not be called for a payment-blocked view');
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          calls.push(['balance']);
          return 0n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-status').textContent, 'Not funded');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'target must not be a configured protocol dependency');
    assert.deepEqual(calls, [['balance']]);
  });
});

test('relay_setup_factory_unavailable_hides_payment_details_and_does_not_notify_when_unfunded', async () => {
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
            calls.push(['view']);
            return setupView({
              target,
              historian: historianId,
              status: { PaymentNotAllowed: null },
              paymentAllowed: false,
              paymentBlockedReason: ['relay factory is disabled'],
              factoryAvailable: false,
            });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            calls.push(['notify']);
            throw new Error('notify should not be called for an unfunded payment-blocked view');
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          calls.push(['balance']);
          return 0n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-factory').textContent, 'Unavailable');
    assert.equal(nodes.get('relay-setup-status').textContent, 'Payment not allowed');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'relay factory is disabled');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, '—');
    assert.deepEqual(calls, [['view'], ['balance']]);
  });
});

test('relay_setup_factory_unavailable_notifies_funded_blocked_account_for_refund_recovery', async () => {
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
            calls.push(['view']);
            return setupView({
              target,
              historian: historianId,
              status: calls.some((call) => call[0] === 'notify')
                ? { Refunding: null }
                : { PaymentNotAllowed: null },
              paymentAllowed: false,
              paymentBlockedReason: ['relay factory is disabled'],
              factoryAvailable: false,
            });
          },
          async get_relay_setup_recovery_view() {
            calls.push(['recovery']);
            return {
              ...recoveryView({ target, message: 'refund in progress' }),
              status: { Refunding: null },
              last_error: [],
            };
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { RefundPending: { reason: 'refund in progress' } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          calls.push(['balance']);
          return 50_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-factory').textContent, 'Unavailable');
    assert.equal(nodes.get('relay-setup-status').textContent, 'Refund pending');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'refund in progress');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, '—');
    assert.deepEqual(calls, [
      ['view'],
      ['balance'],
      ['notify', target],
      ['view'],
      ['recovery'],
    ]);
  });
});

test('relay setup hides payment details while manual recovery details are loading', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';

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
              status: { ManualRecoveryRequired: null },
            });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            throw new Error('notify should not run below dust');
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

    assert.equal(nodes.get('relay-setup-status').textContent, 'Manual recovery required');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'Loading recovery details…');
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.equal(nodes.get('relay-setup-icrc-account').textContent, '—');
    assert.equal(nodes.get('relay-setup-account-identifier').textContent, '—');
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
            return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 20_000n } };
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
    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), []);

    assert.equal(intervals.length, 1);
    await controller.refreshBalanceAndMaybeNotify(target);

    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), [['notify', target]]);
    assert.equal(nodes.get('relay-setup-balance').textContent, '0.0002 ICP');
  });
});

test('relay setup polling does not repeatedly notify an unfunded account', async () => {
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
            return setupView({ target, historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            calls.push(['notify', arg.toText()]);
            return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 0n } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          calls.push(['balance']);
          return 0n;
        },
      }),
      setIntervalFn: () => 7,
      clearIntervalFn: () => {},
      hostProvider: () => 'https://example.test',
    });
    nodes.get('relay-setup-target-input').value = target;

    await controller.submitTarget();
    await controller.refreshBalanceAndMaybeNotify(target);
    await controller.refreshBalanceAndMaybeNotify(target);

    assert.deepEqual(calls.filter((call) => call[0] === 'notify'), []);
    assert.equal(calls.filter((call) => call[0] === 'balance').length, 3);
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
            return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 20_000n } };
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

    assert.equal(calls.filter((call) => call[0] === 'notify' && call[1] === targetA).length, 0);
    assert.equal(controller.state.targetText, targetB);
  });
});

test('relay setup late initial response for old target does not overwrite newer target', async () => {
  const targetA = '22255-zqaaa-aaaas-qf6uq-cai';
  const targetB = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  let resolveFirstView;
  const firstView = new Promise((resolve) => {
    resolveFirstView = resolve;
  });

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view(request) {
            const target = request.target_canister_id.toText();
            if (target === targetA) {
              await firstView;
            }
            return setupView({ target, historian: historianId });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup(arg) {
            return { BelowMinimum: { minimum_e8s: 300_000_000n, current_balance_e8s: 20_000n, target: arg } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 0n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });

    nodes.get('relay-setup-target-input').value = targetA;
    const submitA = controller.submitTarget();
    await new Promise((resolve) => setTimeout(resolve, 0));

    nodes.get('relay-setup-target-input').value = targetB;
    await controller.submitTarget();
    assert.equal(controller.state.targetText, targetB);
    assert.equal(controller.state.target.toText(), targetB);

    resolveFirstView();
    await submitA;

    assert.equal(controller.state.targetText, targetB);
    assert.equal(controller.state.target.toText(), targetB);
    assert.equal(controller.state.view.target_canister_id.toText(), targetB);
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
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'manual intervention required');
    assert.equal(controller.state.polling, false);
    assert.equal(cleared, 1);
  });
});

test('relay setup shows concrete create_canister failure from recovery view', async () => {
  const target = '22255-zqaaa-aaaas-qf6uq-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  const message = 'create_canister required 1307692307692 cycles but only 1000000000000 cycles were attached';
  let copiedDiagnostic = '';

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      copyTextToClipboard: async (value) => {
        copiedDiagnostic = value;
      },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view() {
            return setupView({ target, historian: historianId, status: { FailedRetryable: null } });
          },
          async get_relay_setup_recovery_view(request) {
            assert.equal(request.target_canister_id.toText(), target);
            return recoveryView({ target, message });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            return { Failed: { status: { FailedRetryable: null }, message: 'Retryable failure' } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 300_000_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });
    controller.bindPane();
    nodes.get('relay-setup-target-input').value = target;
    await controller.submitTarget();

    assert.equal(nodes.get('relay-setup-status').textContent, 'Manual recovery required');
    assert.equal(nodes.get('relay-setup-status-label').textContent, message);
    assert.equal(nodes.get('relay-setup-payment-details').hidden, true);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /37414364/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /CMC conversion/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /Cycles required for relay/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /Cycles attached to create call/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /Total received/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /Amount converted/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /Operator diagnostic/);
    assert.match(nodes.get('relay-setup-recovery-details').innerHTML, /Copy details/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /<dt>Status<\/dt>/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Cycle conversion e8s/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Create attachment/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Configured create attachment/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Setup seen/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Setup processed/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /<dt>Relay<\/dt><dd class="pane-detail-value">—<\/dd>/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Raw Relay Wasm review hash/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /Relay install payload hash/);
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /7e1f98468e68235cd003e537016d298527387077f7b48faed893bc94f984d844/);

    const copyButton = { id: 'relay-setup-copy-diagnostic', textContent: 'Copy details' };
    await nodes.get('relay-setup-recovery-details').listeners.get('click')({ target: copyButton });
    const parsedDiagnostic = JSON.parse(copiedDiagnostic);
    assert.equal(parsedDiagnostic.last_error, message);
    assert.match(parsedDiagnostic.last_error, /1307692307692/);
    assert.equal(copyButton.textContent, 'Copy details');
  });
});

test('relay setup discards stale recovery response after target change', async () => {
  const targetA = '22255-zqaaa-aaaas-qf6uq-cai';
  const targetB = 'br5f7-7uaaa-aaaaa-qaaca-cai';
  const historianId = 'qaa6y-5yaaa-aaaaa-aaafa-cai';
  let resolveRecoveryA;
  const recoveryA = new Promise((resolve) => {
    resolveRecoveryA = resolve;
  });

  await withRelaySetupDom(async (nodes) => {
    const controller = createRelaySetupController({
      frontendConfig: { historianCanisterId: historianId },
      createHistorian: async () => ({
        agent: { test: true },
        historian: {
          async get_relay_setup_view(request) {
            return setupView({ target: request.target_canister_id.toText(), historian: historianId });
          },
          async get_relay_setup_recovery_view(request) {
            if (request.target_canister_id.toText() === targetA) {
              await recoveryA;
              return recoveryView({ target: targetA, message: 'target A leaked error' });
            }
            return recoveryView({ target: targetB, message: 'target B current error' });
          },
          async get_public_status() {
            return { ledger_canister_id: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai') };
          },
          async notify_relay_setup() {
            return { Failed: { status: { FailedRetryable: null }, message: 'failed' } };
          },
        },
      }),
      ledgerActorFactory: () => ({
        async icrc1_balance_of() {
          return 300_000_000n;
        },
      }),
      hostProvider: () => 'https://example.test',
    });

    nodes.get('relay-setup-target-input').value = targetA;
    const submitA = controller.submitTarget();
    await new Promise((resolve) => setTimeout(resolve, 0));
    nodes.get('relay-setup-target-input').value = targetB;
    await controller.submitTarget();
    resolveRecoveryA();
    await submitA;

    assert.equal(nodes.get('relay-setup-status').textContent, 'Manual recovery required');
    assert.equal(nodes.get('relay-setup-status-label').textContent, 'target B current error');
    assert.doesNotMatch(nodes.get('relay-setup-recovery-details').innerHTML, /target A leaked error/);
  });
});

test('relay setup controller does not construct browser blackhole actors', () => {
  const source = readFileSync(new URL('../src/app/relay-setup-controller.js', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /Actor\.createActor/);
  assert.doesNotMatch(source, /canister_status/);
  assert.doesNotMatch(source, /BLACKHOLE_CANISTER_IDS/);
});

test('relay setup generated declarations do not require check_cycles_visibility', () => {
  const declarations = readFileSync(new URL('../declarations/jupiter_historian/jupiter_historian.did.js', import.meta.url), 'utf8');
  assert.doesNotMatch(declarations, /check_cycles_visibility/);
});
