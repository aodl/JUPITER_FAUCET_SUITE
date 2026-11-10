# Jupiter SNS Rewards

`jupiter-sns-rewards` maintains the current SNS-neuron owner index used by Relay reward attribution. It receives the Disburser age-bonus flow. Explicitly activated programme modules add two-source discovery, direct patron rewards, reserve custody, proposer maintenance, governance mirroring and participation collection. IO settlement/distribution and complete Relay patron attribution remain integration work.

Production canister: `jupiter_sns_rewards` / `alk7f-5aaaa-aaaar-qb4ra-cai`.

## Owner snapshots

`reward_sns_root_canister_id` is the legacy owner-index binding. OpenChat Root `3e3x2-xyaaa-aaaaq-aaalq-cai` is the existing production configuration. This work does not retarget it. Programme activation uses explicit reviewed jUP/IO Roots. At the start of every complete scan, the canister calls Root `list_sns_canisters` and pins the returned Root, Governance, and Ledger IDs. Governance and Ledger IDs are Root-resolved and are never independently configured.

The separate [acceptance environment](../../deploy/acceptance/README.md) generates
fresh arguments using its own jUP Root and SNS Rewards identity. Its real-SNS
PocketIC case uses no OpenChat components. It does not retarget the existing
production installation file or production owner snapshots.

A daily scan crawls Governance `list_neurons` in bounded 100-neuron pages, processing one page per timer message. The next daily opportunity is a one-shot timer derived from the durable accepted scan-start timestamp, so timer phase cannot skip a due scan for another day. Root/component resolution must succeed before an attempt records that timestamp; a failed start is retried without consuming the daily cadence. Progress and the exclusive neuron cursor are stable and resume after upgrade. Slot A and slot B stable maps provide active/staging publication: a refresh writes only the inactive map, and the metadata cell exposes it only after the complete scan succeeds. Failed or partial refreshes leave the preceding active snapshot public.

A neuron qualifies when `cached_neuron_stake_e8s.saturating_sub(neuron_fees_e8s) > 0`. Its owner is the principal with the most distinct permission codes after entries for the same principal are unioned. A tie uses the lexicographically smallest raw principal bytes. This deterministic heuristic is an attribution policy, not a claim that every unusual multi-principal permission arrangement has one objectively correct owner.

Each selected principal is indexed once under its default ICP AccountIdentifier (`subaccount = null`, the all-zero subaccount). Explicit ICP subaccounts are not indexed.

## Relay API

- `get_relay_reward_context` returns the active snapshot's Root, Governance, Ledger, snapshot ID, and scan timestamps. It returns `null` until a complete snapshot exists for the configured Root.
- `resolve_default_icp_accounts` resolves at most 128 ordered 32-byte account identifiers for an exact active snapshot ID. It never reads staging data or exposes neuron details.

There is no registration API, owner-list API, production force-scan method or public arbitrary reserve-transfer method. Programme queries expose compact status and evidence; programme payments are autonomous and explicitly configured.

Relay uses this conservative snapshot only to decide attribution. The resulting SNS
reward distributions are a separate perpetual best-effort service: an individual
transfer may be abandoned after bounded duplicate-safe attempts when its outcome
cannot be determined. Such a discrepancy is not an indefinite recovery obligation
and cannot halt later distributions. Fixed splitter transfers and irreversible
canister-creation/controller-removal operations retain stronger durable semantics
because their finite economics and security boundaries differ.

## Stable lifecycle

Configuration, active snapshot metadata, scan cursor, and both owner maps are stable. An ordinary no-argument upgrade preserves them and resumes an incomplete scan. `post_upgrade` accepts `Option<UpgradeArgs>`:

- no argument, outer `null`, or `reward_sns_root_canister_id = null`: preserve the Root;
- `reward_sns_root_canister_id = opt null`: clear the Root;
- `reward_sns_root_canister_id = opt opt principal "..."`: replace the Root.

Clearing or changing Root immediately clears both maps, the active snapshot, and any in-progress scan. Ownership from OpenChat therefore cannot remain active after switching to jUP.

Fresh install arguments are checked in at [`mainnet-install-args.did`](mainnet-install-args.did). During the 2026-09-11 rollout, the former empty production placeholder was configured with the temporary OpenChat Root using a reviewed nested upgrade argument. That first configuration is complete; OpenChat remains the development configuration until the future switch to the reviewed jUP SNS Root; see [deployment operations](../../docs/operations/deployment.md).

## Operations

Build with `./tools/scripts/build-canister jupiter-sns-rewards`. The canister emits its effective configuration immediately after init or upgrade and every 24 hours in this explicit form:

```text
SNS_REWARDS_CONFIG reward_sns_root_canister_id=<principal-or-null>
```

The periodic configuration record reads the current runtime state and is emitted independently of scan success, including while Root resolution fails continuously. Its interval timer is observability-only: it never invokes `scan_tick`, changes scan due-ness, or mutates snapshot, cursor, or accepted-start state. Owner scanning remains on the durable accepted-start-derived one-shot scheduler. `SNS_REWARD_SCAN` records continue to show the Root and Root-resolved Governance and Ledger pinned for each accepted scan.

Observe `SNS_REWARD_SCAN`, `SNS_REWARDS_CONFIG`, and lifecycle logs. Before relying on a snapshot, verify the context Root and Root-resolved Ledger, the completion timestamp, and that no scan failure followed the publication log.

Optional OpenChat development verification is read-only: query OpenChat Root, confirm it resolves Root `3e3x2-xyaaa-aaaaq-aaalq-cai`, Governance `2jvtu-yqaaa-aaaaq-aaama-cai`, and CHAT Ledger `2ouva-viaaa-aaaaq-aaamq-cai`; observe a complete snapshot; then query the context. Any real CHAT transfer belongs only in a separately reviewed, controlled deployment with an explicitly reviewed amount. Do not use an immutable production Relay for the first real-value smoke test without reviewing its bounded ambiguity policy.

## Explicit programme activation

`InitArgs.programme` and `UpgradeArgs.programme` are optional. Omission preserves
existing work and activates no new transfers. Legacy stable memories 0–2 remain
unchanged; programme metadata, two owner slots, payment journals, mirror mappings,
participation slots, reserve/treasury and proposer evidence use independent memory
IDs 3–21. No speculative legacy migration chain or production-state reset is used.
An increasing configuration version is required for changes. In-flight batches,
unknown financial work and pending governance operations fence reconfiguration.
The jUP Root, protocol governance neuron, reserve Account/transport and canonical
ICP Ledger/Index/Faucet staking Account cannot be retargeted within a programme.
Unknown reserve disbursements retain their complete source, destination, amount
and fee context for canonical review. Proposer readiness gates new submissions;
existing bound ballots and treasury receipt checks continue independently.

The economic source is [`economics.json`](../../deploy/acceptance/economics.json).
Production and acceptance builds reject patron or native treasury activation while
Relay/splitter and mixed-capital attribution are incomplete. The existing
`debug_api` build is the explicit local payment-test context; it is not a launch
configuration. The acceptance role mapping records proposed assignments; final
operator confirmation, live launch timestamp and NNX authority remain unresolved.

### Ownership and final eligibility

Both configured authentic Roots must resolve valid component sets. Required-source
failure defers processing; it is not an empty set or final ineligibility. A snapshot
is published only after complete scans of both SNSs, with source identities,
configuration version, generation and timestamps. The union deduplicates owners
and derives their exact default ICP AccountIdentifiers; addresses are matched
through known principals, never reverse-decoded. This union is used only for
patron processing. Relay's existing API uses the independent stable single-jUP
index in memories 0–2 with its existing conservative ownership policy. Programme
configuration binds that index to jUP Root without depending on IO resolution.
IO-only owners never enter that Relay index. Daily Relay refreshes continue while
patron history pins its separate union slot. Both scanners allocate from one
monotonic snapshot-ID namespace; neither API exposes staging data.

Programme economic attribution requires positive effective stake and a principal
with Disburse permission. Among economic controllers, union distinct permissions,
then choose the largest set and lexicographically smallest raw principal on ties.
This documented deterministic policy cannot make unusual shared-control neurons
have one objectively unique owner. An unrelated Vote-only principal is excluded.
The single-source Relay policy above applies independently, including when a
programme is configured. Unconfigured OpenChat installations retain their Root.

Each terminal patron record retains compact source/completeness metadata and its
selected owner attribution; rotating owner maps does not erase its review context.
Mirror records retain their actual source SNS and configuration version. No raw
neuron scan is archived indefinitely.

A newly selected batch requires a complete snapshot no older than the configured
1–7 day validity period, with unchanged version/Roots. A refresh does not invalidate
a still-fresh active snapshot. A historical batch freezes a canonical Ledger end
block and one generation until completion; subsequent finite batches do the same.
Pinned batches deliberately retain that processing cohort, even across long
restarts. This does not reconstruct ownership at each historical transaction.
Slots are not reused while pinned. Missing source, malformed pages or incomplete
Index synchronization cannot classify or advance contributions. Exhaustion requires
the canonical oldest anchor, not an arbitrarily short page or recent cutoff.

A recognised principal must use its exact default ICP Account and wait for
recognition before contributing. Absence from the complete pinned snapshot is a
final skip for that contribution. Later ownership does not reopen it. Historical
recognition uses the launch processing snapshot. Later contributions can use later
snapshots. There is no unmatched-claims registry or per-user accumulated balance.

### Direct patron rewards

A direct ICRC-2 transfer is attributed to the authentic Ledger source Account
owner, rather than its authorised spender. Qualifying net external ICP becomes
Faucet stake once, with the canonical block's
commitment timestamp and reviewed memo/minimum rules. Protocol Accounts are
explicitly excluded. Direct rewards use initial 10 ICP = 1 jUP and daily exponential
halving over 1,461 days, including historical pre-launch contributions at the
initial rate. Processing time does not affect the quote. Checked integer Q96
exponentiation uses the launch programme timestamp, full halvings and a bounded
fractional-day exponent; the Python 110-digit Decimal oracle covers every day of
a halving cycle and large values. The maximum tested rounding deficit is below
one e8s unit. The 2x multiplier is applied before the final floor. Only a final
zero amount is dust; there is no one-jUP minimum or threshold release.

Liquid jUP to the recognised owner's default Account is normal. Direct rewards
can compound at 2x only into an economically controlled jUP neuron with positive
stake, non-dissolving exact one-year delay and native `auto_stake_maturity = true`.
IO-only ownership supplies no jUP destination or denominator weight. Protocol
neurons are excluded. Multiple eligible destinations choose the smallest neuron
ID. Eligibility is rechecked before freezing the transfer; this is a point-in-time
check, not a promise that the neuron stays locked forever. Native ClaimOrRefresh
by neuron ID works without granting the reserve arbitrary neuron permissions.
A refresh failure never repeats a token transfer. A separate durable refresh
journal pins the exact patron record, Governance and originally selected neuron.
Both Verified and AssumedPaidUnverified compounding records enter this lane;
refreshing stake never converts an assumed payment into a verified transfer.
Each tick admits at most one refresh attempt, with five attempts separated by
five minutes. It first compares that neuron's cached stake with its actual Ledger
staking balance. Typed permanent refusals or exhausted retries stop visibly for
manual review; only the pinned jUP Governance can authorize another bounded window
through `retry_patron_refresh`. Permission/preference changes never select a new
neuron. Old settled records lacking a confirmed refresh are enqueued through a
bounded migration; all payment identity fields and totals remain unchanged.

The quoted amount reaches its recipient. SNS Rewards pays the actual Ledger fee
in addition: initially a 1/2 jUP quote costs the reserve 1.1/2.1 jUP. A later fee
change cannot reduce the quote. Splitting into many nonzero rewards increases
sponsored fees without limit proportional to the number of contributions. The
unit test demonstrates 100 million one-ICP contributions after 23 halvings consuming
100 million fees, versus one fee for the same aggregate ICP. No hidden minimum or
reserve-size argument removes this accepted economic cost.

**Relay integration is incomplete:** canonical Relay subaccount-1 commitments pause assessment
pending exact FIFO/splitter/multi-hop provenance integration. The intended route
is base-rate liquid to the original recognised contributor, never 2x or the Relay
principal. Production patron and treasury activation are rejected until mixed Relay
batches can exclude protocol capital while preserving ordinary patrons.
Other Relay-child and splitter routes are not recognised by this guard; keep
general patron activation off until their attribution is integrated. The
production install/upgrade validator enforces this even for direct-only settings. Existing
Relay pot distribution is a distinct service and its retry/fee policy is unchanged.

### Durable one-attempt patron policy

Each payment freezes contribution reference, snapshot, formula inputs, exact
amount/destination/source, actual fee, memo, created-at time and dispatch evidence.
A checkpoint is persisted before awaiting the Ledger. Missing readiness or funds
leaves work resumable. A local SDK failure known to precede dispatch remains
Prepared; it is not represented as paid. Confirmed success or an authentic exact
Duplicate records the real block once. Definitive Ledger rejection is Failed.
An uncertain dispatched call or surviving dispatched checkpoint after upgrade is
terminal `AssumedPaidUnverified`, consumes the automatic opportunity and permits
later work. It has no invented block and contributes nothing to verified totals.

The uncertain call may actually have paid or may have paid nothing. Remaining
real reserve inventory is not a debt or permission to retry that contribution.
Compact terminal records and the completed-history boundary prevent replay and
retain DAO evidence. There is no automatic reconciliation, compensation queue or
complaint portal. This discretionary policy does not apply to ICP conversion,
treasury, reserve transport, proposer funding, controller changes or ballots.

The dapp-held reserve has no native SNS treasury-transfer route. The shortest
supported exceptional remediation is a DAO-adopted native upgrade of SNS Rewards
through its actual Root controller, introducing a narrowly reviewed one-off repair
with a unique durable ID and duplicate-safe transfer identity. Such a repair is
not implemented or authorized here. A complaint alone cannot move reserve funds.
No arbitrary public transfer API or operator backdoor is added.

### Governance, treasury and IO

The separate operational proposer holds only SubmitProposal after native permission
narrowing. Its 20/100 jUP replenishment observes cached stake, actual Ledger balance
and canonical open/rejected charges. A confirmed transfer is refreshed separately;
unknown funding requires canonical review and is never assumed paid or replayed.
Funding evidence survives successful refresh. The powerful NNX/reserve/proposer
roles are excluded from external rewards; NNX is only a clearly named local
fixture principal until its real authority is supplied.

Mirrors bind the configured Root/Governance, designated target neuron, proposal ID
and complete official action digest. An installed Governance hash outside the
reviewed DTO schema disables automation. Native automatic proposer Yes votes are
part of the contest. Canonical Jupiter adoption instructs target Yes; canonical
rejection instructs No; open/unavailable never does. Unknown submissions reconcile
canonical proposer/action identity before another submission, and votes reconcile
the actual ballot before retry. Contradictory ballots and exhausted deadlines are
reported. Finite targets, bounded history pages and no automatic rejected-mirror
resubmission constrain fee exposure. Actual deadlines/quiet intervals govern
admission; five versus six days is not a universal guarantee.

Treasury proposes native ICP transfers to canonical Relay subaccount 1 using real
balance, fees, native value/seven-day limits and frozen proposal/transfer identity.
Each operation has a unique durable ID, frozen native proposal identity and
retained history. After the prior native transfer is conclusively verified, the
pinned jUP Governance may explicitly request a new increasing ID with the exact
predecessor through `request_treasury_operation`, without a configuration change.
Rejected, failed and ambiguous operations cannot use that continuation. No timer
creates an automatic proposal loop. `get_treasury_record` retains its per-version
query behavior; `get_treasury_operation` retains each new operation separately.
A pre-extension journal has no operation ID and preserves its original proposal
title/binding; operation ID zero archives that legacy predecessor when explicitly
continuing it. TransferVerified describes the native Ledger leg, not downstream
mixed-capital reconciliation. A real-SNS test follows the actual native credit
through real Relay code and the ICP Ledger to the exact staking Account; only
its NNS neuron lookup/refresh is a local mock. Production treasury automation
remains blocked pending complete provenance. Treasury endowments retain normal
Disburser/Faucet age-bonus economics, including the exact reviewed 95/5 split.
Protocol capital must never issue patron jUP.

Participation collection uses complete official event shares with zero native
inflation, excluding protocol neurons before the denominator and without multiplying
by stake again. It publishes a complete event or waits. Its gross quote is not a
funded IO payment. The Rust-only `io_contract` boundary double proves the exact
funded IO receipt and duplicate behavior; no ICP-to-IO worker or IO payout economics
is activated. See the [minimal IO handoff](../../deploy/acceptance/README.md#io-contract-handoff).

### Next provenance increment

Reuse the Relay owner-index/FIFO reconstruction contracts rather than copying a
scanner. Bind each canonical/child Relay to its authenticated identity and derive
funding credits from complete descending Ledger histories. Pin the available FIFO
credits at each splitter default leg, consume the matched credits across both
legs, and retain exact original contributor Accounts, amounts and commitment
references. Unresolved ancestry must remain pending, never become a personal
Relay contribution or a silent skip. Join native treasury proposal/credit evidence
to those credits as protocol capital, including mixed split jobs; exclude that
capital before patron issuance while preserving ordinary contributions. Cover
mixed FIFO funding, inter-leg deposits, child chains, fee changes, upgrade cursors
and conservation/no-replay before removing the activation guard. This increment
does not implement those attribution paths or activate IO settlement/payouts.
