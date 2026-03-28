# Jupiter Faucet Suite

[Jupiter Faucet](https://jupiter-faucet.com/#intro) is a perpetual cycles top-up protocol for the Internet Computer. Its goal is simple: turn a durable ICP source into durable cycles for canisters, while keeping the value-moving path narrow, deterministic, and hard to tamper with.

In the current production design, one NNS neuron is the economic source of truth. The suite uses that neuron’s recurring maturity to sustain two long-lived value flows, alongside a separate observability path:

1. a **cycles top-up flow** for participating canisters, handled by `jupiter-faucet`
2. an **age-bonus ICP flow** for Jupiter ecosystem rewards and NNS-aligned support, handled by `jupiter-disburser`
3. a **historical indexing and observability path** for tracked canisters, handled by `jupiter-historian`

The operational canisters are intentionally small and specialized. The normal path is designed to settle into self-management plus canonical blackhole control, with a separate recovery canister available if value flow stops for long enough that rescue is justified.

## System map

### Operational path

- `jupiter-disburser`
  - controls one NNS neuron
  - disburses **100% of available maturity** to its own default ICP ledger account
  - applies the fixed base/bonus routing policy
- `jupiter-faucet`
  - receives the base ICP flow from `jupiter-disburser`
  - scans a configured staking account through the ICP index canister
  - treats eligible memos as beneficiary canister principals
  - transfers ICP to CMC deposit subaccounts and calls `notify_top_up`

### Observability path

- `jupiter-historian`
  - incrementally indexes the same staking account used by the faucet
  - keeps memo-derived and SNS-discovered canister sets
  - records capped contribution history, burn history, and cycles samples
  - exposes the public read model used by the production frontend
- `jupiter-faucet-frontend`
  - serves the public site as certified assets
  - loads dashboard data through generated Candid declarations
  - reads from `jupiter-historian`, the configured ledger canister, and NNS Governance

### Recovery / support canisters

- `jupiter-lifeline`
  - minimal recovery canister intended to be added as a controller only when rescue is required
- `jupiter-sns-rewards`
  - current placeholder recipient for the primary age-bonus ICP flow
  - present today mainly to reserve the production principal and default ledger account until reward logic lands

## End-to-end value flow

The live value-moving path is:

1. `jupiter-disburser` controls one NNS neuron.
2. On a successful main tick, it first drains any already-disbursed ICP sitting in its own default ledger account according to the currently persisted payout plan, if one exists.
3. If NNS does **not** already report a maturity disbursement in flight, the disburser initiates `DisburseMaturity` for **100%** of available maturity to its own default ledger account.
4. On the next payout stage, that staged ICP is split into:
   - the **age-neutral base** share for `jupiter-faucet`
   - **80% of the age bonus** for `jupiter-sns-rewards`
   - **20% of the age bonus** for the D-QUORUM neuron staking account
5. `jupiter-faucet` periodically snapshots:
   - its own payout-account ICP balance
   - the configured staking-account ICP balance
6. It scans the staking-account transaction history from the beginning, evaluates each eligible incoming transfer independently, and derives the beneficiary from the memo.
7. For each eligible contribution whose computed share is larger than the ledger fee, the faucet sends ICP to the beneficiary’s CMC deposit subaccount and then calls `notify_top_up`.
8. The CMC converts those deposits into cycles top-ups.

For the exact split math, memo formats, retry semantics, and rescue logic, the component READMEs are the canonical source:

- [`jupiter-disburser/README.md`](jupiter-disburser/README.md)
- [`jupiter-faucet/README.md`](jupiter-faucet/README.md)

## How a canister opts into the faucet flow

At a high level, a participant:

1. transfers ICP into the faucet neuron’s configured `staking_account`
2. puts the **target canister principal** in the transfer memo

The contributor does **not** need to own the target canister. The faucet only cares that the memo decodes to principal text for the beneficiary canister.

Important details that matter in practice:

- the faucet prefers `icrc1_memo`
- if `icrc1_memo` is absent, it falls back to the legacy numeric memo bytes when that numeric memo is non-zero
- empty, malformed, or non-principal memos are ignored
- contributions below `min_tx_e8s` are ignored
- each eligible contribution is processed independently; same-beneficiary contributions are **not** aggregated into one synthetic record
- each new payout job rescans the full staking history against a fresh payout-pot snapshot

See [`jupiter-faucet/README.md`](jupiter-faucet/README.md) for the exact rules and examples.

## What each canister exposes in production

- `jupiter-disburser`
  - no public production methods beyond installation / upgrade
- `jupiter-faucet`
  - no public production methods beyond installation / upgrade
- `jupiter-historian`
  - public read-only query API for counts, status, histories, summaries, recent contributions, and recent burns
- `jupiter-faucet-frontend`
  - `http_request` for certified asset serving
- `jupiter-lifeline`
  - no public methods
- `jupiter-sns-rewards`
  - no public methods

That split is intentional: the value-moving canisters stay narrow; the public read surface lives in the historian and frontend.

## Production canisters recorded in this repo

These production canister IDs are recorded in `canister_ids.json`:

- `jupiter-disburser` — `uccpi-cqaaa-aaaar-qby3q-cai`
- `jupiter-faucet` — `acjuz-liaaa-aaaar-qb4qq-cai`
- `jupiter-historian` — `j5gs6-uiaaa-aaaar-qb5cq-cai`
- `jupiter-lifeline` — `afisn-gqaaa-aaaar-qb4qa-cai`
- `jupiter-sns-rewards` — `alk7f-5aaaa-aaaar-qb4ra-cai`
- `jupiter-faucet-frontend` — `jufzc-caaaa-aaaar-qb5da-cai`

These core canisters are deployed on the Fiduciary subnet:

- `pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`

## External system canisters and dependencies

The suite relies on the following external canisters and protocols:

- ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai` by default)
- ICP Index (`qhbym-qaaaa-aaaaa-aaafq-cai` by default)
- NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai` by default)
- Cycles Minting Canister / CMC (`rkp4c-7iaaa-aaaaa-aaaca-cai` by default)
- canonical blackhole canister (`e3mmv-5qaaa-aaaah-aadma-cai` by default)
- SNS-WASM (`qaa6y-5yaaa-aaaaa-aaafa-cai` by default, for historian SNS discovery)

The frontend also depends on generated JS declarations for the historian and a ledger actor surface.

## Blackholing and recovery model

The suite is designed around **self-management plus rescue handoff**, not around leaving operational canisters permanently operator-controlled.

In practice that means:

- `jupiter-disburser` and `jupiter-faucet` are intended to converge to **self + blackhole** controllers while healthy
- each canister persists enough local state to decide when value flow has stopped for long enough that rescue is justified
- once rescue triggers, the canister widens its controller set to include `jupiter-lifeline` alongside `self + blackhole`

Both value-moving canisters use the same basic time windows when blackhole mode is armed:

- healthy: `<= 7 days` since the last successful transfer / top-up notification
- middle window: `> 7 days` and `<= 14 days`
- broken: `> 14 days`

But the faucet also has extra forced-rescue latches tied to index-health and zero-success CMC runs, so the component docs remain the canonical source.

## Current placeholders and deliberate omissions

The following components are intentionally minimal today:

- `jupiter-sns-rewards`
- `jupiter-lifeline` (recovery code is expected to be added only if a real rescue event occurs)

Those can largely be ignored when building a first-principles understanding of the live operational path.

## Repository layout

- `jupiter-disburser/` — maturity-routing canister
- `jupiter-faucet/` — cycles top-up canister
- `jupiter-historian/` — contribution, burn, and cycles-history canister
- `jupiter-faucet-frontend/` — certified public frontend canister and browser bundle source
- `jupiter-lifeline/` — recovery canister
- `jupiter-sns-rewards/` — placeholder rewards canister
- `xtask/` — local orchestration, mocks, integration suites, and PocketIC suites
- `scripts/` — repository build helpers
- `third_party/` — vendored dependencies used by the build and test paths
- `release-artifacts/` — generated release output

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

Notes:

- the frontend build requires Node.js / npm because the browser bundle is built before the Rust asset canister is compiled
- the build helper stamps content-hashed frontend bundle paths and an asset version into the checked-in static assets before compiling the frontend canister

To build a single canister, for example:

```bash
./scripts/build-canister jupiter-disburser
```

### Test orchestration

The preferred entry point for local Rust/canister testing is `xtask`.

Examples:

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_dfx_integration
cargo run -p xtask -- test_pocketic_integration
cargo run -p xtask -- test_all
```

Browser-only frontend tests live in npm scripts rather than `xtask`:

```bash
npm run test:frontend-dashboard
npm run test:frontend-dashboard-local
```

For the full command matrix and what each layer covers, see [`xtask/README.md`](xtask/README.md).

## Committed mainnet init / install args

Committed copy-pasteable install args live alongside the main operational canisters:

- `jupiter-disburser/mainnet-install-args.did`
- `jupiter-faucet/mainnet-install-args.did`
- `jupiter-historian/mainnet-install-args.did`

These are the repo’s source of truth for the current production wiring captured in version control.

## Suggested reading order

For a fast technical orientation, read the docs in this order:

1. this file
2. [`jupiter-disburser/README.md`](jupiter-disburser/README.md)
3. [`jupiter-faucet/README.md`](jupiter-faucet/README.md)
4. [`jupiter-historian/README.md`](jupiter-historian/README.md)
5. [`jupiter-faucet-frontend/README.md`](jupiter-faucet-frontend/README.md)
6. [`xtask/README.md`](xtask/README.md)

That gives the system view first, then the value-moving path, then the public read model, then the production UI and the test harness that proves the current behavior.
