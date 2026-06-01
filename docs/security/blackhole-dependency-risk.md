# Blackhole Dependency Risk

This document classifies every ignored Rust advisory in `deny.toml`,
`.cargo/audit.toml`, and `osv-scanner.toml` for blackhole readiness. Scanner
ignores are not treated as acceptance by themselves; each remaining exception
needs an explicit production-reachability classification before blackholing.

## Evidence Commands

Advisory inverse trees were captured with:

```bash
cargo tree -i rsa --workspace --edges normal,build,dev
cargo tree -i serde_cbor --workspace --edges normal,build,dev
cargo tree -i bincode --workspace --edges normal,build,dev
cargo tree -i paste --workspace --edges normal,build,dev
cargo tree -i proc-macro-error --workspace --edges normal,build,dev
cargo tree -i derivative --workspace --edges normal,build,dev
cargo tree -i backoff --workspace --edges normal,build,dev
cargo tree -i instant --workspace --edges normal,build,dev
```

Production canister wasm-target trees were captured with:

```bash
cargo tree -p jupiter-disburser --target wasm32-unknown-unknown --edges normal,build
cargo tree -p jupiter-faucet --target wasm32-unknown-unknown --edges normal,build
cargo tree -p jupiter-relay --target wasm32-unknown-unknown --edges normal,build
cargo tree -p jupiter-historian --target wasm32-unknown-unknown --edges normal,build
cargo tree -p jupiter-faucet-frontend --target wasm32-unknown-unknown --edges normal,build
cargo tree -p jupiter-lifeline --target wasm32-unknown-unknown --edges normal,build
cargo tree -p jupiter-sns-rewards --target wasm32-unknown-unknown --edges normal,build
```

Source reachability checks were captured with:

```bash
rg -n "\b(rsa|serde_cbor|bincode)\b|RsaPrivateKey|RsaPublicKey|SigningKey|DecryptingKey" canisters crates --glob '*.rs'
rg -n "serde_cbor::(from_slice|from_reader)|bincode::(deserialize|deserialize_from)|RsaPrivateKey|\.decrypt\(|\.sign\(" \
  ~/.cargo/registry/src/index.crates.io-*/ic-http-certification-3.2.0 \
  ~/.cargo/registry/src/index.crates.io-*/ic-asset-certification-3.2.0 \
  ~/.cargo/registry/src/index.crates.io-*/sev-7.1.0/src \
  ~/.cargo/git/checkouts/ic-a5a9adfef36c4712/b4b0230/rs/types/types/src
```

The first source reachability command returned no matches in Jupiter canister or
shared-crate Rust source. The second command found upstream decode/private-key
adjacent code in DFINITY or `sev` sources, but no Jupiter-owned call site.

## Production Tree Summary

| Production canister | Ignored advisory crates present in wasm-target normal/build tree |
| --- | --- |
| `jupiter-disburser` | `rsa`, `serde_cbor`, `bincode`, `paste` proc macro, `proc-macro-error`, `derivative` proc macro |
| `jupiter-faucet` | `rsa`, `serde_cbor`, `bincode`, `paste` proc macro, `proc-macro-error`, `derivative` proc macro |
| `jupiter-relay` | `rsa`, `serde_cbor`, `bincode`, `paste` proc macro, `proc-macro-error`, `derivative` proc macro |
| `jupiter-historian` | `rsa`, `serde_cbor`, `bincode`, `paste` proc macro, `proc-macro-error`, `derivative` proc macro |
| `jupiter-faucet-frontend` | `serde_cbor`, `paste` proc macro |
| `jupiter-lifeline` | `paste` proc macro |
| `jupiter-sns-rewards` | `paste` proc macro |

`backoff` and `instant` do not appear in any production canister normal/build
tree. They appear only through `pocket-ic` dev-dependencies.

## Advisory Classification

| Advisory ID | Crate/version | Advisory class | Dependency path | Present in production canister normal deps | Present only in build/proc-macro deps | Dev/test-only | Reachable from public production canister methods | Remediation attempted | Blocks blackholing | Evidence command/output reference |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `RUSTSEC-2023-0071` | `rsa 0.9.10` | vulnerability | `rsa -> sev -> attestation -> ic-registry-canister-api -> ic-nervous-system-canisters -> ... -> ic-nns-governance-api -> jupiter-nns-types -> {jupiter-disburser,jupiter-faucet,jupiter-relay,jupiter-historian}` | yes, for disburser/faucet/relay/historian | no | no | no | Reviewed inverse tree, wasm-target trees, Jupiter source, and `sev` source. `sev` uses `rsa::RsaPublicKey`/PSS verification in certificate verification code; no Jupiter source imports `rsa` or calls RSA decrypt/sign/private-key APIs, and no production public method calls SEV attestation certificate verification. No patched `rsa` release is listed by RustSec. Removing this path requires a DFINITY IC dependency revision change, which is intentionally out of scope without full-suite release validation. | no | `cargo tree -i rsa --workspace --edges normal,build,dev`; production tree commands for disburser/faucet/relay/historian; source reachability `rg` commands above. |
| `RUSTSEC-2021-0127` | `serde_cbor 0.11.2` | unmaintained | DFINITY path: `serde_cbor -> ic-types/cycles-minting-canister/icp-ledger/... -> jupiter-nns-types -> {jupiter-disburser,jupiter-faucet,jupiter-relay,jupiter-historian}`; frontend path: `serde_cbor -> ic-http-certification -> ic-asset-certification -> jupiter-faucet-frontend`; dev path: `serde_cbor -> pocket-ic` | yes, for disburser/faucet/relay/historian/frontend | no | no | no | Reviewed inverse tree, wasm-target trees, Jupiter source, `ic-http-certification`, `ic-asset-certification`, and relevant DFINITY type sources. Jupiter production source has no direct `serde_cbor` imports or decoders. Frontend asset-certification sources in this closure did not expose `serde_cbor::from_slice`/`from_reader` call sites. DFINITY `ic-types` contains CBOR decoders for ingress/WebAuthn/type conversion helpers, but Jupiter canister public methods accept Candid and do not route attacker-controlled bytes into those helpers. No maintained drop-in replacement exists for upstream paths. | no | `cargo tree -i serde_cbor --workspace --edges normal,build,dev`; production tree commands; source reachability `rg` commands above. |
| `RUSTSEC-2025-0141` | `bincode 1.3.3` | unmaintained | Runtime DFINITY path: `bincode -> ic-types -> ... -> jupiter-nns-types -> {jupiter-disburser,jupiter-faucet,jupiter-relay,jupiter-historian}`; build path: `bincode -> build-info-build/build-info-proc -> DFINITY crates` | yes, for disburser/faucet/relay/historian | no | no | no | Reviewed inverse tree, wasm-target trees, Jupiter source, and relevant DFINITY type sources. Jupiter production source has no direct `bincode` imports or deserializers. DFINITY `ic-types` contains `bincode::deserialize` in consensus/crypto/message conversion helpers and tests, but Jupiter public canister methods do not call those helpers with attacker-controlled bytes. No safe narrow removal is available without changing the pinned DFINITY IC revision. | no | `cargo tree -i bincode --workspace --edges normal,build,dev`; production tree commands; source reachability `rg` commands above. |
| `RUSTSEC-2024-0436` | `paste 1.0.15` | unmaintained | `paste` proc macro via `candid`, `ic-cdk-macros`, `ic-heap-bytes`, DFINITY crates, and direct canister macro dependencies | no runtime dependency; appears as proc macro in all production wasm-target trees | yes | no | no | Reviewed inverse tree and wasm-target trees. `cargo tree` marks `paste v1.0.15 (proc-macro)`. It expands at build time and is not a production runtime code path. Removing it requires upstream `candid`/DFINITY macro changes. | no | `cargo tree -i paste --workspace --edges normal,build,dev`; production tree commands. |
| `RUSTSEC-2024-0370` | `proc-macro-error 1.0.4` | unmaintained | `proc-macro-error -> build-info-proc (proc-macro) -> build-info -> DFINITY crates -> jupiter-nns-types -> {jupiter-disburser,jupiter-faucet,jupiter-relay,jupiter-historian}` | no runtime dependency; appears through a proc-macro/build path | yes | no | no | Reviewed inverse tree and wasm-target trees. The dependency is reached through `build-info-proc (proc-macro)` and is not runtime code in canister public methods. Removing it requires upstream `build-info`/DFINITY changes. | no | `cargo tree -i proc-macro-error --workspace --edges normal,build,dev`; production tree commands. |
| `RUSTSEC-2024-0388` | `derivative 2.2.0` | unmaintained | `derivative (proc-macro) -> sns-treasury-manager -> ic-sns-governance -> ic-sns-swap -> ic-nns-governance-api -> jupiter-nns-types -> {jupiter-disburser,jupiter-faucet,jupiter-relay,jupiter-historian}` | no runtime dependency; appears as proc macro in production wasm-target trees for DFINITY-derived code | yes | no | no | Reviewed inverse tree and wasm-target trees. `cargo tree` marks `derivative v2.2.0 (proc-macro)`. It is build-time macro expansion support, not a runtime public method path. Removing it requires upstream DFINITY revision changes. | no | `cargo tree -i derivative --workspace --edges normal,build,dev`; production tree commands. |
| `RUSTSEC-2025-0012` | `backoff 0.4.0` | unmaintained | `backoff -> pocket-ic` under dev-dependencies of canister crates and `xtask` | no | no | yes | no | Reviewed inverse tree and all production wasm-target trees. `backoff` is present only through `pocket-ic` dev/test tooling and absent from production canister normal/build trees. | no | `cargo tree -i backoff --workspace --edges normal,build,dev`; production tree commands show no `backoff`. |
| `RUSTSEC-2024-0384` | `instant 0.1.13` | unmaintained | `instant -> backoff -> pocket-ic` under dev-dependencies of canister crates and `xtask` | no | no | yes | no | Reviewed inverse tree and all production wasm-target trees. `instant` is present only through `backoff`/`pocket-ic` dev/test tooling and absent from production canister normal/build trees. | no | `cargo tree -i instant --workspace --edges normal,build,dev`; production tree commands show no `instant`. |

## Blackhole Sign-Off Position

No ignored advisory is classified as production-runtime reachable from Jupiter
public canister methods. No advisory has unknown reachability.

The remaining scanner exceptions are acceptable for blackholing only with owner
acknowledgment that:

- `rsa`, `serde_cbor`, and `bincode` remain in some production dependency
  closures through pinned upstream DFINITY crates, but are not reached by Jupiter
  public canister methods according to the evidence above.
- `paste`, `proc-macro-error`, and `derivative` are build-time/proc-macro
  exceptions rather than runtime public method paths.
- `backoff` and `instant` are dev/test-only exceptions through `pocket-ic` and
  are absent from production canister wasm-target normal/build dependency trees.
- Future DFINITY IC, `candid`, `ic-http-certification`, `pocket-ic`, and
  RustCrypto RSA upgrades should remove these exceptions when safe patched or
  maintained paths become available.
