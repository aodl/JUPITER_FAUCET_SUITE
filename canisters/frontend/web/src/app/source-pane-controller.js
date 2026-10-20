import { loadCanisterModuleHashes, normalizeError } from '../dashboard-data.js';
import { loadCertifiedCanisterStatus } from '../data/dashboard-loaders.js';
import { readOpt } from '../candid-opt.js';
import { formatBytes, formatPrincipal, formatSourceController } from './view-formatters.js';

function renderSourceControllers(controllers) {
  if (!Array.isArray(controllers)) return 'Unavailable';
  return controllers.length ? controllers.map(formatSourceController).join(', ') : 'none';
}

export function createSourcePaneController({
  frontendConfig,
  isLocalHost,
  loadCanisterInfo = loadCanisterModuleHashes,
  loadCertifiedStatus = loadCertifiedCanisterStatus,
  normalizeLoadError = normalizeError,
}) {
  let request = null;
  const nodes = (attribute) => Array.from(document.querySelectorAll(`[${attribute}]`));
  const ensureLoaded = async () => {
    if (request) return request;
    const hashes = nodes('data-source-module-hash');
    const controllerNodes = nodes('data-source-controllers');
    const memories = nodes('data-source-memory');
    const canisterIds = [...new Set([...hashes, ...controllerNodes].map((node) => (
      node.getAttribute('data-source-module-hash') || node.getAttribute('data-source-controllers')
    )))];
    if (!hashes.length && !controllerNodes.length && !memories.length) return;
    // No persistent verifier cache. Every completed refresh reads fresh evidence;
    // only overlapping requests are deduplicated. Query metadata is memory-only.
    request = (async () => {
      const options = { host: window.location.origin, local: frontendConfig?.network === 'local' && isLocalHost() };
      const [certified, observed] = await Promise.allSettled([
        loadCertifiedStatus({ ...options, canisterIds }),
        loadCanisterInfo({ ...options, historianCanisterId: frontendConfig?.historianCanisterId }),
      ]);
      const info = new Map();
      if (certified.status === 'fulfilled') {
        for (const item of certified.value) {
          if (canisterIds.includes(item.canisterId)) info.set(item.canisterId, item);
        }
      }
      const fallback = certified.status === 'rejected' ? normalizeLoadError(certified.reason) : 'Certified paths unavailable';
      for (const node of hashes) {
        const item = info.get(node.getAttribute('data-source-module-hash'));
        const value = item?.moduleHash;
        node.textContent = value || 'Unavailable';
        node.setAttribute('title', value ? `Certificate checked at ${item.checkedAt}: ${value}` : item?.error || fallback);
      }
      for (const node of controllerNodes) {
        const item = info.get(node.getAttribute('data-source-controllers'));
        const controllers = item?.controllers;
        node.innerHTML = renderSourceControllers(controllers);
        node.setAttribute('title', Array.isArray(controllers) ? `Certificate checked at ${item.checkedAt}` : item?.error || fallback);
      }
      const memoryById = new Map(observed.status === 'fulfilled'
        ? observed.value.map((item) => [formatPrincipal(item.canister_id), readOpt(item.total_memory_bytes)]) : []);
      for (const node of memories) {
        const value = memoryById.get(node.getAttribute('data-source-memory'));
        const valid = typeof value === 'bigint' && value >= 0n && value <= 18_446_744_073_709_551_615n;
        node.textContent = valid ? `${formatBytes(value)} total` : 'Unavailable';
        node.setAttribute('title', 'Informational Historian observation; not certificate-verified');
      }
    })();
    try { await request; } finally { request = null; }
  };
  return { ensureLoaded };
}
