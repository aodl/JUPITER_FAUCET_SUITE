import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';

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
  let text = await readFile(filename, 'utf8');
  if ((process.env.JUPITER_BUILD_ENVIRONMENT || process.env.ICP_ENVIRONMENT) === 'acceptance') {
    if (filename.endsWith('/.well-known/ic-domains')) {
      text = '';
    } else {
      text = execFileSync('python3', ['tools/acceptance/profile.py', 'render',
        process.env.JUPITER_ACCEPTANCE_PROFILE || 'deploy/acceptance/profile.json'],
      { input: text, encoding: 'utf8' });
    }
  }
  await writeFile(filename, renderFrontendAsset(text, assetVersion, bundlePath));
}
