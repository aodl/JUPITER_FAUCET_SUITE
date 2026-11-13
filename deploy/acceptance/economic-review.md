# Reviewed Jupiter launch economics

This is a launch candidate, not deployed configuration or a yield forecast.
The source of truth is `economics.json`. Official SNS Governance is pinned to
`98c898f8de068a1fc0e97ebc805f9fa7f315d862`; component, converter and SNS-W hashes
remain in `sns-framework.json` and `launch-tools.json`.

## Native parameter comparison

“Explicit” means rendered into the supported official launch input. “Inherited”
means the pinned converter's actual Governance default, verified in the SNS-W
rehearsal. G means native `ManageNervousSystemParameters`; L means native
`ManageLedgerParameters`. Governance can change supported parameters subject to
native validation, approval and criticality rules. Sale terms are fixed for that
sale; changing an open sale is not an application configuration operation.

| Field / policy | Previous | Candidate | Launch support / origin | After launch / implication |
|---|---:|---:|---|---|
| Initial supply, eight decimals | 100,000,000 jUP | unchanged | Explicit distribution | No added allocation; native reward purse is zero |
| Governance-only neuron | 10,000,000 jUP | unchanged | Explicit distribution | Keeps native governance power; exact ID excluded from external rewards |
| Sale allocation | 10,000 jUP | 20,000 jUP | Explicit distribution | Fixed sale inventory |
| Reserve transport | 89,989,900 jUP | 89,979,900 jUP | Explicit distribution | Disbursement fee deducted, no extra supply |
| Operational proposer | 100 jUP | unchanged | Explicit distribution | 20/100 replenishment policy unchanged |
| Built-in token treasury | 0 | unchanged | Explicit distribution | Distinct from ICP treasury and dapp reserve |
| `transaction_fee_e8s` | 10,000,000 | 1,000,000 | Explicit Token / SNS Init | L; runtime queries remain authoritative |
| `proposal_reject_cost_e8s` | 100,000,000 | unchanged | Explicit Proposals | G; 1 jUP rejection cost |
| `neuron_minimum_stake_e8s` | 100,000,000 | unchanged | Explicit Neurons | G; 1 jUP minimum |
| `max_age_bonus_percentage` | 0 | 200 | Explicit Voting | G; additional VP, not inflation |
| `max_neuron_age_for_age_bonus` | 126,230,400 s | unchanged | Explicit Voting | G; four Julian years |
| `max_dissolve_delay_bonus_percentage` | 0 | 100 | Explicit Voting | G; additional VP |
| `max_dissolve_delay_seconds` | 31,557,600 s | unchanged | Explicit Voting | G; maximum one-year commitment |
| Minimum voting delay | 31,557,600 s | 15,778,800 s | Explicit Voting | G; zero-delay sale member cannot vote |
| Initial/final native reward rate | 0 / 0 bp | unchanged | Explicit Voting | G; no native voting maturity |
| Native transition duration | 0 s | unchanged | Explicit Voting | G |
| Native reward round | 86,400 s | unchanged | Inherited; converter has no round field | G; authentic zero-purse events remain available |
| Initial voting period | 432,000 s | unchanged | Explicit Proposals | G; mirror deadline admission unchanged |
| Wait-for-quiet increase | 3,600 s | unchanged | Explicit Proposals | G |
| Default followees | empty | unchanged | Inherited | G; pinned validator requires empty |
| Maximum followees/function | 15 | unchanged | Inherited | G; practical native ceiling retained |
| Maximum neurons | 200,000 | unchanged | Inherited | G; small behavioral fixtures use 1,000 |
| Retained proposals/action | 100 | unchanged | Inherited | G; reconciliation retention preserved |
| Proposals with ballots | 700 | unchanged | Inherited | G; native resource bound |
| Claim/grant permissions | all native variants 0–10 | unchanged | Converter sets official capability list | G; `Unspecified` grants nothing; app ownership predicate is stricter |
| Principals/neuron | 5 | unchanged | Inherited | G; access control model unchanged |
| Maturity modulation disabled | false | unchanged | Inherited | G; irrelevant to zero jUP purse |
| Ordinary approval | 3% total, >50% exercised | unchanged | Native criticality rules | Not weakened |
| Critical approval | 20% total, 67% exercised | unchanged | Native criticality rules | Not weakened; no custom criticality added |
| Automatic target-version advancement | true | true at constructor | Inherited; **unsupported launch field** | G; tested governed opt-out before fixture financial activation |
| Aggregate direct ICP raise | 10,000–100,000 ICP | unchanged | Explicit Swap; validated | Fixed sale |
| Minimum participants | 1 | unchanged | Explicit Swap | Fixed sale |
| Configured minimum buyer | 22 ICP | 11 ICP | Explicit Swap; real validation required | Exact basket floor is 10.1 ICP |
| Maximum buyer | 100,000 ICP | unchanged | Explicit Swap | Fixed sale |
| Basket | 2, zero/one-year | unchanged | Explicit Swap | Minimum stake and fees enforced per neuron |
| Sale duration | 7 days | unchanged | Explicit Swap; execution supplies timestamps | Fixed sale |
| Neurons' Fund participation | disabled | unchanged | Explicit Swap | No assumed external subsidy |

Supply: `10,000,000 + 20,000 + 100 + 89,979,900 + 0 = 100,000,000 jUP`.
Actual Ledger fee burning reduces supply during transport and basket creation;
the tests reconcile these reductions separately from zero native issuance.
Theoretical sale endpoint prices are 0.5–5 ICP/jUP, without a price or liquidity guarantee.

The pinned Init inequality is
`buyer * 20,000 / 100,000 >= 2 * (1 + 0.01)` in token units.
Thus 10.1 ICP is the exact supported mathematical floor; 11 ICP is the configured
minimum. The official converter checks 10.1 ICP and its immediately preceding e8s,
and the real Swap rehearsal checks the configured buyer minimum at both raises.

The converter cannot set `automatically_advance_target_version`. The test observes
the true inherited default and adopts a native parameter proposal setting false
after Normal mode, before treasury activation. A deployment operator must review
and govern that policy explicitly; do not describe a fabricated constructor
field as a version pin. Normal reviewed native upgrade proposals remain possible.

## Three independent bonuses and actual income

A fresh one-year neuron has 2× raw-stake voting power. At four years of continuous
non-dissolution it has 6×, a 3:1 advantage over an equal fresh commitment. This is
relative voting/participation weight, not six times income or an annual yield.
Native `reward_shares` already include stake, age, delay and eligible Yes/No
activity; SNS Rewards uses them once. No activity means no event share. Starting
dissolution resets age. Remaining delay can permit voting for roughly the first
six months; stopping dissolution starts age accumulation again.

Direct patron 2× issuance remains a separate exact-one-year, non-dissolving,
auto-staking and exclusive-economic-control test. Six-month voting eligibility
does not qualify a neuron for that benefit. Recognition itself needs secure
positive economic stake, not voting eligibility or an IO neuron.

Native ICP treasury reinvestment goes directly to the reviewed NNS staking
Account without a routing memo. It increases principal after native refresh and
can dilute stored NNS age. It generates no patron jUP or Relay pot. NNS maturity
remains conditional on native rules. Disburser's base/Faucet and 95% SNS Rewards /
5% D-QUORUM gross age-bonus split is unchanged. Genuine Relay endowments still
earn their base-rate reserve pot; immutable Relay deducts its actual recipient
fees and applies its existing economic threshold.

Age-bonus ICP inventory is **not** funded IO. The remaining integration boundary
is authenticated ICP-to-IO settlement, verified actual IO receipt, then separately
reviewed inventory/eligibility/fee/duplicate-safe distribution. Current quotes
do not create an entitlement or dispatch IO. Independent IO contract receipt and
duplicate protections remain intact; no IO application repository is imported.

## Deterministic incentive stress cases

Run `python3 tools/acceptance/economic_analysis.py` and the acceptance Python unit
suite. All stakes, age, activity and settled inventory in that output are labelled
hypothetical inputs. Native mechanics are tested separately in `native_economics.rs`.
Both cohorts assume one-year delay and equal eligible activity; the protocol
10-million neuron is assumed four years old and remains excluded economically.

| Assumed case | Earlier cohort's external share | Implication |
|---|---:|---|
| 5,000 old versus 5,000 fresh jUP | 75% | Equal-stake age advantage is 3:1; common age factors cancel |
| 5,000 old versus 100,000 fresh jUP | 3/23 | Large late stake dilutes old economic concentration |
| 5,000 old versus 80 million freshly staked reserve jUP | 3/16,003 | Distributed reserve overwhelms a small early cohort |
| Several years with zero settled IO | 75% of zero | Age preserves relative weight, creates no income |
| Same equal stakes, inventory rises 1,000→10,000 units | 750→7,500 units | Inventory changes absolute amounts; weight does not create inventory |
| 1 million old versus 5,000 fresh jUP | 600/601 | Many old holders can retain strong concentration |
| 5,000 old versus 1 million fresh jUP | 3/203 | New participation dilutes the old cohort |
| NNS principal 100→1,000 ICP, old age four years | Native age becomes 0.4 years | More principal does not retain its previous age multiplier |
| Equal stake: starts dissolution versus retains four-year age | 25% versus 75% | Incentive to stay non-dissolving; withdrawal requires waiting |
| Genuine transfer of an old neuron's exclusive control | Age retained | Native age belongs to the neuron; transferred stake can carry its weight |

In the first assumed case the protocol retains `1500/1501` of native voting
power despite receiving zero external allocation. After rapid distribution of
80 million fresh jUP it retains `6000/22003`. Financial exclusion does not solve
governance concentration. Actual PocketIC ballots separately retain the real
10-million governance neuron while excluding its event shares.

For a second hypothetical comparison, retain 10 million governance jUP, 100
proposer jUP and 10,000 voting-eligible sale jUP at one-year delay. The other
10,000 zero-delay sale jUP and zero-delay reserve have no native voting power.
With fresh neurons the governance neuron's potential voting fraction is
`100000/100101`; if only that neuron ages four years it becomes `300000/300101`.
If all three cohorts age together the fraction remains `100000/100101`.
These figures follow `economic_analysis.weight` with no fees or top-ups assumed.
Protocol financial exclusions leave the eligible external cohort all of the
given external inventory in each case, without increasing that inventory.

More proposals can shift relative rewards toward active voters, but cannot
manufacture a fixed inventory. Native proposal fees, approval and voting periods
still apply. Splitting preserves aggregate weight before fees/rounding, cannot
copy past participation and incurs real fees; no extra patron multiplier exists
per neuron. Every sponsored direct/pot payment costs the reserve its quote plus
the authoritative fee. The 0.01 fee reduces tiny-payment amplification but does
not remove it. Relay's actual downstream thresholds still constrain small receipts.
