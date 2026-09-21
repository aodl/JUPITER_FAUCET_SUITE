import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function renderFrontendAsset(text, assetVersion, bundlePath) {
  const rendered = text
    .replaceAll('__ASSET_VERSION__', assetVersion)
    .replaceAll('__APP_BUNDLE_PATH__', bundlePath);
  if (rendered.includes('__ASSET_VERSION__') || rendered.includes('__APP_BUNDLE_PATH__')) {
    throw new Error('unresolved frontend asset token');
  }
  return rendered;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [, , filename, assetVersion, bundlePath] = process.argv;
  if (!filename || !assetVersion || !bundlePath) throw new Error('render arguments required');
  const text = await readFile(filename, 'utf8');
  await writeFile(filename, renderFrontendAsset(text, assetVersion, bundlePath));
}
