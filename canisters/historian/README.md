# Jupiter Historian

`jupiter-historian` is the indexing and observability canister for the Jupiter Faucet Suite.

It keeps a durable set of declared canisters discovered from faucet staking-account transfer memos, records bounded commitment history for tracked canisters, records protocol-routed ICP output and rewards totals for the dashboard, and records bounded periodic cycles observations when those balances are observable on-chain.

See the suite overview in [`../../README.md`](../../README.md).

Unless otherwise noted, command examples in this README are run from the repository root.

## Role in the suite

`jupiter-historian` owns five things:

1. incrementally indexing the faucet staking account without reprocessing the same transfer twice
2. keeping distinct canister sets discovered from transfer memos and optional SNS discovery
3. recording capped per-canister commitment history so frontends can graph participation over time
4. recording protocol-routed ICP output and rewards totals for the dashboard
5. recording capped cycles history so frontends can show what happened after commitment
6. exposing the public read model consumed by the production frontend

This canister is **read-oriented**. It does not move value, control the NNS neuron, or perform top-ups.

## Observation model

### Commitment indexing

The historian scans the same ICRC-1 staking account address that [`jupiter-faucet`](../faucet) uses: `rrkah-fqaaa-aaaaa-aaaaq-cai-h7evq5y.ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68`.

Unlike the faucet, it keeps an incremental cursor and does not rescan old staking transfers after they have already been indexed.

For each eligible incoming `Transfer` **to** the staking account (`TransferFrom` records are ignored) it can derive:

- transaction ID
- timestamp (from index timestamp when available, otherwise created-at time if available)
- transfer amount
- whether the commitment counts toward faucet eligibility under the current `min_tx_e8s`

Memo handling mirrors the faucet’s input rules:

- only consider non-empty `icrc1_memo` bytes
- ignore legacy numeric memos entirely; neuron IDs must be ASCII digits in `icrc1_memo`
- treat an empty `icrc1_memo` as missing / invalid
- trim ASCII text before trying to parse a supported declaration

If the memo decodes to declared canister ID text in `icrc1_memo` (max 32 bytes) **and** the amount is at least `min_tx_e8s`, historian treats the target as a normal canister top-up beneficiary derived from memo text. The parser accepts short valid principal text, but Jupiter Faucet's supported principal-based target is a declared canister ID; ordinary non-canister principal IDs are too long for the 32-byte memo limit. Historian also indexes `canister_id.memo` raw ICP directives into the recent commitments feed with the declared canister and right-hand memo segment separated, and indexes ASCII decimal NNS neuron IDs into the recent commitments feed as neuron commitments. Below-threshold memo commitments are kept only in separate capped recent feeds and do **not** create durable canister tracking or cycles-sweep targets. The production minimum is intentionally **1 ICP** so registering very large numbers of beneficiaries stays expensive; historian keeps that durable registry specifically for qualifying normal canister top-up targets so later cycles and Jupiter routing activity can be tracked efficiently on-chain and on the frontend. The code also enforces an absolute floor of **0.1 ICP** because lower values can become dust once weekly top-up fees are considered in weak ICP-price conditions.

Operationally, this means historian treats **non-empty ASCII `icrc1_memo` text that parses as short valid principal text, `canister_id.memo`, or a non-zero decimal `u64` neuron ID, fits within 32 bytes, and is neither the anonymous principal nor the management canister principal when a principal is present** as a candidate declaration. The supported plain-principal UX is still a declared canister ID. Legacy numeric memos are ignored, and below-threshold commitments never create durable tracking.

Memo encoding uses `icrc1_memo` text only. Historian intentionally ignores the legacy numeric memo path because the supported declarations require text, and the 64-bit numeric memo field is not a reliable way to carry those declarations. Historian also deliberately does not hard-code a `-cai` suffix check, so future textual canister-ID conventions are not baked into durable indexing logic. This mirrors the faucet’s policy-only memo validation: accepted short principal text is not itself a proof that the target is an installed canister, and short non-canister principal text would be parser behavior rather than a supported user-facing target.

If the memo is valid text but does **not** parse as a supported declaration under that policy, the historian keeps a capped recent-invalid-commitment marker instead of dropping the attempt completely. The feed records that an invalid memo attempt happened without echoing attacker-provided text back through the public dashboard/API.

### Output and rewards accounting

Historian tracks two explicit protocol routing metrics for the frontend:

- **Total Output**: cumulative ICP transferred from the configured disburser staging account to the configured faucet payout account
- **Total Rewards**: cumulative ICP transferred from the configured disburser staging account to the configured SNS rewards account

These are Jupiter routing metrics, not downstream burn metrics. They remain meaningful even when the faucet later gains the ability to forward ICP directly rather than always converting it to cycles.

### Cycles history


Cycles history is recorded periodically rather than on every driver tick.

The historian records cycles samples from these observation routes:

- `BlackholeStatus`
  - for tracked canisters that expose public `canister_status` through a recognized blackhole route
- `SnsRootStatus`
  - for targeted SNS dapp/framework observation through SNS Root `canister_status` calls
- `SnsSwapStatus`
  - for SNS swap `get_canister_status` calls
- `SnsRootSummary`
  - for actual cycles values reported by SNS Root `get_sns_canisters_summary`
- `SelfCanister`
  - for the historian canister’s own balance sample

The historian intentionally does **not** attempt to fetch logs from other canisters. Canisters cannot pull `fetch_canister_logs` from other canisters on-chain, so the historian stays strictly on-chain and uses supported status and SNS summary routes instead.

Historian probing is always Auto. For each target it attempts direct self balance first, and recognized blackhole canisters are probed through their own self-status before any fallback. For ordinary targets, Auto mode first reuses the target's previously successful positive route, whether that route is the 13-node blackhole, the Fiduciary blackhole, SNS Root, or SNS Swap. This sticky route cache is heap-only runtime state and is rebuilt after upgrade as targets are probed again. If that route is absent or fails, route discovery tries the 13-node blackhole, then the Fiduciary blackhole, then SNS discovery. The 13-node blackhole is preferred when selecting an unknown blackhole route, while a healthy positive route is reused to avoid predictable failed calls and unnecessary cycles burn. Route failure triggers immediate rediscovery. No route TTL, negative cache, or stable route map is used. SNS-governed targets do not need blackhole control for discovery or SNS-root-summary cycles observations. Each cycles sweep also includes the historian canister **itself** as a `SelfCanister` sample target.

Tracked principals carry one or more `CanisterTrackingReason` values: `MemoCommitment`, `SnsDiscovery`, `RelayTarget`, and `RelayInstance`. `tracked_canister_count` is the number of unique principals with at least one currently visible tracking reason. The specialized counts `memo_registered_canister_count`, `sns_discovered_canister_count`, `relay_target_canister_count`, and `relay_instance_canister_count` are per-reason counts and do not change the unique-principal rule for `tracked_canister_count`. Active self-service target sets add `RelayTarget` to every target and `RelayInstance` to the spawned Relay. These reasons and cycles histories are independent; they do not form a target-to-Relay relationship.

### SNS discovery

When `enable_sns_tracking = true`, the historian periodically:

1. calls SNS-W (`qaa6y-5yaaa-aaaaa-aaafa-cai`) `list_deployed_snses`
2. reads each SNS root canister summary
3. adds all discovered SNS canister IDs to its tracked set with source `SnsDiscovery`
4. records any cycles values available in the SNS root summary as `SnsRootSummary` samples

The discovery pass is intentionally chunked and resumable across ticks. The historian snapshots the deployed SNS root list once, then walks it in bounded batches using the same per-tick cap used by the cycles sweep. That keeps each run bounded even if the deployed SNS set grows materially over time.

SNS-discovered canisters are not probed through blackhole status in the regular cycles sweep when the source set indicates they should be handled via SNS summaries instead.

## Retention and deduplication

The historian intentionally keeps a bounded read model for **history**. It is not an archive of all transfers ever sent to the staking account. The canonical full transfer history remains on the ICP ledger and its archive canisters, which can also be queried through third-party dashboards. If tracked-canister cardinality ever becomes an operational issue, the intended next step is to add a dedicated archive canister rather than impose a hard cap on the live historian registry. Derived caches are rebuilt at runtime instead of being treated as durable source-of-truth state, and the durable commitment/cycles histories are stored as entry-keyed stable maps with per-canister retained-key indexes so hot-path updates do not rewrite whole per-canister sample vectors in stable memory.

Durable bounded state uses these caps:

- the tracked target-canister registry for normal canister top-up beneficiaries is **not pruned**
- `max_cycles_entries_per_canister` default `100`, hard-clamped to `250`
- `max_commitment_entries_per_canister` default `100`, hard-clamped to `250`
- recent qualifying commitments: `500`
- recent below-threshold memo commitments: `100`
- recent invalid-memo commitments: `100`

Deduplication rules are:

- commitments are deduped by transaction ID within the retained per-canister history window
- recent commitments and invalid commitments are deduped by transaction ID
- cycles samples are not appended twice for the same canister and timestamp

## Public query interface

Production methods:

- `list_canisters`
  - paged list of tracked canisters, optionally filtered by `MemoCommitment` vs `SnsDiscovery`
- `get_cycles_history`
  - paged cycles history for one canister
- `get_commitment_history`
  - paged bounded history of qualifying cycles-top-up canister commitments only for one canister principal
- `get_raw_icp_commitment_history`
  - paged bounded history of qualifying destination-canister-wide raw ICP commitments only for one canister principal
- `get_neuron_commitment_history`
  - paged bounded history of qualifying neuron-wide commitments only for one neuron ID
- `get_canister_overview`
  - one-canister overview including source set, metadata, and point counts
- `get_public_counts`
  - top-level dashboard counts
- `get_public_status`
  - dashboard status, including staking account and configured ledger canister ID
- `list_memo_registered_canister_summaries`
  - paged summary list used by the frontend registry table
  - always ordered by total qualifying committed ICP descending, with canister id as the stable tie-breaker
- `list_recent_commitments`
  - recent valid and invalid commitment feed used by the frontend
  - invalid rows are not exposed through a separate method; they appear in the same feed with `canister_id = null` and a generic placeholder memo label rather than the original attacker-provided text
- `get_relay_setup_view`
  - canonicalizes a submitted 1–20 target vector and returns exact-set state, nominal target-count pricing, and a deterministic setup account only for a new available set
- `notify_relay_setup`
  - explicitly starts a sufficiently funded exact-set setup using only the submitted target vector

The public read model is intentionally richer than the raw history methods because the production frontend should not need to reconstruct aggregate dashboard state in the browser.

### Self-service Relay target sets

Self-service setup accepts 1–20 external target canisters. Historian validates every target, sorts by raw principal bytes, rejects duplicates, and hashes the framed canonical vector with SHA-256. Input order therefore does not affect identity. Exact canonical sets are idempotent, while subsets, supersets, and partial overlaps are separate valid sets. A request exactly matching the configured canonical Relay target set returns the canonical Relay without exposing a payment account.

Memory ID 25 is the only definitive self-service setup map. Its active state is only `target-set hash -> Relay principal`; no target vector or target-to-Relay registry is durable. Targets and Relay instances remain visible independently through generic tracking reasons and `list_canisters` filtering. A blackholed child is immutable, is never upgraded through Historian, and is never queried to reconstruct target configuration.

The deterministic setup subaccount is the 32-byte target-set hash. Funding uses only the aggregate ICP ledger `icrc1_balance_of` result, with no index/history scan and no payer attribution. Notification computes `max(singleton nominal minimum, conversion + safety margin + configured seed + two ledger fees) + 0.25 ICP × extra targets`, so the extra-target charge applies in every rate regime. The CMC receives conversion ICP only; the reread setup-account remainder funds Relay subaccount one, including the safety margin and extra-target charge. Deposits are not automatically refundable or swept.

Creation is an explicit user action. A narrow same-key reservation prevents duplicate execution, all targets are probed before spend, transfer records are persisted before dispatch with fixed timestamps, and create dispatch is fail-closed. Final activation requires small pre- and post-handoff audits around the Fiduciary controller transition. See [`../../docs/relay-setup-recovery.md`](../../docs/relay-setup-recovery.md) for the state and operational procedure.

On Historian upgrade, interrupted `Reserved` and `ProbingTargets` entries are removed. Every later `Creating` phase becomes terminal manual recovery with `HistorianUpgradeInterrupted`; active mappings and existing manual-recovery entries are preserved without calling any child.

### Default paging / limit behavior

The main public queries use these code-backed defaults:

- `list_canisters`: default `limit = 50`, clamped to `1..=100`
- `get_cycles_history`: default `limit = 100`, clamped to `1..=100`
- `get_commitment_history`: default `limit = 100`, clamped to `1..=100`
- `get_raw_icp_commitment_history`: default `limit = 100`, clamped to `1..=100`
- `get_neuron_commitment_history`: default `limit = 100`, clamped to `1..=100`
- `list_memo_registered_canister_summaries`: default `page_size = 25`, clamped to `1..=100`
- `list_recent_commitments`: default `limit = 20`, clamped to `1..=100`

The three commitment-history methods all page over bounded retained samples using
the same cursor semantics and the same configured history cap. Retention remains
bounded by `max_commitment_entries_per_canister`, which defaults to 100 samples
per target and is hard-clamped to 250. Raw canister and neuron durable commitment
samples are target-wide:
they do not retain the optional outgoing memo suffix used by raw ICP payout
memos, so the raw and neuron history APIs cannot provide suffix-specific
commitment attribution.

## Timers and driver model

### Default cadence

Defaults are:

- `scan_interval_seconds = 600` (10 minutes)
- `cycles_interval_seconds = 604800` (7 days)
- `max_index_pages_per_tick = 10`
- `max_canisters_per_cycles_tick = 25`

The historian also schedules an immediate one-shot tick roughly 1 second after install or upgrade so local and fresh deployments do not have to wait for the first full scan interval.

### What the driver does

On each driver run it:

1. advances commitment indexing
2. performs SNS discovery when the SNS / cycles cadence is due and SNS tracking is enabled
3. starts or advances a cycles sweep when the sweep cadence is due or a prior sweep is still in progress

Commitment indexing records a visible durable fault if the historian observes non-monotonic staking-account transaction ids from the index. While the fault is present the dashboard surfaces the degraded state, and later driver ticks retry commitment indexing from the last known-good cursor. Once the upstream index recovers and forward progress resumes cleanly, the fault clears automatically.

The historian logs its own `Cycles: <amount>` line only once per completed sweep sample of **itself**, not on every 10-minute driver tick.

### Runtime config verification

After verifying that the deployed Wasm matches the source build, users can verify the live install-time and upgrade-time config from public canister logs. The historian emits a single `CONFIG ...` line on the cycles-sweep cadence when it records the historian canister's own cycles sample, alongside its regular `Cycles: ...` line. The line is comma-separated `key=value` text and includes the staking, output, rewards, ledger/index/CMC/faucet/SNS-W/XRC settings, SNS tracking flag, scan and cycles intervals, minimum tracked commitment, retention caps, and per-tick work limits.

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
- `faucet_canister_id` (defaults to production [`jupiter-faucet`](../faucet) canister ID)
- `sns_wasm_canister_id` (defaults to SNS-WASM)
- `enable_sns_tracking` (defaults to `false`)
- `scan_interval_seconds` (defaults to `600`)
- `cycles_interval_seconds` (defaults to `604800`)
- `min_tx_e8s` (defaults to `100_000_000`; must be at least `10_000_000`)
- `max_cycles_entries_per_canister` (defaults to `100`)
- `max_commitment_entries_per_canister` (defaults to `100`)
- `max_index_pages_per_tick` (defaults to `10`)
- `max_canisters_per_cycles_tick` (defaults to `25`)

### Upgrade args

Upgrades can change:

- `staking_account`
- `ledger_canister_id`
- `index_canister_id`
- `enable_sns_tracking`
- `clear_commitment_index_fault`
- `output_source_account`
- `output_account`
- `rewards_account`
- `scan_interval_seconds`
- `cycles_interval_seconds`
- `min_tx_e8s`
- `max_cycles_entries_per_canister`
- `max_commitment_entries_per_canister`
- `max_index_pages_per_tick`
- `max_canisters_per_cycles_tick`
- `sns_wasm_canister_id`
- `xrc_canister_id`
- `cmc_canister_id`
- `faucet_canister_id`

Inspect the current `UpgradeArgs` definition in [`src/lifecycle.rs`](src/lifecycle.rs), its imported API type in [`src/api.rs`](src/api.rs), and the exported DID [`jupiter_historian.did`](jupiter_historian.did) before preparing any upgrade-time argument file.

### Mainnet install args committed in this repo

The committed [`mainnet-install-args.did`](mainnet-install-args.did) configures:

- the Jupiter staking account as the commitment source
- default ICP Ledger, ICP Index, CMC, faucet, and SNS-WASM IDs by leaving those principals as `null`
- `enable_sns_tracking = false`
- `scan_interval_seconds = 600`
- `cycles_interval_seconds = 604800`
- `min_tx_e8s = 100_000_000` (must match the faucet config, and both are validated by [`../../tools/scripts/validate-mainnet-install-args`](../../tools/scripts/validate-mainnet-install-args))
- `max_cycles_entries_per_canister = 100`
- `max_commitment_entries_per_canister = 100`
- `max_index_pages_per_tick = 10`
- `max_canisters_per_cycles_tick = 25`

That file is intended to be the copy-pasteable argument source for a brand-new IC deployment of the historian. Routine upgrades preserve existing state and should pass no args unless a temporary `Option<UpgradeArgs>` config-change file is intentionally reviewed.

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
cargo run -p xtask -- historian_local_integration
cargo run -p xtask -- historian_pocketic_integration
cargo run -p xtask -- historian_all
```

The suite-level [PocketIC E2E tests](../../tests/pocketic) also exercise historian-adjacent read-model expectations through the frontend-facing queries.

Coverage includes, among other things:

- memo-derived commitment indexing without duplicate replay
- recent invalid-memo handling
- commitment-index degraded-state detection and automatic recovery on non-monotonic staking-account tx pages
- weekly blackhole-based cycles sampling
- SNS discovery and SNS-root-summary cycles sampling
- state preservation across historian upgrades
- frontend-facing public query surfaces such as:
  - `get_public_counts`
  - `get_public_status`
  - `list_memo_registered_canister_summaries`
  - `list_recent_commitments`

For the broader test matrix, see [`../../tools/xtask/README.md`](../../tools/xtask/README.md).

## Reproducible builds and deployment

### Canonical reproducible release build

For production artifacts, use the pinned Docker-based workflow from the repo root:

```bash
chmod +x tools/scripts/docker-build tools/scripts/build-canister
./tools/scripts/docker-build
```

This uses [`../../Dockerfile.repro`](../../Dockerfile.repro), which pins the base image digest, Rust toolchain, and `ic-wasm` version, then runs [`../../tools/scripts/build-canister`](../../tools/scripts/build-canister) inside that controlled environment.

It produces the canonical release artifacts under `release-artifacts/`, including:

- `release-artifacts/jupiter_historian.wasm`
- `release-artifacts/jupiter_historian.wasm.gz`
- `release-artifacts/jupiter_relay.wasm`
- `release-artifacts/jupiter_relay.wasm.gz`
- corresponding `.sha256` files

The checked-in production args enable `relay_factory_enabled = opt true`, so `jupiter_historian.wasm.gz` is the relay-enabled canonical production Historian artifact. Its embedded Relay install payload must correspond to the reviewed raw Relay Wasm from the same Docker/reproducible release build, recorded as `release-artifacts/jupiter_historian.reviewed-relay-wasm-raw.sha256`, and `release-artifacts/jupiter_relay.wasm.gz` must decompress to those reviewed raw bytes. Runtime self-service Relay reconciliation reads live child status and compares it with the approved installed-module hash. Historian does not persist per-instance expected Relay hashes. Self-service Relays use the canonical Relay daily cadence (`main_interval_seconds = 86400`), their submitted canonical target vector, automatic probe routing, and the configured surplus recipient. If a local no-relay artifact is needed for development or tests, build `jupiter-historian-no-relay`, which writes `release-artifacts/jupiter_historian_no_relay.wasm.gz`.

### Deploy canonical release artifact on the IC

Production canister: `jupiter_historian` / `j5gs6-uiaaa-aaaar-qb5cq-cai`

Existing production Historian must be upgraded in place. Reinstall destroys stable history, tracking metadata, active hash-to-Relay mappings, setup progress, index cursors, aggregates, and other durable state; it is prohibited for the existing production canister.

Routine no-config production upgrade:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian \
  --environment ic \
  --mode upgrade
```

The committed install-args file is for fresh installs only. Do not pass fresh-install args when upgrading an existing Historian.

Normal production upgrades preserve stable state and must use the historian `post_upgrade` argument shape, not the fresh-install `InitArgs` shape.

For a production upgrade with an intentional config change, create a temporary local `UpgradeArgs` file. Fill in only the fields intentionally changed by that deployment. Do not commit the temporary file.

Historian probing is always Auto. There is no `cycles_probe_policy` upgrade field.

```bash
cat > /tmp/historian-upgrade-args.did <<'EOF'
(
  opt record {
    // Fill in only the UpgradeArgs fields intentionally changed by this deployment.
    // Set unchanged optional fields to null, or omit them if the UpgradeArgs type
    // and Candid tooling allow omission.
    //
    // Example shape only:
    // field_to_change = opt <new value>;
    // field_to_leave_unchanged = null;
  }
)
EOF
```

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian \
  --environment ic \
  --mode upgrade \
  --args-file /tmp/historian-upgrade-args.did
```

After upgrade, verify the runtime config from public logs:

```bash
icp canister logs j5gs6-uiaaa-aaaar-qb5cq-cai -n ic
```

Before stopping the canister, record pre-upgrade query results for later comparison. Stopping `jupiter_historian` is the executable self-service factory pause for this deployment.

Tested `icp 0.2.6` maintenance sequence:

```bash
icp canister stop jupiter_historian --environment ic
SNAPSHOT_ID="$(icp canister snapshot create jupiter_historian --environment ic --quiet)"
icp canister snapshot list jupiter_historian --environment ic --json
icp canister snapshot download jupiter_historian "$SNAPSHOT_ID" --environment ic --output /tmp/jupiter-historian-snapshot-"$SNAPSHOT_ID"
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian --environment ic --mode upgrade
icp canister start jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
```

Local testing showed the stopped-canister upgrade succeeds but leaves the canister stopped, so the explicit start command is required. Rollback uses the recorded snapshot ID:

```bash
icp canister stop jupiter_historian --environment ic
icp canister snapshot restore jupiter_historian "$SNAPSHOT_ID" --environment ic
icp canister start jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
```

Before a future upgrade, disable the factory and verify no setup entry is `Creating`. Active hash-to-Relay mappings and independent tracking reasons are preserved by the in-place upgrade. Verify histories, first-seen metadata, cursors, totals, active mappings, automatic cycles probing, and `RelayTarget`/`RelayInstance` counts before re-enabling setup.

## Debug interface

The production canister exposes the query interface described above.

Additional debug-only methods are gated behind the `debug_api` feature and are intended for local integration and PocketIC testing only. Debug builds also check the embedded production canister ID at runtime and reject debug API use when the canister principal is the production historian principal. The operational model treats that production-principal guard as sufficient: debug builds must not be installed on production canister IDs, production canister IDs reject debug API use, and a newly deployed canister with debug APIs is a separate non-production/debug deployment. No additional caller-authorization layer is desired for these debug surfaces. The debug Candid surface is committed at:

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

SNS coverage is sufficient for the historian's generic on-chain behavior, but remains mock-based from the Jupiter suite's perspective.

A future follow-up should add Jupiter-specific SNS smoke / integration coverage once the repo contains the actual Jupiter SNS configuration and deployment flow.

## Related docs

- suite overview: [`../../README.md`](../../README.md)
- faucet mechanics: [`../faucet/README.md`](../faucet/README.md)
- frontend consumer: [`../frontend/README.md`](../frontend/README.md)
- local testing: [`../../tools/xtask/README.md`](../../tools/xtask/README.md)
