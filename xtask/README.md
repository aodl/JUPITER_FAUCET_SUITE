# xtask

`xtask` is the preferred entry point for local orchestration and test execution in this repository.

It wraps three different layers of validation:

- fast Rust unit tests
- local `dfx` integration scenarios against debug canisters and mocks
- PocketIC integration / end-to-end suites

Using `xtask` keeps the command surface stable and avoids having to remember which mocks, features, identities, and ignored tests need to be wired together manually.

## What `xtask` does not cover

`xtask` is focused on Rust / canister / PocketIC / local-`dfx` orchestration.

Browser-only frontend tests still live in npm scripts:

```bash
npm run test:frontend-dashboard
npm run test:frontend-dashboard-local
```

## Prerequisites

Commonly useful tools for `xtask` workflows are:

- Rust / Cargo
- `dfx`
- Node.js / npm for frontend-specific scripts outside `xtask`
- `make` and `nix-build` for historian and suite-level PocketIC paths that build the vendored real blackhole canister reproducibly

## What `setup` does

```bash
cargo run -p xtask -- setup
```

`setup` prepares the local `dfx` environment by:

- creating a dedicated non-interactive identity named `xtask-dev` if needed
- starting the local replica
- deploying the mock canisters used by the integration scenarios:
  - `mock_icrc_ledger`
  - `mock_nns_governance`
  - `mock_icp_index`
  - `mock_cmc`
  - `mock_blackhole`
  - `mock_sns_wasm`
  - `mock_sns_root`
- deploying the debug builds of:
  - `jupiter_disburser_dbg`
  - `jupiter_faucet_dbg`
  - `jupiter_historian_dbg`
- wiring those debug canisters to the local mocks with the expected init args
- adding each debug canister as a controller of itself so local controller-transition tests can run cleanly

The deployed debug canisters intentionally use long timer intervals in most local scenarios so tests can drive the runtime explicitly through debug methods instead of waiting for ambient timers.

In normal use you usually do **not** need to call `setup` directly because the scoped `dfx` commands do it automatically.

## What `teardown` does

```bash
cargo run -p xtask -- teardown
```

`teardown` stops the local `dfx` environment created for xtask-driven scenarios.

## Command matrix

### Utility commands

```bash
cargo run -p xtask -- setup
cargo run -p xtask -- teardown
```

### Disburser commands

```bash
cargo run -p xtask -- disburser_unit
cargo run -p xtask -- disburser_dfx_integration
cargo run -p xtask -- disburser_pocketic_integration
cargo run -p xtask -- disburser_all
```

### Faucet commands

```bash
cargo run -p xtask -- faucet_unit
cargo run -p xtask -- faucet_dfx_integration
cargo run -p xtask -- faucet_pocketic_integration
cargo run -p xtask -- faucet_all
```

### Historian commands

```bash
cargo run -p xtask -- historian_unit
cargo run -p xtask -- historian_dfx_integration
cargo run -p xtask -- historian_pocketic_integration
cargo run -p xtask -- historian_all
```

### End-to-end commands

```bash
cargo run -p xtask -- e2e_pocketic_integration
cargo run -p xtask -- e2e_all
```

There is intentionally **no** `e2e_dfx_integration` command.

### Whole-suite commands

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_dfx_integration
cargo run -p xtask -- test_pocketic_integration
cargo run -p xtask -- test_all
```

## What each layer means

### `*_unit`

Runs the regular Rust unit tests for the relevant crate(s).

Use this when iterating on pure logic and state transitions.

### `*_dfx_integration`

Runs scenario-based integration checks against the local `dfx` replica using the mock canisters declared in `dfx.json`.

These scenarios are especially useful for validating:

- install-time configuration wiring
- local debug endpoints
- ledger / governance / index / CMC interactions through mocks
- controller-change behavior against a local management canister

Examples currently covered include:

- disburser in-flight disbursement no-op behavior
- disburser bonus split math and payout-plan rebuild on `BadFee`
- faucet full-history replay on each new payout job
- faucet notify retry without duplicate transfer
- faucet rescue invariants before first success and after broken / healthy controller transitions
- historian indexing, burn tracking, SNS discovery, and frontend-facing public-read-model behavior

### `*_pocketic_integration`

Runs the heavier PocketIC suites.

These exercise real canister execution more deeply and are where the repo currently validates many of its strongest behavioral guarantees.

The heavier suites live under `xtask/src/pocketIC/`:

- `jupiter_disburser_integration.rs`
- `jupiter_faucet_integration.rs`
- `jupiter_historian_integration.rs`
- `e2e.rs`

The mock canisters used by the local-`dfx` scenarios live under `xtask/src/mocks/`.

Examples covered by the current PocketIC suites include:

- disburser upgrade mid-flight preserving state
- duplicate-proof transfer completion after partial execution
- blackhole timer-only progression
- blackhole / rescue-controller round-trips
- age-bonus behavior at multiple neuron ages
- faucet retry persistence across upgrades
- bounded faucet state footprint under repeated replays
- forced rescue latching for index-anchor, latest-tx invariant, and zero-success CMC runs
- historian public-read-model assertions for:
  - `get_public_counts`
  - `get_public_status`
  - `list_registered_canister_summaries`
  - `list_recent_contributions`
  - `list_recent_burns`

### `e2e_pocketic_integration`

Runs the suite-level PocketIC end-to-end tests across the disburser and faucet together.

The current E2E coverage includes:

- disburser paying faucet and faucet topping up a target canister
- repeated disburser payouts feeding faucet full-history replay
- retry safety across the disburser → faucet → CMC boundary
- faucet upgrade during retry-state recovery

## Reproducible blackhole requirement in historian / E2E suites

Historian and E2E PocketIC coverage build the vendored `third_party/ic-blackhole` source through its pinned reproducible build path:

```bash
cd third_party/ic-blackhole
make repro-build
```

So those commands require `make` and `nix-build` to be available.

If you want to exercise the reproducibility / hash check directly, the historian PocketIC suite runs the ignored real-blackhole verification test via:

```bash
cargo test -p jupiter-historian --test jupiter_historian_integration -- --ignored --nocapture
```

## Recommended usage

### Fast pre-commit pass

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_dfx_integration
```

### Strong local confidence pass

```bash
cargo run -p xtask -- test_all
```

### When working on only one component

```bash
cargo run -p xtask -- disburser_all
cargo run -p xtask -- faucet_all
cargo run -p xtask -- historian_all
```

## Relationship to raw `cargo test`

You can still run crate-level tests directly, for example:

```bash
cargo test -p jupiter-disburser --lib
cargo test -p jupiter-faucet --lib
cargo test -p jupiter-historian --lib
```

But once you need mocks, debug canisters, local replica setup, or the ignored PocketIC suites, `xtask` is the intended abstraction.

## Related docs

- suite overview: [`../README.md`](../README.md)
- disburser details: [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md)
- faucet details: [`../jupiter-faucet/README.md`](../jupiter-faucet/README.md)
- historian details: [`../jupiter-historian/README.md`](../jupiter-historian/README.md)
- frontend details: [`../jupiter-faucet-frontend/README.md`](../jupiter-faucet-frontend/README.md)
