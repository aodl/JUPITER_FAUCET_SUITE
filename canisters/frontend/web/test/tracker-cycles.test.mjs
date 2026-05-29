import test from 'node:test';
import assert from 'node:assert/strict';

import {
  cycleSamplesForBurnEstimate,
  cyclesFromLogText,
  estimateCyclesBurnedPerDay,
  sortedCycleSamples,
} from '../src/tracker-cycles.js';

const NANOS_PER_DAY = 86_400_000_000_000n;

test('cyclesFromLogText reads cycle balances from canister log messages', () => {
  assert.equal(
    cyclesFromLogText('Cycles: 12_717_638_405_339, Proposals: 31 live'),
    12_717_638_405_339n,
  );
  assert.equal(cyclesFromLogText('ERR:1003'), null);
});

test('burn estimate falls back to log samples when probe history has fewer than two points', () => {
  const data = {
    cycles: {
      items: [
        {
          timestamp_nanos: 10n * NANOS_PER_DAY,
          cycles: 42_000n,
        },
      ],
    },
    logs: {
      items: [
        {
          timestamp_nanos: 1n * NANOS_PER_DAY,
          text: 'Cycles: 1_000',
        },
        {
          timestamp_nanos: 2n * NANOS_PER_DAY,
          text: 'ERR:1003',
        },
        {
          timestamp_nanos: 2n * NANOS_PER_DAY,
          text: 'Cycles: 900',
        },
      ],
    },
  };

  assert.equal(sortedCycleSamples(data)[0].source, 'probe');
  assert.equal(cycleSamplesForBurnEstimate(data)[0].source, 'log');
  assert.equal(estimateCyclesBurnedPerDay(data), 100n);
});

test('burn estimate accounts for observed top-ups when probe balances grow', () => {
  const data = {
    status: {
      icp_xdr_rate: [{ rate: 5n, decimals: 0n }],
    },
    cycles: {
      items: [
        {
          timestamp_nanos: 1n * NANOS_PER_DAY,
          cycles: 10_000_000_000_000n,
        },
        {
          timestamp_nanos: 3n * NANOS_PER_DAY,
          cycles: 14_000_000_000_000n,
        },
      ],
    },
    cmcTransfers: {
      items: [{
        timestamp_nanos: [2n * NANOS_PER_DAY],
        amount_e8s: 100_000_000n,
      }],
    },
    logs: { items: [] },
  };

  assert.equal(estimateCyclesBurnedPerDay(data), 500_000_000_000n);
});

test('burn estimate falls back to observed balance drops when top-up conversion is unavailable', () => {
  const data = {
    cycles: {
      items: [
        {
          timestamp_nanos: 1n * NANOS_PER_DAY,
          cycles: 10_000n,
        },
        {
          timestamp_nanos: 2n * NANOS_PER_DAY,
          cycles: 9_000n,
        },
        {
          timestamp_nanos: 3n * NANOS_PER_DAY,
          cycles: 15_000n,
        },
        {
          timestamp_nanos: 4n * NANOS_PER_DAY,
          cycles: 14_000n,
        },
      ],
    },
    logs: { items: [] },
  };

  assert.equal(estimateCyclesBurnedPerDay(data), 666n);
});
