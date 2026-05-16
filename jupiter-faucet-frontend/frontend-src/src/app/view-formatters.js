import { escapeHtml } from '../followee-links.js';
import { calculateAgeBonusMaturityShareBasisPoints } from '../projection-simulator.js';
import { trackerHashForPrincipal } from './hash-routes.js';

export const DASH = '—';

const BLACKHOLE_CANISTER_ID = '77deu-baaaa-aaaar-qb6za-cai';

export function formatPrincipal(value) {
  return value?.toText ? value.toText() : String(value || '');
}

export function renderCanisterTrackerLink(value, { label = null, className = 'pane-canister-tracker-link pane-external-link mono' } = {}) {
  const principalText = formatPrincipal(value).trim();
  if (!principalText) return DASH;
  const display = label === null || label === undefined ? principalText : String(label);
  return `<a href="${escapeHtml(trackerHashForPrincipal(principalText))}" data-tracker-principal="${escapeHtml(principalText)}" class="${escapeHtml(className)}">${escapeHtml(display)}</a>`;
}

export function renderCanisterDashboardLink(value, label = 'Open dashboard') {
  const principalText = formatPrincipal(value).trim();
  if (!principalText) return DASH;
  return `<a href="https://dashboard.internetcomputer.org/canister/${escapeHtml(principalText)}" target="_blank" rel="noopener noreferrer" class="pane-external-link mono">${escapeHtml(label)}</a>`;
}

export function formatSourceController(value) {
  const principalText = formatPrincipal(value).trim();
  if (!principalText) return '';
  if (principalText === BLACKHOLE_CANISTER_ID) return renderCanisterTrackerLink(principalText, { label: 'blackhole' });
  return renderCanisterTrackerLink(principalText);
}

export function formatIcpE8s(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const sign = asBigInt < 0n ? '-' : '';
  const absolute = asBigInt < 0n ? -asBigInt : asBigInt;
  const whole = absolute / 100_000_000n;
  const fraction = (absolute % 100_000_000n).toString().padStart(8, '0').replace(/0+$/, '');
  return fraction ? `${sign}${whole.toString()}.${fraction} ICP` : `${sign}${whole.toString()} ICP`;
}

export function formatGroupedBigInt(value) {
  const text = value.toString();
  const sign = text.startsWith('-') ? '-' : '';
  const digits = sign ? text.slice(1) : text;
  return `${sign}${digits.replace(/\B(?=(\d{3})+(?!\d))/g, ',')}`;
}

export function formatCycles(value) {
  return formatTrillionCycles(value);
}

export function formatTrillionCycles(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const sign = asBigInt < 0n ? '-' : '';
  const absolute = asBigInt < 0n ? -asBigInt : asBigInt;
  const tenThousandths = (absolute * 10_000n) / 1_000_000_000_000n;
  const whole = tenThousandths / 10_000n;
  const fraction = tenThousandths % 10_000n;
  return `${sign}${whole}.${fraction.toString().padStart(4, '0')}T cycles`;
}

export function formatCompactTrillionCycles(value) {
  return formatTrillionCycles(value)
    .replace(/\.0000T cycles$/, 'T cycles')
    .replace(/(\.\d*?[1-9])0+T cycles$/, '$1T cycles');
}

function formatScaledRateForOneDecimal(rate, decimals) {
  if (rate === null || rate === undefined || decimals === null || decimals === undefined) return null;
  const numerator = typeof rate === 'bigint' ? rate : BigInt(rate);
  const scale = 10n ** BigInt(decimals);
  if (scale <= 0n) return null;
  const tenths = (numerator * 10n + scale / 2n) / scale;
  return `${tenths / 10n}.${(tenths % 10n).toString()}`;
}

export function formatIcpXdrRateInput(snapshot) {
  if (!snapshot) return null;
  return formatScaledRateForOneDecimal(snapshot.rate, snapshot.decimals);
}

export function formatIcpXdrRateDisplay(snapshot) {
  const input = formatIcpXdrRateInput(snapshot);
  return input ? `${input} XDR/ICP` : DASH;
}

export function formatIcpXdrRateSource(snapshot, manualOverride = false) {
  if (!snapshot) return 'Manual input';
  const fetchedAt = Number(snapshot.fetched_at_ts || 0);
  const ageSeconds = fetchedAt > 0 ? Math.max(0, Math.floor(Date.now() / 1000) - fetchedAt) : null;
  const ageText = ageSeconds === null ? 'freshness unknown' : `${formatDurationSeconds(ageSeconds)} old`;
  const cacheText = `historian XRC cache: ${formatIcpXdrRateDisplay(snapshot)} (${ageText})`;

  return manualOverride
    ? `Manual override; ${cacheText}`
    : `Historian XRC cache: ${formatIcpXdrRateDisplay(snapshot)} (${ageText})`;
}

export function formatBasisPointsAsPercent(value, decimals = 1) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const sign = asBigInt < 0n ? '-' : '';
  const absolute = asBigInt < 0n ? -asBigInt : asBigInt;
  const scale = 10n ** BigInt(Math.max(0, decimals));
  const scaled = (absolute * scale) / 100n;
  const whole = scaled / scale;
  const fraction = scaled % scale;
  if (decimals <= 0) return `${sign}${whole}%`;
  return `${sign}${whole}.${fraction.toString().padStart(decimals, '0')}%`;
}

export function formatAgeBonusDisplay(ageBonusBasisPoints) {
  const maturityShare = calculateAgeBonusMaturityShareBasisPoints(ageBonusBasisPoints);
  return `${formatBasisPointsAsPercent(maturityShare)} of maturity diverted (${formatBasisPointsAsPercent(ageBonusBasisPoints)} age bonus)`;
}

export function formatInteger(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  return formatGroupedBigInt(asBigInt);
}

export function formatBytes(value) {
  if (value === null || value === undefined) return DASH;
  const asBigInt = typeof value === 'bigint' ? value : BigInt(value);
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let scaled = Number(asBigInt);
  let unitIndex = 0;
  while (scaled >= 1024 && unitIndex < units.length - 1) {
    scaled /= 1024;
    unitIndex += 1;
  }
  const digits = scaled >= 100 || unitIndex === 0 ? 0 : scaled >= 10 ? 1 : 2;
  return `${scaled.toFixed(digits)} ${units[unitIndex]}`;
}

export function formatTimestampSeconds(value) {
  if (!value) return DASH;
  return new Date(Number(value) * 1000).toLocaleString('en-GB', {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC',
    timeZoneName: 'short',
  });
}

export function formatLocalTimestampSeconds(value) {
  if (!value) return DASH;
  return new Date(Number(value) * 1000).toLocaleString('en-GB', {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    timeZoneName: 'short',
  });
}

export function formatTimestampNanos(value) {
  if (!value) return DASH;
  const millis = (typeof value === 'bigint' ? value : BigInt(value)) / 1_000_000n;
  return new Date(Number(millis)).toLocaleString('en-GB', {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC',
    timeZoneName: 'short',
  });
}

export function formatAgeFromSeconds(value) {
  if (!value) return DASH;
  const nowSeconds = Math.floor(Date.now() / 1000);
  const diff = Math.max(0, nowSeconds - Number(value));
  const day = 24 * 60 * 60;
  const year = 365 * day;
  if (diff >= year) {
    const years = Math.floor(diff / year);
    const months = Math.floor((diff % year) / (30 * day));
    return months > 0 ? `${years}y ${months}mo` : `${years}y`;
  }
  if (diff >= 30 * day) return `${Math.floor(diff / (30 * day))}mo`;
  if (diff >= day) return `${Math.floor(diff / day)}d`;
  if (diff >= 60 * 60) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 60)}m`;
}

export function formatDurationSeconds(value) {
  if (value === null || value === undefined) return DASH;
  const seconds = Number(typeof value === 'bigint' ? value : BigInt(value));
  if (!Number.isFinite(seconds) || seconds <= 0) return DASH;
  const units = [
    ['week', 7 * 24 * 60 * 60],
    ['day', 24 * 60 * 60],
    ['hour', 60 * 60],
    ['minute', 60],
  ];
  for (const [label, size] of units) {
    if (seconds >= size && seconds % size === 0) {
      const count = seconds / size;
      return `${formatInteger(count)} ${count === 1 ? label : `${label}s`}`;
    }
  }
  return `${formatInteger(seconds)} seconds`;
}
