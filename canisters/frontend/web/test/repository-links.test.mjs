import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const checkedFiles = [
  new URL('../../public/index.html', import.meta.url),
  new URL('../../../../tools/xtask/README.md', import.meta.url),
];

const staleReferences = [
  ['canisters', 'faucet-frontend'].join('/'),
  ['#reproducible', 'release-artifacts'].join('-'),
  ['frontend', 'src'].join('-'),
];

test('frontend-facing repository links do not use stale restructure paths', () => {
  for (const fileUrl of checkedFiles) {
    const body = readFileSync(fileUrl, 'utf8');
    for (const staleReference of staleReferences) {
      assert.equal(
        body.includes(staleReference),
        false,
        `${fileUrl.pathname} contains stale reference ${staleReference}`,
      );
    }
  }
});
