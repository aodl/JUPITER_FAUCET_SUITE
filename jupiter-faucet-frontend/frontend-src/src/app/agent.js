import { HttpAgent } from '@icp-sdk/core/agent';
import { createActor as createHistorianActor } from '../../declarations/jupiter_historian/index.js';

const agentPromises = new Map();

export function normalizeError(error) {
  if (!error) return 'Unknown error';
  if (typeof error === 'string') return error;
  return error.message || String(error);
}

export function isMethodMissingError(error) {
  const text = normalizeError(error).toLowerCase();
  return text.includes('method') && (text.includes('not found') || text.includes('not part of the service'));
}

export function resetAgentCacheForTests() {
  agentPromises.clear();
}

async function getOrCreateAgent({ host, local, agent }) {
  if (agent) return agent;
  const key = `${host}::${local ? 'local' : 'remote'}`;
  if (!agentPromises.has(key)) {
    const agentPromise = (async () => {
      const httpAgent = await HttpAgent.create({
        host,
        verifyQuerySignatures: true,
      });
      if (local) {
        try {
          await httpAgent.fetchRootKey();
        } catch (error) {
          console.warn('Failed to fetch local root key', error);
        }
      }
      return httpAgent;
    })();
    agentPromise.catch(() => {
      if (agentPromises.get(key) === agentPromise) {
        agentPromises.delete(key);
      }
    });
    agentPromises.set(key, agentPromise);
  }
  return agentPromises.get(key);
}

export async function createHistorianClient({
  historianCanisterId,
  host,
  local = false,
  agent = null,
  historianActor = null,
  historianActorFactory = createHistorianActor,
} = {}) {
  if (!historianActor && !historianCanisterId) {
    throw new Error('Historian canister ID is not configured for this build');
  }

  const resolvedAgent = await getOrCreateAgent({ host, local, agent });

  try {
    return {
      agent: resolvedAgent,
      historian: historianActor || historianActorFactory(historianCanisterId, { agent: resolvedAgent }),
    };
  } catch (error) {
    throw new Error(normalizeError(error));
  }
}
