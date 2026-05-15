import { readOpt } from './candid-opt.js';

function asBigIntOrNull(value) {
  if (value === null || value === undefined) return null;
  try {
    return typeof value === 'bigint' ? value : BigInt(value);
  } catch {
    return null;
  }
}

function disbursementsInProgress(neuron) {
  const value = readOpt(neuron?.maturity_disbursements_in_progress);
  return Array.isArray(value) ? value.filter((item) => item && typeof item === 'object') : [];
}

function earliestFinalizationTimestamp(disbursements) {
  return disbursements
    .map((item) => asBigIntOrNull(readOpt(item.finalize_disbursement_timestamp_seconds)))
    .filter((value) => value !== null && value > 0n)
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))[0] ?? null;
}

function totalAmountE8s(disbursements) {
  return disbursements.reduce((total, item) => {
    const amount = asBigIntOrNull(readOpt(item.amount_e8s));
    return amount === null ? total : total + amount;
  }, 0n);
}

function relativeLandingText(finalizeTs, nowSeconds) {
  if (finalizeTs === null) return null;
  const diff = Number(finalizeTs - BigInt(nowSeconds));
  if (!Number.isFinite(diff)) return null;
  if (diff <= 0) return 'due now';
  const day = 24 * 60 * 60;
  const hour = 60 * 60;
  if (diff >= day) {
    const days = Math.ceil(diff / day);
    return `in ${days} ${days === 1 ? 'day' : 'days'}`;
  }
  if (diff >= hour) {
    const hours = Math.ceil(diff / hour);
    return `in ${hours} ${hours === 1 ? 'hour' : 'hours'}`;
  }
  const minutes = Math.max(1, Math.ceil(diff / 60));
  return `in ${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`;
}

export function formatMaturityDisbursementLandingText(
  neuron,
  {
    formatTimestampSeconds,
    nowSeconds = Math.floor(Date.now() / 1000),
  } = {},
) {
  const disbursements = disbursementsInProgress(neuron);
  if (disbursements.length === 0 || typeof formatTimestampSeconds !== 'function') return null;

  const finalizeTs = earliestFinalizationTimestamp(disbursements);
  if (finalizeTs === null) return null;

  const dueText = formatTimestampSeconds(finalizeTs);
  const relativeText = relativeLandingText(finalizeTs, nowSeconds);
  return `Disbursal currently in flight, due ${dueText}${relativeText ? ` (${relativeText})` : ''}.`;
}

export function formatMaturityDisbursementStatus(
  neuron,
  {
    formatIcpE8s,
    formatTimestampSeconds,
    nowSeconds = Math.floor(Date.now() / 1000),
  } = {},
) {
  const disbursements = disbursementsInProgress(neuron);
  if (disbursements.length === 0) {
    return 'A new disbursal starts after at least 1 ICP of maturity is available; depending on the amount staked, this can take several weeks.';
  }

  const finalizeTs = earliestFinalizationTimestamp(disbursements);
  const total = totalAmountE8s(disbursements);
  const countText = disbursements.length === 1 ? '1 disbursal in flight' : `${disbursements.length} disbursals in flight`;
  const amountText = total > 0n && typeof formatIcpE8s === 'function'
    ? ` (${formatIcpE8s(total)})`
    : '';
  if (finalizeTs === null || typeof formatTimestampSeconds !== 'function') {
    return `${countText}${amountText}; landing time unavailable`;
  }
  const dueText = formatTimestampSeconds(finalizeTs);
  const relativeText = relativeLandingText(finalizeTs, nowSeconds);
  return `${countText}${amountText}; due ${dueText}${relativeText ? ` (${relativeText})` : ''}`;
}
