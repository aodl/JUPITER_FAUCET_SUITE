import test from 'node:test';
import assert from 'node:assert/strict';

import { createSourcePaneController } from '../src/app/source-pane-controller.js';

function makeNode(attributeName, canisterId) {
  const attrs = new Map([[attributeName, canisterId]]);
  return {
    textContent: '',
    innerHTML: '',
    title: '',
    hasAttribute(name) {
      return attrs.has(name);
    },
    getAttribute(name) {
      return attrs.get(name) || null;
    },
    setAttribute(name, value) {
      if (name === 'title') this.title = value;
      attrs.set(name, value);
    },
    removeAttribute(name) {
      if (name === 'title') this.title = '';
      attrs.delete(name);
    },
  };
}

async function withFakeBrowser({ nodes, storageEntries = [] }, fn) {
  const originalDocument = globalThis.document;
  const originalWindow = globalThis.window;
  const storage = new Map(storageEntries);
  globalThis.document = {
    querySelectorAll(selector) {
      if (selector === '[data-source-module-hash]') return nodes.filter((node) => node.hasAttribute('data-source-module-hash'));
      if (selector === '[data-source-controllers]') return nodes.filter((node) => node.hasAttribute('data-source-controllers'));
      if (selector === '[data-source-memory]') return nodes.filter((node) => node.hasAttribute('data-source-memory'));
      return [];
    },
  };
  globalThis.window = {
    location: { origin: 'https://example.test' },
    localStorage: {
      getItem(key) {
        return storage.get(key) || null;
      },
      setItem(key, value) {
        storage.set(key, value);
      },
    },
  };
  try {
    await fn(storage);
  } finally {
    globalThis.document = originalDocument;
    globalThis.window = originalWindow;
  }
}

test('unsigned Historian substitution cannot supply source hashes or controllerless assurance', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  await withFakeBrowser({ nodes: [moduleHash, controllers] }, async () => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => false,
      loadCanisterInfo: async () => [{ canister_id: 'aaaaa-aa', module_hash_hex: ['ab'.repeat(32)], controllers: [[]] }],
      loadCertifiedStatus: async () => { throw new Error('certificate verification failed'); },
    });
    await controller.ensureLoaded();
  });
  assert.equal(moduleHash.textContent, 'Unavailable');
  assert.equal(controllers.innerHTML, 'Unavailable');
});

test('source pane ignores the old cache and renders certified metadata separately from memory', async () => {
  const hash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  const memory = makeNode('data-source-memory', 'aaaaa-aa');
  const forgedHash = 'ab'.repeat(32);
  const actualHash = 'cd'.repeat(32);
  const cacheKey = 'jupiter-faucet:source-pane-canister-info:v5:hist-aa';
  await withFakeBrowser({
    nodes: [hash, controllers, memory],
    storageEntries: [[cacheKey, JSON.stringify({ cachedAt: Date.now(), infoByCanisterId: {
      'aaaaa-aa': { moduleHash: forgedHash, controllers: [], totalMemoryBytes: '1' },
    } })]],
  }, async (storage) => {
    const before = [...storage];
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false,
      loadCanisterInfo: async () => [{ canister_id: 'aaaaa-aa', module_hash_hex: [forgedHash], controllers: [[]], total_memory_bytes: [11_219_763n] }],
      loadCertifiedStatus: async ({ canisterIds, local }) => {
        assert.deepEqual(canisterIds, ['aaaaa-aa']);
        assert.equal(local, false);
        return [{ canisterId: 'aaaaa-aa', moduleHash: actualHash, controllers: ['rrkah-fqaaa-aaaaa-aaaaq-cai'], certifiedAt: '2026-10-20T00:00:00.000Z' }];
      },
    });
    await controller.ensureLoaded();
    assert.equal(hash.textContent, actualHash);
    assert.match(hash.title, /Certificate checked at 2026-10-20/);
    assert.match(controllers.innerHTML, /rrkah-fqaaa-aaaaa-aaaaq-cai/);
    assert.equal(memory.textContent, '10.7 MiB total');
    assert.match(memory.title, /not certificate-verified/);
    assert.deepEqual([...storage], before, 'old cache is neither used nor rewritten');
  });
});

for (const value of [[], null]) {
  test(`certified empty controllers and unavailable controllers remain distinct: ${JSON.stringify(value)}`, async () => {
    const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
    await withFakeBrowser({ nodes: [controllers] }, async () => {
      await createSourcePaneController({
        frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false,
        loadCanisterInfo: async () => [],
        loadCertifiedStatus: async () => [{ canisterId: 'aaaaa-aa', controllers: value, certifiedAt: '2026-10-20T00:00:00Z' }],
      }).ensureLoaded();
    });
    assert.equal(controllers.innerHTML, value === null ? 'Unavailable' : 'none');
  });
}

test('mismatched canister identity is not rendered, even if memory and query metadata are available', async () => {
  const hash = makeNode('data-source-module-hash', 'aaaaa-aa');
  await withFakeBrowser({ nodes: [hash] }, async () => {
    await createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false,
      loadCanisterInfo: async () => [{ canister_id: 'aaaaa-aa', module_hash_hex: ['ab'.repeat(32)] }],
      loadCertifiedStatus: async () => [{ canisterId: 'rrkah-fqaaa-aaaaa-aaaaq-cai', moduleHash: 'cd'.repeat(32) }],
    }).ensureLoaded();
  });
  assert.equal(hash.textContent, 'Unavailable');
});

test('overlapping source requests deduplicate; a later refresh obtains new certificates', async () => {
  let calls = 0;
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  await withFakeBrowser({ nodes: [makeNode('data-source-module-hash', 'aaaaa-aa')] }, async () => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false,
      loadCanisterInfo: async () => [],
      loadCertifiedStatus: async () => { calls += 1; await gate; return []; },
    });
    const first = controller.ensureLoaded();
    const second = controller.ensureLoaded();
    assert.equal(calls, 1);
    release();
    await Promise.all([first, second]);
    await controller.ensureLoaded();
    assert.equal(calls, 2);
  });
});
