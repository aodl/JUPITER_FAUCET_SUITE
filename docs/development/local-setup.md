# Local Setup

Use the repository root as the working directory.

```bash
cargo check --workspace --locked
npm run setup:frontend
```

The repo-aware `xtask` utility lives at `tools/xtask/` but is invoked by package name:

```bash
cargo run -p xtask -- test_unit
```
