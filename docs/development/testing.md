# Testing

Use the repository root as the working directory. The repo-aware [`xtask`](../../tools/xtask) utility is the preferred validation entry point because it knows which mocks, debug canister features, local identities, generated frontend assets, and ignored PocketIC tests belong to each suite.

Basic local setup checks:

```bash
cargo check --workspace --locked
npm run setup:frontend
```

Whole-suite validation commands:

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_local_integration
cargo run -p xtask -- test_pocketic_integration
cargo run -p xtask -- test_all
```

For narrow iteration, prefer the component commands documented in [`tools/xtask/README.md`](../../tools/xtask/README.md), such as `faucet_unit`, `historian_local_integration`, or `relay_pocketic_integration`.

Frontend tests are also available through npm scripts:

```bash
npm run build:frontend
npm run test:frontend-unit
npm run test:frontend-browser
npm run lint:frontend-exports
```

`test_unit` is an execution matrix, not a source-count check. It runs every
workspace library target through Cargo's workspace discovery, xtask's own binary
tests, the Disburser, Faucet, Historian, Relay and SNS Rewards `debug_api` variants in addition to their
default variants, the frontend asset-canister Rust tests, all mock-canister unit
tests, the Node controller/static tests, and the Playwright navbar browser test.
Before compiling workspace Rust tests, it prepares the locked frontend Node
dependencies, runs `npm run build:frontend`, and requires the generated manifest
and its nonempty hashed application bundle. This supports a clean checkout:
`include_dir!` embeds `public/` at compile time. Later Node suites reuse these
assets; the browser fixture still builds its distinct local-network bundle.
The runner requires complete libtest or Node summaries, reconciles Cargo's
discovered selections with execution per target, and requires at least one
passing test for each behavioural command. An explicitly empty library target
is accounted for; a successful zero-match suite filter is rejected.
Validators such as `validate-mainnet-install-args` are recorded separately and
are not assigned invented test counts.

`cargo test --workspace --doc --locked` checks doctest discovery. Cargo excludes
`cdylib`-only targets, and an empty doctest result is not treated as a positive
behavioural suite.

The browser test runs in the pinned Playwright container as the invoking user's
UID/GID, so its local generated bundle is writable by host build tools. It
loads a local-network bundle through the same asset-template renderer as the
canonical build. It rejects unresolved tokens, missing/broken bundle assets,
unexpected external requests, and application exceptions. The ordinary Node
navbar suite uses a fake DOM only for JavaScript state/event behavior; it does
not claim rendered geometry. Static source and markup tests protect structural
contracts, not clicking, focus, layout, scrolling, or hit-testing behavior.

The canonical Node 18 image installs production frontend build dependencies
only. `esbuild` is a production build dependency; Playwright stays dev-only
and runs in its pinned Node 24 browser image. Local test setup still installs
the full locked dependency set.

PocketIC integration sources live under [`tests/pocketic/`](../../tests/pocketic). Mock canisters used by local scenarios live under [`tests/mocks/`](../../tests/mocks). The long-running PocketIC suites are marked ignored under direct `cargo test`; `xtask` invokes them explicitly when a PocketIC or full-suite command is selected.

E2E prepares its canister Wasm caches before starting its first shared PocketIC
instance. A cold optimized link can exceed the server's idle timeout; compilation
must not leave a running test instance waiting on that build. This changes
fixture ordering, not canister deadlines, server timeouts, or value-flow assertions.

Dependency boundaries are intentional and must remain explicit:

- local `icp-cli` scenarios use the mock Ledger, Index, CMC, Governance, XRC,
  blackhole, and SNS canisters under `tests/mocks`;
- Faucet scheduler units use coherent account-history fixtures for nominal Index
  behavior and separately named scripted responses for malformed, stale, or
  interrupted behavior;
- Historian and E2E PocketIC conformance cases use PocketIC's pinned real ICP
  Ledger, Index, and CMC features where the claim is about those contracts;
- production-feature Wasm smoke tests and natural timer/lifecycle tests
  complement, but do not replace, deterministic debug-driver tests;
- serialized Candid size, physical stable-memory size, cycle deltas, and Index
  call/record counts are distinct measurements. Local printed cycle samples are
  diagnostics, not universal production bounds.

The nominal ICP Index mock follows the pinned Index account-history contract.
Its unit matrix covers cursor bounds, ordering, operation-side membership,
oldest anchors, balances, limits and explicit lag. PocketIC cases independently
cover real pagination, outgoing and self transfers, mint and approval
membership, and delegated-transfer wire decoding. Malformed ordering and
cursor fixtures are separately named fault injection rather than being
normalized into nominal history.

`Behavioral Tests` runs on ordinary `pull_request`, pushes to `main`/`master`,
and manual dispatch. Its always-selected matrix uses existing xtask discovery
and execution checks, including ignored PocketIC suites. Hosted runners have
read-only repository permission, no deployment secrets, no persisted checkout
credentials, bounded timeouts, and cancellation of superseded runs. Native
debug information/incremental caches are disabled in CI to reduce disk use;
the canonical Docker build configuration is unchanged.

Provisioning pins Node 24.15.0, repository Rust, icp-cli 1.2.0, didc 0.6.2 and
the launcher version in `icp.yaml`. The launcher is downloaded by a local
network start/stop, then xtask verifies its server version. Browser tests use
the existing Playwright image. Missing tools or unavailable suites fail; there
is no release skip mode.

Both behavioral and canonical-release jobs install the official DFINITY
`didc-v0.6.2` x86_64 Linux MUSL release archive, checking its pinned SHA-256
before extraction/installation. The workflow's shared version/digest pins
must be reviewed together; didc is not installed from crates.io or a moving
installer. Local acceptance must use that version for DID encode/decode,
strict compatibility and actual Wasm export checks, not just `--version`.

Repository owners must require these exact statuses on pull requests:

- `Behavioral (test_unit)`
- `Behavioral (faucet_pocketic_integration)`
- `Behavioral (historian_pocketic_integration)`
- `Behavioral (relay_pocketic_integration)`
- `Dependency security scan`

Require `Canonical release validation` for release promotion of a `main`/`master`
commit, together with retained full local acceptance from
`POCKET_IC_MUTE_SERVER=1 cargo run --locked -p xtask -- test_all`. That release
job depends on every behavioural matrix job, builds canonical artifacts, checks
checksums, artifact-aware arguments, actual Wasm exports/DID modes and embedded
child binding, production reachability, read-only preflight, current security/
SBOM evidence and two clean same-environment reproducibility builds. Security
scanning runs after the canonical build so its SBOMs are retained.

`npm run check:production-exports` requires every canonical raw Wasm. It checks
the embedded Candid structurally with didc, checks actual exports/modes with
ic-wasm (only explicit lifecycle/CDK hooks are hidden), rejects debug/admin
exports, and requires the exact candidate Relay gzip payload in Historian plus
raw/gzip agreement. This complements the reviewed hash pins and local factory
smoke tests; it does not establish an installed-mainnet identity.

After `docker-build`, also run
`POCKET_IC_MUTE_SERVER=1 cargo run --locked -p xtask -- release_artifact_validation`.
This uses the same runner's positive-execution checks for the export gate and
two exact PocketIC scenarios: the canonical no-debug Historian creates and
finalizes its embedded no-debug Relay, and the canonical no-debug Faucet
upgrades at quiescence then pays through its ordinary timers. No production
test endpoint or mainnet operation is required.

Canonical CI and the Historian behavioral CI lane download the immutable
`ninegua/ic-blackhole` 0.0.0 release Wasm into runner-temporary storage. The
workflow verifies the pinned production SHA-256 before exporting the path as
`JUPITER_BLACKHOLE_TEST_WASM`; the PocketIC fixture independently verifies the
same hash when loading it. A failed download, missing file or mismatched hash
fails closed, and a supplied override never falls back to `make` or Nix.

The checked-in [`vendor/ic-blackhole`](../../vendor/ic-blackhole) source and
historical Nix recipe remain an independent local provenance and
reproducibility path through `make repro-build`. They are not a hosted-runner
prerequisite. Release and behavioral CI execute the exact official release
binary whose pinned SHA-256 is also the production module identity.

Workflow source cannot establish branch protection or an actual successful PR
run. Operators must verify required-check settings and retain an ordinary-PR
execution on the candidate. No release job deploys or authorizes deployment.
The 2026-10-16 inspection found `master` unprotected with no repository rulesets.
Enable and verify branch/ruleset protection requiring the five PR statuses
listed above; a green workflow without enforcement does not close this gate.
Retain green candidate run IDs for all four Behavioral jobs and Dependency
Security, plus a non-PR `Canonical release validation` before release promotion.
Run `38977454083` on `34fc00441bead8cd1d7d43408d124f836c02c126`
failed during didc provisioning before any selected suite ran; its skipped
canonical job is not release evidence.
