# Final governance permission correction and approval sheet

The permission correction below is retained. Its earlier schedule/partial-bundle
proposal is superseded by the complete configuration-only candidate in
[installation-approval.md](installation-approval.md): genesis 17 November 2026
14:00 UTC, patron candidate 19 November 16:00 UTC; final installation-window
confirmation and every live-operation approval remain separate.

Preparation base: `0981156dcc30735600d712eed38b74440c43d8cf`, draft PR #18.
This correction authorizes no mainnet call or mutation. Prior status and funding
observations remain dated evidence, not standing read authority.

The 10-million-jUP governance neuron grants the temporary acceptance executor
`amzih-bssz4-twanl-zf6nr-bqsnn-d7rlt-eerbc-oe7vv-uggn5-3naqo-3ae`
only SubmitProposal (3), Vote (4), and ManageVotingPermission (10). It no longer
grants ManagePrincipals (2). Pinned Governance at
`98c898f8de068a1fc0e97ebc805f9fa7f315d862` treats exactly 3/4/10 as voting-related.
Global claimant/grantable permissions and ordinary participant rights remain
1 through 10. Voting still influences financial Governance proposals; this is
removal of direct neuron-permission escalation, not economically powerless voting.

The original neuron ID remains
`ae56e389e2c2a16b9f331ccb9a9ef21e9d02511ecc0ba79644c9ea004dda71e5`.
Its staking Account, stake, delay, age and programme exclusions are unchanged.
The two participant identities, four 5,000-jUP allocations, 100-million supply,
reserve/proposer roles, PR #17 economics and official component tuple are retained.
The personal principal is never a Codex signer. No identity/provider lookup,
credential access, mainnet read or mainnet mutation was needed for this correction.

## Authentic validation

The existing official-SNS operational PocketIC case passed with the pinned real
Governance Wasm (compressed SHA-256
`24ef2004240d2a34bcdaa616134d27bc18d11c1e96b378a46d564feedcc1a8c3`):

- Temporary voter submits a native proposal and casts an explicit native vote.
- It grants a distinct local executor 3/4/10; that executor submits and casts the
  decisive explicit vote before any temporary rights are removed.
- Each non-voting code 1/2/5/6/7/8/9 is denied for self and another principal,
  alone and mixed with Vote-related code 3: 28 exact NotAuthorized responses.
- Temporary voter removes its own 3/4/10. Its former submit, vote, add and remove
  operations return exact native NotAuthorized, while the successor still votes.
- Neuron ID, native stake, staking Ledger balance, dissolve delay, aging timestamp,
  multiplier and fee state are preserved. Actual ballot weights match at the
  fixed local clock, comparing like proposal-fee reservation conditions.
- Production Rewards receives the same original protocol/excluded-neuron ID
  through an actual governed upgrade. Existing reserve transport and no-repeat
  journal assertions pass. This verifies the exclusion binding, not live IO
  settlement or a new audit of the unchanged exclusion algorithm.

The local successor is explicitly synthetic; the real NNX executor remains a
later input. Native `operational.py permissions ... grant|revoke` recipes emit
only 3/4/10. Grant the actual executor, prove its real proposal/vote signing route,
then self-revoke and verify denial. Never remove the sole functioning successor.
No live permission operation is authorized.

The constructor validator enforces positive nat64 clocks and
`genesis_timestamp_seconds <= acceptance_start_timestamp_seconds`. Python checks
cover equal/later activation, reversed ordering with no output, exact supply,
unchanged full user rights, and typed grant/revoke recipes. Both checked-in clocks
remain null. No preparation date or local fixture time enters live constructors.

Commands/results and public source evidence are in
`target/acceptance-governance-correction/`. The official case is
`cargo test --locked -p jupiter-relay --test jupiter_relay_integration acceptance_sns::operational::exact_genesis_governance_handover_and_production_reserve -- --exact --ignored --nocapture`
with didc 0.6.2 and the existing PocketIC environment; 1 passed, 61 filtered.
No unaffected production application or canonical release build is needed.

## Proposed schedule — not approved

| Checkpoint | Proposed UTC | Gate |
|---|---|---|
| Genesis / begin installation | 2026-11-17 14:00:00 | Owner approves this exact genesis anchor; prepare all arguments beforehand |
| Installation window | 2026-11-17 14:00–16:00 | Separate payload/startup and fresh-precondition approval |
| Independent smoke period | Installation completion to 2026-11-18 14:00 | Normal mode, module/controller tuple, target/pending version, owner snapshot, recognition/reserve/proposer readiness; new read approval |
| Patron activation checkpoint | 2026-11-18 14:00:00 | Owner confirms exact distinct patron clock and approves actual Governance execution |

This leaves at least 22 hours after the proposed installation window for smoke
checks. Supply these approved values only after confirming the window. If the
genesis or activation checkpoint is missed, pause and obtain a revised schedule
before regenerating/reviewing constructors or proposals. Do not silently backdate
or enable a programme against an unapproved historical launch timestamp. The
production launch clock is unrelated.

## Funding Accounts and independent methods

ICP Ledger is `ryjl3-tyaaa-aaaaa-aaaba-cai`. All Accounts below use default
subaccount (null); each legacy address is the exact ICP AccountIdentifier.
Amounts and dated 10,000-e8s fees require separate confirmation/approval before
spending. No source is selected and authorized spend remains zero.

| Destination | Owner principal | ICP AccountIdentifier | Proposed receipt |
|---|---|---|---:|
| Participant A | `dpb4d-s3blg-twxrv-w4ucr-2w76w-rctgx-zycty-at3p4-5paoc-x4764-eae` | `db724cfae597fff199cacfdcdbd6c339319ce2eb913407bf99ae8474925d5765` | 1.0001 ICP |
| Participant B | `lsgoi-jv53o-vvez4-teecu-5t66a-7bnlq-mjvi3-iails-23k3p-qykdc-lae` | `3654af7811166ecf9b95f17792bfffd66317169a25c59ef1f1f366c797cc9c2e` | 1.0001 ICP |
| Canonical Relay | `xtv4b-biaaa-aaaas-amyrq-cai` | `620e88ec50f6c35b1a58ea9f7fc3f3c7f23355d1582fc9912ecfca405bc8c069` | 2 ICP |
| SNS treasury | `sipaq-hyaaa-aaaas-amyoq-cai` | `ae61bdb02899de73a2a650399a5d25af0df69942067ed1051525e9d848e6b1d9` | 2.0001 ICP |

A nominated external wallet can fund those Accounts directly using its own
provider; Codex never obtains access to it. Alternatively, a nominated funder
can provision a separately approved capped amount to the existing acceptance
executor default Account `bab708d3f58aad4861d1ece056efbe900721dd02c7037af5cab4488e288f2236` (owner
`amzih-bssz4-twanl-zf6nr-bqsnn-d7rlt-eerbc-oe7vv-uggn5-3naqo-3ae`), followed only by separately approved Codex transfers. Its
plaintext-at-rest custody limitation remains applicable. Source-side provisioning
fees are separate from receipt amounts; do not silently expand the test cap.

Participants later send their approved one-ICP contributions themselves to the
existing NNS staking Account `e458b5ed224bd0b9e884599dd0e986e78d02266674b093b617105528311fa602`,
owner `rrkah-fqaaa-aaaaa-aaaaq-cai`, subaccount
`52eb1b31eb9cf6357e2b2e49e32d2238d2b6db2b2f321cbe7b879ea261ac7390`,
with exact Faucet memo bytes in the proposed-operations record. Those deposits
become NNS stake; wallet provisioning is not already a patron contribution.

An independent existing cycles wallet/provider can instead fund each exact
canister principal through separately approved native cycles top-ups. The dated
main-balance deficit remains **18,088,365,604,174 cycles** to reach **3T each**;
all twelve destinations and deltas remain in [funding-sheet.json](funding-sheet.json).
Do not send ICP to a canister principal expecting its main cycles to increase.
A later conversion method needs a fresh authorized quote, source and fees; the
old CMC observation is not a standing price. Conditional Ledger archive allocation
is **1T**, separate from main balances and only before its actual trigger.
No new thirteenth initial canister is planned.

Primary small-test cap stays **6.0007 ICP**, optional child **3.0001 ICP** extra,
**9.0008 ICP** total with that child; cycles purchases remain separately unpriced.
These are proposed caps, not approved spends. The optional immutable child has
no invented funding Account: its exact returned setup Account/requirement must
be reviewed under a later paid-creation approval. Treasury top-up has no Faucet
routing memo and creates neither patron jUP nor a Relay pot.

## Next approvals and exact operations

[Proposed operations](proposed-operations-2026-11-15.json) retain explicit target,
mode, signer, Wasm and constructor hashes. Eleven empty-code targets use install;
the same Disburser principal uses guarded reinstall without preliminary uninstall.
Five framework constructors remain schedule-blocked; the seven application
constructors and Wasm packages are unchanged. Final source/tree, proposed-operation
hash and package checksums are recorded with the final review archive.

Disburser starts real daily timer/NNS maturity work, including potential 100%
maturity-disbursement requests and normal 95/5 routing. Rescue-unarmed and absent
programme do not make this startup financially inert. Preserve the existing
external NNS neuron/state and both bootstrap controllers; no live mutation occurs
here. Initial Ledger/Index/Relay and other application timers also require the
reviewed startup bindings; disabled new programme workers remain inactive.

| Owner decision | Exact next preparation or gated action |
|---|---|
| Confirm or replace the two proposed UTC checkpoints/window | Populate both distinct clocks, generate/type-check five framework constructors and final source-bound payload hashes |
| Identify independent funding source/method and approve exact budgets | Review fresh authorized rate/fee/balance preconditions and capped transactions to the exact Accounts/canisters; no personal provider access |
| Approve exact installation payload/startup and finite precondition reads | Execute only a later authorized eleven-install / one-reinstall plan; no auto/bulk mode |
| Approve recognition/reserve readiness, then patron checkpoint separately | Actual decisive native Governance vote/upgrade; no financial enablement merely from framework installation |
| Approve treasury activation and bounded continuation separately | Native direct-to-NNS stake, each proposal capped and reviewed |
| Approve final controller removals/arming only after rehearsals | Preserve self/Lifeline recovery and Root/Governance model; irreversible children and optional creation remain separately gated |

The old information-query discovery overrun remains visible in
[authority-2026-11-15.md](authority-2026-11-15.md): 15 listed reads completed plus
two source-inferred unauthorized discovery attempts; CLI wire counts unavailable.
It is not a clean allowlist pass. Private DFX identities remain **plaintext at
rest** in local owner-read-only custody. Neither limitation is erased by this
correction. IO settlement stays disabled, canonical Relay surplus stays temporary
`[]`, and future NNX integration remains unclaimed. Keep PR #18 draft.
