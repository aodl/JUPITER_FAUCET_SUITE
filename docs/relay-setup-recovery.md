# Self-service Relay configuration and recovery

Historian creates an immutable blackholed Relay from one complete configuration: 1–20 managed target canister principals and 1–5 typed surplus recipients combined. Each recipient is either `Principal(principal)` or `Neuron(u64)`. Both vectors are required on every `get_relay_configuration_view` and `notify_relay_configuration` call. No IO recipient, custom memo, custom subaccount, or weighting is added by the self-service path.

Targets are sorted by raw principal bytes. Typed recipients are canonicalized with Principals first in raw-byte order and Neurons second in ascending numeric order. Duplicate typed recipients are rejected; a principal may still appear once as a target and once as a surplus Principal recipient. Targets retain the protected-dependency and observability rules; recipients are payment destinations and are neither probed nor managed. The configured canonical production Relay target set remains reserved as an intentional policy that prevents a self-service duplicate regardless of supplied recipients.

The durable active relationship is only:

```text
full-configuration hash -> Relay canister ID
```

The 32-byte key is:

```text
SHA-256(
  "jupiter-relay-configuration-v1\0"
  || 0x01 || target_count_u8
  || each canonical target as principal_length_u8 || principal_bytes
  || 0x02 || recipient_count_u8
  || each canonical recipient as:
       Principal(P) => principal_length_u8 || principal_bytes
       Neuron(N)    => 0xFF || neuron_id_u64_big_endian
)
```

`0xFF` is outside the possible IC Principal byte length and is reserved solely for neuron recipients. The domain, tags, counts, target encoding, and Principal recipient encoding are unchanged, so every currently supported Principal-only configuration hash remains byte-for-byte identical. Targets and typed recipients jointly determine the Historian-owned setup subaccount. Order within either vector does not matter. The same targets with any recipient type or value change produce a different immutable Relay configuration and setup account. Old target-only hashes and setup accounts are intentionally not consulted or reused.

## Funding and creation

Pricing remains target-based. Every target after the first adds 0.25 ICP to:

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

Funding authority is the aggregate `icrc1_balance_of` value for the deterministic setup account. Historian does not scan transaction history, attribute funds to payers, accept block references, or automatically refund deposits. An incorrectly selected configuration and ICP sent to an obsolete target-only setup account are outside this migration and are not automatically discovered or refunded.

`notify_relay_configuration` rereads the setup balance, live ledger fee, and CMC rate before any Governance lookup. Zero-balance and under-current-requirement requests return the existing shortfall result without reserving an entry, resolving a neuron, probing a target, or starting irreversible work. For an economically qualified request, exact-key reservation enters the existing pre-spend `Reserved` phase and counts against the factory's global concurrent-setup limit. Historian then independently resolves every configured neuron through public NNS Governance while the reservation supplies same-key single-flight protection. After every Governance await, Historian rechecks that the same key is still authoritatively `Reserved` before continuing. A clean missing/unreadable-neuron or invalid-staking-subaccount result returns `FailedPreSpend` and removes that reservation; it performs no target probe, CMC transfer/notification, or child create/install/funding work. Only successful neuron validation transitions the entry to `ProbingTargets`. The resolved subaccount is not persisted; Relay resolves it again at runtime. This ordering uses no new stable phase, map, field, migration, or compatibility lookup. An existing full configuration is still looked up before new-setup-only target policy checks so an already-recorded configuration remains readable if policy later changes.

At most four distinct funded configurations may be in `Creating` at once. A caller for an already reserved full-configuration hash receives the existing phase without causing another external call. Every target must pass the shared Auto cycles probe before the first ledger transfer.

The irreversible workflow durably journals the information needed to prevent replay across awaits and upgrades:

- the prepared CMC transfer, its fixed timestamp, and its accepted block;
- the amount of cycles minted;
- the child-create dispatch timestamp and the optional returned Relay ID;
- the prepared Relay-funding transfer, its fixed timestamp, and its accepted block;
- the current irreversible phase and a bounded diagnostic message.

A clean rejection of the first ledger transfer is a pre-spend failure and removes the reservation. Ambiguous transfer outcomes, CMC notification errors, any child-creation error after dispatch, unreconciled installation errors, Relay-funding failures, and failed final audits may be post-spend or otherwise ambiguous; they enter `ManualRecoveryRequired`. Public `notify_relay_configuration` calls return that terminal state and never retry its external work.

## Child configuration and surplus

The child receives the canonical targets and the typed recipients split into Relay's existing fields, with every memo empty. `Principal(P)` becomes a `SurplusCanisterRecipient` paid to `Account { owner: P, subaccount: None }`. `Neuron(N)` becomes a `SurplusNeuronRecipient` paid to `Account { owner: NNS Governance, subaccount: resolved neuron staking subaccount }`; Relay attempts `claim_or_refresh` after a successful neuron transfer. A refresh failure does not undo the ledger-accepted ICP transfer. Relay's runtime resolution and fail-closed behavior remain authoritative if neuron visibility later changes. The existing equal-allocation logic applies across the mixed set. Top-up and safety gates take priority, and distributable surplus must be at least `recipient_count × (1 ICP + ledger fee)` before every recipient can receive the required minimum net share.

`max_transfers_per_tick` is `target_count + recipient_count + 1`; the final slot covers Relay self in the effective managed set. The maximum self-service configuration therefore uses 26. This cap controls only Relay's default-account allocation job. Relay's separate fixed-splitter and subaccount-1 stages do not change or consume it. The five-recipient limit belongs to Historian self-service policy, not Relay runtime.

Historian preserves the existing optional embedded defaults for SNS Rewards and ICP Index. The CMC receives only conversion ICP. After child creation and installation, Historian deliberately rereads both the live ledger fee and the setup-account balance, then transfers `balance - current ledger fee` to subaccount one owned by the spawned Relay. This second live read matters because the workflow can span enough external work for the fee or balance to change. The safety margin, extra-target charge, configured seed, and any additional balance therefore fund Relay subaccount one. The spawned Relay uses its own principal in its Faucet memo.

Before handoff, the management-canister audit requires the child to be running, to have the reviewed Relay module hash, to have exactly Historian as controller, and to expose public logs. Historian then requests exactly Fiduciary as controller while retaining public logs. The post-handoff audit uses Fiduciary's real blackhole interface and requires the child to be running, to have the same reviewed module hash, and to have exactly Fiduciary as controller. A reported `update_settings` error is reconciled by observing this actual final state: it is accepted only when the Fiduciary audit proves the intended handoff succeeded.

Successful activation replaces the entire progress record with `Active { relay_canister_id }` and then records `RelayTarget` and `RelayInstance` tracking in the same synchronous continuation, with no further await. The blackholed child is immutable and Historian never upgrades or reconstructs it.

On upgrade, `Reserved` and `ProbingTargets` entries are removed because no irreversible action occurred. Later interrupted phases become `ManualRecoveryRequired` with `HistorianUpgradeInterrupted`. Active and existing manual-recovery entries are preserved. Reconciliation makes no external call and never resumes a transfer, notification, child creation, installation, funding, or handoff.

## Manual recovery

`ManualRecoveryRequired` is intentionally terminal for public automation. For a known exact target-and-recipient configuration, the production configuration view exposes the irreversible phase, optional Relay ID, and bounded diagnostic message. Operators use that evidence together with separately reviewed ledger, CMC, management-canister, and Fiduciary state to determine whether value moved and whether a child exists or was blackholed.

Debug setup-entry listing is available only to local tests, PocketIC, and non-production debug builds. Production has no configuration-wide setup-key enumeration, so operators must retain the exact immutable configurations they need to inspect. Operator triage distinguishes `create_canister ambiguous relay ID loss` from `install_code module-hash reconciliation`. If a module hash exists but differs from the reviewed Relay hash, Historian fails closed and does not reinstall or continue automatically.

Unexpected ICP deposits, including deposits made after activation, require operator-assisted recovery. Historian does not automatically sweep or refund them, and an active configuration no longer exposes its setup account for additional funding.

## Stable-memory cutover

- Memory 24 remains the retired pre-launch setup-job area governed by the earlier registry validation.
- Memory 25 is the retired target-set-hash setup map.
- Memory 26 is the definitive full-configuration setup map. It remains the single map for Principal-only, neuron-only, and mixed keys; no new map or stable-memory migration is required.

Initialization flags for memories 25 and 26 are computed before either map is opened because opening writes a stable header. A direct upgrade from a version predating memory 25 first validates the retired registry/memory-24 cutover and initializes memory 25 empty. Before initializing memory 26, Historian requires memory 25 to contain zero entries. Any memory-25 entry traps the upgrade with a fail-closed error; it is not cleared, decoded as a new identity, or migrated. Memory 26 is then initialized empty and all normal lookup, reservation, progress, activation, recovery, debug listing, and interrupted-creation reconciliation use memory 26 only.

This rejection is justified by the confirmed pre-launch state in which self-service Relay creation was unused. It is deliberately not a compatibility path for old children or target-only addresses. All unrelated stable state remains intact, so Historian must be upgraded in place and must never be reinstalled.

Mainnet install args enable `relay_factory_enabled = opt true`. Because `notify_relay_configuration` is public and can consume historian cycles after sufficient funding, monitoring must cover factory concurrency, child-creation cycle spend, and manual-recovery entries.

## Deployment sequence

The Historian and frontend typed-recipient API update must be deployed together while Historian is stopped; the removed principal-only fields intentionally fail closed across mixed versions.

1. Record public state and known full-configuration mappings.
2. Stop Historian and wait for `Stopped` so factory calls are drained.
3. Create and download a snapshot.
4. Upgrade Historian in place; do not reinstall.
5. While Historian remains stopped, deploy the matching frontend.
6. Start Historian only after both upgrades succeed.
7. Verify the typed fields, public state, and known configurations in both input orders.
8. Verify same-target/different-type-or-value configurations produce different accounts and Relays.
9. Verify mixed child recipients, empty memos, reviewed module hash, Fiduciary-only controllers, and tracking counts.
10. Retain the snapshot until acceptance is complete.

If the stable cutover or another gate fails, restore the snapshot and prove the restored canister is queryable before rescheduling. Production cannot enumerate every configuration hash, so later upgrades should record and verify known exact configurations explicitly.
