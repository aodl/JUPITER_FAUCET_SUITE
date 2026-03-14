# Jupiter Faucet Frontend

`jupiter-faucet-frontend` is a placeholder frontend canister in the Jupiter Faucet Suite.

Its present-day purpose is simply to reserve the deployment slot for the eventual public-facing asset canister.

See the suite overview in [`../README.md`](../README.md).

## Current implementation

The implementation is intentionally minimal:

- no frontend assets yet
- no business logic
- no install or upgrade args

## Intended future role

The intended production role is to host the public frontend and asset bundle for the Jupiter Faucet system.

That frontend is not implemented in this repository yet.

## Upgrade command

```bash
dfx canister install jupiter_faucet_frontend \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet_frontend.wasm.gz
```
