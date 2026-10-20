import test from 'node:test';
import assert from 'node:assert/strict';
import { Principal } from '@icp-sdk/core/principal';
import { JUPITER_STAKING_ACCOUNT, JUPITER_NEURON_ID } from '../src/app/config.js';
import { createStakePaneController } from '../src/app/stake-pane-controller.js';

test('staking display, links, tooltips and copy remain canonical despite conflicting or absent status', () => {
  const originalDocument = globalThis.document;
  const nodes = new Map();
  for (const id of ['stake-pane-account-link', 'stake-neuron-id-link', 'stake-pane-balance',
    'how-staking-account-address', 'how-staking-account-identifier',
    'how-staking-account-link', 'how-staking-account-identifier-link', 'stake-neuron-maturity']) {
    nodes.set(id, { textContent: '', title: '', querySelector: () => null });
  }
  globalThis.document = { getElementById: (id) => nodes.get(id) };
  const copies = new Map();
  const bonuses = [];
  try {
    const controller = createStakePaneController({
      neuronId: JUPITER_NEURON_ID,
      simulatorController: { setAgeBonus: (...args) => bonuses.push(args) },
      setCopyButton: (id, value) => copies.set(id, value),
    });
    const matching = { owner: Principal.fromText(JUPITER_STAKING_ACCOUNT.owner), subaccount: [Array.from(Buffer.from(JUPITER_STAKING_ACCOUNT.subaccountHex, 'hex'))] };
    const conflicting = { owner: Principal.fromText('ryjl3-tyaaa-aaaaa-aaaba-cai'), subaccount: [Array(32).fill(9)] };
    for (const account of [matching, conflicting, null]) {
      controller.renderHowItWorksAccount();
      controller.renderStakePane({ status: account ? { staking_account: account } : null, stakeE8s: 100_000_000n }, {
        maturity_e8s_equivalent: 200_000_000n,
        aging_since_timestamp_seconds: BigInt(Math.floor(Date.now() / 1000)),
        created_timestamp_seconds: 1n,
        followees: [],
      });
      assert.equal(nodes.get('stake-pane-account-link').textContent, JUPITER_STAKING_ACCOUNT.address);
      assert.equal(nodes.get('stake-pane-account-link').title, JUPITER_STAKING_ACCOUNT.address);
      for (const id of ['stake-pane-account-link', 'how-staking-account-link', 'how-staking-account-identifier-link']) {
        assert.equal(nodes.get(id).href, `https://dashboard.internetcomputer.org/account/${JUPITER_STAKING_ACCOUNT.explorerAccountHex}`);
      }
      assert.equal(copies.get('copy-how-staking-account')(), JUPITER_STAKING_ACCOUNT.address);
      assert.equal(copies.get('copy-how-staking-account-identifier')(), JUPITER_STAKING_ACCOUNT.explorerAccountHex);
      assert.equal(nodes.get('stake-neuron-id-link').textContent, JUPITER_NEURON_ID.toString());
      assert.match(nodes.get('stake-pane-balance').textContent, /1/);
      assert.match(nodes.get('stake-neuron-maturity').textContent, /2/);
    }
    assert.equal(bonuses.length, 3);
  } finally {
    globalThis.document = originalDocument;
  }
});
