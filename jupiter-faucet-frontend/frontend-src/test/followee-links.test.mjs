import test from 'node:test';
import assert from 'node:assert/strict';

import { escapeHtml, formatFolloweeLinks } from '../src/followee-links.js';

test('escapeHtml escapes reserved characters for html sinks', () => {
  assert.equal(escapeHtml(`<'"&>`), '&lt;&#39;&quot;&amp;&gt;');
});

test('formatFolloweeLinks escapes hostile-looking followee ids before rendering html', () => {
  const hostile = {
    toString() {
      return '<img src=x onerror=alert(1)>';
    },
  };
  const html = formatFolloweeLinks({
    followees: [[123n, { followees: [{ id: hostile }] }]],
  });

  assert.match(html, /&lt;img src=x onerror=alert\(1\)&gt;/);
  assert.doesNotMatch(html, /<img src=x onerror=alert\(1\)>/);
  assert.match(html, /https:\/\/dashboard\.internetcomputer\.org\/neuron\/&lt;img src=x onerror=alert\(1\)&gt;/);
});

test('formatFolloweeLinks deduplicates ids and renders alpha-vote alias safely', () => {
  const html = formatFolloweeLinks({
    followees: [
      [123n, { followees: [{ id: { toString: () => '2947465672511369' } }, { id: { toString: () => '2947465672511369' } }] }],
    ],
  });

  assert.equal((html.match(/href="https:\/\/dashboard\.internetcomputer\.org\/neuron\/2947465672511369"/g) || []).length, 1);
  assert.equal((html.match(/, /g) || []).length, 0, 'deduplicated single followee should not introduce list separators');
  assert.match(html, /title="αlpha-vote \(2947465672511369\)"/);
});
