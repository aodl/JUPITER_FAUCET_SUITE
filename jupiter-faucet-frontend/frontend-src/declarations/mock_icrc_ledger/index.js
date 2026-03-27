import { Actor } from '@dfinity/agent';
import { idlFactory } from './mock_icrc_ledger.did.js';

export { idlFactory } from './mock_icrc_ledger.did.js';
export const canisterId = '';

export const createActor = (resolvedCanisterId, options = {}) => {
  const agent = options.agent;
  if (!agent) {
    throw new Error('createActor requires an HttpAgent instance');
  }
  return Actor.createActor(idlFactory, {
    agent,
    canisterId: resolvedCanisterId,
    ...(options.actorOptions || {}),
  });
};
