# xtask

`xtask` is the preferred entry point for local orchestration and test execution in this repository.

It wraps three different layers of validation:

- fast Rust unit tests
- local `dfx` integration scenarios against debug canisters and mocks
- PocketIC integration / end-to-end suites

Using `xtask` keeps the command surface stable and avoids having to remember which mocks, features, identities, and ignored tests need to be wired together manually.

## What `setup` does

```bash
cargo run -p xtask -- setup
```

`setup` prepares the local `dfx` environment by:

- creating a dedicated non-interactive identity named `xtask-dev` if needed
- starting the local replica
- deploying the mock canisters used by the integration scenarios
- deploying the debug builds of the Jupiter canisters with the expected local arguments

In normal use you usually do **not** need to call `setup` directly because scoped `dfx` commands do it automatically.

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
- ledger/governance/index/CMC interactions through mocks
- controller-change behavior against a local management canister

For `disburser_dfx_integration`, the scenario set currently covers behavior such as:

- in-flight disbursement no-op behavior
- bonus split math on the happy path
- payout-plan preservation across temporary failures
- plan rebuild on `BadFee`
- rescue-controller invariants
- dust remaining in staging

For `faucet_dfx_integration`, the scenario set currently covers behavior such as:

- same-beneficiary contributions staying separate
- full-history replay on each new payout job
- page-boundary scanning across large histories
- notify retry without duplicate transfer
- remainder-to-self behavior
- rescue invariants before first success and after broken/healthy transitions

### `*_pocketic_integration`

Runs the heavier PocketIC suites.

These exercise real canister execution more deeply and are where the repo currently validates many of its strongest behavioral guarantees.

Examples covered by the current PocketIC suites include:

- disburser upgrade mid-flight preserving state
- duplicate-proof transfer completion after partial execution
- blackhole timer-only progression
- rescue-controller round-trips
- age-bonus behavior at multiple neuron ages
- faucet retry persistence across upgrades
- bounded state footprint under repeated replays
- forced rescue latching for index-anchor, latest-tx invariant, and zero-success CMC runs

### `e2e_pocketic_integration`

Runs the suite-level PocketIC end-to-end tests across the disburser and faucet together.

The current E2E coverage includes:

- disburser paying faucet and faucet topping up a target canister
- repeated disburser payouts feeding faucet full-history replay
- retry safety across the disburser → faucet → CMC boundary
- faucet upgrade during retry-state recovery

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
```

## Relationship to raw `cargo test`

You can still run crate-level tests directly, for example:

```bash
cargo test -p jupiter-disburser --lib
cargo test -p jupiter-faucet --lib
```

But once you need mocks, debug canisters, local replica setup, or the ignored PocketIC suites, `xtask` is the intended abstraction.

## Related docs

- suite overview: [`../README.md`](../README.md)
- disburser details: [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md)
- faucet details: [`../jupiter-faucet/README.md`](../jupiter-faucet/README.md)
