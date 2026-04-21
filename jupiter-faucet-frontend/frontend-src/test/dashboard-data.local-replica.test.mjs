import test from 'node:test';
import assert from 'node:assert/strict';

import { accountIdentifierHex, loadDashboardData } from '../src/dashboard-data.js';

const host = process.env.FRONTEND_DASHBOARD_TEST_HOST;
const historianCanisterId = process.env.FRONTEND_DASHBOARD_TEST_HISTORIAN_CANISTER_ID;
const expected = process.env.FRONTEND_DASHBOARD_EXPECTED_JSON
  ? JSON.parse(process.env.FRONTEND_DASHBOARD_EXPECTED_JSON)
  : null;

function normalizeNumericString(text) {
  return typeof text === 'string' && /^[0-9_]+$/.test(text) ? text.replaceAll('_', '') : text;
}

function asString(value) {
  if (value === null || value === undefined) return null;
  const text = typeof value === 'bigint' ? value.toString() : String(value);
  return normalizeNumericString(text);
}

function optionalBigIntString(opt) {
  if (!Array.isArray(opt) || opt.length === 0 || opt[0] === null || opt[0] === undefined) {
    return null;
  }
  return asString(opt[0]);
}

function variantLabel(value) {
  if (!value || Array.isArray(value)) return null;
  const keys = Object.keys(value);
  return keys.length === 1 ? keys[0] : null;
}

const maybeTest = host && historianCanisterId && expected ? test : test.skip;

maybeTest('loadDashboardData matches the expected local replica fixture', async () => {
  const data = await loadDashboardData({
    historianCanisterId,
    host,
    local: true,
  });

  assert.equal(asString(data.stakeE8s), expected.stakeE8s);
  assert.equal(asString(data.counts?.total_output_e8s), expected.counts?.totalOutputE8s);
  assert.equal(asString(data.counts?.total_rewards_e8s), expected.counts?.totalRewardsE8s);
  assert.equal(asString(data.counts?.registered_canister_count), expected.counts?.registeredCanisterCount);
  assert.equal(asString(data.counts?.qualifying_contribution_count), expected.counts?.qualifyingContributionCount);

  if (expected.status) {
    assert.ok(data.status, 'expected historian status to be present');
    assert.equal(data.status.ledger_canister_id.toText(), expected.status.ledgerCanisterId);
    assert.equal(asString(data.status.index_interval_seconds), expected.status.indexIntervalSeconds);
    assert.equal(asString(data.status.cycles_interval_seconds), expected.status.cyclesIntervalSeconds);
    assert.equal(accountIdentifierHex(data.status.staking_account), expected.status.stakingAccountIdentifier);
    assert.equal(optionalBigIntString(data.status.last_index_run_ts) !== null, expected.status.lastIndexRunTsPresent);
    assert.equal(optionalBigIntString(data.status.last_completed_cycles_sweep_ts) !== null, expected.status.lastCyclesSweepTsPresent);
  }

  if (expected.registered) {
    assert.ok(data.registered, 'expected registered canister summaries to be present');
    assert.equal(asString(data.registered.total), expected.registered.total);
    assert.equal(data.registered.items.length, expected.registered.items.length);
    for (let i = 0; i < expected.registered.items.length; i += 1) {
      const actual = data.registered.items[i];
      const exp = expected.registered.items[i];
      assert.equal(actual.canister_id.toText(), exp.canisterId);
      assert.equal(asString(actual.qualifying_contribution_count), exp.qualifyingContributionCount);
      assert.equal(asString(actual.total_qualifying_contributed_e8s), exp.totalQualifyingContributedE8s);
      assert.equal(optionalBigIntString(actual.last_contribution_ts) !== null, exp.lastContributionTsPresent);
      assert.equal(optionalBigIntString(actual.latest_cycles), exp.latestCycles ?? null);
      assert.equal(optionalBigIntString(actual.last_cycles_probe_ts) !== null, exp.lastCyclesProbeTsPresent);
    }
  }

  if (expected.recent) {
    assert.ok(data.recent, 'expected recent contributions to be present');
    assert.equal(data.recent.items.length, expected.recent.items.length);
    for (let i = 0; i < expected.recent.items.length; i += 1) {
      const actual = data.recent.items[i];
      const exp = expected.recent.items[i];
      const actualPrincipal = Array.isArray(actual.canister_id) ? actual.canister_id[0] : actual.canister_id;
      const actualMemoText = Array.isArray(actual.memo_text) ? actual.memo_text[0] : actual.memo_text;
      const actualCanister = actualPrincipal
        ? actualPrincipal.toText()
        : (actualMemoText ?? '');
      assert.equal(actualCanister, exp.canisterId);
      assert.equal(asString(actual.tx_id), exp.txId);
      assert.equal(asString(actual.amount_e8s), exp.amountE8s);
      assert.equal(actual.counts_toward_faucet, exp.countsTowardFaucet);
      if (exp.outcomeCategory !== undefined) {
        assert.equal(variantLabel(actual.outcome_category), exp.outcomeCategory);
      }
    }
  }

  if (expected.errors) {
    assert.equal(data.errors.stake, expected.errors.stake ?? null);
  }
});
