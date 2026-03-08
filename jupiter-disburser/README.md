# Jupiter Disburser

`jupiter-disburser` controls a single NNS neuron, disburses maturity to its own staging account, and then routes the minted ICP according to a fixed split policy.

## Current mainnet canister and recipients

- canister id: `uccpi-cqaaa-aaaar-qby3q-cai`
- neuron id: `11614578985374291210`
- normal recipient: `jupiter-faucet` (`acjuz-liaaa-aaaar-qb4qq-cai`)
- age bonus recipient 1: `jupiter-sns-rewards` (`alk7f-5aaaa-aaaar-qb4ra-cai`)
- age bonus recipient 2: D-QUORUM known-neuron staking account on NNS Governance
  - owner: `rrkah-fqaaa-aaaaa-aaaaq-cai`
  - subaccount: `77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f`
- rescue controller: `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)
- ledger canister: `ryjl3-tyaaa-aaaaa-aaaba-cai`
- governance canister: `rrkah-fqaaa-aaaaa-aaaaq-cai`
- main interval: `86400`
- rescue interval: `86400`

The D-QUORUM recipient is intentional. Jupiter Disburser routes 20% of the age-bonus portion to the D-QUORUM staking account to support NNS due diligence and help fund NNS security.

## What the canister does

On each main cycle, the canister:

1. reads the current neuron state
2. finalizes any payout already staged on the ledger
3. initiates a new maturity disbursement when no governance-side disbursement is already in flight
4. records the neuron age at initiation time so the later split reflects the age that produced the reward
5. refreshes voting power after a successful maturity-disbursement initiation

When staged ICP is present, the canister derives:

- `base`
- `bonus = total - base`
- `bonus_1 = 80% of bonus`
- `bonus_2 = 20% of bonus`

The resulting transfers are executed with ICRC-1 ledger calls.

## Design notes

### Staging first, payout second

NNS Governance disburses maturity to a single destination per call. Jupiter Disburser always disburses to its own staging account first and only then performs the recipient split on the ledger side.

That keeps the payout logic grounded in the amount actually minted and makes retries easier to reason about than repeated governance calls.

### Persistent payout plans

Before the first transfer is attempted, the canister persists a payout plan. If a transfer succeeds but execution stops before the plan is cleared, the next run can safely resume without recalculating the split from a partially-updated state.

### Main tick overlap protection

Main-tick overlap protection uses an expiring lease instead of a persistent boolean lock. If execution stops after an await boundary, the lease self-heals and the next scheduled run can proceed.

### Lifeline-based disaster recovery

Jupiter Disburser is configured with a required `rescue_controller`. In production this is the principal of `jupiter-lifeline`.

The rescue path is intentionally independent of fresh ledger, governance, or canister-status checks. It uses the timestamp of the last confirmed successful transfer already stored in canister state and a management-canister controller update.

If rescue escalation is required and the controller update fails, the canister retries on the next rescue interval. Failed rescue attempts do not consume a long backoff window.

### Blackholing precondition

Do not blackhole `jupiter-disburser` until at least one successful payout has occurred.

The rescue policy is armed by the timestamp of the last confirmed successful transfer. Before that timestamp exists, the canister has no evidence that value flow was ever working and will not trigger the lifeline handoff.

## Install-time configuration

A copy-pasteable mainnet install argument file lives at:

- `mainnet-install-args.did`

That file currently contains:

```candid
(
  record {
    neuron_id = 11614578985374291210 : nat64;
    normal_recipient = record {
      owner = principal "acjuz-liaaa-aaaar-qb4qq-cai";
      subaccount = null;
    };
    age_bonus_recipient_1 = record {
      owner = principal "alk7f-5aaaa-aaaar-qb4ra-cai";
      subaccount = null;
    };
    age_bonus_recipient_2 = record {
      owner = principal "rrkah-fqaaa-aaaaa-aaaaq-cai";
      subaccount = opt vec { 119; 230; 61; 231; 43; 94; 51; 57; 234; 32; 244; 186; 243; 236; 43; 217; 33; 56; 221; 222; 13; 174; 182; 157; 181; 10; 204; 235; 56; 75; 223; 15 };
    };
    ledger_canister_id = opt principal "ryjl3-tyaaa-aaaaa-aaaba-cai";
    governance_canister_id = opt principal "rrkah-fqaaa-aaaaa-aaaaq-cai";
    rescue_controller = principal "afisn-gqaaa-aaaar-qb4qa-cai";
    main_interval_seconds = opt 86400;
    rescue_interval_seconds = opt 86400;
  }
)
```

## Public interface

Production builds expose:

- `metrics() -> Metrics`

Debug-only methods used for local integration and PocketIC tests are gated behind the `debug_api` feature and are not intended for production deployment.

## Build and test

### Production build

```bash
cargo build -p jupiter-disburser --target wasm32-unknown-unknown --release --locked
```

### Debug build

```bash
cargo build -p jupiter-disburser --target wasm32-unknown-unknown --release --features debug_api --locked
```

### Unit tests

```bash
cargo test -p jupiter-disburser --lib
```

### PocketIC end-to-end tests

```bash
RUST_TEST_THREADS=1 cargo test -p jupiter-disburser --test pocketic_e2e -- --ignored
```

## Reproducible build and deployment

```bash
chmod +x scripts/docker-build scripts/build-canister
./scripts/docker-build
```

Install the release artifact with:

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --wasm release-artifacts/jupiter_disburser.wasm.gz \
  --argument-file jupiter-disburser/mainnet-install-args.did
```

Upgrades preserve the existing configuration and do not require an argument:

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

Verification is performed by comparing the deployed module hash to the SHA-256 of `release-artifacts/jupiter_disburser.wasm` printed by `./scripts/docker-build`.
