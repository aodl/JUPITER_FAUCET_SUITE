# Jupiter Relay

`jupiter-relay` is an ICP-funded cycles allocator and optional surplus router for the Jupiter Faucet Suite. [Jupiter Faucet](../faucet) uses the Jupiter Faucet Relay canister as a singular target for perpetual suite top-ups in raw ICP form, using the `.` memo syntax. The relay periodically samples the cycles balance of all Jupiter Faucet Suite canisters and allocates ICP based on recent burn plus any unrecovered per-canister cycle deficit. When raw ICP surplus recipients are configured, fresh burn receives 1% headroom and remaining production surplus ICP is split equally across those recipients only after every recovery deficit is cleared. When no raw ICP surplus recipients are configured, Relay routes ICP as cycles through CMC using burn-plus-deficit-weighted allocations once the all-cycles batch is fee-efficient for every positive-need managed canister. Relay also checks its own subaccount 1 on each main tick and forwards qualifying Jupiter Faucet commitments from that subaccount independently of the default-account allocation job.

It spends ICP from the relay canister default ICP ledger account:

```text
Account { owner = <relay_canister_id>, subaccount = null }
```

This default account remains the only source for managed-canister CMC top-ups and configured surplus transfers.

Fund it through the existing faucet raw-ICP memo route with:

```text
<relay_canister_id>.
```

The trailing dot is required. In [`jupiter-memo-policy`](../../crates/memo-policy), `canister_id.memo` means raw ICP to the canister default account with the right-hand segment used as the outgoing memo; an empty right-hand segment sends raw ICP with an empty memo.

## Role in the Suite

The production relay is intended to top up all Jupiter-operated solution canisters plus the managed blackhole canisters. The relay itself is automatically included at runtime; operators should not include the relay principal in `managed_canisters`.

System and dependency canisters are intentionally excluded from the managed set. The relay should not manage the ICP ledger, CMC, NNS governance, ICP index, SNS-WASM, SNS roots, XRC, or other external dependencies.

## Production Managed Set

```text
jupiter_disburser          uccpi-cqaaa-aaaar-qby3q-cai
jupiter_lifeline           afisn-gqaaa-aaaar-qb4qa-cai
jupiter_faucet             acjuz-liaaa-aaaar-qb4qq-cai
jupiter_sns_rewards        alk7f-5aaaa-aaaar-qb4ra-cai
jupiter_faucet_frontend    jufzc-caaaa-aaaar-qb5da-cai
jupiter_historian          j5gs6-uiaaa-aaaar-qb5cq-cai
blackhole_fiduciary_subnet 77deu-baaaa-aaaar-qb6za-cai
blackhole_13_node_subnet   e3mmv-5qaaa-aaaah-aadma-cai
relay, auto-included       u2qkp-aqaaa-aaaar-qb7ea-cai
```

## Funding

The production relay default account can be funded through the faucet raw ICP route:

```text
u2qkp-aqaaa-aaaar-qb7ea-cai.
```

Start with a small funding amount, observe one baseline tick and one allocation tick, and only then increase funding.

Relay subaccount 1 is reserved for direct Jupiter Faucet commitment forwarding. The subaccount is exactly 32 bytes, with 31 zero bytes followed by `0x01`. On each main tick, Relay checks:

```text
Account { owner = <relay_canister_id>, subaccount = opt blob "\00...\01" }
```

Relay subaccount 1 supports memo-free perpetual funding of the Relay canister, and therefore all Relay-managed canisters. It is useful when the funding source cannot, or should not, attach an ICP ledger memo. A concrete example is minting maturity directly into a Jupiter Faucet funding flow.

The production ICRC textual account is:

```text
u2qkp-aqaaa-aaaar-qb7ea-cai-66ym2xq.1
```

Its equivalent explicit ICRC account fields are:

```text
owner = u2qkp-aqaaa-aaaar-qb7ea-cai
subaccount = 0000000000000000000000000000000000000000000000000000000000000001
```

The Relay default account remains for normal managed-canister CMC top-ups and configured surplus routing. Relay subaccount 1 is only for memo-free Jupiter Faucet commitment forwarding. Funds sent to subaccount 1 accumulate until the account can make a qualifying commitment.

Once the account holds more than the current ledger fee and the net transferable amount is at least 1 ICP, Relay transfers `balance - fee` to the Jupiter Faucet neuron staking account under NNS Governance. The destination neuron is `11614578985374291210`, resolved through NNS Governance `list_neurons`. The production transfer memo is derived from the Relay principal as compact text plus `.Relay`; for `u2qkp-aqaaa-aaaar-qb7ea-cai`, it is `u2qkpaqaaaaaaarqb7eacai.Relay`. Balances below `1 ICP + fee` remain in subaccount 1 for a future tick.

## Managed Canisters

Install args include `managed_canisters : vec principal`. The runtime set is the sorted unique union of that list and the relay canister itself. The relay is always included even when omitted from config.

Anonymous and management canister principals are rejected. Duplicate configured managed canisters are rejected. The relay probes its own cycles directly with `canister_cycle_balance`; ordinary non-self managed canisters must be readable through the configured blackhole canister. Known managed blackhole canisters are probed through themselves by calling their own `canister_status` endpoint with their own principal as the target.

If any required probe fails, the relay fails closed: it records a degraded summary and spends no ICP.

## Runtime Config Verification

The production relay exposes no public application query or update endpoints. Debug endpoints are only available in non-production debug builds, and the debug API guard traps if a debug build is ever installed at the production relay principal. The operational model treats the production-principal guard as sufficient: debug builds must not be installed on production canister IDs, production canister IDs reject debug API use, and a newly deployed relay with debug APIs is a separate non-production/debug deployment. No controller-only production application endpoint is used for recovery.

The relay logs public runtime verification lines on every main tick that actually runs:

```text
Cycles: <relay_self_cycles_balance>
CONFIG relay_canister_id=...
```

The `CONFIG` line includes the configured managed canisters, effective managed canisters including relay self, ledger, CMC, NNS Governance, blackhole, interval, transfer limit, surplus recipients, surplus memo lengths, and whether the configured production managed set matches the known Jupiter suite set.

After deployment, anyone can verify the installed source/config by building the canister from the reviewed source, checking the production canister ID mapping, comparing public logs with [`mainnet-install-args.did`](mainnet-install-args.did), and using the [frontend source pane](../frontend). Public verification happens through logs, reproducible build/source metadata, the production canister ID mapping, and the frontend source pane.

```bash
icp canister logs u2qkp-aqaaa-aaaar-qb7ea-cai -n ic
```

Canister logs have finite retention. Operators should archive logs externally if long-term history is required. Logs are intentionally low-noise: timer callbacks that are suppressed by the guard, startup liveness checks that are suppressed by the recent-run guard, empty scans, and below-threshold subaccount-1 scans do not produce extra public log lines. Main ticks that actually proceed still emit the documented runtime and financial logs.

## Public Log Records

The relay emits consistent, single-line, grep-friendly public records:

```text
Cycles: <relay_self_cycles_balance>
CONFIG relay_canister_id=...
RELAY_SUMMARY mode=<BaselineOnly|TopUpThenSurplus|Degraded|NoFunds> started_at_ts_nanos=<nat64> completed_at_ts_nanos=<nat64-or-null> min_cycles_balance=<nat-or-null> total_burn_cycles=<nat> total_target_topup_cycles=<nat> total_actual_minted_cycles=<nat> total_carried_deficit_cycles=<nat> total_remaining_deficit_cycles=<nat> deficit_canister_count=<nat32> balance_start_e8s=<nat64> fee_e8s=<nat64> transfer_count=<nat32> ledger_transfer_count=<nat32> ledger_sent_e8s=<nat64> ledger_fees_e8s=<nat64> cmc_notify_success_count=<nat32> cmc_notify_failed_count=<nat32> cmc_notify_ambiguous_count=<nat32> planned_retained_e8s=<nat64> known_unspent_e8s=<nat64> ambiguous_e8s=<nat64> failed_transfers=<nat32> ambiguous_transfers=<nat32> partial_tick_count=<nat32> conversion_cycles_per_e8=<nat-or-null> surplus_e8s_before_fees=<nat64> skipped_surplus_reason=<escaped-text-or-null>
RELAY_CANISTER canister_id=<principal> previous_cycles=<nat-or-null> current_cycles=<nat> relay_minted_cycles=<nat> burn_cycles=<nat> carried_deficit_cycles=<nat> target_topup_cycles=<nat> planned_topup_e8s=<nat64> sent_topup_e8s=<nat64> actual_minted_cycles=<nat> remaining_deficit_cycles=<nat> skipped_reason=<escaped-text-or-null>
RELAY_SURPLUS_TRANSFER target=<canister:principal|neuron:nat64> owner=<principal> subaccount=<hex-or-null> gross_share_e8s=<nat64> amount_e8s=<nat64> skipped_reason=<escaped-text-or-null> memo_len=<nat32-or-null>
RELAY_FAUCET_COMMITMENT source_owner=<principal> source_subaccount=<hex> destination_owner=<principal> destination_subaccount=<hex-or-null> balance_start_e8s=<nat64> amount_e8s=<nat64> fee_e8s=<nat64> memo_len=<nat32> skipped_reason=<escaped-text-or-null>
RELAY_PROBE_FAILURE canister_id=<principal> error=<escaped-text>
relay LIFECYCLE event=<init_complete|post_upgrade_complete> timers_installed=true main_interval_seconds=<nat64>
relay ERR message=<escaped-text>
```

`RELAY_SUMMARY` aggregates the per-canister recovery view: `total_target_topup_cycles` is the tick's total CMC target, `total_actual_minted_cycles` is cycles returned by successful CMC notify calls, `total_carried_deficit_cycles` is pre-existing recovery debt, `total_remaining_deficit_cycles` is unrecovered debt after the tick, and `deficit_canister_count` is the number of canisters still blocking surplus. `RELAY_CANISTER` logs show current cycles, previous cycles, relay-minted cycles since the previous sample, estimated fresh burn, carried recovery deficit, the mode-specific top-up target, planned top-up e8s, sent top-up e8s, actual minted cycles, remaining recovery deficit, and skipped reason if any. `planned_topup_e8s` is the intended net CMC top-up amount. `sent_topup_e8s` is the accepted net amount actually sent to CMC, and is zero if the transfer was not accepted. `RELAY_SURPLUS_TRANSFER` logs show surplus recipients, amount, and memo length without printing raw memo bytes. `RELAY_FAUCET_COMMITMENT` logs show successful, ambiguous, or failed subaccount-1 forwarding attempts without printing raw memo bytes. Healthy empty scans and below-threshold scans are quiet; they do not produce repeated public log lines or durable status records. Canister logs are public observability, not durable full history.

## Status and Recovery

Production Relay exposes no public application query or update endpoints. Debug endpoints exist only in non-production debug builds. No controller-only production application endpoint is used for recovery. Replacement with full init args re-installs timers, resets runtime state, and emits one `relay LIFECYCLE event=<init_complete|post_upgrade_complete> ...` log line.

Production Relay identity and subaccount-1 addresses:

```text
relay principal: u2qkp-aqaaa-aaaar-qb7ea-cai
subaccount 1 hex: 0000000000000000000000000000000000000000000000000000000000000001
legacy ICP account identifier: 9fffa5e0762fd8be8e4c3078d4101926fb8d3c15aa3fa077b981ea779ded42ee
ICRC textual account: u2qkp-aqaaa-aaaar-qb7ea-cai-66ym2xq.1
```

## Tick Behavior

The default main interval is one day and timer intervals are clamped to at least 60 seconds. After init or post-upgrade, the relay schedules an internal, stateless one-shot startup liveness tick that calls the normal non-forced main tick path. This is not an endpoint and does not write diagnostic state. If the recent-run guard suppresses that tick, no extra config/timer-firing log line is emitted.

The first successful complete probe is baseline-only. It stores current cycles and does not spend ICP. Later ticks compare the previous completed sample, relay-minted cycles since that sample, and the current probe:

```text
estimated_burn_cycles = max(previous_cycles + relay_minted_cycles_since_previous_sample - current_cycles, 0)
```

Relay-minted cycles come from successful CMC `notify_top_up` responses. This prevents relay top-ups from hiding real burn when a canister's net cycles balance increases.

`max_transfers_per_tick`, when set, limits how many outgoing ledger transfers the default-account allocation job starts in one tick. It applies to CMC top-up transfers and surplus transfers. Set values must be greater than zero. Unstarted transfers remain in the active job and are resumed by later ticks. Surplus transfers are not planned until all canister top-up transfers for that job have either completed or been deterministically skipped. Subaccount-1 Jupiter Faucet commitment forwarding is a separate operation with its own at-most-one-transfer-per-run behavior so it can proceed even when the default account has no funds, is degraded, or is blocked by allocation-job state.

## Lifecycle

Relay operational state is heap-only. Cycle samples, relay-minted-cycle accounting, recovery deficits, conversion estimates, summaries, active jobs, pending transfers, faucet forwarding state, and job IDs are not persisted across install, reinstall, or upgrade.

Install, reinstall, and upgrade with full init args all initialize a fresh relay state from the supplied config. The first successful tick after replacement is `BaselineOnly` and spends no ICP.

Operators must pass full init args when replacing Relay Wasm. Replacement resets any in-flight relay job or recovery deficit. Operators should check managed canister cycle balances after replacement and manually top up any unexpectedly low canister.

## Relay Allocation Modes

Jupiter Relay has two allocation modes depending on whether raw ICP surplus recipients are configured.

Before Relay has a complete previous cycle sample for every effective managed canister, it runs in `BaselineOnly` mode: it records the current cycle balances and does not infer burn. If a later configuration change adds a new managed canister and makes the previous sample incomplete, Relay establishes the new baseline while preserving existing `recovery_deficit_cycles` for canisters that remain managed. New or newly sampled canisters start with no carried deficit.

### Raw ICP Recipients Configured

When one or more raw ICP surplus recipients are configured, Relay performs capped canister top-up planning.

Relay first attempts to refresh the latest CMC ICP/XDR conversion rate. It uses that CMC rate to calculate capped CMC top-ups from any carried recovery deficit plus recent observed burn with 1% headroom:

```text
cycles_per_e8 = xdr_permyriad_per_icp
new_burn_target_cycles = ceil(recent_burn_cycles * 101 / 100)
target_topup_cycles = carried_deficit_cycles + new_burn_target_cycles
planned_topup_e8s = ceil(target_topup_cycles / cycles_per_e8)
```

The 1% headroom applies only to fresh burn. Carried recovery deficits are not multiplied again.

`planned_topup_e8s` is the intended net CMC top-up amount and does not include the ledger fee. `sent_topup_e8s` is the accepted net amount sent to CMC, or zero if no ledger transfer was accepted. Summary-level `fee_e8s`, `ledger_fees_e8s`, and `ledger_sent_e8s` carry the fee accounting.

If the live CMC conversion-rate refresh fails or returns unusable data, Relay may fall back to the cached or bootstrap CMC estimate for capped top-up planning.

Relay always executes canister top-ups before raw ICP surplus routing. If there is not enough ICP to cover all planned top-ups and ledger fees, Relay spends only on canister top-ups and routes no raw ICP surplus. Underfunded, failed, ambiguous, or NoFunds rounds persist the unmet `target_topup_cycles - actual_minted_cycles` as `recovery_deficit_cycles` for that canister. Future ticks add that carried deficit to the fresh-burn target until it is recovered. Surplus routing is allowed only after the top-up phase completes cleanly and no per-canister recovery deficit remains.

Raw ICP surplus is routed only when every configured raw ICP recipient receives at least 1 ICP net of ledger fee. If the equal net share is below 1 ICP, Relay sends no raw ICP surplus transfers and keeps the ICP in its default ledger account for a future tick.

This threshold applies uniformly to all raw ICP surplus recipients, including canister targets and neuron targets.

### No Raw ICP Recipients Configured

When no raw ICP surplus recipients are configured, Relay does not query ICP/XDR and does not apply the 1% capped top-up policy.

Instead, Relay routes ICP to CMC top-ups using need-weighted allocations across managed canisters with positive need:

```text
all_cycles_need_cycles = recent_burn_cycles + carried_deficit_cycles
target_topup_cycles = all_cycles_need_cycles
```

The all-cycles batch is intentionally gated: Relay only sends the batch when every positive-need managed canister would receive a fee-efficient top-up. In practice, the slowest positive-need canister must receive a gross share of at least twice the ICP ledger fee, so that the net amount delivered to CMC is at least the fee paid to transfer it.

If the batch is not yet fee-efficient for every positive-burn canister, Relay sends no CMC top-ups in that tick and leaves the ICP in its default ledger account for a future tick.

This prevents slow-burning canisters from being skipped indefinitely and avoids wasting most of a slow burner's proportional allocation on ledger fees. Canisters with zero fresh burn but a carried recovery deficit still participate. Canisters with zero total need do not participate in the split and do not block the batch. Any unavoidable dust, integer-division remainder, or fee-unspendable balance remains in Relay's default ledger account.

Clearing all raw ICP surplus recipients therefore switches Relay into all-cycles mode.

## Surplus Recipient Configuration

Surplus recipients use split homogeneous public install records:

```text
SurplusCanisterRecipient {
  canister_id = principal
  memo = blob
}

SurplusNeuronRecipient {
  neuron_id = nat64
  memo = blob
}
```

Install args use `surplus_canister_recipients : opt vec SurplusCanisterRecipient`; production sets it to `null` for no canister surplus recipients. Install args use `surplus_neuron_recipients : vec SurplusNeuronRecipient`. An empty `memo = blob ""` means no outgoing ledger memo internally; a non-empty blob is used as the outgoing ledger memo. Canister targets route to `Account { owner = canister_id; subaccount = null }`. Neuron targets require a public NNS neuron; the relay reads NNS Governance, resolves the staking subaccount, transfers ICP to the Governance canister with that subaccount, and best-effort refreshes the neuron after transfer. Refresh failure is logged as a follow-up failure and does not roll back or duplicate a ledger-accepted transfer. The NNS claim/refresh endpoint is publicly callable, so a later natural flow or manual/public retry can refresh the neuron; no durable claim-refresh retry queue is maintained.

Top-ups use the same CMC path as the faucet: transfer ICP to the CMC deposit account derived from the target canister principal, then call `notify_top_up { canister_id, block_index }`.

Production surplus is split 50/50 between two public NNS neuron recipients:

- IO neuron `10292412127977304661`, with `memo = blob ""`
- Jupiter Faucet neuron `11614578985374291210`, with `memo = blob "10292412127977304661"`

The Jupiter Faucet neuron memo encodes the IO neuron ID as ASCII decimal bytes. This preserves the existing memo convention while separating immediate IO stake growth from compounding Jupiter Faucet neuron growth that feeds long-term IO-aligned maturity.

## Retry Safety

Each pending transfer stores a heap/runtime `created_at_time` and memo. Immediate ledger retries reuse the same identity during the current Wasm lifetime, and ledger `Duplicate` is treated as an accepted transfer using the duplicate block index. Once a ledger transfer is accepted, CMC `Processing` and transport-like notify failures are retried once inline. Repeated uncertainty is recorded as ambiguous rather than blindly retried with a changed transfer identity.

Subaccount-1 Jupiter Faucet commitment forwarding uses the same deterministic transfer identity and ledger duplicate handling. After a ledger-accepted transfer to the Jupiter Faucet neuron staking account, Relay marks the transfer complete and schedules NNS Governance `claim_or_refresh_neuron` on a zero-delay timer; a refresh failure is logged as a follow-up failure and does not roll back or duplicate the accepted ledger transfer.

## Operational Warning

A non-self managed canister that is not blackhole-readable prevents spending for that tick. This preserves funds for the next tick and prevents allocation from partial or stale cycle data.

If ledger or CMC uncertainty occurs after a transfer boundary, the summary marks the affected amount ambiguous rather than blindly changing transfer identity. If ledger acceptance never happened, the amount remains known-unspent.

## Production Operations Checklist

1. Verify the blackhole can read every configured managed canister.
2. Verify canister settings: logs public, log memory limit `2MiB`, canonical blackhole as an additional controller, and the current operational/admin controller retained until handoff is complete.
3. Compare `CONFIG` public logs with [`mainnet-install-args.did`](mainnet-install-args.did).
4. Observe a first complete baseline tick and confirm it spends no ICP.
5. Fund the relay with a small ICP amount through `u2qkp-aqaaa-aaaar-qb7ea-cai.`.
6. Observe the first allocation tick and verify CMC notifications and any surplus transfers match the expected policy.
7. Increase funding only after the baseline and first allocation behave as expected.

## Replacement

Production canister: `jupiter_relay` / `u2qkp-aqaaa-aaaar-qb7ea-cai`

Supply [`mainnet-install-args.did`](mainnet-install-args.did) explicitly when replacing Relay Wasm:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_relay \
  --environment ic \
  --mode upgrade \
  --args-file canisters/relay/mainnet-install-args.did
```

Replacement may use install, reinstall, or upgrade with full init args. All replacement paths reset heap-only operational state. Before replacement, confirm that state reset is intentional, save current public logs/settings if needed, and verify managed canister cycle balances are healthy enough for a new baseline. After replacement, confirm config/logs, confirm the first tick is `BaselineOnly`, check managed canister cycle balances, and manually top up if any canister is unexpectedly low.

Post-replacement verification:

```bash
./tools/scripts/smoke-relay-mainnet
icp canister logs u2qkp-aqaaa-aaaar-qb7ea-cai -n ic
```

Exact settings update command:

```bash
icp canister settings update u2qkp-aqaaa-aaaar-qb7ea-cai \
  --add-controller 77deu-baaaa-aaaar-qb6za-cai \
  --log-visibility public \
  --log-memory-limit 2mib \
  -n ic
```

Suggested settings and log checks:

```bash
icp canister settings show u2qkp-aqaaa-aaaar-qb7ea-cai -n ic
icp canister logs u2qkp-aqaaa-aaaar-qb7ea-cai -n ic
```
