# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is the public asset canister for the Jupiter Faucet Suite.

It serves the landing page as certified assets from a Rust asset canister. The browser-side dashboard logic uses generated declarations plus a browser-compatible `@icp-sdk/core/agent` transport to read live data directly from canisters.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `jufzc-caaaa-aaaar-qb5da-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Route and serving model

The canister serves two important HTTP surfaces:

- `/` (via `index.html` fallback)
  - the main landing page / dashboard shell
- `/metrics`
  - a small JSON endpoint served directly by the Rust canister with:
    - `num_assets`
    - `num_fallback_assets`
    - `cycle_balance`

Asset responses are certified and content-typed through the Rust asset router.

The `/metrics` endpoint is intentionally served with certification skipped by policy because it is a simple canister-local diagnostics surface rather than part of the browser dashboard data model.

The canister also applies a deliberate cache policy:

- `index.html`, `/.well-known/ic-domains`, and `/metrics` are served with `cache-control: public, no-cache, no-store`
- hashed JS / CSS assets are served with `cache-control: public, max-age=31536000, immutable`

Asset responses also carry the canister’s standard security headers, including HSTS, `X-Content-Type-Options: nosniff`, a restrictive Content Security Policy, and COEP / COOP / CORP headers.

## Frontend architecture

The frontend keeps the certified-asset Rust canister, but the in-page dashboard data path now follows the normal ICP browser path:

- declarations are generated from `dfx generate`
- the browser uses generated declarations with `@icp-sdk/core/agent`
- actors are created with `Actor.createActor(...)`
- dashboard data comes from Candid query calls, not from custom `/dashboard/*` JSON routes

The browser-side source lives under:

- `frontend-src/src/`
- `frontend-src/declarations/`

The bundled output is written to:

- `assets/generated/app.<content-hash>.js`
- `assets/generated/frontend-bundle.json`

## Data sources

The browser reads live data from three places:

### 1) `jupiter-historian`

Used for:

- `get_public_counts`
- `get_public_status`
- `list_registered_canister_summaries`
- `list_recent_contributions`
- `list_recent_burns`

These power the public dashboard tables and summary cards.

### 2) the configured ledger canister

The browser learns the staking account and ledger canister ID from `historian.get_public_status()`, then reads current stake from the ledger.

The loader attempts:

1. native `account_balance` against the account-identifier bytes
2. fallback `icrc1_balance_of(staking_account)` if native balance lookup fails

### 3) NNS Governance

The frontend also reads the Jupiter neuron directly from NNS Governance so it can show neuron metadata such as age, creation timestamp, refresh timestamp, and followees.

No browser requests are made to custom `/dashboard/*` routes.

## Build

`jupiter-faucet-frontend` keeps placeholder asset URLs / tokens in source and stamps them at build time.

Relevant placeholders and derived values:

- tracked token: `__ASSET_VERSION__`
- bundle token: `__APP_BUNDLE_PATH__`
- default rendered asset version: content-derived `frontend-<sha-prefix>` unless `ASSET_VERSION` is explicitly provided

The frontend bundle is built with a small Node / esbuild step before the Rust canister is compiled. The generated bundle files under `assets/generated/` are build artifacts and should not be committed.

Normal builds therefore go through the standard repository helper:

```bash
./scripts/build-canister jupiter-faucet-frontend
```

That step will:

1. install npm dependencies if needed
2. bundle `frontend-src/src/main.js` to a content-hashed file
3. stamp asset-version and bundle placeholders into the static assets
4. compile the Rust asset canister
5. restore the placeholder-stamped source assets after the build completes

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

`mock_icrc_ledger` is used only to generate the ledger actor surface the frontend needs; at runtime the actor is pointed at the ledger canister ID returned by historian status.

## Frontend-only tests

Browser/data-loader tests currently live in npm scripts rather than `xtask`:

```bash
npm run test:frontend-dashboard
npm run test:frontend-dashboard-local
```

There is also a small historian smoke helper:

```bash
npm run smoke:historian
```

## Upgrade command

```bash
dfx canister install jupiter_faucet_frontend \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```

## Related docs

- suite overview: [`../README.md`](../README.md)
- historian read model: [`../jupiter-historian/README.md`](../jupiter-historian/README.md)
- test harness: [`../xtask/README.md`](../xtask/README.md)
