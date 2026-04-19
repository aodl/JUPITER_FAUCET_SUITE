import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeRegisteredLandingData } from '../src/registered-page-state.js';

test('mergeRegisteredLandingData clears stale registered failures after a successful retry', () => {
  const registered = { items: [{ canister_id: 'abc' }], page: 1n, page_size: 6n, total: 12n };
  const current = {
    hasAnyFailure: true,
    registered: { items: [] },
    errors: { registered: 'temporary outage' },
  };

  const next = mergeRegisteredLandingData(current, {
    registered,
    registeredError: null,
  });

  assert.equal(next.registered, registered);
  assert.equal(next.errors.registered, null);
  assert.equal(next.hasAnyFailure, false);
});

test('mergeRegisteredLandingData preserves overall failure state when another pane is still failing', () => {
  const current = {
    hasAnyFailure: true,
    errors: {
      registered: 'temporary outage',
      counts: 'counts still unavailable',
    },
  };

  const next = mergeRegisteredLandingData(current, {
    registered: { items: [], page: 0n, page_size: 6n, total: 0n },
    registeredError: null,
  });

  assert.equal(next.errors.registered, null);
  assert.equal(next.errors.counts, 'counts still unavailable');
  assert.equal(next.hasAnyFailure, true);
});
