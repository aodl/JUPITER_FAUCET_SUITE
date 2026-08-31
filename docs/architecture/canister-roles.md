# Canister Roles

Jupiter Faucet's purpose is to provide a reusable infrastructure layer for long-term ICP funding, maturity routing, cycles allocation, and related observability while applications retain their own business and incentive logic. Integrating with the canonical mainnet deployment gives applications the established stake and maturity flow, shared observability, and self-service Relay factory described in [Jupiter Faucet as shared infrastructure](shared-infrastructure.md).

The narrow-canister decomposition implements that model; it is not the purpose by itself. Value-moving code is split from the public read model and certified frontend so each component has a small operational surface and a clear verification target.

## Operational Path

- [`canisters/disburser`](../../canisters/disburser) controls the configured NNS neuron, initiates maturity disbursement, stages ICP in its own default ledger account, and routes the resulting ICP according to the fixed base and age-bonus policy.
- [`canisters/faucet`](../../canisters/faucet) receives the base ICP flow, scans the configured staking account through the ICP Index, parses supported memo directives, and performs proportional top-ups or transfers for eligible commitments.
- [`canisters/relay`](../../canisters/relay) receives suite-funding ICP from the faucet, samples cycles balances for managed canisters, tops up recent burn plus headroom, routes usable surplus, and shares the current SNS reward-token pot pro rata among eligible funders of the newest qualifying completed subaccount-1 commitment recorded before both the owner snapshot and the oldest unspent reward credit. It resolves the reward Index through the pinned SNS Root, reconstructs the current token-balance epoch and FIFO credit timestamps statelessly, and paginates both reward and ICP history on demand without persistent attribution cursors or arbitrary depth cutoffs. ICP suffixes become authoritative only after backwards balance inversion proves a zero opening balance; reaching account genesis proves that no older transactions exist but does not waive that zero-balance reconciliation. A Ledger-first chain-length/Index-status barrier prevents stale ICP history from selecting an older funder. For credits from its intrinsic splitters 10–90, Relay applies the same balance proof while reconstructing original AccountIdentifiers from exact anchored ICP Index history and weights only the actual net SubaccountOne leg.
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

Protocol-native status visibility is the preferred observability mechanism: `public` exposes the management canister's status response to every caller, while `allowed_viewers` can authorize selected ordinary samplers. Recognized blackhole and SNS status routes remain compatibility fallbacks. The blackhole pattern still supports immutable or self-managed controller postures for other protocol canisters. Self-service Relays instead become immutable with zero controllers while retaining public logs and public status. Recovery behavior is intentionally component-specific; see the relevant canister README for exact rescue windows and controller transitions.
