# Deployment

Production deployment is a governance-controlled operation. Once Jupiter Faucet is under SNS DAO control, production upgrades are expected to pass through SNS community consensus before execution. During the initial bootstrap phase, upgrades may still be executed by the current bootstrap controller, but the release process should be documented and verifiable as if it will be reviewed by the community.

Use `icp deploy --environment ic` for ordinary production orchestration, and use canonical Docker artifacts when public reproducibility evidence matters.

Historian production deploys are factory-enabled. The checked-in mainnet historian args set `relay_factory_enabled = opt true`, so the canonical production historian deploy artifact is the relay-enabled `release-artifacts/jupiter_historian.wasm.gz`. Self-service Relays use the canonical Relay daily cadence (`main_interval_seconds = 86400`) with their canonical 1–20 target vector, automatic probe routing, and configured surplus recipient.

Existing production Historian must be upgraded in place. Reinstall destroys all Historian stable history and is prohibited for the existing production canister because it clears commitment histories, cycles histories, tracking metadata, active self-service hash mappings, setup progress, index cursors, aggregates, and other durable state. mainnet-install-args.did is for a brand-new Historian installation only; `canisters/historian/mainnet-install-args.did` must not be passed to an existing Historian upgrade.

## Production release flow

Generic release sequence for routine production upgrades:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
./tools/scripts/docker-build
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy <canister_name> --environment ic --mode upgrade
```

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

First deploy a maintenance frontend that prevents ordinary UI submissions. The maintenance frontend is not a security boundary. Stopping `jupiter_historian` is the executable self-service factory pause and the authoritative call drain for this deployment. Record all public query evidence while Historian is still running, then stop it and wait until its status is `Stopped` before creating/downloading the snapshot and performing the in-place upgrade. There is no runtime factory-disable update method.

For the first cutover, memory 25 is new, so there is no pre-existing `Creating` entry to enumerate. The upgrade's one-time retired-memory gate is authoritative. Retired registry memory must be empty or exactly the complete configured canonical Relay projection. Retired setup-job memory must be empty except for the one operator-authorized abandoned pre-launch job keyed by target `2lo52-kiaaa-aaaar-qaqta-cai`; that exception is accepted only when its reviewed safety-critical fingerprint matches. The fingerprint requires the old singleton setup identity, the exact `ManualRecoveryRequired` low-minted-cycles diagnostic, the completed CMC conversion at block `37414364`, the exact unresolved create attempt with no recorded child, and strict absence of installation, Relay funding, an existing-Relay sweep, or controller-handoff evidence. Abandoned payment, refund, cursor, amount, phase, and timestamp bookkeeping is not part of this safety decision. Every other retired setup job blocks the upgrade.

After all retired registry and setup invariants pass, the authorized job is removed from retired memory 24 and memory 24 is required to be logically empty. The job is not inserted into memory 25 and does not add `RelayTarget` or `RelayInstance` tracking. The old deterministic Ledger setup account and its remaining 105,140,000 e8s are intentionally abandoned: no cutover code reads, transfers, refunds, sweeps, burns, or reuses that balance. Ledger history and downloaded snapshots remain historical evidence; application-level map deletion does not erase either. If the upgrade or this gate fails, do not proceed; restore the snapshot using the rehearsed rollback procedure.

After canonical artifacts exist, run `./tools/scripts/preflight-historian-production-upgrade` for a read-only artifact and upgrade-path preflight before the maintenance window.

Record pre-upgrade query results before upgrading `jupiter_historian`:

- Historian module hash, controllers, and stable memory size from `icp canister status j5gs6-uiaaa-aaaar-qb5cq-cai -n ic`.
- `get_public_counts` and `get_public_status`.
- All `list_canisters` pages needed to cover tracked targets, canonical Relay, and self-service Relays.
- Representative commitment histories and cycles histories for known targets.
- Indexing cursors, fault state, aggregate output/reward/burn totals, and factory enabled state.

The production Historian does not expose `debug_api`, so debug setup-entry listing is not a production preflight mechanism. Before later upgrades, record active mappings through known exact target sets with `get_relay_setup_view`; production cannot enumerate every target-set hash.

Before the maintenance window, confirm the live production Historian is on a supported direct-upgrade path. The current release retains the V1 stable decoder because the repository does not contain sufficient production evidence to raise the minimum supported upgrade version. Removing the decoder requires a separately reviewed change that records the production compatibility evidence, snapshot policy, and new minimum supported direct-upgrade version. Rollback procedures must use the snapshot created during the maintenance window; do not rely on restoring a pre-migration snapshot after upgrading to current code.

Maintenance sequence tested with `icp 0.2.6`:

```bash
icp canister stop jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
SNAPSHOT_ID="$(icp canister snapshot create jupiter_historian --environment ic --quiet)"
icp canister snapshot list jupiter_historian --environment ic --json
icp canister snapshot download jupiter_historian "$SNAPSHOT_ID" --environment ic --output /tmp/jupiter-historian-snapshot-"$SNAPSHOT_ID"
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian --environment ic --mode upgrade
icp canister start jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
```

The stopped-canister upgrade behavior above is intentional: local testing showed `icp deploy <name> --environment local --mode upgrade` succeeds while the canister is stopped and leaves it `Stopped`, so the explicit `icp canister start` is required after the upgrade.

Rollback from the recorded snapshot ID:

```bash
icp canister stop jupiter_historian --environment ic
icp canister snapshot restore jupiter_historian "$SNAPSHOT_ID" --environment ic
icp canister start jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
```

After upgrade, verify:

- Module hash matches the canonical `release-artifacts/jupiter_historian.wasm.gz` package hash.
- Controllers are unchanged.
- Counts, cursors, totals, recent feeds, and historical commitment/cycles samples are preserved.
- For the first cutover, retired memory 24 is logically empty, the new memory-25 setup map opens empty, and the abandoned singleton target returns a fresh `NotFunded` setup view with an account different from the retired singleton account.
- For later upgrades, known exact target sets still return their original active Relay mappings.
- Automatic cycles probing is active.
- `RelayTarget` and `RelayInstance` tracking reasons and counts are preserved independently.
- New cycles samples append to existing histories.
- Any setup interrupted after an irreversible spend is `ManualRecoveryRequired`; interrupted `Reserved`/`ProbingTargets` entries are removed, while existing `Active` and `ManualRecoveryRequired` entries are preserved.

Deploy the frontend only after backend verification is complete.

The pre-launch cutover sequence is:

1. Deploy the maintenance frontend that prevents ordinary UI submissions.
2. Record all pre-upgrade public query evidence while Historian is still running.
3. Stop Historian and wait until its status is `Stopped`.
4. Create and download a snapshot.
5. Upgrade Historian in place; do not reinstall it. The one-time retired-memory gate must accept the state.
6. Start Historian.
7. Verify preserved public state and the new setup API.
8. Perform one controlled singleton setup and one overlapping multi-target setup.
9. Verify each child module hash, running status, Fiduciary-only controller set, and independent tracking counts.
10. Retain the snapshot until acceptance is complete, then restore normal UI access.

For later Historian upgrades:

1. Deploy the maintenance frontend.
2. Record public state and known exact target-set mappings.
3. Stop Historian and wait until its status is `Stopped`.
4. Create and download a snapshot.
5. Upgrade Historian in place.
6. Start Historian.
7. Verify active mappings through the known exact target sets.
8. Verify `RelayTarget` and `RelayInstance` counts.
9. Verify any interrupted post-spend setup is `ManualRecoveryRequired`.
10. Re-enable normal UI access.

Self-service children are blackholed and are never upgraded or reconstructed by Historian.

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
| `jupiter_relay` | `InitArgs` from checked-in `mainnet-install-args.did` | Full `InitArgs` | Checked-in reviewed full `InitArgs` from `canisters/relay/mainnet-install-args.did` | Heap-only replacement; config and operational state reset; non-resumable |
| `jupiter_faucet_frontend` | No install args | No args | No args | Asset canister state managed by frontend asset lifecycle |
| `jupiter_lifeline` | No install args | No args | No args | Minimal support canister state |
| `jupiter_sns_rewards` | No install args | No args | No args | Placeholder/reward-recipient canister state |

Relay upgrades are replacement-style and non-resumable. Avoid upgrading during active Relay work where practical. If an in-flight operation is interrupted, Relay starts fresh from the supplied `InitArgs`. After upgrade, confirm the fresh `CONFIG` log, confirm the first successful tick is `BaselineOnly`, check managed canister cycle balances, and manually top up or reconcile if needed.

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

For module-hash verification and deterministic rebuild checks, see [reproducible builds](reproducible-builds.md).

## Troubleshooting

A failed upgrade with an error such as `received InitArgs in post_upgrade` means the canister rejected the wrong argument shape. For Disburser, Faucet, and Historian, rebuild the command using the canister's current `UpgradeArgs` shape instead of the fresh-install `InitArgs` file. For Relay, supply the full Relay `InitArgs`.
