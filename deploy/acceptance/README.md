# Jupiter acceptance SNS

This is offline preparation for a separate, directly installed Jupiter SNS on the
real IC. It authorizes no installation, funding, neuron operation or controller
change. Production `ic`, its mappings and installation arguments retain their
existing meaning. In particular, production SNS Rewards still uses its reviewed
OpenChat binding. Acceptance never uses that binding.

Source baseline: Jupiter `fb231f5f45dc7cdbec50267931db6dc70e10ddc3` (clean), IO
`8b2e1cba06b43bc587898343a0391a6fac7eca23` (untracked `results.txt` left alone).
IO was read-only. No build, test or script here needs its checkout.

## Inputs and isolation

[`profile.json`](profile.json) is the canonical acceptance input. Uncreated
application/SNS identities, operator, NNS neuron and unsettled economics are
`null`, not fake deployment principals. Fill these only from actual provisioning
records after separate authorization, then commit the reviewed inputs. The
`acceptance` CLI environment uses network `ic` and all twelve roles, separately
from production. A Ledger archive is a later-created component, not an initial
thirteenth role.

`tools/acceptance/profile.py` rejects protected Jupiter and IO application
identities in every application/SNS role, shared system canisters used as
application identities, identity collisions, protected NNS neurons and protected
financial Accounts. It derives the Jupiter staking Account from the acceptance
Disburser and its reviewed nonce. The supplied neuron ID must be verified against
that actual NNS neuron, controller and staking Account before installation.

The second age-bonus recipient may explicitly select the shared D-QUORUM
beneficiary: NNS Governance `rrkah-fqaaa-aaaaa-aaaaq-cai`, subaccount
`77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f`.
This is the reviewed production Disburser's 5% age-bonus recipient, not IO's
protected permanent neuron. No other Governance Account is allowed in that role,
including protected Jupiter/IO staking Accounts. Protected application principals
and neuron IDs remain rejected. Both recipient policies stay unresolved in the
checked profile; constructor preparation authorizes no real ICP donation.
Any real funding requires separate explicit authorization.

| Executable boundary | Acceptance binding | Production behavior |
|---|---|---|
| Faucet remainder | Validated profile's canonical Relay at build time | Existing protected production mapping and override-consistency check |
| Relay endowment and reward attribution | Explicit acceptance NNS neuron at build time | Existing production NNS neuron |
| Factory children | Historian's configured SNS Rewards Account owner and ICP Index, explicitly encoded | Same services, now explicit |
| Historian accounts and canonical targets | Explicit generated arguments and acceptance build defaults | Existing defaults |
| SNS Rewards | Acceptance jUP Root only; Governance/Ledger are resolved from Root | Existing installation and upgrade contracts |
| Disburser | Explicit neuron, recipients and Lifeline in generated arguments | Existing installation and upgrade contracts |
| Frontend | Validated build profile for staking Account, neuron, Relay and Historian; rendered links follow it | Existing trusted production build configuration |

These are separate acceptance executables, not a production-ID override or a
stable-state migration. Do not move an installed canister between environments or
retarget recorded identities. Preserve the same bindings on subsequent builds.
Production identity guards on debug APIs remain production-bound even when the
artifact is built for acceptance. Pending payment identities and recovery journals
are unchanged.

The Historian `rewards_account.owner` identifies the SNS Rewards service as well
as the indexed age-bonus receiver. Acceptance generation pins both roles to that
same canister. Children never inherit a production SNS Rewards default.

Shared NNS Governance, ICP Ledger/Index, CMC, SNS-W and XRC are intentional shared
infrastructure. Recognized blackholes remain status compatibility services; the
acceptance canonical managed set does not fund the production blackholes.
Explicit user-supplied community memo targets retain the existing public protocol
semantics. Published global aliases are not environment-specific identities:
`D` remains its immutable published identity and grants no `D.` payment route;
it must never be reassigned to an acceptance Disburser. Event Horizon wakeups are
optional hints, never payment authority. No acceptance subscription is created.

The frontend's unresolved IO destination is disabled, including its static
production neuron/account links and route-history lookup. Payment destinations
still come only from trusted build inputs or authenticated Historian responses.
Acceptance asset builds omit production custom-domain declarations without
changing the production source file.

## Local commands

The tools use Python's standard library, existing `didc` and `ic-wasm`, and the
locked workspace. There is no SNS deployment service or on-chain coordinator.

```bash
# Expected to fail until actual identities are supplied:
python3 tools/acceptance/profile.py check deploy/acceptance/profile.json

# Verify a supplied exact tuple, or explicitly fetch its pinned official bytes:
python3 tools/acceptance/artifacts.py --fetch
# The command prints a content-addressed directory outside source. Export it:
export JUPITER_SNS_ARTIFACT_DIR=/path/printed/by/the/command

PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/acceptance -p 'test_*.py' -v
POCKET_IC_MUTE_SERVER=1 cargo run --locked -p xtask -- acceptance_sns

# After reviewing and completing the real profile:
python3 tools/acceptance/prepare.py applications deploy/acceptance/profile.json target/acceptance/constructors/applications
python3 tools/acceptance/prepare.py sns deploy/acceptance/profile.json "$JUPITER_SNS_ARTIFACT_DIR" target/acceptance/constructors/sns GENESIS_UNIX_SECONDS
python3 tools/acceptance/build.py deploy/acceptance/profile.json target/acceptance/artifacts jupiter_historian
```

`prepare.py` refuses existing output directories, writes typed Candid and binary
constructors, Accounts, frontend bindings, controller expectations and hashes.
Each constructor is encoded against its exact official interface, decoded and
encoded again. Governance, Ledger and Swap fees/minimum stakes must agree.
Application constructors use the checked-in production interfaces. The local
fixture has an explicit `local-fixture-not-for-deployment` purpose; CLI handoff
rejects it. `--debug-api` builds are restricted to such local fixtures.

`build.py` uses the normal locked Rust/Wasm toolchain, embeds its freshly built
acceptance Relay in Historian, and keeps outputs outside `release-artifacts/`.
Frontend templates are restored even on a failed build. These are development
artifacts, not canonical production release hashes. Canonical production release
verification remains the existing Docker flow.

The real PocketIC case uses the verified official Governance, Root, Ledger,
Index and compatible parked Swap; it exercises owner snapshots, actual jUP
transfers, Ledger deduplication, explicit factory child configuration, immutable
children and an adopted native application-upgrade proposal. NNS Governance is a
local mock at the separate NNS boundary; ICP Ledger/Index/CMC are owned PocketIC
system fixtures. The existing reward ambiguity tests remain the bounded-failure
evidence. Timestamp hooks in the controller rehearsal test the existing rescue
policy; they do not establish financial health or implement Lifeline repair.

## Before any mainnet installation

The initial **main-cycles target is 3T per canister**. This is neither a 30-day
availability promise nor an installation-cost calculation. Historian factory
tests have extra local headroom. Ledger archive creation has its own explicit
`archive_cycles` value; provision it before the archive trigger is reached while
retaining operating reserves. No automatic funding is added.

An operator must independently verify the twelve created IDs, initial control,
subnets, actual cycles, NNS neuron and every financial Account. Record the IDs in
the profile and generate the CLI's `acceptance.ids.json` from exactly `roles`;
do not edit or copy the production mapping. The build handoff rejects mapping
drift and non-reviewed/fixture genesis. Do not use group `icp deploy`: installation
order, explicit per-role binary `--args-file ... --args-format bin`, reviewed
controller handoff and artifact observations are separate operator actions.

The checked CLI environment does not reference ungenerated argument files:
icp 1.2 reads those eagerly even when another environment is selected. Generated
constructors must instead be supplied explicitly to each reviewed install.
Never use production install files for acceptance. For large modules use the
CLI's supported install/chunk facilities with the target's own chunk store;
the store and target must be on the same subnet and the concatenated module and
ordered chunk hashes must match the reviewed artifact. See the [management
contract](https://docs.internetcomputer.org/references/management-canister/).
No install/chunk upload has been performed by this preparation task.

The generated controller plan is an expectation, not evidence of deployment:

- Bootstrap under the verified operator; do not arm autonomous rescue yet.
- Governance is controlled by Root; Root by Governance; Ledger/Index/Swap by Root.
- SNS-managed Jupiter services may be handed to Root after normal upgrades work.
- Faucet/Disburser require an explicit reviewed self-controller/arming transition.
  Healthy control is self-only; broken flow permits exactly self plus Lifeline;
  healthy flow narrows it again. Keep no operator backdoor.
- Lifeline remains a minimal reserved recovery principal with no speculative API.
- Canonical Relay retains replacement-style upgrade semantics. Factory children
  end with no controllers and cannot be repaired through Root or Lifeline.

Root discovery accepts the single build-configured acceptance Root only after
its live Root/Governance/Ledger membership checks. It creates no fictitious SNS-W
record. Normal registered-SNS discovery and direct status probing remain active.
Verify physical module hashes, controllers and Root membership after each step;
a successful command alone is insufficient deployment evidence.

## jUP decisions that source cannot settle

The `jUP` name in the UI establishes intent, not a complete Ledger/genesis or
issuance contract. The local fixture is deliberately small and has zero native
staking inflation. None of its quantities are proposed production economics.

| Missing input | Existing source | Options and recommendation |
|---|---|---|
| Supply, decimals, distribution and treasury custody | No executable jUP genesis | Review explicit allocations and decimals before creating mainnet constructors; do not infer supply from marketing copy |
| Endowment-to-jUP issuance and authority | Relay distributes an available reward pot using completed endowment/owner snapshots; general issuance is absent | Decide a bounded prefunded allocation or an actual minting programme and launch mechanism; keep the latter blocked until its valuation/authority contract is established |
| Native SNS mint/treasury operations | Pinned Governance values SNS tokens through Swap | Direct Normal genesis with a parked zero-sale Swap does not establish this valuation; use a real compatible launch if required, not fabricated sale proceeds |
| Proposer/voters, stake, permissions and voting parameters | Owner selection is a heuristic, not voting authority | Specify independently of the operator; do not inherit IO's decisive human voter or 100M allocation |
| Native SNS staking rewards | Not determined by SNS Rewards' owner index or planned IO rewards | Explicitly choose zero native inflation or a reviewed schedule; local zero is test data |
| Second age-bonus recipient and canonical Relay surplus policy | The exact shared D-QUORUM Account is an allowed external beneficiary; protected Jupiter/IO resources remain isolated | Explicitly review the second recipient, with shared D-QUORUM preserving production intent; select acceptance Relay recipients separately. Neither is inserted into the unresolved profile |
| Archive funding | Ledger constructor requires a separate creation budget | Review an affordable explicit value and pre-trigger funding gate; 3T main cycles does not fund every possible archive configuration |

The exact official tuple is in [`sns-framework.json`](sns-framework.json),
initially taken from IO's reviewed SNS-W version after NNS proposal 144202. It
intentionally spans multiple upstream commits. Verification checks all six
compressed/raw hashes, embedded DIDs and source revisions. No framework upgrade
or new launch mode is selected implicitly.

The pinned Governance `proposal.rs::validate_and_render_mint_sns_tokens` calls
the treasury valuation path before accepting a native mint. Its sale-free
limitations apply even in Normal mode. The integration reward pot is a genuine
genesis-funded Ledger transfer, not evidence of the planned issuance programme.

### Narrow issuance choice under the pinned framework

The [production frontend](../../canisters/frontend/public/index.html) describes patron jUP airdrops for qualifying ICP
endowments and IO rewards for jUP stakers. Those are product intentions, not an
implemented issuance formula or native SNS inflation schedule.

| Approach | Authority and supply | Parked Swap compatibility | Minimum additional work |
|---|---|---|---|
| Initially funded jUP distribution reserve | Explicit genesis allocation to a non-minting Account; its custodian distributes existing tokens, without increasing supply | Yes for a dapp-owned reserve. Native SNS-token treasury releases also require valuation, so do not assume a Governance treasury reserve can be released with this Swap | Review allocation/custody and patron eligibility/pacing; implement the missing allocator and governed releases. Existing Relay pots support only their existing SNS-owner attribution policy |
| Ongoing new jUP tokens | Governance's default Account is the Ledger minter; adopted native `MintSnsTokens` proposals create new supply | Native mint validation requires real Swap-derived valuation; the parked Swap fails this check | Decide whether ongoing patron minting is required, then review a compatible launch and issuance policy/allocator. No fabricated sale or price |
| Native SNS voting rewards | Governance credits voting maturity under reviewed reward parameters; maturity disbursement mints jUP, independently of patron allocations | The pinned Normal-mode reward/maturity paths do not use Swap valuation; this is source-supported, not a nonzero-rewards lifecycle result from our zero-inflation fixture | Review inflation and maturity policies, configure them and test actual reward settlement/disbursement; no Jupiter minting service is required |
| IO rewards to jUP stakers | IO's settlement authority issues/routes IO under its own contract; this creates no jUP supply | jUP Swap valuation is irrelevant to IO settlement | Agree and implement the ICP sender, IO settlement destination and staker distributor described below; this flow is absent today |

The [pinned Ledger](https://github.com/dfinity/ic/blob/cf41372e3d4dc1accfe2c09a7969f8bddc729dc1/rs/ledger_suite/icrc1/ledger/src/main.rs)
treats transfers from the minting Account as mints and transfers to it as burns.
Constructors therefore reject initial allocations there and reject duplicate
effective Accounts, including default/zero aliases and neuron-staking collisions.
Native mint/treasury valuation checks are in the
[pinned proposal validator](https://github.com/dfinity/ic/blob/98c898f8de068a1fc0e97ebc805f9fa7f315d862/rs/sns/governance/src/proposal.rs).
The [pinned Governance](https://github.com/dfinity/ic/blob/98c898f8de068a1fc0e97ebc805f9fa7f315d862/rs/sns/governance/src/governance.rs)
also reserves the Ledger from generic-function targets: a generic Ledger mint
proxy is not a supported shortcut around native mint validation. Moving the
minting Account away from Governance would change the accepted SNS contract.

The narrow choice is a bounded initial patron-distribution allocation versus a
programme requiring ongoing new patron tokens. Decide native voting inflation
separately; a finite reserve alone does not establish a hard total-supply cap.
IO rewards are a separate integration in either case. No rate, cap or inflation
schedule is selected by this preparation.

## Small shared contract and extraction seam

The project supplies identities, explicit genesis parameters/balances/neurons,
permissions, registered applications and expected controller policy. Reviewed
inputs are the six-artifact lock and project configuration. Outputs are typed
constructor bytes, exact Account encodings and input/hash records. Observed
modules/controllers/Root membership must be recorded separately from expectations.

The neutral extraction boundary is `ic.py` (encodings), `candid.py` (typed
round trips), `sns.py` (artifact verification and direct constructors), and
`artifacts.py` (explicit pinned fetching). The IO vote-only neuron and Jupiter
fixture vectors exercise the same constructor code; neither allocation is a
shared default. IO's Rust adapter still includes its frozen reserve, redemption,
governance and authority policies. Migrating it requires a deliberate adapter
change. Both projects do **not** yet consume one versioned package, so this is
an extraction seam, not completed cross-project standardization. Keep Jupiter's
environment, financial accounts, build bindings and rescue model in its adapter.

## Next phase: Jupiter-to-IO ballots

Use the pinned standard `AddGenericNervousSystemFunction` and
`ExecuteGenericNervousSystemFunction` proposal path, with Jupiter SNS Rewards as
the fixed validator/executor dapp. No such API or state is added in this task.

Proposed payload: exact IO Governance principal, exact designated governance-only
IO neuron bytes, IO proposal ID, explicit `Yes` or `No`. Validator and executor
must accept calls only from the configured Jupiter Governance and reject every
other target/neuron/action. Execution translates only to IO
`manage_neuron(RegisterVote { proposal; vote })`; it is not a management proxy.

Persist one bounded execution record per relevant IO ballot: payload digest,
target proposal/neuron, requested vote and dispatch/outcome evidence. The standard
executor receives the payload, not a trusted Jupiter proposal ID; do not require
an invented caller-supplied proposal identity. An identical request reconciles; a conflicting
ballot fails. After an ambiguous reply, read the canonical IO proposal ballot
through an authenticated replicated call before permitting a retry; fail if a contradictory ballot or
closed deadline makes the instruction impossible. An adopted instruction is not
successful merely because it dispatched. A rejected Jupiter proposal sends no IO
ballot and must never be represented as an IO `No` vote.

The pinned `canister_control.rs` treats **any successful executor reply** as
successful governance execution, without decoding an application `Result`.
The future executor must reply only after the ballot is confirmed; explicitly
reject failures/unknown outcomes while preserving its journal. Returning
`Result::Err` as a successful reply would falsely mark the proposal executed.
The validator instead returns the standard typed `Result<text, text>` rendering.

IO's designated 100M governance-only neuron is excluded from claim backing and IO
staking rewards. Its voting stake creates no equivalent Jupiter ICP obligation.
Acceptance must grant the distinct acceptance SNS Rewards principal the actual
Vote permission; IO's existing human fixture voter is not the final integration.

Before implementation, set the deadline contract: IO must remain votable through
Jupiter's full voting and wait-for-quiet windows plus execution/reconciliation
margin. IO's reviewed acceptance voting period is 86400 seconds with a 43200
second wait-for-quiet increase; an immediate local majority does not solve this
production timing problem. Tests must include real adoption/registration,
unauthorized/conflicting/replayed instructions, ambiguous delivery with canonical
ballot reconciliation, failed execution and deadlines. No general bridge is needed.

## Next phase: ICP and IO settlement

Current source proves these distinct paths:

1. Disburser routes its reviewed age-bonus share to SNS Rewards' default ICP
   Account. SNS Rewards currently indexes owners; it does not forward ICP or
   distribute newly received IO.
2. IO NNS Manager authenticates an exact ICP block from configured
   `jupiter_account` to its own default `jupiter_staging` Account, including
   amount, fee, creation time, no spender and the activation floor. Both Accounts
   must be defaults. The production template names Faucet as the source, which
   is not the SNS Rewards receiver above.
3. IO's retained Jupiter operation and Stream Manager receipt machinery produce
   settlement to configured `jupiter_io_account`; the production template still
   names a Faucet-owned IO Account with an unresolved subaccount. This does not
   identify a working Jupiter IO-distribution implementation.

The smallest proposed alignment is **SNS Rewards default ICP Account -> IO
acceptance NNS Manager default ICP Account**, with settled IO returned to a
separately approved SNS Rewards-owned IO Account. This is a proposed contract,
not active configuration. IO would need its source role aligned deliberately;
Jupiter needs a narrow, authorized forwarding/settlement/distribution capability.
Do not reroute funds through Faucet merely to fit the old template.

Agree the exact receiving IO Account and distributor before activation. A
Jupiter transfer must retain the exact returned ICP block and pinned identity;
IO authenticates the Ledger block independently. Resume IO's existing deposit/
receipt operation by that block after partial execution, never create a second
deposit to resolve an unknown reply. Pin the activation floor to a verified
Ledger boundary before the first eligible transfer. Both projects' Ledgers,
Index, NNS Manager, Stream Manager, SNS authority, SNS Rewards owner snapshot
and approved IO recipient must be installed and verified first. Separate any
future IO-distribution liability/recovery contract from Relay's existing jUP
best-effort reward pot; these are different tokens and responsibilities.
