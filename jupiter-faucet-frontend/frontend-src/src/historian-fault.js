import { readOpt } from './candid-opt.js';

export function readContributionIndexFault(status) {
  const fault = readOpt(status?.contribution_index_fault);
  if (!fault || typeof fault !== 'object' || Array.isArray(fault)) {
    return null;
  }
  const observedAtTs = fault.observed_at_ts ?? null;
  const lastCursorTxId = readOpt(fault.last_cursor_tx_id);
  const offendingTxId = fault.offending_tx_id ?? null;
  const message = typeof fault.message === 'string' ? fault.message.trim() : '';
  if (observedAtTs === null && lastCursorTxId === null && offendingTxId === null && !message) {
    return null;
  }
  return {
    observedAtTs,
    lastCursorTxId,
    offendingTxId,
    message,
  };
}

export function buildContributionIndexFaultBannerText(status, {
  formatTimestampSeconds,
  formatInteger,
} = {}) {
  const fault = readContributionIndexFault(status);
  if (!fault) return null;
  const observedText = fault.observedAtTs === null ? '—' : formatTimestampSeconds(fault.observedAtTs);
  const cursorText = fault.lastCursorTxId === null ? 'none' : formatInteger(fault.lastCursorTxId);
  const offendingText = fault.offendingTxId === null ? '—' : formatInteger(fault.offendingTxId);
  const parts = [
    'Historian contribution indexing is degraded.',
    `First observed at ${observedText}.`,
    `Last cursor: ${cursorText}.`,
    `Offending tx: ${offendingText}.`,
  ];
  if (fault.message) {
    parts.push(fault.message);
  }
  return parts.join(' ');
}
