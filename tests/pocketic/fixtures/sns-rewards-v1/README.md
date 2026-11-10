# SNS Rewards V1 upgrade fixture

`canonical.wasm.gz` is the retained canonical Docker package from source
`747613f84122a2062f447ed24fb2764ff4f3106f`. SNS Rewards source and manifest
are unchanged between the common predecessor `653f1bcd3b659173840c4313b76185f5fec3b5f9`
and that source. It exposes the previous two-method Relay owner-index interface.
This is compatibility evidence for the previous production implementation,
not a claim about the currently installed mainnet module.

SHA-256 of the exact compressed install package:
`dc60d0c80214f37f06ad42f6785cb2adc123ce1615669a9bace689538e253079`.
Uncompressed module:
`89691e35df7adfb5f8746b7a724ed3277a5d3c85ce05054cf7d3be74dbe0f062`.

Before copying, the retained checksum manifest and embedded Candid were checked.
Every build-info input matched source `747613f`: Cargo.lock, package-lock.json,
package.json, rust-toolchain.toml, tools/scripts/build-canister and the production
ID mapping. Preserve these frozen bytes; do not rebuild them from the candidate.
The integration test must check the fixture hash before installation, then verify
root/snapshot/owner preservation and inactive unconfigured automation after an
upgrade to the candidate production build.
