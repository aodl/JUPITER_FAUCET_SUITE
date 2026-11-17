# Schedule-blocked installation and funding preparation

**SCHEDULE-BLOCKED — NOT EXECUTABLE.** The owner has not approved an
installation day and has withdrawn the earlier genesis and patron dates.
Both active fields in `candidate-inputs.json` are `null`:
`genesis_timestamp_seconds` and `acceptance_start_timestamp_seconds`.
No replacement dates are selected. Existing fail-closed generation rejects a
complete live bundle until the owner selects and approves both exact timestamps.

The complete dated PR #18 package remains historical tested evidence only:
source `f72038f2da8c88c0d46fdf105f14fd78327702a9`, archive SHA-256
`c25ec8fff3e43f6b106b9d8bf422c79847c7152a231352c04f227c7eb6ae4eb2`.
Its manifest, constructor bytes and hashes are preserved unchanged; they do not
apply to a future schedule. The exact dated input is retained in
[historical/2026-11-15-candidate-inputs.json](historical/2026-11-15-candidate-inputs.json)
solely for that historical rehearsal, not live deployment.

The timestamp fields and window in the active operation record are unresolved.
Historical CLI vectors and unsigned payloads are explicitly labelled; active
install command vectors and schedule-dependent framework hashes are null.
After schedule approval, regenerate and review the complete constructors,
activation payloads and hashes before any separate execution approval.
Principals, permissions, economics, Wasms, funding requirements, controller
preconditions and startup effects below remain unchanged. No mainnet operation
is authorized by this administrative withdrawal.

## Historical constructor evidence and retained installation modes

The historical dated bundle contains five official SNS constructors, seven application
constructors and the six pinned official SNS Wasm templates (archive is a
contingency template, not a thirteenth initial installation). All constructor
bytes were encoded/decoded/re-encoded with didc 0.6.2 and have matching raw-hex
sidecars. Seven application constructor binaries and all retained acceptance
application Wasms are unchanged. Acceptance Wasms are not canonical production
artifacts and this is not a new independent reproducibility claim.

| Ordered role | Principal | Mode | Historical constructor SHA-256 |
|---|---|---|---|
| jupiter_lifeline | `xiqae-3qaaa-aaaas-amyta-cai` | install | `0fee102bd16b053022b69f2c65fd5e2f41d150ce9c214ac8731cfaf496ebda4e` |
| sns_governance | `sipaq-hyaaa-aaaas-amyoq-cai` | install | `123f9348365ece40b9c1d212055c68914ce9cd825366eef9d0c5f07870d89e35` |
| sns_ledger | `sgnny-4iaaa-aaaas-amypq-cai` | install | `cb32950dd69d09206735847fb7d5a4e623ca27a0ceef2718fbe7d02563e3edfe` |
| sns_index | `x2wx5-xaaaa-aaaas-amyqa-cai` | install | `c8ad314678a03900d37042030cdbddc57c5b78fe1264d9e90e65e678b63d5b85` |
| sns_swap | `x5xrj-2yaaa-aaaas-amyqq-cai` | install | `ac3e3001699440f1207b0b83dda9c2ff6a8dbc32249fe8c8b73b7211facba528` |
| sns_root | `sbmlm-rqaaa-aaaas-amypa-cai` | install | `44e81e88d9e312e20df0401167e86fce2e8996e67a4771660f6dbf5a0cb8c65e` |
| jupiter_sns_rewards | `xgsnm-aaaaa-aaaas-amysa-cai` | install | `136ddb75378b67ca6c34cc41efd7e41899a07a970b2618b4520b5ff866f5d3fe` |
| jupiter_historian | `xbtly-nyaaa-aaaas-amysq-cai` | install | `a6d8ccb6b103e8c8820e4b65f4e45d0e4749c8e138596286955cbe5b503056f0` |
| jupiter_relay | `xtv4b-biaaa-aaaas-amyrq-cai` | install | `fd43a5dd713a863de08d8f95c1425fd964736d52617e07981c8ebd7138ff2d97` |
| jupiter_faucet | `xuu2v-mqaaa-aaaas-amyra-cai` | install | `bb22dd75d82391bb0a611c3174dddaee27846510da441ab6dc01b17df9257db0` |
| jupiter_disburser | `spoge-kaaaa-aaaas-amyoa-cai` | reinstall | `519a1877c82858bc45aa8c1a5caeb58ad6f35d0dfad613c5ceea21d3363abb4a` |
| jupiter_faucet_frontend | `xprgq-wiaaa-aaaas-amytq-cai` | install | `0fee102bd16b053022b69f2c65fd5e2f41d150ce9c214ac8731cfaf496ebda4e` |

Full raw/gzip Wasm hashes, constructor/hex/interface hashes, explicit DFX argv,
controller preconditions and native transport are in
[proposed-operations-2026-11-15.json](proposed-operations-2026-11-15.json).
The historical command reference pins
`DFX_VERSION=0.31.0`, `--identity codex_local`, `--network ic`, explicit mode,
exact `--wasm`, and `--argument-type raw --argument-file` for the hashed hex file.
No auto mode, bulk deploy, implicit identity or ICP-CLI identity-store bridge.

DFX install has **no --candid flag**. It consumes the already typed raw bytes;
their exact local type definitions and hashes are packaged. All later DFX
`canister call` recipes must supply `--candid` with the pinned local service DID
and raw argument file, preventing metadata/interface discovery. No newly signed
read or proposal was needed to verify the hex handoff; the accepted offline
DFX transport evidence is retained and all twelve hex/bin pairs are compared.

Pinned DFX 0.31 / ic-utils 0.45 chooses chunked installation when Wasm plus
constructor reaches 1.85 MiB. Governance (1,938,237 + 106,032 bytes) and frontend
(2,455,026 + 6 bytes) cross that cutoff. Their exact plan therefore includes
`stored_chunks`, respectively two/three `upload_chunk` calls,
`install_chunked_code`, then `clear_chunk_store`. The other ten targets use
`install_code`, including Disburser in reinstall mode. With empty stores this is
**12 logical install commands / 21 native management calls**, before normal
certificate/status transport. Native chunk hashes and verified CLI-source
identity are recorded. Preflight must confirm both chunk stores empty; stop if
unrelated staged chunks exist. No upload/store-clear/installation is authorized
here. After an ambiguous failure, inspect under a new bounded approval rather
than blindly replaying an install or destructive reinstall.

Both observed bootstrap controllers remain through installation. Installing code
does not remove them. The generated applications/controller-plan.json contains
role/minimum expectations only; it is not permission to apply its smaller sets.
The authoritative full observed sets and later add-before-remove transitions
are the proposed-operations controller records. SNS Root/Governance additions,
Faucet/Disburser self/Lifeline lifecycle, irreversible removals and arming require
separate explicit approval after real operation/recovery has been demonstrated.
No generic Lifeline repair API or permanent hidden controller is introduced.

## Supply, authority and timer boundaries

The constructor equation is unchanged: 10,000,000 governance + 89,979,900 reserve
transport + 100 operational proposer + four 5,000-jUP participant neurons =
**100,000,000 jUP**. Governance-only permissions remain **3/4/10**, normal-user
permissions remain 1 through 10, and native mode is **Normal (1)**. Native
inflation stays zero. Fees, bonuses, voting thresholds, genuine Relay endowment
allocation and all PR #17 financial guards remain unchanged. The four test
allocations are not sale purchases or fabricated sale history.

Exact derived Accounts/neurons are in the constructor profile/neuron-ids and
[derived-accounts.json](derived-accounts.json). The existing Disburser
`spoge-kaaaa-aaaas-amyoa-cai`, NNS neuron `2234355977000034918` and staking bytes
`52eb1b31eb9cf6357e2b2e49e32d2238d2b6db2b2f321cbe7b879ea261ac7390`
are preserved. Same-principal direct reinstall discards the completed helper's
local state under the accepted waiver; no preliminary uninstall, migration,
new neuron or NNS setting mutation is proposed.

Disburser initialization starts daily timers/NNS work, including potential
100-percent maturity-disbursement requests into its staging Account and normal
95/5 age routing. Previously observed maturity is nonzero; fresh public-neuron
and staking/balance reconciliation must be reviewed before startup approval.
Rescue remains unarmed, but that does not make startup financially inert.
Faucet and canonical Relay daily workers and Historian tracking/factory timers
also start under fixed reviewed bindings; funding must respect runtime readiness.
SNS Rewards begins with **programme absent**. Recognition/reserve preparation,
patron activation, treasury activation and bounded continuation are separate
native Governance stages; their exact unsigned payload/argument/policy hashes are
packaged. No payload has been signed/submitted. Verify deployed tuple, target,
pending upgrades and schema guards before automation. IO settlement remains
disabled and canonical Relay surplus remains the approved temporary empty list.

## External-wallet funding — no personal credential access

Use the exact default Accounts below on ICP Ledger
`ryjl3-tyaaa-aaaaa-aaaba-cai`. An independent funder operates its own wallet;
Codex does not obtain its signer, identity alias or credentials. Do not provision
the executor as an intermediate funding source in this selected method.
The source wallet and cycles provider still need identification by the owner.

| Receipt destination | Owner principal (null subaccount) | ICP AccountIdentifier | Proposed receipt |
|---|---|---|---:|
| Participant A | `dpb4d-s3blg-twxrv-w4ucr-2w76w-rctgx-zycty-at3p4-5paoc-x4764-eae` | `db724cfae597fff199cacfdcdbd6c339319ce2eb913407bf99ae8474925d5765` | 1.0001 ICP |
| Participant B | `lsgoi-jv53o-vvez4-teecu-5t66a-7bnlq-mjvi3-iails-23k3p-qykdc-lae` | `3654af7811166ecf9b95f17792bfffd66317169a25c59ef1f1f366c797cc9c2e` | 1.0001 ICP |
| Canonical Relay | `xtv4b-biaaa-aaaas-amyrq-cai` | `620e88ec50f6c35b1a58ea9f7fc3f3c7f23355d1582fc9912ecfca405bc8c069` | 2 ICP |
| SNS Governance treasury | `sipaq-hyaaa-aaaas-amyoq-cai` | `ae61bdb02899de73a2a650399a5d25af0df69942067ed1051525e9d848e6b1d9` | 2.0001 ICP |

Procedure after separate approval: verify Ledger/network and each exact Account
against the package; obtain fresh balances, Ledger fees and wallet/provider
charges; calculate the net receipt delta; review the capped transaction; the
external funder sends and retains its block/receipt. Use no invented Faucet memo
on wallet provisioning, Relay funding or treasury inventory. Stop on a fee,
destination, amount or timing mismatch. A receipt is verified only under a new
approved read plan; do not silently repeat old inventory queries.

At the dated 10,000-e8s Ledger fee, the four external transfers need 6.0003 ICP
of receipts plus 0.0004 ICP sender fees = **6.0007 ICP** primary new-funding cap.
Each participant then spends 1 ICP + 0.0001 ICP from its funded wallet under a
later participant-signed contribution approval. Those 2 ICP become NNS stake.
Treasury's later separately approved 1 ICP stake transfer consumes another
0.0001 ICP fee and leaves 1 ICP retained capital. Thus planned use is 3 ICP new
NNS stake, 2 ICP Relay operational liquidity, 1 ICP retained treasury and 0.0007
ICP total budgeted fees; actual Relay operations have their own fee accounting.
Existing NNS principal is not a new patron deposit. Treasury staking creates no
patron jUP and no Relay pot. Do not double-count participant wallet provisioning
and its subsequent contribution as two fresh funding requirements.

The optional immutable child is a separate **3.0001 ICP** cap, total **9.0008 ICP**
with the primary plan. Its exact returned setup Account and cost must be reviewed
under a later creation approval; no fabricated child destination or creation.
All caps remain **unapproved**. A different fresh Ledger fee, provider charge or
balance changes the exact spending package and requires review, not silent
rounding or extra funding. Source wallet/exchange surcharges are not hidden.

For cycles, an independent supported cycles wallet/provider deposits only fresh
per-canister deficits into the twelve exact principals in the funding sheet.
Do not send ICP to a canister principal expecting main cycles to increase. The
dated deficit totals **18,088,365,604,174 cycles** toward **3T main each**; reserved
balances were zero. Proposed unapproved delivery ceiling: **18.2T cycles**.
Deliver only the fresh summed deficits, not the unused margin. Stop/reapprove if
the fresh requirement exceeds that ceiling. **36T is the desired aggregate
balance, not the amount to purchase.** Any ICP conversion/purchase cap remains
unpriced until a fresh authorized quote and provider/fee calculation exists.
The old CMC observation is not a current price. Ledger's conditional **1T archive
allocation** remains separate, only before its actual trigger; factory creation
is also separate. No uptime guarantee follows from a 3T starting target.

## Freshness, smoke and approvals

[execution-preconditions.json](execution-preconditions.json) proposes a fresh
finite read-only preflight for separate approval. It is not executed or authorized.
Normal query/certificate verification remains enabled, no retries/fallback
identities or automatic interface discovery, and verification-support transport
must be separately counted. Use the twelve status/controller/module/cycle
results and explicit empty chunk-store checks immediately before installation.
Funding balances, fee, conversion quote and pending NNS effects must be fresh at
the actual spend/startup approval; obtain a revised bound if another refresh is
needed. A denied read is a reported limitation, not authority to substitute a signer.

The existing **18 post-installation checks** remain a separate bounded smoke
proposal: jUP metadata/supply and selected balances, Root membership, Governance
Normal mode/parameters/running tuple/upgrade journal, and Rewards initial/reserve
status. Check initial programme absence before any activation. The complete
local exact-clock rehearsal checks official constructor installation, all four
participant neurons and staking balances, native 3/4/10 escalation denial and
handover, reserve transport, actual governed upgrades and no-repeat journals.
It is local evidence, never a mainnet observation. The two initial failed clock
fixture attempts and native unordered-permission assertion are retained with
the corrected result; no application or economics change was made to satisfy them.

| Separate owner approval | Exact scope |
|---|---|
| Select and approve a deployment schedule | Deployment day and both exact timestamps remain unresolved; regenerate/review complete constructors and payload hashes before execution approval |
| Fresh bounded preflight | Listed status/chunk-store/balance/fee/quote/public-neuron reads only; no standing read permission |
| External funding | Named source wallet/provider, fresh exact deltas/fees and explicit cycles/ICP purchase/spend caps; no credentials provided to Codex |
| Initial installation/startup | Twelve ordered explicit commands, including listed native chunk methods and Disburser reinstall/timer effects; retain both bootstrap controllers |
| Financial/participant stages | Recognition/reserve; later patron checkpoint; participant-owned SNS auto-stake changes and transfers; treasury activation/each bounded continuation separately |
| Irreversible transitions | Controller additions/removals and rescue arming after demonstrations; optional immutable child creation separately |

The protected personal principal remains inaccessible to Codex. Existing DFX
executor/participant custody is plaintext at rest in private owner-read-only
local storage. The earlier information-query discovery overrun remains recorded:
15 listed reads plus two source-inferred unapproved discovery attempts, with
uncaptured CLI wire counts. Neither limitation is erased by this complete bundle.
Keep PR #18 draft. Nothing in timestamp selection or this package authorizes
mainnet operations, proposals, transfers, visibility changes or IO activation.
