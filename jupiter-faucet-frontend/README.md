# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is the frontend/assets canister slot in the Jupiter Faucet Suite.

Its present-day purpose is simply to reserve the deployment slot for the eventual public-facing asset canister.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `gvey7-gyaaa-aaaar-qb4fq-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Current implementation

The implementation is intentionally minimal:

- no frontend assets yet
- no business logic
- no install or upgrade args

## Intended future role

The intended production role is to host the public frontend and asset bundle for the Jupiter Faucet system.

That frontend is not implemented in this repository yet.

## Upgrade command

```bash
dfx canister install jupiter_faucet_frontend \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```
