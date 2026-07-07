import { Principal } from '@icp-sdk/core/principal';
import { JUPITER_RELAY_CANISTER_ID } from '../app/config.js';
import { accountIdentifierHex, readOptional } from './dashboard-transforms.js';

function normalizeIdentifier(value) {
  return String(value || '').trim().toLowerCase();
}

export function defaultCanisterAccountIdentifier(canisterId) {
  if (!canisterId) return '';
  const owner = typeof canisterId === 'string' ? Principal.fromText(canisterId) : canisterId;
  return accountIdentifierHex({ owner, subaccount: [] }).toLowerCase();
}

export function relayRegistrySourceMap(relayRegistrations = []) {
  const map = new Map();
  for (const entry of relayRegistrations || []) {
    const relayCanisterId = readOptional(entry?.relay_canister_id) || entry?.relay_canister_id;
    if (!relayCanisterId) continue;
    const relayText = typeof relayCanisterId.toText === 'function' ? relayCanisterId.toText() : String(relayCanisterId);
    map.set(defaultCanisterAccountIdentifier(relayText), {
      entry,
      relayCanisterId: relayText,
      label: `Relay ${relayText.slice(0, 5)}…`,
    });
  }
  return map;
}

export function classifyTransferSource({
  fromAccountIdentifier,
  status,
  relayCanisterId = JUPITER_RELAY_CANISTER_ID,
  relaySourceMap = null,
  protocolCanisterId = null,
} = {}) {
  const from = normalizeIdentifier(fromAccountIdentifier);
  if (!from) return 'other';

  const faucetAccount = readOptional(status?.output_account);
  if (faucetAccount && from === accountIdentifierHex(faucetAccount).toLowerCase()) return 'faucet';

  if (relaySourceMap?.has(from)) return 'relay';
  if (relayCanisterId && from === defaultCanisterAccountIdentifier(relayCanisterId)) return 'relay';
  if (protocolCanisterId && from === defaultCanisterAccountIdentifier(protocolCanisterId)) return 'protocol';
  return 'other';
}

export function classifyTransferItem(item, options = {}) {
  const from = normalizeIdentifier(item?.from_account_identifier);
  const relayMatch = options.relaySourceMap?.get(from) || null;
  const sourceCategory = classifyTransferSource({
    ...options,
    fromAccountIdentifier: item?.from_account_identifier,
  });
  return {
    ...item,
    source_category: sourceCategory,
    ...(sourceCategory === 'relay' && relayMatch ? {
      source_relay_canister_id: relayMatch.relayCanisterId,
      source_label: relayMatch.label,
    } : {}),
  };
}
