# Jupiter Lifeline

`jupiter-lifeline` is the recovery-controller canister for blackholed Jupiter operational canisters.

It exists so `jupiter-disburser` and `jupiter-faucet` can keep their normal controller sets narrow during healthy operation (`self + blackhole`) while still having a pre-positioned rescue target if their local rescue policy triggers.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `afisn-gqaaa-aaaar-qb4qa-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Current implementation

The implementation is intentionally minimal.

In steady state it:

- installs a timer on init/post-upgrade
- logs its cycle balance every `20 days`
- exposes no business logic beyond the empty canister interface generated from the module

There is no built-in recovery workflow yet because the canister is intended to be upgraded with **incident-specific** recovery logic only if a real lifeline event occurs.

## Role in the suite

Today the canister’s practical role is to be the configured `rescue_controller` for:

- `jupiter-disburser`
- `jupiter-faucet`

Those canisters decide for themselves when rescue should be activated. `jupiter-lifeline` is the additional controller they add alongside the existing `self + blackhole` controller set when that happens.

## Install and upgrade

No install args or upgrade args are currently required.

Example upgrade command:

```bash
dfx canister install jupiter_lifeline \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_lifeline.wasm.gz
```

## Build

```bash
cargo build -p jupiter-lifeline --target wasm32-unknown-unknown --release --locked
```

For canonical release artifacts, use the suite build scripts described in [`../README.md`](../README.md).
