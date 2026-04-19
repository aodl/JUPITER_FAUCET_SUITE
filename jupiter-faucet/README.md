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

The faucet then scans the staking account’s indexed transfer history from the beginning in a streaming, page-by-page pass and evaluates each eligible incoming transfer independently.

For each eligible contribution it computes:

`gross_share = floor(contribution_amount * pot_start / denom_staking_balance)`

If `gross_share` is greater than the current ledger fee, the faucet:

1. sends `gross_share - fee` ICP to the beneficiary’s CMC deposit subaccount
2. calls `notify_top_up`

If not, that contribution is skipped for that payout job and the unallocated value remains available for the end-of-job remainder path.

## Beneficiary attribution rules

A staking-account transaction only contributes to attribution if all of the following are true:

1. it is an incoming `Transfer` **to** the configured `staking_account` (`TransferFrom` records are ignored)
2. the transferred amount is at least `min_tx_e8s`
3. the memo can be decoded as short ASCII principal text for the beneficiary (the supported UX is to enter the target canister ID)

### Memo parsing rules

Memo handling is intentionally simple and code-driven:

- only non-empty `icrc1_memo` bytes are considered
- legacy numeric memos are ignored entirely
- an empty `icrc1_memo` is treated as empty / invalid
- empty memo = invalid
- malformed memo = invalid
- memo that does not decode to principal text within the ICP memo limit = invalid
- the trimmed memo must be ASCII and at most 32 bytes
- whitespace around canister text is tolerated because the parser trims before decoding

Invalid memos are counted as `ignored_bad_memo` in the payout summary and do not block later transfers.

### Minimum tracked contribution

The default minimum tracked contribution is:

- `min_tx_e8s = 100_000_000` (`1 ICP`)

Transfers below that threshold are ignored for attribution and counted as `ignored_under_threshold`. They also do **not** create durable historian tracking for the memo target. The production minimum is intentionally high because historian only keeps a durable beneficiary registry for memo-derived targets that actually qualify; a much lower threshold would make registry spam far cheaper.

Memo encoding uses `icrc1_memo` principal text only. The faucet intentionally ignores the legacy 64-bit numeric memo path so the accepted input is one unambiguous thing: non-empty ASCII bytes that decode to principal text within the ICP memo size limit. We do not hard-code a `-cai` suffix check, because the 32-byte memo limit already excludes ordinary long user principals and we do not want to bake a textual canister-ID convention into canister logic. Users should still enter the intended target canister ID in the memo; that is the supported UX and the wording elsewhere in the suite assumes that path.

The faucet also intentionally does **not** perform an eager canister-existence probe for every eligible memo target. That would add extra network work and cycle cost directly to the value-moving path. The design bias here is to keep the blackholed faucet's hot path as small and deterministic as possible. Principal text in the memo is therefore treated as syntax and policy input only; the canister does not try to prove that every accepted short principal text identifies an installed canister before attempting a top-up. Operationally, that means memo validation is a syntax/policy check rather than an installation proof: if the current CMC path accepts the target principal, the faucet may still attempt the top-up.

This is an explicit economic trade-off, not an oversight. A contributor can still submit syntactically valid memo text that leads to a useless top-up attempt, so the faucet may spend ledger fee / CMC work on a target that never turns into a productive canister top-up. The design accepts that bounded griefing surface because the alternative — probing canister existence on the hot path — would permanently add more complexity, cost, and failure surface to the blackholed value-moving path. The mitigation is the contribution floor itself: repeated attempts remain expensive for the attacker and still send real ICP into the protocol's funding source.

## Important payout semantics

### 1) Every new payout job rescans the full history

The faucet does **not** permanently checkpoint “already attributed” staking transfers across jobs.

Instead, each new payout job rescans the staking account history from the beginning and re-evaluates contributions against the new payout-pot snapshot.

That replay is intentionally streaming and page-bounded rather than history-buffering. The design prefers constant resident attribution state in the blackholed canister over a permanently growing durable attribution set, so the accepted growth vector is replay work and cycles consumption over time rather than unbounded attribution memory.

To cap repeated replay cost on obviously barren history, the faucet also persists large tx-id skip ranges for spans that were previously found to contain no transactions worth revisiting under the current attribution rules. This is a replay-work cache, not a new source of truth. For safety and simplicity, every upgrade clears the persisted skip-range cache before the faucet resumes. That behavior is unconditional by design: skip ranges are only valid under the current contribution-classification rules, so retaining them across a future code/config change risks trusting stale replay hints. In practice upgrades are expected to be exceptional DAO-directed recovery events after blackhole activation, so conservative re-evaluation of historical staking activity is preferable to preserving cache warmth.

The `10_000`-transaction persistence threshold is also intentional. The goal is to avoid repeated replay work for clearly barren history without turning skip-range storage into its own durable indexing system. Below-threshold barren spans can therefore be shaped and replayed, but the chosen threshold was set conservatively below the estimated economic break-even point where repeated replay would become more expensive for the faucet than periodically inserting fresh qualifying stake to prevent larger cached spans from forming. That keeps the durable cache small, keeps the implementation simple, and still makes large barren spans worth caching.

Only the **currently active** job persists the scan cursor, partial skip-span state, and aggregate counters. 
### 2) Contributions are not aggregated

Each eligible contribution is processed independently, even when multiple contributions map to the same beneficiary.

So if the same beneficiary appears twice in staking-account history, the faucet treats those as two distinct contribution records for payout purposes. That is an intentional trade-off of the single-pass streaming model, and it means repeated qualifying contributions for the same beneficiary may incur repeated outbound ledger fees.

### 3) The denominator is a round-effective staking snapshot

A payout job still snapshots the payout pot exactly once at job start, but it no longer uses the raw live staking balance as the beneficiary denominator for the completed reward round.

Instead, the faucet now carries forward a **round-start staking snapshot** and builds a **round-effective denominator** for the round that just finished:

- stake already present at the start of the round counts at full weight
- valid in-round contributions are added with a conservative time weight
- contributions whose tx id is beyond the round-end snapshot are excluded from the current round entirely

The time weight is intentionally conservative. The faucet uses the contribution timestamp plus a configured stake-recognition delay (default `86400` seconds) before treating that contribution as effective for the current round. This approximates the fact that the staking neuron only begins earning the larger maturity stream after a later `ClaimOrRefresh`, and it biases slightly against over-crediting very recent stake.

The tx-id boundaries are more authoritative than timestamps for inclusion. The faucet captures the latest staking-account tx id at the end of each completed round and uses that as the inclusive upper bound for the next payout job, so equal timestamps do not create ambiguity.

### 4) The payout pot is snapshotted once per job

A job uses the payout-account balance captured at the beginning of the job. It does not dynamically rescale shares mid-run based on whatever later transfers may have arrived. The same job-start moment also becomes the stored start boundary for the following reward round.

### 4a) Timing-aware payout fairness

The faucet now explicitly addresses the case where the same additional stake amount arrives at different times within the reward accumulation window. The intended property is:

- if extra stake is present for the full window, pot growth and denominator growth should track closely, so beneficiary payout should stay roughly unchanged
- if the same stake arrives late in the window, it should receive only the weight justified by the time it could plausibly have been earning, rather than pinching earlier contributors
- once a later round begins cleanly, any remaining payout differences are expected to reflect real factors such as age-bonus differences rather than unfair denominator timing

Operationally, the mitigation strategy is therefore:

1. persist the round-start staking balance, latest tx id, and timestamp at the end of each completed payout round
2. snapshot the next round's payout pot and latest tx id exactly once at job start
3. build the current round's effective denominator as `round_start_balance + weighted valid in-round contributions`
4. use the same weighted amount for each in-round contribution's numerator and for the round-effective denominator
5. ignore invalid memo contributions in the weighting adjustment path so adversaries cannot force large numbers of pointless weighting calculations with malformed deposits

The repo now covers this in three layers:

- `src/logic.rs` unit tests verify the weighting, boundary, and payout arithmetic used by the faucet
- `src/scheduler.rs` tests verify that the faucet clamps a round by tx id, computes the round-effective denominator before payout scanning, and falls back safely for exactly one transition payout if no prior round snapshot exists yet
- the disburser/faucet PocketIC suite keeps canonical end-to-end economics tests that prove very late valid and very late invalid top-ups do not reduce the existing beneficiary's affected-round payout under the weighted-round mitigation

The detailed reward-environment caveats and the rationale for the PocketIC whale background live in `xtask/README.md` and in the comments around the PocketIC reward helpers.

### 5) Any unallocated remainder stays useful

At the end of a completed payout job, any remainder that was not successfully allocated to beneficiaries can be sent as a **remainder-to-self** CMC top-up, as long as the remaining gross amount is greater than the ledger fee. Internally the faucet tracks `gross_outflow_e8s` as **ledger-accepted outflow**, not as a promise that every corresponding top-up ultimately produced useful beneficiary cycles. That distinction matters at failure boundaries: summary accounting treats value as committed once the ledger has accepted the transfer identity, while beneficiary success / failure / ambiguity is tracked separately.

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
For ICRC-1 transfers, the faucet encodes the CMC top-up memo as an 8-byte **little-endian** blob so it matches how the CMC interprets ICRC memo bytes.

### Outgoing ledger transfer memo

The faucet uses a fixed ledger memo equal to `TopUpCanister` (`1_347_768_404` as a `u64`) for its ICP transfers into the CMC deposit accounts.

## Runtime model

### Timer cadence

Default timers are:

- `main_interval_seconds = 7 days`
- `rescue_interval_seconds = 1 day`
- index page size = `500`

Each interval timer is clamped to at least 60 seconds by the runtime code. On `post_upgrade`, if an `ActivePayoutJob` already exists, the faucet also schedules a one-shot forced main tick about 1 second later so an interrupted payout resumes promptly instead of waiting for the regular cadence. There is no separate deferred retry queue or backoff worker: retries are attempted inline during the same payout pass. If an ordinary transient failure ends a payout tick early, the active job remains persisted. It can then resume either on the next scheduled main tick or, if the daily rescue tick fires first, as the final action of that rescue tick via a forced main resume. The historical replay itself is chunked by index pages and by async transfer/notify boundaries, so no payout job relies on one monolithic message execution.
The PocketIC integration suite includes an end-to-end upgrade test that interrupts a live payout after the ledger transfer lands but before the faucet persists acceptance/notify progress, then upgrades and verifies duplicate-proof recovery to a single final notification.

### Main tick sequence

On each successful main tick, the canister:

1. acquires a 15-minute main-tick lease
2. enforces a minimum gap between automatic runs
3. if no active payout job exists, snapshots:
   - current ledger fee
   - payout-account balance
   - staking-account balance
4. if the payout pot is too small or the denominator is zero, it performs only index-health probing and bootstrap-rescue checks
5. otherwise, creates an `ActivePayoutJob`
6. scans the staking account through the ICP index canister, page by page
7. evaluates each eligible incoming transfer independently
8. for each eligible beneficiary contribution, performs ledger transfer then `notify_top_up`
9. if a transfer fails before acceptance or a post-acceptance notify fails, retries that step immediately once in-line
10. when scanning is complete, optionally sends the remainder-to-self top-up
11. finalizes the job into a persisted summary and applies health observations

## Retry and failure behavior

The faucet performs top-ups on a **best-effort** basis. A payout job attempts to convert the current payout pot into beneficiary top-ups, but it does not guarantee that every individually eligible contribution will be topped up during that run.

### Persisted job state

An active payout job persists:

- the pot and denominator snapshots
- the scan cursor (`next_start`)
- aggregate counters used for the eventual summary
- CMC attempt / success counters used by rescue heuristics
- the currently in-flight top-up phase (`AwaitingTransfer` vs `TransferAccepted`) together with the original `created_at_time`, so an upgrade can resume safely at the ledger or notify boundary

The runtime still does **not** buffer an unbounded deferred retry queue; it only persists the single in-flight transfer/notify phase for the active job. That keeps state bounded, and the faucet's recovery model remains cadence-based after an ordinary failed tick: interrupted work is preserved rather than discarded, then retried on the next available scheduler opportunity. In practice that means `post_upgrade` triggers an immediate forced resume, and otherwise the unfinished job is resumed either by the next main tick or by the daily rescue tick's final forced main resume if that arrives first.

### What is retried

The faucet retries at most once, immediately and inline, at these two ambiguous boundaries:

- ledger transfer failed before a block index was obtained
- CMC `notify_top_up` failed after a ledger transfer had already been accepted (typed terminal replies are still retried once safely, then classified separately if they remain terminal)

Typed terminal `notify_top_up` rejections such as `Refunded`, `TransactionTooOld`, and `InvalidTransaction` are still retried once safely after an accepted ledger transfer; if both notify attempts remain terminal, the beneficiary is counted as a deterministic failure rather than an ambiguity.

### Duplicate-proof behavior

If the ledger replies with `Duplicate`, the faucet reuses the returned block index and continues with `notify_top_up` instead of sending a second transfer. Immediate transfer retries reuse the same transfer identity (`memo` + `created_at_time`) so duplicate detection remains safe.

### Bounded retry policy

The faucet does **not** retry forever and does **not** buffer a retry queue in memory. Behavior is:

- first accepted-ledger notify failure → retry that notify once immediately, inline
- if both notify replies are typed terminal rejections → count that contribution as **failed** and continue
- otherwise, if the retry still leaves transport / retryable uncertainty → count that contribution as **ambiguous** and continue
- if the wider payout tick later aborts for some unrelated transient reason, the unfinished active job is preserved and retried on the next scheduler opportunity (weekly main tick by default, or sooner via the daily rescue tick's forced main resume)

This keeps memory bounded and avoids long-lived paused payout jobs. It also means top-ups are strictly **best effort**: some eligible contributions may fail deterministically, while others may end in an ambiguous transfer/notify boundary and be reflected separately in the summary counters. The faucet also proactively rejects obviously invalid memo targets such as the anonymous principal and the management canister principal.


### Logging policy

To avoid filling the canister log buffer with repetitive transfer-level noise, the faucet prefers aggregate accounting over per-record error logs. Operators should expect compact run summaries and counters such as `failed_topups` and `ambiguous_topups`, not one log line per beneficiary top-up attempt. `failed_topups` is used for deterministic beneficiary failures; `ambiguous_topups` is used when the faucet exhausts its one inline retry at a boundary where a prior ledger / CMC action may already have taken effect. These counters are beneficiary-only; a failed remainder-to-self cleanup transfer does not increment either one.

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
  - if the staking-account balance changes and the latest staking-account tx ID is successfully read but does not change, twice in a row
- `IndexLatestUnreadable`
  - if the staking-account balance changes and the canister cannot confirm the latest staking-account tx ID, twice in a row
- `CmcZeroSuccessRuns`
  - if two completed payout jobs in a row make beneficiary CMC notify attempts but record zero successful beneficiary top-up notifications

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
- `blackhole_controller` (optional; defaults to canonical blackhole; when present it must not equal the faucet canister principal or `rescue_controller`)
- `blackhole_armed` (optional)
- `expected_first_staking_tx_id` (optional)
  - a safety anchor for the oldest expected staking-account transaction **ID** visible through the index canister
  - type: `opt nat64`
- `main_interval_seconds` (optional; defaults to 7 days)
- `rescue_interval_seconds` (optional; defaults to 1 day)
- `min_tx_e8s` (optional; defaults to `1 ICP`; must be at least `0.1 ICP`)
  - upgrades already clear persisted skip ranges before resuming, so any rescue-time threshold change will cause historical staking activity to be re-scanned from first principles

A copy-pasteable mainnet install args file is committed at [`mainnet-install-args.did`](mainnet-install-args.did).

### Upgrade args

Upgrade args currently support:

- `blackhole_controller`
- `blackhole_armed`
- `clear_forced_rescue`

Every upgrade also clears the persisted skip-range cache before the faucet resumes. That behavior is unconditional and intentionally conservative: skip ranges are treated as disposable replay hints rather than durable truth, and upgrades are expected to be exceptional enough that paying the re-scan cost is preferable to risking stale cache semantics.

`clear_forced_rescue = true` clears:

- the latched forced-rescue reason
- the related consecutive-failure counters

When `clear_forced_rescue = true` is used while blackhole mode remains armed, the faucet now also schedules an immediate one-shot rescue/controller reconciliation after `post_upgrade` so a stale widened controller set does not linger until the next periodic rescue timer. If blackhole mode is not armed, no automatic controller target is imposed; the armed-mode controller policy is the only one the canister reconciles itself toward.

### Current production wiring recorded in this repo

The committed mainnet install args wire the current production constants used by the suite:

- staking account owner: NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai`)
- staking account subaccount bytes:
  `ff0c0b36afefffd0c7a4d85c0bcea366acd6d74f45f7703d0783cc6448899c68`
- expected first staking tx ID:
  intended to be an oldest-transaction **ID** anchor (`opt nat64`), not a hash / string; verify the committed `mainnet-install-args.did` value before using it
- payout account: the faucet canister default account (`acjuz-liaaa-aaaar-qb4qq-cai`, with `payout_subaccount = null`)
- rescue controller: `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)
- blackhole controller: canonical blackhole (`e3mmv-5qaaa-aaaah-aadma-cai`)
- ledger canister: ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- index canister: ICP Index (`qhbym-qaaaa-aaaaa-aaafq-cai`)
- CMC canister: Cycles Minting Canister (`rkp4c-7iaaa-aaaaa-aaaca-cai`)
- `main_interval_seconds = 604800`
- `rescue_interval_seconds = 86400`
- `min_tx_e8s = 100000000` (`1 ICP`, with an enforced hard floor of `10000000` / `0.1 ICP`)

## Public interface

Production builds expose **no public methods**.

Debug builds expose helper surfaces behind `debug_api`, including:

- `debug_state`
- `debug_last_summary`
- `debug_accounts`
- `debug_footprint`
- debug methods for forcing timer runs and manipulating rescue state during tests

These are for local integration and PocketIC tests only. The committed debug Candid file is:

- [`jupiter_faucet_debug.did`](jupiter_faucet_debug.did)

Notable fault-injection helpers include both `debug_set_trap_after_successful_transfers` (simulated early abort) and `debug_set_real_trap_after_successful_transfers` (actual post-await trap) for upgrade-boundary PocketIC tests.

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

- immediate duplicate-safe retry for ambiguous transfer and notify failures
- full-history replay on each new job
- page-boundary scanning across large histories
- same-beneficiary contributions staying separate
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

- beneficiary is identified by short ASCII principal text in `icrc1_memo` (the supported usage is to put the target canister ID there)
- malformed or missing memos are ignored
- ownership of the beneficiary canister is not checked by the faucet

## Related docs

- suite overview: [`../README.md`](../README.md)
- disburser mechanics: [`../jupiter-disburser/README.md`](../jupiter-disburser/README.md)
- historian read model: [`../jupiter-historian/README.md`](../jupiter-historian/README.md)
- local testing: [`../xtask/README.md`](../xtask/README.md)
