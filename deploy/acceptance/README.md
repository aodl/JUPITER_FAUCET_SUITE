# Jupiter acceptance SNS

This is offline preparation for a separate, directly installed Jupiter SNS on the
real IC. It authorizes no installation, funding, neuron operation or controller
change. Production `ic`, its mappings and installation arguments retain their
existing meaning. In particular, production SNS Rewards still uses its reviewed
OpenChat binding. Acceptance never uses that binding.

The application and framework tests use Jupiter infrastructure. IO revision
`8b2e1cba06b43bc587898343a0391a6fac7eca23` supplies the read-only settlement
contract; no build, test or script needs its checkout.

## Inputs and isolation

[`profile.json`](profile.json) is the canonical acceptance identity input. It contains the confirmed Disburser
`spoge-kaaaa-aaaas-amyoa-cai` and exact NNS neuron `2234355977000034918`.
The operator explicitly delegated selection of the other eleven roles; all twelve
assignments are recorded in the profile and the [identity handoff](identity-handoff.md).
`provisioning.json` retains the original inventory and delegation provenance.
No existing mapping was inferred from ID ordering. Role isolation is independently
validated with `profile.py check-roles`. Operator, production NNX authority, launch
timestamp and reviewed genesis/Relay surplus inputs remain unresolved, so full
constructor preparation still fails closed. The
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
and neuron IDs remain rejected. The exact D-QUORUM recipient is encoded in the checked profile. The canonical
Relay surplus policy remains unresolved; constructor preparation authorizes no
real ICP donation.
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
# Role isolation succeeds; full preparation still requires the unresolved inputs:
python3 tools/acceptance/profile.py check-roles deploy/acceptance/profile.json

# Expected to fail until operator and installation inputs are reviewed:
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

The retained compatibility PocketIC case uses its explicitly historical small
constructor fixture (including its historical fees/bonuses), not the agreed launch
economics. The new programme and sale cases consume `economics.json`. The
compatibility case uses the verified official Governance, Root, Ledger,
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

## One economic contract, two launch paths

[`economics.json`](economics.json) is consumed by the launch renderer, operational
fixtures, Rust programme and economic tests. It conserves exactly 100 million
jUP at eight decimals: 10 million governance, 10 thousand sale, 100 operational
proposer and 89,989,900 reserve transport; built-in SNS treasury starts at zero.
The transfer fee is 0.1 jUP, rejection charge 1 jUP, and native reward rates and
voting-power age/delay bonuses are zero. Native participation events remain active.
The Julian year is 31,557,600 seconds; four years is 1,461 days.

The 10-million neuron is intended for NNX authority, whose implementation and
live principal remain external dependencies. It is separate from the canister's
SubmitProposal-only operational neuron. Both and the reserve transport are
excluded from external reward shares. Excluding the protocol-only proposer is
an explicit operating recommendation adopted by the fixtures. Replenishment
uses 20/100 jUP watermarks, authentic outstanding charges and permissionless
native stake refresh. It sponsors protocol operations only.

The six installed components are pinned individually in
[`sns-framework.json`](sns-framework.json). The official SNS CLI, testing bundle
and SNS-W deployment validator have independent source/artifact pins in
[`launch-tools.json`](launch-tools.json). Maintained upstream constructors and
validators run against owned PocketIC infrastructure; no third repository or
IO Wasm is a dependency.

`production_launch.rs` validates the exact production allocation with the official
NNS/SNS configuration converter. The large developer neurons are accepted by
this pinned path; the historical developer-versus-swap restriction is not
present. Equal one-year minimum/maximum voting delays are accepted. An initial
10 ICP minimum buyer is authentically rejected for the two-neuron basket and
1 jUP minimum stake plus fees. The supported minimum is 22 ICP. This buyer
minimum does not change the 10,000–100,000 ICP total sale range.

`sale_rehearsal.rs` deploys through real SNS-W, opens the real Swap, pays local ICP,
and finalizes successful and failed sales. It verifies participant neurons,
refunds, treasury receipts, physical controllers, fees and supply. The supported
basket has zero-delay and one-year members; not all participant stake qualifies
for Jupiter's exact-year bonus. The canister-controlled zero-delay, zero-vesting
reserve neuron disburses through native Governance into the SNS Rewards reserve
Account after Normal mode. The reserve receives its allocation minus the actual
native disbursement fee. No arbitrary direct Ledger allocation is presented as
production launch support.

Long-lived acceptance retains the direct-installed Normal SNS and parked Swap.
Its real Ledger/governance/controller behavior is useful operational evidence,
but it has no sale history or authentic SNS-token sale valuation. Its small contested ownership
fixture is not the production participant distribution. Native SNS-token treasury/mint valuation belongs in the sale rehearsal.
ICP-treasury valuation uses the real ICP Ledger and CMC, independently of Swap,
as the [pinned valuation source](https://github.com/dfinity/ic/blob/98c898f8de068a1fc0e97ebc805f9fa7f315d862/rs/sns/governance/token_valuation/src/lib.rs)
shows. The sale test reinvests actual sale proceeds; a funded operational ICP
treasury need not invent sale history. No fake price, sale proceeds or
Governance mode transition substitutes for finalization.

## Implemented programme and remaining boundaries

The SNS Rewards [programme documentation](../../canisters/sns-rewards/README.md)
defines discovery, final contribution assessment, fixed-point patron rewards,
dispatch uncertainty, reserve custody, mirror bindings and participation shares.
New installations can explicitly configure these modules. Existing installations
retain their deployed root and stable state and enable no new transfers through
an upgrade default. Production's OpenChat installation binding is untouched.

Two real official SNS instances exercise union ownership, direct liquid and
compounded patron transfers, permissionless refresh, canonical Yes/No mirror
ballots and observable target application settings. A deliberate Ledger fault
boundary covers uncertain transfers that actually paid and actually did not pay.
An actual post-dispatch callback trap plus upgrade exercises the durable message
boundary. Those fault tests are not mocked Governance or Swap outcomes.

Motion mirroring asks whether Jupiter should vote Yes on the bound target.
Canonical adoption means Yes, rejection means No, and open/unavailable means no
vote. Full official action DTOs and installed Governance hashes protect the
content digest. Targets, Roots, neurons and proposer are configured and bound;
caller-supplied titles cannot create a mapping. Unknown submissions reconcile
canonical identity, and unknown ballots reconcile the real target ballot.

Jupiter ordinary voting is five days; target fixtures use six. Timing admission
includes the source wait-for-quiet extension bound, discovery and execution
margin. The shared proposed operating default is a positive one-hour native quiet
interval, with a one-hour delivery margin. A real late-vote extension test crosses
the original source deadline while it remains open, verifies no target ballot,
then delivers the canonical rejection. Ordinary five/six-day cases retain room
for the bounded native extension. Larger quiet intervals, critical proposal rules
and late discovery still require actual deadline admission; six days is not a
universal guarantee. NNX's suggested four-day period is documentation only.

The native treasury worker uses the same proposer, authenticated balances,
seven-day/value limits and one durable operation per reviewed configuration.
It proposes ICP to canonical Relay subaccount 1 and verifies the actual Ledger
transfer. Rejection/failure requires reviewed reactivation; ambiguity blocks
ordinary reactivation. This first implementation stops verification at the Relay
credit. Endowment forwarding and mixed patron/treasury provenance are not yet
integrated into its terminal proof. Existing Relay memo/fee behavior and the
normal 95% SNS Rewards / 5% D-QUORUM age-bonus split remain unchanged.

**Integration limits:** patron history pauses at canonical Relay subaccount-1
commitments pending exact FIFO/splitter attribution; it does not mark those
commitments ineligible or pay the canonical Relay principal. Other Relay-child
and splitter routes are not recognised by this guard. General patron activation
must remain off until those routes are integrated. Combined automatic patron and
treasury activation is rejected until mixed-batch provenance exclusion is
implemented. Direct patron processing, discovery, reserve transport, governance
mirroring and participation collection remain independently testable. This
branch therefore is not production activation readiness.

## IO contract handoff

[`io-handoff.json`](io-handoff.json) records the exact proposed Accounts with the
assigned SNS Rewards principal, known reserved IO acceptance components and the
read-only source pin. It is disabled and records no deployment observation.

The independent collector reads official per-neuron event participation shares,
checks complete/coherent pagination, removes protocol roles before the denominator
and publishes atomically. Shares already contain voting power; no second stake
multiplication is applied. A zero-weight event leaves value unallocated. The gross
share query is a mathematical quote, not an IO entitlement or funded payout.

`io_contract.rs` tests the public IO interface at the pinned read-only revision:
exact ICP deposit Accounts/fee/time/block floor, `notify_jupiter_deposit`, Pending
and Completed results, exact 40/60 net-of-two-fees settlement and the actual IO
receipt Account, block, amount, fee and receipt-sequence memo. Duplicate receipts
credit inventory once. Uncertain ICP or Pending conversion credits nothing.
This is a contract-faithful interface test, not a run of the real IO application.
No ICP forwarding or IO distribution worker is activated.

The minimal IO-side change is to bind `jupiter_account` to **SNS Rewards' default
ICP Account**, which receives age-bonus ICP, rather than the production template's
Faucet source. `jupiter_staging` remains the IO NNS Manager default ICP Account.
Bind `jupiter_io_account` to an explicitly reviewed SNS Rewards-owned IO Account,
and pin the activation floor to an authenticated ICP Ledger boundary before the
first transfer. Confirm actual IO Ledger, manager and Stream Manager reserve
Accounts and native settlement receipts. Jupiter must not spoof Faucet or alter
IO accounting. IO payout fees/emissions still need a reviewed policy; patron's
sponsored jUP fee does not authorize unlimited IO fee subsidies.

There is no mainnet installation, transfer, controller change or neuron action
in these tests. Hosted CI provisions the pinned official artifacts for the SNS
Rewards PocketIC lane; ignored real-SNS tests are explicitly discovered by xtask.
