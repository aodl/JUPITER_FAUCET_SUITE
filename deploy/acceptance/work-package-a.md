# Acceptance installation and operation package A

Status: offline preparation, **not deployed and not authority-representative**.
PR #17 is closed. Source at task start is `40e81937350fcb33ce70d8d15131ebbd91f1d92b`,
tree `d19a64ae264b2e3912add64f15d82559d1c711e9`, identical to the supplied merged
tree. The local commit differs from the supplied master commit; unchanged
PR17 mechanics do not require revalidation merely for that difference.
`../IO` is read-only and is not required by these commands.

No mainnet reads, installs, funding, controller changes or neuron operations
are authorized by this package. Commands below are an approval plan. Do not run
the mainnet commands until their separate bounded authorization arrives.

## Operator decisions

Supply public values in [`operator-inputs.json`](operator-inputs.json). `approved`
means the supplied configuration has been reviewed, not authorization to deploy.
No signing key, identity export or credential file is needed.

| Value | Status/value | Exact consumer | Blocks |
|---|---|---|---|
| Twelve roles | All assignments approved in operator reply | `roles_confirmed`, existing profile role map; build bindings and all constructors | No further role decision |
| Existing signer | Expected `zkkkd-…-wqe`; provider/public principal and authority not established | `operator`; bootstrap controller plan and parked Swap fallback | Constructors and signer verification |
| 10m neuron executor | Temporary **verified operator** approved; actual callable provider remains unresolved | `voting_authority = {principal, kind, executable_route}`; nonce-1 Governance neuron | Executable ordinary governance |
| Temporary exception | Approved for acceptance only | `kind = temporary-acceptance`; native permissions 2/3/4 | Real NNX is a later handover, not an installation prerequisite |
| Acceptance clock | Activation-checkpoint policy approved; final genesis/start seconds await schedule | `genesis_timestamp_seconds`, `acceptance_start_timestamp_seconds`; neuron age and programme reward clock separately | Framework constructors and patron activation |
| 20,000 jUP test bucket | A and B each get 5,000 zero-delay + 5,000 one-year; two principals unresolved | `test_allocations`; nonces 10/11 for A and 20/21 for B | Framework constructors |
| Relay surplus | Temporary `[]` approved and recorded in profile | `relay_surplus_neuron_recipients`; canonical Relay constructor | No initial surplus decision; eventual policy remains separate |
| Existing helper | Purpose/state/pending effects unresolved | `bootstrap_disburser` operator record; transition selection below | Disburser replacement only; other offline work continues |
| D-QUORUM / economics | Settled, preserve profile and economics | Disburser second age recipient; `economics.json` | No new decision |
| Cycles | 3T target each; all current balances unknown | Funding delta sheet | Actual top-up budget |
| Spend approval | None | Funding-sheet row caps and a separate signed-off operation list | Every mainnet spend |

For the authority object use `kind = real-nnx` only when its actual executor can
make `manage_neuron` calls as that principal. An address, inaccessible canister,
or principal copied from production is insufficient. A temporary executor gets
ManagePrincipals, SubmitProposal and Vote (2/3/4), without Disburse/Split.
Grant final 2/3/4, prove a final-executor vote, then revoke temporary 2/3/4 and
prove denial. Retain the original neuron ID/Account and exact exclusions.
The local rehearsal demonstrates this mechanism with owned fixture callers;
it supplies no approval or evidence about a real NNX implementation.

`bootstrap_disburser` requires `{transition, state_and_pending_effects,
source_record}`. The transition is `preserving-upgrade` or
`disposable-helper-replacement`, supported by the supplied records. Unknown
obligations block a complete installation bundle; they do not block the
independent artifact build or local rehearsal.

## Exact genesis and programme policy

`operational.py` constructs a new direct-install genesis from `economics.json`;
it does not relabel the small contested fixture. Supply reconciliation is:

| Role | jUP | Identity / permissions |
|---|---:|---|
| Governance authority | 10,000,000 | Approved executor, nonce 1; 2/3/4; exact one-year delay |
| Reserve transport | 89,979,900 | Acceptance SNS Rewards, nonce 2; Disburse only; zero delay/vesting |
| Operational proposer | 100 | Acceptance SNS Rewards, nonce 3; SubmitProposal only; one-year delay |
| Test bucket | 20,000 | Approved test users' liquid balances and/or zero/year-delay neurons |
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
`derived-accounts.json`; the governance ID awaits its real executor. All three
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

The signing invocation, **after approval**, is `icp canister call
sipaq-hyaaa-aaaas-amyoq-cai manage_neuron --args-file PATH_TO_GENERATED_BIN
--args-format bin --candid PATH_TO_VERIFIED_GOVERNANCE_DID -n ic --identity
CONFIRMED_EXISTING_IDENTITY`. The uppercase arguments are explanatory unresolved
values; no executable script containing these placeholders is emitted. Identity
names do not establish principals. Use the existing provider and public identity
CLI information only; no credentials need to be inspected.

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
For each approved fresh role the command shape is `icp canister install
EXACT_ROLE_PRINCIPAL -n ic --mode install --wasm EXACT_PACKAGE.wasm.gz
--args-file EXACT_ROLE.bin --args-format bin --identity CONFIRMED_IDENTITY`.
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
| 11 | Existing Disburser | Conditional preserved upgrade OR explicitly approved disposable-helper replacement | Same `spoge-…` principal; unarmed |
| 12 | Frontend | Acceptance assets / empty constructor | Root after destination and certified-asset smoke checks |

At every checkpoint compare installed compressed package module hash, controllers,
configuration, cycles/headroom and intended runtime state. Installation starts
timers: Disburser can read/disburse maturity, Faucet can scan/settle and notify
CMC, Relay can fund managed canisters, Historian indexes and factory is enabled,
SNS Rewards starts owner scans. No programme is **not** financial inertness.
Review existing ICP/maturity and pending work before installing any application.

The Disburser helper decision needs its purpose/source or creation record,
stable-state obligations and outstanding asynchronous effects, supplied by the
operator. `identity-handoff.md` is historical: module present, empty service
metadata, private full-neuron denial. Empty service metadata proves no absence
of state. A real stateful Disburser requires its supported preserving upgrade.
Only a demonstrably disposable helper permits an approved reinstall at the same
principal with fresh reviewed arguments; no preliminary uninstall. Unknown
obligations block that replacement, not independent preparation.

Before treasury or Relay activation, verify the existing neuron controller,
staking bytes, cached stake versus Ledger balance, dissolve/age/maturity/follow
settings and pending effects through an authorized supported route. Nonce zero
is only an expectation. The treasury worker uses public-full-neuron
`list_neurons`; private operator visibility does not satisfy it. A Public neuron
visibility mutation through its real controller needs separate approval. Relay's
own neuron lookup must succeed under its actual caller and API shape as well.
No new broad management proxy is proposed.

Rehearse ordinary upgrades and normal runtime while bootstrap control remains.
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
[`live-read-plan.json`](live-read-plan.json) for the finite next authorization.
The next inventory is capped at 65 pre-installation read attempts with no retry
(63 fixed reads plus two participant-balance reads once their principals are supplied);
18 post-installation checks are a later, separately authorized stage. No read
approval includes installation or a financial operation.
Primary small-test provisioning is capped in the draft at 6.0007 ICP, including
up to 0.0002 ICP for funding the two donor wallets if needed. The optional child
adds a separate 3.0001 ICP, for 9.0008 ICP maximum. These are proposed caps, not
observed deltas or spending authorization; cycles purchases remain unpriced.
[`relay-surplus-plan.json`](relay-surplus-plan.json) records the separate proposed
eventual acceptance IO/Jupiter recipients and exact memo bytes. IO's checked-in
draft lacks its permanent neuron's actual staking bytes; that Account is
explicitly unresolved, not derived from the neuron ID. No IO files were changed.
Actual observations and owner decisions must replace unknowns before any live
operation package is approved. Build and test evidence is recorded separately
in `validation-a.md`; no local result is a mainnet observation.
