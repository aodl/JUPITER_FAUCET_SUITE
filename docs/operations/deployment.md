# Deployment

Production deployment is a governance-controlled operation. Once Jupiter Faucet is under SNS DAO control, production upgrades are expected to pass through SNS community consensus before execution. During the initial bootstrap phase, upgrades may still be executed by the current bootstrap controller, but the release process should be documented and verifiable as if it will be reviewed by the community.

Use `icp deploy --environment ic` for ordinary production orchestration, and use canonical Docker artifacts when public reproducibility evidence matters.

Historian production deploys are factory-enabled. The checked-in mainnet historian args set `relay_factory_enabled = opt true`, so the canonical production historian deploy artifact is the relay-enabled `release-artifacts/jupiter_historian.wasm.gz`. Self-service Relays use the canonical daily cadence with canonical 1–20 targets, automatic probe routing, and either zero or 1–5 submitted typed recipients carrying exact 0–32-byte memos. Zero recipients selects all-cycles mode; otherwise Principals are paid equally at default ICP accounts and public NNS neuron IDs at resolved Governance staking accounts. Pricing remains target-based; no IO recipient is added automatically.

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

Stopping `jupiter_historian` is the executable self-service factory pause and authoritative call drain. Record public evidence, stop it, and wait for `Stopped` before snapshot and in-place upgrade. Keep it stopped while deploying the matching frontend because the current Historian and frontend Candid declarations must agree. Start Historian only after both upgrades succeed. There is no runtime factory-disable update method.

This release has a clean-slate precondition: public counts, every `RelayInstance` and `RelayTarget` page, known operational records, known frontend launch status, and an offline inspection of the downloaded Historian snapshot when tooling permits must show no self-service Relay, in-progress setup, manual-recovery entry, or known funded setup account. The configured canonical Relay and its configured targets are expected and must be distinguished from self-service tracking. If contrary evidence appears, stop the release and investigate; do not reinstall Historian or add an ad hoc compatibility path.

Memory 26 is the sole current self-service Relay setup map. All unrelated Historian stable state is preserved, so upgrade in place remains mandatory. If a precondition or audit fails, restore the maintenance-window snapshot.

After canonical artifacts exist, run `./tools/scripts/preflight-historian-production-upgrade` for a read-only artifact and upgrade-path preflight before the maintenance window.

Record pre-upgrade query results before upgrading `jupiter_historian`:

- Historian module hash, controllers, and stable memory size from `icp canister status j5gs6-uiaaa-aaaar-qb5cq-cai -n ic`.
- `get_public_counts` and `get_public_status`.
- All `list_canisters` pages needed to cover tracked targets, canonical Relay, and self-service Relays.
- Representative commitment histories and cycles histories for known targets.
- Indexing cursors, fault state, aggregate output/reward/burn totals, and factory enabled state.

The production Historian does not expose `debug_api`, so debug setup-entry listing is not a production preflight mechanism. Before later upgrades, record active mappings through known exact full configurations with `get_relay_configuration_view`; production cannot enumerate every configuration hash.

Before the maintenance window, confirm the live production Historian is on the repository's supported direct-upgrade path. Rollback procedures must use the snapshot created during that same maintenance window.

Maintenance sequence tested with `icp 0.2.6`:

```bash
icp canister stop jupiter_historian --environment ic
icp canister status jupiter_historian --environment ic --json
SNAPSHOT_ID="$(icp canister snapshot create jupiter_historian --environment ic --quiet)"
icp canister snapshot list jupiter_historian --environment ic --json
icp canister snapshot download jupiter_historian "$SNAPSHOT_ID" --environment ic --output /tmp/jupiter-historian-snapshot-"$SNAPSHOT_ID"
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian --environment ic --mode upgrade
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet_frontend --environment ic --mode upgrade
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
- Current entries in the memory-26 Relay setup map preserve their exact keys and states.
- Known exact full configurations still return their original active Relay mappings.
- Automatic cycles probing is active.
- `RelayTarget` and `RelayInstance` tracking reasons and counts are preserved independently.
- New cycles samples append to existing histories.
- Any setup interrupted after an irreversible spend is `ManualRecoveryRequired`; interrupted `Reserved`/`ProbingTargets` entries are removed, while existing `Active` and `ManualRecoveryRequired` entries are preserved.

Deploy the matching frontend while Historian remains stopped after its successful upgrade. Start Historian only after both deployments succeed.

The maintenance sequence is:

1. Record all pre-upgrade public query evidence while Historian is still running.
2. Stop Historian and wait until its status is `Stopped`.
3. Create and download a snapshot.
4. Upgrade Historian in place; do not reinstall it.
5. While Historian remains stopped, deploy the matching frontend.
6. Start Historian only after both deployments succeed.
7. Verify preserved public state and the current setup API with both canonical vectors.
8. Verify order independence and that the same targets with a recipient change use a different account and Relay.
9. Verify each child module hash, running status, empty controller set, public logs/status, and independent tracking counts.
10. Retain the snapshot until acceptance is complete.

For later Historian upgrades:

1. Record public state and known exact full-configuration mappings.
2. Stop Historian and wait until its status is `Stopped`.
3. Create and download a snapshot.
4. Upgrade Historian in place.
5. Start Historian.
6. Verify active mappings through the known exact configurations.
7. Verify `RelayTarget` and `RelayInstance` counts.
8. Verify any interrupted post-spend setup is `ManualRecoveryRequired`.

Self-service children are finalized with zero controllers and public log/status visibility. They cannot be upgraded and are never reconstructed by Historian.

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
| `jupiter_relay` | `InitArgs` from checked-in `mainnet-install-args.did` | Full `InitArgs` | Checked-in reviewed full `InitArgs` from `canisters/relay/mainnet-install-args.did` | Heap configuration/ordinary operations reset; stable SNS reward and fixed-splitter journals preserved |
| `jupiter_faucet_frontend` | No install args | No args | No args | Asset canister state managed by frontend asset lifecycle |
| `jupiter_lifeline` | No install args | No args | No args | Minimal support canister state |
| `jupiter_sns_rewards` | `InitArgs` with optional SNS Root | No args | Temporary nested `Option<UpgradeArgs>` | Stable configuration, active owner snapshot, staging scan, and cursor preserved when Root is unchanged |

Relay ordinary default-account allocation and subaccount-1 work remain replacement-style and non-resumable. Avoid upgrading during active Relay work where practical. If ordinary ICP work is interrupted, Relay starts fresh from supplied `InitArgs`. Stable memory 0 preserves the latest completed reward cadence timestamp and any pinned multi-recipient payout with its exact transfer identities and progress; reward attribution itself has no historical cursor. Stable memory 1 preserves any pinned fixed-splitter execution transaction and quarantine evidence. An active splitter requires the replacement `InitArgs` to name the same ICP Ledger. After upgrade, confirm the fresh `CONFIG` log, stable journal state, first successful `BaselineOnly` ordinary allocation tick, managed-canister cycle balances, and any required reconciliation.

The Relay V3 reward migration resets every V1/V2 cadence timestamp to zero because the historical field recorded attempts, including failed attempts. Any migrated pending identity is settled first. Ambiguous identities retain their exact amount, fee, memo, and creation time until conclusively reconciled; definitively rejected unpaid recipients in a partially completed V3 payout instead enter recoverable repricing/balance-waiting state. Completed-recipient progress and fixed unpaid entitlements remain durable while live fee and later token accrual provide fee headroom. Pending settlement does not recompute historical attribution. Once complete, the next fresh sweep reconstructs the residual reward balance and its oldest remaining FIFO credit directly from reward Ledger/Index history.

## SNS rewards lifecycle and Root switch

`jupiter_sns_rewards` discovers Governance and Ledger from its one configured SNS Root. The checked-in fresh-install argument configures OpenChat Root `3e3x2-xyaaa-aaaaq-aaalq-cai` only as a development placeholder. No Governance, CHAT Ledger, or future jUP Ledger ID is separately configured.

Before enabling the canonical Relay reward path, verify that recent canonical Relay subaccount-1 history contains supported debit shapes and reconciles into completed Faucet commitments. Attribution paginates backwards on demand without a lifetime page, transaction, or source-count cutoff; source ownership is resolved in deterministic API-sized chunks against one snapshot. Relay inverts each account's indexed transactions from the response's current balance and trusts a suffix only when it proves a zero opening balance. Genuine account genesis means that no older transactions exist, while the reconstructed opening balance must still reconcile exactly to zero. This proof includes post-cutoff account activity and applies independently to subaccount 1 and every referenced splitter. Older history is fetched only when FIFO carry or newer ineligible commitments require it, while pagination and reconstruction inconsistencies fail closed.

Relay also reads the ICP Ledger `query_blocks(start=0, length=0).chain_length` before the ICP Index `status().num_blocks_synced` and requires the Index to cover the observed Ledger prefix. Index lag therefore cannot hide a net-zero newer deposit-plus-commitment pair and redirect the payout to an older commitment; an unavailable or behind Index retries daily without consuming weekly cadence.

Also verify that the context's pinned SNS Root returns the same Root and reward Ledger from `list_sns_canisters`, returns an Index, and that the Index `ledger_id` names that reward Ledger. Relay reconstructs its live reward balance from that Index back to the latest zero-balance boundary or account genesis, where genesis proves that no older transactions exist and the reconstructed opening balance must still be zero, then FIFO-replays incoming credits against outgoing payouts and fees. The oldest remaining non-zero reward-credit Ledger block time and owner-snapshot scan-start time form the effective exclusive ICP cutoff. There is no reward-history cursor or depth cutoff. Detectable Index lag, including any live/index balance mismatch, retries on the next accepted daily tick without consuming weekly cadence. An undetectable unindexed net-zero outgoing-plus-incoming suffix can conservatively make the reconstructed cutoff older, but cannot make it newer than the fully synchronized cutoff.

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

For the first upgrade from the former empty placeholder, prepare a temporary reviewed argument file containing:

```did
(opt record {
  reward_sns_root_canister_id = opt opt principal "3e3x2-xyaaa-aaaaq-aaalq-cai";
})
```

Then use the ordinary canonical deployment workflow with that temporary file as `--args-file`. Codex must not perform this production upgrade. After completion, wait for `SNS_REWARD_SCAN status=completed` and query `get_relay_reward_context`; confirm the Root, Root-resolved Governance/Ledger, snapshot ID, and scan timestamps. Relay `CONFIG` logs must show canonical SNS-rewards canister `alk7f-5aaaa-aaaar-qb4ra-cai` and ICP Index `qhbym-qaaaa-aaaaa-aaafq-cai`.

Before switching from OpenChat to jUP:

1. Confirm every Relay has no pending, ambiguous, repricing, or balance-waiting SNS reward payout.
2. Reconcile any remaining development CHAT balance.
3. Upgrade `jupiter-sns-rewards` with a temporary nested argument naming the reviewed jUP SNS Root.
4. Wait for the first complete jUP owner snapshot.
5. Verify the context exposes the expected Root-derived jUP Ledger.
6. Allow Relay's next fresh adjudication to use the new completed context.

A Root switch clears both owner maps and prevents OpenChat ownership from remaining public. Relay has no main, splitter, or reward-token attribution cursor and never caches the reward Ledger or Index ID. An already-pinned payout remains bound to its original Root, Ledger and snapshot until settlement completes; a fresh adjudication naturally resolves the new Index through the new Root. A normal SNS Ledger fee change requires no Relay configuration upgrade because every new sweep resolves context and reads the live token fee. The release review must cover the SNS rewards Wasm, both production Candid interfaces, install arguments, stable-memory compatibility, and Relay's stable-journal upgrade behavior before any production change.

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
