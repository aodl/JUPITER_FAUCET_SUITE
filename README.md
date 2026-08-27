# Jupiter Faucet Suite

[Jupiter Faucet](https://jupiter-faucet.com/#intro) is a perpetual cycles top-up protocol for the Internet Computer, built to help canister smart contracts keep running. This repository contains the production canisters, certified frontend, shared crates, tests, and release tooling that implement and verify the Jupiter Faucet suite.

![Jupiter Faucet](canisters/frontend/public/og/preview-20260520.jpg)

<img src="/canisters/frontend/public/perpetual-canister-topups.svg">

The suite turns durable ICP and NNS maturity into durable cycles support. A controlled NNS neuron produces recurring maturity, the disburser stages that maturity as ICP, the faucet allocates the base ICP flow to memo-declared targets, and the relay helps keep the suite's own canisters funded before routing surplus ICP to configured neuron recipients. Historian and frontend canisters provide public observability, while small recovery/support canisters keep the value-moving path narrow and auditable.

## Protocol Overview

The operational path is intentionally split across small canisters:

- [`canisters/disburser`](canisters/disburser) controls one NNS neuron, disburses available maturity, and routes staged ICP into the fixed base/age-bonus recipients.
  <img src="/canisters/frontend/public/disburser.svg">
- [`canisters/faucet`](canisters/faucet) receives the base ICP flow, scans the configured staking account, interprets eligible transfer memos, and performs proportional payouts as cycles top-ups, raw ICP transfers, or NNS neuron stake transfers (the target canister/neuron and top-up mode is determined by the staking account transfer memo).
  <img src="/canisters/frontend/public/faucet.svg">
- [`canisters/relay`](canisters/relay) receives suite-funding ICP from the faucet, tops up managed suite canisters from recent cycles-burn observations plus carried recovery deficits, and routes remaining surplus only after canister recovery targets are met.
- [`canisters/historian`](canisters/historian) indexes commitment history, target canisters, cycles samples, SNS discovery, and dashboard-facing public state.
- [`canisters/frontend`](canisters/frontend) serves the certified public site and browser dashboard.
- [`canisters/lifeline`](canisters/lifeline) provides minimal recovery support.
- [`canisters/sns-rewards`](canisters/sns-rewards) maintains SNS owner snapshots and supplies snapshot-scoped reward context and account-owner lookups to Relay.

<img src="/canisters/frontend/public/jupiter-faucet-overview.svg">

At a high level, a participant declares a faucet target by transferring ICP to the configured staking account and placing a supported ASCII directive in `icrc1_memo`. Plain declared canister ID text is the primary cycles top-up form. The faucet also supports `canister_id.memo` for raw ICP routing and decimal NNS neuron IDs, optionally with `.memo`, for neuron staking-account top-ups. The exact eligibility, memo, fee, retry, and rescue rules live in the component READMEs:

- [`canisters/disburser/README.md`](canisters/disburser/README.md)
- [`canisters/faucet/README.md`](canisters/faucet/README.md)
- [`canisters/relay/README.md`](canisters/relay/README.md)

The value-moving canisters expose little or no public production API. Public verification and dashboard data are concentrated in [`canisters/historian`](canisters/historian), [`canisters/frontend`](canisters/frontend), public logs, source code, Candid files, and reproducible build artifacts.

Historian's self-service Relay factory accepts an immutable configuration of 1–20 managed target canisters and either zero or 1–5 typed surplus recipients. Each recipient is either a Principal, paid at `Account { owner: principal, subaccount: None }`, or a public NNS neuron ID, paid at `Account { owner: NNS Governance, subaccount: resolved staking subaccount }`; after a successful neuron transfer, Relay requests `claim_or_refresh`. Every recipient has an exact-byte memo of 0–32 bytes. Hexadecimal is authoritative for arbitrary bytes; Text entry is encoded exactly as UTF-8, and a mode switch is accepted only when the browser field round-trips to the same bytes. If Text cannot represent the bytes exactly, the frontend retains valid Hexadecimal mode without blocking submission. Canonical summaries always show exact lowercase hexadecimal and suppress optional text containing invisible, control, format, bidirectional, line-separator, or paragraph-separator characters. An empty memo means no outgoing Ledger memo. One explicitly framed canonical encoder determines the authoritative configuration hash and setup account for every configuration, including empty memos and zero recipients. Input order does not matter, while changing a recipient type, destination, or memo changes the immutable configuration. Zero recipients is represented by the empty recipient vector and selects Relay's all-cycles allocator, while routing mode still requires at least one recipient. All-cycles mode produces no raw-ICP surplus transfer. Creation pricing remains target-based, and no weights, custom subaccounts, or automatic IO recipient are supported. See [Relay setup and recovery](docs/relay-setup-recovery.md).

Every Relay also has intrinsic fixed splitter subaccounts 10–90. Funding splitter `P` routes a pinned gross `P%` budget to Relay's default account and the complement to its existing subaccount-1 Faucet staging account, with one ledger fee per leg. The two-leg operation is durably journaled before transfer and adds no install argument or public method. See the [Relay README](canisters/relay/README.md#workflow-4-fixed-splitter-subaccounts-1090).

## Source Verification and Reproducible Builds

Reproducible builds are part of the trust model for Jupiter Faucet. A deployed canister's Wasm module hash can be compared with locally rebuilt release artifacts so readers can connect public source code to the code running on the Internet Computer.

Start with [reproducible builds](docs/operations/reproducible-builds.md) for the scenario-based verification flow. If your goal is to compare this source checkout with mainnet, use the Docker-backed release path documented there; it prints the `.wasm.gz` installed package hashes that should match the live canister module hashes.

For canonical verification:

```bash
./tools/scripts/docker-build
```

For production deployment from canonical artifacts:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy <canister_name> --environment ic --mode upgrade
```

Routine no-config-change production upgrades pass no args for Disburser, Faucet, and Historian. Existing production Historian must be upgraded in place; reinstall destroys its stable histories, tracking metadata, self-service hash mappings and progress, cursors, and aggregates, and is prohibited for the existing production canister. Checked-in install argument files are fresh install/reinstall `InitArgs`, not routine upgrade inputs for those stateful canisters; `canisters/historian/mainnet-install-args.did` is for a brand-new Historian installation only. Config-changing upgrades use temporary canister-specific `Option<UpgradeArgs>` files where supported. Canonical Relay is replacement-style: every named Relay install, reinstall, or upgrade passes full `InitArgs`, initializes fresh heap state, and resets operational state. Blackholed self-service Relays are immutable and are never upgraded through Historian. The first successful post-upgrade tick of the canonical Relay is expected to be `BaselineOnly`.

The full lifecycle matrix is in [deployment operations](docs/operations/deployment.md#lifecycle-matrix). Canister-specific lifecycle notes live in each component README.

For local artifact work, direct local installs, frontend prototype deployment, and quick inspection:

```bash
./tools/scripts/build-canister all
```

`icp deploy` is the preferred production deployment orchestrator, but it is not itself reproducible-build proof unless it is fed artifacts already produced by the canonical Docker build. Docker access and mainnet canister visibility may be required for parts of an end-to-end verification workflow.

## Repository Layout

- [`canisters/`](canisters) - production IC canisters.
  - [`disburser/`](canisters/disburser) - NNS maturity staging and payout routing.
  - [`faucet/`](canisters/faucet) - staking-account scan, memo-derived registration, and CMC top-up flow.
  - [`historian/`](canisters/historian) - indexed public read model for dashboard and protocol history.
  - [`relay/`](canisters/relay) - suite cycles funding and surplus-routing support.
  - [`lifeline/`](canisters/lifeline) - minimal recovery/support canister.
  - [`sns-rewards/`](canisters/sns-rewards) - SNS owner-snapshot and Relay reward-context support canister.
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
# Development
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_all
npm run build:frontend
npm run test:frontend-unit
./tools/scripts/build-canister all

# Release verification
./tools/scripts/security-scan
python3 ./tools/scripts/validate-mainnet-install-args
./tools/scripts/docker-build
npm run verify:reproducible-artifacts
```

## Documentation

- [Canister roles](docs/architecture/canister-roles.md)
- [Testing](docs/development/testing.md)
- [Dependency scanning](docs/security/dependency-scanning.md)
- [Deployment](docs/operations/deployment.md)
- [Reproducible builds](docs/operations/reproducible-builds.md)

Canister-specific details remain next to each canister under [`canisters/*/README.md`](canisters).
