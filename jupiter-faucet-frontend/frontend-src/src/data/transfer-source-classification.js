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

export function classifyTransferSource({
  fromAccountIdentifier,
  status,
  relayCanisterId = JUPITER_RELAY_CANISTER_ID,
  protocolCanisterId = null,
} = {}) {
  const from = normalizeIdentifier(fromAccountIdentifier);
  if (!from) return 'other';

  const faucetAccount = readOptional(status?.output_account);
  if (faucetAccount && from === accountIdentifierHex(faucetAccount).toLowerCase()) return 'faucet';

  if (relayCanisterId && from === defaultCanisterAccountIdentifier(relayCanisterId)) return 'relay';
  if (protocolCanisterId && from === defaultCanisterAccountIdentifier(protocolCanisterId)) return 'protocol';
  return 'other';
}

export function classifyTransferItem(item, options = {}) {
  return {
    ...item,
    source_category: classifyTransferSource({
      ...options,
      fromAccountIdentifier: item?.from_account_identifier,
    }),
  };
}
