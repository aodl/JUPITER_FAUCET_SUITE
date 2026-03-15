# Jupiter SNS Rewards

`jupiter-sns-rewards` is currently a placeholder canister in the Jupiter Faucet Suite.

Its present-day purpose is to reserve the production canister principal and default ledger account that receive the primary age-bonus ICP flow from `jupiter-disburser`.

See the suite overview in [`../README.md`](../README.md).

## Current mainnet canister recorded in this repo

- canister ID: `alk7f-5aaaa-aaaar-qb4ra-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

## Current role in the live wiring

`jupiter-disburser` sends **80% of the age-bonus portion** of each payout here.

The age-bonus source, formula, and routing policy are documented in [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md).

## Current implementation

The current implementation is deliberately minimal:

- no business logic
- no public methods of its own
- no install or upgrade args

That makes it effectively a reserved deployment slot and account endpoint until the real SNS reward-distribution logic is implemented.

## Intended future role

The intended production role is to receive ICP from the disburser and distribute rewards to JUP SNS stakers according to a future staking/reward policy.

That policy is not implemented in this repository yet.

## Upgrade command

```bash
dfx canister install jupiter_sns_rewards \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_sns_rewards.wasm.gz
```


## Future historian / SNS testing note

**TODO:** Revisit `jupiter-historian` SNS integration testing once the Jupiter Faucet Suite's own SNS configuration is represented in this repository. At that point the current mock-based SNS historian tests should be supplemented with Jupiter-specific SNS smoke/integration coverage driven from the in-repo SNS configuration.
