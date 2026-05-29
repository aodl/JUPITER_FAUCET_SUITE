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
npm run lint:frontend-exports
```

PocketIC integration sources live under [`tests/pocketic/`](../../tests/pocketic). Mock canisters used by local scenarios live under [`tests/mocks/`](../../tests/mocks). The long-running PocketIC suites are marked ignored under direct `cargo test`; `xtask` invokes them explicitly when a PocketIC or full-suite command is selected.
