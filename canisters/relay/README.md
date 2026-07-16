# Jupiter Relay

`jupiter-relay` keeps Jupiter Faucet Suite canisters funded with cycles. Relay spends ICP from its default ICP ledger account to top up managed canisters through CMC. Once managed-canister recovery targets are satisfied, any surplus ICP can be routed to configured surplus recipients.

Relay can be funded in three ways:

- direct ICP payment to Relay's default account for immediate one-off liquidity;
- a Jupiter Faucet commitment that targets Relay and perpetually pays raw ICP into Relay's default account;
- Relay subaccount 1, which accumulates or memo-fixes ICP and then creates that Jupiter Faucet commitment for Relay.

Default-account funding is direct liquidity. Faucet commitments are perpetual funding. Subaccount 1 is a staging helper for creating Faucet commitments when a direct commitment is not possible.

## Three ways to fund Relay

| Workflow | Send ICP to | Use when | What happens | Outcome |
|---|---|---|---|---|
| 1. Direct Relay funding | Relay default account | You need ICP available to Relay now for short-term canister-cycle funding, bootstrap, or manual recovery | Relay spends the ICP directly on managed-canister CMC top-ups first, then surplus if safe | One-off operational liquidity; must be replenished periodically |
| 2. Direct Jupiter Faucet commitment for Relay | Jupiter Faucet neuron staking account, with memo `u2qkp-aqaaa-aaaar-qb7ea-cai.` | You can attach the memo and send at least the qualifying commitment amount | Jupiter Faucet records a commitment whose payout target is Relay's default account in raw ICP form | Recommended perpetual funding stream into Relay |
| 3. Relay subaccount 1 staging | Relay subaccount 1 `u2qkp-aqaaa-aaaar-qb7ea-cai-66ym2xq.1` | You cannot attach the memo, or ICP arrives in small amounts below the Faucet threshold | Relay accumulates the ICP, adds the correct memo, and forwards a qualifying Jupiter Faucet commitment | Helper path that creates the same perpetual Faucet-backed funding stream |

## Workflow 1: Direct Relay default-account funding

This is the direct way to give Relay ICP to spend immediately. It is useful for bootstrapping, short-term funding gaps, manual recovery, or emergency canister-cycle support.

ICP sent to Relay's default account is not a Jupiter Faucet commitment. It does not create a perpetual stream. Once spent, it must be replenished by another direct payment or by Faucet-backed flows.

Relay uses default-account ICP for managed-canister CMC top-ups first. Surplus is only routed after canister recovery targets are satisfied.

Relay's default ICP ledger account is:

```text
Account { owner = u2qkp-aqaaa-aaaar-qb7ea-cai; subaccount = null }
```

The Relay default account remains the only account used for managed-canister CMC top-ups and configured surplus routing.

## Workflow 2: Direct Jupiter Faucet commitment for perpetual Relay funding

This is the recommended long-term funding path when the sender can make a normal qualifying Jupiter Faucet commitment.

The sender commits ICP through Jupiter Faucet with memo:

```text
u2qkp-aqaaa-aaaar-qb7ea-cai.
```

The Relay canister ID before the dot identifies Relay as the target. The trailing `.` matters: it asks Jupiter Faucet to route raw ICP to Relay, with an empty outgoing memo, rather than convert the payout to cycles first.

In [`jupiter-memo-policy`](../../crates/memo-policy), `canister_id.memo` means raw ICP to the canister default account with the right-hand segment used as the outgoing memo. An empty right-hand segment sends raw ICP with an empty memo.

Jupiter Faucet then perpetually pays Relay's default account in raw ICP according to the commitment's future payouts. Relay, not Jupiter Faucet, orchestrates the downstream CMC conversion and allocation across managed canisters.

This is different from sending ICP directly to Relay's default account. A direct payment is finite. A Faucet commitment is a recurring/perpetual funding source for Relay.

## Workflow 3: Relay subaccount 1 staging

Relay subaccount 1 is not for immediate canister top-ups. It is a staging account for creating a Jupiter Faucet commitment that targets Relay.

Subaccount 1 exists for two cases:

1. Memo is unavailable. Some funding paths can send ICP but cannot attach the required Jupiter Faucet memo. For example, minting maturity can send or mint ICP to Relay subaccount 1, and Relay later forwards the ICP with the correct memo.
2. ICP arrives in small pieces. If ICP arrives in dribs and drabs below Jupiter Faucet's minimum qualifying commitment threshold, sending each piece directly to Jupiter Faucet would not create qualifying commitments. Relay subaccount 1 accumulates those pieces until it can make one qualifying commitment.

Once subaccount 1 holds enough ICP to send at least the qualifying commitment amount after paying the ledger fee, Relay forwards `balance - fee` to the Jupiter Faucet neuron staking account and attaches the memo for Relay. Jupiter Faucet then treats that as a normal commitment that perpetually pays raw ICP back to Relay's default account.

Subaccount 1 eventually produces the same kind of perpetual funding stream as workflow 2. It just lets Relay assemble the commitment when the sender cannot do so directly.

Relay subaccount 1 is exactly 32 bytes: 31 zero bytes followed by `0x01`.

The production ICRC textual account is:

```text
u2qkp-aqaaa-aaaar-qb7ea-cai-66ym2xq.1
```

Its equivalent explicit ICRC account fields are:

```text
owner = u2qkp-aqaaa-aaaar-qb7ea-cai
subaccount = 0000000000000000000000000000000000000000000000000000000000000001
```

On each main tick, Relay checks:

```text
Account { owner = u2qkp-aqaaa-aaaar-qb7ea-cai, subaccount = opt blob "\00...\01" }
```

The destination Jupiter Faucet neuron is `11614578985374291210`, resolved through NNS Governance `list_neurons`. The forwarding memo is `u2qkpaqaaaaaaarqb7eacai.Relay`. Balances below `1 ICP + fee` remain in subaccount 1 for a future tick. `RELAY_FAUCET_COMMITMENT` logs show subaccount-1 forwarding attempts without printing raw memo bytes.

Subaccount 1 flow:

1. ICP arrives at Relay subaccount 1.
2. Relay checks subaccount 1 on each main tick.
3. If the balance is below `1 ICP + ledger fee`, Relay leaves it there.
4. Once the balance can produce at least a qualifying net commitment, Relay sends `balance - fee` to the Jupiter Faucet neuron staking account.
5. Relay attaches memo `u2qkpaqaaaaaaarqb7eacai.Relay`.
6. Jupiter Faucet records the commitment.
7. Future Faucet payouts from that commitment flow as raw ICP to Relay's default account.
8. Relay then uses that default-account ICP for managed-canister top-ups and surplus routing.

## When should I use which workflow?

Use direct Relay default-account funding when:

- Relay needs ICP available now;
- you are bootstrapping;
- you are filling a short-term gap;
- you are manually recovering after replacement or unexpected funding shortfall.

Use a direct Jupiter Faucet commitment when:

- you want long-term/perpetual Relay funding;
- you can attach memo `u2qkp-aqaaa-aaaar-qb7ea-cai.`;
- you can meet the Jupiter Faucet minimum qualifying commitment threshold.

Use Relay subaccount 1 when:

- you want long-term/perpetual Relay funding, but cannot attach the memo directly;
- the source produces small ICP amounts that individually do not meet the Jupiter Faucet threshold;
- you want Relay to accumulate those amounts and make the qualifying Faucet commitment later.

Do not use subaccount 1 when:

- you need immediate ICP available to Relay;
- you can already make a direct qualifying Jupiter Faucet commitment with the right memo;
- you intend a one-off operational top-up rather than a perpetual Faucet-backed funding stream.

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

## Managed Canisters

Install args include `managed_canisters : vec principal`. The runtime set is the sorted unique union of that list and the relay canister itself. The relay is always included even when omitted from config.

Anonymous and management canister principals are rejected. Duplicate configured managed canisters are rejected. The relay probes its own cycles directly with `canister_cycle_balance`.

Relay supports two cycles-observation modes:

- **Fixed mode** is used when `blackhole_canister_id` is configured. Non-self managed targets are probed through that configured blackhole canister. Known managed blackhole canisters are probed through themselves by calling their own `canister_status` endpoint with their own principal as the target.
- **Auto mode** is used when no `blackhole_canister_id` is configured. Relay tries the cached positive route first, then the 13-node blackhole, the Fiduciary blackhole, and SNS discovery. SNS-governed targets do not need the SNS root or target to be controlled by a blackhole; Relay can use SNS root status routes when the target is an SNS dapp.

The canonical production Relay remains Fixed mode through the Fiduciary blackhole route. Self-service Relays created by Historian use Auto mode and are immutable after controller handoff.

A failed probe does not immediately mean a managed target is deleted. Relay keeps an in-memory consecutive probe failure count per effective managed target. One or two consecutive failures preserve the conservative fail-closed behavior: Relay records a degraded summary and spends no default-account ICP. After three consecutive scheduled runs fail to probe the same target, Relay treats that target as unavailable for allocation in that run only. This is an availability classification after consecutive probe failures, not cryptographic proof of deletion. Relay excludes the unavailable target from target top-up planning, continues probing it on every later schedule, and resets the count to zero after any successful probe. Targets are never permanently marked deleted by this policy.

When unavailable targets are excluded and no transient failures remain, Relay keeps the relay canister itself in the managed set and applies the normal allocation rules to observable targets. For a self-service relay whose external target is unavailable after the threshold, Relay does not attempt that target top-up, still respects relay self-management, and routes remaining default-account ICP through the configured surplus rules. Unavailable targets at or above the threshold do not by themselves block surplus routing; transient failures below the threshold still do.

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

Canister logs have finite retention. Operators should archive logs externally if long-term history is required. Logs are intentionally low-noise: timer callbacks that are suppressed by the guard, startup liveness checks that are suppressed by the recent-run guard, empty scans, below-threshold subaccount-1 scans, routine per-canister allocation skips, and observable target-probe statuses do not produce extra public detail lines. Main ticks that actually proceed still emit the documented runtime and financial summary logs.

## Public Log Records

The relay emits consistent, single-line, grep-friendly public records:

```text
Cycles: <relay_self_cycles_balance>
CONFIG relay_canister_id=...
RELAY_SUMMARY mode=<BaselineOnly|TopUpThenSurplus|Degraded|NoFunds> started_at_ts_nanos=<nat64> completed_at_ts_nanos=<nat64-or-null> min_cycles_balance=<nat-or-null> min_cycles_canister_id=<principal-or-null> min_cycles_sample=<nat-or-null> total_burn_cycles=<nat> total_target_topup_cycles=<nat> total_actual_minted_cycles=<nat> total_carried_deficit_cycles=<nat> total_remaining_deficit_cycles=<nat> deficit_canister_count=<nat32> max_remaining_deficit_canister_id=<principal-or-null> max_remaining_deficit_cycles=<nat-or-null> balance_start_e8s=<nat64> fee_e8s=<nat64> transfer_count=<nat32> ledger_transfer_count=<nat32> ledger_sent_e8s=<nat64> ledger_fees_e8s=<nat64> cmc_notify_success_count=<nat32> cmc_notify_failed_count=<nat32> cmc_notify_ambiguous_count=<nat32> planned_retained_e8s=<nat64> known_unspent_e8s=<nat64> ambiguous_e8s=<nat64> failed_transfers=<nat32> ambiguous_transfers=<nat32> partial_tick_count=<nat32> conversion_cycles_per_e8=<nat-or-null> surplus_e8s_before_fees=<nat64> skipped_surplus_reason=<escaped-text-or-null> canister_skip_counts=<escaped-reason:count|none> surplus_allowed_despite_unavailable_targets=<bool>
RELAY_CANISTER canister_id=<principal> previous_cycles=<nat-or-null> current_cycles=<nat> relay_minted_cycles=<nat> burn_cycles=<nat> carried_deficit_cycles=<nat> target_topup_cycles=<nat> planned_topup_e8s=<nat64> sent_topup_e8s=<nat64> actual_minted_cycles=<nat> remaining_deficit_cycles=<nat> skipped_reason=<escaped-text-or-null>
RELAY_SURPLUS_TRANSFER target=<canister:principal|neuron:nat64> owner=<principal> subaccount=<hex-or-null> gross_share_e8s=<nat64> amount_e8s=<nat64> skipped_reason=<escaped-text-or-null> memo_len=<nat32-or-null>
RELAY_FAUCET_COMMITMENT source_owner=<principal> source_subaccount=<hex> destination_owner=<principal> destination_subaccount=<hex-or-null> balance_start_e8s=<nat64> amount_e8s=<nat64> fee_e8s=<nat64> memo_len=<nat32> skipped_reason=<escaped-text-or-null>
RELAY_PROBE_FAILURE canister_id=<principal> consecutive_failures=<nat32> error=<escaped-text>
RELAY_TARGET_PROBE canister_id=<principal> consecutive_probe_failures=<nat32> classification=<observable|transient_probe_failure|target_unavailable_after_consecutive_probe_failures> skipped_reason=<escaped-text-or-null>
relay LIFECYCLE event=<init_complete|post_upgrade_complete> timers_installed=true main_interval_seconds=<nat64>
relay ERR message=<escaped-text>
```

`RELAY_SUMMARY` aggregates the per-canister recovery view: `total_target_topup_cycles` is the tick's total CMC target for observable targets, `total_actual_minted_cycles` is cycles returned by successful CMC notify calls, `total_carried_deficit_cycles` is pre-existing recovery debt, `total_remaining_deficit_cycles` is unrecovered debt after the tick, and `deficit_canister_count` is the number of observable planned canisters still blocking surplus. The min/max fields identify the lowest observed cycles sample and largest remaining recovery deficit, while `canister_skip_counts` summarizes routine skip reasons without one line per canister. `RELAY_CANISTER` logs show current cycles, previous cycles, relay-minted cycles since the previous sample, estimated fresh burn, carried recovery deficit, the mode-specific top-up target, planned top-up e8s, sent top-up e8s, actual minted cycles, remaining recovery deficit, and skipped reason if any. Relay emits `RELAY_CANISTER` detail only for actionable cases such as accepted or minted top-ups, transfer failure or ambiguity context, and abnormal per-canister skipped reasons. `planned_topup_e8s` is the intended net CMC top-up amount. `sent_topup_e8s` is the accepted net amount actually sent to CMC, and is zero if the transfer was not accepted. `RELAY_SURPLUS_TRANSFER` logs show surplus recipients, amount, and memo length without printing raw memo bytes. `RELAY_TARGET_PROBE` logs expose transient probe failures and targets that are unavailable after consecutive probe failures; observable targets are summarized instead of logged one by one. `RELAY_FAUCET_COMMITMENT` logs show successful, ambiguous, or failed subaccount-1 forwarding attempts without printing raw memo bytes. Healthy empty scans and below-threshold scans are quiet; they do not produce repeated public log lines or durable status records. Canister logs are public observability, not durable full history.

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

Relay is replacement-style and heap-only. It does not persist config or operational state in stable memory. Config, cycle samples, relay-minted-cycle accounting, recovery deficits, consecutive probe failure counts, conversion estimates, summaries, active jobs, pending transfers, faucet forwarding state, and job IDs are initialized fresh from supplied `InitArgs` on install, reinstall, and upgrade.

Relay upgrades are non-resumable. Avoid upgrading during active Relay work where practical, including active top-ups, ambiguous transfers, or CMC notify sequences. If an operation is interrupted, Relay starts fresh from the supplied `InitArgs`. After upgrade, confirm the fresh `CONFIG` log, check managed canister cycle balances, and manually top up or reconcile if needed.

This is intentional and differs from Faucet, Disburser, and Historian, which preserve safety-critical stable state across ordinary upgrades.

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

Relay always executes canister top-ups before raw ICP surplus routing. If there is not enough ICP to cover all planned top-ups and ledger fees, Relay spends only on canister top-ups and routes no raw ICP surplus. Underfunded, failed, ambiguous, or NoFunds rounds persist the unmet `target_topup_cycles - actual_minted_cycles` as `recovery_deficit_cycles` for that canister. Future ticks add that carried deficit to the fresh-burn target until it is recovered. Surplus routing is allowed only after relay self-management, observable-target top-up planning, retained ICP accounting, and ledger fees are satisfied, and no observable planned canister recovery deficit remains. Targets skipped after the consecutive-probe-failure threshold do not block surplus for that run.

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

A non-self managed canister that is not observable through the configured Fixed route or through Auto route discovery prevents spending for that tick. This preserves funds for the next tick and prevents allocation from partial or stale cycle data.

If ledger or CMC uncertainty occurs after a transfer boundary, the summary marks the affected amount ambiguous rather than blindly changing transfer identity. If ledger acceptance never happened, the amount remains known-unspent.

## Production Operations Checklist

1. For Fixed mode, verify the configured blackhole can read every configured managed canister. For Auto mode, verify the target is observable through cached route, 13-node, Fiduciary, or SNS status discovery.
2. Verify canister settings: logs public, log memory limit `2MiB`, canonical blackhole as an additional controller, and the current operational/admin controller retained until handoff is complete.
3. Compare `CONFIG` public logs with [`mainnet-install-args.did`](mainnet-install-args.did).
4. Observe a first complete baseline tick and confirm it spends no ICP.
5. For allocation testing, fund Relay's default account with a small direct ICP payment so liquidity is available immediately.
6. Observe the first allocation tick and verify CMC notifications and any surplus transfers match the expected policy.
7. Increase funding only after the baseline and first allocation behave as expected.

## Production upgrades

Production canister: `jupiter_relay` / `u2qkp-aqaaa-aaaar-qb7ea-cai`

### Routine replacement upgrade

Routine Relay upgrades pass the full reviewed `InitArgs` file. Relay intentionally requires full InitArgs on upgrade because it does not persist config in stable memory. Under this replacement-style lifecycle, Relay does not support no-arg upgrades and does not support Relay UpgradeArgs.

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_relay \
  --environment ic \
  --mode upgrade \
  --args-file canisters/relay/mainnet-install-args.did
```

Avoid running this command during active Relay work where practical. After upgrade, confirm the fresh `CONFIG` log from supplied `InitArgs`, confirm the first successful tick is `BaselineOnly`, check managed canister cycle balances, and manually top up or reconcile if needed.

### Config-changing replacement upgrade

Config-changing Relay upgrades use full `InitArgs`. Relay has no `UpgradeArgs`, so the standard production path is to update [`canisters/relay/mainnet-install-args.did`](mainnet-install-args.did) in the repo, review the diff, and deploy that reviewed checked-in file. A config-changing upgrade is also non-resumable and resets all Relay heap state; avoid active Relay work where practical.

Example shape:

```did
(
  record {
    managed_canisters = vec { principal "..." };
    ledger_canister_id = opt principal "ryjl3-tyaaa-aaaaa-aaaba-cai";
    cmc_canister_id = opt principal "rkp4c-7iaaa-aaaaa-aaaca-cai";
    governance_canister_id = opt principal "rrkah-fqaaa-aaaaa-aaaaq-cai";
    blackhole_canister_id = opt principal "e3mmv-5qaaa-aaaah-aadma-cai";
    main_interval_seconds = opt (86400 : nat64);
    max_transfers_per_tick = opt (10 : nat32);
    surplus_canister_recipients = null;
    surplus_neuron_recipients = vec {};
  },
)
```

Deploy with:

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_relay \
  --environment ic \
  --mode upgrade \
  --args-file canisters/relay/mainnet-install-args.did
```

`max_transfers_per_tick = opt <nat32>` sets the transfer limit; `max_transfers_per_tick = null` clears it. For `surplus_canister_recipients`, `null` means no canister recipients. For `surplus_neuron_recipients`, `vec {}` means no neuron recipients.

### Fresh install

Fresh install uses the reviewed Relay `InitArgs` file. This creates fresh config and fresh operational state.

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_relay \
  --environment ic \
  --mode install \
  --args-file canisters/relay/mainnet-install-args.did
```

### Destructive reinstall

Destructive reinstall uses the reviewed Relay `InitArgs` file. This creates fresh config and fresh operational state.

```bash
JUPITER_USE_CANONICAL_ARTIFACTS=1 icp deploy jupiter_relay \
  --environment ic \
  --mode reinstall \
  --args-file canisters/relay/mainnet-install-args.did
```

Reinstall is destructive to Relay Wasm and heap state. Use it only when replacing Relay as a fresh deployment and after confirming external ICP/cycles conditions are safe.

Before any Relay upgrade or reinstall, confirm that state reset is intentional, save current public logs/settings if needed, and verify managed canister cycle balances are healthy enough for a new baseline.

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
