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

The `/metrics` endpoint is also certified now, but as a periodically refreshed snapshot rather than an uncached live recomputation. The frontend canister refreshes and re-certifies the metrics document on a timer, so `/metrics` stays trustworthy without relying on uncertified boundary-node data for the JSON body.

The canister also applies a deliberate cache policy:

- `index.html`, `/.well-known/ic-domains`, and `/metrics` are served with `cache-control: public, no-cache, no-store`
- hashed JS / CSS assets are served with `cache-control: public, max-age=31536000, immutable`

Asset responses also carry the canister’s standard security headers, including HSTS, `X-Content-Type-Options: nosniff`, a restrictive Content Security Policy, and COEP / COOP / CORP headers.

## Frontend architecture

The frontend keeps the certified-asset Rust canister, but the in-page dashboard data path now follows the normal ICP browser path:

- declarations are generated from `dfx generate`
- the browser uses generated declarations with `@icp-sdk/core/agent`
- actors are created through the generated `createActor(...)` helpers
- dashboard data comes from Candid query calls, not from custom `/dashboard/*` JSON routes

The browser-side source lives under:

- `frontend-src/src/`
- `frontend-src/declarations/`

The bundled output is written to:

- `assets/generated/app.<content-hash>.js`
- `assets/generated/frontend-bundle.json`

### Runtime config and canister ID resolution

The browser bundle is built with a tiny runtime config object that currently carries:

- `network`
- `historianCanisterId`
- `frontendCanisterId`

During the build, those values are resolved in this order:

1. explicit `CANISTER_ID_<NAME>` environment variables
2. `canister_ids.json` for the selected network
3. fallback `ic` / `local` entries when present

The selected network comes from `DFX_NETWORK`, then `JUPITER_FRONTEND_NETWORK`, and otherwise defaults to `ic`.

## Data sources

The browser reads live data from three places:

### 1) `jupiter-historian`

Used for:

- `get_public_counts`
- `get_public_status`
- `list_registered_canister_summaries`
- `list_recent_commitments`

These power the registry table, commitment feed, and historian-backed status surface.

### 2) the configured ledger canister

The browser learns the staking account and ledger canister ID from `historian.get_public_status()`, then reads current stake from the ledger.

The loader attempts:

1. native `account_balance` against the account-identifier bytes
2. fallback `icrc1_balance_of(staking_account)` if native balance lookup fails

### 3) NNS Governance

The frontend also reads the Jupiter neuron directly from NNS Governance so it can show neuron metadata such as age, creation timestamp, refresh timestamp, and followees.

No browser requests are made to custom `/dashboard/*` routes.

### Dashboard loader behavior

The browser data loader is intentionally defensive:

- it fetches historian counts, status, registered-canister summaries, and recent commitments together, then uses historian status to discover the ledger canister id for stake
- the displayed historian history tables are intentionally bounded views and now also show the historian canister's current allocated memory footprint; the tracked target-canister registry is not pruned, and full transfer history still lives on the ICP ledger and its archive canisters
- invalid memo text is not echoed back in the dashboard tables; invalid entries render as a generic placeholder instead
- it only instantiates the ledger actor after historian status reveals the staking account and ledger canister ID
- it preserves partial success, so one failed dashboard query does not blank the whole page
- it explicitly detects the "all requested historian methods are missing" shape and flags that as a likely outdated historian deployment

That behavior matters in practice because the frontend is expected to keep rendering as much live state as it can even during partial upgrades or mismatched deployments.

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

The checked-in historian declarations under `frontend-src/declarations/jupiter_historian/` can be regenerated with:

```bash
dfx generate jupiter_historian
```

The relevant `declarations` output paths are configured in `dfx.json`.

`frontend-src/declarations/icp_ledger/` is a checked-in declaration subset for the production ICP ledger methods the frontend uses. It is intentionally kept separate from any mock canister declarations; at runtime the actor is pointed at the ledger canister ID returned by historian status.

## Frontend-only tests

These browser/data-loader checks can now be driven through `xtask` as well:

```bash
cargo run -p xtask -- frontend_setup
cargo run -p xtask -- frontend_unit
cargo run -p xtask -- frontend_dfx_integration
cargo run -p xtask -- frontend_all
```

The direct npm entry points remain available:

```bash
npm run setup:frontend
npm run test:frontend-dashboard
npm run test:frontend-neuron
npm run test:frontend-unit
npm run test:frontend-dashboard-local
```

The checked-in Node tests cover the highest-value browser-side invariants, including:

- stable internal account-identifier derivation for ledger/index reads while displaying the ICRC-1 staking account address to users
- the exact historian query shapes and limits the dashboard loader issues
- native-ledger balance reads with `icrc1_balance_of` fallback
- graceful handling of zero-valued metrics
- detection of an outdated historian interface when every required public method is missing
- stale neuron-detail responses being ignored after a controller reset
- DOM helper cleanup when a previously available link becomes unavailable

The local-replica variant is fixture-driven: it compares the live loader result against an expected JSON snapshot provided through environment variables.


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
