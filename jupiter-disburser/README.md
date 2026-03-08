# Jupiter Disburser

Jupiter Disburser is an Internet Computer canister that automates maturity disbursement for a single NNS neuron and routes the resulting ICP according to a fixed policy.

## Overview

The canister controls one NNS neuron, initiates `DisburseMaturity` calls against that neuron, receives minted ICP into a staging account owned by the canister, and then distributes the staged ICP across three destinations:

- the normal recipient receives the age-neutral base portion
- bonus recipient 1 receives 80% of the age bonus portion
- bonus recipient 2 receives 20% of the age bonus portion

The intended operating model is straightforward:

- install with a fixed neuron and fixed recipient accounts
- validate behavior with local integration tests and PocketIC end-to-end tests
- deploy a pinned release artifact built in a reproducible container environment
- compare the deployed module hash against the reproducible local build output

## What the canister does

On each main cycle, the canister:

1. reads the current neuron state
2. finalizes any payout that is already staged on the ledger
3. initiates a new maturity disbursement to its staging account when no governance-side disbursement is already in flight
4. records the neuron age at the point of initiation so the later split is based on the age that produced the reward

When staged ICP is present, the canister derives:

- `base`
- `bonus = total - base`
- `bonus_1 = 80% of bonus`
- `bonus_2 = 20% of bonus`

The resulting transfers are executed with ICRC-1 ledger calls.

## Age bonus model

The canister follows the NNS age-bonus ramp:

- age 0 years: `1.00`
- age 2 years: `1.125`
- age 4 years and above: `1.25`

The multiplier increases linearly from `1.00` to `1.25` over the first four years, then clamps. The canister uses the recorded neuron age to derive the age-neutral base component and the bonus component from the actual amount minted to the staging account.

## Design notes

### Staging first, payout second

NNS Governance disburses maturity to one destination per call. Jupiter Disburser always disburses to its own staging account first and only then performs the recipient split on the ledger side.

That keeps the payout logic grounded in the amount actually minted and makes retries much easier to reason about than repeated governance calls.

### Persistent payout plans

Before the first transfer is attempted, the canister persists a payout plan. If a transfer succeeds but execution stops before the plan is cleared, the next run can safely resume without recalculating the split from a partially-updated state.

### Idempotent timers

Recurring timers drive the canister, but safety comes from state-based idempotence rather than from assuming that timers fire exactly on schedule.

### Lifeline-based disaster recovery

Jupiter Disburser is configured with a required `rescue_controller` principal. In production this should be the principal of the `jupiter-lifeline` canister that ships in the same repository.

The rescue path is intentionally independent of ledger and governance health checks. The canister records the timestamp of the last confirmed successful ledger transfer in stable state. A separate rescue timer only compares the current time against that persisted timestamp and, when the broken-window policy is reached, asks the management canister to widen the controller set to `[jupiter_lifeline, jupiter_disburser]`.

That means recovery still works if ledger or governance APIs change in a way that prevents fresh transfers, because the rescue decision does not depend on any successful ledger or governance call at the point of escalation.

### Lifeline operating model

The `jupiter-lifeline` canister is intentionally minimal. It is only a durable controller target and a place to land emergency recovery authority if transfer flow stops for long enough to trip the rescue policy.

Normal operation does not require any recovery logic inside the lifeline canister. If a lifeline event ever occurs, the expected response is to inspect the exact failure mode, prepare targeted recovery code, and upgrade `jupiter-lifeline` with the specific actions needed for that incident.

## Public interface

Production builds expose a very small public surface:

- `metrics() -> Metrics`

`Metrics` includes:

- `prev_age_seconds`
- `last_successful_transfer_ts`
- `rescue_triggered`

Debug-only methods used for local testing and PocketIC scenarios are gated behind the `debug_api` feature and are not intended for production deployment.

## Install-time configuration

The canister is configured at install time with the following fields:

- `neuron_id: nat64`
- `normal_recipient: Account`
- `age_bonus_recipient_1: Account`
- `age_bonus_recipient_2: Account`
- `ledger_canister_id: opt principal`
- `governance_canister_id: opt principal`
- `rescue_controller: principal`
- `main_interval_seconds: opt nat64`
- `rescue_interval_seconds: opt nat64`

If the ledger or governance canister IDs are omitted, the production NNS canister IDs are used.

### Example init payload

```candid
(
  record {
    neuron_id = 123_456_789 : nat64;
    normal_recipient = record {
      owner = principal "aaaaa-aa";
      subaccount = null;
    };
    age_bonus_recipient_1 = record {
      owner = principal "bbbbb-bb";
      subaccount = null;
    };
    age_bonus_recipient_2 = record {
      owner = principal "ccccc-cc";
      subaccount = null;
    };
    ledger_canister_id = null;
    governance_canister_id = null;
    rescue_controller = principal "ddddd-dd";
    main_interval_seconds = opt 86400;
    rescue_interval_seconds = opt 86400;
  }
)
```

## Repository layout

The main pieces of the repository are:

- `src/` — canister code
- `xtask/` — local orchestration and test tooling
- `xtask/src/pocketIC/` — PocketIC end-to-end tests
- `xtask/src/mocks/` — local mock canisters used by integration scenarios
- `scripts/` — reproducible build helpers
- `jupiter-lifeline/` — emergency recovery canister used as the rescue controller target

## Prerequisites

- Rust stable
- `wasm32-unknown-unknown` Rust target
- `dfx`
- Docker for reproducible release builds

Install the wasm target if needed:

```bash
rustup target add wasm32-unknown-unknown
```

## Development build

### Production canister build

```bash
cargo build -p jupiter-disburser --target wasm32-unknown-unknown --release --locked
```

### Debug build

The debug build enables test-only methods used by `xtask` and PocketIC scenarios.

```bash
cargo build -p jupiter-disburser --target wasm32-unknown-unknown --release --features debug_api --locked
```

## Testing

The repository has three test layers.

### Unit tests

```bash
cargo test -p jupiter-disburser --lib
```

### Local integration scenarios

These use `dfx` together with the mock governance and ledger canisters.

```bash
cargo run -p xtask -- test
```

### PocketIC end-to-end tests

These tests exercise real governance and ledger behavior and are intentionally slower. They are marked `#[ignore]`.

```bash
RUST_TEST_THREADS=1 cargo test -p jupiter-disburser --test pocketic_e2e -- --ignored
```

### Useful combined commands

Run local setup, integration tests, unit tests, and teardown:

```bash
cargo run -p xtask -- setup_test_teardown
```

Run local setup, integration tests, unit tests, PocketIC E2E, and teardown:

```bash
cargo run -p xtask -- setup_test_all_teardown
```

Run the full test stack without the setup and teardown wrapper:

```bash
cargo run -p xtask -- test-all
```

## Reproducible build

The release build is produced inside a pinned Docker environment. The build outputs both:

- `release-artifacts/jupiter_disburser.wasm`
- `release-artifacts/jupiter_disburser.wasm.gz`

The `.wasm.gz` file is the deployment package. The uncompressed `.wasm` file is the installed module reference used for hash comparison.

Build the release artifacts with:

```bash
chmod +x scripts/docker-build scripts/build-canister
./scripts/docker-build
```

The script prints two hashes:

- the SHA-256 of `jupiter_disburser.wasm`
- the SHA-256 of `jupiter_disburser.wasm.gz`

The first hash is the one to compare against the on-chain module hash. The second hash is the one that identifies the compressed deployment package.

## Production deployment

Compressed Wasm can be installed with `dfx canister install --wasm <file>.wasm.gz`. The release flow in this repository uses that path rather than `dfx deploy`, because `dfx deploy` does not support compressed Wasm installation.

### Disaster recovery deployment pattern

A production deployment should include both canisters:

1. deploy `jupiter_lifeline`
2. record its principal
3. install `jupiter_disburser` with `rescue_controller` set to that principal
4. add `jupiter_disburser` as a controller of itself so it can update its own controller set when the rescue policy triggers

The rescue path is deliberately based on elapsed time since the last confirmed transfer, not on a live ledger probe. This avoids a circular dependency where a broken ledger or governance API would also block the handoff to the lifeline canister.

### Create the canister

```bash
dfx canister create jupiter_disburser --network ic
```

### Install the reproducible release artifact

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --wasm release-artifacts/jupiter_disburser.wasm.gz \
  --argument-file <INIT_ARGS_FILE>
```

For upgrades, use `--mode upgrade` and provide the appropriate argument form for the release being installed.

## Verification against a deployed canister

After installation, retrieve the deployed module hash with:

```bash
dfx canister --network ic info <CANISTER_ID>
```

Compare the reported module hash to the SHA-256 of `release-artifacts/jupiter_disburser.wasm` printed by `./scripts/docker-build`.

That comparison is the verification step. If the source tree, pinned toolchain, and build environment match, the locally rebuilt `jupiter_disburser.wasm` hash should match the deployed module hash.

## Release checklist

- run the full local test stack
- run the ignored PocketIC end-to-end tests
- build the reproducible release artifact with `./scripts/docker-build`
- record the printed SHA-256 values
- install `release-artifacts/jupiter_disburser.wasm.gz`
- compare the deployed module hash with the local `jupiter_disburser.wasm` hash
- tag the exact source revision used for the release

## Notes

- `Cargo.lock` is part of the reproducible build input and should be committed.
- The reproducible build is pinned to a Linux `amd64` Docker environment.
- The reproducible release flow is intended for shipping artifacts. Local development can continue to use normal Cargo and `dfx` workflows.



