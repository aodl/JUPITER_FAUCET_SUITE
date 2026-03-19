# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is the public asset canister for the Jupiter Faucet Suite.

It now contains the migrated frontend source from the legacy standalone frontend repo and serves the site as a certified-asset Rust canister.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `jufzc-caaaa-aaaar-qb5da-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Implementation notes

- frontend assets are embedded into the canister WASM under `assets/`
- assets are served with HTTP certification via vendored response-verification support crates under `third_party/response-verification/`
- `/metrics` returns a small uncertified JSON payload with the asset counts and current cycle balance
- the public service interface is declared in `jupiter_faucet_frontend.did`

## Build

`jupiter-faucet-frontend` keeps placeholder asset URLs in source and stamps them at build time.

- tracked token: `__ASSET_VERSION__`
- default rendered version: `YYYY-MM-DD-<git short sha>` derived from `SOURCE_DATE_EPOCH` and the current commit
- optional override: set `ASSET_VERSION` before invoking the build script

Normal canister builds for `jupiter_faucet_frontend` go through `./scripts/build-canister`, including `dfx build` and the reproducible Docker build path, so the asset-version stamping is part of the standard build flow.

```bash
./scripts/build-canister jupiter-faucet-frontend
```

Example override:

```bash
ASSET_VERSION=2026-03-19 ./scripts/build-canister jupiter-faucet-frontend
```

## Upgrade command

```bash
dfx canister install jupiter_faucet_frontend \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```

