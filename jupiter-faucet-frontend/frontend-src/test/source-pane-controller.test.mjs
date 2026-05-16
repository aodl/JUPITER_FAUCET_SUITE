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
      if (selector === '[data-source-heap-memory], [data-source-stable-memory], [data-source-total-memory]') {
        return nodes.filter((node) => (
          node.hasAttribute('data-source-heap-memory')
          || node.hasAttribute('data-source-stable-memory')
          || node.hasAttribute('data-source-total-memory')
        ));
      }
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

test('source pane renders cached canister info without calling the loader', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  const heap = makeNode('data-source-heap-memory', 'aaaaa-aa');
  const cacheKey = 'jupiter-faucet:source-pane-canister-info:v4:hist-aa';
  const cached = {
    cachedAt: Date.now(),
    infoByCanisterId: {
      'aaaaa-aa': {
        moduleHash: 'abc123',
        controllers: ['aaaaa-aa'],
        heapMemoryBytes: 2048,
      },
    },
  };

  await withFakeBrowser({
    nodes: [moduleHash, controllers, heap],
    storageEntries: [[cacheKey, JSON.stringify(cached)]],
  }, async () => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => false,
      loadCanisterInfo: async () => {
        throw new Error('loader should not run');
      },
    });
    await controller.ensureLoaded();
  });

  assert.equal(moduleHash.textContent, 'abc123');
  assert.equal(moduleHash.title, 'abc123');
  assert.match(controllers.innerHTML, /data-tracker-principal="aaaaa-aa"/);
  assert.match(controllers.innerHTML, />aaaaa-aa<\/a>/);
  assert.equal(heap.textContent, '2.00 KiB');
});

test('source pane writes cache only after controller data is complete', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  const cacheKey = 'jupiter-faucet:source-pane-canister-info:v4:hist-aa';
  let calls = 0;

  await withFakeBrowser({ nodes: [moduleHash, controllers] }, async (storage) => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => true,
      loadCanisterInfo: async (request) => {
        calls += 1;
        assert.deepEqual(request, {
          historianCanisterId: 'hist-aa',
          host: 'https://example.test',
          local: true,
        });
        return [{
          canister_id: { toText: () => 'aaaaa-aa' },
          module_hash_hex: ['def456'],
          controllers: [[{ toText: () => 'bbbbb-bb' }]],
          heap_memory_bytes: [],
          stable_memory_bytes: [],
          total_memory_bytes: [],
        }];
      },
    });
    await controller.ensureLoaded();
    assert.ok(storage.has(cacheKey), 'complete controller data should be cached');
  });

  assert.equal(calls, 1);
  assert.equal(moduleHash.textContent, 'def456');
  assert.match(controllers.innerHTML, /data-tracker-principal="bbbbb-bb"/);
  assert.match(controllers.innerHTML, />bbbbb-bb<\/a>/);
});

test('source pane marks values unavailable with the normalized loader failure', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');

  await withFakeBrowser({ nodes: [moduleHash, controllers] }, async () => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => false,
      loadCanisterInfo: async () => {
        throw new Error('network down');
      },
      normalizeLoadError: (error) => `normalized:${error.message}`,
    });
    await controller.ensureLoaded();
  });

  assert.equal(moduleHash.textContent, 'Unavailable');
  assert.equal(moduleHash.title, 'normalized:network down');
  assert.equal(controllers.innerHTML, 'Unavailable');
  assert.equal(controllers.title, 'normalized:network down');
});
