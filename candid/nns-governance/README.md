# NNS Governance Candid Evidence

This directory pins the NNS Governance Candid wire shape used by
`jupiter-nns-types` to generate Jupiter's committed NNS Governance wire DTOs.
Run `cargo run -p nns-bindgen-check -- --check` to verify the committed DTO file
is in sync with this DID and `nns-governance-bindgen.toml`.

Upstream source:

- Repository: <https://github.com/dfinity/ic>
- File: `rs/nns/governance/canister/governance.did`
- Commit: `0c7c8b83144844e1a598633585b3ee1beebe338b`
- Raw URL: <https://raw.githubusercontent.com/dfinity/ic/0c7c8b83144844e1a598633585b3ee1beebe338b/rs/nns/governance/canister/governance.did>
- Date copied: 2026-06-01

`governance.subset.did` is a documented subset, not a full copy of the
upstream DID. It contains the Governance methods and DTOs that Jupiter canisters
or test harnesses use:

- `list_neurons`
- `manage_neuron`
- `simulate_manage_neuron`
- `get_full_neuron`
- `get_full_neuron_by_id_or_subaccount`

The subset intentionally excludes unrelated NNS proposal, economics, node
provider, SNS, and canister-management DTOs. It keeps generated output
reviewable while preserving pinned Candid evidence for the wire types used by
Jupiter's faucet, disburser, relay, historian, mocks, and PocketIC tests.

Do not update this file from an unpinned `master` URL. When refreshing the
subset, record the exact upstream commit and review the generated
`jupiter-nns-types` API changes before updating downstream call sites.
