# NNS Governance Bindgen Architecture

`jupiter-nns-types` uses a checked-in generated Rust file for Jupiter's NNS
Governance wire DTOs. `jupiter-ic-clients` uses a second checked-in generated
Rust file for the raw NNS Governance transport surface. Both files are
regenerated and verified from a pinned Candid subset by `nns-bindgen-check`.

## Pinned Input

The source DID is:

- `candid/nns-governance/governance.subset.did`
- `dfinity/ic` commit `0c7c8b83144844e1a598633585b3ee1beebe338b`
- upstream path `rs/nns/governance/canister/governance.did`
- copy date `2026-06-01`

The subset covers Jupiter-used NNS Governance calls:

- `list_neurons`
- `manage_neuron`
- `simulate_manage_neuron`
- `get_full_neuron`
- `get_full_neuron_by_id_or_subaccount`

## Production Generation Path

Production builds do not run bindgen. `crates/nns-types/src/lib.rs` includes
the committed generated DTO file:

- `crates/nns-types/src/generated/nns_governance_types.rs`

`crates/ic-clients/src/generated/mod.rs` includes the committed generated raw
transport file:

- `crates/ic-clients/src/generated/nns_governance_transport.rs`

Those files are generated from the pinned DID and reviewed in git. The
verification command is:

```bash
cargo run -p nns-bindgen-check -- --check
```

To refresh the committed generated files after an intentional DID/config update,
run:

```bash
cargo run -p nns-bindgen-check -- --update
```

Then review the generated diff.

The checker uses `candid_parser = 0.2.4` directly. It writes
`emit_bindgen(...).type_defs` plus an audit header for DTOs, and renders
`emit_bindgen(...).methods` through a Jupiter-owned raw transport template for
the low-level NNS Governance call surface. `ic-cdk-bindgen` is not used directly
for either committed output.

This is structured bindgen output, not marker extraction from a generated source
file. It keeps generated NNS wire DTOs and method-name transport auditable while
avoiding unused generated call stubs.

## Current Architecture

`jupiter-nns-types` remains a DTO-only crate. Its normal dependencies stay
limited to `candid` and `serde`; the dev-only verifier owns the `candid_parser`
dependency. The crate does not depend on `ic-cdk`, does not expose generated call
stubs, and does not make generated transport code part of its runtime behavior.

`jupiter-ic-clients` owns the generated raw NNS Governance transport because it
already owns shared inter-canister client code and depends on `ic-cdk`. The
generated transport accepts dynamic `Principal` callees, supports
`GovernanceCallWait` for bounded default, bounded explicit-timeout, and
unbounded waits, and returns raw `ic_cdk::call::Response` values.

The generator still validates that each generated runtime method has the pinned
Candid return arity. That check is intentional: generated raw transport verifies
the method shape, while Jupiter-owned adapters keep response decoding and error
classification.

Governance clients remain hand-owned traits and adapters in the calling crates.
Those traits and adapters own timeout selection, response decoding, error
classification, retries, deterministic mocks, and scheduler test boundaries.
Tests and mocks construct DTOs directly instead of mocking the generated
low-level transport surface.

Only production-used runtime methods are generated:

- `get_full_neuron`
- `list_neurons`
- `manage_neuron`

The pinned Candid subset may include additional methods for DTO compatibility
and future verification, but unused runtime transport stubs are not committed.

Architecture validation includes `nns-bindgen-check`, dependency inverse
checks, workspace checks, full xtask validation, canister builds, and the
security scan.

## Public Rust Shape

Generated bindgen names are flat. Examples:

- `Command`
- `Operation`
- `By`
- `Result2`

`jupiter-nns-types` directly re-exports the generated structs and enums. It also
keeps a few compatibility modules, such as `manage_neuron`,
`manage_neuron_response`, `list_neurons`, and `neuron`, as type aliases to the
generated types. These modules are not parallel wire DTO definitions; they are
import-path compatibility aliases.

The only hand-maintained items in `jupiter-nns-types` are:

- type aliases such as `PrincipalId` and `NeuronResult`
- compatibility modules that alias generated types
- `Default` impls for generated structs used by existing tests and mocks
- `governance_error::ErrorType`, a local numeric convenience enum for
  constructing error fixtures

No hand-written NNS Governance request or response wire structs remain.

## Known Generated Layout Choices

`ListNeuronsResponse` follows the upstream shape, including the required
`neuron_infos : vec record { nat64; NeuronInfo }` field. Jupiter runtime code
currently ignores `neuron_infos`, and mocks populate it through
`Default::default()` when no entries are needed.

Generated map-like Candid records use vectors of tuples, for example
`Neuron.followees: Vec<(i32, Followees)>`, matching the Candid wire shape.

Generated empty record variants use struct-variant syntax, for example:

```rust
By::NeuronIdOrSubaccount {}
Command::Configure {}
```

## Dependency Boundary

`jupiter-nns-types` has no build script and no bindgen dependency. Its
production normal dependency surface remains `candid` plus `serde`.

The dev-only `nns-bindgen-check` tool pins `candid_parser = "=0.2.4"` to verify
that the committed generated DTO and raw transport files remain in sync with the
pinned DID and type selector config.

This architecture must not reintroduce broad DFINITY NNS dependencies such as:

- `ic-base-types`
- `ic-nns-common`
- `ic-nns-governance-api`
- `dfinity/ic` git crates
- `rsa`
- `bincode`
- `proc-macro-error`
- `derivative`

Validate this with the dependency-tree commands documented in the task or
release checklist before considering dependency-sensitive work complete.

Public canister `.did` files, debug `.did` files, `mainnet-install-args.did`,
`dfx.json`, and `canister_ids.json` are not part of this generated transport
path and should remain unchanged unless a separate public interface or
deployment-wiring change explicitly requires it.
