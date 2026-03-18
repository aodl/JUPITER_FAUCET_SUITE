# Jupiter Historian

`jupiter-historian` is the indexing and observability canister for the Jupiter Faucet Suite.

It keeps a distinct set of target canisters discovered from faucet staking-account transfer memos, records contribution history for those canisters, and records weekly cycles observations for tracked canisters when those balances are observable on-chain.

See the suite overview in [`../README.md`](../README.md).

## What it is responsible for

`jupiter-historian` owns four things:

1. incrementally indexing the faucet staking account without reprocessing the same transfer twice
2. keeping a distinct set of canister IDs discovered from transfer memos
3. recording capped per-canister contribution history so frontends can graph stake top-ups over time
4. recording capped per-canister cycles history on a weekly cadence

## Observation model

### Contribution indexing

The historian scans the same staking account that `jupiter-faucet` uses.

Unlike the faucet, it keeps an incremental cursor and does not rescan old transfers after they have already been indexed.

For each valid memo-derived canister ID it stores:

- transaction ID
- timestamp (when available from index data)
- transfer amount
- whether the contribution counts toward faucet eligibility under the current `min_tx_e8s`

### Cycles history

Cycles history is recorded once per weekly sweep.

The historian supports three observation sources:

- `BlackholeStatus` for memo-tracked canisters that expose public `canister_status` through the canonical blackhole canister
- `SnsRootSummary` for SNS canisters discovered through SNS root summaries
- `SelfCanister` for the historian canister’s own weekly balance sample

The historian intentionally does **not** attempt to fetch logs from other canisters. Canisters cannot pull `fetch_canister_logs` from other canisters on-chain, so the historian stays strictly on-chain and uses blackhole status and SNS root summaries instead.

## Retention

Both timelines are capped per canister and pruned oldest-first.

Install / upgrade args configure:

- `max_cycles_entries_per_canister` (default `100`)
- `max_contribution_entries_per_canister` (default `100`)

Duplicate protection rules are:

- contributions are deduped by transaction ID
- cycles samples are not appended twice for the same canister and timestamp

## Public query interface

Production methods:

- `list_canisters`
- `get_cycles_history`
- `get_contribution_history`
- `get_canister_overview`

`list_canisters` supports paging and optional source filtering so a frontend can distinguish memo-tracked canisters from SNS-discovered canisters.

## Timers

The historian uses a 10-minute driver timer by default.

On each driver run it:

- advances contribution indexing
- performs SNS discovery when the weekly SNS schedule is due
- advances or starts the weekly cycles sweep when due

The historian logs its own `Cycles: <amount>` line only once per completed weekly sweep, not on every 10-minute driver tick.

## Install args

Required:

- `staking_account`

Optional:

- `ledger_canister_id`
- `index_canister_id`
- `blackhole_canister_id`
- `sns_wasm_canister_id`
- `enable_sns_tracking`
- `scan_interval_seconds`
- `cycles_interval_seconds`
- `min_tx_e8s`
- `max_cycles_entries_per_canister`
- `max_contribution_entries_per_canister`
- `max_index_pages_per_tick`
- `max_canisters_per_cycles_tick`

Defaults follow the current suite conventions and mainnet public-system canister IDs.

### Mainnet install args committed in this repo

The committed [`mainnet-install-args.did`](mainnet-install-args.did) currently configures:

- the Jupiter staking account as the contribution source
- default ICP Ledger, ICP Index, canonical blackhole, and SNS-WASM canister IDs by leaving those principals as `null`
- `enable_sns_tracking = false`
- `scan_interval_seconds = 600`
- `cycles_interval_seconds = 604800`
- `min_tx_e8s = 10_000_000`
- `max_cycles_entries_per_canister = 100`
- `max_contribution_entries_per_canister = 100`
- `max_index_pages_per_tick = 10`
- `max_canisters_per_cycles_tick = 25`

That file is intended to be the copy-pasteable install/upgrade argument source for an IC deployment of the historian.

## Build and test

### Local development build

This is useful for iterative local work, but it is **not** the canonical reproducible release workflow used for production artifacts:

```bash
cargo build -p jupiter-historian --target wasm32-unknown-unknown --release --locked
```

### Local debug-interface build

This enables the debug-only API surface for local integration work. It is also **not** the canonical reproducible release workflow:

```bash
cargo build -p jupiter-historian --target wasm32-unknown-unknown --release --features debug_api --locked
```

### Tests

Run specific tests with the following xtask commands:

```bash
cargo run -p xtask -- historian_unit
cargo run -p xtask -- historian_dfx_integration
cargo run -p xtask -- historian_pocketic_integration
cargo run -p xtask -- e2e_pocketic_integration
```

Run all jupiter-historian tests with:

```bash
cargo run -p xtask -- historian_all
```

Those cover, among other things:

- memo-derived contribution indexing without duplicate replay
- weekly blackhole-based cycles sampling
- SNS discovery and SNS-root-summary cycles sampling
- state preservation across historian upgrades
- suite-level coordination with the faucet staking flow
- reproducible blackhole wasm verification before PocketIC installation

For the broader test matrix, see [`../xtask/README.md`](../xtask/README.md).

## Reproducible builds and deployment

### Canonical reproducible release build

For production artifacts, use the pinned Docker-based workflow from the repo root:

```bash
chmod +x ../scripts/docker-build ../scripts/build-canister
../scripts/docker-build
```

This uses `../Dockerfile.repro`, which pins the base image digest, Rust toolchain, and `ic-wasm` version, then runs `../scripts/build-canister` inside that controlled environment.

It produces the canonical release artifacts under `../release-artifacts/`, including:

- `release-artifacts/jupiter_historian.wasm`
- `release-artifacts/jupiter_historian.wasm.gz`
- corresponding `.sha256` files

### Install canonical release artifact on the IC

Fresh install:

```bash
dfx canister install jupiter_historian \
  --network ic \
  --wasm release-artifacts/jupiter_historian.wasm.gz \
  --argument-file jupiter-historian/mainnet-install-args.did
```

Upgrade:

```bash
dfx canister install jupiter_historian \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_historian.wasm.gz \
  --argument-file jupiter-historian/mainnet-install-args.did
```

## Debug interface

The production canister exposes the query interface described above.

Additional debug-only methods are gated behind the `debug_api` feature and are intended for local integration and PocketIC testing only. The debug Candid surface is committed at:

- [`jupiter_historian_debug.did`](jupiter_historian_debug.did)

## Future SNS test coverage

**TODO:** Revisit `jupiter-historian` SNS integration testing once the Jupiter Faucet Suite’s own SNS configuration is represented in this repository. At that point, replace or supplement mock-based SNS historian tests with Jupiter-specific SNS smoke/integration coverage using the in-repo SNS configuration.

