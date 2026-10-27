# Dependency Scanning

Jupiter Faucet release checks include Rust advisory scanning, Rust license/source policy checks, npm advisory scanning, OSV lockfile scanning, and CycloneDX SBOM generation.

## Tool Setup

Release security evidence uses pinned scanner versions so SBOMs and advisory output
are reproducible across release reruns. Install the Rust scanners with the
checked-in Rust toolchain:

```bash
cargo install cargo-audit --version 0.22.1 --locked
cargo install cargo-deny --version 0.19.8 --locked
cargo install cargo-cyclonedx --version 0.5.9 --locked
```

Install Node dependencies from the committed [`package-lock.json`](../../package-lock.json):

```bash
npm ci
```

Install OSV Scanner from the official GitHub release binary. The version observed while adding this gate was `v2.3.8`:

```bash
mkdir -p "$HOME/bin"
curl -fsSLo /tmp/osv-scanner_linux_amd64 https://github.com/google/osv-scanner/releases/download/v2.3.8/osv-scanner_linux_amd64
curl -fsSLo /tmp/osv-scanner_SHA256SUMS https://github.com/google/osv-scanner/releases/download/v2.3.8/osv-scanner_SHA256SUMS
grep 'osv-scanner_linux_amd64$' /tmp/osv-scanner_SHA256SUMS | (cd /tmp && sha256sum -c -)
install -m 0755 /tmp/osv-scanner_linux_amd64 "$HOME/bin/osv-scanner"
```

The npm SBOM step uses `npx --yes @cyclonedx/cyclonedx-npm@4.2.1` so the
CycloneDX npm CLI is provisioned by npm when it is not already cached, while
remaining pinned for release evidence.

The helper reports installed scanner versions at runtime. Keep the install
versions above aligned with the versions expected by release evidence review.

## Local Gate

Run the canonical local command from any directory in the repository:

```bash
./tools/scripts/security-scan
```

The script fails non-zero if any of these steps fails:

- [`tools/scripts/check-production-reachability`](../../tools/scripts/check-production-reachability)
- `cargo test --locked -p xtask --test block_buffer_advisory`
- `cargo audit`
- `cargo deny check advisories licenses bans sources`
- [`node tools/scripts/check-npm-lock-hermetic.mjs`](../../tools/scripts/check-npm-lock-hermetic.mjs)
- `npm audit --omit=dev`
- `osv-scanner scan -L Cargo.lock -L package-lock.json`
- Rust or npm SBOM generation

The license allowlist is self-cleaning: `unused-allowed-license = "deny"`
requires every globally permitted license to occur in the checked graph. The
duplicate-version policy likewise denies new splits by default. Each accepted
older version has an exact `[[bans.skip]]` entry in
[`deny.toml`](../../deny.toml) that identifies the incompatible upstream path;
broad `skip-tree` exceptions are not used.

`cargo-cyclonedx 0.5.9` interprets Cargo `links` values as URIs. DFINITY uses
non-URI `links` metadata in `ic-cdk-executor` and `ic-cdk-timers` deliberately
as a CDK version-conflict guard. The security script collapses only those two
exact known diagnostics into one acknowledgement. Any other
`cargo_cyclonedx::generator` warning is printed and fails the gate, while the
generated SBOM remains unchanged.

The npm lockfile gate requires every package entry to include both an integrity
hash and the exact `https://registry.npmjs.org/...tgz` tarball URL. This still
trusts the npm registry when dependencies must be fetched, but it prevents a
future lockfile refresh from silently dropping tarball origin pins.

## Advisory Exception Policy

Scanner findings are release-blocking when they affect production
value-moving runtime paths or privileged operational authority. For Jupiter
Faucet, that attack surface is the production canister interaction path with
the ledger, index, CMC, NNS Governance, and SNS/system canisters, plus the
reserved lifeline rescue-controller principal. A finding is not accepted merely
because it appears in an ignore list.

Allowed RustSec ignores must be classified as one of:

- `production-value-moving-runtime`: release-blocking; do not ignore.
- `production-proc-macro-only`: non-runtime proc-macro support only, tracked
  separately from runtime vulnerabilities and accepted only with
  package-specific automated proof.
- `frontend-production-certification`: production frontend HTTP/asset certification;
  integrity matters for donor-facing destinations and source-verification evidence.
- `dev-test-only`: local tooling, mocks, tests, or PocketIC-only paths.

Global advisory ignores are allowed only when paired with automated
reachability checks. The specific GHSA-qwgh-2vcv-g2f7 exception below instead
requires the gate's exact-version applicability guard.
[`tools/scripts/check-production-reachability`](../../tools/scripts/check-production-reachability), called by
[`tools/scripts/security-scan`](../../tools/scripts/security-scan), validates the locked wasm-target normal/build
trees for `jupiter-disburser`, `jupiter-faucet`, `jupiter-relay`,
`jupiter-historian`, `jupiter-lifeline`, and `jupiter-sns-rewards`. Disburser,
Faucet, Relay, and Historian own the current production value-moving and
system-observation paths. `jupiter-lifeline` is included because it is the
configured reserved rescue-controller principal for operational canisters and
is therefore part of the privileged operational attack surface, even though
its current code is intentionally minimal. `jupiter-sns-rewards` is included
because its stable owner snapshots and SNS Root/Governance reads supply Relay's
reward-context and account-owner resolution path.

The gate fails if an explicitly forbidden package enters the covered production
trees, and it fails if `paste` appears there as anything other than proc-macro
support. `paste` may appear only as proc-macro support, not as runtime logic or
broader build-only support.

The current allowed RustSec findings are classified as:

| Advisory | Package | Classification | Enforced scope |
| --- | --- | --- | --- |
| `RUSTSEC-2025-0012` | `backoff 0.4.0` | `dev-test-only` | Must be absent from covered production value-moving and privileged operational wasm trees. |
| `RUSTSEC-2024-0384` | `instant 0.1.13` | `dev-test-only` | Must be absent from covered production value-moving and privileged operational wasm trees. |
| `RUSTSEC-2024-0436` | `paste 1.0.15` | `production-proc-macro-only` | May appear only as proc-macro support, not runtime logic or broader build-only support. |
| `RUSTSEC-2021-0127` | `serde_cbor 0.11.2` | `frontend-production-certification`, with additional `dev-test-only` PocketIC paths | Must be absent from covered production value-moving and privileged operational wasm trees. |

The exception evidence was last revalidated against current upstream releases:
PocketIC 15.0.0 still requires `backoff 0.4` and `serde_cbor 0.11.2`, and
`backoff 0.4` still requires `instant 0.1`; Candid 0.10.35 still requires
`paste 1.0`; the locked `ic-http-certification`/`ic-asset-certification` 3.2.0
certification path requires `serde_cbor 0.11`.

`serde_cbor 0.11.2` reaches production through the locked frontend
`ic-asset-certification` / `ic-http-certification` stack, and separately through
PocketIC dev/test tooling. Its unmaintained status is an accepted, scoped
maintenance risk, not evidence that this path is harmless: certified frontend
assets present payment destinations and the certificate-verification UI, so
frontend integrity can affect users even without backend transfer authority.
Protocol maintainers retain the pinned certification stack and this existing
exception until a supported upstream path permits removal. This exception does
not assert an active exploit or prove exploit absence. Reachability checks must
continue to exclude the package from covered production value-moving and
privileged operational canister runtimes.

Do not broaden ignores or silence a new advisory without adding a classification,
owner, mitigation path, and automated proof that the finding is outside the
production value-moving runtime path. The GHSA-qwgh-2vcv-g2f7 decision below is
the narrowly scoped exception based on absence of the vulnerable implementation
in the exact reviewed package. Production reachability must not be relabeled
as dev/test-only.

### GHSA-qwgh-2vcv-g2f7: reviewed block-buffer 0.10.4

The [advisory](https://github.com/RustCrypto/utils/security/advisories/GHSA-qwgh-2vcv-g2f7)
reports an inline buffer cursor corrupted when a callback panics, the panic is
caught, and the same buffer is reused. OSV's range includes every version below
0.12.1. Jupiter's locked `sha2 0.10.9 -> digest 0.10.7 -> block-buffer 0.10.4`
path reaches production Wasm, including value-moving canisters, principal
derivation and frontend certification. It also reaches native xtask,
NNS binding generation, build/proc-macro dependencies and PocketIC tests.

Classification: `version-specific-inapplicable-code`, owned by
**protocol-maintainers**, reviewed 27 October 2026. The
[0.10.4 implementation](https://docs.rs/block-buffer/0.10.4/src/block_buffer/lib.rs.html)
has a separate `pos: u8`; filling the block cannot overwrite it. Its compression
callback runs before updating that separate cursor, and its generation API
uses a local partial block before assigning it. There is no `ReadBuffer` in
this version. The advisory's inline-cursor failure therefore does not apply to
the reviewed implementation. This is Jupiter's source-based assessment, not a
claim that the upstream advisory range has been corrected or that arbitrary
hash computations can safely resume after a panic.

The existing canonical build uses Rust 1.94.1's `wasm32-unknown-unknown`
`panic="abort"` target; the locked CDK executor's panic hook calls `ic0::trap`.
A production panic cannot unwind into a handler that reuses the failed buffer;
IC traps roll back the trapping message's changes, not earlier messages across
an await. This is an additional boundary, not an assertion that panics cannot
occur. Native tooling/tests can unwind, so the Wasm argument alone does not
cover them. The separate-cursor review and native caught-panic regressions
cover the specific reported mechanism there as well. No Miri or live-canister
proof is claimed.

The latest compatible block-buffer release is 0.10.4. The published fix, 0.12.1,
does not satisfy digest 0.10.7's `block-buffer = "0.10"`; moving the RustCrypto
stack and its downstream consumers is outside a compatible lockfile fix.
No dependency or cryptographic implementation is changed for this exception.

Before OSV applies the single-advisory exception, the security gate runs
[`block_buffer_advisory.rs`](../../tools/xtask/tests/block_buffer_advisory.rs).
It requires exactly the reviewed registry package/version/archive checksum,
rejects changed or additional versions (including underscore spelling), and
exercises the advisory's compression panic/reuse plus the existing generation
API's panic paths. The exception cannot silently cover a newly resolved
block-buffer version. Other advisories and existing production exclusions
remain enforced; this GHSA-only entry does not add a RustSec ignore.

Review/remove the exception when upstream corrects applicability, publishes a
compatible release, or any buffering, dependency or panic model changes, and
immediately on contrary evidence. A failed guard requires a fresh review, not
updating its expected version merely to restore green CI.

## NNS Dependency Boundary

[`jupiter-nns-types`](../../crates/nns-types) provides minimal Candid-compatible
NNS Governance wire DTOs from the pinned subset DID under
[`candid/nns-governance/`](../../candid/nns-governance). The DTO file is
committed and verified by the dev-only
[`nns-bindgen-check`](../../tools/nns-bindgen-check) tool, which uses
`candid_parser` directly. `jupiter-nns-types` remains DTO-only and has no
`ic-cdk` dependency.

[`jupiter-ic-clients`](../../crates/ic-clients) contains the committed generated
raw NNS Governance transport. That crate already owns shared inter-canister
client code and depends on `ic-cdk`, so generated transport does not add
`ic-cdk` to the DTO crate. Production canister builds include plain Rust source
and do not run bindgen, depend on `ic-cdk-bindgen`, or rely on generated marker
extraction.

Keep broad DFINITY NNS graph crates such as `rsa`, `bincode`,
`proc-macro-error`, and `derivative` out of disburser, faucet, relay, and
historian production trees unless a separate dependency review explicitly
accepts the runtime scope.

## ICRC Index Transaction Schema Exception

The workspace pins `icrc-ledger-types` to DFINITY's mainline ICRC-122/152
squash-merge revision `d3b3351fa343a893aea2cfa8d39ac6b0d507b477`.
The published crates.io `0.2.0` package does not contain the committed Index transaction schema's
`authorized_mint` and `authorized_burn` fields or the official
`TRANSACTION_AUTHORIZED_MINT` (`122mint`) constant. Relay requires those exact
wire fields to reconstruct value-moving account history and fail closed on
unsupported operations.

[`deny.toml`](../../deny.toml) therefore permits only the DFINITY IC Git source,
while the workspace manifest pins the exact reviewed revision. Remove this
exception when a published `icrc-ledger-types` release contains the required
schema and the locked reverse dependency tree has been revalidated.

The script derives deterministic SBOM timestamps and serial numbers from
`SOURCE_DATE_EPOCH`. If `SOURCE_DATE_EPOCH` is unset, it uses the Unix timestamp
of the current `HEAD` commit (`git log -1 --pretty=%ct`). Override
`SOURCE_DATE_EPOCH` only when intentionally reproducing evidence for another
source date.

## Release Artifacts

The security gate writes CycloneDX SBOMs and hashes to:

```text
release-artifacts/sbom/cargo.cdx.json
release-artifacts/sbom/npm.cdx.json
release-artifacts/sbom/SHA256SUMS
```

Keep advisory and license exceptions narrow. Any exception in [`deny.toml`](../../deny.toml) must
explain why it is acceptable for production and whether the dependency is
runtime, build-time, test-only, dev-only, or frontend-production-certification. OSV
exceptions live in [`osv-scanner.toml`](../../osv-scanner.toml) because OSV Scanner does not read
[`deny.toml`](../../deny.toml); keep the two files aligned for shared RustSec findings.
