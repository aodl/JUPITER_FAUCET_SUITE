# Reproducible Builds

Release build helpers live under [`tools/scripts/`](../../tools/scripts).

```bash
./tools/scripts/build-canister all
./tools/scripts/docker-build
npm run verify:reproducible-artifacts
```

The reproducibility path is intentionally heavier than normal local validation and may require Docker access.
