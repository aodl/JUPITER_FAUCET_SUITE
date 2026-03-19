# Vendored response-verification crates

This directory vendors the minimal Rust crates needed by `jupiter-faucet-frontend` for certified static asset serving.

Source provenance:
- migrated from the legacy standalone Jupiter frontend repo that previously carried these crates inline
- originally derived from DFINITY's response-verification project

They are kept vendored here so the Jupiter Faucet Suite stays self-contained and reproducible without introducing a second frontend-specific workspace at the repository root.
