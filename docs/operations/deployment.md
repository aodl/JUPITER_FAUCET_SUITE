# Deployment

Production deployment is a governance-controlled operation. Once Jupiter Faucet is under SNS DAO control, production upgrades are expected to pass through SNS community consensus before execution. During the initial bootstrap phase, upgrades may still be executed by the current bootstrap controller, but the release process should be documented and verifiable as if it will be reviewed by the community.

Use `icp deploy --environment ic` for ordinary production orchestration, and use canonical Docker artifacts when public reproducibility evidence matters.

## Production release flow

Recommended release sequence:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
./tools/scripts/docker-build
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy <canister_name> --environment ic --mode upgrade
```

`./tools/scripts/docker-build` produces canonical `.wasm.gz` install packages and `release-artifacts/release-artifacts.sha256`. `JUPITER_USE_CANONICAL_ARTIFACTS=1` tells the `icp.yaml` build helper to verify that manifest and deploy those existing packages instead of rebuilding with the local toolchain.

For routine no-config-change production upgrades, pass no args for Disburser, Faucet, and Historian. Those stateful canisters decode omitted upgrade args as no config change. Relay is replacement-style and requires full `InitArgs` on every upgrade.

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet --environment ic --mode upgrade
```

For config-changing upgrades, create a temporary deployment-specific `UpgradeArgs` file and pass it explicitly:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet \
  --environment ic \
  --mode upgrade \
  --args-file /tmp/jupiter-faucet-upgrade-args.did
```

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
| `jupiter_historian` | `InitArgs` from checked-in `mainnet-install-args.did` | No args | Temporary `Option<UpgradeArgs>` | Stable state preserved |
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
