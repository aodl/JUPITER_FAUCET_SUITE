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

## Fresh installs vs upgrades

`mainnet-install-args.did` files are fresh-install `InitArgs`. Do not use those files for ordinary production upgrades.

> Warning:
> Do not pass `canisters/<name>/mainnet-install-args.did` to `--mode upgrade`
> for stateful production canisters. Those files are fresh-install `InitArgs`
> and may be intentionally rejected during `post_upgrade`.

For no-config-change production upgrades, pass no args. This preserves stable state and lets the canister decode the omitted upgrade argument as no config change.

For config-changing production upgrades, create a temporary local `UpgradeArgs` file for that specific deployment and pass it with `--args-file`. Do not check in example upgrade-args files; realistic examples are easy to copy later for the wrong deployment.

Canister-specific README sections define the correct production canister ID, artifact name, `UpgradeArgs` shape, and verification commands:

- [`canisters/disburser/README.md`](../../canisters/disburser/README.md)
- [`canisters/faucet/README.md`](../../canisters/faucet/README.md)
- [`canisters/historian/README.md`](../../canisters/historian/README.md)
- [`canisters/relay/README.md`](../../canisters/relay/README.md)

A failed upgrade with an error such as `received InitArgs in post_upgrade` means the canister rejected the wrong argument shape. Rebuild the command using the canister's current `UpgradeArgs` shape instead of the fresh-install `InitArgs` file.

For module-hash verification and deterministic rebuild checks, see [reproducible builds](reproducible-builds.md).
