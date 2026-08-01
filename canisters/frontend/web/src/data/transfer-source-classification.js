import { Principal } from '@icp-sdk/core/principal';
import { JUPITER_RELAY_CANISTER_ID } from '../app/config.js';
import { accountIdentifierHex, readOptional } from './dashboard-transforms.js';

function normalizeIdentifier(value) {
  return String(value || '').trim().toLowerCase();
}

function canisterIdText(value) {
  if (!value) return '';
  return typeof value.toText === 'function' ? value.toText() : String(value);
}

export function defaultCanisterAccountIdentifier(canisterId) {
  if (!canisterId) return '';
  const owner = typeof canisterId === 'string' ? Principal.fromText(canisterId) : canisterId;
  return accountIdentifierHex({ owner, subaccount: [] }).toLowerCase();
}

export function relayInstanceSourceMap(trackedCanisters = []) {
  const map = new Map();
  for (const entry of trackedCanisters || []) {
    const relayCanisterId = entry?.canister_id;
    if (!relayCanisterId) continue;
    const relayText = canisterIdText(relayCanisterId);
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
  const relayCanisterId = sourceCategory === 'relay'
    ? relayMatch?.relayCanisterId || canisterIdText(options.relayCanisterId ?? JUPITER_RELAY_CANISTER_ID)
    : '';
  return {
    ...item,
    source_category: sourceCategory,
    ...(relayCanisterId ? {
      source_relay_canister_id: relayCanisterId,
      source_label: relayMatch?.label || `Relay ${relayCanisterId.slice(0, 5)}…`,
    } : {}),
  };
}
