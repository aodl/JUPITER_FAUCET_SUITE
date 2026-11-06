import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { spawnSync } from 'node:child_process';
import { buildSync } from 'esbuild';

test('an acceptance frontend build rejects a conflicting production network override', () => {
  const result = spawnSync(process.execPath, ['canisters/frontend/web/build-frontend.mjs'], {
    env: { ...process.env, JUPITER_BUILD_ENVIRONMENT: 'acceptance', JUPITER_FRONTEND_NETWORK: 'ic' },
    encoding: 'utf8',
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /acceptance.*network.*conflict/i);
});

test('acceptance payment copies and neuron links consume only trusted build bindings', async () => {
  const config = JSON.parse(execFileSync('python3', ['-c', `
import sys, json
sys.path.insert(0, 'tools/acceptance')
from test_profile import fixture
from profile import frontend
print(json.dumps(frontend(fixture())))
  `], { encoding: 'utf8' }));
  const bundle = buildSync({
    stdin: { contents: `
      export * from './canisters/frontend/web/src/app/config.js';
      export { createStakePaneController } from './canisters/frontend/web/src/app/stake-pane-controller.js';
      export { dquorumStakingAccount, accountIdentifierHex } from './canisters/frontend/web/src/data/dashboard-transforms.js';
    `, resolveDir: process.cwd() },
    bundle: true, write: false, format: 'esm', platform: 'browser',
    define: { __JUPITER_FRONTEND_CONFIG__: JSON.stringify(config) },
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].contents).toString('base64')}`);
  assert.equal(module.JUPITER_NEURON_ID.toString(), config.neuronId);
  assert.equal(module.JUPITER_RELAY_CANISTER_ID, config.relayCanisterId);
  assert.equal(module.dquorumStakingAccount(), null);
  const previousDocument = globalThis.document;
  const nodes = new Map();
  for (const id of ['stake-pane-account-link', 'stake-neuron-id-link', 'stake-pane-balance',
    'how-staking-account-address', 'how-staking-account-identifier',
    'how-staking-account-link', 'how-staking-account-identifier-link']) {
    nodes.set(id, { textContent: '', querySelector: () => null });
  }
  const copies = new Map();
  globalThis.document = { getElementById: (id) => nodes.get(id) };
  try {
    const controller = module.createStakePaneController({
      neuronId: module.JUPITER_NEURON_ID,
      simulatorController: { setAgeBonus() {} },
      setCopyButton: (id, callback) => copies.set(id, callback),
    });
    controller.renderHowItWorksAccount();
    controller.renderStakePane({ status: { staking_account: { owner: 'acjuz-liaaa-aaaar-qb4qq-cai' } }, stakeE8s: 1n });
    assert.equal(copies.get('copy-how-staking-account')(), config.stakingAccount.address);
    assert.equal(copies.get('copy-how-staking-account-identifier')(), config.stakingAccount.explorerAccountHex);
    assert.equal(nodes.get('stake-neuron-id-link').href, `https://dashboard.internetcomputer.org/neuron/${config.neuronId}`);
    assert.notEqual(config.stakingAccount.explorerAccountHex, '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d');
  } finally {
    globalThis.document = previousDocument;
  }
});
