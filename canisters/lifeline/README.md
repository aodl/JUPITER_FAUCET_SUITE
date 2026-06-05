# Jupiter Lifeline

`jupiter-lifeline` is the recovery canister in the Jupiter Faucet Suite.

Its job is deliberately minimal: exist as the configured rescue controller principal for operational canisters that would otherwise converge toward `self + blackhole` control while healthy. The production intent is that this principal is governed by the SNS DAO rather than any individual.

See the suite overview in [`../../README.md`](../../README.md).

Unless otherwise noted, command examples in this README are run from the repository root.

## Current mainnet canister recorded in this repo

- canister ID: `afisn-gqaaa-aaaar-qb4qa-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Implementation

The implementation is intentionally tiny:

- no public methods
- no recovery workflow baked into the module
- a timer that logs `Cycles: <amount>` every 20 days
- `init` and `post_upgrade` only reinstall that timer

The underlying assumption is that real rescue logic should be added only in the specific failure scenario that actually occurs.

## Role in the suite

During healthy operation, [`jupiter-disburser`](../disburser) and [`jupiter-faucet`](../faucet) are expected to reconcile to `self + blackhole` controller sets.

If their local rescue policy concludes that value flow is broken, they widen their controller sets to include `jupiter-lifeline`.

That means this canister is mostly a **reserved rescue principal**, not an active coordinator. In the intended production model, it serves as the DAO-governed recovery hook for the suite rather than as a point of unilateral human control.

## Install and upgrade

Production canister: `jupiter_lifeline` / `afisn-gqaaa-aaaar-qb4qa-cai`

Fresh install:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_lifeline \
  --environment ic \
  --mode install
```

Upgrade:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_lifeline \
  --environment ic \
  --mode upgrade
```

## Build

```bash
./tools/scripts/build-canister jupiter-lifeline
```

For canonical reproducible artifacts, use the repo-root Docker workflow described in [`../../README.md`](../../README.md).

## Related docs

- suite overview: [`../../README.md`](../../README.md)
- disburser rescue policy: [`../disburser/README.md`](../disburser/README.md)
- faucet rescue policy: [`../faucet/README.md`](../faucet/README.md)
