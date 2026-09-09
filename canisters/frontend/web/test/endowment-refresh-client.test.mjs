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

test('route queries enforce the committed refresh revision before UI state can consume them', async () => {
  const routes = [{ CyclesTopUp: { canister_id: 'target' } }];
  const historianActor = {
    async get_commitment_route_summaries(args) {
      assert.deepEqual(args, { routes });
      return { revision: [7n], items: [] };
    },
  };
  await assert.rejects(
    loadHistorianEndowmentRoutes({
      agent: {},
      historianActor,
      routes,
      minimumRevision: 8n,
    }),
    /older than the committed refresh revision/i,
  );
  const current = await loadHistorianEndowmentRoutes({
    agent: {},
    historianActor,
    routes,
    minimumRevision: 7n,
  });
  assert.equal(current.revision[0], 7n);
});

test('browser Candid keeps polling queries but omits the canister-only refresh update', () => {
  const productionDid = readFileSync(new URL('../../../historian/jupiter_historian.did', import.meta.url), 'utf8');
  const debugDid = readFileSync(new URL('../../../historian/jupiter_historian_debug.did', import.meta.url), 'utf8');
  const browserDid = readFileSync(new URL('../declarations/jupiter_historian/jupiter_historian.did.js', import.meta.url), 'utf8');
  for (const source of [productionDid, debugDid]) {
    assert.match(source, /refresh_endowments/);
    assert.match(source, /get_endowment_transaction_status/);
    assert.match(source, /newly_indexed_qualifying_endowments/);
    assert.match(source, /revision/);
  }
  assert.match(browserDid, /get_endowment_transaction_status/);
  assert.match(browserDid, /get_commitment_route_summaries/);
  assert.doesNotMatch(browserDid, /refresh_endowments|RefreshEndowments/);
  assert.match(productionDid, /refresh_endowments\s*:\s*\(\)\s*->/);
  assert.doesNotMatch(productionDid, /RefreshEndowmentsArgs/);
  assert.doesNotMatch(productionDid, /debug_driver_tick|debug_state/);
  assert.doesNotMatch(productionDid, /public.*tick|trigger.*scheduler/i);
});
