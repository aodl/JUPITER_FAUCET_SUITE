# Work package A validation — 14 November 2026

This is an offline acceptance preparation candidate, not an installation or a
canonical production release. No IC mainnet observations or mutations were made.
Real authority, participant, timestamp and helper-preservation inputs remain
unresolved. The guarded candidate command rejects those inputs before creating
an output directory; no deployable real constructor bundle is included.

Source baseline: master `0373056528de6e502d94b277ee7ceaf2d66dd85b`, tree
`d19a64ae264b2e3912add64f15d82559d1c711e9`, identical to the reviewed PR17 tree.
Canister behavior, public interfaces, stable schemas, economic policy and lockfiles
are unchanged. Local fixture state is never evidence of existing mainnet state.

## Executed checks

Commands used didc 0.6.2 from the checksum-verified official archive, ic-wasm
0.11.0 and the repository Rust toolchain. The verified official SNS cache was
`/home/codexdev/.cache/verified-sns/4d5e00cd65581c64`.

- `python3 -m unittest discover -s tools/acceptance -p 'test_*.py' -v`:
  36 tests passed after the final preparation changes. Environment:
  `PYTHONDONTWRITEBYTECODE=1` and `JUPITER_SNS_ARTIFACT_DIR` pointing to that cache.
- `cargo fmt --all -- --check`: passed.
- `python3 tools/scripts/validate-mainnet-install-args`: passed.
- `tools/scripts/check-production-reachability`: passed.
- `python3 tools/acceptance/artifacts.py`: all six official raw/gzip hashes,
  embedded Candid and source revisions verified.
- `python3 tools/acceptance/profile.py check-roles deploy/acceptance/profile.json`:
  passed.
- `POCKET_IC_MUTE_SERVER=1 cargo run --locked -p xtask -- acceptance_sns`:
  passed, two discovered/selected/executed scenarios. This includes the existing
  real SNS sale rehearsal and the new operational acceptance case. The saved
  subprocess result records exit zero and complete successful teardown; an outer
  terminal wrapper later returned 143, which is not attributed to Cargo.
- After strengthening the operational fixture's exact canister IDs, initial 3T
  balances and second physical upgrade-hash checks, the affected case passed:
  `E2E_TRACE=1 POCKET_IC_MUTE_SERVER=1 RUST_TEST_THREADS=1 JUPITER_POCKETIC_IDLE_TTL_SECS=3600 cargo test --locked -p jupiter-relay --test jupiter_relay_integration acceptance_sns::operational::exact_genesis_governance_handover_and_production_reserve -- --exact --ignored --nocapture`.
  One passed, 61 filtered. The Python suite, formatting and install-args checks
  also passed after the final changes. The unchanged real-sale case was not rerun.

The new case uses owned synthetic actors at the approved twelve canister IDs in
fresh local PocketIC state. It proves the exact 100-million supply and four
5,000-jUP test neurons, native grant/final-executor vote/revoke ordering,
unchanged dominant neuron staking Account, actual Governance-controlled
production Rewards upgrades, reserve disbursement and preserved receipt/no
replay after a distinct complete gzip upgrade. It does not verify the owner's
signer, real NNX route or existing NNS neuron. Unchanged cross-component financial
and recovery mechanics retain the source-bound PR17 and real-sale evidence.

## Acceptance artifacts

Each of the following roles was built twice using the existing command:
`python3 tools/acceptance/build.py deploy/acceptance/profile.json OUTPUT ROLE`.
Roles: `jupiter_historian`, `jupiter_disburser`, `jupiter_faucet`,
`jupiter_lifeline`, `jupiter_sns_rewards`, `jupiter_faucet_frontend`.
Historian builds and embeds the acceptance-specific Relay. Outputs were
`target/acceptance-preparation-a/artifacts` and `comparison`.
All fourteen raw/gzip application files matched. This uses the retained locked
Cargo cache and is **not independent clean reproducibility**.

`python3 tools/scripts/check-production-exports --artifacts-dir target/acceptance-preparation-a/artifacts`
passed strict Candid/export/debug separation and embedded Relay checks. The
frontend binary's acceptance Historian, Relay, neuron and payment Account bindings
were checked separately. Build records explicitly mark unresolved constructor
inputs and deny canonical-production/mainnet-operation status.

Acceptance Relay raw SHA-256:
`b1e79e4dabf7919eb764217c299ff110168cdfff0c97156e9581520b8fa39e80`.
Compressed install-package SHA-256:
`d6fc978715d55fea3c417e6b247524808a8ad283354fb6a28c606c416972a9e7`.
These are acceptance hashes, not reviewed production Relay hashes.

The review package is `target/acceptance-preparation-a/review-package`;
its sibling `review-package.tar.gz` is the portable artifact. It contains the
seven application pairs, six verified framework templates (archive is a template,
not a thirteenth installation), public configuration/operation plans, validation
records and source/tool/file hashes. Synthetic local constructor binaries are
excluded. Full local logs are retained under
`/home/codexdev/.cache/jupiter-work-a`.

## Limits and deferred checks

No full `test_all`, new general security review, fresh advisory scan or canonical
Docker reproducibility run was commissioned: no merged canister behavior,
interfaces, stable schema or dependency inputs changed. The acceptance aggregate
and final affected-case checks exercise the changed preparation inputs. No
frontend source changed, so no prototype deployment was applicable. No mainnet
read, installation, transfer, controller change, neuron mutation or autonomy
arming occurred. Hosted CI is separate from these local results.

The next approval is the bounded pre-installation read inventory, after selecting
an existing authorized provider; post-installation checks need later approval.
The funding sheet is proposed only, with zero authorized spend. Observed balances,
fees, visibility and helper-preservation facts must precede a concrete live
installation/funding approval. See `work-package-a.md` for the ordered commands
and the exact remaining owner inputs.
