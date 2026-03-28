# Jupiter Faucet

`jupiter-faucet` is the cycles top-up canister in the Jupiter Faucet Suite.

It receives the **base ICP flow** from `jupiter-disburser`, attributes staking-account deposits to beneficiaries using transaction memos, and performs proportional CMC top-ups for those beneficiaries.

This is the canister that turns “someone deposited ICP into the reward neuron’s staking path” into “a beneficiary canister received a cycles top-up.”

See the suite overview in [`../README.md`](../README.md).

## Role in the suite

`jupiter-faucet` owns five things:

1. identifying the **staking account** whose incoming transfers define contribution history
2. scanning that account through the ICP index canister
3. interpreting eligible transfer memos as beneficiary canister principals
4. converting a payout pot of ICP into proportional per-contribution top-ups
5. managing its own blackhole / recovery policy once armed

It does **not** control the NNS neuron itself. `jupiter-disburser` is responsible for producing the ICP that the faucet spends.

## External dependencies

By default the canister talks to:

- ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- ICP Index (`qhbym-qaaaa-aaaaa-aaafq-cai`)
- Cycles Minting Canister / CMC (`rkp4c-7iaaa-aaaaa-aaaca-cai`)
- canonical blackhole (`e3mmv-5qaaa-aaaah-aadma-cai`) when blackhole mode is configured / armed

## High-level payout model

Each payout job works from two snapshots taken at the beginning of the job:

- `pot_start_e8s` = current ICP balance of the faucet payout account
- `denom_staking_balance_e8s` = current ICP balance of the configured staking account

The faucet then scans the staking account’s indexed transfer history from the beginning and evaluates each eligible incoming transfer independently.

For each eligible contribution it computes:

`gross_share = floor(contribution_amount * pot_start / denom_staking_balance)`

If `gross_share` is greater than the current ledger fee, the faucet:

1. sends `gross_share - fee` ICP to the beneficiary’s CMC deposit subaccount
2. calls `notify_top_up`

If not, that contribution is skipped for that payout job and the unallocated value remains available for the end-of-job remainder path.

## Beneficiary attribution rules

A staking-account transaction only contributes to attribution if all of the following are true:

1. it is an incoming transfer **to** the configured `staking_account`
2. the transferred amount is at least `min_tx_e8s`
3. the memo can be decoded as principal text for a beneficiary canister

### Memo parsing rules

Memo handling is intentionally simple and code-driven:

- first preference: `icrc1_memo`
- fallback: legacy numeric memo bytes if `icrc1_memo` is absent and the numeric memo is non-zero
- empty memo = invalid
- malformed memo = invalid
- memo that is not valid principal text = invalid
- whitespace around principal text is tolerated because the parser trims before decoding

Invalid memos are counted as `ignored_bad_memo` in the payout summary and do not block later transfers.

### Minimum tracked contribution

The default minimum tracked contribution is:

- `min_tx_e8s = 10_000_000` (`0.1 ICP`)

Transfers below that threshold are ignored for attribution and counted as `ignored_under_threshold`.

## Important payout semantics

### 1) Every new payout job rescans the full history

The faucet does **not** permanently checkpoint “already attributed” staking transfers across jobs.

Instead, each new payout job rescans the staking account history from the beginning and re-evaluates contributions against the new payout-pot snapshot.

Only the **currently active** job persists scan cursor and retry state.

### 2) Contributions are not aggregated

Each eligible contribution is processed independently, even when multiple contributions map to the same beneficiary.

So if the same beneficiary appears twice in staking-account history, the faucet treats those as two distinct contribution records for payout purposes.

### 3) The denominator is the staking-account balance snapshot

The proportional split is based on the staking account’s balance at job start, not on a sum the faucet reconstructs from history in memory.

That is why the code also contains index-health invariants around the observed oldest / latest transaction IDs.

### 4) The payout pot is snapshotted once per job

A job uses the payout-account balance captured at the beginning of the job. It does not dynamically rescale shares mid-run based on whatever later transfers may have arrived.

### 5) Any unallocated remainder stays useful

At the end of a completed payout job, any remainder that was not successfully allocated to beneficiaries can be sent as a **remainder-to-self** CMC top-up, as long as the remaining gross amount is greater than the ledger fee.

### 6) A computed share at or below the fee is not a failure

When `gross_share <= fee`, the contribution is classified as `NoTransfer`.

That means:

- it is not counted as `ignored_under_threshold`
- it is not counted as `ignored_bad_memo`
- it is not counted as a failed top-up
- its economic value remains available to the remainder path

## Accounts and transfer details

### Staking account

The staking account is the input side of the faucet. Incoming transfers into this account define contribution history.

### Payout account

Outgoing top-ups are sent from:

- the canister default account if `payout_subaccount` is omitted
- the configured subaccount if `payout_subaccount` is provided

The current suite wiring from `jupiter-disburser` targets the faucet’s default account.

### Deposit account used for top-ups

For a beneficiary canister principal `P`, the faucet transfers ICP to the CMC deposit account:

- owner = CMC canister principal
- subaccount = 32-byte encoding derived from `P`

The subaccount layout is:

- byte `0` = principal byte length
- bytes `1..` = principal bytes
- remaining bytes = zero padding

That matches the standard top-up pattern used before calling `notify_top_up`.

### Outgoing ledger transfer memo

The faucet uses a fixed ledger memo equal to `TopUpCanister` (`1_347_768_404` as a `u64`) for its ICP transfers into the CMC deposit accounts.

## Runtime model

### Timer cadence

Default timers are:

- `main_interval_seconds = 7 days`
- `rescue_interval_seconds = 1 day`
- retry backoff = about `60 seconds`
- index page size = `500`

Each timer interval is clamped to at least 60 seconds by the runtime code.

### Main tick sequence

On each successful main tick, the canister:

1. acquires a short-lived main-tick lease
2. enforces a minimum gap between automatic runs
3. if no active payout job exists, snapshots:
   - current ledger fee
   - payout-account balance
   - staking-account balance
4. if the payout pot is too small or the denominator is zero, it performs only index-health probing and bootstrap-rescue checks
5. otherwise, creates an `ActivePayoutJob`
6. processes any due retry first
7. scans the staking account through the ICP index canister, page by page
8. evaluates each eligible incoming transfer independently
9. for each eligible beneficiary contribution, performs ledger transfer then `notify_top_up`
10. when scanning is complete, optionally sends the remainder-to-self top-up
11. finalizes the job into a persisted summary and applies health observations

## Retry and failure behavior

The faucet is deliberately strict about bounded retries.

### Persisted job state

An active payout job persists:

- the pot and denominator snapshots
- the scan cursor (`next_start`)
- aggregate counters used for the eventual summary
- CMC attempt / success counters used by rescue heuristics
- the currently active retry item, if any
- an optional queued list of later retry items discovered during the same job

### What is retried

The faucet has a deferred retry path for exactly these two ambiguous boundaries:

- ledger transfer failed before a block index was obtained
- CMC `notify_top_up` failed after a ledger transfer had already been accepted

### Duplicate-proof behavior

If the ledger replies with `Duplicate`, the faucet reuses the returned block index and continues with `notify_top_up` instead of sending a second transfer.

That keeps the ledger / CMC boundary safe across retries and upgrades.

### Bounded retry policy

The faucet does **not** retry forever.

Behavior is:

- first ambiguous failure → persist retry state and retry once later
- retry still fails → count that contribution as failed and continue the job

Retries are scheduled for roughly `60 seconds` later. A single bad contribution therefore cannot wedge the whole payout job.

### Upgrade safety

Active payout jobs, retry state, summaries, and rescue-relevant observations are persisted across upgrades.

## Rescue and blackhole policy

### Intended controller model

The intended healthy-state controller set is:

- `jupiter-faucet` itself
- the canonical blackhole canister

When rescue is required, the controller set becomes:

- `jupiter-lifeline`
- the canonical blackhole canister
- `jupiter-faucet`

### Time-based windows

When blackhole mode is armed, time-based controller reconciliation follows the same windows as the disburser:

- healthy: `<= 7 days` since last successful top-up notification → blackhole + self
- middle window: `> 7 days` and `<= 14 days` → no controller change
- broken: `> 14 days` → blackhole + rescue controller + self

There is also a bootstrap rescue condition:

- more than `14 days` since blackhole arming with **no** successful top-up ever recorded → forced rescue reason `BootstrapNoSuccess`

### Faucet-specific forced rescue reasons

Unlike the disburser, the faucet also has code-backed forced rescue latches tied to health invariants:

- `IndexAnchorMissing`
  - if `expected_first_staking_tx_id` is configured and the observed oldest staking-account tx ID does not match it twice in a row
- `IndexLatestInvariantBroken`
  - if the staking-account balance changes but the observed latest tx ID does not change, twice in a row
- `CmcZeroSuccessRuns`
  - if two payout jobs in a row make CMC attempts but record zero successful top-up notifications

These latches are persisted and can be cleared via upgrade args when appropriate.

## Install-time and upgrade-time configuration

### Init args

- `staking_account`
  - the account whose incoming transfers define contribution history
- `payout_subaccount` (optional)
  - the faucet account subaccount to spend from; if omitted the canister default account is used
- `ledger_canister_id` (optional; defaults to ICP Ledger)
- `index_canister_id` (optional; defaults to ICP Index)
- `cmc_canister_id` (optional; defaults to the Cycles Minting Canister)
- `rescue_controller`
- `blackhole_controller` (optional; defaults to canonical blackhole)
- `blackhole_armed` (optional)
- `expected_first_staking_tx_id` (optional)
  - a safety anchor for the oldest expected staking-account tx visible through the index canister
- `main_interval_seconds` (optional; defaults to 7 days)
- `rescue_interval_seconds` (optional; defaults to 1 day)
- `min_tx_e8s` (optional; defaults to `0.1 ICP`)

A copy-pasteable mainnet install args file is committed at [`mainnet-install-args.did`](mainnet-install-args.did).

### Upgrade args

Upgrade args currently support:

- `blackhole_controller`
- `blackhole_armed`
- `clear_forced_rescue`

`clear_forced_rescue = true` clears:

- the latched forced-rescue reason
- the related consecutive-failure counters

### Current production wiring recorded in this repo

The committed mainnet install args wire the current production constants used by the suite:

- staking account owner: NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai`)
- staking account subaccount bytes:
  `ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68`
- expected first staking tx ID:
  `390be24d51d6b006afcb9774585d6eb353e7cdbb72bc2b96f0978a5a1aab7ae5`
- payout account: the faucet canister default account (`acjuz-liaaa-aaaar-qb4qq-cai`, with `payout_subaccount = null`)
- rescue controller: `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)
- blackhole controller: canonical blackhole (`e3mmv-5qaaa-aaaah-aadma-cai`)
- ledger canister: ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- index canister: ICP Index (`qhbym-qaaaa-aaaaa-aaafq-cai`)
- CMC canister: Cycles Minting Canister (`rkp4c-7iaaa-aaaaa-aaaca-cai`)
- `main_interval_seconds = 604800`
- `rescue_interval_seconds = 86400`
- `min_tx_e8s = 10000000`

## Public interface

Production builds expose **no public methods**.

Debug builds expose helper surfaces behind `debug_api`, including:

- `debug_state`
- `debug_last_summary`
- `debug_accounts`
- `debug_footprint`
- debug methods for forcing timer runs and manipulating retry / rescue state during tests

These are for local integration and PocketIC tests only. The committed debug Candid file is:

- [`jupiter_faucet_debug.did`](jupiter_faucet_debug.did)

## Build and test

### Production build

```bash
cargo build -p jupiter-faucet --target wasm32-unknown-unknown --release --locked
```

### Debug build

```bash
cargo build -p jupiter-faucet --target wasm32-unknown-unknown --release --features debug_api --locked
```

### Unit tests

```bash
cargo test -p jupiter-faucet --lib
```

### Preferred integration and PocketIC entry points

```bash
cargo run -p xtask -- faucet_dfx_integration
cargo run -p xtask -- faucet_pocketic_integration
cargo run -p xtask -- faucet_all
```

Those cover, among other things:

- retry persistence for both transfer and notify steps
- duplicate-safe notify retry without duplicate ledger transfer
- full-history replay on each new job
- page-boundary scanning across large histories
- same-beneficiary contributions staying separate
- upgrade mid-job / mid-retry behavior
- bounded state footprint across repeated runs
- rescue-controller round-trips and forced-rescue latching

For the suite-wide matrix, see [`../xtask/README.md`](../xtask/README.md).

## Operational guidance

### Before blackholing

Do not rely on the healthy `self + blackhole` controller set until the canister has recorded at least one successful top-up notification.

### When reading payout behavior

The most important thing to remember is that the faucet is **job-snapshot based**, not streaming-accounting based. New jobs recompute from the full history and a fresh pot snapshot.

### When documenting or operating beneficiaries

Treat the memo requirement as canonical:

- beneficiary is identified by principal text in the memo
- malformed or missing memos are ignored
- ownership of the beneficiary canister is not checked by the faucet

## Related docs

- suite overview: [`../README.md`](../README.md)
- disburser mechanics: [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md)
- historian read model: [`../jupiter-historian/README.md`](../jupiter-historian/README.md)
- local testing: [`../xtask/README.md`](../xtask/README.md)
