# Jupiter Disburser

`jupiter-disburser` is the maturity-routing canister in the Jupiter Faucet Suite.

It controls one NNS neuron, periodically disburses maturity as ICP, and routes that ICP according to a fixed three-recipient policy:

- the **age-neutral base** payout goes to `jupiter-faucet`
- **80% of the age bonus** goes to `jupiter-sns-rewards`
- **20% of the age bonus** goes to the D-QUORUM neuron staking account

This canister is intentionally narrow in scope. It does not top up cycles directly and it does not expose a public production API.

See the suite overview in [`../README.md`](../README.md).

## Role in the suite

`jupiter-disburser` owns four things:

1. reading the configured NNS neuron
2. initiating `DisburseMaturity` to its own default ICP ledger account
3. splitting staged ICP into the configured recipients
4. reconciling healthy vs rescue controller sets when blackhole mode is armed

It also performs two best-effort NNS maintenance calls that are useful but intentionally non-blocking:

- `RefreshVotingPower` after a successful maturity-disbursement initiation
- `ClaimOrRefresh` on every successful main tick

## External dependencies

By default the canister talks to:

- ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai`)
- canonical blackhole (`e3mmv-5qaaa-aaaah-aadma-cai`) when blackhole mode is configured / armed

All three can be overridden at install time except the blackhole default itself, which is only used when no explicit controller is provided.

## Runtime model

### Timer cadence

Default timers are:

- `main_interval_seconds = 86400` (daily)
- `rescue_interval_seconds = 86400` (daily)

Each timer interval is clamped to at least 60 seconds by the runtime code.

### Main tick sequence

On each successful main tick, the canister does the following:

1. acquires a 15-minute main-tick lease so overlapping timer runs do not race
2. reads the configured neuron from NNS Governance
3. checks whether NNS already reports a maturity disbursement in progress
4. if **no** disbursement is in flight:
   - processes any staged ICP already sitting in the canister’s default ledger account
   - initiates `DisburseMaturity` for **100%** of available maturity to the canister’s default account
   - records the neuron age that will be used for the *next* payout split
   - best-effort calls `RefreshVotingPower`
5. if a maturity disbursement **is** already in flight, the canister intentionally skips payout processing and does not initiate another disbursement
6. best-effort calls `ClaimOrRefresh` on every successful tick, regardless of whether a maturity disbursement was already in flight
7. logs only errors plus a single `Cycles: ...` line per run

The skip while in flight is intentional. The current implementation stores exactly one captured age snapshot (`prev_age_seconds`) and later uses that snapshot when staged ICP is split. By refusing to overlap payout work with an already in-flight maturity disbursement, the canister avoids applying the wrong captured age to staged ICP from a different disbursement cycle.

## Payout policy

### Staging account

The canister receives disbursed maturity into its **default ledger account**:

- owner = `jupiter-disburser`
- subaccount = `None`

The code always treats that default account as the staging area for pending ICP.

If the staged balance is zero on a payout pass, any stale persisted payout plan is cleared and the payout stage succeeds immediately.

### Age multiplier

The split depends on the neuron age captured when `DisburseMaturity` is successfully initiated.

The code uses:

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

That means the 80% side receives the rounding bias.

### Outgoing transfer memo format

Outgoing ledger transfers use a deterministic 16-byte memo:

- bytes `0..8` = payout ID, big-endian
- bytes `8..16` = transfer index, big-endian

The code also uses deterministic `created_at_time` values derived from the payout-plan creation time and transfer index.

Together, those two fields are part of the duplicate-proof retry model.

The planner intentionally streams the three configured recipient shares in a single pass and does **not** try to coalesce identical recipient accounts. That is a deliberate memory/simplicity tradeoff rather than an oversight: the payout plan stays compact and deterministic, at the cost that intentionally duplicated recipients would pay duplicate ledger fees.

## Transfer planning and retry semantics

### Persisted payout plan

When staged ICP is present and there is no existing plan, the canister:

1. reads the current ledger fee
2. computes the gross three-way split from the full staged balance
3. creates up to three planned transfers
4. persists that plan in canister state before executing it

A planned transfer is only created if its gross share is **strictly greater** than the ledger fee. Shares at or below the fee are skipped and remain in staging for a later payout stage.

### Execution behavior

The canister executes pending transfers in order until the plan completes or a retry-worthy failure occurs.

Important properties:

- transfer status is persisted per transfer
- `Duplicate` is treated as success and the returned block index is recorded
- `TemporarilyUnavailable` keeps the plan and retries later
- non-transport typed ledger rejections (`BadFee`, `BadBurn`, `InsufficientFunds`, `TooOld`, `CreatedInFuture`, `GenericError`) clear the plan so the next run rebuilds it from the current fee and current staged balance
- transport / call failures abort the run and leave the plan for a later retry
- once all transfers are marked sent, the whole plan is cleared

This gives deterministic, duplicate-safe retry behavior without reconstructing intent from logs.

The clear-and-rebuild policy is intentional: it prefers liveness over preserving the exact original split when a persisted plan becomes invalid mid-execution. If transfer `0` already succeeded and transfer `1` later hits a typed ledger rejection, the next run recomputes shares from the remaining staged balance rather than attempting to force the old three-way allocation forever. That behavior is acceptable here because the recipient set is fixed and trusted, and avoiding an indefinitely wedged staged balance is the higher priority.

On `post_upgrade`, if a persisted payout plan already exists, the canister also schedules a one-shot forced main tick about 1 second later so an interrupted payout resumes promptly instead of waiting for the normal main interval.

## Rescue and blackhole policy

### Intended controller model

The intended healthy-state controller set is:

- `jupiter-disburser` itself
- the canonical blackhole canister

When rescue is required, the controller set widens to:

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

Do **not** rely on the healthy `self + blackhole` controller posture until at least one successful payout transfer has occurred.

Without a recorded successful transfer, the time-based rescue logic has no proof that value flow ever worked.

## Install-time and upgrade-time configuration

### Init args

- `neuron_id`
  - the NNS neuron controlled by this canister
- `normal_recipient`
  - recipient of the age-neutral base payout
- `age_bonus_recipient_1`
  - recipient of the 80% age-bonus share
- `age_bonus_recipient_2`
  - recipient of the 20% age-bonus share
- `ledger_canister_id` (optional; defaults to ICP Ledger)
- `governance_canister_id` (optional; defaults to NNS Governance)
- `rescue_controller`
- `blackhole_controller` (optional; defaults to canonical blackhole)
- `blackhole_armed` (optional)
- `main_interval_seconds` (optional; defaults to 86400)
- `rescue_interval_seconds` (optional; defaults to 86400)

A copy-pasteable mainnet install args file is committed at [`mainnet-install-args.did`](mainnet-install-args.did).

### Upgrade args

Upgrades currently support:

- `blackhole_controller`
- `blackhole_armed`
- `clear_forced_rescue`

`clear_forced_rescue = true` clears the latched forced-rescue reason but does not rewrite payout history. It also intentionally does **not** force an immediate controller rewrite during `post_upgrade`; after DAO-directed recovery and a successful upgrade, the next rescue evaluation recomputes controller posture from current state and current policy inputs.

### Current production wiring recorded in this repo

The committed mainnet install args currently wire:

- normal recipient: `jupiter-faucet` (`acjuz-liaaa-aaaar-qb4qq-cai`)
- age bonus recipient 1: `jupiter-sns-rewards` (`alk7f-5aaaa-aaaar-qb4ra-cai`)
- age bonus recipient 2: D-QUORUM staking account on NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai` plus the committed subaccount bytes)
- rescue controller: `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)
- blackhole controller: canonical blackhole (`e3mmv-5qaaa-aaaah-aadma-cai`)
- ledger canister: ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- governance canister: NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai`)
- `blackhole_armed = false`
- `main_interval_seconds = 86400`
- `rescue_interval_seconds = 86400`

## Public interface

Production builds expose **no public methods**.

Debug-only methods are gated behind the `debug_api` feature and are intended for local integration and PocketIC tests only. The committed debug Candid file is:

- [`jupiter_disburser_debug.did`](jupiter_disburser_debug.did)

Useful debug helpers include:

- `debug_state`
- `debug_state_size_bytes`
- `debug_main_tick`
- `debug_rescue_tick`
- `debug_build_payout_plan`
- `debug_set_trap_after_successful_transfers` (simulated early-abort fault injection)
- `debug_set_real_trap_after_successful_transfers` (actual post-await trap fault injection)
- debug-only state shims for simulating rescue and payout edge cases

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

### Preferred integration and PocketIC entry points

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
- blackhole / rescue-controller round-trips
- age-bonus routing at multiple neuron ages
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

Upgrades preserve existing state and do not require args unless you are intentionally toggling upgrade-only settings.

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

### Toggle blackhole arming via upgrade args

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

Before handing the canister off to self-management, the DAO-governed deployment flow still needs to configure canister settings such as public log visibility and ensure the canister is a controller of itself.

Example:

```bash
dfx canister update-settings jupiter_disburser --network ic --log-visibility public
dfx canister update-settings jupiter_disburser --network ic --add-controller uccpi-cqaaa-aaaar-qby3q-cai
```

After blackhole mode is armed and at least one successful payout transfer has been recorded, the DAO-governed deployment flow can hand the canister off to the healthy `self + blackhole` controller set:

```bash
dfx canister update-settings jupiter_disburser \
  --network ic \
  --set-controller uccpi-cqaaa-aaaar-qby3q-cai \
  --add-controller e3mmv-5qaaa-aaaah-aadma-cai
```

## Related docs

- suite overview: [`../README.md`](../README.md)
- faucet mechanics: [`../jupiter-faucet/README.md`](../jupiter-faucet/README.md)
- historian read model: [`../jupiter-historian/README.md`](../jupiter-historian/README.md)
- local testing: [`../xtask/README.md`](../xtask/README.md)
