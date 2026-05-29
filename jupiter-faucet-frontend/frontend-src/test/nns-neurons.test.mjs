import test from 'node:test';
import assert from 'node:assert/strict';

import { loadPublicNeuronStakingAccount, PUBLIC_NEURON_VISIBILITY_MESSAGE } from '../src/data/nns-neurons.js';

test('loadPublicNeuronStakingAccount returns ICRC account for a public neuron', async () => {
  const account = Array.from({ length: 32 }, (_, index) => index);
  const result = await loadPublicNeuronStakingAccount({
    neuronId: 7n,
    governance: {
      async list_neurons(args) {
        assert.deepEqual(args.neuron_ids, [7n]);
        assert.deepEqual(args.include_public_neurons_in_full_neurons, [true]);
        return { full_neurons: [{ id: [{ id: 7n }], account }] };
      },
    },
  });
  assert.equal(result.owner.toText(), 'rrkah-fqaaa-aaaaa-aaaaq-cai');
  assert.deepEqual(result.subaccount, [account]);
});

test('loadPublicNeuronStakingAccount reports non-public neurons clearly', async () => {
  await assert.rejects(
    () => loadPublicNeuronStakingAccount({
      neuronId: 7n,
      governance: { async list_neurons() { return { full_neurons: [] }; } },
    }),
    { code: 'not-public', message: PUBLIC_NEURON_VISIBILITY_MESSAGE },
  );
});

test('loadPublicNeuronStakingAccount rejects wrong-size staking accounts', async () => {
  await assert.rejects(
    () => loadPublicNeuronStakingAccount({
      neuronId: 7n,
      governance: { async list_neurons() { return { full_neurons: [{ id: [{ id: 7n }], account: [1, 2, 3] }] }; } },
    }),
    { code: 'invalid-subaccount' },
  );
});
