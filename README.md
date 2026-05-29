# Jupiter Faucet Suite

Jupiter Faucet Suite is an Internet Computer canister workspace for the Jupiter Faucet protocol. The repository is organized by domain so contributors can find production canisters, shared crates, tests, tooling, documentation, and vendored code at a glance.

## Repository Layout

- `canisters/` - production IC canisters.
  - `disburser/` - NNS maturity staging and payout routing.
  - `faucet/` - staking-account scan, memo-derived registration, and CMC top-up flow.
  - `historian/` - indexed public read model for dashboard and protocol history.
  - `relay/` - suite cycles funding and surplus-routing support.
  - `lifeline/` - minimal recovery/support canister.
  - `sns-rewards/` - rewards-recipient placeholder canister.
  - `frontend/` - certified asset canister plus browser dashboard.
- `crates/` - reusable internal Rust crates.
- `tests/mocks/` - local mock canisters used by integration scenarios.
- `tests/pocketic/` - PocketIC integration and E2E test sources.
- `tools/xtask/` - repo-aware test and local orchestration utility.
- `tools/scripts/` - build, validation, smoke-test, and reproducibility scripts.
- `docs/` - architecture, development, and operations notes.
- `vendor/` - vendored third-party source used by tests or reproducible builds.

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

- [Architecture system map](docs/architecture/system-map.md)
- [Canister roles](docs/architecture/canister-roles.md)
- [Local setup](docs/development/local-setup.md)
- [Testing](docs/development/testing.md)
- [Deployment](docs/operations/deployment.md)
- [Reproducible builds](docs/operations/reproducible-builds.md)

Canister-specific details remain next to each canister under `canisters/*/README.md`.
