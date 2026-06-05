# Reproducible Builds

Jupiter Faucet uses reproducible release artifacts so public source code can be checked against the Wasm module hashes deployed on the Internet Computer.

Most readers want one specific outcome: rebuild the canister install packages from this repository, get the module-hash references, and compare those hashes with mainnet. Use the Docker-backed release path for that.

## How to compare this source checkout with mainnet

Run the canonical Docker build with [`tools/scripts/docker-build`](../../tools/scripts/docker-build):

```bash
./tools/scripts/docker-build
```

This builds the release artifacts in the pinned environment from [`Dockerfile.repro`](../../Dockerfile.repro), copies the artifacts into `release-artifacts/`, and prints two kinds of hashes:

- `Installed module hash reference (*.wasm.gz)` - compare this SHA-256 hash with the deployed canister's mainnet module hash when the release was installed from the compressed package.
- `Decompressed Wasm hash (*.wasm)` - useful release evidence for the uncompressed module bytes, but not the hash shown by mainnet canister metadata for these compressed installs.

Then compare each `Installed module hash reference` with the live mainnet module hash. The easiest public view is the Source Code pane:

- [Jupiter Faucet Source Code pane](https://jupiter-faucet.com/#source)

The Source Code pane loads module hashes, controllers, and memory information from mainnet canister metadata through the historian/frontend read surface. You can also compare against the IC dashboard canister pages or an authenticated CLI metadata read if you have an IC toolchain available.

## How to rebuild artifacts locally for development

Use [`tools/scripts/build-canister`](../../tools/scripts/build-canister) when you need local release artifacts quickly, for example before a direct local install, a frontend prototype deployment, or a focused artifact inspection:

```bash
./tools/scripts/build-canister all
```

To build one canister:

```bash
./tools/scripts/build-canister jupiter-faucet
./tools/scripts/build-canister jupiter-faucet-frontend
```

This script writes artifacts and `build-info.json` into `release-artifacts/` using the checked-in lockfiles and the local machine's toolchain. It is intentionally useful for day-to-day artifact work. It is not the canonical production deployment path; use the canister-specific deployment docs and `icp deploy` flow for ordinary production deployment and upgrade operations. It is also not the strongest evidence for outside observers because it does not isolate the build inside the pinned Docker environment.

## Same-environment determinism check

Use the verification command when you want evidence that the pinned release environment produces identical artifacts across clean rebuilds on the same machine:

```bash
npm run verify:reproducible-artifacts
```

That command runs [`tools/scripts/verify-reproducible-artifacts`](../../tools/scripts/verify-reproducible-artifacts). It performs two no-cache Docker builds from [`Dockerfile.repro`](../../Dockerfile.repro), hashes every emitted file from each run, and diffs those hash manifests.

This checks deterministic rebuilds inside one pinned environment. It is useful release evidence, but it does not by itself prove reproducibility across independent machines and it does not compare the output to mainnet. For the live mainnet comparison, use `./tools/scripts/docker-build` output plus the Source Code pane or canister metadata.

## How the build scripts differ

| Scenario | Command | What it gives you |
| --- | --- | --- |
| Fast local artifact build | `./tools/scripts/build-canister all` | `release-artifacts/` from the local toolchain. Good for development, direct local installs, frontend prototype deployment, and quick inspection. |
| Canonical mainnet hash comparison | `./tools/scripts/docker-build` | `release-artifacts/` from the pinned Docker environment plus printed module-hash references to compare with mainnet. |
| Same-environment determinism check | `npm run verify:reproducible-artifacts` | Two clean Docker builds compared against each other on the same machine. Useful release evidence, but not a cross-machine reproducibility proof or a mainnet metadata check. |

## How to verify runtime configuration

After the deployed Wasm hash matches the source build, verify the runtime configuration separately. Canisters that take configuration arguments periodically emit a public `CONFIG ...` line in canister logs alongside their regular health or activity logs. These logs are public and are also exposed through the Jupiter Faucet frontend canister views for the relevant canister principals.

Configuration-bearing mainnet canisters:

| Canister | Frontend canister view | Public log command |
| --- | --- | --- |
| `jupiter-disburser` | [uccpi-cqaaa-aaaar-qby3q-cai](https://jupiter-faucet.com/#metric-tracker-uccpi-cqaaa-aaaar-qby3q-cai) | `icp canister logs uccpi-cqaaa-aaaar-qby3q-cai -n ic` |
| `jupiter-faucet` | [acjuz-liaaa-aaaar-qb4qq-cai](https://jupiter-faucet.com/#metric-tracker-acjuz-liaaa-aaaar-qb4qq-cai) | `icp canister logs acjuz-liaaa-aaaar-qb4qq-cai -n ic` |
| `jupiter-historian` | [j5gs6-uiaaa-aaaar-qb5cq-cai](https://jupiter-faucet.com/#metric-tracker-j5gs6-uiaaa-aaaar-qb5cq-cai) | `icp canister logs j5gs6-uiaaa-aaaar-qb5cq-cai -n ic` |
| `jupiter-relay` | [u2qkp-aqaaa-aaaar-qb7ea-cai](https://jupiter-faucet.com/#metric-tracker-u2qkp-aqaaa-aaaar-qb7ea-cai) | `icp canister logs u2qkp-aqaaa-aaaar-qb7ea-cai -n ic` |

Use the canister-specific README for the exact config fields each `CONFIG ...` line is expected to contain:

- [`canisters/disburser/README.md`](../../canisters/disburser/README.md)
- [`canisters/faucet/README.md`](../../canisters/faucet/README.md)
- [`canisters/historian/README.md`](../../canisters/historian/README.md)
- [`canisters/relay/README.md`](../../canisters/relay/README.md)

The checked-in fresh-install argument files are still useful release inputs, but live config should be verified from the deployed canister logs after module-hash verification:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
```

Run that validator when changing production install arguments, Candid files, production canister IDs, or deployment-policy documentation.
