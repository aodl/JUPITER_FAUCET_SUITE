export const TRACKER_HASH_PREFIX = '#metric-tracker-';
export const TRACKER_QUERY_HASH_PREFIX = '#metric-tracker';
export const SIMULATOR_HASH_PREFIX = '#simulator-';

export function normalizeTrackerRange(range) {
  const text = String(range || '').trim().toLowerCase();
  return text === 'year' || text === 'all' ? text : 'month';
}

function appendTrackerRange(params, range) {
  if (range === undefined || range === null || range === '') return;
  params.set('range', normalizeTrackerRange(range));
}

function trackerRangeSuffix(range) {
  const params = new URLSearchParams();
  appendTrackerRange(params, range);
  const query = params.toString();
  return query ? `?${query}` : '';
}

export function trackerHashForMemo({ memo = '', protocolCanister = '', range = '' } = {}) {
  const text = String(memo || '').trim();
  const params = new URLSearchParams();
  if (text) params.set('memo', text);
  if (protocolCanister) params.set('protocol-canister', String(protocolCanister).trim());
  appendTrackerRange(params, range);
  const query = params.toString();
  return query ? `${TRACKER_QUERY_HASH_PREFIX}?${query}` : TRACKER_QUERY_HASH_PREFIX;
}

export function trackerHashForPrincipal(principalText, { range = '' } = {}) {
  const text = String(principalText || '').trim();
  return text ? `${TRACKER_HASH_PREFIX}${encodeURIComponent(text)}${trackerRangeSuffix(range)}` : trackerHashForMemo({ range });
}

export function trackerStateFromHash(hash = window.location.hash) {
  const fragment = String(hash || '');
  if (fragment.startsWith(TRACKER_HASH_PREFIX)) {
    const queryStart = fragment.indexOf('?');
    const params = queryStart >= 0 ? new URLSearchParams(fragment.slice(queryStart + 1)) : new URLSearchParams();
    const legacyPrincipal = trackerPrincipalFromHash(queryStart >= 0 ? fragment.slice(0, queryStart) : fragment);
    return {
      memo: legacyPrincipal,
      protocolCanister: '',
      legacyPrincipal,
      range: normalizeTrackerRange(params.get('range')),
    };
  }
  if (!fragment.startsWith(`${TRACKER_QUERY_HASH_PREFIX}?`)) {
    return { memo: '', protocolCanister: '', legacyPrincipal: '', range: 'month' };
  }
  const params = new URLSearchParams(fragment.slice(`${TRACKER_QUERY_HASH_PREFIX}?`.length));
  return {
    memo: params.get('memo') || '',
    protocolCanister: params.get('protocol-canister') || '',
    legacyPrincipal: '',
    range: normalizeTrackerRange(params.get('range')),
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
