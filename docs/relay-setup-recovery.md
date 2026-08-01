# Self-service Relay setup and recovery

Historian creates an immutable blackholed Relay for a submitted set of 1–20 target canisters. The caller supplies the complete target vector on every query or notification. Historian sorts principals by raw principal bytes, rejects duplicates, and hashes the canonical vector with the framed `jupiter-relay-target-set-v1\0` SHA-256 format.

The durable active relationship is only:

```text
target-set hash -> Relay canister ID
```

Memory ID 25 contains the single definitive setup map. Retired memories 22–24 are validated during upgrade and are never reused or rewritten. Active entries contain only the Relay principal. Target principals are not stored with the entry, no target-to-Relay registry exists, and Historian never inspects a blackholed child to reconstruct its targets.

Targets and Relay instances are tracked independently. Successful activation adds `RelayTarget` to every canonical target and `RelayInstance` to the child. Overlapping sets are valid, and set semantics keep each tracked-principal count unique. Tracker source classification obtains Relay principals through paginated `list_canisters` calls filtered by `RelayInstance`; observed ledger transfers determine which Relay sent a commitment.

## Funding and creation

The deterministic setup account uses the target-set hash as its Historian-owned subaccount. Funding authority is the aggregate `icrc1_balance_of` value for that account. Historian does not scan transaction history, inspect transfer sources, attribute funds to payers, accept block references, or automatically refund deposits.

The nominal minimum preserves the configured singleton minimum and adds 0.25 ICP for every target after the first. The live requirement is the greater of that nominal minimum and:

```text
ICP needed to mint configured create cycles
+ configured conversion safety margin
+ configured Relay subaccount-one seed
+ setup-to-CMC ledger fee
+ setup-to-Relay ledger fee
```

The frontend displays the cached-rate estimate and current ledger balance, but creation begins only when a user presses **Create Relay**. `notify_relay_setup` independently reads the live balance, ledger fee, and CMC rate. Underfunded requests leave the deposit in place and write no setup entry.

After sufficient funding, Historian synchronously reserves the exact hash. At most four distinct funded setups may be in `Creating`; a same-hash caller receives the existing phase without making further external calls. Every target must pass the shared Auto cycles probe before the first ledger transfer.

The irreversible workflow journals only the information needed to prevent replay:

- the prepared CMC transfer, its fixed timestamp, and accepted block;
- minted cycles;
- the create-dispatch timestamp and optional returned Relay ID;
- the prepared Relay-funding transfer, fixed timestamp, and accepted block;
- the current irreversible phase and a bounded diagnostic message.

Clean rejection of the first ledger transfer removes the reservation. Ambiguous transfers, CMC notification errors, any create error after dispatch, unreconciled installation errors, child funding failures, and failed final audits enter `ManualRecoveryRequired`. Public notifications never retry a manual-recovery entry.

## Child configuration and handoff

Relay `InitArgs` use the canonical targets held in the active call, `blackhole_canister_id = null`, and `max_transfers_per_tick = target_count + 2`. The two additional slots cover Relay self and surplus transfers. The normal remainder transfer funds subaccount one owned by the spawned Relay. Its Faucet memo therefore remains `<spawned Relay principal without hyphens>.Relay`; it never uses the canonical production Relay identity.

Before handoff, management state must report a running child, the approved Relay module hash, exactly Historian as controller, and public logs. Historian then requests exactly Fiduciary as controller with public logs. The post-handoff audit uses Fiduciary's real blackhole interface and requires running status, the approved hash, and exactly Fiduciary as controller. A reported `update_settings` error is accepted only if the observed final state is correct.

Successful activation replaces the entire progress record with `Active { relay_canister_id }`, then records independent tracking reasons without another await. The blackholed child is immutable and cannot be upgraded through Historian. Historian upgrades preserve the hash mapping and tracking state but never call, upgrade, or reconstruct a child Relay.

## Manual recovery

`ManualRecoveryRequired` is intentionally terminal for public automation. Controller/debug preflight exposes the setup hash, entry variant, phase, and optional Relay ID. Operators investigate the stored transfer timestamp/amount/fee/block, live ledger state, management state, and Fiduciary state through separately reviewed operational procedures. Unexpected deposits, including deposits after activation, also require operator-assisted recovery; they are not automatically swept or refunded.

## Pre-launch cutover and deployment sequence

No self-service setup has been used, so the upgrade performs no migration. It traps if retired setup-job memory contains a row or if retired registry memory contains anything other than the configured canonical Relay projection.

Production rollout order:

1. Deploy a maintenance frontend that disables new setup.
2. Disable the Relay factory.
3. Snapshot Historian.
4. Verify no old self-service setup or registration exists.
5. Verify no setup entry is `Creating`.
6. Stop or drain outstanding Historian calls under the normal deployment procedure.
7. Upgrade Historian in place; do not reinstall it.
8. Verify memory 25 is open and initially empty.
9. Deploy the final frontend.
10. Perform one singleton setup.
11. Perform one overlapping multi-target setup.
12. Verify active hash mappings, independent tracking reasons, child module hashes, running status, and Fiduciary-only controllers.
13. Re-enable the factory.

For subsequent Historian upgrades, disable the factory, verify no entry is `Creating`, drain calls, upgrade in place, verify active entries and tracking counts, then re-enable the factory.
