import { Actor } from '@icp-sdk/core/agent';
import { idlFactory } from './icp_index.did.js';

export { idlFactory } from './icp_index.did.js';
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
