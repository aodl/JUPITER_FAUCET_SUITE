import { readOpt } from './candid-opt.js';

export function readCommitmentIndexFault(status) {
  const fault = readOpt(status?.commitment_index_fault);
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

export function readRouteIndexFault(status) {
  const fault = readOpt(status?.route_index_fault);
  if (typeof fault !== 'string') return null;
  const message = fault.trim();
  return message || null;
}

export function buildHistorianFaultBannerText(status, {
  formatTimestampSeconds,
  formatInteger,
} = {}) {
  const parts = [];
  const commitmentFault = readCommitmentIndexFault(status);
  if (commitmentFault) {
    const observedText = commitmentFault.observedAtTs === null ? '—' : formatTimestampSeconds(commitmentFault.observedAtTs);
    const cursorText = commitmentFault.lastCursorTxId === null ? 'none' : formatInteger(commitmentFault.lastCursorTxId);
    const offendingText = commitmentFault.offendingTxId === null ? '—' : formatInteger(commitmentFault.offendingTxId);
    parts.push(
      'Historian endowment indexing is degraded.',
      `First observed at ${observedText}.`,
      `Last cursor: ${cursorText}.`,
      `Offending tx: ${offendingText}.`,
    );
    if (commitmentFault.message) {
      parts.push(commitmentFault.message);
    }
  }
  const routeFault = readRouteIndexFault(status);
  if (routeFault) {
    parts.push('Historian output/rewards indexing is degraded.', routeFault);
  }
  return parts.length > 0 ? parts.join(' ') : null;
}
