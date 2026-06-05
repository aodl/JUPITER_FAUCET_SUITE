# Jupiter SNS Rewards

`jupiter-sns-rewards` is the placeholder rewards canister in the Jupiter Faucet Suite.

See the suite overview in [`../../README.md`](../../README.md).

Unless otherwise noted, command examples in this README are run from the repository root.

## Current mainnet canister recorded in this repo

- canister ID: `alk7f-5aaaa-aaaar-qb4ra-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Role in the live wiring

This canister is the configured recipient for **95% of the disburser's age-bonus ICP flow**.

Its main practical role is to reserve the production canister principal and its default ledger account so that reward-distribution logic can be added later without changing the live routing destination.

## Implementation

The implementation is intentionally empty:

- no public methods
- no timers
- no stable state
- `init` is a no-op
- there is no upgrade hook because no runtime reinitialization is required

That means the canister is a principal / account placeholder, not a live reward-distribution engine.

## Intended future role

Once Jupiter-specific SNS reward logic lands, this canister is the natural place for that distribution policy to live because the [disburser](../disburser) already routes the primary age-bonus flow here.

Until then, it can largely be ignored when trying to understand the operational path.

## Build and upgrade

Production canister: `jupiter_sns_rewards` / `alk7f-5aaaa-aaaar-qb4ra-cai`

Build:

```bash
./tools/scripts/build-canister jupiter-sns-rewards
```

Upgrade:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_sns_rewards \
  --environment ic \
  --mode upgrade
```

## Future historian / SNS testing note

[Historian](../historian) SNS coverage is generic and mock-based. Once the actual Jupiter SNS configuration is represented in-repo, the historian and [end-to-end test paths](../../tests/pocketic) should be extended to cover the live Jupiter SNS reward topology specifically.

## Related docs

- suite overview: [`../../README.md`](../../README.md)
- disburser routing policy: [`../disburser/README.md`](../disburser/README.md)
- historian notes: [`../historian/README.md`](../historian/README.md)
