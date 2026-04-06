# Jupiter Historian

`jupiter-historian` is the indexing and observability canister for the Jupiter Faucet Suite.

It keeps a durable set of target canisters discovered from faucet staking-account transfer memos, records bounded contribution history for tracked canisters, records ICP burn observations by scanning CMC deposit accounts, and records bounded periodic cycles observations when those balances are observable on-chain.

See the suite overview in [`../README.md`](../README.md).

## Role in the suite

`jupiter-historian` owns five things:

1. incrementally indexing the faucet staking account without reprocessing the same transfer twice
2. keeping distinct canister sets discovered from transfer memos and optional SNS discovery
3. recording capped per-canister contribution history so frontends can graph participation over time
4. recording capped per-canister burn and cycles history so frontends can show what happened after contribution
5. exposing the public read model consumed by the production frontend

This canister is **read-oriented**. It does not move value, control the NNS neuron, or perform top-ups.

## Observation model

### Contribution indexing

The historian scans the same staking account that `jupiter-faucet` uses.

Unlike the faucet, it keeps an incremental cursor and does not rescan old staking transfers after they have already been indexed.

For each eligible incoming `Transfer` **to** the staking account (`TransferFrom` records are ignored) it can derive:

- transaction ID
- timestamp (from index timestamp when available, otherwise created-at time if available)
- transfer amount
- whether the contribution counts toward faucet eligibility under the current `min_tx_e8s`

Memo handling mirrors the faucet’s input rules:

- only consider non-empty `icrc1_memo` bytes
- ignore legacy numeric memos entirely
- treat an empty `icrc1_memo` as missing / invalid
- trim ASCII text before trying to parse principal text (the supported UX is to enter the target canister ID)

If the memo decodes to ASCII principal text in `icrc1_memo` (max 32 bytes) **and** the amount is at least `min_tx_e8s`, the beneficiary principal is tracked in the historian registry and attached to that parsed principal as qualifying history. The supported UX is still to enter the target canister ID in the memo. Below-threshold memo contributions are kept only in a separate capped recent feed and do **not** create durable canister tracking, burn targets, or cycles-sweep targets. The production minimum is intentionally **1 ICP** so registering very large numbers of beneficiaries stays expensive; historian keeps that durable registry specifically for qualifying memo-derived targets so later cycles and burn activity can be tracked efficiently on-chain and on the frontend. The code also enforces an absolute floor of **0.1 ICP** because lower values can become dust once weekly top-up fees are considered in weak ICP-price conditions.

Operationally, this means historian only treats **non-empty ASCII `icrc1_memo` text that parses as principal text, fits within 32 bytes, and is neither the anonymous principal nor the management canister principal** as a candidate beneficiary memo. Legacy numeric memos are ignored, and below-threshold contributions never create durable tracking.

Memo encoding uses `icrc1_memo` principal text only. Historian intentionally ignores the legacy numeric memo path because the supported UX is a text target-canister memo, and the 64-bit numeric memo field is not a reliable way to carry a canister ID. Historian also deliberately does not hard-code a `-cai` suffix check, so future textual canister-ID conventions are not baked into durable indexing logic.

If the memo is valid text but does **not** parse as principal text under that policy, the historian keeps a capped recent-invalid-contribution marker instead of dropping the attempt completely. The feed records that an invalid memo attempt happened without echoing attacker-provided text back through the public dashboard/API.

### Burn indexing

The historian also indexes ICP burns by scanning the CMC deposit accounts for:

- every qualifying memo-derived canister
- the faucet canister itself
- any canister that already has prior burn history in historian state

For each burn target it tracks:

- the last scanned deposit-account transaction ID (used as the pagination cursor; the implementation intentionally assumes the index cursor contract is monotonic and exclusive across pages rather than adding extra complexity for hypothetical duplicate page-boundary delivery)
- the last actual burn transaction ID
- cumulative burned ICP in e8s
- recent burn items for the public dashboard

The burn-target set is intentionally broader than just "currently qualifying memo-derived canisters": it includes qualifying tracked memo-derived canisters, the configured faucet canister itself when present, and any canister that already has prior burn state recorded in historian storage. That lets burn tracking continue even if an older qualifying canister stops receiving fresh contributions.

This burn indexing keys off actual `Burn` records on the CMC deposit account history, so merely transferring ICP into the deposit account does not count as “burned into cycles” until the ledger records the burn.

### Cycles history

Cycles history is recorded periodically rather than on every driver tick.

The historian supports three observation sources:

- `BlackholeStatus`
  - for memo-tracked canisters that expose public `canister_status` through the canonical blackhole canister
- `SnsRootSummary`
  - for SNS canisters discovered through SNS root summaries
- `SelfCanister`
  - for the historian canister’s own balance sample

The historian intentionally does **not** attempt to fetch logs from other canisters. Canisters cannot pull `fetch_canister_logs` from other canisters on-chain, so the historian stays strictly on-chain and uses blackhole status plus SNS root summaries instead.

One subtle but important implementation detail: each cycles sweep always includes the historian canister **itself** as a `SelfCanister` sample target, while canisters whose source set includes `SnsDiscovery` are skipped by the normal blackhole sweep and are expected to get their cycles observations from SNS root summaries instead.

### SNS discovery

When `enable_sns_tracking = true`, the historian periodically:

1. calls SNS-WASM `list_deployed_snses`
2. reads each SNS root canister summary
3. adds all discovered SNS canister IDs to its tracked set with source `SnsDiscovery`
4. records any cycles values available in the SNS root summary as `SnsRootSummary` samples

The discovery pass is intentionally chunked and resumable across ticks. The historian snapshots the deployed SNS root list once, then walks it in bounded batches using the same per-tick cap used by the cycles sweep. That keeps each run bounded even if the deployed SNS set grows materially over time.

SNS-discovered canisters are not probed through blackhole status in the regular cycles sweep when the source set indicates they should be handled via SNS summaries instead.

## Retention and deduplication

The historian intentionally keeps a bounded read model for **history**. It is not an archive of all transfers ever sent to the staking account. The canonical full transfer history remains on the ICP ledger and its archive canisters, which can also be queried through third-party dashboards. If tracked-canister cardinality ever becomes an operational issue, the intended next step is to add a dedicated archive canister rather than impose a hard cap on the live historian registry.

Durable bounded state currently uses these caps:

- the tracked target-canister registry is **not pruned**
- `max_cycles_entries_per_canister` default `100`, hard-clamped to `250`
- `max_contribution_entries_per_canister` default `100`, hard-clamped to `250`
- recent qualifying contributions: `500`
- recent below-threshold memo contributions: `100`
- recent invalid-memo contributions: `100`
- recent burns: `500`

Deduplication rules are:

- contributions are deduped by transaction ID within the retained per-canister history window
- recent contributions / invalid contributions / burns are deduped by transaction ID
- cycles samples are not appended twice for the same canister and timestamp

## Public query interface

Production methods:

- `list_canisters`
  - paged list of tracked canisters, optionally filtered by `MemoContribution` vs `SnsDiscovery`
- `get_cycles_history`
  - paged cycles history for one canister
- `get_contribution_history`
  - paged contribution history for one canister
- `get_canister_overview`
  - one-canister overview including source set, metadata, and point counts
- `get_public_counts`
  - top-level dashboard counts
- `get_public_status`
  - dashboard status, including staking account and configured ledger canister ID
- `list_registered_canister_summaries`
  - paged / sorted summary list used by the frontend registry table
  - default sort is `TotalQualifyingContributedDesc`
- `list_recent_contributions`
  - recent valid and invalid contribution feed used by the frontend
  - invalid rows are not exposed through a separate method; they appear in the same feed with `canister_id = null` and a generic placeholder memo label rather than the original attacker-provided text
- `list_recent_burns`
  - recent ICP burn feed used by the frontend

The public read model is intentionally richer than the raw history methods because the production frontend should not need to reconstruct aggregate dashboard state in the browser.

### Default paging / limit behavior

The main public queries use these code-backed defaults:

- `list_canisters`: default `limit = 50`, clamped to `1..=100`
- `get_cycles_history`: default `limit = 100`, clamped to `1..=100`
- `get_contribution_history`: default `limit = 100`, clamped to `1..=100`
- `list_registered_canister_summaries`: default `page_size = 25`, clamped to `1..=100`
- `list_recent_contributions`: default `limit = 20`, clamped to `1..=100`
- `list_recent_burns`: default `limit = 20`, clamped to `1..=100`

## Timers and driver model

### Default cadence

Defaults are:

- `scan_interval_seconds = 600` (10 minutes)
- `cycles_interval_seconds = 604800` (7 days)
- `max_index_pages_per_tick = 10`
- `max_canisters_per_cycles_tick = 25`

The historian also schedules an immediate one-shot tick roughly 1 second after install / upgrade so local and fresh deployments do not have to wait for the first full scan interval.

### What the driver does

On each driver run it:

1. advances contribution indexing
2. advances burn indexing
3. performs SNS discovery when the SNS / cycles cadence is due and SNS tracking is enabled
4. starts or advances a cycles sweep when the sweep cadence is due or a prior sweep is still in progress

The historian logs its own `Cycles: <amount>` line only once per completed sweep sample of **itself**, not on every 10-minute driver tick.

### Sweep batching

The cycles sweep is resumable:

- the canister snapshots the current list of probe targets into `active_cycles_sweep`
- it processes at most `max_canisters_per_cycles_tick` targets per driver run
- when the list is exhausted, it clears the active sweep and records `last_completed_cycles_sweep_ts`

That keeps sweep work bounded even when the tracked set grows.

## Install-time and upgrade-time configuration

### Init args

Required:

- `staking_account`

Optional:

- `ledger_canister_id` (defaults to ICP Ledger)
- `index_canister_id` (defaults to ICP Index)
- `cmc_canister_id` (defaults to CMC)
- `faucet_canister_id` (defaults to production `jupiter-faucet` canister ID)
- `blackhole_canister_id` (defaults to canonical blackhole)
- `sns_wasm_canister_id` (defaults to SNS-WASM)
- `enable_sns_tracking` (defaults to `false`)
- `scan_interval_seconds` (defaults to `600`)
- `cycles_interval_seconds` (defaults to `604800`)
- `min_tx_e8s` (defaults to `100_000_000`; must be at least `10_000_000`)
- `max_cycles_entries_per_canister` (defaults to `100`)
- `max_contribution_entries_per_canister` (defaults to `100`)
- `max_index_pages_per_tick` (defaults to `10`)
- `max_canisters_per_cycles_tick` (defaults to `25`)

### Upgrade args

Upgrades can change:

- `enable_sns_tracking`
- `scan_interval_seconds`
- `cycles_interval_seconds`
- `min_tx_e8s`
- `max_cycles_entries_per_canister`
- `max_contribution_entries_per_canister`
- `max_index_pages_per_tick`
- `max_canisters_per_cycles_tick`
- `blackhole_canister_id`
- `sns_wasm_canister_id`
- `cmc_canister_id`
- `faucet_canister_id`

### Mainnet install args committed in this repo

The committed [`mainnet-install-args.did`](mainnet-install-args.did) currently configures:

- the Jupiter staking account as the contribution source
- default ICP Ledger, ICP Index, canonical blackhole, CMC, faucet, and SNS-WASM IDs by leaving those principals as `null`
- `enable_sns_tracking = false`
- `scan_interval_seconds = 600`
- `cycles_interval_seconds = 604800`
- `min_tx_e8s = 100_000_000` (must match the faucet config, and both are validated by `scripts/validate-mainnet-install-args`)
- `max_cycles_entries_per_canister = 100`
- `max_contribution_entries_per_canister = 100`
- `max_index_pages_per_tick = 10`
- `max_canisters_per_cycles_tick = 25`

That file is intended to be the copy-pasteable install / upgrade argument source for an IC deployment of the historian.

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

Run specific historian-focused suites with:

```bash
cargo run -p xtask -- historian_unit
cargo run -p xtask -- historian_dfx_integration
cargo run -p xtask -- historian_pocketic_integration
cargo run -p xtask -- historian_all
```

The suite-level PocketIC E2E tests also exercise historian-adjacent read-model expectations through the frontend-facing queries.

Coverage includes, among other things:

- memo-derived contribution indexing without duplicate replay
- recent invalid-memo handling
- burn indexing through CMC deposit accounts
- weekly blackhole-based cycles sampling
- SNS discovery and SNS-root-summary cycles sampling
- state preservation across historian upgrades
- frontend-facing public query surfaces such as:
  - `get_public_counts`
  - `get_public_status`
  - `list_registered_canister_summaries`
  - `list_recent_contributions`
  - `list_recent_burns`

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

Useful debug helpers include:

- `debug_state`
- `debug_driver_tick`
- `debug_set_last_completed_cycles_sweep_ts`
- `debug_set_last_sns_discovery_ts`
- `debug_set_last_indexed_staking_tx_id`
- `debug_reset_runtime_state`
- `debug_reset_derived_state`

## Future SNS test coverage

SNS coverage is currently sufficient for the historian’s generic on-chain behavior, but still mock-based from the Jupiter suite’s perspective.

A future follow-up should add Jupiter-specific SNS smoke / integration coverage once the repo contains the actual Jupiter SNS configuration and deployment flow.

## Related docs

- suite overview: [`../README.md`](../README.md)
- faucet mechanics: [`../jupiter-faucet/README.md`](../jupiter-faucet/README.md)
- frontend consumer: [`../jupiter-faucet-frontend/README.md`](../jupiter-faucet-frontend/README.md)
- local testing: [`../xtask/README.md`](../xtask/README.md)
