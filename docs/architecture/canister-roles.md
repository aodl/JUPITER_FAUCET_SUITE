# Canister Roles

Jupiter Faucet is organized as a suite of narrow Internet Computer canisters instead of one broad service. The value-moving code is split from the public read model and certified frontend so each component has a small operational surface and a clear verification target.

## Operational Path

- [`canisters/disburser`](../../canisters/disburser) controls the configured NNS neuron, initiates maturity disbursement, stages ICP in its own default ledger account, and routes the resulting ICP according to the fixed base and age-bonus policy.
- [`canisters/faucet`](../../canisters/faucet) receives the base ICP flow, scans the configured staking account through the ICP Index, parses supported memo directives, and performs proportional top-ups or transfers for eligible commitments.
- [`canisters/relay`](../../canisters/relay) receives suite-funding ICP from the faucet, samples cycles balances for managed canisters, tops up recent burn plus headroom, routes usable surplus, and attributes completed subaccount-1 commitments before forwarding the current SNS reward-token pot to one eligible winner.
- [`canisters/sns-rewards`](../../canisters/sns-rewards) resolves the configured SNS canister set through Root, publishes stable daily snapshots of neuron-owner default ICP accounts, and supplies snapshot-scoped reward context and bounded lookups to Relay. It does not distribute the future general SNS rewards program.

These canisters are deliberately conservative about public production APIs. Most verification happens through source review, logs, module hashes, Candid files, and the historian/frontend read surfaces.

## Observability Path

- [`canisters/historian`](../../canisters/historian) maintains the public read model for commitments, tracked canisters, output and reward flows, cycles samples, SNS discovery, and dashboard status.
- [`canisters/frontend`](../../canisters/frontend) serves the certified public site and dashboard. It reads dashboard data from the historian plus generated ledger and NNS actor declarations.

The historian keeps bounded, dashboard-friendly views in canister state. The ICP ledger and archive canisters remain the source of full transfer history.

### Public runtime configuration invariant

Every production canister with install-time or mutable runtime configuration periodically re-publishes its complete effective configuration in public canister logs. This keeps the running configuration community-verifiable after the original install or upgrade records have rolled out of bounded log history. The recurring representation describes values actually in use after defaults, upgrades, clamps, and other normalization—not merely the original argument payload.

The representation must distinguish every materially different effective configuration, including lossless deterministic encodings for binary values and distinct encodings for absent versus present-empty values where both are valid effective runtime states. A dedicated configuration-only publication mechanism must not depend on unrelated external services being healthy. Canisters that publish configuration through an established recurring operational or health path must document that cadence and coupling explicitly. Adding a persistent or effective configuration field therefore also requires updating the recurring representation and its regression tests. Configuration logging is observability only: it must not drive protocol decisions, scheduling due-ness, stable state, payouts, reward attribution, or recovery behavior.

The current implementations are:

- Disburser re-publishes effective `CONFIG` through normal main ticks that reach its established payout/maturity path.
- Faucet logs effective `CONFIG` with completed main ticks.
- Historian re-publishes complete effective `CONFIG` through its established periodic cycles-observability path.
- Relay re-publishes complete effective `CONFIG` on its normal main cadence, including lossless deterministic hexadecimal surplus-recipient memos.
- SNS Rewards re-publishes its sole configured SNS Root through an independent daily observability-only timer and separately logs the Root/Governance/Ledger context pinned for accepted scans.
- Lifeline has no install/upgrade runtime configuration beyond code-defined behavior.
- The frontend has no comparable canister runtime `InitArgs` or `UpgradeArgs` configuration.

## Recovery and Support

- [`canisters/lifeline`](../../canisters/lifeline) is the minimal recovery/support canister used when a value-moving canister widens controllers after a sustained failure condition.
- Relay, rather than Historian, owns commitment attribution and reward-token forwarding. Historian has no registry, eligibility, Ledger-discovery, or settlement role in this flow.

The blackhole pattern makes selected canister status public and supports immutable or self-managed controller postures. Recovery behavior is intentionally component-specific; see the relevant canister README for the exact rescue windows and controller transitions.
