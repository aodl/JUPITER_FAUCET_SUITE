import test from 'node:test';
import assert from 'node:assert/strict';

import { dashboardCountDisplays } from '../src/app/count-displays.js';

test('tracked metric and declared canister count use distinct historian counts', () => {
  const displays = dashboardCountDisplays({
    tracked_canister_count: 10n,
    memo_registered_canister_count: 3n,
  });

  assert.equal(displays.trackedCanisterMetric, '10');
  assert.equal(displays.declaredCanisterBadge, '(3)');
});
