# Acceptance identity handoff, 7 November 2026

The operator supplied Disburser `spoge-kaaaa-aaaas-amyoa-cai` and existing NNS
neuron `2234355977000034918`, reporting that exact canister as its controller.
The confirmed fields are recorded in `profile.json`; the operator remains unresolved. The operator subsequently delegated arbitrary
selection of the other eleven roles. The assignments below are explicit choices,
not inferred creation history; `provisioning.json` retains that provenance. No replacement
canister or neuron is required or authorized. The 3T figure is an intended
main-balance target, not an observed balance.

Anonymous read-only checks used the installed SDK's mainnet root key and normal
certificate freshness verification. At client check time 15:51 UTC, the certified
Disburser module hash was
`03e8ba94621d28bf166595a9a9dd0d9943c60caad48c7a2740285a7fee283bbe`.
Its certified controller set contained
`amzih-bssz4-twanl-zf6nr-bqsnn-d7rlt-eerbc-oe7vv-uggn5-3naqo-3ae` and
`zkkkd-i34qc-367ln-e2u7o-ezznu-dkfqh-gtfvz-cviph-6qa4v-evtfs-wqe`.
The separate certified Candid metadata check returned `service : { }`.
These observations prove installed code, not its bootstrap provenance or stable
state. The SDK checks each requested certificate; these are not one atomic
cross-canister observation. Raw observations are retained under the ignored
`target/acceptance-identity-handoff/` directory.

The signed public NNS neuron-info query at client check time 15:53 UTC reported
1 ICP stake, state 1 (non-dissolving), and a 63,115,200-second dissolve delay.
This is a replica-signed query, not a certified full-neuron record. The full
neuron query rejected the anonymous caller as unauthorized. Controller, actual
staking bytes, auto-stake maturity, hot keys, followees and other private settings
have therefore **not** been independently verified. No financial operation is
ready on this evidence.

The existing nonce-zero convention derives expected NNS staking subaccount
`52eb1b31eb9cf6357e2b2e49e32d2238d2b6db2b2f321cbe7b879ea261ac7390`
and ICP AccountIdentifier
`e458b5ed224bd0b9e884599dd0e986e78d02266674b093b617105528311fa602`.
This derivation must be compared against an authorized full-neuron observation;
it does not prove that the existing neuron uses nonce zero. Do not create a new
neuron to make the comparison pass, change dissolve/auto-stake settings, or move
ICP without separate authorization.

Before selecting any code replacement, obtain the installed module's creation
record/source and establish whether it owns meaningful stable state or in-flight
work. If it is Jupiter Disburser code, preserve state with the reviewed upgrade
contract. If it is a verified disposable bootstrap with no required state,
reinstalling the reviewed Disburser package at **the same principal**, with
reviewed fresh arguments and autonomous rescue unarmed, is the smallest
conditional replacement; no preliminary uninstall is needed. Do not select that
procedure merely from an empty Candid service. Both procedures still require
explicit mainnet installation authorization. Preserve the eventual self/Lifeline
rescue policy and controllerless-child policy; do not import IO's controller graph.

Bootstrap provenance, authorized full-neuron settings, operator identity and
actual cycles observations remain outstanding. Offline implementation and
owned PocketIC tests proceed independently. No mainnet install, upgrade,
controller operation, neuron modification, funding or transfer was performed.

## Explicit role assignment authorized by the operator

| Role | Principal |
|---|---|
| sns_root | `sbmlm-rqaaa-aaaas-amypa-cai` |
| sns_governance | `sipaq-hyaaa-aaaas-amyoq-cai` |
| sns_ledger | `sgnny-4iaaa-aaaas-amypq-cai` |
| sns_index | `x2wx5-xaaaa-aaaas-amyqa-cai` |
| sns_swap | `x5xrj-2yaaa-aaaas-amyqq-cai` |
| jupiter_disburser | `spoge-kaaaa-aaaas-amyoa-cai` |
| jupiter_faucet | `xuu2v-mqaaa-aaaas-amyra-cai` |
| jupiter_lifeline | `xiqae-3qaaa-aaaas-amyta-cai` |
| jupiter_sns_rewards | `xgsnm-aaaaa-aaaas-amysa-cai` |
| jupiter_historian | `xbtly-nyaaa-aaaas-amysq-cai` |
| jupiter_relay | `xtv4b-biaaa-aaaas-amyrq-cai` |
| jupiter_faucet_frontend | `xprgq-wiaaa-aaaas-amytq-cai` |

`python3 tools/acceptance/profile.py check-roles deploy/acceptance/profile.json`
validates all twelve principals against protected production/shared identities
and rejects collisions. The exact confirmed neuron remains bound to Disburser.
This is offline configuration; it does not claim installation, controller handoff
or 3T actual balances.
