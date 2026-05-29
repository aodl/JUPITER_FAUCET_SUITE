# Deployment

Production deployment remains a manual, privileged operation. This repository contains build and validation helpers, but moving files into the new layout does not change canister names, Candid interfaces, or deployment policy.

Fresh install argument files live with their owning canisters:

- `canisters/disburser/mainnet-install-args.did`
- `canisters/faucet/mainnet-install-args.did`
- `canisters/historian/mainnet-install-args.did`
- `canisters/relay/mainnet-install-args.did`

Validate production install arguments and DID separation with:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
```
