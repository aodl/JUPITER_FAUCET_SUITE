import { Principal } from '@icp-sdk/core/principal';
import {
  accountIdentifierHex,
  bytesToHex,
  icrcAccountText,
} from './dashboard-transforms.js';

const SPLITTER_PERCENTAGES = [10, 20, 30, 40, 50, 60, 70, 80, 90];

export const RELAY_FUNDING_DESTINATIONS = [
  {
    subaccountNumber: null,
    name: 'Default / subaccount 0',
    purpose: 'Liquid raw ICP for later burn-responsive cycles allocation and configured surplus routing.',
  },
  {
    subaccountNumber: 1,
    name: 'Subaccount 1 · endowment staging',
    purpose: 'Accumulates ICP for a Faucet endowment whose future raw-ICP payouts return to this Relay’s default account.',
  },
  ...SPLITTER_PERCENTAGES.map((percentage) => ({
    subaccountNumber: percentage,
    name: `Subaccount ${percentage} · ${percentage}/${100 - percentage} splitter`,
    purpose: `Gross ${percentage}% budget to default and ${100 - percentage}% to endowment staging; each initial leg pays one ledger fee.`,
  })),
];

export function relayFundingSubaccount(subaccountNumber) {
  const bytes = new Uint8Array(32);
  if (subaccountNumber !== null) bytes[31] = subaccountNumber;
  return bytes;
}

export function deriveRelayFundingDestinations(relayCanisterId) {
  const owner = typeof relayCanisterId === 'string'
    ? Principal.fromText(relayCanisterId)
    : relayCanisterId;
  return RELAY_FUNDING_DESTINATIONS.map((destination) => {
    const subaccount = relayFundingSubaccount(destination.subaccountNumber);
    const account = {
      owner,
      subaccount: destination.subaccountNumber === null ? [] : [Array.from(subaccount)],
    };
    return {
      ...destination,
      owner,
      account,
      icrcAccountText: icrcAccountText(account),
      legacyAccountIdentifier: accountIdentifierHex(account),
      subaccountHex: bytesToHex(subaccount),
      semanticSubaccount: destination.subaccountNumber === null
        ? 'null (all-zero equivalent)'
        : String(destination.subaccountNumber),
    };
  });
}

export function relayMemoBuilderHash(relayCanisterId) {
  const canister = typeof relayCanisterId === 'string'
    ? Principal.fromText(relayCanisterId).toText()
    : relayCanisterId.toText();
  const params = new URLSearchParams({
    canister,
    mode: 'rawIcp',
    title: 'Relay Canister',
    label: 'Optional Donor Name',
  });
  return `#memo-builder?${params}`;
}
