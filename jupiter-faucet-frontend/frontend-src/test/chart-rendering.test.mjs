import test from 'node:test';
import assert from 'node:assert/strict';

import { renderAmountBarChart, renderEmptyChart, renderLineChart, scaleBigInt } from '../src/chart-rendering.js';

function countLabels(html, prefix) {
  const matches = html.match(new RegExp(`>${prefix}\\d+<\\/text>`, 'g')) || [];
  return matches.length;
}

test('renderAmountBarChart uses caller-provided formatting and escapes labels', () => {
  const html = renderAmountBarChart({
    buckets: [{ label: 'M<1>', projectedTopupCycles: 10n, topupDays: 1 }],
    amountKey: 'projectedTopupCycles',
    countKey: 'topupDays',
    emptyMessage: 'empty',
    ariaLabel: 'topups <chart>',
    valueFormatter: (value) => `${value} cycles`,
    labelBuilder: () => 'unsafe <label>',
  });

  assert.match(html, /10 cycles/);
  assert.match(html, /M&lt;1&gt;/);
  assert.match(html, /unsafe &lt;label&gt;/);
  assert.match(html, /aria-label="topups &lt;chart&gt;"/);
});

test('renderAmountBarChart returns an escaped empty chart when all amounts are zero', () => {
  const html = renderAmountBarChart({
    buckets: [{ label: 'M1', amount: 0n }],
    amountKey: 'amount',
    countKey: 'count',
    emptyMessage: 'No <data>',
    ariaLabel: 'empty',
  });

  assert.equal(html, '<div class="tracker-chart-empty">No &lt;data&gt;</div>');
});

test('renderLineChart supports negative projection balances with a zero axis', () => {
  const html = renderLineChart({
    buckets: [
      { label: 'M1', projectedBalanceCycles: 5n },
      { label: 'M2', projectedBalanceCycles: -5n },
    ],
    valueKey: 'projectedBalanceCycles',
    emptyMessage: 'empty',
    ariaLabel: 'balance',
    valueFormatter: (value) => `${value}`,
  });

  assert.match(html, /tracker-chart-axis--zero/);
  assert.match(html, />-5</);
});

test('renderLineChart can show all x-axis labels when explicitly requested', () => {
  const buckets = Array.from({ length: 12 }, (_, index) => ({
    label: `W${index + 1}`,
    projectedBalanceCycles: BigInt(index + 1),
  }));

  const defaultHtml = renderLineChart({
    buckets,
    valueKey: 'projectedBalanceCycles',
    emptyMessage: 'empty',
    ariaLabel: 'default ticks',
  });
  const allTicksHtml = renderLineChart({
    buckets,
    valueKey: 'projectedBalanceCycles',
    emptyMessage: 'empty',
    ariaLabel: 'all ticks',
    showAllTicks: true,
  });

  assert.ok(countLabels(defaultHtml, 'W') < 12);
  assert.equal(countLabels(allTicksHtml, 'W'), 12);
});

test('renderAmountBarChart can show all x-axis labels when explicitly requested', () => {
  const buckets = Array.from({ length: 12 }, (_, index) => ({
    label: `W${index + 1}`,
    projectedTopupCycles: 10n,
  }));

  const html = renderAmountBarChart({
    buckets,
    amountKey: 'projectedTopupCycles',
    countKey: 'count',
    emptyMessage: 'empty',
    ariaLabel: 'all ticks',
    showAllTicks: true,
  });

  assert.equal(countLabels(html, 'W'), 12);
});

test('scaleBigInt maps signed ranges into chart ratios and clamps out-of-range values', () => {
  assert.equal(scaleBigInt(-10n, -10n, 10n), 0);
  assert.equal(scaleBigInt(0n, -10n, 10n), 0.5);
  assert.equal(scaleBigInt(10n, -10n, 10n), 1);
  assert.equal(scaleBigInt(-20n, -10n, 10n), 0);
  assert.equal(scaleBigInt(20n, -10n, 10n), 1);
});

test('renderEmptyChart escapes custom class names and message content', () => {
  const html = renderEmptyChart('<empty>', { className: 'custom <class>' });
  assert.equal(html, '<div class="custom &lt;class&gt;">&lt;empty&gt;</div>');
});
