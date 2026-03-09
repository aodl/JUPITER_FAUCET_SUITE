# Jupiter Faucet Suite

Jupiter Faucet is a suite of Internet Computer canisters utilising a max-staked NNS neuron that follows alpha-vote on all topics to maximise maturity. The purpose is to power unstoppable dapps by delivering a simple, set-and-forget, perpetual cycles top-up solution that's trustless, permissionless and immutable. The Internet Computer is designed for tamperproof, "unstoppable" on-chain services; Jupiter Faucet focuses on the practical part: making sure canisters don’t run out of cycles, even if nobody is maintaining the project.

The top-up process is intentionally simple: transfer at least 0.1 ICP directly into the staking account of Jupiter Faucet's canister-controlled neuron and set your target canister ID as the transaction memo. You do not need to be the owner of the canister. That’s it. With your memo declaring a canister, that canister becomes the recipient of perpetual cycles top-ups, funded by the stake’s ongoing rewards (propotional to your ICP contribution and any further contributions made for that canister).

[Adopters receive front-loaded JUP SNS airdrops.](https://jupiter-faucet.com/#intro)

The core production canisters are:

- `jupiter-disburser` (`uccpi-cqaaa-aaaar-qby3q-cai`)
- `jupiter-faucet` (`acjuz-liaaa-aaaar-qb4qq-cai`)
- `jupiter-sns-rewards` (`alk7f-5aaaa-aaaar-qb4ra-cai`)
- `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)

These canisters are deployed on the Fiduciary subnet (`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`) for the stronger consensus profile and operational security expected for treasury-facing infrastructure.

## Why the suite is structured this way

The system is intentionally split into narrowly scoped canisters:

- `jupiter-disburser` controls a single NNS neuron, disburses maturity, and routes minted ICP according to a fixed policy.
- `jupiter-faucet` receives the age-neutral base payout and, once implemented, will convert it into cycles top-ups for participating canisters. User / protocol stake top-ups into the reward neuron are currently recognized by `jupiter-disburser` via periodic best-effort `ClaimOrRefresh`; in the intended user flow, the deposit memo identifies the canister the user wants topped up.
- `jupiter-sns-rewards` receives the primary age-bonus payout and will distribute it to JUP SNS stakers.
- `jupiter-lifeline` is the recovery controller target for blackholed operational canisters.
- `jupiter-faucet-frontend` is the placeholder for the public-facing asset canister.

## Why `jupiter-disburser` and `jupiter-faucet` are blackholed

`jupiter-disburser` and `jupiter-faucet` are intended to be blackholed after deployment has been validated. In this repository, that means operator control is handed off to canister-controlled self-management rather than leaving the canister literally controllerless. The point is to keep the core maturity-routing and cycle-top-up path operationally immutable during normal operation while still allowing the canister to reconcile to a recovery controller when its persisted rescue policy says that value flow has stopped for long enough.

That immutability matters even in the unlikely event of a successful governance attack elsewhere in the Jupiter ecosystem. If the payout path can be upgraded at will, the top-up policy can be changed at will. Blackholing narrows that risk and makes the core value flow materially harder to tamper with.

## Why the lifeline canister exists

A completely blackholed operational path without any recovery option would be too brittle. `jupiter-lifeline` exists to hold a pre-positioned recovery role for blackholed canisters.

If value flow stops for long enough to satisfy the local rescue policy, the affected canister can widen its controller set to include `jupiter-lifeline`. That handoff is driven by persisted local state and a management-canister controller update. It does not depend on a fresh ledger or governance API health check at the point of escalation.

That design avoids a circular failure mode where a system API change could both stop normal transfers and also block the recovery handoff.

The same recovery pattern is intended for both `jupiter-disburser` and `jupiter-faucet`.

## Operational invariant before blackholing

Do not blackhole `jupiter-disburser` or `jupiter-faucet` until at least one successful payout / top-up event has occurred and been recorded.

For `jupiter-disburser`, rescue is armed by the timestamp of the last confirmed successful transfer. Before that timestamp exists, the canister has no evidence that value flow was ever working and will not trigger the lifeline handoff.

## Current `jupiter-disburser` production configuration

The current production recipients are:

- normal recipient: `jupiter-faucet` (`acjuz-liaaa-aaaar-qb4qq-cai`)
- age bonus recipient 1: `jupiter-sns-rewards` (`alk7f-5aaaa-aaaar-qb4ra-cai`)
- age bonus recipient 2: the staking account for the D-QUORUM known neuron on NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai` + subaccount `77e63de72b5e3339ea20f4baf3ec2bd92138ddde0daeb69db50acceb384bdf0f`)
- rescue controller: `jupiter-lifeline` (`afisn-gqaaa-aaaar-qb4qa-cai`)
- ledger canister: ICP Ledger (`ryjl3-tyaaa-aaaaa-aaaba-cai`)
- governance canister: NNS Governance (`rrkah-fqaaa-aaaaa-aaaaq-cai`)
- `blackhole_armed = opt false`
- `main_interval_seconds = 86400`
- `rescue_interval_seconds = 86400`

The D-QUORUM recipient is intentional. Jupiter Disburser routes 20% of the age-bonus portion to the staking account for the D-QUORUM known neuron to support NNS due diligence and help fund NNS security.

A copy-pasteable mainnet install argument file lives at:

- `jupiter-disburser/mainnet-install-args.did`

## Repository layout

- `jupiter-disburser/` — NNS maturity routing canister
- `jupiter-faucet/` — faucet placeholder / future cycles top-up canister
- `jupiter-sns-rewards/` — SNS rewards placeholder / future staking rewards canister
- `jupiter-lifeline/` — minimal recovery canister
- `jupiter-faucet-frontend/` — frontend placeholder canister
- `xtask/` — local orchestration, mocks, and PocketIC tests
- `scripts/` — reproducible build helpers
- `release-artifacts/` — built reproducible artifacts

## Reproducible builds

The pinned Docker build emits artifacts for the full suite:

```bash
chmod +x scripts/docker-build scripts/build-canister
./scripts/docker-build
```

The deployment package for each canister is the `.wasm.gz` file in `release-artifacts/`. Verification is performed against the uncompressed `.wasm` hash printed by the build script.

## Mainnet deployment and upgrade commands

### Create canisters on the Fiduciary subnet

```bash
dfx deploy --network=ic --subnet=pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae jupiter_lifeline
dfx deploy --network=ic --subnet=pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae jupiter_faucet
dfx deploy --network=ic --subnet=pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae jupiter_sns_rewards
```

### Install `jupiter-disburser`

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --wasm release-artifacts/jupiter_disburser.wasm.gz \
  --argument-file jupiter-disburser/mainnet-install-args.did
```

### Required post-install settings

Actioned before handing controller ownership away from the deployment operator.

Make logs public on all deployed canisters:

```bash
dfx canister update-settings jupiter_disburser --network ic --log-visibility public
dfx canister update-settings jupiter_lifeline --network ic --log-visibility public
dfx canister update-settings jupiter_faucet --network ic --log-visibility public
dfx canister update-settings jupiter_sns_rewards --network ic --log-visibility public
dfx canister update-settings jupiter_faucet_frontend --network ic --log-visibility public
```

Added `jupiter-disburser` as a controller of itself. This is required for the canister's internal controller reconciliation and rescue escalation flow to work:

```bash
dfx canister update-settings jupiter_disburser \
  --network ic \
  --add-controller uccpi-cqaaa-aaaar-qby3q-cai
```

After at least one successful payout has occurred, logs are configured, and `blackhole_armed` has been enabled, hand `jupiter-disburser` off to self-only control with:

```bash
dfx canister update-settings jupiter_disburser \
  --network ic \
  --set-controller uccpi-cqaaa-aaaar-qby3q-cai
```

The future `jupiter-faucet` production rollout should use the same self-controller handoff pattern once its rescue logic stops being a stub. Its top-up attribution flow can still rely on users transferring into the neuron's staking account with the target canister id encoded in the expected memo / attribution flow, but the current periodic best-effort `ClaimOrRefresh` for that neuron now lives in `jupiter-disburser`, not in user flow.

### Upgrade commands

`jupiter-disburser` persists its configuration across upgrades. Ordinary code upgrades do not require an argument. Blackhole self-management is intentionally installed in an unarmed state (`blackhole_armed = opt false`) until the canister is ready to self-blackhole.

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_disburser.wasm.gz

dfx canister install jupiter_lifeline \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_lifeline.wasm.gz

dfx canister install jupiter_faucet \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_faucet.wasm.gz

dfx canister install jupiter_sns_rewards \
  --network ic \
  --mode upgrade \
  --wasm release-artifacts/jupiter_sns_rewards.wasm.gz
```

### Arm blackhole self-management 

```bash
dfx canister install jupiter_disburser \
  --network ic \
  --mode upgrade \
  --argument '(opt record { blackhole_armed = opt true; })' \
  --wasm release-artifacts/jupiter_disburser.wasm.gz
```

## Component documentation

- `jupiter-disburser/README.md`
- `jupiter-faucet/README.md`
- `jupiter-sns-rewards/README.md`
- `jupiter-lifeline/README.md`
- `jupiter-faucet-frontend/README.md`
