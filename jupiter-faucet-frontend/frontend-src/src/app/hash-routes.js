export const TRACKER_HASH_PREFIX = '#metric-tracker-';
export const TRACKER_QUERY_HASH_PREFIX = '#metric-tracker';
export const SIMULATOR_HASH_PREFIX = '#simulator-';

export function trackerHashForMemo({ memo = '', protocolCanister = '' } = {}) {
  const text = String(memo || '').trim();
  if (!text) return '#metric-tracker';
  const params = new URLSearchParams();
  params.set('memo', text);
  if (protocolCanister) params.set('protocol-canister', String(protocolCanister).trim());
  return `${TRACKER_QUERY_HASH_PREFIX}?${params.toString()}`;
}

export function trackerHashForPrincipal(principalText) {
  const text = String(principalText || '').trim();
  return text ? `${TRACKER_HASH_PREFIX}${encodeURIComponent(text)}` : '#metric-tracker';
}

export function trackerStateFromHash(hash = window.location.hash) {
  const fragment = String(hash || '');
  if (fragment.startsWith(TRACKER_HASH_PREFIX)) {
    const legacyPrincipal = trackerPrincipalFromHash(fragment);
    return { memo: legacyPrincipal, protocolCanister: '', legacyPrincipal };
  }
  if (!fragment.startsWith(`${TRACKER_QUERY_HASH_PREFIX}?`)) {
    return { memo: '', protocolCanister: '', legacyPrincipal: '' };
  }
  const params = new URLSearchParams(fragment.slice(`${TRACKER_QUERY_HASH_PREFIX}?`.length));
  return {
    memo: params.get('memo') || '',
    protocolCanister: params.get('protocol-canister') || '',
    legacyPrincipal: '',
  };
}

export function trackerPrincipalFromHash(hash = window.location.hash) {
  const fragment = String(hash || '');
  if (!fragment.startsWith(TRACKER_HASH_PREFIX)) return '';
  try {
    return decodeURIComponent(fragment.slice(TRACKER_HASH_PREFIX.length)).trim();
  } catch {
    return fragment.slice(TRACKER_HASH_PREFIX.length).trim();
  }
}

export function simulatorHashForPrefill({
  dailyBurn = '',
  icpCommitment = '',
  assumedIcpPrice = '',
  annualApyPercent = '',
} = {}) {
  const params = new URLSearchParams();
  if (dailyBurn) params.set('burn', String(dailyBurn));
  if (icpCommitment) params.set('commitment', String(icpCommitment));
  if (assumedIcpPrice) params.set('price', String(assumedIcpPrice));
  if (annualApyPercent) params.set('apy', String(annualApyPercent));
  const encoded = params.toString();
  return encoded ? `${SIMULATOR_HASH_PREFIX}${encoded}` : '#simulator';
}

export function simulatorPrefillFromHash(hash = window.location.hash) {
  const fragment = String(hash || '');
  if (!fragment.startsWith(SIMULATOR_HASH_PREFIX)) return null;
  const params = new URLSearchParams(fragment.slice(SIMULATOR_HASH_PREFIX.length));
  return {
    dailyBurn: params.get('burn') || '',
    icpCommitment: params.get('commitment') || '',
    assumedIcpPrice: params.get('price') || '',
    annualApyPercent: params.get('apy') || '',
  };
}
