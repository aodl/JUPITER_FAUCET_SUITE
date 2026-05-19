import { loadCanisterModuleHashes, normalizeError } from '../dashboard-data.js';
import { readOpt } from '../candid-opt.js';
import { SOURCE_PANE_CACHE_TTL_MS } from './config.js';
import { formatBytes, formatPrincipal, formatSourceController } from './view-formatters.js';

const SOURCE_PANE_NEGATIVE_CACHE_TTL_MS = 30 * 1000;

function sourcePaneModuleHashNodes() {
  return Array.from(document.querySelectorAll('[data-source-module-hash]'));
}

function sourcePaneControllerNodes() {
  return Array.from(document.querySelectorAll('[data-source-controllers]'));
}

function sourcePaneMemoryNodes() {
  return Array.from(document.querySelectorAll('[data-source-memory]'));
}

function sourcePaneCanisterInfoNodes() {
  return [...sourcePaneModuleHashNodes(), ...sourcePaneControllerNodes(), ...sourcePaneMemoryNodes()];
}

function normalizeSourcePaneInfo(infoByCanisterId, canisterId) {
  const entry = infoByCanisterId?.[canisterId];
  if (!entry) return { moduleHash: null, controllers: null, totalMemoryBytes: null };
  if (typeof entry === 'string') return { moduleHash: entry || null, controllers: null, totalMemoryBytes: null };
  if (typeof entry !== 'object' || Array.isArray(entry)) return { moduleHash: null, controllers: null, totalMemoryBytes: null };
  return {
    moduleHash: entry.moduleHash || entry.module_hash_hex || null,
    controllers: Array.isArray(entry.controllers) ? entry.controllers : null,
    totalMemoryBytes: entry.totalMemoryBytes ?? entry.total_memory_bytes ?? null,
  };
}

function renderSourceControllers(controllers) {
  if (controllers === null || controllers === undefined) return 'Unavailable';
  if (!Array.isArray(controllers) || controllers.length === 0) return 'none';
  return controllers.map(formatSourceController).filter(Boolean).join(', ') || 'none';
}

function hasMemoryValue(value) {
  return value !== null && value !== undefined;
}

export function createSourcePaneController({
  frontendConfig,
  isLocalHost,
  loadCanisterInfo = loadCanisterModuleHashes,
  normalizeLoadError = normalizeError,
  now = () => Date.now(),
}) {
  let sourcePaneModuleHashesLoadedAt = 0;
  let sourcePaneModuleHashesLoadedTtlMs = 0;
  let sourcePaneModuleHashesRequest = null;

  const sourcePaneModuleHashCacheKey = () => {
    if (!frontendConfig?.historianCanisterId) return null;
    return `jupiter-faucet:source-pane-canister-info:v4:${frontendConfig.historianCanisterId}`;
  };

  const sourcePaneExpectedCanisterIds = () => Array.from(new Set(sourcePaneCanisterInfoNodes()
    .map((node) => (
      node.getAttribute('data-source-module-hash')
      || node.getAttribute('data-source-controllers')
      || node.getAttribute('data-source-memory')
      || ''
    ))
    .filter(Boolean)));

  const sourcePaneInfoHasCompleteControllerData = (infoByCanisterId) => sourcePaneExpectedCanisterIds().every((canisterId) => (
    Array.isArray(normalizeSourcePaneInfo(infoByCanisterId, canisterId).controllers)
  ));

  const applySourcePaneModuleHashes = (infoByCanisterId, { fallbackTitle = '' } = {}) => {
    sourcePaneModuleHashNodes().forEach((node) => {
      const canisterId = node.getAttribute('data-source-module-hash') || '';
      const { moduleHash } = normalizeSourcePaneInfo(infoByCanisterId, canisterId);
      node.textContent = moduleHash || 'Unavailable';
      if (moduleHash) node.setAttribute('title', moduleHash);
      else if (fallbackTitle) node.setAttribute('title', fallbackTitle);
      else node.removeAttribute('title');
    });
    sourcePaneControllerNodes().forEach((node) => {
      const canisterId = node.getAttribute('data-source-controllers') || '';
      const { controllers } = normalizeSourcePaneInfo(infoByCanisterId, canisterId);
      node.innerHTML = renderSourceControllers(controllers);
      if (controllers === null && fallbackTitle) node.setAttribute('title', fallbackTitle);
      else node.removeAttribute('title');
    });
    sourcePaneMemoryNodes().forEach((node) => {
      const canisterId = node.getAttribute('data-source-memory') || '';
      const { totalMemoryBytes } = normalizeSourcePaneInfo(infoByCanisterId, canisterId);
      node.textContent = hasMemoryValue(totalMemoryBytes) ? `${formatBytes(totalMemoryBytes)} total` : 'Unavailable';
      if (!hasMemoryValue(totalMemoryBytes) && fallbackTitle) node.setAttribute('title', fallbackTitle);
      else node.removeAttribute('title');
    });
  };

  const readSourcePaneModuleHashCache = () => {
    const cacheKey = sourcePaneModuleHashCacheKey();
    if (!cacheKey) return null;
    try {
      const raw = window.localStorage.getItem(cacheKey);
      if (!raw) return null;
      const parsed = JSON.parse(raw);
      if (!parsed || typeof parsed !== 'object') return null;
      const cachedAt = Number(parsed.cachedAt || 0);
      if (!Number.isFinite(cachedAt) || cachedAt <= 0) return null;
      if ((now() - cachedAt) > SOURCE_PANE_CACHE_TTL_MS) return null;
      const infoByCanisterId = parsed.infoByCanisterId || parsed.hashByCanisterId;
      if (!infoByCanisterId || typeof infoByCanisterId !== 'object') return null;
      return { cachedAt, infoByCanisterId };
    } catch { return null; }
  };

  const writeSourcePaneModuleHashCache = (infoByCanisterId) => {
    const cacheKey = sourcePaneModuleHashCacheKey();
    if (!cacheKey || !sourcePaneInfoHasCompleteControllerData(infoByCanisterId)) return;
    try { window.localStorage.setItem(cacheKey, JSON.stringify({ cachedAt: now(), infoByCanisterId })); }
    catch { /* Ignore storage failures. */ }
  };

  const ensureLoaded = async () => {
    const infoNodes = sourcePaneCanisterInfoNodes();
    if (infoNodes.length === 0 || !frontendConfig?.historianCanisterId) return;
    if (
      sourcePaneModuleHashesLoadedAt > 0
      && sourcePaneModuleHashesLoadedTtlMs > 0
      && (now() - sourcePaneModuleHashesLoadedAt) <= sourcePaneModuleHashesLoadedTtlMs
    ) return;
    const cached = readSourcePaneModuleHashCache();
    if (cached) {
      applySourcePaneModuleHashes(cached.infoByCanisterId);
      sourcePaneModuleHashesLoadedAt = cached.cachedAt;
      sourcePaneModuleHashesLoadedTtlMs = SOURCE_PANE_CACHE_TTL_MS;
      return;
    }
    if (sourcePaneModuleHashesRequest) {
      await sourcePaneModuleHashesRequest;
      return;
    }
    sourcePaneModuleHashesRequest = (async () => {
      try {
        const infos = await loadCanisterInfo({
          historianCanisterId: frontendConfig.historianCanisterId,
          host: window.location.origin,
          local: isLocalHost(),
        });
        const infoByCanisterId = Object.fromEntries(
          infos.map((item) => [formatPrincipal(item.canister_id), {
            moduleHash: readOpt(item.module_hash_hex) || null,
            controllers: readOpt(item.controllers)?.map(formatPrincipal) || null,
            totalMemoryBytes: readOpt(item.total_memory_bytes),
          }]),
        );
        applySourcePaneModuleHashes(infoByCanisterId);
        writeSourcePaneModuleHashCache(infoByCanisterId);
        sourcePaneModuleHashesLoadedAt = now();
        sourcePaneModuleHashesLoadedTtlMs = sourcePaneInfoHasCompleteControllerData(infoByCanisterId)
          ? SOURCE_PANE_CACHE_TTL_MS
          : SOURCE_PANE_NEGATIVE_CACHE_TTL_MS;
      } catch (error) {
        const reason = normalizeLoadError(error);
        applySourcePaneModuleHashes({}, { fallbackTitle: reason });
        sourcePaneModuleHashesLoadedAt = now();
        sourcePaneModuleHashesLoadedTtlMs = SOURCE_PANE_NEGATIVE_CACHE_TTL_MS;
      } finally {
        sourcePaneModuleHashesRequest = null;
      }
    })();
    await sourcePaneModuleHashesRequest;
  };

  return { ensureLoaded };
}
