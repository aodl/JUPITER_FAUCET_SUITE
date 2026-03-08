# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is currently a deployable placeholder.

The production intent is for this canister to serve the public-facing asset bundle for the Jupiter Faucet system.

## Current implementation

The current implementation is intentionally minimal. It exists to reserve the deployment slot while the frontend asset canister is built.

## Upgrade command

No install or upgrade argument is currently required.

```bash
dfx canister install jupiter_faucet_frontend   --network ic   --mode upgrade   --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```
