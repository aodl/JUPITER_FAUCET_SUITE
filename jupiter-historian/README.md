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

## Testing

Coverage added with this module includes:

- unit tests for memo parsing, dedupe, pruning, and source-merging logic
- local mock-backed DFX scenarios
- PocketIC integration tests for contribution indexing, blackhole cycles history, SNS discovery, and upgrade persistence
- PocketIC happy-path blackhole coverage builds and installs the vendored `third_party/ic-blackhole` source via its pinned `make repro-build` path
- suite-level E2E coverage so historian participates in the integrated flow
- PocketIC historian coverage also exercises the vendored blackhole reproducibility/hash verification path

The vendored `ic-blackhole` build is now expected to be reproducible, and the historian PocketIC flow runs the corresponding ignored hash-verification test when invoked through the suite harness.

### Future SNS test coverage

**TODO:** Revisit `jupiter-historian` SNS integration testing once the Jupiter Faucet Suite’s own SNS configuration is represented in this repository. At that point, replace or supplement mock-based SNS historian tests with Jupiter-specific SNS smoke/integration coverage using the in-repo SNS configuration.
