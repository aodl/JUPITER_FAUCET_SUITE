import { Principal } from '@icp-sdk/core/principal';
import { GOVERNANCE_CANISTER_ID } from '../app/config.js';

export class PublicNeuronStakingAccountError extends Error {
  constructor(message, code) {
    super(message);
    this.name = 'PublicNeuronStakingAccountError';
    this.code = code;
  }
}

export const PUBLIC_NEURON_VISIBILITY_MESSAGE = 'This neuron cannot be tracked from the public frontend unless it is configured as public. Public visibility is required so NNS Governance can return the staking subaccount used to calculate the neuron’s staking account.';

function neuronIdValue(neuron) {
  const id = Array.isArray(neuron?.id) ? neuron.id[0] : neuron?.id;
  return id?.id === undefined || id?.id === null ? null : BigInt(id.id);
}

export async function loadPublicNeuronStakingAccount({
  governance,
  neuronId,
  governanceCanisterId = GOVERNANCE_CANISTER_ID,
} = {}) {
  if (!governance || typeof governance.list_neurons !== 'function') {
    throw new PublicNeuronStakingAccountError('NNS Governance actor is unavailable.', 'governance-unavailable');
  }
  const id = typeof neuronId === 'bigint' ? neuronId : BigInt(neuronId);
  const response = await governance.list_neurons({
    neuron_ids: [id],
    include_neurons_readable_by_caller: false,
    include_empty_neurons_readable_by_caller: [false],
    include_public_neurons_in_full_neurons: [true],
    page_number: [],
    page_size: [],
    neuron_subaccounts: [],
  });
  const neuron = (response?.full_neurons || []).find((item) => neuronIdValue(item) === id);
  if (!neuron) {
    throw new PublicNeuronStakingAccountError(PUBLIC_NEURON_VISIBILITY_MESSAGE, 'not-public');
  }
  const account = neuron.account || neuron.cached_neuron_stake_e8s_account || [];
  const bytes = Array.from(account);
  if (bytes.length !== 32) {
    throw new PublicNeuronStakingAccountError('NNS Governance returned an invalid neuron staking subaccount.', 'invalid-subaccount');
  }
  return {
    owner: Principal.fromText(governanceCanisterId),
    subaccount: [bytes],
  };
}
