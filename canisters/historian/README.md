# Jupiter Historian

`jupiter-historian` is the indexing and observability canister for the Jupiter Faucet Suite.

It keeps a durable set of declared canisters discovered from faucet staking-account transfer memos, records bounded commitment history for tracked canisters, records protocol-routed ICP output and rewards totals for the dashboard, and records bounded periodic cycles observations when those balances are observable on-chain.

See the suite overview in [`../../README.md`](../../README.md).

Unless otherwise noted, command examples in this README are run from the repository root.

## Role in the suite

`jupiter-historian` owns seven things:

1. incrementally indexing the faucet staking account without reprocessing the same transfer twice
2. keeping distinct canister sets discovered from transfer memos and optional SNS discovery
3. recording capped per-canister commitment history so frontends can graph participation over time
4. recording authoritative lifetime qualifying commitment totals for exact commitment routes
5. recording protocol-routed ICP output and rewards totals for the dashboard
6. recording capped cycles history so frontends can show what happened after commitment
7. exposing the public read model consumed by the production frontend

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

### Lifetime commitment-route totals

`get_commitment_route_summaries` is the authoritative batch query for cumulative qualifying ICP committed through Jupiter Faucet to exact memo-declared routes. It reports committed/staked ICP only. It does not report payouts, delivered cycles, balances, conversion rates, fees, burn coverage, maturity, Relay support, or estimated future results.

The three route variants preserve the memo parser's exact distinctions:

- `CyclesTopUp { canister_id }` represents a plain `<canister>` memo.
- `RawIcp { destination_canister_id; memo }` represents `<canister>.<suffix>`. `<canister>.` has an empty memo blob and is distinct from the plain `<canister>` cycles-top-up route. Empty and non-empty suffixes, and equal suffixes for different destination canisters, remain separate routes.
- `NeuronStake { neuron_id; memo }` represents a decimal NNS neuron ID with its optional outgoing memo. `<neuron_id>` uses `memo = null`, while `<neuron_id>.` uses an explicitly present empty blob; those routes are distinct. Equal suffixes for different neuron IDs also remain separate.

Canister route identity uses parsed Principal bytes, so compact and hyphenated text that parses to the same Principal addresses one route. Raw and neuron suffix bytes are preserved exactly, including empty bytes; they are not trimmed, lowercased, or normalized. Neuron routes are keyed by the declared neuron ID rather than a principal or derived staking account. NNS neuron staking accounts share Governance as owner and are distinguished by neuron-derived subaccounts, while Jupiter's declaration already provides the stable neuron ID; this query therefore neither derives an account nor calls Governance.

Each exact route has one stable cumulative roll-up containing its qualifying commitment count and total qualifying committed e8s. Multiple transactions update that one entry. The total is independent of the bounded normal-canister, destination-wide raw-ICP, and neuron-wide retained histories and does not decrease when those histories prune old samples.

The response is an as-of view. `indexed_through_staking_tx_id` is the existing staking-account index cursor, `last_index_run_ts` reports freshness, and `commitment_index_fault` carries any durable index-order fault. `complete_from_genesis` is true only after this projection has covered staking-account history from genesis through the returned cursor. A zero count and total are authoritative only when `complete_from_genesis = true` and `commitment_index_fault = null`; otherwise zero may mean the projection is incomplete or indexing is degraded. The query preserves request order and duplicates, processes at most 100 routes, and sets `truncated = true` when additional inputs were supplied.

### Output and rewards accounting

Historian tracks two explicit protocol routing metrics for the frontend:

- **Total Output**: cumulative ICP transferred from the configured disburser staging account to the configured faucet payout account
- **Total Rewards**: cumulative ICP transferred from the configured disburser staging account to the configured SNS rewards account

These are Jupiter routing metrics, not downstream burn metrics. They remain meaningful even when the faucet later gains the ability to forward ICP directly rather than always converting it to cycles.

### Cycles history


Cycles history is recorded periodically rather than on every driver tick.

The historian records cycles samples from these observation routes:

- `DirectCanisterStatus`
  - for the protocol-native management-canister `canister_status` route
- `BlackholeStatus`
  - for tracked canisters observed through a recognized blackhole fallback
- `SnsRootStatus`
  - for targeted SNS dapp/framework observation through SNS Root `canister_status` calls
- `SnsSwapStatus`
  - for SNS swap `get_canister_status` calls
- `SnsRootSummary`
  - retained only so historical samples remain decodable; discovery no longer creates this source
- `SelfCanister`
  - for the historian canister’s own balance sample

The historian intentionally does **not** attempt to fetch logs from other canisters. Canisters cannot pull `fetch_canister_logs` from other canisters on-chain, so the historian stays strictly on-chain and uses supported status routes.

Historian probing is always Auto. It uses local self balance for Historian itself, then attempts direct management-canister `canister_status` before every cached or newly discovered proxy. A successful direct call may rely on `public`, a suitable `allowed_viewers` entry, or controller access. If direct access is unavailable, Auto tries the canonical-blackhole self-status special case, the cached positive fallback, the 13-node blackhole, the Fiduciary blackhole, and finally SNS Root or Swap discovery. The heap-only positive-route cache is rebuilt after upgrade and can never pre-empt a later direct success. Direct denial is not negatively cached. Route failure triggers immediate rediscovery; no route TTL, negative cache, or stable route map is used. Each cycles sweep also includes the historian canister **itself** as a `SelfCanister` sample target.

Tracked principals carry one or more `CanisterTrackingReason` values: `MemoCommitment`, `SnsDiscovery`, `RelayTarget`, and `RelayInstance`. `tracked_canister_count` is the number of unique principals with at least one currently visible tracking reason. The specialized counts `memo_registered_canister_count`, `sns_discovered_canister_count`, `relay_target_canister_count`, and `relay_instance_canister_count` are per-reason counts and do not change the unique-principal rule for `tracked_canister_count`. Active self-service configurations add `RelayTarget` to every managed target and `RelayInstance` to the spawned Relay; surplus recipients of either type are not tracked or probed as targets. These reasons and cycles histories are independent; they do not form a target-to-Relay relationship.

### SNS discovery

When `enable_sns_tracking = true`, the historian periodically:

1. calls SNS-W (`qaa6y-5yaaa-aaaaa-aaafa-cai`) `list_deployed_snses`
2. calls each authoritative SNS Root's `list_sns_canisters`
3. adds the root, governance, ledger, swap, index, dapps, archives, and extensions to its tracked set with source `SnsDiscovery`
4. queues newly discovered members for the ordinary direct-first cycles probe

The discovery pass is intentionally chunked and resumable across ticks. The historian snapshots the deployed SNS root list once, then walks it in bounded batches using the same per-tick cap used by the cycles sweep. That keeps each run bounded even if the deployed SNS set grows materially over time.

SNS discovery supplies membership only. It writes no cycles sample or probe result. SNS-only and mixed-reason members participate in the same ordinary cycles sweep as every other target; SNS Root and Swap status remain fallbacks when direct status is unavailable.

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

## Check a Jupiter Faucet commitment

External consumers can call the production Historian at `j5gs6-uiaaa-aaaar-qb5cq-cai` using the anonymous `get_commitment_route_summaries` query to ask whether one or more exact Jupiter Faucet routes have qualifying commitments and how much ICP has been committed over their lifetime.

The two canister-route forms are:

- Jupiter memo `<canister>` → `CyclesTopUp { canister_id = <canister> }`
- Jupiter memo `<canister>.<memo>` → `RawIcp { destination_canister_id = <canister>; memo = <exact suffix bytes> }`

`<canister>` is **not** the same route as `<canister>.`. The trailing-dot form is `RawIcp` with an empty memo blob. Principal text is normalized by parsing it into Principal bytes, while suffix bytes are exact.

For example, Jupiter memo `vpa37giaaaaaaamqdxeqcai.5suig-4y` declares destination principal `vpa37-giaaa-aaaam-qdxeq-cai` and exact raw-ICP suffix bytes `5suig-4y`:

```bash
icp canister call j5gs6-uiaaa-aaaar-qb5cq-cai get_commitment_route_summaries \
  '(record {
    routes = vec {
      variant {
        RawIcp = record {
          destination_canister_id =
            principal "vpa37-giaaa-aaaam-qdxeq-cai";
          memo = blob "5suig-4y";
        }
      }
    };
  })' \
  --environment ic \
  --identity anonymous \
  --query
```

`total_qualifying_committed_e8s` is the lifetime qualifying ICP committed to that exact route; divide it by `100_000_000` for ICP. `qualifying_commitment_count` is the number of qualifying commitments included in that total. The response is authoritative through `indexed_through_staking_tx_id`.

**A zero total is authoritative only when `complete_from_genesis == true` and `commitment_index_fault == null`.** If either condition is not satisfied, treat the amount as unavailable or incomplete, not as zero.

Up to 100 exact routes can be queried in one call, so consumers can batch the records they display. The complete machine-readable public interface is [`jupiter_historian.did`](jupiter_historian.did).

## Self-service Relay configurations

Self-service setup accepts 1–20 external target canisters and either zero or 1–5 typed surplus recipients. Each recipient carries an exact 0–32-byte memo; empty means no outgoing Ledger memo. Targets are sorted by raw principal bytes. Recipients are ordered with Principals first in raw-byte order, followed by nonzero `u64` neuron IDs in numeric order, without sorting by memo. Duplicate destinations are rejected even when their memos differ, while one principal may still be both a target and a surplus Principal recipient. Principal recipients may be arbitrary non-anonymous, non-management principals. Recipient destinations do not receive target-only protected-dependency or probe checks. The two canonical vectors jointly determine identity, so input order does not matter and changing a recipient type, destination, or memo forms a different immutable configuration. The canonical production Relay target set remains reserved as an explicit policy preventing a self-service duplicate, regardless of supplied recipients.

Memory ID 26 is the one Relay setup map. Its key is the 32-byte canonical configuration hash and its value is the current `RelaySetupEntry`. Targets and Relay instances remain visible independently through generic tracking reasons and `list_canisters` filtering. A finalized child is immutable because it has zero controllers; it is never upgraded through Historian and is never queried to reconstruct configuration.

The deterministic setup subaccount is the 32-byte SHA-256 configuration hash. Every configuration uses the explicitly framed `jupiter-relay-configuration-v1\0` encoding documented in [`../../docs/relay-setup-recovery.md`](../../docs/relay-setup-recovery.md). Empty memos and zero recipients are encoded explicitly in that same format. Funding uses only aggregate ICP ledger `icrc1_balance_of`, with no index/history scan or payer attribution. Pricing remains target-based: notification computes `max(singleton nominal minimum, conversion + safety margin + configured seed + two ledger fees) + 0.25 ICP × extra targets`. Recipient count affects Relay's runtime allocation and transfer cap, not creation price. Deposits are not automatically discovered, refunded, or swept.

Child install arguments split canonical typed recipients into Relay's existing fields and copy each memo byte-for-byte: Principals become `SurplusCanisterRecipient` values and neuron IDs become `SurplusNeuronRecipient` values. Zero recipients produces `surplus_canister_recipients = null` plus an empty neuron vector and selects all-cycles allocation, which routes no raw ICP surplus. A Principal receives at `Account { owner: principal, subaccount: None }`; a neuron receives at `Account { owner: NNS Governance, subaccount: resolved staking subaccount }`, and Relay attempts `claim_or_refresh` after a successful neuron transfer. Historian first validates the setup balance and live current requirement, then reserves the configuration in the existing `Reserved` phase. While reserved, it independently confirms only configured neurons are public/readable and have valid staking subaccounts; zero-recipient and Principal-only configurations make no Governance lookup. A clean validation failure removes the reservation before target probes or spending. Relay repeats immutable recipient validation and resolves neurons at runtime. `max_transfers_per_tick` is `targets + recipients + Relay self` (at most 26, or `targets + 1` in all-cycles mode). No IO recipient is added automatically.

Creation is an explicit user action. A narrow same-key reservation prevents duplicate execution, all targets are probed before spend, transfer records are persisted before dispatch with fixed timestamps, and create dispatch is fail-closed. For a future child, direct status is reusable only when visibility is exactly `public`; Historian-only controller or `allowed_viewers` access is insufficient, although a recognized reusable blackhole/SNS fallback may qualify the target. Final activation requires exact pre-finalization and post-controller-removal audits of the approved module, running state, controller set, public logs, and public status. A transient direct-status failure after accepted Relay funding leaves `RelayFunded` or `FinalizationAttempted` in progress. Submitting the same canonical configuration can explicitly resume only this safe tail: it never repeats a Ledger transfer, CMC notification, child creation, installation, or Relay-funding transfer. Exact finalized live state activates without another settings update; exact `[Historian]` state permits one idempotent controller-removal attempt per explicit submission, while any unexpected live state remains terminal manual recovery. Frontend polling stays query-only. See [`../../docs/relay-setup-recovery.md`](../../docs/relay-setup-recovery.md) for the state and operational procedure.

On Historian upgrade, interrupted `Reserved` and `ProbingTargets` entries are removed. Every later `Creating` phase becomes terminal manual recovery with `HistorianUpgradeInterrupted`; active mappings and existing manual-recovery entries are preserved without calling any child.

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

### Stable route-roll-up state

Stable memory 29 is the authoritative lifetime commitment-route map and is preserved directly by normal in-place upgrades. The normal staking-account indexer is its only writer: a fresh Historian starts with `commitment_route_rollups_complete_from_genesis = Some(false)`, builds the map while indexing from genesis, and changes the marker to `Some(true)` only after genesis coverage is established.

Production is expected to report `complete_from_genesis = true` and `commitment_index_fault = null` before and after a routine upgrade. If completeness unexpectedly becomes false or an index fault is present, investigate the invariant violation rather than initiating a historical rebuild. Existing production Historian must be upgraded in place and must not be reinstalled.

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
- direct-first cycles sampling with blackhole/SNS fallbacks
- SNS membership discovery feeding the unified cycles probe
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

The checked-in production args enable `relay_factory_enabled = opt true`, so `jupiter_historian.wasm.gz` is the relay-enabled canonical production Historian artifact. Its embedded Relay install payload must correspond to the reviewed raw Relay Wasm from the same Docker/reproducible release build, recorded as `release-artifacts/jupiter_historian.reviewed-relay-wasm-raw.sha256`, and `release-artifacts/jupiter_relay.wasm.gz` must decompress to those reviewed raw bytes. Runtime self-service Relay reconciliation reads live child status and compares it with the approved installed-module hash. Historian does not persist per-instance expected Relay hashes. Self-service Relays use the canonical Relay daily cadence (`main_interval_seconds = 86400`), their submitted canonical targets, automatic probe routing, and their submitted canonical typed recipients with exact memo bytes. If a local no-relay artifact is needed for development or tests, build `jupiter-historian-no-relay`, which writes `release-artifacts/jupiter_historian_no_relay.wasm.gz`.

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

For the production maintenance and risk-based snapshot procedure, follow [`../../docs/operations/deployment.md`](../../docs/operations/deployment.md). Preserve memory-26 setup entries, exact active configuration mappings, `RelayTarget`/`RelayInstance` tracking, and memory-29 route roll-ups across every in-place upgrade. Record representative known configurations before upgrading and verify them afterwards.

## Debug interface

The production canister exposes the query interface described above.

Additional debug-only methods are gated behind the `debug_api` feature and are available only to local tests, PocketIC, and non-production debug builds. They are not a production preflight mechanism and must not be called against the production Historian. Debug builds also check the embedded production canister ID at runtime and reject debug API use when the canister principal is the production historian principal. The operational model treats that production-principal guard as sufficient: debug builds must not be installed on production canister IDs, production canister IDs reject debug API use, and a newly deployed canister with debug APIs is a separate non-production/debug deployment. No additional caller-authorization layer is desired for these debug surfaces. The debug Candid surface is committed at:

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
