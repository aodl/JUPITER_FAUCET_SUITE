# Reproducible Builds

Jupiter Faucet release evidence connects reviewed source code, canonical build artifacts, and the Wasm package installed on mainnet. The intended uploaded production package is the compressed `.wasm.gz` artifact.

## What is being verified

This flow compares the live canister module hash with the canonical Docker-built `.wasm.gz` package hash. The decompressed `.wasm` hash remains useful supporting evidence about the module bytes, but it is not the normal mainnet comparison target.

## Canonical Docker artifact build

Run the canonical Docker build from the repo root:

```bash
./tools/scripts/docker-build
```

This runs [`Dockerfile.repro`](../../Dockerfile.repro), copies artifacts into `release-artifacts/`, writes `release-artifacts/release-artifacts.sha256`, and prints the main verification hashes.

The `Mainnet module-hash comparison targets (*.wasm.gz)` section contains the hashes to compare with the live canister module hashes.

The decompressed `.wasm` hashes remain available in the sidecar files and full manifest as supporting release evidence, but the normal mainnet comparison uses the `.wasm.gz` installed package hashes.

For self-service Relay setup, keep two Relay hashes distinct in release evidence. `release-artifacts/jupiter_relay.wasm` is the reviewed raw Relay Wasm evidence. `release-artifacts/jupiter_relay.wasm.gz` is the compressed Relay install payload embedded in Historian and passed to `install_code`; Historian derives the expected setup reconciliation hash from the exact embedded bytes and compares live management `canister_info(relay_id).module_hash` values before retrying install or handing off final control. The gzip payload must decompress to bytes matching the reviewed raw Relay Wasm hash. The production Historian deployment artifact remains `release-artifacts/jupiter_historian.wasm.gz`.

## Hash comparison with mainnet

After a canonical artifact build or canonical-artifact deployment, compare each live canister module hash with the matching hash from the `Mainnet module-hash comparison targets (*.wasm.gz)` section printed by `./tools/scripts/docker-build`.

The public Source Code pane is the easiest suite-level view:

- [Jupiter Faucet Source Code pane](https://jupiter-faucet.com/#source)

The Source Code pane loads module hashes, controllers, and memory information from mainnet canister metadata through the historian/frontend read surface.

You can also compare against IC dashboard canister metadata or an authenticated CLI metadata read when you have the appropriate toolchain access.

## Runtime configuration verification

Module hashes do not prove runtime configuration. Validate checked-in production arguments and DID separation with:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
```

Then verify deployed configuration from public canister logs. Configuration-bearing mainnet canisters emit `CONFIG ...` log lines.

| Canister | Frontend log view | Public log command |
| --- | --- | --- |
| `jupiter-disburser` | [Frontend view](https://jupiter-faucet.com/#metric-tracker-uccpi-cqaaa-aaaar-qby3q-cai) | `icp canister logs uccpi-cqaaa-aaaar-qby3q-cai -n ic` |
| `jupiter-faucet` | [Frontend view](https://jupiter-faucet.com/#metric-tracker-acjuz-liaaa-aaaar-qb4qq-cai) | `icp canister logs acjuz-liaaa-aaaar-qb4qq-cai -n ic` |
| `jupiter-historian` | [Frontend view](https://jupiter-faucet.com/#metric-tracker-j5gs6-uiaaa-aaaar-qb5cq-cai) | `icp canister logs j5gs6-uiaaa-aaaar-qb5cq-cai -n ic` |
| `jupiter-relay` | [Frontend view](https://jupiter-faucet.com/#metric-tracker-u2qkp-aqaaa-aaaar-qb7ea-cai) | `icp canister logs u2qkp-aqaaa-aaaar-qb7ea-cai -n ic` |

Use the canister-specific README for expected config fields.

## Workflow quick reference

| Scenario | Command | What it gives you |
| --- | --- | --- |
| Fast local artifact build | `./tools/scripts/build-canister all` | Local-toolchain artifacts for inspection and development. |
| One-canister local artifact build | `./tools/scripts/build-canister jupiter-faucet` | Local-toolchain artifact for one canister. |
| Canonical artifact build | `./tools/scripts/docker-build` | Docker-built `.wasm.gz` packages and hash manifest. |
| Production deploy from canonical artifacts | `JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy <canister_name> --environment ic --mode upgrade` | Deploys the existing Docker-built `.wasm.gz` package through `icp deploy`. |
| Ordinary local `icp deploy` | `icp deploy <canister_name> --environment ic --mode upgrade` | Runs the configured local build helper and deploys the resulting package. Convenient, but not canonical reproducibility evidence. |
| Same-environment determinism check | `npm run verify:reproducible-artifacts` | Two clean Docker builds compared on the same machine. |

With `relay_factory_enabled = opt true` in the checked-in Historian mainnet args, both `build-canister all` and Docker builds produce `release-artifacts/jupiter_historian.wasm.gz` as the production Historian package.

## Deploying Docker-built artifacts with icp deploy

The strongest production release flow is:

```bash
./tools/scripts/docker-build
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy <canister_name> --environment ic --mode upgrade
```

`icp deploy` is the preferred deployment orchestrator. It does not, by itself, prove that the installed bytes came from the canonical Docker build. For a reproducible production release, first produce canonical artifacts with `./tools/scripts/docker-build`, then deploy those prebuilt `.wasm.gz` artifacts through the documented canonical-artifact deploy mode, and finally compare the live canister module hash with the installed package hash from the Docker build evidence.

With `JUPITER_USE_CANONICAL_ARTIFACTS=1`, the `icp.yaml` build helper refuses to rebuild locally and instead verifies `release-artifacts/release-artifacts.sha256`, confirms the requested artifact is present in that Docker-generated manifest, and copies the matching `release-artifacts/<name>.wasm.gz` package into the `icp deploy` build output path. It also prints the package SHA-256 before deployment.

For routine no-config-change upgrades, pass no args for Disburser, Faucet, and Historian. Existing production Historian must be upgraded in place; reinstall destroys stable history and is prohibited for the existing production canister. Checked-in `mainnet-install-args.did` files are fresh install/reinstall `InitArgs`, not routine upgrade inputs for those stateful canisters. `canisters/historian/mainnet-install-args.did` is for a brand-new Historian installation only. Relay is replacement-style and requires full `InitArgs` on every upgrade. For Disburser, Faucet, and Historian config-changing upgrades, pass a temporary, deployment-specific `Option<UpgradeArgs>` file:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet \
  --environment ic \
  --mode upgrade \
  --args-file /tmp/jupiter-faucet-upgrade-args.did
```

Historian probing is always Auto. There is no `cycles_probe_policy` deployment field.

For fresh install or reinstall only, use the checked-in `mainnet-install-args.did` `InitArgs` file:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_faucet \
  --environment ic \
  --mode install \
  --args-file canisters/faucet/mainnet-install-args.did
```

Reinstall clears canister Wasm/stable state and must be treated as a separate destructive operation.

Historian production upgrade command:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_historian \
  --environment ic \
  --mode upgrade
```

Pause the self-service factory during the Historian maintenance window, take a canister snapshot or equivalent backup, and record pre-upgrade query results for after-upgrade comparison. Existing setup/recovery jobs and Relay registrations are preserved by the upgrade. Deploy the frontend after backend verification.

Lifecycle summary:

| Canister group | Routine upgrade args | Config-changing upgrade args | State behavior |
| --- | --- | --- | --- |
| Disburser/Faucet/Historian | No args | Temporary `Option<UpgradeArgs>` | Stable state preserved |
| Relay | Full `InitArgs` | Checked-in reviewed full `InitArgs` from `canisters/relay/mainnet-install-args.did` | Heap-only replacement; config and operational state reset; non-resumable |
| Frontend/Lifeline/SNS Rewards | No args | No args | No install args |

Relay has no `UpgradeArgs`. Relay config-changing upgrades update and review the checked-in full `InitArgs` file at `canisters/relay/mainnet-install-args.did`. Relay upgrades are replacement-style and non-resumable. Avoid deploying Relay artifacts during active Relay work where practical. After upgrade, verify `CONFIG` logs, the `BaselineOnly` first tick, and managed canister cycle balances.

## Ordinary local icp deploy

For development, inspection, or non-canonical operator builds, use `icp deploy` without `JUPITER_USE_CANONICAL_ARTIFACTS`:

```bash
icp deploy jupiter_faucet --environment ic --mode upgrade
```

That runs [`tools/scripts/build-canister`](../../tools/scripts/build-canister) on the local machine and deploys the resulting `.wasm.gz` package. This is convenient, but it is local-toolchain output. It can be compared afterwards against canonical Docker artifacts, but the deployment command itself is not reproducible-build proof.

For fast local artifact work without deployment:

```bash
./tools/scripts/build-canister all
./tools/scripts/build-canister jupiter-faucet
./tools/scripts/build-canister jupiter-faucet-frontend
```

## Same-environment deterministic rebuild check

Use the verification command to check deterministic rebuilds inside the pinned Docker environment:

```bash
npm run verify:reproducible-artifacts
```

That command runs two no-cache `docker buildx build --platform linux/amd64` artifact builds and diffs hash manifests for every emitted file. It proves same-environment determinism on that machine. It does not prove cross-machine reproducibility by itself and does not compare with mainnet.

As a manifest regression check, run the canonical Docker build twice and verify the manifest afterwards:

```bash
./tools/scripts/docker-build
./tools/scripts/docker-build
sha256sum -c release-artifacts/release-artifacts.sha256
```

This verifies that the generated `release-artifacts/release-artifacts.sha256` manifest does not include and hash a previous copy of itself.
