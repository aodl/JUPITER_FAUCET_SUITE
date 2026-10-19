import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

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

test('source pane renders cached canister info without calling the loader', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  const memory = makeNode('data-source-memory', 'aaaaa-aa');
  const cacheKey = 'jupiter-faucet:source-pane-canister-info:v5:hist-aa';
  const cached = {
    cachedAt: Date.now(),
    infoByCanisterId: {
      'aaaaa-aa': {
        moduleHash: 'abc123',
        controllers: ['aaaaa-aa'],
        totalMemoryBytes: '2048',
      },
    },
  };

  await withFakeBrowser({
    nodes: [moduleHash, controllers, memory],
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
  assert.equal(memory.textContent, '2.00 KiB total');
});

test('source pane writes cache only after controller data is complete', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  const cacheKey = 'jupiter-faucet:source-pane-canister-info:v5:hist-aa';
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

test('source pane renders total memory without requiring a heap or stable breakdown', async () => {
  const memory = makeNode('data-source-memory', 'aaaaa-aa');

  await withFakeBrowser({ nodes: [memory] }, async () => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => false,
      loadCanisterInfo: async () => [{
        canister_id: { toText: () => 'aaaaa-aa' },
        module_hash_hex: [],
        controllers: [[{ toText: () => 'bbbbb-bb' }]],
        total_memory_bytes: [11_219_763n],
      }],
    });
    await controller.ensureLoaded();
  });

  assert.equal(memory.textContent, '10.7 MiB total');
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

test('source pane retries incomplete module hash responses with the short negative TTL', async () => {
  const moduleHash = makeNode('data-source-module-hash', 'aaaaa-aa');
  const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
  let calls = 0;
  let currentNow = 1_000;

  await withFakeBrowser({ nodes: [moduleHash, controllers] }, async (storage) => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' },
      isLocalHost: () => false,
      now: () => currentNow,
      loadCanisterInfo: async () => {
        calls += 1;
        return [];
      },
    });

    await controller.ensureLoaded();
    currentNow += 29_000;
    await controller.ensureLoaded();
    currentNow += 2_000;
    await controller.ensureLoaded();

    assert.equal(storage.size, 0, 'incomplete responses should not be persisted to localStorage');
  });

  assert.equal(calls, 2);
  assert.equal(moduleHash.textContent, 'Unavailable');
  assert.equal(controllers.innerHTML, 'Unavailable');
});

test('generated historian declaration marks get_canister_module_hashes as a query', () => {
  const declaration = readFileSync(
    new URL('../declarations/jupiter_historian/jupiter_historian.did.js', import.meta.url),
    'utf8',
  );

  assert.match(
    declaration,
    /'get_canister_module_hashes'\s*:\s*IDL\.Func\(\s*\[\],\s*\[IDL\.Vec\(CanisterModuleHash\)\],\s*\['query'\],\s*\)/,
  );
});

for (const memoryValue of [11_219_763n, 0n, null, 9_007_199_254_740_993n, 18_446_744_073_709_551_615n]) {
  test(`source pane persists exact Nat64 memory ${memoryValue} and reuses it in a fresh controller`, async () => {
    const memory = makeNode('data-source-memory', 'aaaaa-aa');
    const hash = makeNode('data-source-module-hash', 'aaaaa-aa');
    const controllers = makeNode('data-source-controllers', 'aaaaa-aa');
    let calls = 0;
    await withFakeBrowser({ nodes: [memory, hash, controllers] }, async (storage) => {
      const options = {
        frontendConfig: { historianCanisterId: 'hist-aa' },
        isLocalHost: () => false,
        now: () => 100_000,
        loadCanisterInfo: async () => {
          calls += 1;
          return [{
            canister_id: { toText: () => 'aaaaa-aa' },
            module_hash_hex: ['abc123'], controllers: [[{ toText: () => 'bbbbb-bb' }]],
            total_memory_bytes: memoryValue === null ? [] : [memoryValue],
          }];
        },
      };
      await createSourcePaneController(options).ensureLoaded();
      const raw = storage.get('jupiter-faucet:source-pane-canister-info:v5:hist-aa');
      assert.ok(raw, 'a complete real Nat64 response must be persisted');
      assert.deepEqual(JSON.parse(raw), {
        cachedAt: 100_000,
        infoByCanisterId: { 'aaaaa-aa': {
          moduleHash: 'abc123', controllers: ['bbbbb-bb'], totalMemoryBytes: memoryValue?.toString() ?? null,
        } },
      });
      const renderedMemory = memory.textContent;
      memory.textContent = '';
      hash.textContent = '';
      controllers.innerHTML = '';
      await createSourcePaneController(options).ensureLoaded();
      assert.equal(calls, 1, 'a fresh instance should use the persisted complete response');
      assert.equal(memory.textContent, renderedMemory);
      assert.equal(hash.textContent, 'abc123');
      assert.match(controllers.innerHTML, /bbbbb-bb/);
      if (memoryValue === 11_219_763n) assert.equal(memory.textContent, '10.7 MiB total');
      if (memoryValue === 0n) assert.equal(memory.textContent, '0 B total');
      if (memoryValue === null) assert.equal(memory.textContent, 'Unavailable');
    });
  });
}

const cacheKey = 'jupiter-faucet:source-pane-canister-info:v5:hist-aa';
const validCacheInfo = () => ({ moduleHash: 'abc123', controllers: ['bbbbb-bb'], totalMemoryBytes: '11219763' });

for (const value of ['-1', '1.0', '1e3', '', '01', 'invalid', '18446744073709551616', 11219763]) {
  test(`source pane discards malformed persisted Nat64 ${JSON.stringify(value)}`, async () => {
    const memory = makeNode('data-source-memory', 'aaaaa-aa');
    let calls = 0;
    const cached = { cachedAt: 100_000, infoByCanisterId: { 'aaaaa-aa': { ...validCacheInfo(), totalMemoryBytes: value } } };
    await withFakeBrowser({ nodes: [memory], storageEntries: [[cacheKey, JSON.stringify(cached)]] }, async () => {
      await createSourcePaneController({
        frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false, now: () => 100_001,
        loadCanisterInfo: async () => { calls += 1; return []; },
      }).ensureLoaded();
    });
    assert.equal(calls, 1);
    assert.equal(memory.textContent, 'Unavailable');
  });
}

for (const [label, raw] of [
  ['corrupt JSON', '{'],
  ['string entry', JSON.stringify({ cachedAt: 100_000, infoByCanisterId: { 'aaaaa-aa': 'abc123' } })],
  ['old hash map', JSON.stringify({ cachedAt: 100_000, hashByCanisterId: { 'aaaaa-aa': validCacheInfo() } })],
  ['snake case entry', JSON.stringify({ cachedAt: 100_000, infoByCanisterId: { 'aaaaa-aa': { module_hash_hex: 'abc123', controllers: [], total_memory_bytes: '123' } } })],
  ['missing controllers', JSON.stringify({ cachedAt: 100_000, infoByCanisterId: { 'aaaaa-aa': { ...validCacheInfo(), controllers: null } } })],
  ['missing memory field', JSON.stringify({ cachedAt: 100_000, infoByCanisterId: { 'aaaaa-aa': { moduleHash: null, controllers: [] } } })],
  ['string timestamp', JSON.stringify({ cachedAt: '100000', infoByCanisterId: { 'aaaaa-aa': validCacheInfo() } })],
  ['nonfinite timestamp', '{"cachedAt":1e999,"infoByCanisterId":{}}'],
  ['future timestamp', JSON.stringify({ cachedAt: 100_002, infoByCanisterId: { 'aaaaa-aa': validCacheInfo() } })],
]) {
  test(`source pane discards ${label}`, async () => {
    let calls = 0;
    const nodes = [makeNode('data-source-module-hash', 'aaaaa-aa')];
    await withFakeBrowser({ nodes, storageEntries: [[cacheKey, raw]] }, async () => {
      await createSourcePaneController({
        frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false, now: () => 100_001,
        loadCanisterInfo: async () => { calls += 1; return []; },
      }).ensureLoaded();
    });
    assert.equal(calls, 1);
    assert.equal(nodes[0].textContent, 'Unavailable');
  });
}

test('source pane expires a well-formed persistent cache after its TTL', async () => {
  const memory = makeNode('data-source-memory', 'aaaaa-aa');
  const cached = { cachedAt: 100_000, infoByCanisterId: { 'aaaaa-aa': validCacheInfo() } };
  let calls = 0;
  await withFakeBrowser({ nodes: [memory], storageEntries: [[cacheKey, JSON.stringify(cached)]] }, async () => {
    const { SOURCE_PANE_CACHE_TTL_MS } = await import('../src/app/config.js');
    await createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false,
      now: () => 100_001 + SOURCE_PANE_CACHE_TTL_MS,
      loadCanisterInfo: async () => { calls += 1; return []; },
    }).ensureLoaded();
  });
  assert.equal(calls, 1);
  assert.equal(memory.textContent, 'Unavailable');
});

for (const failingMethod of ['getItem', 'setItem']) {
  test(`source pane tolerates a genuine storage ${failingMethod} failure with real BigInt metadata`, async () => {
    let calls = 0;
    let failures = 0;
    const memory = makeNode('data-source-memory', 'aaaaa-aa');
    await withFakeBrowser({ nodes: [memory] }, async () => {
      window.localStorage[failingMethod] = () => { failures += 1; throw new Error('storage denied'); };
      const controller = createSourcePaneController({
        frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false,
        loadCanisterInfo: async () => {
          calls += 1;
          return [{ canister_id: 'aaaaa-aa', module_hash_hex: [], controllers: [[]], total_memory_bytes: [11_219_763n] }];
        },
      });
      await controller.ensureLoaded();
      await controller.ensureLoaded();
    });
    assert.equal(calls, 1);
    assert.equal(failures, 1);
    assert.equal(memory.textContent, '10.7 MiB total');
  });
}

test('source pane does not persist complete memory when controller metadata is unavailable', async () => {
  const memory = makeNode('data-source-memory', 'aaaaa-aa');
  let calls = 0;
  let timestamp = 100_000;
  await withFakeBrowser({ nodes: [memory] }, async (storage) => {
    const controller = createSourcePaneController({
      frontendConfig: { historianCanisterId: 'hist-aa' }, isLocalHost: () => false, now: () => timestamp,
      loadCanisterInfo: async () => {
        calls += 1;
        return [{ canister_id: 'aaaaa-aa', module_hash_hex: ['abc123'], controllers: [], total_memory_bytes: [11_219_763n] }];
      },
    });
    await controller.ensureLoaded();
    assert.equal(storage.size, 0);
    timestamp += 31_000;
    await controller.ensureLoaded();
    assert.equal(storage.size, 0);
  });
  assert.equal(calls, 2);
  assert.equal(memory.textContent, '10.7 MiB total');
});
