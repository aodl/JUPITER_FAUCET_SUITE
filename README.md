# Jupiter Faucet Suite

[Jupiter Faucet](https://www.jupiter-faucet.com/#intro) is a perpetual cycles top-up protocol and shared infrastructure layer for long-lived Internet Computer projects. It turns committed ICP and recurring NNS maturity into reusable funding routes so applications do not each need to build and operate their own neuron-management, maturity-routing, and cycles-management stack.

Applications remain responsible for their own business and incentive logic: which behaviour deserves rewards, how value should be weighted, how abuse is constrained, and which governance or product policies apply. Jupiter provides generic infrastructure beneath those decisions; it does not make them for the application.

The primary integration model is to use **the canonical Jupiter Faucet deployed on IC mainnet**. Projects can make commitments through the established productive-stake and maturity flow, share its public observability and review surface, and create a dedicated immutable Relay through the deployed frontend and Historian when they need managed multi-canister funding.

This repository contains the production canisters, certified frontend, shared crates, tests, and release tooling that implement and verify the suite. The implementation is split into narrow value-moving, allocation, observation, and support roles so each authority boundary can be reviewed independently.

![Jupiter Faucet](canisters/frontend/public/og/preview-20260520.jpg)

<img src="/canisters/frontend/public/perpetual-canister-topups.svg">

## Shared infrastructure for IC projects

Keeping an application running introduces generic work beyond its product logic: maintaining cycles buffers, managing an ICP reserve, harvesting and routing maturity, avoiding chronic overfunding, handling surplus, and making long-lived value-moving automation reviewable. Jupiter makes those concerns composable rather than requiring each project to engineer them from scratch.

- **Your project keeps:** application-specific reward eligibility, anti-sybil rules, business economics, product revenue policy, and project governance.
- **Jupiter can handle:** long-term commitment routes, NNS maturity routing, direct or managed cycles funding, raw ICP and supported neuron routes, observed-demand allocation, surplus execution, and public operational history.

The [shared-infrastructure architecture guide](docs/architecture/shared-infrastructure.md) explains this boundary, common composition patterns, the separate pre-launch IO relationship, and the concrete trust properties relevant to adopting shared infrastructure.

The suite overview shows what can be composed: one long-term commitment can produce recurring output, route directly to a target, or supply a Relay that allocates cycles according to measured demand. It also depicts the intended relationship with the separate IO project. IO is not yet release-ready or live.

<img src="/canisters/frontend/public/jupiter-faucet-overview.svg">

| If your project needs... | Start with... |
| --- | --- |
| Long-term cycles funding for one canister | A [direct Faucet commitment](https://www.jupiter-faucet.com/#how-it-works) |
| Funding for several canisters with changing demand | A Relay created through the [mainnet self-service factory](https://www.jupiter-faucet.com/#relay-setup) |
| A blend of immediately spendable and future funding | [Relay splitter funding](https://github.com/aodl/JUPITER_FAUCET_SUITE/tree/master/canisters/relay#funding-a-relay) plus Faucet |
| Surplus returned to a treasury account or public NNS neuron | [Relay surplus recipients](https://github.com/aodl/JUPITER_FAUCET_SUITE/tree/master/canisters/relay#surplus-recipients-and-reward-attribution) |
| A liquid-staking or reward asset | The [IO relationship](docs/architecture/shared-infrastructure.md#relationship-with-io), a separate pre-launch layer |

## Protocol Overview

The suite turns durable ICP and NNS maturity into durable cycles support. A controlled NNS neuron produces recurring maturity, the disburser stages that maturity as ICP, the faucet allocates the base ICP flow to memo-declared targets, and Relay can keep configured canisters funded before routing safely distributable surplus ICP to configured recipients. Historian and frontend canisters provide public observability, while small recovery/support canisters keep the value-moving path narrow and auditable.

The operational path is intentionally split across small canisters:

- [`canisters/disburser`](canisters/disburser) controls one NNS neuron, disburses available maturity, and routes staged ICP into the fixed base/age-bonus recipients. 
  <img src="/canisters/frontend/public/disburser.svg">
   - [D-QUORUM](https://dashboard.internetcomputer.org/neuron/4713806069430754115) is a special known neuron owned by the NNS governance canister itself. Jupiter Faucet's neuron follows D-QUORUM (indirectly via [αlpha-vote](https://dashboard.internetcomputer.org/neuron/2947465672511369) to maximise rewards) ensuring maturity is earned through diligent voting. Therefore a small portion of age bonus maturity is allocated to help incentivise elected NNS governance reviewers. **This partnership is foundational to Jupiter Faucet** because a prerequisite for truly unstoppable canisters is a secure and decentralized network.
     
- [`canisters/faucet`](canisters/faucet) receives the base ICP flow, scans the configured staking account, interprets eligible transfer memos, and performs proportional payouts as cycles top-ups, raw ICP transfers, or NNS neuron stake transfers (the target canister/neuron and top-up mode is determined by the staking account transfer memo).
  <img src="/canisters/frontend/public/faucet.svg">
- [`canisters/relay`](canisters/relay) receives suite-funding ICP from the faucet, tops up managed suite canisters from recent cycles-burn observations plus carried recovery deficits, and routes remaining surplus only after canister recovery targets are met.
- [`canisters/historian`](canisters/historian) indexes commitment history, target canisters, cycles samples, SNS discovery, and dashboard-facing public state.
- [`canisters/frontend`](canisters/frontend) serves the certified public site and browser dashboard.
- [`canisters/lifeline`](canisters/lifeline) provides minimal recovery support.
- [`canisters/sns-rewards`](canisters/sns-rewards) maintains SNS owner snapshots and supplies snapshot-scoped reward context and account-owner lookups to Relay.

At a high level, a participant declares a faucet target by transferring ICP to the configured staking account and placing a supported ASCII directive in `icrc1_memo`. Plain declared canister ID text is the primary cycles top-up form. The faucet also supports `canister_id.memo` for raw ICP routing and decimal NNS neuron IDs, optionally with `.memo`, for neuron staking-account top-ups. The exact eligibility, memo, fee, retry, and rescue rules live in the component READMEs:

- [`canisters/disburser/README.md`](canisters/disburser/README.md)
- [`canisters/faucet/README.md`](canisters/faucet/README.md)
- [`canisters/relay/README.md`](canisters/relay/README.md)

The value-moving canisters expose little or no public production API. Public verification and dashboard data are concentrated in [`canisters/historian`](canisters/historian), [`canisters/frontend`](canisters/frontend), public logs, source code, Candid files, and reproducible build artifacts.

Through Historian, users can create an immutable Relay to keep 1–20 canisters funded. Surplus ICP can be sent to as many as five principal or public NNS neuron recipients, each with an optional memo, or kept entirely in the cycles-funding loop by choosing no recipients. See [Relay setup and recovery](docs/relay-setup-recovery.md).

Users can also fund a Relay through fixed 10–90% splitter accounts, dividing a deposit between immediate Relay funding and Faucet staging. See the [Relay README](canisters/relay/README.md#splitter-subaccounts-1090).

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

Routine no-config-change production upgrades pass no args for Disburser, Faucet, and Historian. Existing production Historian must be upgraded in place; reinstall destroys its stable histories, tracking metadata, self-service hash mappings and progress, cursors, and aggregates, and is prohibited for the existing production canister. Checked-in install argument files are fresh install/reinstall `InitArgs`, not routine upgrade inputs for those stateful canisters; `canisters/historian/mainnet-install-args.did` is for a brand-new Historian installation only. Config-changing upgrades use temporary canister-specific `Option<UpgradeArgs>` files where supported. Canonical Relay is replacement-style: every named Relay install, reinstall, or upgrade passes full `InitArgs`, initializes fresh heap state, and resets operational state. Controllerless self-service Relays are immutable and cannot be upgraded. The first successful post-upgrade tick of the canonical Relay is expected to be `BaselineOnly`.

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

- [Shared infrastructure / why Jupiter Faucet exists](docs/architecture/shared-infrastructure.md)
- [Canister roles](docs/architecture/canister-roles.md)
- [Testing](docs/development/testing.md)
- [Dependency scanning](docs/security/dependency-scanning.md)
- [Deployment](docs/operations/deployment.md)
- [Reproducible builds](docs/operations/reproducible-builds.md)

Canister-specific details remain next to each canister under [`canisters/*/README.md`](canisters).
