# Jupiter Disburser

`jupiter-disburser` is the maturity-routing canister in the Jupiter Faucet Suite.

It controls one NNS neuron, periodically disburses maturity as ICP, and routes that ICP according to a fixed three-recipient policy:

- the **age-neutral base** payout goes to `jupiter-faucet`
- **80% of the age-bonus** goes to `jupiter-sns-rewards`
- **20% of the age-bonus** goes to the D-QUORUM neuron staking account

This canister is intentionally narrow in scope. It does not top up cycles directly and it does not expose a production public API.

See the suite overview in [`../README.md`](../README.md).

## What it is responsible for

`jupiter-disburser` owns four things:

1. reading the NNS neuron state
2. initiating `DisburseMaturity` to its own default ledger account
3. splitting the resulting ICP across the configured recipients
4. managing self-controller vs rescue-controller reconciliation when blackhole mode is armed

It also performs best-effort NNS maintenance calls that help the neuron reflect fresh staking-account deposits:

- `RefreshVotingPower` after a successful maturity-disbursement initiation
- `ClaimOrRefresh` on every successful main tick

## Runtime model

### Main tick sequence

On each successful main tick, the canister does the following:

1. acquires a short-lived main-tick lease so overlapping timer runs do not race
2. reads the configured neuron from NNS Governance
3. checks whether NNS already reports a maturity disbursement in progress
4. if **no** disbursement is in flight:
   - processes any staged ICP already sitting in the canister’s default ledger account
   - initiates `DisburseMaturity` for **100%** of available maturity to the canister’s default account
   - records the neuron age used for the *next* payout split
   - best-effort calls `RefreshVotingPower`
5. best-effort calls `ClaimOrRefresh` on every successful tick, regardless of whether a maturity disbursement was already in flight
6. logs only errors plus a single `Cycles: ...` line per run

Default timer cadence:

- `main_interval_seconds = 86400`
- `rescue_interval_seconds = 86400`

## Payout policy

### Age multiplier

The payout split is based on the neuron age captured when maturity disbursement is successfully initiated.

The multiplier used by the code is:

`1 + min(age, 4 years) / 16 years`

So:

- age `0` gives a multiplier of `1.0x`
- age `4 years` gives a multiplier of `1.25x`
- age above `4 years` is clamped and still gives `1.25x`

### Base and bonus split

Given a total staged ICP amount:

- `base = floor(total / multiplier)`
- `bonus = total - base`
- `bonus80 = ceil(0.8 * bonus)`
- `bonus20 = bonus - bonus80`

That means the 80% side receives any rounding bias.

### Memo format for outgoing ICP transfers

Outgoing ledger transfers use a deterministic 16-byte memo:

- bytes `0..8` = payout ID, big-endian
- bytes `8..16` = transfer index, big-endian

This memo, together with deterministic `created_at_time`, is part of the duplicate-proof retry model.

## Transfer planning and retry semantics

### Staging account

The canister receives disbursed maturity into its **default ledger account** (`subaccount = None`).

If that balance is zero on a payout pass, any stale persisted payout plan is cleared and the payout stage succeeds immediately.

### Persisted payout plan

When staged ICP is present and there is no existing plan, the canister:

1. reads the current ledger fee
2. computes the gross three-way split from the full staged balance
3. creates up to three planned transfers
4. persists that plan in canister state before executing it

A planned transfer is only created if its gross share is **strictly greater** than the ledger fee. Shares at or below the fee are skipped and remain in staging.

### Execution behavior

The canister then executes pending transfers in order until the plan completes or a transient failure occurs.

Important properties:

- transfer status is persisted per transfer
- `Duplicate` is treated as success and the duplicate block index is recorded
- `TemporarilyUnavailable` keeps the plan and retries later
- `BadFee`, `TooOld`, and `CreatedInFuture` clear the plan so the next run rebuilds it from the current fee and current staged balance
- other failures leave the run incomplete and are retried later
- once all plan entries are marked sent, the whole plan is cleared

This gives the canister deterministic, duplicate-safe retry behavior without having to reconstruct intent from logs.

## Rescue and blackhole policy

### Intended controller model

The intended healthy-state controller set for `jupiter-disburser` is:

- `jupiter-disburser` itself
- the canonical blackhole canister

When rescue is required, the canister widens that controller set to:

- `jupiter-lifeline`
- the canonical blackhole canister
- `jupiter-disburser`

### Time windows

When blackhole mode is armed, controller reconciliation follows this policy based on `last_successful_transfer_ts`:

- healthy: `<= 7 days` since last successful transfer → blackhole + self
- middle window: `> 7 days` and `<= 14 days` → no controller change
- broken: `> 14 days` → blackhole + rescue controller + self

There is also a bootstrap rescue condition:

- if blackhole mode has been armed for more than `14 days` and the canister has **never** recorded a successful transfer, rescue is forced with reason `BootstrapNoSuccess`

### Operational precondition before blackholing

Do **not** rely on the healthy blackhole+self controller posture until at least one successful payout transfer has occurred.

Without a recorded successful transfer, the time-based rescue policy has no proof that value flow ever worked.

## Install-time configuration

The init args are:

- `neuron_id`
  - the NNS neuron controlled by this canister
- `normal_recipient`
  - recipient of the age-neutral base payout
- `age_bonus_recipient_1`
  - recipient of the 80% age-bonus share
- `age_bonus_recipient_2`
  - recipient of the 20% age-bonus share
- `ledger_canister_id` (defaults to ICP Ledger)
- `governance_canister_id` (defaults to NNS Governance)
- `rescue_controller`
- `blackhole_controller`
- `blackhole_armed`
- `main_interval_seconds`
- `rescue_interval_seconds`

A copy-pasteable mainnet install args file is committed at:

- [`mainnet-install-args.did`](mainnet-install-args.did)

### Current production wiring recorded in this repo

The committed mainnet install args currently wire:

- normal recipient: `jupiter-faucet` (`acjuz-liaaa-aaaar-qb4qq-cai`)
- age bonus recipient 1: `jupiter-sns-rewards` (`alk7f-5aaaa-aaaar-qb4ra-cai`)
- age bonus recipient 2: D-QUORUM staking account on NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai` plus subaccount from the committed args file)
- rescue controller: `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)
- blackhole controller: canonical blackhole (`e3mmv-5qaaa-aaaah-aadma-cai`)
- ledger canister: ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- governance canister: NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai`)
- `blackhole_armed = false`
- `main_interval_seconds = 86400`
- `rescue_interval_seconds = 86400`

## Public interface

Production builds expose **no public methods**.

You can verify that with:

```bash
candid-extractor target/wasm32-unknown-unknown/release/jupiter_disburser.wasm > verify_no_endpoints.did
```

Debug-only methods are gated behind the `debug_api` feature and are intended for local integration and PocketIC tests only.

## Build and test

### Production build

```bash
cargo build -p jupiter-disburser --target wasm32-unknown-unknown --release --locked
```

### Debug build

```bash
cargo build -p jupiter-disburser --target wasm32-unknown-unknown --release --features debug_api --locked
```

### Unit tests

```bash
cargo test -p jupiter-disburser --lib
```

### Preferred integration and end-to-end entry points

```bash
cargo run -p xtask -- disburser_dfx_integration
cargo run -p xtask -- disburser_pocketic_integration
cargo run -p xtask -- disburser_all
```

Those cover, among other things:

- maturity disbursement landing in staging
- payout-plan persistence and duplicate-proof retry
- upgrade mid-flight behavior
- blackhole timer progression
- blackhole/rescue-controller round-trips
- age-bonus routing at 0y, 2y, and 4y
- `ClaimOrRefresh` / `RefreshVotingPower` behavior

For the broader test matrix, see [`../xtask/README.md`](../xtask/README.md).

## Reproducible builds and deployment

### Canonical release build

```bash
chmod +x ../scripts/docker-build ../scripts/build-canister
../scripts/docker-build
```

### Install release artifact

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --wasm release-artifacts/jupiter_disburser.wasm.gz \
  --argument-file jupiter-disburser/mainnet-install-args.did
```

### Upgrade release artifact

Upgrades preserve existing state and do not require install args unless you are intentionally toggling upgrade-only settings.

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

### Toggle blackhole arming via upgrade args

Upgrade args may also set `blackhole_controller` when migrating an older deployment onto the canonical blackhole-backed controller model.

Arm:

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --argument '(opt record { blackhole_armed = opt true; })' \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

Disarm:

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --argument '(opt record { blackhole_armed = opt false; })' \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

Clear a latched forced-rescue reason:

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --argument '(opt record { clear_forced_rescue = opt true; })' \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

### Controller handoff notes

Before handing the canister off to self-management, the deployment operator must still configure canister settings such as public log visibility and ensure the canister is a controller of itself before the healthy controller set converges to `self + blackhole`.

Example:

```bash
dfx canister update-settings jupiter_disburser --network ic --log-visibility public
dfx canister update-settings jupiter_disburser --network ic --add-controller uccpi-cqaaa-aaaar-qby3q-cai
```

After blackhole mode is armed and at least one successful payout transfer has been recorded, the operator can hand the canister off to the healthy `self + blackhole` controller set:

```bash
dfx canister update-settings jupiter_disburser \
  --network ic \
  --set-controller uccpi-cqaaa-aaaar-qby3q-cai \
  --add-controller e3mmv-5qaaa-aaaah-aadma-cai
```

## Related docs

- suite overview: [`../README.md`](../README.md)
- faucet mechanics: [`../jupiter-faucet/README.md`](../jupiter-faucet/README.md)
- local testing: [`../xtask/README.md`](../xtask/README.md)
