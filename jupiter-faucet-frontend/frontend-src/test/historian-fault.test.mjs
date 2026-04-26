import test from 'node:test';
import assert from 'node:assert/strict';

import { readOpt } from '../src/candid-opt.js';
import {
  buildCommitmentIndexFaultBannerText,
  readCommitmentIndexFault,
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

test('buildCommitmentIndexFaultBannerText formats a valid historian fault', () => {
  const text = buildCommitmentIndexFaultBannerText(
    {
      commitment_index_fault: [{
        observed_at_ts: 123n,
        last_cursor_tx_id: [51n],
        offending_tx_id: 49n,
        message: 'Non-monotonic transaction ids observed from the index.',
      }],
    },
    {
      formatTimestampSeconds: (value) => `ts:${value.toString()}`,
      formatInteger: (value) => `n:${value.toString()}`,
    },
  );

  assert.equal(
    text,
    'Historian commitment indexing is degraded. First observed at ts:123. Last cursor: n:51. Offending tx: n:49. Non-monotonic transaction ids observed from the index.',
  );
});

test('buildCommitmentIndexFaultBannerText does not append undefined text for malformed payloads', () => {
  const text = buildCommitmentIndexFaultBannerText(
    { commitment_index_fault: [{}] },
    {
      formatTimestampSeconds: () => 'unused',
      formatInteger: () => 'unused',
    },
  );

  assert.equal(text, null);
});
