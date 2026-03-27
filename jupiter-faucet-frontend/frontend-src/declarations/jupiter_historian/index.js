import { Actor } from '@dfinity/agent';
import { idlFactory } from './jupiter_historian.did.js';

export { idlFactory } from './jupiter_historian.did.js';
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
