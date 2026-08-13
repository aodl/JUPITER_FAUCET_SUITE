# Jupiter SNS Rewards

`jupiter-sns-rewards` maintains the current SNS-neuron owner index used by Relay reward attribution. It remains the recipient of the disburser's age-bonus flow, but it does not yet calculate or distribute the future general SNS rewards program.

Production canister: `jupiter_sns_rewards` / `alk7f-5aaaa-aaaar-qb4ra-cai`.

## Owner snapshots

`reward_sns_root_canister_id` is the canister's sole configured external component. OpenChat Root `3e3x2-xyaaa-aaaaq-aaalq-cai` is the temporary development configuration; it will be replaced by the jUP SNS Root after launch. At the start of every complete scan, the canister calls Root `list_sns_canisters` and pins the returned Root, Governance, and Ledger IDs. Governance and Ledger IDs are Root-resolved and are never independently configured.

A daily scan crawls Governance `list_neurons` in bounded 100-neuron pages, processing one page per timer message. The next daily opportunity is a one-shot timer derived from the durable accepted scan-start timestamp, so timer phase cannot skip a due scan for another day. Root/component resolution must succeed before an attempt records that timestamp; a failed start is retried without consuming the daily cadence. Progress and the exclusive neuron cursor are stable and resume after upgrade. Slot A and slot B stable maps provide active/staging publication: a refresh writes only the inactive map, and the metadata cell exposes it only after the complete scan succeeds. Failed or partial refreshes leave the preceding active snapshot public.

A neuron qualifies when `cached_neuron_stake_e8s.saturating_sub(neuron_fees_e8s) > 0`. Its owner is the principal with the most distinct permission codes after entries for the same principal are unioned. A tie uses the lexicographically smallest raw principal bytes. This deterministic heuristic is an attribution policy, not a claim that every unusual multi-principal permission arrangement has one objectively correct owner.

Each selected principal is indexed once under its default ICP AccountIdentifier (`subaccount = null`, the all-zero subaccount). Explicit ICP subaccounts are not indexed.

## Relay API

- `get_relay_reward_context` returns the active snapshot's Root, Governance, Ledger, snapshot ID, and scan timestamps. It returns `null` until a complete snapshot exists for the configured Root.
- `resolve_default_icp_accounts` resolves at most 128 ordered 32-byte account identifiers for an exact active snapshot ID. It never reads staging data or exposes neuron details.

There is no registration API, owner-list API, production force-scan method, or token-distribution method.

## Stable lifecycle

Configuration, active snapshot metadata, scan cursor, and both owner maps are stable. An ordinary no-argument upgrade preserves them and resumes an incomplete scan. `post_upgrade` accepts `Option<UpgradeArgs>`:

- no argument, outer `null`, or `reward_sns_root_canister_id = null`: preserve the Root;
- `reward_sns_root_canister_id = opt null`: clear the Root;
- `reward_sns_root_canister_id = opt opt principal "..."`: replace the Root.

Clearing or changing Root immediately clears both maps, the active snapshot, and any in-progress scan. Ownership from OpenChat therefore cannot remain active after switching to jUP.

Fresh install arguments are checked in at [`mainnet-install-args.did`](mainnet-install-args.did). Because the existing production principal was previously an empty placeholder, the first configuration uses a temporary nested upgrade argument rather than the fresh-install file; see [deployment operations](../../docs/operations/deployment.md).

## Operations

Build with `./tools/scripts/build-canister jupiter-sns-rewards`. The canister emits its effective configuration immediately after init or upgrade and every 24 hours in this explicit form:

```text
SNS_REWARDS_CONFIG reward_sns_root_canister_id=<principal-or-null>
```

The periodic configuration record reads the current runtime state and is emitted independently of scan success, including while Root resolution fails continuously. Its interval timer is observability-only: it never invokes `scan_tick`, changes scan due-ness, or mutates snapshot, cursor, or accepted-start state. Owner scanning remains on the durable accepted-start-derived one-shot scheduler. `SNS_REWARD_SCAN` records continue to show the Root and Root-resolved Governance and Ledger pinned for each accepted scan.

Observe `SNS_REWARD_SCAN`, `SNS_REWARDS_CONFIG`, and lifecycle logs. Before relying on a snapshot, verify the context Root and Root-resolved Ledger, the completion timestamp, and that no scan failure followed the publication log.

Optional OpenChat development verification is read-only: query OpenChat Root, confirm it resolves Root `3e3x2-xyaaa-aaaaq-aaalq-cai`, Governance `2jvtu-yqaaa-aaaaq-aaama-cai`, and CHAT Ledger `2ouva-viaaa-aaaaq-aaamq-cai`; observe a complete snapshot; then query the context. Any real CHAT transfer belongs only in a separately reviewed, controlled deployment with an explicitly reviewed amount. Do not use an immutable production Relay for the first real-value smoke test without reviewing ambiguity recovery.
