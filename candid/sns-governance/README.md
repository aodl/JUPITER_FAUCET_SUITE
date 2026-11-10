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
