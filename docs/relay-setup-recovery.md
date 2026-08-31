# Self-service Relay configuration and recovery

Historian creates an immutable controllerless Relay from one complete configuration: 1–20 managed target canister principals and either zero or 1–5 typed surplus recipients. Each recipient is `Principal { principal; memo }` or `Neuron { neuron_id; memo }`, where `memo` is a required exact 0–32-byte blob and an empty blob means no outgoing Ledger memo. Both vectors are required by `get_relay_configuration_view` and `notify_relay_configuration`. The empty recipient vector is the sole backend representation of all-cycles mode.

Targets are sorted by raw principal bytes. Recipients are canonicalized with Principals first in raw-byte order and neurons second in ascending numeric order. A memo stays attached to its destination and does not participate in sorting. Duplicate type-and-destination pairs are rejected even when their memos differ. No IO recipient, custom subaccount, weighting, or target-canister memo is added.

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

Pricing is target-based:

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

Funding authority is the aggregate `icrc1_balance_of` value for the authoritative setup account. Historian does not attribute deposits to payers or automatically refund an incorrectly funded configuration.

Notification rereads the setup balance, Ledger fee, and CMC rate before Governance work. Underfunded calls do not reserve an entry, resolve a neuron, probe a target, or spend funds. A funded exact key enters `Reserved`; same-key callers observe the existing phase, and at most four distinct funded configurations may be creating concurrently. This is a total unresolved `Creating`-state circuit breaker: `RelayFunded` and `FinalizationAttempted` entries continue to consume capacity until activation replaces them with `Active`. Public neuron recipients must resolve through NNS Governance before targets are probed. Zero-recipient and Principal-only configurations make no Governance request.

Every target must pass the shared Auto cycles probe before irreversible work. Auto prefers protocol-native direct `canister_status`; recognized blackhole and SNS status routes remain fallbacks. Preflight uses the `AnyCanister` requirement: a direct observation is reusable by the future Relay only when status visibility is exactly `public`. Historian controller access or a Historian-only `allowed_viewers` entry is insufficient, although a recognized reusable proxy fallback may still qualify the target. The state machine journals the prepared CMC transfer, accepted block, cycles minted, child-create dispatch fence and returned ID, prepared Relay-funding transfer, accepted block, phase, and bounded diagnostic information. Clean pre-spend failures remove the reservation. Ambiguous transfer results, creation results after dispatch, install reconciliation failures, funding failures, and failed final audits become `ManualRecoveryRequired`; public notification never replays those operations.

## Child configuration and surplus

The child receives canonical targets and typed recipients with exact memo bytes. Principal recipients use their default ICP accounts. Neuron recipients use the NNS Governance staking account resolved from the neuron ID, and Relay requests `claim_or_refresh` after an accepted transfer. Relay independently enforces destination and memo policy.

The browser treats Hexadecimal as authoritative for arbitrary bytes. A switch to Text succeeds only after assigning the candidate to the actual input and proving an exact byte round trip. Failed conversion retains the original Hexadecimal mode and string, remains submit-able, and produces a non-blocking status notice. Canonical display always contains exact lowercase Hexadecimal. Optional text is shown only after fatal UTF-8 decode, exact re-encode, and rejection of Unicode category `C`, `Zl`, `Zp`, and entirely invisible or whitespace-only text.

Zero recipients installs `surplus_canister_recipients = null` and an empty neuron-recipient vector. The child then uses all-cycles allocation: positive measured needs receive fee-efficient top-ups; funds remain when no need is positive or a share is fee-inefficient; no raw-ICP recipient transfer is produced. Routing mode requires at least one recipient. `max_transfers_per_tick` is `target_count + recipient_count + 1`, so all-cycles mode uses `target_count + 1`.

Historian creates the child with `[Historian]` as its temporary controller plus public logs and public status. Before finalization it directly audits the running state, approved module hash, exact `[Historian]` controller set, public logs, and public status. It then journals `FinalizationAttempted` and performs one settings update to empty controllers while explicitly retaining public logs and public status. Whether that update reports success or error, Historian directly reads live status again. Activation requires the running approved module with exactly zero controllers and public log/status visibility; public status is what permits this audit after Historian has lost control. A correct live state reconciles an ambiguous update result. An unexpected live state becomes `ManualRecoveryRequired`.

A transient management-status read failure in `RelayFunded` or `FinalizationAttempted` instead leaves the entry `InProgress`. The frontend's **Resume finalization** action submits the same canonical configuration and drives only this already-funded tail; interval polling remains query-only. Resumption never repeats a Ledger transfer, CMC notification, child creation, code installation, or Relay-funding transfer. Exact finalized state activates without another settings update. Exact running approved state with public log/status visibility and controllers exactly `[Historian]` permits one idempotent controller-removal settings attempt per explicit resume. If the post-update read still shows that exact safe state, or the read itself fails, the entry remains resumable rather than guessing the mutation outcome. Successful activation uses one shared path to store `Active { relay_canister_id }` and record generic `RelayTarget` and `RelayInstance` tracking exactly once.

## Upgrade and manual recovery

Historian persists one Relay setup map keyed by the 32-byte canonical configuration hash:

| Purpose | Memory ID | Key | Value |
|---|---:|---|---|
| Relay setup entries | 26 | `RelaySetupKey([u8; 32])` | current `RelaySetupEntry` |

Normal upgrades preserve current entries. `Reserved` and `ProbingTargets` entries can be removed because no irreversible action occurred. Later interrupted `Creating` phases—including same-version-resumable `RelayFunded` and `FinalizationAttempted`—become `ManualRecoveryRequired` with `HistorianUpgradeInterrupted`. `Active` and existing manual-recovery entries remain unchanged. Reconciliation issues no external call. The installed Relay hash is not persisted in setup progress, so finalization resumption deliberately does not cross a Historian upgrade; maintenance must stop Historian and drain factory calls first.

`ManualRecoveryRequired` is terminal for public automation. Operators use the exact configuration view together with reviewed Ledger, CMC, and management-canister evidence. Debug entry listing exists only in local and non-production debug builds. Production has no configuration-wide entry enumeration, so operational records must retain exact immutable configurations.

Operator triage distinguishes `create_canister ambiguous relay ID loss` from `install_code module-hash reconciliation`. If a module hash exists but differs from the reviewed Relay hash, Historian fails closed and does not reinstall or continue automatically.

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
