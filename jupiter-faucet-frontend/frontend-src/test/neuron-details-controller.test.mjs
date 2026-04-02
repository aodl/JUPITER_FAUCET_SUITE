import test from 'node:test';
import assert from 'node:assert/strict';

import { createNeuronDetailsController } from '../src/neuron-details-controller.js';

test('neuron details can retry after a transient failure and then load successfully', async () => {
  const paneCalls = [];
  const statusCalls = [];
  const globalErrors = [];
  let attempts = 0;

  const originalConsoleInfo = console.info;
  console.info = () => {};
  const controller = createNeuronDetailsController({
    loadNeuronDetails: async () => {
      attempts += 1;
      if (attempts === 1) {
        throw new Error('temporary governance outage');
      }
      return { id: 'neuron-1' };
    },
    renderStakePane: (data, neuron, options = {}) => {
      paneCalls.push({ data, neuron, options });
    },
    renderStakeNeuronStatus: (options = {}) => {
      statusCalls.push(options);
    },
    normalizeError: (error) => `normalized:${error.message}`,
    setGlobalNeuronError: (value) => {
      globalErrors.push(value);
    },
  });

  try {
    await controller.ensureLoaded({ marker: 'dashboard' });
    assert.equal(attempts, 1);
  assert.equal(controller.state.loaded, false);
  assert.equal(controller.state.inFlight, false);
  assert.equal(controller.state.value, null);
  assert.equal(controller.state.error, 'normalized:temporary governance outage');

  await controller.ensureLoaded({ marker: 'dashboard' });
  assert.equal(attempts, 2);
  assert.equal(controller.state.loaded, true);
  assert.equal(controller.state.inFlight, false);
  assert.deepEqual(controller.state.value, { id: 'neuron-1' });
  assert.equal(controller.state.error, null);

  assert.deepEqual(statusCalls, [
    { loading: true },
    { error: 'normalized:temporary governance outage' },
    { loading: true },
    { error: null },
  ]);
  assert.deepEqual(globalErrors, ['normalized:temporary governance outage', null]);
    assert.deepEqual(paneCalls.map(({ neuron, options }) => ({ neuron, options })), [
      { neuron: null, options: { neuronLoading: true } },
      { neuron: null, options: { neuronError: 'normalized:temporary governance outage' } },
      { neuron: null, options: { neuronLoading: true } },
      { neuron: { id: 'neuron-1' }, options: { neuronError: null } },
    ]);
  } finally {
    console.info = originalConsoleInfo;
  }
});

test('neuron details does not launch a duplicate request while one is already in flight', async () => {
  const release = {};
  const firstAttempt = new Promise((resolve) => {
    release.resolve = resolve;
  });
  let attempts = 0;

  const controller = createNeuronDetailsController({
    loadNeuronDetails: async () => {
      attempts += 1;
      await firstAttempt;
      return { id: 'neuron-2' };
    },
    renderStakePane: () => {},
    renderStakeNeuronStatus: () => {},
    normalizeError: (error) => String(error?.message || error),
    setGlobalNeuronError: () => {},
  });

  const first = controller.ensureLoaded({ marker: 'dashboard' });
  const second = controller.ensureLoaded({ marker: 'dashboard' });

  assert.equal(controller.state.inFlight, true);
  assert.equal(attempts, 1);

  release.resolve();
  await Promise.all([first, second]);

  assert.equal(attempts, 1);
  assert.equal(controller.state.inFlight, false);
  assert.equal(controller.state.loaded, true);
  assert.deepEqual(controller.state.value, { id: 'neuron-2' });
  assert.equal(controller.state.error, null);
});
