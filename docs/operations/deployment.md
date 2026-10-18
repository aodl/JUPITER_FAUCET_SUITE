# Deployment

Production deployment is a governance-controlled operation. Once Jupiter Faucet is under SNS DAO control, production upgrades are expected to pass through SNS community consensus before execution. During the initial bootstrap phase, upgrades may still be executed by the current bootstrap controller, but the release process should be documented and verifiable as if it will be reviewed by the community.

Use `icp deploy --environment ic` for ordinary production orchestration, and use canonical Docker artifacts when public reproducibility evidence matters.

Historian production deploys are factory-enabled. The checked-in mainnet historian args set `relay_factory_enabled = opt true`, so the canonical production historian deploy artifact is the relay-enabled `release-artifacts/jupiter_historian.wasm.gz`. Self-service Relays use the canonical daily cadence with canonical 1–20 targets, automatic probe routing, and either zero or 1–5 submitted typed recipients carrying exact 0–32-byte memos. Zero recipients selects all-cycles mode; otherwise Principals are paid equally at default ICP accounts and public NNS neuron IDs at resolved Governance staking accounts. Pricing remains target-based; no IO recipient is added automatically.

Existing production Historian must be upgraded in place. Reinstall destroys all Historian stable history and is prohibited for the existing production canister because it clears endowment histories, cycles histories, tracking metadata, active self-service hash mappings, setup progress, index cursors, aggregates, and other durable state. mainnet-install-args.did is for a brand-new Historian installation only; `canisters/historian/mainnet-install-args.did` must not be passed to an existing Historian upgrade.

## Controller and release authority

Record installed hashes, controller vectors, public status/log visibility, account destinations, cycles and capacity before any operator action. Verify public management status through a non-controller caller, not merely a state-tree lookup. Release acceptance does not authorize settings changes or autonomous-rescue arming. Controller handoff is a separate reviewed governance operation; production rescue remains unarmed in the checked-in fresh-install configuration. Community blackholes remain funding targets independently of controller relationships.

## Readiness and recovery boundaries

The developer's unused-factory inventory applies to self-service setup, not to
canonical Faucet/Historian accounts or history. This release preserves those
live roots and authoritative maps. Faucet's memory 2 records one pending
transfer's dispatch provenance without changing its strict V1 root. Missing
provenance is conservative (possibly dispatched), never proof of no debit.
Deploy Faucet only at its existing active-payout quiescent boundary. A possible
debit consumes its tranche budget even if later rejected retries cannot resolve
it; unused real funds can remain conservatively held without being counted as
confirmed delivery. No balancing transfer or automatic recovery replay exists.

The factory's authoritative setup view is a consensus-backed update, not a
payment-authorizing query. Coordinate backend/frontend method mode in one
maintenance window; an authenticated read failure must leave funding controls
disabled. Factory admission is bounded and best effort, with a separate
completion allowance; see [factory limits](../relay-setup-recovery.md).

Controllerless children remain immutable. Updating Historian's embedded Relay
changes new children only. Existing canonical Relay and factory-template
identities must be recorded separately rather than inferred from one another.

Lifeline is only a reserved recovery-authority principal. Its present module
has no recovery methods and cannot itself execute a rescue upgrade. Recovery
requires the actual authorized controller/governance route, a separately
reviewed failure-specific artifact and arguments, preserved state inventory,
and a local rehearsal of the proposed operation. Existing controller-roundtrip,
upgrade, factory-finalization and interruption tests exercise those supported
boundaries, not a hypothetical universal rescue. An ambiguous external spend,
lost child ID, mismatched child module or `ManualRecoveryRequired` record needs
investigation; neither a canister snapshot nor a Lifeline upgrade rolls back
Ledger/CMC/management effects. Do not add a hidden child controller or reinstall
a stateful canonical canister as a shortcut.

## Production release flow

Freeze and review source commits before collecting release evidence. The local
release gates are not deployment authorization:

```bash
POCKET_IC_MUTE_SERVER=1 cargo run --locked -p xtask -- test_all
./tools/scripts/docker-build
sha256sum -c release-artifacts/release-artifacts.sha256
python3 ./tools/scripts/validate-mainnet-install-args
POCKET_IC_MUTE_SERVER=1 cargo run --locked -p xtask -- release_artifact_validation
./tools/scripts/check-production-reachability
./tools/scripts/preflight-historian-production-upgrade
./tools/scripts/security-scan
npm run verify:reproducible-artifacts
```

Retain exact command exits, source identity, tool versions, logs and raw/gzip
hashes outside the source tree. Preserve earlier evidence before Docker replaces
`release-artifacts/`. Security runs after the build so SBOMs are not erased.
The verifier compares two clean same-environment builds with each other and the
canonical candidate, including build metadata; this is not independent-toolchain
or installed-mainnet equality. An absent artifact, skipped required suite,
unapproved hash or failed gate blocks release. See the required CI status names
and operator-only enforcement steps in [testing](../development/testing.md).
Only the separately authorized operator may then execute the component's
reviewed deployment procedure below.

`./tools/scripts/docker-build` produces canonical `.wasm.gz` install packages and `release-artifacts/release-artifacts.sha256`. `JUPITER_USE_CANONICAL_ARTIFACTS=1` tells the `icp.yaml` build helper to verify that manifest and deploy those existing packages instead of rebuilding with the local toolchain.

Routine no-config Historian upgrades pass no arguments:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian \
  --environment ic \
  --mode upgrade
```

Config-changing Historian upgrades use a temporary Option<UpgradeArgs> file. Do not commit that file, and do not use `canisters/historian/mainnet-install-args.did` for an upgrade:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian \
  --environment ic \
  --mode upgrade \
  --args-file /tmp/historian-upgrade-args.did
```

Keep canonical Relay lifecycle instructions separate. Relay is replacement-style and requires full `InitArgs` on every upgrade:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_relay \
  --environment ic \
  --mode upgrade \
  --args-file canisters/relay/mainnet-install-args.did
```

For unrelated routine no-config-change production upgrades, pass no args for Disburser, Faucet, and Historian. Those stateful canisters decode omitted upgrade args as no config change. Relay is replacement-style and requires full `InitArgs` on every upgrade.

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet \
  --environment ic \
  --mode upgrade
```

Do not pass `canisters/historian/mainnet-install-args.did` to an already-installed Historian upgrade.

For fresh install only, use the checked-in `mainnet-install-args.did` `InitArgs` file:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet \
  --environment ic \
  --mode install \
  --args-file canisters/faucet/mainnet-install-args.did
```

For reinstall, use the same pattern with `--mode reinstall` only after confirming state may be discarded:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet \
  --environment ic \
  --mode reinstall \
  --args-file canisters/faucet/mainnet-install-args.did
```

Reinstall clears canister Wasm/stable state. It is not an ordinary upgrade path.

## Historian upgrade audit checklist

### State-preserving maintenance

Historian restores required current accounting/configuration fields or fails before persistence. It preserves lifetime totals exactly, including during pending backfill. Retained samples cannot reconstruct missing lifetime accounting. After indexing starts, accounting accounts, Ledger, Index and threshold cannot change; operational tuning remains available. Review the current-state upgrade regression and installed predecessor identity before maintenance.

Faucet requires an empty active-payout journal at upgrade. Disburser requires its documented maturity/payout quiescence. SNS Rewards must restore its persisted configuration, active snapshot, staging scan and owner maps; missing state is an upgrade failure. Reserved stable-memory identifiers remain reserved.

Stopping `jupiter_historian` is the executable self-service factory pause and authoritative call drain. Record public evidence, stop it, and wait for `Stopped` before an in-place upgrade. Keep it stopped while deploying a matching frontend when the two Candid declarations change together, then explicitly start Historian after both upgrades succeed. There is no runtime factory-disable update method.

Self-service Relay state is legitimate durable production state once the factory is used: memory-26 entries, exact active configuration mappings, `RelayTarget`/`RelayInstance` tracking and the bounded memory-30 Creating-key index are authoritative. Preserve both memories together. Before future upgrades, record representative known full configurations and their Relay mappings; production cannot enumerate every configuration hash. Verify those mappings and tracking counts afterwards.

After canonical artifacts exist, run `./tools/scripts/preflight-historian-production-upgrade` for a read-only artifact and upgrade-path preflight. Record:

- Historian module hash, controllers, and stable memory size from `icp canister status j5gs6-uiaaa-aaaar-qb5cq-cai -n ic`.
- `get_public_counts`, `get_public_status`, and the empty-route endowment health query below. Compare the recorded counts and totals before and after the upgrade. `qualifying_commitment_count` must not decrease across the upgrade; an increase after restart/indexing is permitted and may represent newly indexed qualifying endowments. Bounded retained histories must never be used to reconstruct or reduce the lifetime count.
- All `list_canisters` pages needed to cover tracked targets, canonical Relay, and self-service Relays.
- Representative endowment/cycles histories, known exact Relay configuration mappings, indexing cursors, fault state, and aggregate totals.

Routine maintenance sequence tested with `icp 0.2.6`:

```bash
icp canister stop jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian --environment ic --mode upgrade
icp canister start jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
```

Local testing showed that upgrading a stopped canister leaves it stopped, so the explicit start command is required.

Immediately after post-upgrade restore, public queries must already fail closed when historical index coverage is unsupported and must remain degraded while historical route backfill is incomplete. After starting Historian, allow the normal initial or forced main tick to run before making the final assertion that indexing has resumed normally.

After upgrade, verify:

- The module hash matches the canonical `release-artifacts/jupiter_historian.wasm.gz` package hash and controllers are unchanged.
- Unrelated counts, cursors, totals, recent feeds, and representative historical samples are preserved. `qualifying_commitment_count` must not decrease across an upgrade; an increase after restart/indexing is permitted and may represent newly indexed qualifying endowments.
- Memory-26 entries, known exact active configuration mappings, and `RelayTarget`/`RelayInstance` tracking are preserved.
- Automatic cycles probing and normal staking indexing continue, and new samples/endowments append exactly once.
- Interrupted `CmcTransferAccepted` and `CmcNotifySucceeded` entries retain paid evidence for explicit serialized continuation. Other post-spend interrupted phases become `ManualRecoveryRequired`; pre-spend reservations are removed. Active mappings and existing manual evidence are preserved.

### Historian index health

```bash
icp canister call jupiter_historian get_public_status '()' --environment ic --identity anonymous --query
icp canister call jupiter_historian get_public_counts '()' --environment ic --identity anonymous --query
```

Memory 29 is ordinary authoritative Historian state and is preserved directly by normal in-place upgrades. Query route metadata anonymously before and after an upgrade; an empty route vector is sufficient:

```bash
icp canister call jupiter_historian get_commitment_route_summaries \
  '(record { routes = vec {} })' \
  --environment ic \
  --identity anonymous \
  --query
```

Healthy production requires this query to report `complete_from_genesis = true` and `commitment_index_fault = null`, and the already-recorded `get_public_status()` response to report `route_index_fault = null`, before and after a routine upgrade. The first pair verifies staking/endowment indexing; `route_index_fault` verifies output/rewards indexing.

Unsupported historical pagination fails closed; it does not trigger an automatic rebuild or counter reset. Current newest-first backfills keep `route_index_fault` non-null and totals explicitly partial until complete coverage is established.

Do not reinstall Historian as a recovery mechanism. After any in-place upgrade, require `complete_from_genesis = true`, `commitment_index_fault = null`, and `route_index_fault = null` before treating Historian as healthy.

### High-risk upgrade and rollback

Snapshots are risk-based. A snapshot is strongly recommended, and may be made a release requirement, when an upgrade materially changes the stable schema, stable-memory layout, migration logic, authoritative historical state, or another rollback-sensitive area. Routine low-risk upgrades that preserve those contracts and have strong upgrade/PocketIC coverage do not require a snapshot as a universal repository invariant.

For a high-risk maintenance window, create and record the snapshot before the upgrade:

```bash
icp canister stop jupiter_historian --environment ic
SNAPSHOT_ID="$(icp canister snapshot create jupiter_historian --environment ic --quiet)"
icp canister snapshot list jupiter_historian --environment ic --json
icp canister snapshot download jupiter_historian "$SNAPSHOT_ID" --environment ic --output /tmp/jupiter-historian-snapshot-"$SNAPSHOT_ID"
```

Rollback from that recorded snapshot ID only after reviewing why the candidate failed:

```bash
icp canister stop jupiter_historian --environment ic
icp canister snapshot restore jupiter_historian "$SNAPSHOT_ID" --environment ic
icp canister start jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
```

## Production canister IDs

Keep the named `icp deploy` command and the production principal together during review:

| Canister name | Production principal |
| --- | --- |
| `jupiter_disburser` | `uccpi-cqaaa-aaaar-qby3q-cai` |
| `jupiter_faucet` | `acjuz-liaaa-aaaar-qb4qq-cai` |
| `jupiter_historian` | `j5gs6-uiaaa-aaaar-qb5cq-cai` |
| `jupiter_relay` | `u2qkp-aqaaa-aaaar-qb7ea-cai` |
| `jupiter_lifeline` | `afisn-gqaaa-aaaar-qb4qa-cai` |
| `jupiter_sns_rewards` | `alk7f-5aaaa-aaaar-qb4ra-cai` |
| `jupiter_faucet_frontend` | `jufzc-caaaa-aaaar-qb5da-cai` |

## Fresh installs vs upgrades

Fresh install argument files live with their owning canisters:

- [`canisters/disburser/mainnet-install-args.did`](../../canisters/disburser/mainnet-install-args.did)
- [`canisters/faucet/mainnet-install-args.did`](../../canisters/faucet/mainnet-install-args.did)
- [`canisters/historian/mainnet-install-args.did`](../../canisters/historian/mainnet-install-args.did)
- [`canisters/relay/mainnet-install-args.did`](../../canisters/relay/mainnet-install-args.did)
- [`canisters/sns-rewards/mainnet-install-args.did`](../../canisters/sns-rewards/mainnet-install-args.did)

`mainnet-install-args.did` files are fresh-install/reinstall `InitArgs`. Do not use those files for ordinary production upgrades, except Relay where upgrades intentionally require full replacement `InitArgs`.

> Warning:
> Do not pass `canisters/<name>/mainnet-install-args.did` to `--mode upgrade`
> for Disburser, Faucet, or Historian. Those files are fresh-install `InitArgs`
> and may be intentionally rejected during `post_upgrade`. Relay is the
> exception: Relay upgrades require full `InitArgs`.

For config-changing upgrades, Disburser, Faucet, and Historian use the canister's current `UpgradeArgs` shape from source and keep the args file temporary. Relay has no `UpgradeArgs`; Relay config-changing upgrades update and review the checked-in full `InitArgs` file at `canisters/relay/mainnet-install-args.did`.

## Lifecycle matrix

| Canister | Fresh install/reinstall | Routine no-config-change upgrade | Config-changing upgrade | State behavior |
| --- | --- | --- | --- | --- |
| `jupiter_disburser` | `InitArgs` from checked-in `mainnet-install-args.did` | No args | Temporary `Option<UpgradeArgs>` | Stable state preserved |
| `jupiter_faucet` | `InitArgs` from checked-in `mainnet-install-args.did` | No args | Temporary `Option<UpgradeArgs>` | Stable state preserved |
| `jupiter_historian` | `InitArgs` from checked-in `mainnet-install-args.did` for brand-new canister only | No args | Temporary `Option<UpgradeArgs>` | Stable state preserved; existing production canister must not be reinstalled |
| `jupiter_relay` | `InitArgs` from checked-in `mainnet-install-args.did` | Full `InitArgs` | Checked-in reviewed full `InitArgs` from `canisters/relay/mainnet-install-args.did` | Heap configuration/ordinary operations reset; compact SNS reward state and fixed-splitter journal preserved |
| `jupiter_faucet_frontend` | No install args | No args | No args | Asset canister state managed by frontend asset lifecycle |
| `jupiter_lifeline` | No install args | No args | No args | Minimal support canister state |
| `jupiter_sns_rewards` | `InitArgs` with optional SNS Root | No args | Temporary nested `Option<UpgradeArgs>` | Stable configuration, active owner snapshot, staging scan, and cursor preserved when Root is unchanged |

Relay ordinary default-account allocation and subaccount-1 work remain replacement-style and non-resumable. Avoid upgrading during active Relay work where practical. If ordinary ICP work is interrupted, Relay starts fresh from supplied `InitArgs`. Stable memory 0 preserves the latest completed reward cadence timestamp and any compact self-consuming multi-recipient payout with its exact transfer identities and progress; reward attribution itself has no historical cursor. The deployed V3 label is retained, while Candid record-width compatibility discards removed fields from the earlier wider V3 record. There is no V1/V2 migration implementation. Stable memory 1 preserves any pinned fixed-splitter execution transaction and quarantine evidence. An active splitter requires the replacement `InitArgs` to name the same ICP Ledger. After upgrade, confirm the fresh `CONFIG` log, stable state, first successful `BaselineOnly` ordinary allocation tick and managed-canister cycle balances.

Ordinary Relay infrastructure allocations and SNS reward distributions are perpetual best-effort services. Individual transfers may be abandoned after bounded duplicate-safe attempts when their outcome cannot be determined. The design deliberately prefers bounded state, continued liveness, auditability and confidence in an immutable implementation over exact recovery of every individual payment. Unmet infrastructure need is carried forward through Relay recovery deficits and prioritized against future perpetual funding before normal surplus distribution. For SNS rewards, exact duplicate execution of a pinned transfer identity remains prevented, but a material fee change or insufficient balance may abandon a partially completed plan. Residual tokens may then enter a fresh adjudication based on current Ledger/history state, without excluding a recipient already paid by the abandoned plan. Operators must therefore treat cumulative recipient amounts across that boundary as best-effort rather than an exact reproduction of the original pot's ideal pro-rata split. This bounded-liveness policy is deliberate: discrepancies never become indefinite recovery obligations or halt later rewards. By contrast, fixed splitter transfers and irreversible Historian child-creation/controller-removal operations keep stronger durable duplicate-safe or fail-closed state because their finite economics and security boundaries differ.

## SNS rewards lifecycle and Root switch

`jupiter_sns_rewards` discovers Governance and Ledger from its one configured SNS Root. The checked-in fresh-install argument configures OpenChat Root `3e3x2-xyaaa-aaaaq-aaalq-cai` only as a development placeholder. No Governance, CHAT Ledger, or future jUP Ledger ID is separately configured.

Before enabling the canonical Relay reward path, verify that recent canonical Relay subaccount-1 history contains supported debit shapes and reconciles into completed Faucet endowments. Attribution paginates backwards on demand within one shared per-attempt allowance: 64 external calls, 16,384 retained history records, 8 MiB of encoded history and 2,000,000 charged work units, with a 120-second deadline and main-lease fencing. Nested splitter scans, owner-resolution chunks and repeated reconstruction share that allowance. Core funding runs first and records completed heap cadence before the optional reward stage's first await, while retaining the exclusive lease. This survives a later reward callback trap, not a replacement-style Relay upgrade. An exhausted attempt holds rewards without paying from incomplete attribution or consuming successful weekly cadence; it can defer indefinitely, retrying on an accepted ordinary funding tick rather than in a tight loop. Pinned payout identities/progress remain pinned and receive prompt one-second timer slices under the original lease. Each further slice must advance the recipient cursor; lease expiry bounds continuation. Weekly attribution is not repeated for that plan. A deadline does not cancel an already dispatched call or guarantee an exact wall-clock duration under arbitrary network delay.

Source ownership is resolved in deterministic API-sized chunks against one snapshot. Relay inverts each account's indexed transactions from the response's current balance and trusts a suffix only when it proves a zero opening balance. Genuine account genesis means that no older transactions exist, while the reconstructed opening balance must still reconcile exactly to zero. This proof includes post-cutoff account activity and applies independently to subaccount 1 and every referenced splitter. Older history is fetched only when FIFO carry or newer ineligible endowments require it, while pagination and reconstruction inconsistencies fail closed. A history that cannot establish that proof inside the attempt allowance is unsupported for that attempt, not truncated or treated as empty.

Relay also reads the ICP Ledger `query_blocks(start=0, length=0).chain_length` before the ICP Index `status().num_blocks_synced` and requires the Index to cover the observed Ledger prefix. Index lag therefore cannot hide a net-zero newer deposit-plus-endowment pair and redirect the payout to an older endowment; an unavailable or behind Index retries daily without consuming weekly cadence.

Also verify that the context's pinned SNS Root returns the same Root and reward Ledger from `list_sns_canisters`, returns an Index, and that the Index `ledger_id` names that reward Ledger. Relay reconstructs its live reward balance from that Index back to the latest zero-balance boundary or account genesis, where genesis proves that no older transactions exist and the reconstructed opening balance must still be zero, then FIFO-replays incoming credits against outgoing payouts and fees. The oldest remaining non-zero reward-credit Ledger block time and owner-snapshot scan-start time form the effective exclusive ICP cutoff. There is no persistent reward-history cursor; the same attempt budget covers both token and ICP provenance. Detectable Index lag, including any live/index balance mismatch, retries on the next accepted daily tick without consuming weekly cadence. An undetectable unindexed net-zero outgoing-plus-incoming suffix can conservatively make the reconstructed cutoff older, but cannot make it newer than the fully synchronized cutoff. Funding performed earlier in the tick cannot replace that historical cutoff or capture an older reward pot.

Use this deployment order for the first usable reward epoch:

1. Upgrade or configure `jupiter_sns_rewards`.
2. Wait for a complete fresh owner snapshot.
3. Verify `get_relay_reward_context` exposes the expected Root-derived Ledger, and Root exposes its matching Index whose `ledger_id` names that Ledger.
4. Only then upgrade Relay.

This lets Relay begin adjudication immediately with usable owner context. If context is nevertheless unavailable, the failed attempt does not consume the weekly cadence and Relay retries on its next daily main tick.

Routine upgrades pass no argument and preserve configuration, active/staging maps, published snapshot, and an incomplete scan cursor. Configuration-changing upgrades pass an optional `UpgradeArgs` record. The nested field semantics are:

| Argument | Result |
| --- | --- |
| no argument or outer `null` | preserve Root |
| outer `opt record { reward_sns_root_canister_id = null }` | preserve Root |
| outer `opt record { reward_sns_root_canister_id = opt null }` | clear Root and invalidate both owner maps |
| outer `opt record { reward_sns_root_canister_id = opt opt principal "..." }` | replace Root and invalidate both owner maps |

During the 2026-09-11 rollout, the former empty production placeholder was configured with the temporary OpenChat Root using a reviewed argument file containing:

```did
(opt record {
  reward_sns_root_canister_id = opt opt principal "3e3x2-xyaaa-aaaaq-aaalq-cai";
})
```

That first configuration is complete. OpenChat remains the temporary development configuration; the later switch to the reviewed jUP SNS Root is a separate future operation. Operational verification still requires `SNS_REWARD_SCAN status=completed` and `get_relay_reward_context` to confirm the Root, Root-resolved Governance/Ledger, snapshot ID, and scan timestamps. Relay `CONFIG` logs must show canonical SNS-rewards canister `alk7f-5aaaa-aaaar-qb4ra-cai` and ICP Index `qhbym-qaaaa-aaaaa-aaafq-cai`.

Before switching from OpenChat to jUP:

1. Confirm every Relay has no in-flight SNS reward payout.
2. Reconcile any remaining development CHAT balance.
3. Upgrade `jupiter-sns-rewards` with a temporary nested argument naming the reviewed jUP SNS Root.
4. Wait for the first complete jUP owner snapshot.
5. Verify the context exposes the expected Root-derived jUP Ledger.
6. Allow Relay's next fresh adjudication to use the new completed context.

A Root switch clears both owner maps and prevents OpenChat ownership from remaining public. Relay has no main, splitter, or reward-token attribution cursor and never caches the reward Ledger or Index ID. An already-pinned payout keeps its original Ledger and exact identities until it self-consumes through payment or bounded abandonment; a fresh adjudication resolves the new Index through the new Root. A normal SNS Ledger fee change requires no Relay configuration upgrade because every new sweep resolves context and reads the live token fee. The release review must cover the SNS rewards Wasm, both production Candid interfaces, install arguments, stable-memory compatibility, and Relay's stable-state upgrade behavior before any production change.

## Local development builds

For fast local release artifacts and inspection, use:

```bash
./tools/scripts/build-canister all
```

When the checked-in mainnet args enable the relay factory, `build-canister all` produces the relay-enabled `release-artifacts/jupiter_historian.wasm.gz` for the production Historian path. If a local no-relay artifact is needed for development or tests, explicitly request `./tools/scripts/build-canister jupiter-historian-no-relay`, which writes `release-artifacts/jupiter_historian_no_relay.wasm.gz`.

For local-toolchain deployment orchestration, omit `JUPITER_USE_CANONICAL_ARTIFACTS`:

```bash
icp deploy jupiter_faucet --environment ic --mode upgrade
```

This runs the configured build step locally and installs the resulting `.wasm.gz` package. It is convenient, but it is not a canonical reproducible-build boundary.

## Verification

After deployment, compare the live module hash with the `.wasm.gz` SHA-256 from the canonical Docker build. This verifies the installed package hash; runtime configuration must still be verified separately from public logs and canister-specific README checklists.

Canister-specific README sections define production canister IDs, artifact names, lifecycle argument usage, and verification commands:

- [`canisters/disburser/README.md`](../../canisters/disburser/README.md)
- [`canisters/faucet/README.md`](../../canisters/faucet/README.md)
- [`canisters/historian/README.md`](../../canisters/historian/README.md)
- [`canisters/relay/README.md`](../../canisters/relay/README.md)

For Relay fee verification, query the ICP ledger and compare the result with `fee_e8s` in a recent `RELAY_SUMMARY`:

```bash
icp canister call ryjl3-tyaaa-aaaaa-aaaba-cai icrc1_fee '()' --environment ic
```

Also inspect Relay error logs for `ledger_fee_fallback` with a `cached` or `bootstrap` source, and for `ledger_fee_changed`. A normal ICP ledger fee change should not require a Relay configuration upgrade.

For module-hash verification and deterministic rebuild checks, see [reproducible builds](reproducible-builds.md).

## Troubleshooting

A failed upgrade with an error such as `received InitArgs in post_upgrade` means the canister rejected the wrong argument shape. For Disburser, Faucet, and Historian, rebuild the command using the canister's current `UpgradeArgs` shape instead of the fresh-install `InitArgs` file. For Relay, supply the full Relay `InitArgs`.
