import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync } from 'node:fs';
import { relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const publicRoot = fileURLToPath(new URL('../../public/', import.meta.url));
const URL_SAFE_ASSET_PATH = /^[A-Za-z0-9._/-]+$/u;

function publicAssetPaths(directory = publicRoot) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) return publicAssetPaths(path);
    return [relative(publicRoot, path)];
  });
}

test('public asset paths use URL-safe characters', () => {
  const unsafePaths = publicAssetPaths()
    .filter((path) => !URL_SAFE_ASSET_PATH.test(path))
    .sort();

  assert.deepEqual(
    unsafePaths,
    [],
    `URL-unsafe public asset paths:\n${unsafePaths.join('\n')}`,
  );
});
