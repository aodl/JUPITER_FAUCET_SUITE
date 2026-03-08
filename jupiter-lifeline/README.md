# Jupiter Lifeline

`jupiter-lifeline` is the recovery controller target for blackholed Jupiter canisters.

## Current mainnet canister

- canister id: `afisn-gqaaa-aaaar-qb4qa-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Purpose

The canister is intentionally minimal. In steady state it only logs its cycle balance every 20 days.

If a lifeline event occurs, the expected response is to inspect the specific failure mode and upgrade `jupiter-lifeline` with targeted recovery logic for that incident.

## Upgrade command

No install or upgrade argument is currently required.

```bash
dfx canister install jupiter_lifeline   --network ic   --mode upgrade   --wasm release-artifacts/jupiter_lifeline.wasm.gz
```
