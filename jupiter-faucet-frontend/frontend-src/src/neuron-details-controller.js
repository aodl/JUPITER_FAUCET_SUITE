export function createNeuronDetailsController({
  loadNeuronDetails,
  renderStakePane,
  renderStakeNeuronStatus,
  normalizeError,
  setGlobalNeuronError = (value) => {
    if (typeof window !== 'undefined') {
      window.__JUPITER_NEURON_ERROR__ = value;
    }
  },
} = {}) {
  if (typeof loadNeuronDetails !== 'function') {
    throw new Error('createNeuronDetailsController requires loadNeuronDetails');
  }
  if (typeof renderStakePane !== 'function') {
    throw new Error('createNeuronDetailsController requires renderStakePane');
  }
  if (typeof renderStakeNeuronStatus !== 'function') {
    throw new Error('createNeuronDetailsController requires renderStakeNeuronStatus');
  }
  if (typeof normalizeError !== 'function') {
    throw new Error('createNeuronDetailsController requires normalizeError');
  }

  const state = {
    inFlight: false,
    loaded: false,
    value: null,
    error: null,
  };

  return {
    state,
    reset() {
      state.inFlight = false;
      state.loaded = false;
      state.value = null;
      state.error = null;
      setGlobalNeuronError(null);
    },
    async ensureLoaded(data) {
      if (state.inFlight || state.loaded) return;
      state.inFlight = true;
      renderStakePane(data, null, { neuronLoading: true });
      renderStakeNeuronStatus({ loading: true });
      try {
        const neuron = await loadNeuronDetails();
        state.loaded = true;
        state.value = neuron;
        state.error = neuron ? null : 'Public neuron details unavailable';
        setGlobalNeuronError(state.error);
        renderStakePane(data, neuron, { neuronError: state.error });
        renderStakeNeuronStatus({ error: state.error });
      } catch (error) {
        state.loaded = false;
        state.value = null;
        state.error = normalizeError(error);
        setGlobalNeuronError(state.error);
        renderStakePane(data, null, { neuronError: state.error });
        renderStakeNeuronStatus({ error: state.error });
        console.info('Public neuron details unavailable; core dashboard metrics load independently.', error);
      } finally {
        state.inFlight = false;
      }
    },
  };
}
