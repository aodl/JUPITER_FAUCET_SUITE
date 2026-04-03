# Jupiter Lifeline

`jupiter-lifeline` is the recovery canister in the Jupiter Faucet Suite.

Its current job is deliberately minimal: exist as the configured rescue controller principal for operational canisters that would otherwise converge toward `self + blackhole` control while healthy. The production intent is that this principal is governed by the SNS DAO rather than any individual.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `afisn-gqaaa-aaaar-qb4qa-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Current implementation

Today the implementation is intentionally tiny:

- no public methods
- no recovery workflow baked into the module
- a timer that logs `Cycles: <amount>` every 20 days
- `init` and `post_upgrade` only reinstall that timer

The underlying assumption is that real rescue logic should be added only in the specific failure scenario that actually occurs.

## Role in the suite

During healthy operation, `jupiter-disburser` and `jupiter-faucet` are expected to reconcile to `self + blackhole` controller sets.

If their local rescue policy concludes that value flow is broken, they widen their controller sets to include `jupiter-lifeline`.

That means this canister is mostly a **reserved rescue principal** today, not an active coordinator. In the intended production model, it serves as the DAO-governed recovery hook for the suite rather than as a point of unilateral human control.

## Install and upgrade

Fresh install:

```bash
dfx canister install jupiter_lifeline \
  --network ic \
  --wasm release-artifacts/jupiter_lifeline.wasm.gz
```

Upgrade:

```bash
dfx canister install jupiter_lifeline \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_lifeline.wasm.gz
```

## Build

```bash
./scripts/build-canister jupiter-lifeline
```

For canonical reproducible artifacts, use the repo-root Docker workflow described in [`../README.md`](../README.md).

## Related docs

- suite overview: [`../README.md`](../README.md)
- disburser rescue policy: [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md)
- faucet rescue policy: [`../jupiter-faucet/README.md`](../jupiter-faucet/README.md)
