# ic-blackhole vendoring note

This repository is the placeholder location for a verbatim vendored copy of the upstream `ninegua/ic-blackhole` source.

The intent is to keep the upstream repository unchanged and to use its own documented reproducible-build flow when validating the published `0.0.0` artifact hash and canonical canister ID.

In this workspace snapshot the actual upstream source tree was not copied into place yet, so historian testing currently uses the local `mock-blackhole` canister declared under `xtask/src/mocks/mock_blackhole/`.

When the upstream source is copied in, keep it unmodified and record:

- upstream repository URL
- upstream release/tag
- expected optimized wasm hash
- expected production canister ID
