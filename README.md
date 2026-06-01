# Jupiter Faucet Suite

[Jupiter Faucet](https://jupiter-faucet.com/#intro) is a perpetual cycles top-up protocol for the Internet Computer, built to help canister smart contracts keep running. This repository contains the production canisters, certified frontend, shared crates, tests, and release tooling that implement and verify the Jupiter Faucet suite.

![Jupiter Faucet](canisters/frontend/public/og/preview-20260520.jpg)

The suite turns durable ICP and NNS maturity into durable cycles support. A controlled NNS neuron produces recurring maturity, the disburser stages that maturity as ICP, the faucet allocates the base ICP flow to memo-declared targets, and the relay helps keep the suite's own canisters funded before routing surplus ICP to configured neuron recipients. Historian and frontend canisters provide public observability, while small recovery/support canisters keep the value-moving path narrow and auditable.

## Protocol Overview

The operational path is intentionally split across small canisters:

- [`canisters/disburser`](canisters/disburser) controls one NNS neuron, disburses available maturity, and routes staged ICP into the fixed base/age-bonus recipients.
- [`canisters/faucet`](canisters/faucet) receives the base ICP flow, scans the configured staking account, interprets eligible transfer memos, and performs proportional payouts as cycles top-ups, raw ICP transfers, or NNS neuron stake transfers.
- [`canisters/relay`](canisters/relay) receives suite-funding ICP from the faucet, tops up managed suite canisters from recent cycles-burn observations, and routes remaining surplus.
- [`canisters/historian`](canisters/historian) indexes commitment history, target canisters, cycles samples, SNS discovery, and dashboard-facing public state.
- [`canisters/frontend`](canisters/frontend) serves the certified public site and browser dashboard.
- [`canisters/lifeline`](canisters/lifeline) and [`canisters/sns-rewards`](canisters/sns-rewards) provide recovery/support and reward-recipient roles.

At a high level, a participant declares a faucet target by transferring ICP to the configured staking account and placing a supported ASCII directive in `icrc1_memo`. Plain declared canister ID text is the primary cycles top-up form. The faucet also supports `canister_id.memo` for raw ICP routing and decimal NNS neuron IDs, optionally with `.memo`, for neuron staking-account top-ups. The exact eligibility, memo, fee, retry, and rescue rules live in the component READMEs:

- [`canisters/disburser/README.md`](canisters/disburser/README.md)
- [`canisters/faucet/README.md`](canisters/faucet/README.md)
- [`canisters/relay/README.md`](canisters/relay/README.md)

The value-moving canisters expose little or no public production API. Public verification and dashboard data are concentrated in [`canisters/historian`](canisters/historian), [`canisters/frontend`](canisters/frontend), public logs, source code, Candid files, and reproducible build artifacts.

## Source Verification and Reproducible Builds

Reproducible builds are part of the trust model for Jupiter Faucet. A deployed canister's Wasm module hash can be compared with locally rebuilt release artifacts so readers can connect public source code to the code running on the Internet Computer.

Start with [reproducible builds](docs/operations/reproducible-builds.md) for the verification flow and [`tools/scripts/build-canister`](tools/scripts/build-canister) for the normal release-artifact builder. The high-level local build command is:

```bash
./tools/scripts/build-canister all
```

The full reproducibility check uses the heavier Docker-backed path documented in [reproducible builds](docs/operations/reproducible-builds.md). Docker access and mainnet canister visibility may be required for parts of an end-to-end verification workflow.

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
- [`tests/`](tests) - integration and end-to-end test assets.
  - [`mocks/`](tests/mocks) - local mock canisters used by integration scenarios.
  - [`pocketic/`](tests/pocketic) - PocketIC integration and E2E test sources.
- [`tools/`](tools) - developer, test, build, and release tooling.
  - [`xtask/`](tools/xtask) - repo-aware test and local orchestration utility.
  - [`scripts/`](tools/scripts) - build, validation, smoke-test, and reproducibility scripts.
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
- [Testing](docs/development/testing.md)
- [Dependency scanning](docs/security/dependency-scanning.md)
- [Deployment](docs/operations/deployment.md)
- [Reproducible builds](docs/operations/reproducible-builds.md)

Canister-specific details remain next to each canister under [`canisters/*/README.md`](canisters).
