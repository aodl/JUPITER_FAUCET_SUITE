# Reproducible Builds

Jupiter Faucet uses reproducible release artifacts so public source code can be checked against deployed Internet Computer Wasm module hashes. The normal release-artifact builder is [`tools/scripts/build-canister`](../../tools/scripts/build-canister):

```bash
./tools/scripts/build-canister all
```

That command builds the suite artifacts into `release-artifacts/` using the checked-in lockfiles and local toolchain. To build one canister, pass the package/canister build name, for example:

```bash
./tools/scripts/build-canister jupiter-faucet
./tools/scripts/build-canister jupiter-faucet-frontend
```

The heavier reproducibility flow rebuilds release artifacts in a pinned Docker environment and compares output hashes:

```bash
./tools/scripts/docker-build
npm run verify:reproducible-artifacts
```

This verifies deterministic rebuilds in the pinned environment used by the project. It does not remove every external trust assumption: Docker access is required, Rust and npm inputs are resolved through the committed lockfiles and bootstrap checks, and live module-hash verification still depends on reading deployed canister metadata from the Internet Computer.

For production operations, compare each deployed module hash with the corresponding locally rebuilt module-hash reference emitted by the release tooling, and keep [`tools/scripts/validate-mainnet-install-args`](../../tools/scripts/validate-mainnet-install-args) in the release checklist when install arguments or Candid files change.
