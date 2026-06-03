# Production Canister Dependency Advisory Risk

This document classifies the remaining ignored Rust advisory dependencies for
production canisters. Scanner ignores are not treated as acceptance by
themselves; each remaining exception needs an explicit reachability reason, and
owner acceptance is the fallback rather than the target.

Production canisters covered here:

- `jupiter-disburser`
- `jupiter-faucet`
- `jupiter-relay`
- `jupiter-historian`
- `jupiter-faucet-frontend`
- `jupiter-lifeline`
- `jupiter-sns-rewards`

## Evidence Commands

Advisory inverse trees were captured with:

```bash
cargo tree -i rsa --workspace --edges normal,build,dev
cargo tree -i serde_cbor --workspace --edges normal,build,dev
cargo tree -i bincode --workspace --edges normal,build,dev
cargo tree -i paste --workspace --edges normal,build,dev
cargo tree -i proc-macro-error --workspace --edges normal,build,dev
cargo tree -i derivative --workspace --edges normal,build,dev
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

Required validation for this dependency posture:

```bash
cargo check --workspace --locked
cargo test --workspace --locked
./tools/scripts/build-canister all
python3 ./tools/scripts/validate-mainnet-install-args
./tools/scripts/security-scan
```

## Current NNS Dependency Posture

`jupiter-nns-types` provides the minimal Candid-compatible NNS Governance wire
DTOs used by Jupiter canisters and tests from the pinned subset DID under
`candid/nns-governance/`. The DTO file is committed and verified by the
dev-only `nns-bindgen-check` tool, which uses `candid_parser` directly.
`jupiter-nns-types` remains DTO-only and has no `ic-cdk` dependency.

`jupiter-ic-clients` contains the committed generated raw NNS Governance
transport. That crate already owns shared inter-canister client code and already
depends on `ic-cdk`, so the generated transport does not add `ic-cdk` to the
DTO crate. The raw transport is generated from the same pinned subset and
returns raw `ic_cdk::call::Response` values to Jupiter-owned adapters for
decode and error classification.

Production canister builds include plain Rust source and do not run bindgen,
depend on `ic-cdk-bindgen`, or rely on generated marker extraction.

This keeps the broad DFINITY NNS graph out of disburser, faucet, relay, and
historian production trees, including `rsa`, `bincode`, `proc-macro-error`, and
`derivative`. The refactor also does not reintroduce broad `dfinity/ic` git
crates. The production public `.did` files remain unchanged.

Scanner exception files currently exclude advisories for dependencies that are
not present in the production NNS DTO graph:

- Removed ignores for `RUSTSEC-2023-0071` / `rsa`.
- Removed ignores for `RUSTSEC-2025-0141` / `bincode`.
- Removed ignores for `RUSTSEC-2024-0370` / `proc-macro-error`.
- Removed ignores for `RUSTSEC-2024-0388` / `derivative`.
- Kept ignores only for remaining `paste`, `serde_cbor`, plus the existing
  dev-tooling `backoff` and `instant` exceptions.

## Before And After

Before this reduction:

| Production canister | Advisory crates present in wasm-target normal/build tree |
| --- | --- |
| `jupiter-disburser` | `rsa`, `serde_cbor`, `bincode`, `paste`, `proc-macro-error`, `derivative` |
| `jupiter-faucet` | `rsa`, `serde_cbor`, `bincode`, `paste`, `proc-macro-error`, `derivative` |
| `jupiter-relay` | `rsa`, `serde_cbor`, `bincode`, `paste`, `proc-macro-error`, `derivative` |
| `jupiter-historian` | `rsa`, `serde_cbor`, `bincode`, `paste`, `proc-macro-error`, `derivative` |
| `jupiter-faucet-frontend` | `serde_cbor`, `paste` |
| `jupiter-lifeline` | `paste` |
| `jupiter-sns-rewards` | `paste` |

After this reduction:

| Production canister | Advisory crates present in wasm-target normal/build tree | Reason |
| --- | --- | --- |
| `jupiter-disburser` | `paste` | Build-time proc macro through `candid` / `ic-cdk` macro dependencies. |
| `jupiter-faucet` | `paste` | Build-time proc macro through `candid` / `ic-cdk` macro dependencies. |
| `jupiter-relay` | `paste` | Build-time proc macro through `candid` / `ic-cdk` macro dependencies. |
| `jupiter-historian` | `paste` | Build-time proc macro through `candid` / `ic-cdk` macro dependencies. |
| `jupiter-faucet-frontend` | `serde_cbor`, `paste` | `serde_cbor` is pulled by `ic-http-certification` through `ic-asset-certification`; `paste` is a build-time proc macro. |
| `jupiter-lifeline` | `paste` | Build-time proc macro through `candid` / `ic-cdk` macro dependencies. |
| `jupiter-sns-rewards` | `paste` | Build-time proc macro through `candid` / `ic-cdk` macro dependencies. |

`rsa`, `bincode`, `proc-macro-error`, and `derivative` are no longer present in
the workspace dependency graph. `serde_cbor` is no longer present in backend
production canister wasm-target normal/build trees.

## Current Advisory Classification

| Crate | Status | Production canisters affected | Reason remaining or removal evidence |
| --- | --- | --- | --- |
| `rsa` | Removed | None | Removed with the broad DFINITY NNS graph. `cargo tree -i rsa --workspace --edges normal,build,dev` reports no matching package. |
| `bincode` | Removed | None | Removed with the broad DFINITY NNS graph. `cargo tree -i bincode --workspace --edges normal,build,dev` reports no matching package. |
| `proc-macro-error` | Removed | None | Removed with the broad DFINITY NNS graph. `cargo tree -i proc-macro-error --workspace --edges normal,build,dev` reports no matching package. |
| `derivative` | Removed | None | Removed with the broad DFINITY NNS graph. `cargo tree -i derivative --workspace --edges normal,build,dev` reports no matching package. |
| `paste` | Still present, build-time only | All production canisters | `cargo tree` marks `paste v1.0.15 (proc-macro)`. It is pulled by upstream `candid` / `ic-cdk` macro paths and expands at build time. Removing it requires upstream macro dependency changes. |
| `serde_cbor` | Still present through unavoidable upstream runtime dependency | `jupiter-faucet-frontend` | `serde_cbor -> ic-http-certification -> ic-asset-certification -> jupiter-faucet-frontend`. Current `ic-http-certification 3.2.0` has a direct `serde_cbor` dependency; current `ic-asset-certification` depends on it with `default-features = false`, so no local feature flag removes it. |
| `serde_cbor` | Still present, dev/test-only | Test and PocketIC tooling for backend crates | `serde_cbor` also appears through `pocket-ic` / `ic-transport-types` dev dependency paths. These paths are absent from production wasm-target normal/build trees for disburser, faucet, relay, historian, lifeline, and sns-rewards. |

The security scan still also ignores:

| Crate | Status | Reason |
| --- | --- | --- |
| `backoff` | Still present, dev/test-only | Pulled by `pocket-ic` dev/test tooling and absent from production canister wasm-target normal/build trees. |
| `instant` | Still present, dev/test-only | Pulled by `backoff` / `pocket-ic` dev/test tooling and absent from production canister wasm-target normal/build trees. |

## Wasm And Candid Impact

The canister build was regenerated with:

```bash
./tools/scripts/build-canister all
```

The resulting artifact hashes were:

```text
fecd620438bb7e66d3ff91c44e9ab9b699ca0d9db2645b36de18997dda22b25a  release-artifacts/jupiter_faucet.wasm
15a461334944d396f2e7a925fe3f6a829f67e0fce0587a984d084723869c798d  release-artifacts/jupiter_faucet.wasm.gz
ab37996f1e92b29c404893d32eb2de5a4c5c1e1877631423b4fde6b807659cfb  release-artifacts/jupiter_disburser.wasm
546ad3508249fe52703f50e2fee7745ca35e4ef011492f45b70c3a32ae3efc85  release-artifacts/jupiter_disburser.wasm.gz
d16d08e0599e88750d271e3842b1b7e89180bb9d8e37b029d415f5a36a910936  release-artifacts/jupiter_relay.wasm
5d294698e2803ce7bc691b27b19709bd9fd6b55c7b829927040dff449727e149  release-artifacts/jupiter_relay.wasm.gz
44fb2d04dd79af12b7f1a7b28fc969c674279759d81ae6cfe1e57540716e3a3a  release-artifacts/jupiter_historian.wasm
aab3c2993b135ecbe2da38ee9f33f37e909dbdcf150615de34be9ce62649854b  release-artifacts/jupiter_historian.wasm.gz
a62037344b5ed46fae222bc60cb44b462225a9489242412c646e2b5c0d65f4ac  release-artifacts/jupiter_lifeline.wasm
ba4c0ee63bd04bf3603eb2345ef14a462de694ffac1ce22c5db02dd727a0e0be  release-artifacts/jupiter_lifeline.wasm.gz
ea2f0c68d4f97439b3395db684f6a9f48efab181a94888a042af2771d47f942e  release-artifacts/jupiter_sns_rewards.wasm
f223b65e8de6957bbf92f2433ffb9a2618b96a9c9cf9f08a128dd2c54137e62d  release-artifacts/jupiter_sns_rewards.wasm.gz
215c32dc743fb934bda6cabb134c06c1e32a30b297b83ee3bf48e6a6be52a688  release-artifacts/jupiter_faucet_frontend.wasm
aa607f9ab2f4824efefc1e70edf4a289bf1cd102cf2d2ec47ba79fc8d1084adf  release-artifacts/jupiter_faucet_frontend.wasm.gz
```

There is no tracked pre-change artifact baseline in this branch, so this
document records the regenerated artifact hashes rather than claiming unchanged
Wasm.

No production `.did`, debug `.did`, `dfx.json`, `canister_ids.json`, or
`mainnet-install-args.did` file changed. Production install arguments remain
valid according to:

```bash
python3 ./tools/scripts/validate-mainnet-install-args
```

## Remaining Owner Acceptance

Owner acceptance covers:

- `paste` as a build-time upstream proc-macro dependency in all production
  canister build trees.
- `serde_cbor` as an upstream frontend certification runtime dependency through
  `ic-http-certification` / `ic-asset-certification`.
- `serde_cbor`, `backoff`, and `instant` as dev/test-only dependencies through
  PocketIC tooling.

No production canister carries `rsa`, `bincode`, `proc-macro-error`, or
`derivative`, and no backend production canister carries `serde_cbor`, merely
because of an unnecessarily broad Jupiter-owned dependency.
