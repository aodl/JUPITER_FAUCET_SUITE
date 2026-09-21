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
tests, the Relay and SNS Rewards `debug_api` variants in addition to their
default variants, the frontend asset-canister Rust tests, all mock-canister unit
tests, the Node controller/static tests, and the Playwright navbar browser test.
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

The root CI automatically enforces dependency security only. The separate
`Behavioral Tests` workflow is manual (`workflow_dispatch`) and provides a
prepared hosted-runner unit gate; it is not a branch-protection claim. Full
local/PocketIC acceptance uses
`POCKET_IC_MUTE_SERVER=1 cargo run -p xtask -- test_all` until repository owners
explicitly approve an automatic long-running CI policy.
