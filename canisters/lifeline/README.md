# Jupiter Lifeline

`jupiter-lifeline` is the recovery canister in the Jupiter Faucet Suite.

Its job is deliberately minimal: exist as the configured recovery-authority principal for Faucet and Disburser. The production intent is that this principal is governed by the SNS DAO rather than any individual.

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

Once autonomous rescue is deliberately armed, healthy [`jupiter-disburser`](../disburser) and [`jupiter-faucet`](../faucet) deployments reconcile to self-only controller sets.

If their durable rescue policy concludes that value flow is broken, they widen their controller sets to self plus `jupiter-lifeline`. When healthy value flow returns, they remove Lifeline again.

That means this canister is a **reserved rescue principal**, not an active coordinator. It has no normal recovery API, periodic controller orchestration, or speculative recovery workflow. A real Lifeline event can be handled by a separately reviewed, failure-specific Lifeline upgrade.

Observation is a separate concern: Jupiter protocol canisters use public `canister_status`, so Lifeline does not need controller authority merely to expose status or cycles.

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
