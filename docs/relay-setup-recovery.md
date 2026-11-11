# Self-service Relay configuration and recovery

Historian creates an immutable controllerless Relay from one complete configuration: 1–20 managed target canister principals and either zero or 1–5 typed surplus recipients. Each recipient is `Principal { principal; memo }` or `Neuron { neuron_id; memo }`, where `memo` is a required exact 0–32-byte blob and an empty blob means no outgoing Ledger memo. Both vectors are required by `get_relay_configuration_view` and `notify_relay_configuration`. The empty recipient vector is the sole backend representation of all-cycles mode.

Targets are sorted by raw principal bytes. Recipients are canonicalized with Principals first in raw-byte order and neurons second in ascending numeric order. A memo stays attached to its destination and does not participate in sorting. Duplicate type-and-destination pairs are rejected even when their memos differ. No IO recipient, custom recipient subaccount, weighting, or target-canister memo is added; Relay's fixed operational funding subaccounts remain available after activation.

## Canonical configuration identity

Every valid configuration uses one SHA-256 preimage:

```text
"jupiter-relay-configuration-v1\0"

0x01
target_count as u8
for each canonical target:
    target_principal_length as u8
    target_principal_bytes

0x02
recipient_count as u8
for each canonical recipient:
    Principal:
        0x01
        principal_length as u8
        principal_bytes
        memo_length as u8
        exact memo bytes
    Neuron:
        0x02
        neuron_id as unsigned big-endian u64
        memo_length as u8
        exact memo bytes
```

Zero is a valid recipient count. Every Principal and memo is length-framed, including an empty memo with length zero. No Candid, JSON, text formatting, Principal text, Unicode normalization, or browser-local hashing participates. The resulting 32 bytes are both the configuration key and the Historian-owned setup-account subaccount. The browser obtains the funding account only from Historian.

The committed vectors use one-byte test Principals written as raw bytes:

| Configuration | Preimage hex | SHA-256 |
|---|---|---|
| target `01`; zero recipients | `6a7570697465722d72656c61792d636f6e66696775726174696f6e2d763100010101010200` | `49700876356a6229a8dfa8a8379a844a5545eda72e8083b96c740fc431495c04` |
| target `01`; Principal `02`, empty memo | `6a7570697465722d72656c61792d636f6e66696775726174696f6e2d76310001010101020101010200` | `3a0ed735d092e75bcd929d71235e2b3757af7e7c50d342427592cbf1a1bb0b1f` |
| target `01`; Principal `02`, memo `00ff` | `6a7570697465722d72656c61792d636f6e66696775726174696f6e2d7631000101010102010101020200ff` | `953bb6f741d0fd96d989a53d442f9ef919ba92c5848c7b328cd2585af634087f` |
| target `01`; neuron 42, memo `55` repeated 32 times | `6a7570697465722d72656c61792d636f6e66696775726174696f6e2d76310001010101020102000000000000002a205555555555555555555555555555555555555555555555555555555555555555` | `97d68d2705438ae59b7c94b40e4d36d775b6487d28b0ffa45caaa57f98c021a1` |
| targets `01`,`02`; Principal `03` memo `10`; neuron 7 memo `2021` | `6a7570697465722d72656c61792d636f6e66696775726174696f6e2d76310001020101010202020101030110020000000000000007022021` | `184f8586f6385a1649b1980017e2537cd1948488f42a260e92abc57f803c5dca` |

Input order does not change the key after canonicalization. Empty memo differs from `[0x00]`; changing any target, recipient type, destination, neuron ID, or memo byte changes the key. Text and Hexadecimal entry that parse to identical bytes produce the same key.

## Funding and creation

Obtain configuration, setup funding account and Active mapping through the anonymous `get_relay_configuration_view` update. The verified update response is payment authority; ordinary dashboard queries are not. For a new or active configuration, the view computes locally without external calls, reservations or durable unfunded entries. Paid paused/manual entries also receive a bounded, non-spending recovery assessment through the same authenticated update. Matching backend/frontend update declarations are a coordinated deployment prerequisite. Browser payment Ledger selection is pinned by the trusted build, and legacy account identifiers are derived from the authenticated account.

Pricing is target-based:

```text
max(
  configured singleton minimum,
  ICP needed to mint max(configured initial cycles, supported provisioning floor)
  + configured conversion safety margin
  + configured Relay subaccount-one seed including its retained safety margin
  + setup-to-CMC ledger fee
  + setup-to-Relay ledger fee
)
+ 0.25 ICP * (target count - 1)
```

The conversion transfer includes the configured safety margin, so it buys additional cycles against a falling conversion rate. The final seed separately retains the same margin; both amounts and both Ledger fees are covered by the setup deposit. Known minted cycles are journaled even when insufficient for creation. A proven deficit pauses the paid setup and releases its executable Creating slot.

Funding authority is the aggregate `icrc1_balance_of` value for the authoritative setup account. Historian does not attribute deposits to payers or automatically refund an incorrectly funded configuration.

Notification rereads the setup balance, Ledger fee, and CMC rate before Governance work. Underfunded calls do not reserve an entry, resolve a neuron, probe a target, or spend funds. A funded exact key enters `Reserved`; same-key callers observe the existing phase, and at most four distinct funded configurations may be creating concurrently. This is a total unresolved `Creating`-state circuit breaker: `RelayFunded` and `FinalizationAttempted` entries continue to consume capacity until activation replaces them with `Active`. Public neuron recipients must resolve through NNS Governance before targets are probed. Zero-recipient and Principal-only configurations make no Governance request.

Every target must pass the shared Auto cycles probe before irreversible work. Auto prefers protocol-native direct `canister_status`. Preflight uses the `AnyCanister` requirement: a direct observation is reusable by the future Relay only when status visibility is exactly `public`; Historian controller access or a Historian-only `allowed_viewers` entry is insufficient. A successful non-public response still provides authoritative controllers, allowing a recognized reusable proxy to qualify the target without another metadata call. Fresh compatibility discovery calls only production blackholes that control the target, canonical NNS Root when it controls the target, and SNS Root/Swap routes authenticated through SNS-W and membership validation. NNS Root may also observe itself. Arbitrary controllers are never called as status proxies. Successful fallback routes are cached only in heap, behind the direct attempt, and stale routes rediscover after failure. The state machine journals the prepared CMC transfer, accepted block, cycles minted, child-create dispatch fence and returned ID, prepared Relay-funding transfer, accepted block, phase, and bounded diagnostic information. Clean pre-spend failures remove the reservation. Normal setup notifications also examine at most the four Creating keys and reclaim journal-empty `Reserved`/`ProbingTargets` entries with no live transient attempt. This permits same-key retries and unrelated setups after a pre-spend callback interruption without requiring an upgrade. Cleanup is fenced by the existing admission attempt token, so an old callback cannot clear a replacement at the same phase. Prepared or dispatched operations are never reclaimed by this path. Unknown original transfer or creation outcomes remain `ManualRecoveryRequired`. An installation deficit or an insufficient final reserve before transfer dispatch can pause for user-funded continuation. Other failures remain fail-closed; approved installed code can be reconciled without another installation.

Admission precedes the first balance/fee/rate, neuron or probe call. New preflight is limited to four outstanding attempts and eight attempts per aligned 60-second window. Paid CMC reconciliation and already-funded finalization share a protected two-slot/eight-attempt allowance, separate from new setup traffic. Recovery assessments have a separate two-slot/eight-attempt read allowance, so polling cannot consume continuation permission. Same-key work is single-flight across all three lanes; completed reads and structurally unsafe manual entries are cheap. Failed work still consumes its window token. Transient memory holds at most eight keys, never all unfunded keys or callers. Pre-spend work is fenced by a five-minute lease and a 512-call ceiling shared with nested probe calls. Admission is checked in the same message immediately before CMC-transfer preparation/dispatch. Once dispatched, the durable journal and bounded Creating index govern the paid tail; each callback must still match its expected phase. A delayed transfer/CMC/create/install/funding reply is classified normally, not replaced with `Busy` because the transient lease expired. Ambiguous original irreversible operations retain manual-recovery evidence and release their Creating slot. Supplemental conversions use their separately persisted idempotency identity; they are never replaced merely because a reply was lost. Admission failure in finalization leaves only the existing safely resumable funded phases. A stale pre-spend response cannot reserve, spend or clear a successor's guard. Capacity/refill permits retries, but an unlimited Sybil flood has no new-caller fairness guarantee.

## Provisioning budget

Fresh-install defaults and the production constructor set `relay_initial_cycles = 5_000_000_000_000`. Persisted configuration changes only through supported upgrade arguments. The existing paid-Creating configuration guard remains in force; drain executing work before changing that setting.

The small budget policy uses `ic0.cost_create_canister` for the actual local subnet charge:

```text
creation allocation = max(configured initial cycles, 3 * creation charge + 1 trillion)
empty-child continuation target = creation allocation - creation charge
```

The conversion requirement and the cycles attached to `create_canister` use the same creation allocation. The 0.05 ICP conversion margin buys exchange-rate protection for Historian; it is separate from the child allocation's installation headroom. The final Relay seed retains its own margin and target surcharge. Each transfer pays a separately quoted Ledger fee, and cycles-to-e8s division rounds up.

Supported deployment conditions are 13-34-node application/fiduciary pricing, zero compute and reserved-memory allocations, a freezing threshold no greater than 30 days and at most 256 MiB of initialized Relay memory. Recovery checks the live allocation/freezing/memory settings before authorizing a payment. A creation charge outside the 13-34-node bounds rejects the budget assessment before spending. Subnet capacity and exceptional storage-reservation pressure remain external constraints; no fixed budget guarantees installation on a full subnet.

The [official replica configuration](https://github.com/dfinity/ic/blob/d3b3351fa343a893aea2cfa8d39ac6b0d507b477/rs/config/src/subnet_config.rs) sets a 500 billion creation charge at 13 nodes and a 300 billion instruction install limit. The [live-subnet revision's cost configuration](https://github.com/dfinity/ic/blob/83f29e0803496ba6aaea3d6b43f6c6bebea12dd9/rs/config/src/subnet_config.rs) confirms those values, one cycle per Wasm32 instruction, 317,500 cycles/GiB/second storage pricing and a 10,000-cycles/second base fee before subnet scaling. [Subnet costs scale with node count](https://docs.internetcomputer.org/guides/canister-management/subnet-selection/). At 34 nodes:

- Creation consumes 1,307,692,307,692 cycles.
- The maximum upfront instruction reservation is about 784.7 billion, even when successful installation ultimately burns less.
- Thirty days of storage freezing reserve for the 256 MiB bound is about 538.1 billion, plus about 67.8 billion for the base fee.
- A separate 300 billion initialization/first-call allowance and one trillion operating allowance bring the conservative child requirement below 2.691 trillion, leaving further headroom within the policy floor's 3.615 trillion child balance. At 13 nodes the floor leaves 2 trillion, exceeding the corresponding roughly 1.832 trillion requirement. The policy test checks every node count in the supported range.
- The configured 5 trillion allocation leaves about 3.692 trillion after creation. The former 2 trillion allocation left only 692,307,692,308. The live revision's 300 billion instruction limit plus 5 million execution fee scale to 784,628,461,538. The observed empty child's daily idle cost of 2,260,528,197 gives a 30-day freezing reserve of 67,815,845,910. Their sum minus the original post-creation balance is exactly the reported 160,136,615,140 deficit. This reconstructs the failed attempt; it is neither a current funding quote nor the cycles ultimately burned by installation.

Representative PocketIC regressions use the 34-node fiduciary cost schedule, the approved production payload, default allocation/freezing settings and real ICP Ledger/CMC. They retain normal instruction limits and do not add child cycles to the successful fresh bootstrap. PocketIC does not establish live subnet saturation or future replica pricing, and its pinned version can differ from the live baseline-fee implementation; the policy test explicitly includes the live 10,000-cycles/second base fee; the budget rule is covered directly and unsupported settings fail closed.

## Checking and continuing a paid setup

Reopen the exact targets, recipient types and memo bytes. `get_relay_configuration_view` returns optional `recovery` information in its authenticated response:

- `FundingRequired` supplies the original setup account, recorded child ID, current setup balance, required remaining balance, cycles deficit and **additional e8s that must arrive**.
- `Ready` allows explicit continuation with no additional deposit. Existing eligible funds and independent child cycles top-ups are credited first.
- `CheckPending` permits another bounded non-spending check. No payment instructions are exposed while status, balances, fees or rate are unavailable.
- `Unsafe` retains the do-not-pay warning. Unknown identity, unexpected code/controllers, contradictory evidence and exhausted financial reconciliation do not become funding requests.

The user adds only the displayed additional amount to the original account, then selects **Continue setup**. A wallet may charge its own sending fee in addition to that amount. Polling and rechecking never call notification. An explicit recovery assessment overrides phase-based resume rules; only Ready permits continuation. Re-check remains locked through continuation and its follow-up read. Continuation rereads live child state, balance, rate and fees. It never charges the original minimum or target surcharge again. A funding request includes the configured exchange margin. Existing funds that already purchase the entire conservative child budget can use the available portion of that margin, so a small idle-cycle change after an exact deposit does not demand another dust payment. The promised final seed and its margin remain reserved in full.

The browser locks continuation, editing, rechecking and setup-account copying while a creation/continuation request or its follow-up assessment is pending. During a separate non-spending assessment, it hides payment instructions and locks copying, rechecking and continuation; editing can discard the check without allowing a late response to restore stale instructions. Payment instructions return only after a successful read. `CheckPending` never authorizes funding or continuation: the message promises an automatic check only while polling is scheduled; otherwise it offers the enabled **Re-check this setup** action. `Unsafe` requires review and prohibits further spending. Polling performs assessments only and stops on activation. Backend diagnostics are operator evidence, not user instructions.


An empty, running child controlled exactly by Historian can receive an installation attempt only when the paid journal and under-budget creation/funding evidence justify it. Each explicit continuation dispatches at most one installation. Another authoritative funding rejection can pause that same child again; a new attempt requires reconciliation and funding of its observed deficit. Unknown or non-funding installation outcomes never authorize another empty-child installation. The child comes only from the journal. Matching approved code skips installation; exact installed/funded/finalized state continues at the unfinished stage. An absent module or rejection code 2 alone is insufficient. A further non-funding install rejection fails closed, retaining diagnostics.

Additional cycles use a separate setup-to-CMC transfer to the backend-derived deposit subaccount for the recorded child. A pre-create minting deficit instead targets Historian and credits only that setup's recorded conversions before its single child creation. Both routes preserve the promised final seed plus its fee. The immutable operation records amount, fee, created-at timestamp, accepted block, exact CMC destination account, exact memo bytes, creation/child binding and minted result. New operation timestamps advance beyond every prior setup conversion timestamp; an accepted block already credited to another operation fails closed. The key, pinned Ledger/CMC and fixed CMC memo bind the source and other identity fields. At most four supplemental financial conversions are retained. Definitive first-dispatch no-spend rejections may be compacted before a newly quoted operation is prepared; accepted, minted and uncertain identities remain intact. Retry counters saturate as diagnostics and cannot turn a safely retryable operation into an unsafe one. Ledger transport uncertainty, known no-spend rejections and retryable CMC replies impose a durable 30-second backoff. Per-request work and the admission windows remain bounded.

A lost Ledger reply retries exactly the same operation and accepts `Duplicate` as the same block. The retry window is conservatively limited to 23 hours within ICP Ledger's 24-hour deduplication window. It never refreshes an uncertain timestamp. Rejection after an earlier uncertain dispatch cannot authorize a replacement operation. Accepted CMC blocks are notified against the same destination/block through Processing or transport uncertainty; refunded, invalid or pruned results stop. An outstanding operation is reconciled before even an independent child top-up can skip ahead. No raw cycles-forwarding retry or unrelated setup subsidy is introduced.

## Child configuration and surplus

The child receives canonical targets and typed recipients with exact memo bytes. Principal recipients use their default ICP accounts. Neuron recipients use the NNS Governance staking account resolved from the neuron ID, and Relay requests `claim_or_refresh` after an accepted transfer. Relay independently enforces destination and memo policy.

The browser treats Hexadecimal as authoritative for arbitrary bytes. A switch to Text succeeds only after assigning the candidate to the actual input and proving an exact byte round trip. Failed conversion retains the original Hexadecimal mode and string, remains submit-able, and produces a non-blocking status notice. Canonical display always contains exact lowercase Hexadecimal. Optional text is shown only after fatal UTF-8 decode, exact re-encode, and rejection of Unicode category `C`, `Zl`, `Zp`, and entirely invisible or whitespace-only text.

Zero recipients installs `surplus_canister_recipients = null` and an empty neuron-recipient vector. The child then uses all-cycles allocation: positive measured needs receive fee-efficient top-ups; funds remain when no need is positive or a share is fee-inefficient; no raw-ICP recipient transfer is produced. Routing mode requires at least one recipient. New children use `max_transfers_per_tick = 50` in either mode, with headroom above the maximum 26 planned payments. Bounded retries reuse the same payment identity; an allocation exceeding its cap retains unpaid work for a later invocation. Existing children retain their installed configuration.

Historian creates the child with `[Historian]` as its temporary controller plus public logs and public status. Before finalization it directly audits the running state, approved module hash, exact `[Historian]` controller set, public logs, and public status. It then journals `FinalizationAttempted` and performs one settings update to empty controllers while explicitly retaining public logs and public status. Whether that update reports success or error, Historian directly reads live status again. Activation requires the running approved module with exactly zero controllers and public log/status visibility; public status is what permits this audit after Historian has lost control. A correct live state reconciles an ambiguous update result. An unexpected live state becomes `ManualRecoveryRequired`.

The frontend's **Resume creation** action explicitly reconciles `CmcTransferAccepted` or continues `CmcNotifySucceeded` under the bounded completion lane. A lost reply or retryable CMC result retains the same paid block; terminal/refunded/pruned results require manual recovery. This action makes no additional conversion transfer. Polling remains a setup-view read and never invokes notification automatically.

A transient management-status read failure in `RelayFunded` or `FinalizationAttempted` instead leaves the entry `InProgress`. The frontend's **Resume finalization** action submits the same canonical configuration and drives only this already-funded tail; interval polling uses the bounded setup-view update only, never notification or creation. Resumption never repeats a Ledger transfer, CMC notification, child creation, code installation, or Relay-funding transfer. Exact finalized state activates without another settings update. Exact running approved state with public log/status visibility and controllers exactly `[Historian]` permits one idempotent controller-removal settings attempt per explicit resume. If the post-update read still shows that exact safe state, or the read itself fails, the entry remains resumable rather than guessing the mutation outcome. Successful activation uses one shared path to store the active child mapping and its completed journal and record generic `RelayTarget` and `RelayInstance` tracking exactly once.

## Upgrade and manual recovery

Historian persists one Relay setup map keyed by the 32-byte canonical configuration hash:

| Purpose | Memory ID | Key | Value |
|---|---:|---|---|
| Relay setup entries | 26 | `RelaySetupKey([u8; 32])` | current `RelaySetupEntry` |
| In-flight creation index | 30 | one cell | at most four Creating keys |
| Permanently reserved | 31 | — | retired installation-template cell; never reused or rewritten |

An absent memory-30 index is initialized only over an empty factory registry. A nonempty registry without its index fails closed. Entry and bounded-index transitions commit atomically; both memories must be preserved together. Canonical indexing/payout history is authoritative and cannot be reset as part of factory maintenance.

Normal upgrades preserve current entries and both memories. Journal-empty reservations can be removed. Original accepted CMC blocks remain reconcilable. Unknown create outcomes and uncertain final transfers remain manual. New paid entries pin exact Candid initialization bytes, the install-payload hash, Ledger/CMC identities, the final funding reserve and conversion margin. The persisted continuation and completed-journal shapes remain unchanged; their exact identity bytes and financial evidence survive upgrades. An unfinished installation requires its pinned payload to remain available; it cannot spend toward installing replacement bytes. An already-installed child can finish against its recorded module hash even when the embedded factory template differs. Paused entries do not occupy memory 30; an executing continuation enters the same bounded Creating index. Supplemental operations interrupted by a drained upgrade become paused and retain every transfer identity and accepted block. An installation attempt is reconciled against child status. A proven funding rejection can pause for another bounded supplemental conversion; an uncertain installation outcome never authorizes a blind retry.

Activation retains the completed journal as an optional field alongside the existing active child mapping. The optional field remains part of the stable format; an absent completed journal never authorizes spending. Original conversion and final funding records, supplemental identities and accepted results remain available in durable state after completion; they do not authorize further spending. The bounded value remains within the existing 4,096-byte stable-map limit.

Mainnet install args enable `relay_factory_enabled = opt true`. Because `notify_relay_configuration` is public and can consume historian cycles after sufficient funding, monitoring must cover factory concurrency, child-creation cycle spend, and manual-recovery entries.

## Deployment sequence

Deploy matching Historian and frontend builds in the same maintenance window because their current Candid declarations must agree.

1. Record public state, every relevant `RelayInstance` and `RelayTarget` page, and representative known exact configuration mappings.
2. Stop Historian and wait for `Stopped` so factory calls drain.
3. For a rollback-sensitive release, create and record a snapshot according to the risk-based policy in [`operations/deployment.md`](operations/deployment.md).
4. Upgrade Historian in place; do not reinstall.
5. While Historian remains stopped, deploy a matching frontend when its Candid declaration changes with Historian.
6. Start Historian after the required upgrades succeed.
7. Verify memory-26 setup state, known active mappings, `RelayTarget`/`RelayInstance` tracking, public state, canonical hashes/accounts, and any interrupted setup state.

Future upgrades may begin with active, recoverable, and tracked self-service Relay state. That state is expected and must be preserved.

## Incident records and recovery rehearsal

Release operators retain known configurations and rehearse their recovery. The installed
factory enablement, existing setup inventory, and retention process require
operator verification; checked-in fresh-install arguments do not prove live
configuration or absence of paid jobs.

The setup controller keeps its configuration and notification result in browser
memory; it does not provide an independent operator archive. For a known,
installed child, retained public Relay `CONFIG` logs can supply
`managed_canisters`, typed `surplus_recipients` and aligned
`surplus_recipient_memos` (`hex:...`, or `none` for an empty memo).
Use the configured target list, not `effective_managed_canisters`, which also
includes Relay self. Activated children are tracked as `RelayInstance`s. These
are useful reconstruction sources when present, but bounded logs can be evicted,
and an interrupted setup before child installation/activation need not have
such a record. They are not a complete incident inventory.

Retain the Historian principal/network, exact target principals, every recipient's
kind and principal or numeric neuron ID, and **raw memo bytes in hexadecimal**.
Record the authoritative setup-view response (including setup subaccount/key),
deposit Ledger block IDs, notification result/phase, returned child ID if any,
and observation times. Keep the canonical configuration independently of the
original browser/session; screenshots or rendered memo text alone are inadequate.
No new receipt or enumeration service is implied by this procedure.

For a concrete known-configuration rehearsal:

1. Start from an independently retained configuration record, not browser state.
   Reconstruct `RelaySetupArgs` with both vectors and decode each memo hex string
   into bytes. Preserve an empty memo as an empty blob, not a missing recipient.
2. Check canonicalization against the committed vector above: target raw `01`,
   Principal recipient raw `02`, memo `00ff` yields setup key
   `953bb6f741d0fd96d989a53d442f9ef919ba92c5848c7b328cd2585af634087f`.
   This synthetic vector verifies byte reconstruction, not a funded live incident.
3. For the actual incident, call `get_relay_configuration_view` with the recovered
   exact configuration on the recorded Historian. Compare its setup subaccount
   with the retained key and Ledger deposit destination. Do not call notification
   as a discovery probe. A key alone cannot reconstruct its hashed configuration.
4. Correlate the returned phase and child ID with retained transfer block IDs,
   Ledger debits/fees, CMC notification evidence and management creation/install/
   controller evidence. Distinguish observed facts from missing replies. In
   particular, an unknown create result is not proof that no child was created.
5. Use only the documented explicit resumable phases or authenticated
   `Ready`/`FundingRequired` recovery eligibility. A `ManualRecoveryRequired`
   entry without that eligibility still requires a separately reviewed operator
   decision; never repeat a conversion transfer, create, install or funding
   operation merely because a response is missing.

Record the rehearsal's configuration source, query response, correlated evidence,
remaining unknowns and reviewer approval with release evidence. No live incident
rehearsal is established by the synthetic vector. If neither operator nor user
retained the configuration and no independent source can reconstruct it,
production offers no general discovery mechanism. Operators must accept this
recovery limitation and retain configurations independently; a downloadable
record alone does not solve complete record loss.

## Completed recovery baseline

The only predecessor-created self-service setup has completed recovery and is active: Historian `j5gs6-uiaaa-aaaar-qb5cq-cai`, Relay `xsan5-rqaaa-aaaar-qciza-cai`, setup key `14707352000941b9a592eca1e83f9c2ae7c055924ab561f547d777434a7e3ef8`. There are no outstanding predecessor setups. The one-time deployment procedure and binary identities remain in Git history. Its completed memory-26 journal, pinned identity, original and supplemental accepted operations, active mapping and tracking remain durable upgrade inputs. Memory 31 remains reserved without being opened or modified.
