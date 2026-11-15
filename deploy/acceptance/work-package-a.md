# Acceptance installation and operation package A

Status: acceptance preparation, **not deployed and not final-NNX-authority-representative**.
The 15 November authority/identity/helper decisions and signed status supplement
are in [authority-2026-11-15.md](authority-2026-11-15.md). The Codex executor is
DFX `codex_local` (`amzih-…-3ae`), verified by authenticated status. The personal
`zkkkd-…-wqe` principal is protected and must never be a Codex signer.
PR #17 is closed. Source at task start is `40e81937350fcb33ce70d8d15131ebbd91f1d92b`,
tree `d19a64ae264b2e3912add64f15d82559d1c711e9`, identical to the supplied merged
tree. The local commit differs from the supplied master commit; unchanged
PR17 mechanics do not require revalidation merely for that difference.
`../IO` is read-only and is not required by these commands.

The separately approved read stages are complete; their observations and the
DFX automatic-discovery scope deviation are recorded in the dated supplements.
No standing read authorization remains. No installs, funding, controller changes
or neuron operations are authorized. Commands below remain a review plan.

## Operator decisions

Supply public values in [`operator-inputs.json`](operator-inputs.json). `approved`
means the supplied configuration has been reviewed, not authorization to deploy.
No signing key, identity export or credential file is needed.

| Value | Status/value | Exact consumer | Blocks |
|---|---|---|---|
| Twelve roles | All assignments approved in operator reply | `roles_confirmed`, existing profile role map; build bindings and all constructors | No further role decision |
| Deployment executor | Verified `amzih-…-3ae`, existing DFX `codex_local`; not an ICP CLI alias | `operator`; constructor/controller role and parked Swap fallback | Resolved; only exact later operations may be signed |
| Protected personal principal | `zkkkd-…-wqe`; historical/bootstrap controller only, no signing access | Explicit protected-identity rejection in preparation tools | Never a required Codex signer |
| 10m neuron executor | Same verified `amzih-…-3ae`; temporary acceptance-only 2/3/4 | `voting_authority = {principal, kind, executable_route}`; nonce-1 Governance neuron | Executable ordinary governance |
| Temporary exception | Approved for acceptance only | `kind = temporary-acceptance`; native permissions 2/3/4 | Real NNX is a later handover, not an installation prerequisite |
| Acceptance clock | Activation-checkpoint policy approved; final genesis/start seconds await schedule | `genesis_timestamp_seconds`, `acceptance_start_timestamp_seconds`; neuron age and programme reward clock separately | Framework constructors and patron activation |
| 20,000 jUP test bucket | A/B created, isolated and signing-tested; exact public principals in owner decisions | `test_allocations`; nonces 10/11 for A and 20/21 for B | Framework constructors |
| Relay surplus | Temporary `[]` approved and recorded in profile | `relay_surplus_neuron_recipients`; canonical Relay constructor | No initial surplus decision; eventual policy remains separate |
| Existing helper | Completed neuron-creation helper; operator waives all helper-local state preservation | `bootstrap_disburser = disposable-helper-replacement` | Exact reinstall payload/startup/timing approval; no source hunt or migration |
| D-QUORUM / economics | Settled, preserve profile and economics | Disburser second age recipient; `economics.json` | No new decision |
| Cycles | Main balances observed; total minimum top-up 18,088,365,604,174 cycles; all reserved zero | Funding delta sheet and signed status supplement | Fresh pricing/funding source and spend approval |
| Spend approval | None | Funding-sheet row caps and a separate signed-off operation list | Every mainnet spend |

For the authority object use `kind = real-nnx` only when its actual executor can
make `manage_neuron` calls as that principal. An address, inaccessible canister,
or principal copied from production is insufficient. A temporary executor gets
ManagePrincipals, SubmitProposal and Vote (2/3/4), without Disburse/Split.
Grant final 2/3/4, prove a final-executor vote, then revoke temporary 2/3/4 and
prove denial. Retain the original neuron ID/Account and exact exclusions.
The local rehearsal demonstrates this mechanism with owned fixture callers;
it supplies no approval or evidence about a real NNX implementation.

`bootstrap_disburser` records the operator's completed-helper decision and
explicit waiver, not a claim of empty state or absent historical effects. The
chosen transition is direct same-principal `reinstall`, without preliminary
uninstall. Preserve the external NNS neuron and settings; no helper-local state
migration or original-source investigation is required. Installation starts the
reviewed timer behavior and requires explicit startup/timing approval.

## Exact genesis and programme policy

`operational.py` constructs a new direct-install genesis from `economics.json`;
it does not relabel the small contested fixture. Supply reconciliation is:

| Role | jUP | Identity / permissions |
|---|---:|---|
| Governance authority | 10,000,000 | Approved executor, nonce 1; 2/3/4; exact one-year delay |
| Reserve transport | 89,979,900 | Acceptance SNS Rewards, nonce 2; Disburse only; zero delay/vesting |
| Operational proposer | 100 | Acceptance SNS Rewards, nonce 3; SubmitProposal only; one-year delay |
| Test bucket | 20,000 | Four approved 5,000-jUP neurons: A memos 10/11, B memos 20/21 |
| Built-in treasury / parked Swap | 0 / 0 | No fabricated sale proceeds |
| **Total** | **100,000,000** | Eight decimals, no extra test minting |

Test allocation records require `{owner, amount_e8s, kind, memo,
dissolve_delay_seconds}`. The approved four records each have `kind=neuron`,
5000 × 10^8 e8s, and zero or 31,557,600 seconds. The distinct chosen nonces are
10/11 for A and 20/21 for B. No initial liquid allocation is approved. Test-wallet neurons
receive native owner permissions; all maturity preferences start disabled.
For the small tests, A keeps auto-stake disabled and receives liquid rewards;
B enables it natively on its one-year neuron before funding. The candidate
contains typed A-disable/B-enable `manage_neuron` payloads to be signed by each
participant. Query the effective setting, exact delay, owner and stake before
contributing; a disabled native preference may be represented as null.
At least one economically controlled neuron is needed to recognize each test
donor; a liquid balance by itself does not confer recognition. Liquid jUP from
the direct reward test is a subsequent reserve-funded payment, not extra genesis supply.
These are operational allocations, not purchases or sale finalization.
Acceptance metadata uses `jupiter-logo.png`, a 256px Chromium rasterization of
the existing frontend token-logo SVG with external requests blocked. It labels
the SNS as acceptance. No historical fixture genesis/metadata is relabelled.

Reserve and proposer IDs and staking Accounts are derived offline in
`derived-accounts.json`, including the verified temporary executor and all four
participant neuron IDs/Accounts. All three
protocol neuron IDs are excluded. The reserve disburses natively to SNS Rewards
default Account, net of 0.01 jUP at the initial fee. It is inventory, not an ICP
redemption liability. No parallel minting or sale pot is introduced.

Initial application constructors omit `programme`. Stage 1 recognition uses
version 1, patron disabled, launch clock absent, reserve transport fixed,
snapshot maximum age 48h, proposer fixed, empty mirror targets and treasury off.
Stage 1 can **disburse the reserve** and replenish a depleted proposer.
Stage 2 enables patrons with the approved acceptance clock and version 2.
Stage 3 uses version 3, treasury enabled, 1 ICP retained and maximum 1 ICP per
native proposal; the native seven-day framework limit also applies. This is a
proposed test policy, requiring separate activation/spend approval.
No stage enables IO settlement or mirrors. Fixed financial bindings do not
change between stages. Reconfiguration waits for existing journal fences.

## Offline candidate and governance commands

Use workflow-pinned `didc 0.6.2`, `ic-wasm 0.11.0` on the host and the repository
Rust toolchain/lockfiles. `artifacts.py` verifies all six raw/gzip hashes,
embedded Candid and source revisions from `sns-framework.json`. Archive is a
template, not a thirteenth initial installation. No tuple update is proposed.

```bash
python3 tools/acceptance/profile.py check-roles deploy/acceptance/profile.json
python3 tools/acceptance/artifacts.py
# Use the exact verified directory printed above as the third positional input.
# This workspace's verified tuple directory is shown below. Generation fails
# before writing output until operator-inputs.json is explicitly completed.
python3 tools/acceptance/operational.py candidate deploy/acceptance/profile.json deploy/acceptance/operator-inputs.json /home/codexdev/.cache/verified-sns/4d5e00cd65581c64 target/acceptance-candidate --build
```

The candidate contains the completed profile, seven application and five typed
framework constructors, frontend bindings, Accounts, controller plan, derived
neuron IDs, recognition/patron/treasury policies and upgrade arguments, native
disable-auto-target proposal, non-debug application Wasms/build records and a
hash manifest with tool/source identity. Historian embeds its freshly built
acceptance Relay. Frontend destinations use that same profile; IO is disabled.
Output directories cannot already exist. Constructors round trip through exact
Candid types. Local rehearsal profiles retain `local-fixture-not-for-deployment`.
Do not use them with the CLI installation handoff.

Application artifacts alone can be built before those answers, because their
fixed canister/neuron bindings do not contain the operator or SNS genesis:
`python3 tools/acceptance/build.py deploy/acceptance/profile.json
target/acceptance-preparation-a/artifacts jupiter_historian` (Historian also
builds its exact embedded Relay). Repeat the same existing command for
`jupiter_disburser`, `jupiter_faucet`, `jupiter_lifeline`, `jupiter_sns_rewards`
and `jupiter_faucet_frontend`. Build records mark unresolved constructor inputs
and absent mainnet authorization. Strict constructor and CLI installation checks
remain in force; an artifact-only package is not a deployable bundle.

For each stage, generate `operational.py upgrade GOVERNANCE_DID SUBACCOUNT_HEX
APPLICATION_PRINCIPAL WASM_GZ UPGRADE_ARGS_BIN OUTPUT_DIRECTORY` from that
candidate. The result is a typed `ManageNeuron` proposal, not a direct operator
configuration call. A fourth `treasury-continuation` policy keeps the same fixed
bindings and limits with version 4, through that same reviewed native upgrade
path. Use it only after conclusively verified settlement, quiescence, renewed
funding approval and the native seven-day limit check. It authorizes one further
bounded operation, not a recurring loop. `operational.py vote GOVERNANCE_DID VOTER_NEURON_HEX
ADOPTED_PROPOSAL_ID OUTPUT_DIRECTORY` produces the Yes ballot. `permissions`
uses the same DID/neuron, the destination principal, `grant` or `revoke`, and an
output directory. Inspect and hash each payload before signing; proposal IDs
come from real Governance replies, never guessed values. Modules over the
bounded inline proposal size require a separately reviewed native chunked
upgrade recipe, not a raised ingress limit.

The final tool check and pinned offline signing verification use DFX 0.31.0
`codex_local`; the repository's 0.27.0 context was an unverified read-time label.
Read-time version was not captured; no live read or toolchain upgrade was repeated.
This is not a matching ICP
CLI alias. After a separate exact-operation approval, use `dfx --identity
codex_local canister call sipaq-hyaaa-aaaas-amyoq-cai manage_neuron` with
`--network ic --update --candid` pointing to the verified official Governance
DID and `--argument-type raw --argument-file` pointing to the generated hex sidecar.
DFX uses `--argument-file` / `--argument-type raw` for validated hex text, whereas ICP CLI uses
`--args-file` / `--args-format bin`; do not pass binary files as DFX Candid text.
Participant operations use their own aliases. Always supply a local Candid file
for bounded DFX calls: automatic interface discovery exceeded the read allowlist
in the information-query stage. No private-key export or identity-store bridge
is needed. Plaintext file custody and the command-scoped DFX warning
acknowledgement are explicit conditions of any later signing approval.

There is no public programme configuration update API. Use the tested native
`UpgradeSnsControlledCanister` action with generated upgrade arguments through
Root/Governance. No registered custom function or validator is required for this
path. The proposer can submit its existing automation proposals but cannot
decide them; the decisive executor must vote. The alternative
`request_treasury_operation` update accepts **only pinned Governance**, after a
TransferVerified predecessor and a new increasing operation ID. An operator or
small proposer cannot invoke it directly as Governance. No registered function
and validator for that alternative is installed by this package; use the
generated native upgrade continuation instead. Its resulting native treasury
transfer proposal still needs the decisive vote.

## Ordered installation and transition plan

First execute only the separately approved inventory below. Bind its results to
the final package hashes and signer. A role with an existing module/state needs
an explicit preservation decision; never default `auto` into replacement.
For each approved empty-code role, use DFX `codex_local` with an explicit
principal, `--network ic --mode install --wasm` exact package, and
`--argument-file` exact validated constructor hex sidecar / `--argument-type raw`. The
proposed-operations JSON records the actual per-role argument vectors and hashes.
Seven application constructors are available; five framework constructors remain
blocked on the schedule. Never treat partial preparation as a complete bundle.
There is no bulk `icp deploy` or canister creation in this plan.

| Order | Canister(s), explicit IDs in profile | Package / constructor | Controller checkpoint |
|---|---|---|---|
| 1 | Lifeline | Acceptance application / empty | Verified bootstrap operator; minimal reserved recovery role |
| 2 | SNS Governance | Pinned official / governance.bin, Normal=1 | Bootstrap until framework verified, then Root |
| 3 | SNS Ledger | Pinned official / ledger.bin | Root after bootstrap checks; Governance minting Account |
| 4 | SNS Index | Pinned official / index.bin | Root; exact Ledger |
| 5 | Parked Swap | Pinned official / swap.bin | Root; zero initial allocation, no sale claim |
| 6 | SNS Root | Pinned official / root.bin | Governance after bootstrap checks; verify membership |
| 7 | SNS Rewards | Acceptance production-mode / application.bin, no programme | Root after controlled upgrade rehearsal |
| 8 | Historian | Acceptance with embedded acceptance Relay / application.bin | Root after upgrade/factory-binding checks |
| 9 | Canonical Relay | Acceptance / application.bin, approved surplus policy | Root; replacement semantics, reconcile/quiesce before replacement |
| 10 | Faucet | Acceptance / application.bin, rescue unarmed | Bootstrap pending lifecycle rehearsal |
| 11 | Existing Disburser | Approved disposable-helper transition: guarded `reinstall`, fresh typed args; execution still unapproved | Same `spoge-…` principal; unarmed |
| 12 | Frontend | Acceptance assets / empty constructor | Root after destination and certified-asset smoke checks |

At every checkpoint compare installed compressed package module hash, controllers,
configuration, cycles/headroom and intended runtime state. Installation starts
timers: Disburser can read/disburse maturity, Faucet can scan/settle and notify
CMC, Relay can fund managed canisters, Historian indexes and factory is enabled,
SNS Rewards starts owner scans. No programme is **not** financial inertness.
Review existing ICP/maturity and pending work before installing any application.

The operator confirms the helper's neuron-creation lifecycle is complete and
accepts discarding its heap/stable state. Record that decision without claiming
the state was empty or that no past pending effects existed. Direct reinstall
at `spoge-kaaaa-aaaas-amyoa-cai` needs no preliminary uninstall. It preserves
the external neuron principal relationship and Account; no neuron recreation or
settings change is proposed. The initial production code installs daily main
and rescue timers. A main tick can initiate 100% native maturity disbursement
to Disburser's default staging Account and later execute its payout plan. The
00:39 NNS observation contained nonzero maturity; it was not repeated. Review
that financial startup with the exact initialization and installation schedule;
rescue-unarmed and absent Rewards programme do not make it inert.

The 00:39 public full-neuron observation matched the existing controller,
staking bytes, cached stake and Ledger balance and exposed dissolve/follow/
maturity fields. It is dated replica-signed evidence, not a new observation in
the status stage. Any later freshness reads need separate authorization. The treasury worker uses public-full-neuron
`list_neurons`; private operator visibility does not satisfy it. Public visibility was observed and no visibility mutation is proposed. Relay's
own neuron lookup must succeed under its actual caller and API shape as well.
No new broad management proxy is proposed.

Before rehearsal, explicitly add Root as a controller alongside both bootstrap
controllers for SNS-managed roles, Governance alongside both for Root, and self
alongside both for Faucet/Disburser. This gives the real governed/runtime path
its required authority without premature removals. These additions need separate
controller-change approval. Rehearse ordinary upgrades and normal runtime while
both bootstrap controllers remain; only then approve the narrowing steps.
Faucet/Disburser then use the existing operator+self staging, explicitly arm
only after normal-runtime/rescue evidence, and reach self-only healthy control.
Broken flow permits self+Lifeline and healthy flow narrows it. Lifeline has no
generic repair API; an incident-specific reviewed code operation is separate.
Factory children finish controllerless, immutable and unrepairable. Do not add
an operator backdoor to satisfy recovery. Framework and other SNS-managed
applications use their exact generated Root/Governance topology.

## Framework version and smoke gates

Direct genesis explicitly sets `automatically_advance_target_version=false`.
The pinned official production-launch converter cannot supply that field and
inherits true; execute the generated native disable proposal after Normal mode
for that path. Record the effective parameter, actual deployed tuple, target
version, pending upgrade and physical module hashes before schema-sensitive
automation. Disabling advancement does not cancel an already pending upgrade.
Keep application module/schema guards intact. For a future security update:
review the exact official tuple/revisions/interfaces, regenerate/check DTOs,
rehearse state/journals and controlled native upgrades, approve the tuple and
execute under Governance, then recheck physical hashes and pending state.
Acceptance pinning is reviewed control, not indefinite avoidance of updates.

Smoke evidence, in order: unconfigured financial inactivity of programme;
Normal mode and exact supply; real executor submit/vote and rejection of revoked
authority; actual Governance upgrade execution; verified reserve receipt and
proposer readiness; complete secure owner snapshot; 1-ICP liquid and 1-ICP 2x
test with exact fees/native refresh; genuine Relay aggregate pot and immutable
distribution; treasury directly to NNS stake with **null routing memo**, no patron
jUP or Relay pot, eventual native refresh, normal 95/5 age-bonus routing; upgrade
without cohort/journal loss or repeated payments; physical controller and arming
transitions. Native new stake changes NNS age; do not assert age preservation.
IO settlement remains disabled throughout. Local real-sale evidence is the
existing SNS-W sale rehearsal; direct acceptance cannot prove sale valuation.

See [`funding-sheet.json`](funding-sheet.json) for proposed small-test caps and
[`live-read-plan.json`](live-read-plan.json) for the historical finite inventory
definition. That public stage and the later 15-item authority stage are complete;
no unused slots grant continuing access. The latter completed the listed reads
but DFX information-call interface discovery added two out-of-scope attempts;
see the authority supplement for the exact evidence limitation. Eighteen
post-installation checks remain separately unauthorized. No read approval
includes installation or a financial operation.
Primary small-test provisioning is capped in the draft at 6.0007 ICP, including
up to 0.0002 ICP for funding the two donor wallets if needed. The optional child
adds a separate 3.0001 ICP, for 9.0008 ICP maximum. These are proposed caps, not
observed deltas or spending authorization; cycles purchases remain unpriced.
[`relay-surplus-plan.json`](relay-surplus-plan.json) records the separate proposed
eventual acceptance IO/Jupiter recipients and exact memo bytes. IO's checked-in
draft lacks its permanent neuron's actual staking bytes; that Account is
explicitly unresolved, not derived from the neuron ID. No IO files were changed.
The remaining owner inputs are the two schedule timestamps, funding-source/
spend approval and exact installation/activation/controller-transition approvals.
No helper-source, participant-selection or personal-signing request remains. Build and test evidence is recorded separately
in `validation-a.md`; no local result is a mainnet observation.
