import test from 'node:test';
import assert from 'node:assert/strict';

import {
  formatMaturityDisbursementLandingText,
  formatMaturityDisbursementStatus,
} from '../src/maturity-disbursement.js';

const formatters = {
  formatIcpE8s: (value) => `${value.toString()} e8s`,
  formatTimestampSeconds: (value) => `ts:${value.toString()}`,
};

test('maturity disbursement status reports no in-flight work', () => {
  assert.equal(
    formatMaturityDisbursementStatus(
      { maturity_disbursements_in_progress: [] },
      formatters,
    ),
    'A new disbursal starts after at least 1 ICP of maturity is available; depending on the amount staked, this can take several weeks.',
  );
});

test('maturity disbursement status highlights amount and landing time', () => {
  const text = formatMaturityDisbursementStatus(
    {
      maturity_disbursements_in_progress: [[
        {
          amount_e8s: [25_000_000n],
          timestamp_of_disbursement_seconds: [1_000n],
          finalize_disbursement_timestamp_seconds: [87_400n],
        },
      ]],
    },
    {
      ...formatters,
      nowSeconds: 1_000,
    },
  );

  assert.equal(text, '1 disbursal in flight (25000000 e8s); due ts:87400 (in 1 day)');
});

test('maturity disbursement landing text is concise for the persistent landing status', () => {
  const text = formatMaturityDisbursementLandingText(
    {
      maturity_disbursements_in_progress: [[
        {
          finalize_disbursement_timestamp_seconds: [87_400n],
        },
      ]],
    },
    {
      formatTimestampSeconds: (value) => `ts:${value.toString()}`,
      nowSeconds: 1_000,
    },
  );

  assert.equal(text, 'Disbursal currently in flight, due ts:87400 (in 1 day).');
});

test('maturity disbursement landing text stays hidden without an in-flight disbursal', () => {
  assert.equal(
    formatMaturityDisbursementLandingText(
      { maturity_disbursements_in_progress: [] },
      { formatTimestampSeconds: (value) => `ts:${value.toString()}` },
    ),
    null,
  );
});

test('maturity disbursement status handles multiple disbursals and missing amounts', () => {
  const text = formatMaturityDisbursementStatus(
    {
      maturity_disbursements_in_progress: [[
        { finalize_disbursement_timestamp_seconds: [20_000n] },
        { amount_e8s: [2n], finalize_disbursement_timestamp_seconds: [10_000n] },
      ]],
    },
    {
      ...formatters,
      nowSeconds: 9_000,
    },
  );

  assert.equal(text, '2 disbursals in flight (2 e8s); due ts:10000 (in 17 minutes)');
});

test('maturity disbursement status does not invent a landing time', () => {
  const text = formatMaturityDisbursementStatus(
    {
      maturity_disbursements_in_progress: [[
        { amount_e8s: [5n] },
      ]],
    },
    formatters,
  );

  assert.equal(text, '1 disbursal in flight (5 e8s); landing time unavailable');
});
