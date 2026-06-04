# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is the public asset canister for the Jupiter Faucet Suite.

It serves the landing page as certified assets from a Rust asset canister. The browser-side dashboard logic uses generated declarations plus a browser-compatible `@icp-sdk/core/agent` transport to read live data directly from canisters.

See the suite overview in [`../../README.md`](../../README.md).

Unless otherwise noted, command examples in this README are run from the repository root.

## Current mainnet canister recorded in this repo

- canister ID: `jufzc-caaaa-aaaar-qb5da-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Route and serving model

The canister serves two important HTTP surfaces:

- `/` (via the `index.html` alias)
  - the main landing page / dashboard shell
- unknown asset/page paths
  - a certified `404.html` fallback served with HTTP 404

Asset responses are certified and content-typed through the Rust asset router.

The frontend canister intentionally does not expose a public metrics endpoint. Live cycle and accounting data should come from the suite's existing controller, blackhole, and status monitoring paths.

The canister also applies a deliberate cache policy:

- `index.html` and `/.well-known/ic-domains` are served with `cache-control: public, no-cache, no-store`
- hashed JS / CSS assets are served with `cache-control: public, max-age=31536000, immutable`

Asset responses also carry the canister’s standard security headers, including HSTS, `X-Content-Type-Options: nosniff`, a restrictive Content Security Policy, and COEP / COOP / CORP headers.

## Frontend architecture

The frontend keeps the certified-asset Rust canister, and the in-page dashboard data path follows the normal ICP browser path:

- declarations are checked in under `web/declarations/`
- the browser uses generated declarations with `@icp-sdk/core/agent`
- actors are created through the generated `createActor(...)` helpers
- dashboard data comes from Candid query calls, not from custom `/dashboard/*` JSON routes

The browser-side source lives under:

- `web/src/`
- `web/declarations/`

The bundled output is written to:

- `public/generated/app.<content-hash>.js`
- `public/generated/frontend-bundle.json`

Only the content-hashed bundle is a public asset. `frontend-bundle.json` is a build-time manifest used to stamp `index.html`, and the Rust asset canister deliberately excludes it from certified/routable assets.

### Runtime config and canister ID resolution

The browser bundle is built with a tiny runtime config object that carries:

- `network`
- `historianCanisterId`
- `frontendCanisterId`

During the build, those values are resolved in this order:

1. explicit `CANISTER_ID_<NAME>` environment variables
2. `.icp/data/mappings/<network>.ids.json` for mainnet-style networks
3. `.icp/cache/mappings/local.ids.json` for local builds

The selected network comes from `JUPITER_FRONTEND_NETWORK`, then `ICP_ENVIRONMENT`, then `ICP_NETWORK`, and otherwise defaults to `ic`.

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
- the displayed historian history tables are intentionally bounded views and also show the historian canister's current allocated memory footprint; the tracked target-canister registry is not pruned, and full transfer history still lives on the ICP ledger and its archive canisters
- the source/governance verification view includes relay source/module metadata even though `jupiter-relay` has no public production app API
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

The frontend bundle is built with a small Node / esbuild step before the Rust canister is compiled. The generated bundle files under `public/generated/` are build artifacts and should not be committed.

Normal builds therefore go through the standard repository helper:

```bash
./tools/scripts/build-canister jupiter-faucet-frontend
```

That step will:

1. install npm dependencies if needed
2. bundle `web/src/main.js` to a content-hashed file
3. stamp asset-version and bundle placeholders into the static assets
4. compile the Rust asset canister
5. restore the placeholder-stamped source assets after the build completes

Example asset-version override:

```bash
ASSET_VERSION=2026-03-19 ./tools/scripts/build-canister jupiter-faucet-frontend
```

## Security header notes

The frontend CSP keeps `frame-ancestors 'self' https://jupiter-faucet.com https://www.jupiter-faucet.com`. These origins are intentional for the deployment model so the custom domain and its `www` host can frame same-site frontend content when needed. Any future tightening of this directive should be handled as a separate policy decision.

Inline JavaScript and inline CSS are not allowed by the frontend CSP. Page and fallback styles live in static stylesheet assets so the policy can keep `script-src 'self'`, `style-src 'self'`, and `style-src-attr 'none'`.

## Declarations

The checked-in historian declarations under `web/declarations/jupiter_historian/` are the browser-facing Candid bindings used by the bundle.

`web/declarations/icp_ledger/` is a checked-in declaration subset for the production ICP ledger methods the frontend uses. It is intentionally kept separate from any mock canister declarations; at runtime the actor is pointed at the ledger canister ID returned by historian status.

## Frontend-only tests

These browser/data-loader checks can be driven through `xtask` as well:

```bash
cargo run -p xtask -- frontend_setup
cargo run -p xtask -- frontend_unit
cargo run -p xtask -- frontend_local_integration
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
- DOM helper cleanup when an available link becomes unavailable

The local-replica variant is fixture-driven: it compares the live loader result against an expected JSON snapshot provided through environment variables.


## Upgrade command

```bash
icp canister install jupiter_faucet_frontend \
  --environment ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```

## Related docs

- suite overview: [`../../README.md`](../../README.md)
- historian read model: [`../historian/README.md`](../historian/README.md)
- test harness: [`../../tools/xtask/README.md`](../../tools/xtask/README.md)
