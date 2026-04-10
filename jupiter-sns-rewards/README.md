# Jupiter SNS Rewards

`jupiter-sns-rewards` is the current placeholder rewards canister in the Jupiter Faucet Suite.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `alk7f-5aaaa-aaaar-qb4ra-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Current role in the live wiring

Today this canister is the configured recipient for **80% of the disburser’s age-bonus ICP flow**.

Its main practical role right now is to reserve the production canister principal and its default ledger account so that reward-distribution logic can be added later without changing the live routing destination.

## Current implementation

The implementation is intentionally empty:

- no public methods
- no timers
- no stable state
- `init` is a no-op
- there is no upgrade hook because no runtime reinitialization is required

That means the canister is currently a principal / account placeholder, not a live reward-distribution engine.

## Intended future role

Once Jupiter-specific SNS reward logic lands, this canister is the natural place for that distribution policy to live because the disburser already routes the primary age-bonus flow here.

Until then, it can largely be ignored when trying to understand the current operational path.

## Build and upgrade

Build:

```bash
./scripts/build-canister jupiter-sns-rewards
```

Upgrade:

```bash
dfx canister install jupiter_sns_rewards \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_sns_rewards.wasm.gz
```

## Future historian / SNS testing note

Historian SNS coverage is currently generic and mock-based. Once the actual Jupiter SNS configuration is represented in-repo, the historian and end-to-end test paths should be extended to cover the live Jupiter SNS reward topology specifically.

## Related docs

- suite overview: [`../README.md`](../README.md)
- disburser routing policy: [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md)
- historian notes: [`../jupiter-historian/README.md`](../jupiter-historian/README.md)
