# Jupiter Faucet Suite

Jupiter Faucet Suite is a set of Internet Computer canisters that turns the recurring ICP output of one NNS neuron into two long-lived on-chain flows:

1. a **cycles top-up flow** for participating canisters, handled by `jupiter-faucet`
2. an **age-bonus ICP flow** for Jupiter ecosystem rewards and NNS-aligned support, handled by `jupiter-disburser`

The design goal is to make the core payout path boring, predictable, and hard to tamper with. The operational canisters are narrowly scoped, intended to be self-managed after rollout, and paired with a separate recovery canister so the normal path can stay effectively immutable without becoming unrecoverable.

## What the suite does

The end-to-end flow is:

1. `jupiter-disburser` controls one NNS neuron.
2. On each successful main tick it disburses **100% of available maturity** to its own ICP ledger account.
3. It then routes that ICP according to a fixed policy:
   - the **base** portion goes to `jupiter-faucet`
   - the **age-bonus** portion is split **80/20** between `jupiter-sns-rewards` and the D-QUORUM neuron staking account
4. `jupiter-faucet` scans the configured staking account history, infers beneficiaries from transaction memos, and sends ICP to the CMC deposit subaccounts for those beneficiaries.
5. The Cycles Minting Canister (`CMC`) converts those deposits into cycles top-ups.

The current repository therefore contains both the **reward source** (`jupiter-disburser`) and the **distribution mechanism** (`jupiter-faucet`).

## How someone opts a canister into the faucet flow

At a high level, a participant:

1. transfers ICP into the faucet neuron’s configured `staking_account`
2. puts the **target canister principal** in the transfer memo

The contributor does **not** need to own the target canister. The faucet treats the memo as the beneficiary identifier and routes future top-ups proportionally to the amount attributed to that canister.

For the exact memo rules, payout math, retry semantics, and threshold handling, see [`jupiter-faucet/README.md`](jupiter-faucet/README.md).

## Production canisters recorded in this repo

These production canister IDs are recorded in `canister_ids.json`:

- `jupiter-disburser` — `uccpi-cqaaa-aaaar-qby3q-cai`
- `jupiter-faucet` — `acjuz-liaaa-aaaar-qb4qq-cai`
- `jupiter-lifeline` — `afisn-gqaaa-aaaar-qb4qa-cai`
- `jupiter-sns-rewards` — `alk7f-5aaaa-aaaar-qb4ra-cai`

These core canisters are deployed on the Fiduciary subnet:

- `pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`

`jupiter-faucet-frontend` exists in the workspace as a placeholder canister, but its production ID is not recorded in `canister_ids.json`.

## Canister map

### `jupiter-disburser`
Controls the NNS neuron, initiates maturity disbursement, persists deterministic ICP payout plans, applies the age-bonus routing policy, and owns the blackhole/recovery policy for that maturity-routing path.

See [`jupiter-disburser/README.md`](jupiter-disburser/README.md).

### `jupiter-faucet`
Receives the base ICP flow from `jupiter-disburser`, scans a configured staking account via the ICP index canister, attributes contributions to beneficiaries from memos, and performs CMC top-ups with persisted one-retry safety around the ledger and CMC boundaries.

See [`jupiter-faucet/README.md`](jupiter-faucet/README.md).

### `jupiter-lifeline`
Minimal recovery canister intended to hold the rescue-controller role for blackholed operational canisters.

See [`jupiter-lifeline/README.md`](jupiter-lifeline/README.md).

### `jupiter-sns-rewards`
Current placeholder canister that receives the primary age-bonus ICP flow. It is present mainly to reserve the production principal and ledger account until reward-distribution logic lands.

See [`jupiter-sns-rewards/README.md`](jupiter-sns-rewards/README.md).

### `jupiter-faucet-frontend`
Current placeholder for the eventual public asset/frontend canister.

See [`jupiter-faucet-frontend/README.md`](jupiter-faucet-frontend/README.md).

## Blackholing and recovery model

The operational path is designed around **self-management plus rescue handoff**, not around leaving core canisters permanently operator-controlled.

In practice that means:

- `jupiter-disburser` and `jupiter-faucet` are intended to reconcile to **self-only controllers** during healthy operation
- each canister persists enough local state to decide when value flow has stopped for long enough to justify rescue
- once the local rescue policy triggers, the canister widens its controller set to include `jupiter-lifeline`

This matters because the suite wants two properties at once:

- the normal payout policy should be hard to change
- recovery should still be possible if a system change or environmental failure breaks the payout path

The detailed rescue conditions differ slightly between the disburser and the faucet, so the component READMEs are the right canonical source:

- [`jupiter-disburser/README.md`](jupiter-disburser/README.md)
- [`jupiter-faucet/README.md`](jupiter-faucet/README.md)

## Current placeholders and deliberate omissions

A few components are intentionally minimal today:

- `jupiter-sns-rewards`
- `jupiter-faucet-frontend`

Those can largely be ignored when trying to understand the live operational path.

One important repo gap is that the committed mainnet install arguments are only provided for `jupiter-disburser` (`jupiter-disburser/mainnet-install-args.did`). The faucet’s install-time configuration schema is documented in its README, but a production `mainnet-install-args.did` for `jupiter-faucet` is not currently committed in this repo.

## Repository layout

- `jupiter-disburser/` — maturity-routing canister
- `jupiter-faucet/` — cycles top-up canister
- `jupiter-lifeline/` — recovery canister
- `jupiter-sns-rewards/` — placeholder rewards canister
- `jupiter-faucet-frontend/` — placeholder frontend canister
- `xtask/` — local orchestration, mocks, integration suites, and end-to-end suites
- `scripts/` — reproducible build helpers
- `release-artifacts/` — generated build output

## Build and test

### Reproducible release artifacts

```bash
chmod +x scripts/docker-build scripts/build-canister
./scripts/docker-build
```

That produces canonical release artifacts under `release-artifacts/`.

### Local one-off builds

```bash
./scripts/build-canister all
```

Or build a single canister, for example:

```bash
./scripts/build-canister jupiter-disburser
```

### Test orchestration

The preferred entry point for local testing is `xtask`.

Examples:

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_dfx_integration
cargo run -p xtask -- test_pocketic_integration
cargo run -p xtask -- test_all
```

For the full command matrix and what each layer covers, see [`xtask/README.md`](xtask/README.md).

## Suggested reading order

For a fast technical orientation, read the docs in this order:

1. this file
2. [`jupiter-disburser/README.md`](jupiter-disburser/README.md)
3. [`jupiter-faucet/README.md`](jupiter-faucet/README.md)
4. [`xtask/README.md`](xtask/README.md)

That gives the system-level picture first, then the two canisters that actually move value, then the practical test surface that proves the current behavior.
