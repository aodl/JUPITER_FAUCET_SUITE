# Deployment

Production deployment is a manual, privileged operation. This repository contains build and validation helpers for the suite's canister names, Candid interfaces, install arguments, and deployment policy.

Fresh install argument files live with their owning canisters:

- [`canisters/disburser/mainnet-install-args.did`](../../canisters/disburser/mainnet-install-args.did)
- [`canisters/faucet/mainnet-install-args.did`](../../canisters/faucet/mainnet-install-args.did)
- [`canisters/historian/mainnet-install-args.did`](../../canisters/historian/mainnet-install-args.did)
- [`canisters/relay/mainnet-install-args.did`](../../canisters/relay/mainnet-install-args.did)

Validate production install arguments and DID separation with:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
```

Build release artifacts with [`tools/scripts/build-canister`](../../tools/scripts/build-canister):

```bash
./tools/scripts/build-canister all
```

For module-hash verification and deterministic rebuild checks, see [reproducible builds](reproducible-builds.md).
