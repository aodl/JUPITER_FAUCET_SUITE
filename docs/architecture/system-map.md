# System Map

The Jupiter Faucet Suite is split into production canisters, shared internal crates, integration-test assets, developer tooling, and vendored third-party source.

- Production canisters live under `canisters/`.
- Shared Rust support crates live under `crates/`.
- Mock canisters and PocketIC integration tests live under `tests/`.
- Repo-aware automation lives under `tools/`.
- Vendored external source lives under `vendor/`.

This first-pass structure change preserves Rust package names, IC canister names, Candid interfaces, and runtime behavior.
