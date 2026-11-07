import test from 'node:test';
import assert from 'node:assert/strict';
import { formatIcpE8s, formatIcpE8sSummary } from '../src/app/view-formatters.js';

test('navbar stake rounds to two decimals while detailed stake retains precision', () => {
  assert.equal(formatIcpE8sSummary(123456789n), '1.23 ICP');
  assert.equal(formatIcpE8sSummary(199999999n), '2.00 ICP');
  assert.equal(formatIcpE8sSummary(0n), '0.00 ICP');
  assert.equal(formatIcpE8sSummary(18446744073709551615n), '184467440737.10 ICP');
  assert.equal(formatIcpE8sSummary(null), '-');
  assert.equal(formatIcpE8s(123456789n), '1.23456789 ICP');
});
