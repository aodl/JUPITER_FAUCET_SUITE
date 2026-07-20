import { formatInteger } from './view-formatters.js';

export function dashboardCountDisplays(counts) {
  const trackedCount = counts?.tracked_canister_count;
  const memoRegisteredCanisterCount = counts?.memo_registered_canister_count;
  return {
    trackedCanisterMetric: trackedCount === undefined || trackedCount === null
      ? ''
      : formatInteger(trackedCount),
    declaredCanisterBadge: memoRegisteredCanisterCount === undefined || memoRegisteredCanisterCount === null
      ? ''
      : `(${formatInteger(memoRegisteredCanisterCount)})`,
  };
}
