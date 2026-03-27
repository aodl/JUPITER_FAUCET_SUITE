# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is the public asset canister for the Jupiter Faucet Suite.

It serves the landing page and a lightweight live-metrics pointer page as certified assets from the Rust asset canister, while the browser-side dashboard logic now uses generated declarations and a browser-compatible `@icp-sdk/core/agent` transport.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `jufzc-caaaa-aaaar-qb5da-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Frontend architecture

The frontend keeps the existing certified-asset Rust canister, but the live metrics implementation now follows the normal ICP browser path:

- declarations are generated from `dfx generate`
- the browser uses generated declarations with `@icp-sdk/core/agent`
- actors are created with `Actor.createActor(...)`
- live metrics come from Candid query calls, not from custom `http_request` JSON routes

The browser-side source lives under:

- `frontend-src/src/`
- `frontend-src/declarations/`

The bundled output is written to:

- `assets/generated/app.<content-hash>.js`

## Data sources

The frontend reads:

- **current Jupiter stake** from the configured ledger canister via `icrc1_balance_of(staking_account)`
- **historian public counts/status** from `jupiter-historian`
- **registered canister summaries** from `jupiter-historian`
- **recent contributions** from `jupiter-historian`

No browser requests are made to custom `/dashboard/*` routes.

## Build

`jupiter-faucet-frontend` keeps placeholder asset URLs in source and stamps them at build time.

- tracked token: `__ASSET_VERSION__`
- default rendered version: `YYYY-MM-DD-<git short sha>` derived from `SOURCE_DATE_EPOCH` and the current commit
- optional override: set `ASSET_VERSION` before invoking the build script

The frontend bundle is built with a small Node/esbuild step before the Rust canister is compiled. The generated bundle files under `assets/generated/` are build artifacts and should not be committed.

Normal frontend builds therefore go through the standard repository helper:

```bash
./scripts/build-canister jupiter-faucet-frontend
```

That step will:

1. install npm dependencies if needed
2. bundle `frontend-src/src/main.js` to a content-hashed file
3. stamp asset-version placeholders into the static HTML/CSS/JS references
4. compile the Rust asset canister

Example asset-version override:

```bash
ASSET_VERSION=2026-03-19 ./scripts/build-canister jupiter-faucet-frontend
```

## Regenerating declarations

The checked-in declarations under `frontend-src/declarations/` are intended to match the outputs from:

```bash
dfx generate jupiter_historian
dfx generate mock_icrc_ledger
```

The relevant `declarations` output paths are configured in `dfx.json`.

`mock_icrc_ledger` is used only to generate the ICRC-1 balance actor surface the frontend needs; at runtime the actor is pointed at the ledger canister ID returned by historian status.

## Upgrade command

```bash
dfx canister install jupiter_faucet_frontend \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```
