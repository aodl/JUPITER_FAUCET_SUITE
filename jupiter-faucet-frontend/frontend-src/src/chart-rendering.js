const DEFAULT_WIDTH = 640;
const DEFAULT_HEIGHT = 180;
const DEFAULT_PAD_LEFT = 44;
const DEFAULT_PAD_RIGHT = 18;
const DEFAULT_PAD_TOP = 18;
const DEFAULT_PAD_BOTTOM = 42;
const BAR_TOP_LABEL_HEADROOM = 18;

function escapeChartHtml(value) {
  return String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

export function toBigIntValue(value, fallback = 0n) {
  if (value === null || value === undefined) return fallback;
  return typeof value === 'bigint' ? value : BigInt(value);
}

export function ratioBigInt(value, max) {
  const denominator = toBigIntValue(max, 0n);
  if (denominator <= 0n || value === null || value === undefined) return 0;
  const numerator = toBigIntValue(value) * 1_000_000n;
  return Math.max(0, Math.min(1, Number(numerator / denominator) / 1_000_000));
}

export function scaleBigInt(value, min, max) {
  const lower = toBigIntValue(min, 0n);
  const upper = toBigIntValue(max, 0n);
  if (upper <= lower || value === null || value === undefined) return 0;
  const numerator = (toBigIntValue(value) - lower) * 1_000_000n;
  const denominator = upper - lower;
  return Math.max(0, Math.min(1, Number(numerator / denominator) / 1_000_000));
}

export function renderEmptyChart(message, { className = 'tracker-chart-empty' } = {}) {
  return `<div class="${escapeChartHtml(className)}">${escapeChartHtml(message)}</div>`;
}

function chartGeometry() {
  const chartWidth = DEFAULT_WIDTH - DEFAULT_PAD_LEFT - DEFAULT_PAD_RIGHT;
  const chartHeight = DEFAULT_HEIGHT - DEFAULT_PAD_TOP - DEFAULT_PAD_BOTTOM;
  return {
    width: DEFAULT_WIDTH,
    height: DEFAULT_HEIGHT,
    padLeft: DEFAULT_PAD_LEFT,
    padRight: DEFAULT_PAD_RIGHT,
    padTop: DEFAULT_PAD_TOP,
    padBottom: DEFAULT_PAD_BOTTOM,
    chartWidth,
    chartHeight,
  };
}

function visibleTickIndexes(length, showAllTicks = false) {
  if (length <= 0) return new Set();
  if (showAllTicks || length <= 8) {
    return new Set(Array.from({ length }, (_, index) => index));
  }

  const targetTicks = 8;
  const step = Math.max(2, Math.ceil(length / targetTicks));
  const indexes = new Set();
  for (let index = 0; index < length; index += step) {
    indexes.add(index);
  }
  return indexes;
}

function clampNumber(value, min, max) {
  if (max < min) return max;
  return Math.max(min, Math.min(max, value));
}

function bucketTimeRange(bucket) {
  const startMs = Number(bucket?.startMs);
  const endMs = Number(bucket?.endMs);
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs <= startMs) return null;
  return { startMs, endMs };
}

function timeDomain(rows, explicitRows = null) {
  const ranges = (Array.isArray(explicitRows) ? explicitRows : rows).map(bucketTimeRange).filter(Boolean);
  if (ranges.length === 0) return null;
  const startMs = Math.min(...ranges.map((range) => range.startMs));
  const endMs = Math.max(...ranges.map((range) => range.endMs));
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs <= startMs) return null;
  return { startMs, endMs, durationMs: endMs - startMs };
}

function bucketTimeGeometry(bucket, { domain, padLeft, chartWidth }) {
  const range = bucketTimeRange(bucket);
  if (!range || !domain) return null;
  const x = padLeft + ((range.startMs - domain.startMs) / domain.durationMs) * chartWidth;
  const width = ((range.endMs - range.startMs) / domain.durationMs) * chartWidth;
  const center = x + width / 2;
  return { x, width, center };
}

function renderBucketTicks({ buckets, slot, padLeft, height, showAllTicks = false, xForBucket = null }) {
  const visible = visibleTickIndexes(buckets.length, showAllTicks);
  return buckets.map((bucket, index) => {
    if (!visible.has(index)) return '';
    const x = xForBucket ? xForBucket(bucket, index) : padLeft + (index * slot) + slot / 2;
    return `<text class="tracker-chart-axis-label" x="${x.toFixed(2)}" y="${height - 14}" text-anchor="middle">${escapeChartHtml(bucket.label)}</text>`;
  }).join('');
}

export function renderAmountBarChart({
  buckets,
  amountKey,
  countKey,
  emptyMessage,
  ariaLabel,
  barClass = '',
  labelBuilder,
  valueFormatter,
  showAllTicks = false,
  xAxis = 'category',
  minBarWidth = 8,
  maxBarWidth = 44,
  barWidthRatio = 0.58,
}) {
  const rows = Array.isArray(buckets) ? buckets : [];
  const maxAmount = rows.reduce((max, bucket) => {
    const amount = toBigIntValue(bucket?.[amountKey], 0n);
    return amount > max ? amount : max;
  }, 0n);
  if (maxAmount <= 0n) {
    return renderEmptyChart(emptyMessage);
  }

  const { width, height, padLeft, padRight, padTop, chartWidth, chartHeight } = chartGeometry();
  const slot = chartWidth / Math.max(1, rows.length);
  const domain = xAxis === 'time' ? timeDomain(rows) : null;
  const className = `tracker-chart-bar${barClass ? ` ${barClass}` : ''}`;
  const bars = rows.map((bucket, index) => {
    const amount = toBigIntValue(bucket?.[amountKey], 0n);
    const ratio = ratioBigInt(amount, maxAmount);
    const usableBarHeight = Math.max(1, chartHeight - BAR_TOP_LABEL_HEADROOM);
    const barHeight = Math.max(amount > 0n ? 2 : 0, ratio * usableBarHeight);
    const timeGeometry = domain ? bucketTimeGeometry(bucket, { domain, padLeft, chartWidth }) : null;
    const bucketWidth = timeGeometry?.width || slot;
    const barWidth = clampNumber(bucketWidth * barWidthRatio, minBarWidth, Math.min(maxBarWidth, bucketWidth));
    const x = timeGeometry
      ? timeGeometry.x + (bucketWidth - barWidth) / 2
      : padLeft + (index * slot) + (slot - barWidth) / 2;
    const y = padTop + BAR_TOP_LABEL_HEADROOM + usableBarHeight - barHeight;
    const label = labelBuilder
      ? labelBuilder(bucket)
      : `${bucket.label}: ${valueFormatter ? valueFormatter(amount) : amount.toString()} across ${bucket?.[countKey] || 0} items`;
    return `<rect class="${escapeChartHtml(className)}" x="${x.toFixed(2)}" y="${y.toFixed(2)}" width="${barWidth.toFixed(2)}" height="${barHeight.toFixed(2)}" rx="4"><title>${escapeChartHtml(label)}</title></rect>`;
  }).join('');
  const ticks = renderBucketTicks({
    buckets: rows,
    slot,
    padLeft,
    height,
    showAllTicks,
    xForBucket: domain ? (bucket, index) => bucketTimeGeometry(bucket, { domain, padLeft, chartWidth })?.center ?? (padLeft + (index * slot) + slot / 2) : null,
  });
  const axisLabel = valueFormatter ? valueFormatter(maxAmount) : maxAmount.toString();
  return `
    <svg class="tracker-chart-svg" viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeChartHtml(ariaLabel)}">
      <line class="tracker-chart-axis" x1="${padLeft}" y1="${padTop + chartHeight}" x2="${width - padRight}" y2="${padTop + chartHeight}"></line>
      <text class="tracker-chart-y-label" x="8" y="20">${escapeChartHtml(axisLabel)}</text>
      ${bars}
      ${ticks}
    </svg>`;
}

export function renderLineChart({
  buckets,
  valueKey,
  emptyMessage,
  ariaLabel,
  valueFormatter,
  pointLabelBuilder,
  showAllTicks = false,
  xAxis = 'category',
  xDomainBuckets = null,
  xTickBuckets = null,
}) {
  const rows = Array.isArray(buckets) ? buckets : [];
  const valueBuckets = rows.filter((bucket) => bucket?.[valueKey] !== null && bucket?.[valueKey] !== undefined);
  if (valueBuckets.length === 0) {
    return renderEmptyChart(emptyMessage);
  }

  let minValue = valueBuckets.reduce((min, bucket) => {
    const value = toBigIntValue(bucket[valueKey]);
    return value < min ? value : min;
  }, toBigIntValue(valueBuckets[0][valueKey]));
  let maxValue = valueBuckets.reduce((max, bucket) => {
    const value = toBigIntValue(bucket[valueKey]);
    return value > max ? value : max;
  }, toBigIntValue(valueBuckets[0][valueKey]));

  if (minValue > 0n) minValue = 0n;
  if (maxValue < 0n) maxValue = 0n;
  if (minValue === maxValue) {
    if (maxValue === 0n) {
      maxValue = 1n;
    } else if (maxValue > 0n) {
      minValue = 0n;
    } else {
      maxValue = 0n;
    }
  }

  const { width, height, padLeft, padRight, padTop, chartWidth, chartHeight } = chartGeometry();
  const slot = chartWidth / Math.max(1, rows.length);
  const domain = xAxis === 'time' ? timeDomain(rows, xDomainBuckets) : null;
  const pointFor = (bucket, index) => {
    const x = domain
      ? bucketTimeGeometry(bucket, { domain, padLeft, chartWidth })?.center ?? (padLeft + (index * slot) + slot / 2)
      : padLeft + (index * slot) + slot / 2;
    const ratio = scaleBigInt(bucket[valueKey], minValue, maxValue);
    const y = padTop + chartHeight - ratio * chartHeight;
    return { x, y };
  };
  const points = rows
    .map((bucket, index) => bucket?.[valueKey] === null || bucket?.[valueKey] === undefined ? null : pointFor(bucket, index))
    .filter(Boolean);
  const polyline = points.map((point) => `${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(' ');
  const circles = rows.map((bucket, index) => {
    if (bucket?.[valueKey] === null || bucket?.[valueKey] === undefined) return '';
    const point = pointFor(bucket, index);
    const label = pointLabelBuilder
      ? pointLabelBuilder(bucket)
      : `${bucket.label}: ${valueFormatter ? valueFormatter(bucket[valueKey]) : String(bucket[valueKey])}`;
    return `<circle class="tracker-chart-point" cx="${point.x.toFixed(2)}" cy="${point.y.toFixed(2)}" r="4"><title>${escapeChartHtml(label)}</title></circle>`;
  }).join('');
  const tickRows = Array.isArray(xTickBuckets) ? xTickBuckets : rows;
  const tickSlot = chartWidth / Math.max(1, tickRows.length);
  const ticks = renderBucketTicks({
    buckets: tickRows,
    slot: tickSlot,
    padLeft,
    height,
    showAllTicks,
    xForBucket: domain ? (bucket, index) => bucketTimeGeometry(bucket, { domain, padLeft, chartWidth })?.center ?? (padLeft + (index * tickSlot) + tickSlot / 2) : null,
  });
  const baselineY = padTop + chartHeight;
  const zeroAxis = minValue < 0n && maxValue > 0n
    ? (() => {
        const ratio = scaleBigInt(0n, minValue, maxValue);
        const y = padTop + chartHeight - ratio * chartHeight;
        return `<line class="tracker-chart-axis tracker-chart-axis--zero" x1="${padLeft}" y1="${y.toFixed(2)}" x2="${width - padRight}" y2="${y.toFixed(2)}"></line>`;
      })()
    : '';
  const maxLabel = valueFormatter ? valueFormatter(maxValue) : maxValue.toString();
  const minLabel = minValue < 0n ? `<text class="tracker-chart-y-label" x="8" y="${Math.max(34, baselineY - 2).toFixed(2)}">${escapeChartHtml(valueFormatter ? valueFormatter(minValue) : minValue.toString())}</text>` : '';
  return `
    <svg class="tracker-chart-svg" viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeChartHtml(ariaLabel)}">
      <line class="tracker-chart-axis" x1="${padLeft}" y1="${baselineY}" x2="${width - padRight}" y2="${baselineY}"></line>
      ${zeroAxis}
      <text class="tracker-chart-y-label" x="8" y="20">${escapeChartHtml(maxLabel)}</text>
      ${minLabel}
      <polyline class="tracker-chart-line" points="${polyline}"></polyline>
      ${circles}
      ${ticks}
    </svg>`;
}
