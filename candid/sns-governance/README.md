# Official SNS Governance DTO source

`governance.did` is the exact embedded `candid:service` from the verified official
Governance artifact at DFINITY IC revision
`98c898f8de068a1fc0e97ebc805f9fa7f315d862`. Its SHA-256 is
`0d3a00e821c8959991cfb90f4366d101fce340e78f3fb76b0de4ed28c1c08c38`,
independently checked against `deploy/acceptance/sns-framework.json`.
The upstream file includes trailing whitespace. Its path-specific Git attribute
preserves those exact bytes; the pinned hash test still checks the whole file.

Generate DTOs with the locked `candid_parser 0.2.4` structured type output:

```sh
cargo run --locked -p nns-bindgen-check --example sns -- candid/sns-governance/governance.did target/sns.rs
cmp canisters/sns-rewards/src/generated/sns.rs target/sns.rs
```

The selected methods retain the full reachable Proposal/Action graph. There are
no generated call stubs, IO application dependencies or hand-trimmed action
records. Automation rejects installed Governance hashes outside this reviewed
schema so a future optional action field cannot disappear silently from the
binding digest. A framework upgrade needs explicit schema/artifact review.

Launch CLI and SNS-W source/artifact pins are separate in
`deploy/acceptance/launch-tools.json`; the six installed SNS components need not
share one upstream revision.

Permission integers use separately generated `NeuronPermissionType` constants from
that same revision's `rs/sns/governance/proto/ic_sns_governance/pb/v1/governance.proto`.
`tools/acceptance/generate-permissions.py <verified-proto> <output.rs>` checks the
whole source SHA-256 before generation. Candid itself carries these as `int32`.
The ownership allowlist follows `Neuron::PERMISSIONS_RELATED_TO_VOTING` and
`check_principal_authorized_to_change_permissions` in that revision's
`rs/sns/governance/src/neuron.rs`: ManageVotingPermission can grant Vote,
SubmitProposal and itself, but cannot grant economic rights. SubmitProposal can
incur rejection fees; normal voting delegation remains supported. Competing
dissolve, permission-management, disbursement, split and maturity rights, as well
as unknown permission codes, make the individual neuron unsafe for attribution.
The named `Unspecified` value (0) is explicitly unused in this protobuf. Official
sale neurons include it; it grants no authority and is ignored.
