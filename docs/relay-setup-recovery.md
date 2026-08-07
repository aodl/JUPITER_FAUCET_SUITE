# Self-service Relay setup and recovery

Historian creates an immutable blackholed Relay for a submitted set of 1–20 target canisters. The caller supplies the complete target vector on every query or notification. Historian sorts principals by raw principal bytes, rejects duplicates, and hashes the canonical vector with the framed `jupiter-relay-target-set-v1\0` SHA-256 format.

The durable active relationship is only:

```text
target-set hash -> Relay canister ID
```

Memory ID 25 contains the single definitive setup map. Retired registry memory 22 is validated only during the first cutover, and retired setup-job memory 24 is purged only after that validation succeeds. Retired memories are never reused by the new setup system. Active entries contain only the Relay principal. Target principals are not stored with the entry, no target-to-Relay registry exists, and Historian never inspects a blackholed child to reconstruct its targets.

Targets and Relay instances are tracked independently. Successful activation adds `RelayTarget` to every canonical target and `RelayInstance` to the child. Overlapping sets are valid, and set semantics keep each tracked-principal count unique. Tracker source classification obtains Relay principals through paginated `list_canisters` calls filtered by `RelayInstance`; observed ledger transfers determine which Relay sent a commitment.

## Funding and creation

The deterministic setup account uses the target-set hash as its Historian-owned subaccount. Funding authority is the aggregate `icrc1_balance_of` value for that account. Historian does not scan transaction history, inspect transfer sources, attribute funds to payers, accept block references, or automatically refund deposits.

Every target after the first adds 0.25 ICP. The authoritative requirement is:

```text
max(
  configured singleton minimum,
  ICP needed to mint configured create cycles
  + configured conversion safety margin
  + configured Relay subaccount-one seed
  + setup-to-CMC ledger fee
  + setup-to-Relay ledger fee
)
+ 0.25 ICP * (target count - 1)
```

The query view displays the nominal minimum and current ledger balance. The authoritative live requirement is computed only when a user presses **Create Relay**. `notify_relay_setup` independently reads the live ledger fee, balance, and CMC rate and returns the exact balance, requirement, and shortfall when underfunded. The frontend retains that returned requirement while polling. Underfunded requests leave the deposit in place and write no setup entry.

After sufficient funding, Historian synchronously reserves the exact hash. At most four distinct funded setups may be in `Creating`; a same-hash caller receives the existing phase without making further external calls. Every target must pass the shared Auto cycles probe before the first ledger transfer.

The irreversible workflow journals only the information needed to prevent replay:

- the prepared CMC transfer, its fixed timestamp, and accepted block;
- minted cycles;
- the create-dispatch timestamp and optional returned Relay ID;
- the prepared Relay-funding transfer, fixed timestamp, and accepted block;
- the current irreversible phase and a bounded diagnostic message.

Clean rejection of the first ledger transfer removes the reservation. Ambiguous transfers, CMC notification errors, any create error after dispatch, unreconciled installation errors, child funding failures, and failed final audits enter `ManualRecoveryRequired`. Public notifications never retry a manual-recovery entry.

Operator triage distinguishes `create_canister ambiguous relay ID loss` from `install_code module-hash reconciliation`. If the module hash exists but differs from the approved Relay hash, Historian does not reinstall or continue automatically.

## Child configuration and handoff

Relay `InitArgs` use the canonical targets held in the active call, `blackhole_canister_id = null`, and `max_transfers_per_tick = target_count + 2`. The two additional slots cover Relay self and surplus transfers. The CMC receives only the ICP conversion amount. Historian then rereads both the live ledger fee and the setup-account balance before transferring `balance - current ledger fee` to subaccount one owned by the spawned Relay. This second fee read is deliberate because Relay creation can span enough external work for the fee to change. The safety margin, extra-target charge, configured seed, and any additional balance therefore fund Relay subaccount one. Its Faucet memo remains `<spawned Relay principal without hyphens>.Relay`; it never uses the canonical production Relay identity.

Historian-created Relays omit the optional SNS-rewards and ICP-Index fields and inherit the canonical production defaults embedded in Relay. No OpenChat, CHAT, jUP, or other SNS Ledger ID is embedded in child install arguments: Relay asks `jupiter-sns-rewards` for a fresh Root-derived Ledger context on every new reward sweep.

Historian's irreversible setup workflow remains fail-closed or manual-recovery-oriented. Relay's runtime heap cache and bootstrap fee fallback apply only after Relay exists; they do not silently change Historian's authoritative requirement, final funding transfer, or other irreversible setup phases.

Before handoff, management state must report a running child, the approved Relay module hash, exactly Historian as controller, and public logs. Historian then requests exactly Fiduciary as controller with public logs. The post-handoff audit uses Fiduciary's real blackhole interface and requires running status, the approved hash, and exactly Fiduciary as controller. A reported `update_settings` error is accepted only if the observed final state is correct.

Successful activation replaces the entire progress record with `Active { relay_canister_id }`, then records independent tracking reasons without another await. The blackholed child is immutable and cannot be upgraded through Historian. Historian upgrades preserve the hash mapping and tracking state but never call, upgrade, or reconstruct a child Relay.

After an upgrade, interrupted `Reserved` and `ProbingTargets` entries are removed because no irreversible action occurred. Every later `Creating` phase becomes `ManualRecoveryRequired` with `HistorianUpgradeInterrupted`. Existing active and manual-recovery entries remain unchanged; reconciliation makes no external call and never resumes a transfer or child creation.

## Manual recovery

`ManualRecoveryRequired` is intentionally terminal for public automation. For a known exact target set, the production setup query exposes the phase, optional Relay ID, and bounded message. Operators investigate ledger state, management state, and Fiduciary state through separately reviewed operational procedures. Debug entry listing is available only to local tests, PocketIC, and non-production debug builds; it is not a production preflight or enumeration mechanism. Unexpected deposits, including deposits after activation, also require operator-assisted recovery; they are not automatically swept or refunded.

## Pre-launch cutover and deployment sequence

Memory 25 is new at this cutover, so no pre-existing `Creating` entry exists to enumerate. Production was confirmed to contain multiple pre-launch setup-job records. The operator was the sole pre-launch user and explicitly authorized abandoning every legacy setup job, every deterministic setup-account balance, all associated cycles, and any possible unrecorded or incomplete child attempt. No legacy recovery record needs to survive in the new system. The first cutover therefore does not inspect, identify, enumerate, classify, or individually validate memory-24 targets or job contents.

The cutover first validates the complete retired registry. Memory 22 must be empty or exactly the complete configured canonical Relay projection: its length must equal the configured target count, every configured target must exist under its matching key, and every entry must use the configured canonical Relay ID with matching target, `Canonical` kind, and `Active` status. Any self-service, partial, extra, incorrect, or inconsistent registry traps before memory 24 changes, leaving every retired job intact even before IC upgrade rollback semantics are considered.

After registry validation, the cutover logically clears memory 24 with `clear_new()` without iterating, reading, or decoding any stored key or value, proves memory 24 is empty, opens memory 25, and proves memory 25 is empty. Nothing is migrated as `Creating`, `Active`, or `ManualRecoveryRequired`, and no `RelayTarget`, `RelayInstance`, audit record, cursor, or migration-status state is created. No Ledger, Index, CMC, or management-canister call occurs.

Old Ledger balances, Ledger history, cycles, and possible orphaned child canisters are neither recovered nor erased; they are deliberately abandoned in place. The new target-set setup system starts empty. This is a one-time bootstrap decision based on the operator's pre-launch ownership and authorization, not a general future migration policy. After a successful cutover, memory 25 marks the new system as initialized and later upgrades do not rerun the purge path.

Mainnet install args enable `relay_factory_enabled = opt true`. Because `notify_relay_setup` is public and can consume historian cycles after sufficient funding, production monitoring must cover factory concurrency, child-creation cycle spend, and manual-recovery entries.

Production rollout order:

1. Record all pre-upgrade public query evidence while Historian is still running.
2. Stop Historian and wait until its status is `Stopped`; this is the authoritative factory pause and call drain.
3. Create and download a snapshot.
4. Upgrade Historian in place; do not reinstall it.
5. While Historian remains stopped, deploy the matching multi-target frontend.
6. Start Historian only after both deployments succeed.
7. Verify public state and the new setup API.
8. Perform one controlled singleton setup and one overlapping multi-target setup.
9. Verify child module hashes, running status, Fiduciary-only controllers, and independent tracking counts.
10. Retain the snapshot until acceptance is complete.

If the upgrade or one-time cutover gate fails, do not proceed. Restore the snapshot according to the rehearsed rollback procedure and prove the restored canister is queryable before rescheduling the cutover.

For later Historian upgrades:

1. Record public state and active mappings for known exact target sets.
2. Stop Historian and wait until its status is `Stopped`.
3. Create and download a snapshot.
4. Upgrade Historian in place.
5. Start Historian.
6. Verify active mappings through the known exact target sets; production cannot enumerate every target-set hash.
7. Verify `RelayTarget` and `RelayInstance` counts.
8. Verify any interrupted post-spend setup is `ManualRecoveryRequired`.
9. Re-enable normal UI access.

The post-upgrade rule remains: `Reserved`/`ProbingTargets` entries are removed; every later `Creating` phase becomes `ManualRecoveryRequired`; `Active` and existing `ManualRecoveryRequired` entries are preserved.
