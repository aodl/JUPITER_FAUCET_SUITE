# Testing

Use `xtask` for repository-aware validation:

```bash
cargo run -p xtask -- test_unit
cargo run -p xtask -- test_local_integration
cargo run -p xtask -- test_pocketic_integration
cargo run -p xtask -- test_all
```

Frontend tests are available through npm scripts:

```bash
npm run build:frontend
npm run test:frontend-unit
npm run lint:frontend-exports
```

PocketIC integration sources live under `tests/pocketic/`. Mock canisters used by local scenarios live under `tests/mocks/`.
