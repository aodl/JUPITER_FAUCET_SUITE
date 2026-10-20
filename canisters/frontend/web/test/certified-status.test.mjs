import test from 'node:test';
import assert from 'node:assert/strict';
import { HttpAgent } from '@icp-sdk/core/agent';
import { resetAgentCacheForTests } from '../src/app/agent.js';
import { loadCertifiedCanisterStatus } from '../src/data/dashboard-loaders.js';

test('certified mainnet status uses the trusted API, binds paths to the requested ID and has no query fallback', async () => {
  const originalCreate = HttpAgent.create;
  const calls = [];
  resetAgentCacheForTests();
  HttpAgent.create = async (options) => {
    assert.equal(options.host, 'https://icp-api.io');
    assert.equal(options.rootKey, undefined, 'SDK default mainnet root key');
    return {
      rootKey: Uint8Array.of(1),
      async fetchRootKey() { assert.fail('mainnet must never fetch an untrusted root key'); },
      async query() { assert.fail('no unsigned query fallback'); },
      async readState(id, { paths }) {
        calls.push(paths);
        assert.equal(id.toText(), 'rrkah-fqaaa-aaaaa-aaaaq-cai');
        if (new TextDecoder().decode(paths[0][0]) === 'canister') {
          assert.deepEqual(paths[0][1], id.toUint8Array());
        }
        return { certificate: Uint8Array.of(0) };
      },
    };
  };
  try {
    const [result] = await loadCertifiedCanisterStatus({ canisterIds: ['rrkah-fqaaa-aaaaa-aaaaq-cai'], host: 'https://untrusted.example' });
    assert.equal(result.moduleHash, null);
    assert.equal(result.controllers, null);
    assert.ok(result.error);
    assert.equal(calls.length, 3);
  } finally {
    HttpAgent.create = originalCreate;
    resetAgentCacheForTests();
  }
});

test('local certified reads refuse a non-loopback endpoint', async () => {
  await assert.rejects(loadCertifiedCanisterStatus({ canisterIds: ['aaaaa-aa'], host: 'https://icp-api.io', local: true }), /loopback/);
});
