# Jupiter SNS Rewards

`jupiter-sns-rewards` maintains the current SNS-neuron owner index used by Relay reward attribution. It receives the Disburser age-bonus flow. Explicitly activated programme modules add jUP-only recognition, direct patron rewards and Relay-pot funding, reserve custody, proposer maintenance, governance mirroring and participation collection. IO settlement/distribution remains separate integration work. Relay distributes its own pot using its existing approximate allocation policy.

Production canister: `jupiter_sns_rewards` / `alk7f-5aaaa-aaaar-qb4ra-cai`.

## Owner snapshots

`reward_sns_root_canister_id` is the legacy owner-index binding. OpenChat Root `3e3x2-xyaaa-aaaaq-aaalq-cai` is the existing production configuration. This work does not retarget it. Programme activation uses an explicit reviewed jUP Root; IO mirror targets retain their own bindings. At the start of every complete scan, the canister calls Root `list_sns_canisters` and pins the returned Root, Governance, and Ledger IDs. Governance and Ledger IDs are Root-resolved and are never independently configured.

The separate [acceptance environment](../../deploy/acceptance/README.md) generates
fresh arguments using its own jUP Root and SNS Rewards identity. Its real-SNS
PocketIC case uses no OpenChat components. It does not retarget the existing
production installation file or production owner snapshots.

A daily scan crawls Governance `list_neurons` in bounded 100-neuron pages, processing one page per timer message. The next daily opportunity is a one-shot timer derived from the durable accepted scan-start timestamp, so timer phase cannot skip a due scan for another day. Root/component resolution must succeed before an attempt records that timestamp; a failed start is retried without consuming the daily cadence. Progress and the exclusive neuron cursor are stable and resume after upgrade. Slot A and slot B stable maps provide active/staging publication: a refresh writes only the inactive map, and the metadata cell exposes it only after the complete scan succeeds. Failed or partial refreshes leave the preceding active snapshot public.

A neuron qualifies through positive economically owned stake and Disburse authority.
Duplicate permissions for one principal are unioned. No other principal may hold
dissolve-state, permission-management, disbursement, split or maturity authority;
unknown codes fail closed. The named, unused Unspecified value grants no authority
and is ignored, including in official sale-neuron permission lists. Vote, SubmitProposal and ManageVotingPermission
delegates are allowed under the pinned official framework. A beneficiary needs
no ManagePrincipals permission. Unsafe neurons are ignored individually, so they
cannot disqualify another securely owned neuron. Protocol-only neurons are
excluded by exact ID. Root-resolved Governance is never an ordinary recipient.

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
existing work and activates no new transfers. The deployed V1 layout in memories 0–2 is retained; incompatible owner contents
are rebuilt under the secure policy; programme metadata, three owner slots, payment journals, mirror mappings,
participation slots, reserve/treasury and proposer evidence use independent memory
IDs 3–22. No speculative legacy migration chain or production-state reset is used.
The pre-refresh-journal patron format and pre-operation-ID treasury format were
intermediate versions of this unshipped programme. Review of the supplied history
identified no live installation or independently required state using them; their
backfill and legacy-ID archive paths are removed. Compatibility with the deployed
owner-index implementation is retained through the frozen canonical V1 package
and its real production-build upgrade test in `tests/pocketic/fixtures/sns-rewards-v1/`.
An increasing configuration version is required for changes. In-flight batches,
unknown financial work and pending governance operations fence reconfiguration.
The jUP Root, protocol governance neuron, reserve Account/transport and canonical
ICP Ledger/Index/Faucet staking Account cannot be retargeted within a programme.
Unknown reserve disbursements retain their complete source, destination, amount
and fee context for canonical review. Proposer readiness gates new submissions;
existing bound ballots and treasury receipt checks continue independently.

The economic source is [`economics.json`](../../deploy/acceptance/economics.json).
Production and acceptance constructors accept explicitly configured patron and native
treasury work. Missing configuration activates neither. Launch time, reserve,
the fixed canonical ICP Ledger/Index identities, an NNS Governance staking subaccount,
complete snapshots and authority checks remain
mandatory. Acceptance role assignments and real launch/NNX inputs require their
separate operator review.

### Ownership and final eligibility

One complete jUP scan feeds direct patrons and Relay's existing resolver. Root
resolution pins Governance/Ledger, validates eight decimals and verifies that the
Ledger minting Account is Governance's default Account. IO absence cannot block
recognition or patron payments, and IO-only owners do not qualify. Three bounded
slots hold active, staging and the one actually pinned historical generation.
The active Relay-facing generation refreshes while historical patron work keeps
its original map. All publications share a monotonic snapshot-ID namespace;
staging is never visible. Upgrading the old permission-count owner index retains
its Root and generation counter but invalidates the incompatible snapshot for a
secure rebuild. An unconfigured OpenChat deployment is never retargeted.

### Deployed V1 ownership-policy transition

An upgrade with omitted Root/programme arguments preserves the temporary OpenChat
Root `3e3x2-xyaaa-aaaaq-aaalq-cai`, its resolved Governance and CHAT Ledger. It
invalidates the old permission-count publication, retains the monotonic snapshot
namespace and publishes only after a complete secure rebuild. It activates no
reserve transfers, treasury proposals or IO distribution. The frozen canonical
V1 upgrade test uses owned local mocks at the exact historical OpenChat IDs and
compares old and new resolver results; it does not query live OpenChat.

The policy deliberately changes eligibility. Previous count winners without
Disburse, including voting-only or permission-manager-only principals, become
ineligible. Neurons with a different principal holding any economic authority
(ConfigureDissolveState, ManagePrincipals, Disburse, Split or maturity rights) are
unsafe; unknown permission codes, exhausted stake and invalid beneficiaries fail
closed. Root-resolved Governance itself is excluded as an ordinary recipient.
A sole Disburse beneficiary remains eligible without any permission
manager; ordinary voting delegates remain allowed. A voting delegate that won the
old count contest loses that attribution and the secure Disburse beneficiary can
become recognised instead. Genuine transferred control can qualify after all
competing economic authority is relinquished. Rejection is per neuron: another
independently valid neuron still recognises the same principal.

Preserving Root/token identities prevents cross-SNS retargeting; it does **not**
preserve the old mistaken recipient cohort. An existing immutable Relay can use a
later owner snapshot to allocate its remaining CHAT pot. Before an actual V1
upgrade, review its bound Relays' remaining token inventory and rehearse the
old/new default-Account eligibility differences from reviewed state. Defer the
live transition while an outstanding pot has unreviewed exposure, or explicitly
accept the documented policy change through the authorised governance process.
Do not reinstall/modify controllerless Relays or preserve the unsafe count rule.
Use the normal state-preserving upgrade with omitted financial activation inputs;
review any later explicit jUP Root/programme activation separately. This task
performs no live inventory reads or deployment.

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
neuron. Refresh jobs are enqueued when payments settle.

The native launch voting policy is distinct: six months of dissolve delay admits
new votes, with up to 100% additional delay weight at one year and 200% additional
age weight after four non-dissolving years. A mature one-year neuron has 6× raw
stake voting power, three times an equal fresh one-year commitment. These are
relative native participation weights, not an issuance multiplier or income rate.
Starting dissolution resets native age. The programme consumes official event
shares once, retaining zero native jUP inflation. The complete launch parameter
comparison and conditional inventory analysis are in the
[economic review](../../deploy/acceptance/economic-review.md).

A confirmed direct transfer sends the quoted amount. SNS Rewards pays the actual Ledger fee
in addition: initially a 1/2 jUP quote costs the reserve 1.01/2.01 jUP. A later fee
change cannot reduce the quote. Splitting into many nonzero rewards increases
sponsored fees without limit proportional to the number of contributions. The
unit test demonstrates 100 million one-ICP contributions after 23 halvings consuming
100 million fees, versus one fee for the same aggregate ICP. No hidden minimum or
reserve-size argument removes this accepted economic cost.

### Reserve-funded Relay pots

An authentic Relay subaccount-1 transfer with the exact Relay forwarding memo,
configured staking destination and Faucet minimum earns a base-rate aggregate pot.
The basis is the net ICP actually credited to stake at the original Ledger block
time; reserve pays the pot plus the inbound jUP fee. The destination is that
Relay's default jUP Account. It needs no neuron of its own. A configured canonical
Relay is a trust anchor. Factory children require finalised Historian Active
membership, read in bounded revision-fenced pages; partial/unavailable evidence
defers, while a proven unsupported sender is skipped. Memo text alone cannot
establish the source or factory membership.

Direct and Relay routes share one canonical history cursor and one-attempt
journal, including archives, finite batch boundaries and lost-callback recovery.
A user who names Relay in a valid memo remains a direct patron only. Liquidity,
setup deposits and memo-less treasury staking credits do not independently earn
a pot. Payment records preserve the classification and canonical source proof.

An unresolved factory candidate is recorded as `PendingRelay` /
`AwaitingRelayMembership` in that same per-block journal before the primary cursor
advances. This includes Pending (Creating, Paused or ManualRecoveryRequired),
unavailable/malformed dependencies and incomplete membership pagination. These
are neither eligibility decisions nor payment attempts. Original canonical
source, net amount, timestamp, launch rate inputs and complete snapshot metadata
are retained; no historical owner map is pinned for a Relay intermediary.

A separate optional retry cursor/count and finite journal end use at most 32
record decodes, one end-key lookup and one factory-membership page per tick. Each pass freezes its upper block boundary
so continuous arrivals cannot postpone revisiting older entries. It advances round-robin, so a permanent
pending entry cannot prevent another child's recovery or unrelated primary
history. A fresh complete jUP snapshot with unchanged financial source bindings,
authoritative Active membership and real reserve/fee readiness are required before
freezing the payment. Pagination resumes per record and restarts on registry
revision changes. No canonical Ledger scan is duplicated. The existing one-attempt
payment checkpoint also marks a retry dispatch, so confirmed/assumed outcomes and
upgrades consume one opportunity without rewinding the primary batch.

The explicit trade-off is retained journal storage and recovery latency: there is
one patron-journal record per qualifying canonical commitment, no additional
unbounded queue/index, and no capacity threshold that drops or stalls uncertain
opportunities. Additional retry-control storage is constant size; total journal
storage remains proportional to retained commitments. A strict total-space cap
would require expiration, backpressure or canonical-history rescanning. Scanning terminal records can delay recovery as history grows;
each invocation remains bounded. Unresolved opportunities have no expiry or
invented payment entitlement. Disabling patron issuance pauses both lanes without
erasing them. Changes to a later launch setting do not rewrite retained rate inputs.

Relay's distribution implementation is unchanged. It selects eligible historical
cohorts under its existing cutoffs, fees, economical threshold and bounded retries.
Funding proves delivery to Relay, not exact individual receipt or a guaranteed
per-contribution rate. SNS Rewards implements no Relay FIFO or multi-hop allocation.

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
Funding evidence survives successful refresh. Designated protocol-voting, reserve-transport and proposer neurons are excluded
by exact ID; that does not blacklist a principal's independently valid external
neuron. Reserve/proposer IDs are also derived unconditionally from the reviewed
native SNS genesis controller (SNS Rewards) and nonces 2/3, even if optional
transport/automation settings and explicit exclusions are omitted. NNX is a labelled local fixture until its actual authority is supplied.

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

Treasury proposes native ICP transfers directly to the reviewed Disburser NNS
neuron staking Account, with a null routing memo, using real
balance, fees, native value/seven-day limits and frozen proposal/transfer identity.
Each operation has a unique durable ID, frozen native proposal identity and
retained history. After the prior native transfer is conclusively verified, the
pinned jUP Governance may explicitly request a new increasing ID with the exact
predecessor through `request_treasury_operation`, while treasury automation remains
enabled in that same configuration version. Disabled or stale requests persist
no pending operation and do not block reconfiguration.
Rejected, failed and ambiguous operations cannot use that continuation. No timer
creates an automatic proposal loop. `get_treasury_record` retains its per-version
query behavior; `get_treasury_operation` retains each new operation separately.
TransferVerified describes the direct staking Ledger credit. The Disburser's
existing ClaimOrRefresh separately updates NNS cached stake and applies the
native stake-weighted age adjustment. The real-SNS sale rehearsal binds the native
proposal to a real NNS neuron; controlled Ledger-funded maturity pots are labelled
separately from actual NNS reward generation in Disburser's integration suite.
Treasury principal retains normal Disburser/Faucet maturity economics, including
the reviewed gross 95/5 age-bonus split, and creates no patron jUP opportunity.

Participation collection uses complete official event shares with zero native
inflation, excluding protocol neurons before the denominator and without multiplying
by stake again. It publishes a complete event or waits. An upgrade invalidates
published participation quotes for a native-event rebuild under the installed
exclusion policy; stored aggregates cannot establish their own policy provenance.
Unconfigured deployed V1 retains empty participation state. A gross quote is not a
funded IO payment. The Rust-only `io_contract` boundary double proves the exact
funded IO receipt and duplicate behavior; no ICP-to-IO worker or IO payout economics
is activated. See the [minimal IO handoff](../../deploy/acceptance/README.md#io-contract-handoff).

### Treasury principal and maturity

The native ICP transfer freezes the exact configured staking Account. Before new
proposal dispatch, the public NNS neuron must match the reviewed neuron ID,
Disburser controller and staking bytes. The neuron ID is never treated as its
staking subaccount. Existing Disburser ClaimOrRefresh eventually recognises the
additional principal; Ledger receipt alone does not prove that refresh.

Memo-less treasury capital stays staked and earns no direct jUP or Relay jUP pot.
Faucet retains IgnoreBadMemo semantics and excludes it from the valid-beneficiary
denominator. Later base maturity follows normal allocations; unallocated remainder
goes to canonical Relay's default ICP Account. The age-bonus split remains 95%
SNS Rewards / 5% D-QUORUM. No treasury exception or compensating issuance exists.
