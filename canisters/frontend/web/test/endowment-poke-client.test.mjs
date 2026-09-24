import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import {
  loadHistorianEndowmentRoutes,
  loadHistorianEndowmentTransactionStatus,
} from '../src/app/agent.js';

test('transaction classification remains a separate bounded browser query', async () => {
  const historianActor = {
    async get_endowment_transaction_status(transactionId) {
      assert.equal(transactionId, 42n);
      return { transaction_id: transactionId, status: { NotYetObserved: null }, revision: 8n };
    },
  };
  const response = await loadHistorianEndowmentTransactionStatus({
    agent: {},
    historianActor,
    transactionId: 42,
  });
  assert.equal(response.transaction_id, 42n);
  assert.deepEqual(response.status, { NotYetObserved: null });
});

test('route queries retain their own revision', async () => {
  const routes = [{ CyclesTopUp: { canister_id: 'target' } }];
  const historianActor = {
    async get_commitment_route_summaries(args) {
      assert.deepEqual(args, { routes });
      return { revision: [7n], items: [] };
    },
  };
  const current = await loadHistorianEndowmentRoutes({ agent: {}, historianActor, routes });
  assert.equal(current.revision[0], 7n);
});

test('browser Candid keeps queries and omits Event Horizon poke', () => {
  const productionDid = readFileSync(new URL('../../../historian/jupiter_historian.did', import.meta.url), 'utf8');
  const debugDid = readFileSync(new URL('../../../historian/jupiter_historian_debug.did', import.meta.url), 'utf8');
  const browserDid = readFileSync(new URL('../declarations/jupiter_historian/jupiter_historian.did.js', import.meta.url), 'utf8');
  for (const source of [productionDid, debugDid]) {
    assert.match(source, /poke\s*:\s*\(vec nat64\)\s*->\s*\(\)/);
    assert.match(source, /get_endowment_transaction_status/);
    assert.doesNotMatch(source, /refresh_endowments|RefreshEndowments|EndowmentIndexProgress/);
  }
  assert.match(browserDid, /get_endowment_transaction_status/);
  assert.match(browserDid, /get_commitment_route_summaries/);
  assert.doesNotMatch(browserDid, /poke|refresh_endowments|RefreshEndowments/);
  assert.doesNotMatch(productionDid, /debug_driver_tick|debug_state/);
});
