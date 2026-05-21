# Jupiter Relay

`jupiter-relay` is an ICP-funded cycles allocator and surplus router for the Jupiter Faucet Suite. Jupiter Faucet uses the Jupiter Faucet Relay canister as a singular target for perpetual suite top-ups in raw ICP form, using the `.` memo syntax. The relay periodically samples the cycles balance of all Jupiter Faucet Suite canisters and tops them up proportional to recent burn. Each canister top-up is capped at 1% more than recent burn, so dependent canisters are designed to keep growing in cycles without over-accumulating surplus. Remaining production surplus ICP is split equally between IO neuron `6345890886899317159` and Jupiter Faucet neuron `11614578985374291210`. This gives 50% immediate IO stake growth and 50% compounding Jupiter Faucet neuron growth that feeds long-term IO-aligned maturity.

It spends ICP from the relay canister default ICP ledger account:

```text
Account { owner = <relay_canister_id>, subaccount = null }
```

Fund it through the existing faucet raw-ICP memo route with:

```text
<relay_canister_id>.
```

The trailing dot is required. In `jupiter-memo-policy`, `canister_id.memo` means raw ICP to the canister default account with the right-hand segment used as the outgoing memo; an empty right-hand segment sends raw ICP with an empty memo.

## Role in the Suite

The production relay is intended to top up all Jupiter-operated solution canisters plus the canonical blackhole canister. The relay itself is automatically included at runtime; operators should not include the relay principal in `managed_canisters`.

System and dependency canisters are intentionally excluded from the managed set. The relay should not manage the ICP ledger, CMC, NNS governance, ICP index, SNS-WASM, SNS roots, XRC, or other external dependencies.

## Production Managed Set

```text
jupiter_disburser       uccpi-cqaaa-aaaar-qby3q-cai
jupiter_lifeline        afisn-gqaaa-aaaar-qb4qa-cai
jupiter_faucet          acjuz-liaaa-aaaar-qb4qq-cai
jupiter_sns_rewards     alk7f-5aaaa-aaaar-qb4ra-cai
jupiter_faucet_frontend jufzc-caaaa-aaaar-qb5da-cai
jupiter_historian       j5gs6-uiaaa-aaaar-qb5cq-cai
blackhole               77deu-baaaa-aaaar-qb6za-cai
relay, auto-included    u2qkp-aqaaa-aaaar-qb7ea-cai
```

## Funding

The production relay default account can be funded through the faucet raw ICP route:

```text
u2qkp-aqaaa-aaaar-qb7ea-cai.
```

Start with a small funding amount, observe one baseline tick and one allocation tick, and only then increase funding.

## Managed Canisters

Install args include `managed_canisters : vec principal`. The runtime set is the sorted unique union of that list and the relay canister itself. The relay is always included even when omitted from config.

Anonymous and management canister principals are rejected. Duplicate configured managed canisters are rejected. The relay probes its own cycles directly with `canister_cycle_balance`; every non-self managed canister must be readable through the configured blackhole canister.

If any required probe fails, the relay fails closed: it records a degraded summary and spends no ICP.

## Runtime Config Verification

The production relay intentionally exposes no public application endpoints. Debug endpoints are only available in non-production debug builds, and the debug API guard traps if a debug build is ever installed at the production relay principal.

The relay logs public runtime verification lines on every main tick that actually runs:

```text
Cycles: <relay_self_cycles_balance>
CONFIG relay_canister_id=...
```

The `CONFIG` line includes the configured managed canisters, effective managed canisters including relay self, ledger, CMC, NNS Governance, blackhole, interval, transfer limit, surplus recipients, surplus memo lengths, and whether the configured production managed set matches the known Jupiter suite set.

After deployment, anyone can verify the installed source/config by building the canister from the reviewed source, checking the production canister ID mapping, comparing public logs with `jupiter-relay/mainnet-install-args.did`, and using the frontend source pane. Public verification happens through logs, reproducible build/source metadata, the production canister ID mapping, and the frontend source pane.

```bash
icp canister logs u2qkp-aqaaa-aaaar-qb7ea-cai -n ic
```

Canister logs have finite retention. Operators should archive logs externally if long-term history is required.

## Public Log Records

The relay emits stable, single-line, grep-friendly public records:

```text
Cycles: <relay_self_cycles_balance>
CONFIG relay_canister_id=...
RELAY_SUMMARY mode=<BaselineOnly|TopUpThenSurplus|Degraded|NoFunds> started_at_ts_nanos=<nat64> completed_at_ts_nanos=<nat64-or-null> min_cycles_balance=<nat-or-null> total_burn_cycles=<nat> balance_start_e8s=<nat64> fee_e8s=<nat64> transfer_count=<nat32> ledger_transfer_count=<nat32> ledger_sent_e8s=<nat64> ledger_fees_e8s=<nat64> cmc_notify_success_count=<nat32> cmc_notify_failed_count=<nat32> cmc_notify_ambiguous_count=<nat32> planned_retained_e8s=<nat64> known_unspent_e8s=<nat64> ambiguous_e8s=<nat64> failed_transfers=<nat32> ambiguous_transfers=<nat32> partial_tick_count=<nat32> conversion_cycles_per_e8=<nat-or-null> surplus_e8s_before_fees=<nat64> skipped_surplus_reason=<escaped-text-or-null>
RELAY_CANISTER canister_id=<principal> previous_cycles=<nat-or-null> current_cycles=<nat> relay_minted_cycles=<nat> burn_cycles=<nat> target_topup_cycles=<nat> planned_topup_e8s=<nat64> actual_topup_e8s=<nat64> actual_minted_cycles=<nat> skipped_reason=<escaped-text-or-null>
RELAY_SURPLUS_TRANSFER target=<canister:principal|neuron:nat64> owner=<principal> subaccount=<hex-or-null> gross_share_e8s=<nat64> amount_e8s=<nat64> skipped_reason=<escaped-text-or-null> memo_len=<nat32-or-null>
RELAY_PROBE_FAILURE canister_id=<principal> error=<escaped-text>
```

`RELAY_CANISTER` logs show current cycles, previous cycles, relay-minted cycles since the previous sample, estimated burn, burn plus fixed 1% headroom, planned top-up e8s, actual top-up e8s, actual minted cycles, and skipped reason if any. `RELAY_SURPLUS_TRANSFER` logs show surplus recipients, amount, and memo length without printing raw memo bytes.

## Tick Behavior

The default main interval is seven days and timer intervals are clamped to at least 60 seconds. After upgrade, an active job schedules an immediate forced resume.

The first successful complete probe is baseline-only. It stores current cycles and does not spend ICP. Later ticks compare the previous completed sample, relay-minted cycles since that sample, and the current probe:

```text
estimated_burn_cycles = max(previous_cycles + relay_minted_cycles_since_previous_sample - current_cycles, 0)
target_topup_cycles = ceil(estimated_burn_cycles * 101 / 100)
```

Relay-minted cycles come from successful CMC `notify_top_up` responses. This prevents relay top-ups from hiding real burn when a canister's net cycles balance increases.

`max_transfers_per_tick`, when set, limits how many outgoing ledger transfers the relay starts in one tick. It applies to CMC top-up transfers and surplus transfers. Set values must be greater than zero. Unstarted transfers remain in the active job and are resumed by later ticks. Surplus transfers are not planned until all canister top-up transfers for that job have either completed or been deterministically skipped.

## Upgrade Args

Optional upgrade fields use Candid tri-state values where supported:

```text
null            = leave the existing value unchanged
opt null        = clear the existing optional value
opt opt <value> = set the value
```

This applies to `max_transfers_per_tick`. Plain optional fields such as `managed_canisters`, `ledger_canister_id`, `cmc_canister_id`, `governance_canister_id`, `blackhole_canister_id`, `main_interval_seconds`, and `surplus_recipients` use `null` to leave unchanged and `opt <value>` to set.

## Top-Ups And Surplus

The relay always executes canister top-ups before surplus routing. If there is not enough ICP to cover all planned top-ups and ledger fees, the relay spends only on canister top-ups and routes no surplus.

Top-up planning uses a recent CMC conversion estimate:

```text
cycles_per_e8 = minted_cycles / transferred_e8s
planned_topup_e8s = ceil(target_topup_cycles / cycles_per_e8)
```

If no reliable conversion estimate exists, the relay may use an internal bootstrap estimate for canister top-up planning, but surplus routing is disabled for that tick. Stale or ambiguous conversion data also disables surplus routing. A sharp drop in cycles-per-e8 reduces or eliminates surplus routing until canister top-up needs are covered.

Surplus recipients are typed targets:

```text
SurplusRecipient {
  target = Canister(principal) | Neuron(nat64)
  memo = opt blob
}
```

Canister targets route to `Account { owner = canister_id; subaccount = null }`. Neuron targets require a public NNS neuron; the relay reads NNS Governance, resolves the staking subaccount, transfers ICP to the Governance canister with that subaccount, and best-effort refreshes the neuron after transfer. Refresh failure is logged as a follow-up failure and does not roll back or duplicate a ledger-accepted transfer.

Top-ups use the same CMC path as the faucet: transfer ICP to the CMC deposit account derived from the target canister principal, then call `notify_top_up { canister_id, block_index }`.

Production surplus is split equally between two public NNS neuron recipients:

- IO neuron `6345890886899317159`, with `memo = null`
- Jupiter Faucet neuron `11614578985374291210`, with memo `6345890886899317159`

The Jupiter Faucet neuron memo encodes the IO neuron ID as ASCII decimal bytes. This preserves the existing memo convention while separating immediate IO stake growth from compounding Jupiter Faucet neuron growth that feeds long-term IO-aligned maturity.

Example upgrade args to set the production surplus recipients:

```candid
(
  opt record {
    managed_canisters = null;
    ledger_canister_id = null;
    cmc_canister_id = null;
    governance_canister_id = null;
    blackhole_canister_id = null;
    main_interval_seconds = null;
    max_transfers_per_tick = null;
    surplus_recipients = opt vec {
      record {
        target = variant { Neuron = 6_345_890_886_899_317_159 : nat64 };
        memo = null;
      };
      record {
        target = variant { Neuron = 11_614_578_985_374_291_210 : nat64 };
        memo = opt blob "6345890886899317159";
      };
    };
  }
)
```

## Retry Safety

Each pending transfer stores a stable `created_at_time` and memo. Immediate ledger retries reuse the same identity, and ledger `Duplicate` is treated as an accepted transfer using the duplicate block index. Once a ledger transfer is accepted, CMC `Processing` and transport-like notify failures are retried once inline. Repeated uncertainty is recorded as ambiguous rather than blindly retried with a changed transfer identity.

## Operational Warning

A non-self managed canister that is not blackhole-readable prevents spending for that tick. This preserves funds for the next tick and prevents allocation from partial or stale cycle data.

If ledger or CMC uncertainty occurs after a transfer boundary, the summary marks the affected amount ambiguous rather than blindly changing transfer identity. If ledger acceptance never happened, the amount remains known-unspent.

## Production Operations Checklist

1. Verify the blackhole can read every configured managed canister.
2. Verify canister settings: logs public, log memory limit `2MiB`, canonical blackhole as an additional controller, and the current operational/admin controller retained until handoff is complete.
3. Compare `CONFIG` public logs with `jupiter-relay/mainnet-install-args.did`.
4. Observe a first complete baseline tick and confirm it spends no ICP.
5. Fund the relay with a small ICP amount through `u2qkp-aqaaa-aaaar-qb7ea-cai.`.
6. Observe the first allocation tick and verify CMC notifications and any surplus transfers match the expected policy.
7. Increase funding only after the baseline and first allocation behave as expected.

This MR does not perform deployment. Production deployment and settings changes remain manual operator actions after review.

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
