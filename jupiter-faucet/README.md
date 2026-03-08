# Jupiter Faucet

`jupiter-faucet` is currently deployed as a placeholder canister.

## Current mainnet canister

- canister id: `acjuz-liaaa-aaaar-qb4qq-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

This canister is the configured normal recipient for `jupiter-disburser`.

The production intent is for `jupiter-faucet` to receive the age-neutral ICP flow, convert it into cycles, and top up participating canisters proportionally to the stake commitments encoded upstream.

Like `jupiter-disburser`, `jupiter-faucet` is intended to be blackholed once deployment has been validated. It will use the same `jupiter-lifeline` recovery pattern.

## Current implementation

The current implementation is intentionally minimal. It exists to reserve the canister principal and default account while the production faucet logic is built.

## Upgrade command

No install or upgrade argument is currently required.

```bash
dfx canister install jupiter_faucet   --network ic   --mode upgrade   --wasm release-artifacts/jupiter_faucet.wasm.gz
```
