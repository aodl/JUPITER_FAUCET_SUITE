export const GOVERNANCE_CANISTER_ID = 'rrkah-fqaaa-aaaaa-aaaaq-cai';
export const JUPITER_NEURON_ID = 11614578985374291210n;
export const MAINNET_CMC_CANISTER_ID = 'rkp4c-7iaaa-aaaaa-aaaca-cai';
export const JUPITER_RELAY_CANISTER_ID = 'u2qkp-aqaaa-aaaar-qb7ea-cai';
export const DQUORUM_STAKING_ACCOUNT_SUBACCOUNT_HEX = '77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f';
export const SOURCE_PANE_CACHE_TTL_MS = 60 * 60 * 1000;

export const JUPITER_STAKING_ACCOUNT = Object.freeze({
  address: 'rrkah-fqaaa-aaaaa-aaaaq-cai-h7evq5y.ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68',
  explorerAccountHex: '22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d',
  owner: GOVERNANCE_CANISTER_ID,
  subaccountHex: 'ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68',
});

export const SIMULATOR_DEFAULTS = Object.freeze({
  dailyBurnTrillionCycles: '0.0001',
  assumedIcpPrice: '10.0',
  annualApyPercent: '7.0',
});
