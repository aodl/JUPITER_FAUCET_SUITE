import { mkdir, readFile, writeFile, rm, rename, readdir } from 'node:fs/promises';
import crypto from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..', '..', '..');
const assetsDir = path.join(repoRoot, 'canisters', 'frontend', 'public');
const outDir = path.join(assetsDir, 'generated');
const entryPoint = path.join(__dirname, 'src', 'main.js');
const manifestPath = path.join(outDir, 'frontend-bundle.json');
const tempOutfile = path.join(outDir, '__app-build.js');

const network = process.env.JUPITER_FRONTEND_NETWORK || process.env.ICP_ENVIRONMENT || process.env.ICP_NETWORK || 'ic';
const mappingPath = path.join(
  repoRoot,
  '.icp',
  network === 'local' ? 'cache' : 'data',
  'mappings',
  `${network}.ids.json`
);
let canisterIds = {};
try {
  canisterIds = JSON.parse(await readFile(mappingPath, 'utf8'));
} catch {
  canisterIds = {};
}

const resolveCanisterId = (name) => {
  return process.env[`CANISTER_ID_${name.toUpperCase()}`] || canisterIds?.[name] || '';
};

const runtimeConfig = {
  network,
  historianCanisterId: resolveCanisterId('jupiter_historian'),
  frontendCanisterId: resolveCanisterId('jupiter_faucet_frontend'),
};

await mkdir(outDir, { recursive: true });
for (const oldName of await readdir(outDir)) {
  if (/^app(?:\.[a-f0-9]{12})?\.js$/.test(oldName) || oldName === '__app-build.js') {
    await rm(path.join(outDir, oldName), { force: true });
  }
}

await build({
  entryPoints: [entryPoint],
  bundle: true,
  outfile: tempOutfile,
  format: 'esm',
  platform: 'browser',
  target: ['es2022'],
  sourcemap: false,
  minify: false,
  define: {
    __JUPITER_FRONTEND_CONFIG__: JSON.stringify(runtimeConfig),
    global: 'globalThis',
  },
});

const bytes = await readFile(tempOutfile);
const hash = crypto.createHash('sha256').update(bytes).digest('hex').slice(0, 12);
const finalName = `app.${hash}.js`;
const finalPath = path.join(outDir, finalName);
await rename(tempOutfile, finalPath);
await writeFile(manifestPath, JSON.stringify({ bundlePath: `generated/${finalName}` }, null, 2) + '\n');

console.log(`Built frontend bundle for ${network} -> ${path.relative(repoRoot, finalPath)}`);
