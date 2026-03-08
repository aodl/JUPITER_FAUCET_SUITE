# Jupiter SNS Rewards

`jupiter-sns-rewards` is currently deployed as a placeholder canister.

## Current mainnet canister

- canister id: `alk7f-5aaaa-aaaar-qb4ra-cai`
- subnet: Fiduciary (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`)

This canister is the configured 80% age-bonus recipient for `jupiter-disburser`.

The production intent is for `jupiter-sns-rewards` to receive that ICP flow and distribute it to JUP SNS stakers according to its own staking policy.

## Current implementation

The current implementation is intentionally minimal. It exists to reserve the canister principal and default account while the production SNS rewards logic is built.

## Upgrade command

No install or upgrade argument is currently required.

```bash
dfx canister install jupiter_sns_rewards   --network ic   --mode upgrade   --wasm release-artifacts/jupiter_sns_rewards.wasm.gz
```
