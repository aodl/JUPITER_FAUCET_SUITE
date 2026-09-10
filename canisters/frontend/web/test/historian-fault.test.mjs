import test from 'node:test';
import assert from 'node:assert/strict';

import { readOpt } from '../src/candid-opt.js';
import {
  buildHistorianFaultBannerText,
  readCommitmentIndexFault,
  readRouteIndexFault,
} from '../src/historian-fault.js';

test('readOpt returns null for a candid opt none array', () => {
  assert.equal(readOpt([]), null);
});

test('readOpt unwraps a candid opt some array', () => {
  assert.equal(readOpt([42n]), 42n);
});

test('readCommitmentIndexFault ignores a candid opt none array', () => {
  assert.equal(readCommitmentIndexFault({ commitment_index_fault: [] }), null);
});

const formatters = {
  formatTimestampSeconds: (value) => `ts:${value.toString()}`,
  formatInteger: (value) => `n:${value.toString()}`,
};

const commitmentFault = {
  observed_at_ts: 123n,
  last_cursor_tx_id: [51n],
  offending_tx_id: 49n,
  message: 'Non-monotonic transaction ids observed from the index.',
};

test('buildHistorianFaultBannerText hides the banner when neither fault is present', () => {
  assert.equal(buildHistorianFaultBannerText({}, formatters), null);
});

test('buildHistorianFaultBannerText formats a commitment fault', () => {
  const text = buildHistorianFaultBannerText(
    {
      commitment_index_fault: [commitmentFault],
    },
    formatters,
  );

  assert.equal(
    text,
    'Historian endowment indexing is degraded. First observed at ts:123. Last cursor: n:51. Offending tx: n:49. Non-monotonic transaction ids observed from the index.',
  );
});

test('buildHistorianFaultBannerText formats a route fault', () => {
  assert.equal(
    buildHistorianFaultBannerText(
      { route_index_fault: ['output route cursor is unsupported'] },
      formatters,
    ),
    'Historian output/rewards indexing is degraded. output route cursor is unsupported',
  );
});

test('buildHistorianFaultBannerText combines both faults in one warning', () => {
  assert.equal(
    buildHistorianFaultBannerText(
      {
        commitment_index_fault: [commitmentFault],
        route_index_fault: ['rewards route cursor is unsupported'],
      },
      formatters,
    ),
    'Historian endowment indexing is degraded. First observed at ts:123. Last cursor: n:51. Offending tx: n:49. Non-monotonic transaction ids observed from the index. Historian output/rewards indexing is degraded. rewards route cursor is unsupported',
  );
});

test('empty or malformed route faults are ignored defensively', () => {
  for (const route_index_fault of [[], ['  '], [{}], [undefined], { message: 'bad shape' }]) {
    const text = buildHistorianFaultBannerText({ route_index_fault }, formatters);
    assert.equal(text, null);
    assert.equal(readRouteIndexFault({ route_index_fault }), null);
  }
});

test('buildHistorianFaultBannerText does not append undefined text for malformed payloads', () => {
  const text = buildHistorianFaultBannerText(
    { commitment_index_fault: [{}] },
    formatters,
  );

  assert.equal(text, null);
});
