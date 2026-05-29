# Jupiter Faucet Suite

Jupiter Faucet Suite is an Internet Computer canister workspace for the Jupiter Faucet protocol. The repository is organized by domain so contributors can find production canisters, shared crates, tests, tooling, documentation, and vendored code at a glance.

![Jupiter Faucet](canisters/frontend/public/og/preview-20260520.jpg)

## Repository Layout

- [`canisters/`](canisters) - production IC canisters.
  - [`disburser/`](canisters/disburser) - NNS maturity staging and payout routing.
  - [`faucet/`](canisters/faucet) - staking-account scan, memo-derived registration, and CMC top-up flow.
  - [`historian/`](canisters/historian) - indexed public read model for dashboard and protocol history.
  - [`relay/`](canisters/relay) - suite cycles funding and surplus-routing support.
  - [`lifeline/`](canisters/lifeline) - minimal recovery/support canister.
  - [`sns-rewards/`](canisters/sns-rewards) - rewards-recipient placeholder canister.
  - [`frontend/`](canisters/frontend) - certified asset canister plus browser dashboard.
- [`crates/`](crates) - reusable internal Rust crates.
- [`tests/mocks/`](tests/mocks) - local mock canisters used by integration scenarios.
- [`tests/pocketic/`](tests/pocketic) - PocketIC integration and E2E test sources.
- [`tools/xtask/`](tools/xtask) - repo-aware test and local orchestration utility.
- [`tools/scripts/`](tools/scripts) - build, validation, smoke-test, and reproducibility scripts.
- [`docs/`](docs) - architecture, development, and operations notes.
- [`vendor/`](vendor) - vendored third-party source used by tests or reproducible builds.

## Common Commands

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_all
npm run build:frontend
npm run test:frontend-unit
./tools/scripts/build-canister all
python3 ./tools/scripts/validate-mainnet-install-args
```

## Documentation

- [Canister roles](docs/architecture/canister-roles.md)
- [Local setup](docs/development/local-setup.md)
- [Testing](docs/development/testing.md)
- [Deployment](docs/operations/deployment.md)
- [Reproducible builds](docs/operations/reproducible-builds.md)

Canister-specific details remain next to each canister under [`canisters/*/README.md`](canisters).
