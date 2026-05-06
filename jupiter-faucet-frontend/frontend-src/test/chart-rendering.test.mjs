import test from 'node:test';
import assert from 'node:assert/strict';

import { renderAmountBarChart, renderEmptyChart, renderLineChart, scaleBigInt } from '../src/chart-rendering.js';

function countLabels(html, prefix) {
  const matches = html.match(new RegExp(`>${prefix}\\d+<\\/text>`, 'g')) || [];
  return matches.length;
}

function labels(html, prefix) {
  return Array.from(html.matchAll(new RegExp(`>${prefix}(\\d+)<\\/text>`, 'g'))).map((match) => `${prefix}${match[1]}`);
}

function firstRectY(html) {
  const match = html.match(/<rect[^>]+ y="([0-9.]+)"/);
  assert.ok(match, 'missing rect y coordinate');
  return Number(match[1]);
}

function rectWidths(html) {
  return Array.from(html.matchAll(/<rect[^>]+ width="([0-9.]+)"/g)).map((match) => Number(match[1]));
}

function polylineXs(html) {
  const match = html.match(/<polyline[^>]+ points="([^"]+)"/);
  assert.ok(match, 'missing polyline points');
  return match[1].split(' ').map((point) => Number(point.split(',')[0]));
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

test('default ticks use a regular nth-label cadence', () => {
  const buckets = Array.from({ length: 52 }, (_, index) => ({
    label: `W${index + 1}`,
    projectedBalanceCycles: BigInt(index + 1),
  }));

  const html = renderLineChart({
    buckets,
    valueKey: 'projectedBalanceCycles',
    emptyMessage: 'empty',
    ariaLabel: 'weekly ticks',
  });

  const rendered = labels(html, 'W');
  assert.equal(rendered.length, 8);
  assert.deepEqual(rendered, ['W1', 'W8', 'W15', 'W22', 'W29', 'W36', 'W43', 'W50']);
});

test('monthly default ticks prefer every other label when all labels would crowd', () => {
  const buckets = Array.from({ length: 12 }, (_, index) => ({
    label: `M${index + 1}`,
    projectedBalanceCycles: BigInt(index + 1),
  }));

  const html = renderLineChart({
    buckets,
    valueKey: 'projectedBalanceCycles',
    emptyMessage: 'empty',
    ariaLabel: 'monthly ticks',
  });

  assert.deepEqual(labels(html, 'M'), ['M1', 'M3', 'M5', 'M7', 'M9', 'M11']);
});

test('amount bar chart reserves headroom so max bars do not overlap the y-axis label', () => {
  const html = renderAmountBarChart({
    buckets: [{ label: 'W1', projectedTopupCycles: 100n }],
    amountKey: 'projectedTopupCycles',
    countKey: 'count',
    emptyMessage: 'empty',
    ariaLabel: 'headroom',
  });

  assert.ok(firstRectY(html) >= 36, `expected y-axis label headroom, got y=${firstRectY(html)}`);
});

test('time-scaled line chart spaces points by elapsed bucket time', () => {
  const html = renderLineChart({
    buckets: [
      { label: 'D1', startMs: Date.UTC(2026, 0, 1), endMs: Date.UTC(2026, 0, 2), cycles: 10n },
      { label: 'D2', startMs: Date.UTC(2026, 0, 2), endMs: Date.UTC(2026, 0, 3), cycles: 20n },
      { label: 'D11', startMs: Date.UTC(2026, 0, 11), endMs: Date.UTC(2026, 0, 12), cycles: 30n },
    ],
    valueKey: 'cycles',
    emptyMessage: 'empty',
    ariaLabel: 'time line',
    xAxis: 'time',
  });

  const [firstX, secondX, thirdX] = polylineXs(html);
  assert.ok(secondX - firstX < thirdX - secondX, 'later gap should occupy more x-axis space');
  assert.ok(thirdX - secondX > (secondX - firstX) * 5, 'nine empty days should be visible on the x-axis');
});

test('time-scaled line chart can use an explicit x-domain and tick buckets', () => {
  const html = renderLineChart({
    buckets: [
      { label: 'Probe', startMs: Date.UTC(2026, 0, 10), endMs: Date.UTC(2026, 0, 10) + 1, cycles: 10n },
    ],
    valueKey: 'cycles',
    emptyMessage: 'empty',
    ariaLabel: 'aligned time line',
    xAxis: 'time',
    xDomainBuckets: [
      { label: 'D1', startMs: Date.UTC(2026, 0, 1), endMs: Date.UTC(2026, 0, 2) },
      { label: 'D31', startMs: Date.UTC(2026, 0, 31), endMs: Date.UTC(2026, 1, 1) },
    ],
    xTickBuckets: [
      { label: 'D1', startMs: Date.UTC(2026, 0, 1), endMs: Date.UTC(2026, 0, 2) },
      { label: 'D31', startMs: Date.UTC(2026, 0, 31), endMs: Date.UTC(2026, 1, 1) },
    ],
  });

  const [pointX] = polylineXs(html);
  assert.ok(pointX > 44, 'point should be positioned inside the explicit domain');
  assert.ok(pointX < 640 - 18, 'point should not stretch to the chart edge as a one-point domain');
  assert.match(html, />D1<\/text>/);
  assert.match(html, />D31<\/text>/);
});

test('time-scaled bar chart can use sub-slot bar widths for dense timelines', () => {
  const html = renderAmountBarChart({
    buckets: [
      { label: 'Jan', startMs: Date.UTC(2026, 0, 1), endMs: Date.UTC(2026, 1, 1), amount: 10n },
      { label: 'Dec', startMs: Date.UTC(2026, 11, 1), endMs: Date.UTC(2027, 0, 1), amount: 20n },
    ],
    amountKey: 'amount',
    countKey: 'count',
    emptyMessage: 'empty',
    ariaLabel: 'time bars',
    xAxis: 'time',
    minBarWidth: 2,
    maxBarWidth: 28,
  });

  assert.ok(rectWidths(html).every((width) => width <= 28), 'tracker bars should honor the narrower cap');
});

test('time-scaled bar chart can preserve minimum bar width on dense timelines', () => {
  const html = renderAmountBarChart({
    buckets: [
      { label: 'D1', startMs: Date.UTC(2026, 0, 1), endMs: Date.UTC(2026, 0, 2), amount: 10n },
      { label: 'D365', startMs: Date.UTC(2026, 11, 31), endMs: Date.UTC(2027, 0, 1), amount: 20n },
    ],
    amountKey: 'amount',
    countKey: 'count',
    emptyMessage: 'empty',
    ariaLabel: 'dense time bars',
    xAxis: 'time',
    minBarWidth: 2,
    maxBarWidth: 28,
    allowMinBarOverflow: true,
  });

  assert.ok(rectWidths(html).every((width) => width >= 2), 'dense tracker bars should remain visible');
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
