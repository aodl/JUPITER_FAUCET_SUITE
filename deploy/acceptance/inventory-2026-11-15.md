# Acceptance actual-state inventory — 15 November 2026

The approved public inventory completed at **00:38:51–00:39:03 UTC** using
source `a7c6cabdb58c3f2b1f26b3654905df24bb131826` and an explicitly anonymous
`@icp-sdk/core 5.0.0` agent (`2vxsx-fae`). No identity store or signing credential
was accessed. No restricted provider was supplied or inferred from controllers.
No mainnet mutation, post-installation check, retry or additional method occurred.

**38 logical read items completed; 27 were not attempted.** Transport recorded
40 HTTP reads: 26 requested certificate paths, ten application queries, three
SDK certificate reads for query-signature verification and one subnet node-key
read. One topology item reused existing certificates without a new request.
This is below 65 even counting every HTTP read. There were no failed attempted
items and no denied anonymous probes. The runner's transport rejected update
requests, out-of-scope targets/methods, redirects and duplicate requests.
Per-item timeout was 30 seconds, total deadline 1,200 seconds; actual duration
was approximately 12 seconds. The completed approval is not a standing permit
to repeat observations or consume unused slots on other methods.

## Verified canister observations

C1 = `amzih-bssz4-twanl-zf6nr-bqsnn-d7rlt-eerbc-oe7vv-uggn5-3naqo-3ae`.
C2 = `zkkkd-i34qc-367ln-e2u7o-ezznu-dkfqh-gtfvz-cviph-6qa4v-evtfs-wqe`.
Both are certified controllers of all twelve canisters. C2 matches the expected
operator principal; this proves controller membership, **not access to its
signing provider or ability to sign**. Do not select C1 as a substitute signer.

| Role | Principal | Installed module hash | Controllers | Main/reserved cycles |
|---|---|---|---|---|
| sns_root | `sbmlm-rqaaa-aaaas-amypa-cai` | Certified **Absent** | C1, C2 | Unknown |
| sns_governance | `sipaq-hyaaa-aaaas-amyoq-cai` | Certified **Absent** | C1, C2 | Unknown |
| sns_ledger | `sgnny-4iaaa-aaaas-amypq-cai` | Certified **Absent** | C1, C2 | Unknown |
| sns_index | `x2wx5-xaaaa-aaaas-amyqa-cai` | Certified **Absent** | C1, C2 | Unknown |
| sns_swap | `x5xrj-2yaaa-aaaas-amyqq-cai` | Certified **Absent** | C1, C2 | Unknown |
| jupiter_disburser | `spoge-kaaaa-aaaas-amyoa-cai` | `03e8ba94621d28bf166595a9a9dd0d9943c60caad48c7a2740285a7fee283bbe` | C1, C2 | Unknown |
| jupiter_faucet | `xuu2v-mqaaa-aaaas-amyra-cai` | Certified **Absent** | C1, C2 | Unknown |
| jupiter_lifeline | `xiqae-3qaaa-aaaas-amyta-cai` | Certified **Absent** | C1, C2 | Unknown |
| jupiter_sns_rewards | `xgsnm-aaaaa-aaaas-amysa-cai` | Certified **Absent** | C1, C2 | Unknown |
| jupiter_historian | `xbtly-nyaaa-aaaas-amysq-cai` | Certified **Absent** | C1, C2 | Unknown |
| jupiter_relay | `xtv4b-biaaa-aaaas-amyrq-cai` | Certified **Absent** | C1, C2 | Unknown |
| jupiter_faucet_frontend | `xprgq-wiaaa-aaaas-amytq-cai` | Certified **Absent** | C1, C2 | Unknown |

The normal SDK verified mainnet BLS certificates, delegation/ranges and freshness
for each module/controller path independently. Times are client observation
bounds, not an atomic cross-canister snapshot. Certified module absence means
no installed Wasm at that check; it does not prove a never-used canister or its
creation history. The eleven empty-code roles can use explicit **install** mode
once remaining gates pass. None contains the proposed framework/application
package. Disburser's installed hash differs from both raw and gzip acceptance
candidate hashes. All retain bootstrap controllers rather than the eventual
framework/application topology; that is a staging gap, not authority to change it.

All twelve certificates bind to subnet
`bkfrj-6k62g-dycql-7h53p-atvkj-zg4to-gaogh-netha-ptybj-ntsgw-rqe`.
The supported subnet node-key certificate reported **13 nodes**. No new Registry
method or management cost call was added. An actual creation-cost quote remains
unknown; node count alone is not an observed payable creation charge.

## Disburser and NNS state

Disburser's hash matches the historical 7 November observation. Its certified
`candid:service` remains `service : { }`; `git_commit_id` metadata is certified
Absent. Code is installed. **Heap/stable state, bootstrap source/purpose and
in-flight helper effects remain unknown.** Empty service metadata, zero liquid
ICP and an empty neuron maturity queue do not establish disposability.
No source hunt, uninstall or replacement was performed.

The single public `list_neurons` request reproduced the treasury worker's three
fields: exact neuron ID, `include_neurons_readable_by_caller=false`,
`include_public_neurons_in_full_neurons=opt true`. Optional absent fields have
the same wire behavior as Relay's unset pagination/subaccount fields; Relay also
explicitly excludes empty readable-by-caller neurons. This targeted request
returned one full neuron and one page. Its saved raw bytes were decoded offline
with the checked-in NNS subset and didc 0.6.2, without another read.

| Field | Replica-signed observation |
|---|---|
| Neuron ID | `2234355977000034918` |
| Controller | `spoge-kaaaa-aaaas-amyoa-cai` |
| Staking subaccount | `52eb1b31eb9cf6357e2b2e49e32d2238d2b6db2b2f321cbe7b879ea261ac7390` |
| ICP AccountIdentifier, derived from observed bytes | `e458b5ed224bd0b9e884599dd0e986e78d02266674b093b617105528311fa602` |
| Cached stake / separate ICP Ledger query | 100,000,000 / 100,000,000 e8s (1 ICP each) |
| Visibility | Public, native enum `2`; public full record actually returned |
| Dissolve state | Non-dissolving; delay 63,115,200 seconds |
| Created / aging-since | Both `1794065742`; public-info age 637,396 seconds |
| Auto-stake maturity | `None`, effectively disabled; staked maturity absent |
| Unstaked maturity | 54,459 e8s equivalent (0.00054459 ICP equivalent) |
| Neuron fees | 0 e8s |
| Followees | Topics `0`, `4`, `14`, each follows `2947465672511369` |
| Hot keys | Empty |
| Spawn / maturity-disbursement queue | No spawn timestamp; `Some([])` queue |

ID, controller, staking bytes and cached-versus-Ledger stake match the expected
binding. The previously unresolved visibility prerequisite is **satisfied by this
public observation**; no visibility mutation is proposed. The old anonymous
full-neuron denial is historical, not current privacy evidence. Query values
are signature-verified replica observations, **not certified NNS state**. The
new application callers must still pass their ordinary runtime checks after a
separately authorized installation. Preserve existing dissolve/follow/maturity
policy and normal application 95/5 routing. Installing Disburser can start
maturity effects even with rescue unarmed and no Rewards programme.

## Funding observations and proposed deltas

| ICP Ledger Account | Observed e8s | Proposed next deposit, before sender fee |
|---|---:|---:|
| Verified NNS staking Account | 100,000,000 | Two separate 100,000,000 patron tests after readiness |
| Canonical Relay default | 0 | 200,000,000 |
| SNS Governance default treasury | 0 | 200,010,000 |
| Disburser / Faucet / SNS Rewards default | 0 each | No extra standing deposit proposed |
| Participant A / B default | Unknown | Provision only after supplied identities/balances |

ICP Ledger fee was **10,000 e8s**. Existing 1-ICP stake does not count as a new
patron test deposit. Zero default-account balances do not prove zero assets or
obligations in every subaccount. Cycles and reserved balances remain unknown for
all twelve, so every 3T-target top-up delta and the total purchase budget remain
**null**, not 36T newly required. Ledger's extra archive allocation remains a
conditional incremental 1T before its real trigger; no new archive is proposed.

CMC reported `xdr_permyriad_per_icp=15943`, timestamp `1794702840`
(15 November 00:34:00 UTC). Its supplied certificate, freshness, certified-data
hash-tree root and encoded rate leaf verified offline at 00:39:50 UTC with the
normal SDK, with network disabled. At that observation the 13-node basis is
1,594,300,000,000 cycles/ICP. For a known deficit D, the conversion principal
would be `ceil(D / 15943)` ICP e8s before applicable fees; D is unresolved.
This observation is not a guaranteed later conversion quote or spending cap.

The updated [funding sheet](funding-sheet.json) records observed balances and
actual known deltas while retaining **zero authorized spend**. The primary
proposed cap remains 6.0007 ICP (6.0005 without donor provisioning fees), plus a
separate optional 3.0001-ICP child spend, totaling 9.0008 ICP if both are later
approved. Cycles purchases are additional and unpriced. Treasury inventory
retains 1 ICP; only a separately approved native treasury transfer stakes up to
1 ICP plus its fee, with no routing memo, patron jUP or Relay pot. The optional
child needs its actual immutable setup Account and a requirement no greater
than 3 ICP; it is not part of initial installation.

## Unattempted methods

- Twelve `canister_status`: no genuinely authorised existing provider supplied.
  Main/reserved cycles, memory, settings and runtime status remain unknown.
- Twelve `canister_info(num_requested_changes=5)`: no supported existing
  canister-caller/provider route established. No proxy or ingress update tried.
  Code-change history and helper provenance remain unknown.
- One `get_full_neuron`: no confirmed controller/hot-key observation provider.
  Its required public fields were obtained through the separately listed public
  `list_neurons` item; no identity fallback or duplicate method was used.
- Two participant balances: principals not supplied; no guessed Accounts.

The 18 post-installation checks were not executed. No restricted denial was
interpreted as permission to try another identity. Obtaining a provider does not
itself authorize a new live run; any additional reads require explicit approval.

## Exact proposed operations — not executed

[proposed-operations-2026-11-15.json](proposed-operations-2026-11-15.json)
records each target, observed precondition, install mode, exact raw/gzip candidate
hash, future constructor path, timer effects, funding destination/memo and
controller transition. Sender and unresolved constructor hashes remain null.
This is review data, not an executable bundle or deployment authorization.
The prior artifact archive SHA-256 is
`c18f86b639c10bf6c3fbc56df4b09e5fdf9f3653575373e358ae0758f2abacba`.

| Order | Target | Proposed mode | Exact gzip package SHA-256 |
|---|---|---|---|
| 1 | `xiqae-3qaaa-aaaas-amyta-cai` | install | `0688ab5567bd9cdd4c11447380f2e63c7f91629e598086183047fc06dd5f6d70` |
| 2 | `sipaq-hyaaa-aaaas-amyoq-cai` | install | `24ef2004240d2a34bcdaa616134d27bc18d11c1e96b378a46d564feedcc1a8c3` |
| 3 | `sgnny-4iaaa-aaaas-amypq-cai` | install | `354dd6ecfdc72b5409805b31dea22c9db11df6e14095a5a68924eb63535e6d8a` |
| 4 | `x2wx5-xaaaa-aaaas-amyqa-cai` | install | `dab6808d0dfc06e5e88336d0c3d3e45e5448c6e36c2a781f3e9e09bd450f528c` |
| 5 | `x5xrj-2yaaa-aaaas-amyqq-cai` | install | `f3c9cd2a503f3136c91f1d44c5b4bac2d5f15a7d347731aaff03e5fa80df8c48` |
| 6 | `sbmlm-rqaaa-aaaas-amypa-cai` | install | `3762dd6f1348830a1d6b19252ca21f518c9f9d3a2b79b1dd99d75c0de6d46ef4` |
| 7 | `xgsnm-aaaaa-aaaas-amysa-cai` | install | `56babefd47b5c2a95fd5e1b926e77c447a71b571167e4d0b6c537f3d765264a1` |
| 8 | `xbtly-nyaaa-aaaas-amysq-cai` | install | `2bc0797000c52c289dca92252369cc3129f3746bc8c81ccf4acf890ffb7faf9b` |
| 9 | `xtv4b-biaaa-aaaas-amyrq-cai` | install | `d6fc978715d55fea3c417e6b247524808a8ad283354fb6a28c606c416972a9e7` |
| 10 | `xuu2v-mqaaa-aaaas-amyra-cai` | install | `ed661e744e616306dbe00a580e9fe5eb72fb34b19bfa0566f91ed76f1b8449e2` |
| 11 | `spoge-kaaaa-aaaas-amyoa-cai` | **Unresolved preserving upgrade / approved helper replacement** | `473a1bed784cf4734b2173c8d724d1c093ccbd85f427a391b7914284c16cd070` |
| 12 | `xprgq-wiaaa-aaaas-amytq-cai` | install | `6df4b325c73792ca8ee71ae8ce9e3f8b6fe2c354c8b5885347936064ef83bfbc` |

Use the existing `operational.py candidate` guard after supplying the actual
signing principal, A/B principals, timestamps and helper-preservation record.
Then hash and review all seven application and five framework constructors.
The future signing command uses `icp canister install` with the explicit target,
mode, matched `--wasm`, `--args-file`, `--args-format bin`, `-n ic` and the
confirmed existing provider. No `auto`, bulk deploy, new canister or preliminary
uninstall is proposed. No complete command containing invented values is emitted.
For Disburser choose exactly one supported preserving upgrade or explicitly
approved disposable-helper replacement at the same principal; neither is yet
selected. Stop if any expected physical module/controller precondition differs.

Retain both observed bootstrap controllers during installation/rehearsal unless
an explicit replacement is approved. After the actual governed-upgrade and
configuration checkpoints, framework Governance/Ledger/Index/Swap and the
SNS-managed applications (including Lifeline) become Root-controlled; Root
becomes Governance-controlled. Each final replacement explicitly removes C1
and C2; approve those removals separately after proving the governing route.
Faucet/Disburser instead stage verified operator+self, retain unarmed rescue,
then use a separately approved same-code upgrade to arm the existing lifecycle
only after ordinary runtime/rescue rehearsal. Healthy reconciliation reaches
self-only; broken flow permits self+Lifeline. Do not put them under Root or
invent a generic Lifeline repair API. Factory children, if later approved,
finish controllerless. The JSON spells out the full proposed sets per role.

Cycle top-ups would use the installed CLI's `icp canister top-up TARGET --amount
DELTA -n ic --identity VERIFIED_PROVIDER`, after real deficits and source/funding
route are approved; DELTA/provider are unresolved, so no runnable script exists.
Financial tests use exact `icrc1_transfer` records in the proposal JSON, including
10000-e8s observed fee, fixed Accounts and memo bytes; source and transaction
identity/timing remain unresolved. Use the generated native Governance upgrades
for recognition, approved patron checkpoint, treasury and bounded continuation.
No direct operator configuration, visibility mutation or NNX handover/revocation
is authorized. IO settlement stays disabled and initial Relay surplus stays [].

## Evidence and validation

Raw requests/responses, verified certificates, query replies, decodes and the
one-shot runner are in `target/acceptance-inventory-20261115/`.
Portable evidence: `target/acceptance-inventory-20261115-evidence.tar.gz`, SHA-256
`26431e244fe929fa80bc3c8897954f5834ba69d8b9a406aed2b7cbcc62c3efc9`.
The archive includes a per-file checksum manifest. No synthetic credentials or
constructor binaries are included. The original preparation artifact is retained
unchanged; these observations are a dated supplement.

Validation performed for this documentation/data change: `node --check` on the
one-shot runner, offline didc decoding of the saved full reply, offline CMC
certification, request/target/budget/no-retry audit, exact package-hash and funding
arithmetic reconciliation, JSON parsing and `git diff --check`.
No canister code, frontend source, lockfile or protected mappings changed; no
build, protocol suite or repeat mainnet inventory was needed. Prior local test
results are in [validation-a.md](validation-a.md). Deployment classification:
**NO FRONTEND CHANGE**. All mutations remain proposed for separate approval.
