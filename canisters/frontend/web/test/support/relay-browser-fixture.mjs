// Bundled only in memory by navbar-browser.test.mjs. Never shipped as an asset.
import { Principal } from '@icp-sdk/core/principal';
import { createRelaySetupController } from '../../src/app/relay-setup-controller.js';

const RELAY = 'br5f7-7uaaa-aaaaa-qaaca-cai';
const HISTORIAN = 'j5gs6-uiaaa-aaaar-qb5cq-cai';
const ledger = 'ryjl3-tyaaa-aaaaa-aaaba-cai';
const calls = { views: 0, notifications: 0, neurons: [], copied: [], args: [] };
let balance = 100_000_000n;
let scanState = { NotFunded: null };
let notifyResult = null;
let poll;
let pendingQuery;
let holdQuery = false;

function view(args) {
  const extra = BigInt(args.target_canister_ids.length - 1);
  return {
    canonical_target_canister_ids: args.target_canister_ids,
    canonical_surplus_recipients: args.surplus_recipients,
    setup_key_identifier: 'ab'.repeat(32),
    setup_account: 'NotFunded' in scanState ? [{
      owner: Principal.fromText(HISTORIAN), subaccount: [Array.from({ length: 32 }, (_, i) => i)],
    }] : [],
    target_count: args.target_canister_ids.length,
    surplus_recipient_count: args.surplus_recipients.length,
    singleton_nominal_minimum_e8s: 300_000_000n,
    extra_target_count: extra,
    extra_target_unit_charge_e8s: 25_000_000n,
    total_extra_target_charge_e8s: extra * 25_000_000n,
    nominal_minimum_e8s: 300_000_000n + extra * 25_000_000n,
    factory_available: true,
    state: scanState,
  };
}

const controller = createRelaySetupController({
  frontendConfig: { historianCanisterId: HISTORIAN },
  isLocalHost: () => false,
  createHistorian: async () => ({ agent: {}, historian: {
    async get_relay_configuration_view(args) {
      calls.views += 1;
      calls.args.push({ targets: args.target_canister_ids.map((p) => p.toText()), recipients: args.surplus_recipients });
      const response = { Ok: view(args) };
      if (holdQuery) {
        holdQuery = false;
        return new Promise((resolve) => { pendingQuery = { resolve: () => resolve(response) }; });
      }
      return response;
    },
    async notify_relay_configuration() {
      calls.notifications += 1;
      if (notifyResult) return notifyResult;
      scanState = { Active: { relay_canister_id: Principal.fromText(RELAY) } };
      return scanState;
    },
  } }),
  ledgerActorFactory: (id) => {
    if (id !== ledger) throw new Error('Unexpected fixture Ledger');
    return { icrc1_balance_of: async () => balance };
  },
  governanceActorFactory: () => ({}),
  publicNeuronLoader: async ({ neuronId }) => {
    calls.neurons.push(String(neuronId));
    return { owner: Principal.fromText(HISTORIAN), subaccount: [Array(32).fill(0)] };
  },
  copyTextToClipboard: async (value) => {
    calls.copied.push(value);
  },
  setIntervalFn: (callback) => { poll = callback; return 1; },
  clearIntervalFn: () => { poll = null; },
});
controller.bindPane();
window.relayFixture = {
  calls,
  controller,
  setBalance: (value) => { balance = BigInt(value); },
  setState(kind, phase = 'CreateDispatched') {
    scanState = kind === 'Active' ? { Active: { relay_canister_id: Principal.fromText(RELAY) } }
      : kind === 'NotFunded' ? { NotFunded: null }
        : { [kind]: { phase: { [phase]: null }, relay_canister_id: [Principal.fromText(RELAY)], message: 'The previous operation needs operator investigation.' } };
  },
  setNotify: (value) => { notifyResult = value; },
  holdQuery: () => { holdQuery = true; },
  releaseQuery: () => { pendingQuery.resolve(); pendingQuery = null; },
  poll: () => poll?.(),
};
