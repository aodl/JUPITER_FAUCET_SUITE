# Jupiter Faucet

`jupiter-faucet` receives the age-neutral ICP flow from `jupiter-disburser`, converts it into cycles top-ups, and routes those top-ups proportionally across beneficiary canisters inferred from memo-tagged deposits into the faucet neuron’s staking account.

## Configuration

Install-time arguments configure:

- `staking_account`: the neuron staking account whose incoming ICP transfers define beneficiary stake commitments
- `payout_subaccount`: optional faucet payout subaccount; if omitted, the faucet uses its default account, which matches the current suite wiring from `jupiter-disburser`
- `ledger_canister_id`: defaults to ICP Ledger
- `index_canister_id`: defaults to ICP Index
- `cmc_canister_id`: defaults to the Cycles Minting Canister
- `rescue_controller`: the `jupiter-lifeline` canister in production
- `blackhole_armed`, `main_interval_seconds`, `rescue_interval_seconds`
- `min_tx_e8s`: minimum staking contribution considered for attribution; defaults to 0.1 ICP

## Execution model

Each main tick:

1. snapshots the faucet payout balance and the staking-account denominator
2. scans the staking account’s indexed history from the beginning
3. processes valid contributions one transfer at a time
4. transfers ICP to the corresponding CMC deposit subaccount and calls `notify_top_up`
5. persists only the minimal in-flight state needed to retry safely after an inter-canister failure

Important properties of this flow:

- every new payout job revisits the full contribution history
- contributions are processed independently; they are not aggregated by beneficiary
- the faucet does not buffer the full contribution set in memory
- malformed, under-threshold, dust, and other deterministic per-contribution failures are skipped immediately and do not block later contributions in the same payout job
- ambiguous or plausibly transient failures are retried once after roughly 60 seconds; if the retry still fails, that contribution is skipped and the rest of the job continues
- the faucet never retries forever and never aborts a payout job because a single contribution fails
- skipped contributions do not block the payout job; any value not successfully allocated is handled by the existing payout-job accounting, including remainder-to-self behavior where applicable

## Public interface

Production builds expose no public endpoints.

The debug build exposes query/update methods for local integration and PocketIC tests via the `debug_api` feature.

## Build and test

```bash
cargo build -p jupiter-faucet --target wasm32-unknown-unknown --release --locked
cargo build -p jupiter-faucet --target wasm32-unknown-unknown --release --features debug_api --locked
cargo test -p jupiter-faucet --lib
RUST_TEST_THREADS=1 cargo test -p jupiter-faucet --test jupiter_faucet_integration -- --ignored
RUST_TEST_THREADS=1 cargo test -p jupiter-faucet --test e2e -- --ignored
