# Jupiter Relay

`jupiter-relay` is an ICP-funded cycles allocator for the Jupiter Faucet Suite. It spends ICP from the relay canister default ICP ledger account:

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
relay, auto-included    cm5kl-iiaaa-aaaac-be6za-cai
```

## Funding

The production relay default account can be funded through the faucet raw ICP route:

```text
cm5kl-iiaaa-aaaac-be6za-cai.
```

Start with a small funding amount, observe one allocation tick, and only then increase funding.

## Managed Canisters

Install args include `managed_canisters : vec principal`. The runtime set is the sorted unique union of that list and the relay canister itself. The relay is always included even when omitted from config.

Anonymous and management canister principals are rejected. Duplicate configured managed canisters are rejected. The relay probes its own cycles directly with `canister_cycle_balance`; every non-self managed canister must be readable through the configured blackhole canister.

If any required probe fails, the relay fails closed: it records a degraded summary and spends no ICP.

## Runtime Config Verification

The relay logs public runtime verification lines on every main tick that actually runs:

```text
Cycles: <relay_self_cycles_balance>
CONFIG relay_canister_id=...
```

The `CONFIG` line includes the configured managed canisters, effective managed canisters including relay self, ledger, CMC, blackhole, interval, transfer limit, raw ICP mode presence, raw ICP threshold, raw recipients, raw recipient memos, and whether the configured production managed set matches the known Jupiter suite set.

After deployment, anyone can verify the installed source/config by building the canister from the reviewed source, checking the production canister ID mapping, querying `get_config`, and comparing public logs:

```bash
icp canister call cm5kl-iiaaa-aaaac-be6za-cai get_config '()' -n ic
icp canister logs cm5kl-iiaaa-aaaac-be6za-cai -n ic
```

## Tick Behavior

The default main interval is seven days and timer intervals are clamped to at least 60 seconds. After upgrade, an active job schedules an immediate forced resume.

The first successful complete probe is baseline-only. It stores current cycles and does not spend ICP. Later ticks compare the previous completed sample with the current probe:

```text
burn = previous_cycles.saturating_sub(current_cycles)
```

If any canister gained cycles, that canister's burn is zero.

`max_transfers_per_tick`, when set, limits how many outgoing ledger transfers the relay starts in one tick. It applies to CMC top-up transfers and raw ICP recipient transfers. Set values must be greater than zero. Unstarted transfers remain in the active job and are resumed by later ticks.

## Upgrade Args

Optional upgrade fields use Candid tri-state values where supported:

```text
null            = leave the existing value unchanged
opt null        = clear the existing optional value
opt opt <value> = set the value
```

This applies to `max_transfers_per_tick` and `raw_icp_mode`. Plain optional fields such as `managed_canisters`, `ledger_canister_id`, `cmc_canister_id`, `blackhole_canister_id`, and `main_interval_seconds` use `null` to leave unchanged and `opt <value>` to set.

## CMC Top-Up Mode

When raw ICP mode is absent or inactive, the relay allocates the default account balance as CMC top-ups.

If total burn is positive, each canister's weight is its burn. Canisters with zero burn during a positive-burn interval receive no share. If no burn is detected anywhere, every managed canister gets weight `1` and the split is equal.

Gross shares are floored:

```text
gross_share = floor(balance_e8s * weight / total_weight)
```

The gross share includes the ledger fee. A top-up is sent only when `gross_share > fee`; the transferred amount is `gross_share - fee`. Dust and rounding remainder stay in the relay default account.

Top-ups use the same CMC path as the faucet: transfer ICP to the CMC deposit account derived from the target canister principal, then call `notify_top_up { canister_id, block_index }`.

## Raw ICP Mode

`raw_icp_mode` is optional. When configured, it activates per tick only if every managed canister probe succeeds and the minimum current cycles balance is strictly greater than `min_cycles_threshold`.

Raw ICP recipients split the default account balance equally by gross share. For each recipient:

- If the recipient is the relay default account, no transfer is made and that share remains in place.
- If `gross_share > fee`, the relay transfers `gross_share - fee` with the configured memo bytes.
- Otherwise the share is retained as dust.

The mode is not a latch. If a later tick finds any managed canister at or below the threshold, the relay returns to CMC top-up mode.

Example upgrade args to enable raw ICP mode later:

```candid
(
  opt record {
    managed_canisters = null;
    ledger_canister_id = null;
    cmc_canister_id = null;
    blackhole_canister_id = null;
    main_interval_seconds = null;
    max_transfers_per_tick = null;
    raw_icp_mode = opt opt record {
      min_cycles_threshold = 5_000_000_000_000 : nat;
      recipients = vec {
        record {
          account = record {
            owner = principal "<recipient-1>";
            subaccount = null;
          };
          memo = opt blob "\01";
        };
        record {
          account = record {
            owner = principal "cm5kl-iiaaa-aaaac-be6za-cai";
            subaccount = null;
          };
          memo = null;
        };
      };
    };
  }
)
```

Example upgrade args to clear raw ICP mode:

```candid
(
  opt record {
    managed_canisters = null;
    ledger_canister_id = null;
    cmc_canister_id = null;
    blackhole_canister_id = null;
    main_interval_seconds = null;
    max_transfers_per_tick = null;
    raw_icp_mode = opt null;
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
3. Query `get_config` and compare it with `jupiter-relay/mainnet-install-args.did`.
4. Observe a first complete baseline tick and confirm it spends no ICP.
5. Fund the relay with a small ICP amount through `cm5kl-iiaaa-aaaac-be6za-cai.`.
6. Observe the first allocation tick and verify CMC notifications or raw ICP transfers match the expected mode.
7. Increase funding only after the baseline and first allocation behave as expected.

Suggested settings checks:

```bash
icp canister settings show cm5kl-iiaaa-aaaac-be6za-cai -n ic
icp canister logs cm5kl-iiaaa-aaaac-be6za-cai -n ic
```
