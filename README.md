# Jupiter Faucet Suite

Jupiter Faucet is comprised of a suite of Internet Computer canisters for routing NNS neuron rewards into a fixed downstream value flow.

At the center of the system is `jupiter-disburser`, which controls a single NNS neuron, disburses maturity, and routes the resulting ICP according to a fixed policy. The remaining canisters exist to preserve that value flow, keep the operational path narrow, and provide a controlled recovery mechanism if a system dependency changes unexpectedly.

## System overview

The intended production topology is:

- `jupiter-disburser` controls one NNS neuron and routes minted ICP
- `jupiter-faucet` receives the base maturity flow and converts it into cycles top-ups
- `jupiter-sns-rewards` receives the SNS-directed bonus ICP flow and distributes it to JUP SNS stakers
- `jupiter-faucet-frontend` serves the public interface for the faucet side of the system
- `jupiter-lifeline` is the recovery controller target for blackholed operational canisters

In normal operation, the flow is:

1. ICP is committed to the NNS neuron controlled by `jupiter-disburser`
2. `jupiter-disburser` periodically disburses maturity to its staging account
3. the age-neutral base portion is routed to `jupiter-faucet`
4. the age-bonus portion is split between the configured bonus recipients
5. one bonus recipient can be `jupiter-sns-rewards`, which then distributes that ICP according to its own staking policy
6. `jupiter-faucet` converts its incoming ICP into cycles and tops up target canisters proportionally to the stake commitments encoded in the original neuron-funding flow

## Components

### `jupiter-disburser`

`jupiter-disburser` is the canister that controls the NNS neuron.

Its job is to:

- initiate `DisburseMaturity` calls against the configured neuron
- receive minted ICP into a staging account controlled by the canister
- derive the age-neutral base component and the age-bonus component from the amount actually minted
- route the resulting ICP across a fixed set of recipients
- preserve enough local state to resume safely if execution stops part-way through payout
- trigger lifeline-based controller recovery if transfer flow stops for long enough to satisfy the rescue policy

The current split model is:

- normal recipient receives the age-neutral base portion
- bonus recipient 1 receives 80% of the age bonus portion
- bonus recipient 2 receives 20% of the age bonus portion

Detailed design and operational notes for this canister belong in `jupiter-disburser/README.md`.

### `jupiter-faucet`

`jupiter-faucet` is the destination for the base maturity payout.

Its role in the full system is to:

- receive ICP from `jupiter-disburser`
- convert that ICP into cycles
- top up target canisters proportionally to the stake commitments associated with the original neuron funding

The placeholder canister in this repository exists so it can be deployed, assigned a principal, and integrated into the full system while the production logic is being built.

### `jupiter-sns-rewards`

`jupiter-sns-rewards` is the intended destination for one of the bonus maturity outputs from `jupiter-disburser`.

Its role in the full system is to:

- receive ICP routed from `jupiter-disburser`
- calculate each JUP SNS staker's share according to its own reward logic
- distribute ICP to those stakers

The current repository contains a deployable placeholder canister so that its principal and default canister account can already be used in configuration.

### `jupiter-faucet-frontend`

`jupiter-faucet-frontend` serves the user-facing website for the faucet side of the system.

The current repository contains a deployable placeholder canister for principal allocation and integration work.

### `jupiter-lifeline`

`jupiter-lifeline` is the recovery controller target for blackholed operational canisters.

It is intentionally minimal. Under normal conditions it does not perform recovery work. Its purpose is to exist as a durable, pre-positioned controller target so that recovery authority can be activated if a blackholed canister needs to be recovered after an unexpected dependency failure.

The expected operating model is:

- keep the lifeline canister simple and durable during normal operation
- only introduce incident-specific recovery logic if a real lifeline event occurs
- if a lifeline event occurs, inspect the exact failure mode and upgrade `jupiter-lifeline` with narrowly-scoped recovery code for that incident

## Why `jupiter-disburser` and `jupiter-faucet` are blackholed

`jupiter-disburser` and `jupiter-faucet` are intended to be blackholed in production after deployment has been validated.

The purpose of blackholing is to make the core maturity-routing and cycle-top-up path operationally immutable during normal operation. That reduces the risk that a successful governance attack against a related control surface could alter how ICP is routed, converted to cycles, or applied as top-ups.

For `jupiter-faucet` in particular, blackholing helps preserve the guarantee that incoming ICP is only processed according to the fixed conversion and top-up rules encoded in the deployed canister. The goal is an immutable cycle top-up path even under an unlikely but successful governance attack elsewhere in the broader Jupiter ecosystem.

Blackholing is not used to eliminate recovery options entirely. Instead, recovery authority is pre-positioned through `jupiter-lifeline`, which allows the operational path to stay immutable in normal conditions while still leaving a controlled recovery path for unexpected failures.

## Lifeline-based recovery model

Both `jupiter-disburser` and `jupiter-faucet` are expected to use the same recovery pattern.

Each canister is configured so that, if value flow stops for long enough to satisfy its local rescue policy, it can widen its controller set to include `jupiter-lifeline`. This activates a recovery path without relying on a human-controlled principal in the normal operating path.

The recovery trigger is intentionally based on elapsed time since the last confirmed successful transfer recorded in canister state. It does not depend on a fresh ledger or governance API health check at the point of escalation.

That design avoids a circular failure mode where a system dependency change could both:

- stop normal transfer flow
- and also prevent the canister from proving that transfer flow has stopped

By basing the rescue decision on already-persisted local state, the controller handoff remains available even if a dependency API changes in a way that breaks the ordinary payout path.

## `jupiter-disburser` design notes

The key design decisions for `jupiter-disburser` are:

### Staging first, payout second

NNS Governance disburses maturity to one destination per call. `jupiter-disburser` always disburses to its own staging account first and only then performs the recipient split on the ledger side.

That keeps the payout logic grounded in the amount actually minted and makes retries much easier to reason about than repeated governance calls.

### Persistent payout plans

Before the first transfer is attempted, the canister persists a payout plan. If a transfer succeeds but execution stops before the plan is cleared, the next run can safely resume without recalculating the split from a partially updated state.

### Idempotent timers

Recurring timers drive the canister, but safety comes from state-based idempotence rather than from assuming that timers fire exactly on schedule.

### Age bonus model

`jupiter-disburser` follows the NNS age-bonus ramp:

- age 0 years: `1.00`
- age 2 years: `1.125`
- age 4 years and above: `1.25`

The multiplier increases linearly from `1.00` to `1.25` over the first four years, then clamps. The canister records the neuron age at the point of disbursement initiation so that the later split is based on the age that produced the reward.

## Reproducible builds and verification

The release build is produced inside a pinned Docker environment.

The repository build flow is intended to output reproducible artifacts for the full suite. For production canisters that support compressed installation, the release flow uses:

- an uncompressed `.wasm` artifact as the installed module reference for hash comparison
- a compressed `.wasm.gz` artifact as the deployment package

The standard release build entrypoint is:

```bash
chmod +x scripts/docker-build scripts/build-canister
./scripts/docker-build
```

Verification is performed by comparing the SHA-256 of the rebuilt uncompressed Wasm module with the installed module hash reported on-chain.

## Repository layout

The repository is intended to be structured as a suite root with one directory per canister.

```text
jupiter-disburser/
jupiter-lifeline/
jupiter-faucet/
jupiter-sns-rewards/
jupiter-faucet-frontend/
scripts/
xtask/
release-artifacts/
```

Typical responsibilities:

- `jupiter-disburser/` — disburser canister source, Candid files, and component documentation
- `jupiter-lifeline/` — minimal lifeline canister used as the rescue controller target
- `jupiter-faucet/` — faucet canister source
- `jupiter-sns-rewards/` — SNS rewards canister source
- `jupiter-faucet-frontend/` — frontend canister source
- `scripts/` — reproducible build helpers
- `xtask/` — local orchestration, mocks, and integration tooling
- `release-artifacts/` — reproducible build outputs

## Testing strategy

The suite is expected to maintain three layers of tests:

### Unit tests

Component-level correctness tests that live with each canister.

### Component integration tests

Local integration scenarios using `dfx`, mocks, and targeted flows for a single canister.

### System end-to-end tests

Full-flow PocketIC scenarios that cover the cross-canister path from neuron funding, to maturity routing, to faucet conversion and top-up behavior, to SNS reward routing.

## Deployment pattern

A production deployment should include the lifeline canister before operational canisters are installed in their final configuration.

For blackholed operational canisters, the intended pattern is:

1. deploy `jupiter-lifeline`
2. record its principal
3. install `jupiter-disburser` and `jupiter-faucet` with `jupiter-lifeline` configured as the rescue controller target
4. validate ordinary flow
5. blackhole the operational canisters once configuration has been confirmed

## Component documentation

The root README describes the Jupiter suite as a whole.

Detailed component documentation should live next to each canister, for example:

- `jupiter-disburser/README.md`
- `jupiter-lifeline/README.md`
- `jupiter-faucet/README.md`
- `jupiter-sns-rewards/README.md`
- `jupiter-faucet-frontend/README.md`

