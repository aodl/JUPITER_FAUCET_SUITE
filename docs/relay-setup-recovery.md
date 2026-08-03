# Self-service Relay setup and recovery

Historian creates an immutable blackholed Relay for a submitted set of 1–20 target canisters. The caller supplies the complete target vector on every query or notification. Historian sorts principals by raw principal bytes, rejects duplicates, and hashes the canonical vector with the framed `jupiter-relay-target-set-v1\0` SHA-256 format.

The durable active relationship is only:

```text
target-set hash -> Relay canister ID
```

Memory ID 25 contains the single definitive setup map. Retired memories 22–24 are validated only during the first cutover and are never reused or rewritten. Active entries contain only the Relay principal. Target principals are not stored with the entry, no target-to-Relay registry exists, and Historian never inspects a blackholed child to reconstruct its targets.

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

The query view displays the nominal minimum and current ledger balance. The authoritative live requirement is computed only when a user presses **Create Relay**. `notify_relay_setup` reads the live balance, ledger fee, and CMC rate and returns the exact balance, requirement, and shortfall when underfunded. The frontend retains that returned requirement while polling. Underfunded requests leave the deposit in place and write no setup entry.

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

Relay `InitArgs` use the canonical targets held in the active call, `blackhole_canister_id = null`, and `max_transfers_per_tick = target_count + 2`. The two additional slots cover Relay self and surplus transfers. The CMC receives only the ICP conversion amount. Historian then rereads the setup-account balance and transfers `balance - current ledger fee` to subaccount one owned by the spawned Relay. The safety margin, extra-target charge, configured seed, and any additional balance therefore fund Relay subaccount one. Its Faucet memo remains `<spawned Relay principal without hyphens>.Relay`; it never uses the canonical production Relay identity.

Before handoff, management state must report a running child, the approved Relay module hash, exactly Historian as controller, and public logs. Historian then requests exactly Fiduciary as controller with public logs. The post-handoff audit uses Fiduciary's real blackhole interface and requires running status, the approved hash, and exactly Fiduciary as controller. A reported `update_settings` error is accepted only if the observed final state is correct.

Successful activation replaces the entire progress record with `Active { relay_canister_id }`, then records independent tracking reasons without another await. The blackholed child is immutable and cannot be upgraded through Historian. Historian upgrades preserve the hash mapping and tracking state but never call, upgrade, or reconstruct a child Relay.

After an upgrade, interrupted `Reserved` and `ProbingTargets` entries are removed because no irreversible action occurred. Every later `Creating` phase becomes `ManualRecoveryRequired` with `HistorianUpgradeInterrupted`. Existing active and manual-recovery entries remain unchanged; reconciliation makes no external call and never resumes a transfer or child creation.

## Manual recovery

`ManualRecoveryRequired` is intentionally terminal for public automation. For a known exact target set, the production setup query exposes the phase, optional Relay ID, and bounded message. Operators investigate ledger state, management state, and Fiduciary state through separately reviewed operational procedures. Debug entry listing is available only to local tests, PocketIC, and non-production debug builds; it is not a production preflight or enumeration mechanism. Unexpected deposits, including deposits after activation, also require operator-assisted recovery; they are not automatically swept or refunded.

## Pre-launch cutover and deployment sequence

Memory 25 is new at this cutover, so no pre-existing `Creating` entry exists to enumerate. The first production cutover recognizes one operator-authorized abandoned pre-launch job for target `2lo52-kiaaa-aaaar-qaqta-cai`. It is accepted only when its complete reviewed stable fingerprint matches, including the old singleton setup identity, `ManualRecoveryRequired` low-minted-cycles diagnostic, completed CMC conversion at block `37414364`, exact unresolved create attempt, completed refund metadata, timestamps and amounts, and the absence of any child, installation, Relay funding, sweep, or controller-handoff evidence.

The cutover validates the complete retired registry and every other first-cutover invariant before changing memory 24. It then removes that exact job from retired memory 24, proves memory 24 is logically empty, and opens memory 25 empty. The job is not migrated as `Creating`, `Active`, or `ManualRecoveryRequired`, and it creates no target or Relay tracking reason. The old deterministic Ledger account and its remaining 105,140,000 e8s are intentionally abandoned; no new production code reads or transfers that balance. The Ledger transaction history and retained/downloaded snapshots are not erased.

This is not a general legacy migration policy. Any other setup job or near-match traps, as does retired registry state other than an empty registry or the complete configured canonical Relay projection. The compatibility rule remains in the code so a snapshot restored during the rollback window can still upgrade directly from the pre-cutover Historian. It becomes inert after the successful first cutover because memory 25 is then initialized.

Mainnet install args enable `relay_factory_enabled = opt true`. Because `notify_relay_setup` is public and can consume historian cycles after sufficient funding, production monitoring must cover factory concurrency, child-creation cycle spend, and manual-recovery entries.

Production rollout order:

1. Deploy a maintenance frontend that prevents ordinary UI submissions. It is not the factory security boundary.
2. Record all pre-upgrade public query evidence while Historian is still running.
3. Stop Historian and wait until its status is `Stopped`; this is the authoritative factory pause and call drain.
4. Create and download a snapshot.
5. Upgrade Historian in place; do not reinstall it.
6. Start Historian.
7. Verify public state and the new setup API.
8. Perform one controlled singleton setup and one overlapping multi-target setup.
9. Verify child module hashes, running status, Fiduciary-only controllers, and independent tracking counts.
10. Retain the snapshot until acceptance is complete, then restore normal UI access.

If the upgrade or one-time cutover gate fails, do not proceed. Restore the snapshot according to the rehearsed rollback procedure and prove the restored canister is queryable before rescheduling the cutover.

For later Historian upgrades:

1. Deploy the maintenance frontend.
2. Record public state and active mappings for known exact target sets.
3. Stop Historian and wait until its status is `Stopped`.
4. Create and download a snapshot.
5. Upgrade Historian in place.
6. Start Historian.
7. Verify active mappings through the known exact target sets; production cannot enumerate every target-set hash.
8. Verify `RelayTarget` and `RelayInstance` counts.
9. Verify any interrupted post-spend setup is `ManualRecoveryRequired`.
10. Re-enable normal UI access.

The post-upgrade rule remains: `Reserved`/`ProbingTargets` entries are removed; every later `Creating` phase becomes `ManualRecoveryRequired`; `Active` and existing `ManualRecoveryRequired` entries are preserved.
