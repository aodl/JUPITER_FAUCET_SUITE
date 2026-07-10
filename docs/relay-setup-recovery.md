# Relay Setup Recovery Runbook

This runbook covers self-service relay setup jobs that reach `ManualRecoveryRequired`.

`ManualRecoveryRequired` means the historian saw an operation whose outcome can no longer be recovered safely by automatic retry. Operators must inspect public ledger/index records and canister state before deciding whether to refund, register a relay, or leave the job blocked.

## Inspection Entry Points

Use `get_relay_setup_view` for the user-facing status and setup account. Use `get_relay_setup_recovery_view` for recovery details:

- target canister ID
- setup account identifier
- setup amount seen and processed
- last error
- relay canister ID, if known
- redacted transfer records for CMC conversion, relay funding, existing-relay sweep, and refund count
- create-canister intent, if one was persisted before a possibly ambiguous create

The recovery query is read-only. It does not redirect funds, register relays, or retry setup work.

## Setup Account Balance

Identify the target setup account from `setup_account_identifier` in the recovery view. Inspect the ICP ledger/index for:

- current balance
- inbound setup payments
- outbound CMC conversion transfer
- outbound relay funding transfer
- outbound existing-relay sweep transfer
- outbound refund transfers

If the ICP index has not caught up, wait for the indexed transactions to explain the ledger balance before taking manual action.

## CMC Conversion

For a stale CMC conversion transfer, compare the recovery view's `cycle_transfer` with the ICP index:

- `to_account_identifier` should be the CMC deposit account for the historian canister.
- the memo should be the top-up memo recorded by protocol code, when visible in index tooling.
- if the transfer block exists but CMC notify is unresolved, inspect whether cycles were minted to the historian.
- if the transfer is not indexed and is older than the duplicate window, keep the job in manual recovery until ledger/index evidence is clear.

## Relay Canister Creation

For create-canister ambiguity, the historian persists a create intent before calling `create_canister`. If the management-canister reply is lost with no relay ID recorded, the recovery view shows the intent but `relay_canister_id` remains empty.

This is the explicit create_canister ambiguous relay ID loss case. The created canister ID may be unrecoverable from historian state, so automatic retry must not create another relay. Do not mark the job solved unless operators independently identify the created relay canister or decide to refund/replace through governance-reviewed operations.

## Install Code

If `relay_canister_id` is known and `code_installed` was not recorded, automatic retry first checks the relay canister module hash. The reviewed raw relay wasm hash is the review and reconciliation hash. If `canister_status.module_hash` matches that raw installed module hash, the historian marks code installed and resumes relay funding. This install_code module-hash reconciliation prevents a second `install_code` call in `Install` mode after a lost reply.

The historian-with-relay build embeds `release-artifacts/jupiter_relay.wasm.gz` to keep the historian artifact smaller. `install_code` receives those compressed bytes directly; the IC accepts gzip-compressed Wasm modules and installs the decompressed module. Operators should keep these hashes distinct:

- reviewed raw relay wasm hash: `sha256sum release-artifacts/jupiter_relay.wasm`
- compressed embedded relay wasm.gz hash: `sha256sum release-artifacts/jupiter_relay.wasm.gz`
- installed module hash: `canister_status.module_hash`, compared against the reviewed raw relay wasm hash

The reviewed raw relay wasm hash is the reviewer verification and module-hash reconciliation value. The compressed relay wasm is embedded only to reduce the Historian artifact size. Release notes must also record the `release-artifacts/jupiter_historian_with_relay.wasm.gz` hash, which is the production Historian install package hash.

If the module hash is missing, automatic setup may retry install while the historian is still controller.

If the module hash exists but differs from the reviewed raw relay wasm hash, the job enters `ManualRecoveryRequired`. Operators must inspect the relay canister before any governance action.

## Relay Funding

For a stale relay funding transfer, compare `relay_funding_transfer` against the ICP index:

- if the transfer is indexed, confirm the relay subaccount-1 balance.
- if it is not indexed and the duplicate window has expired, do not retry blindly.
- if the relay is funded but not active, inspect blackhole/controller status before registering.

## Blackhole Update

Final blackhole control is complete only when the relay canister controllers are the configured blackhole canister and logs are public as expected. If `blackhole_update_attempted` was recorded but activation did not finish, inspect canister status through the blackhole or management tooling available to the current controller.

Register or supersede a relay manually only after confirming the relay canister runs the reviewed wasm, targets the intended canister, has expected funding, and is controlled by the blackhole.

## Refunds

For a refund transfer possibly succeeded case, compare `refund_transfer_count`, `refund_blocks`, and index records for each source account. A stale refund transfer may have succeeded even if the historian did not record the block. Do not issue a second manual refund until the index proves the original transfer did not happen.

Refund manually when:

- setup funds remain in the setup account,
- no CMC conversion or relay funding consumed the funds,
- indexed inbound payments identify refund destinations,
- and no relay has been created or funded for the target.

Tell users that setup is blocked for manual reconciliation, the setup account and public transaction evidence are being checked, and no new payment should be sent for the same target until operators resolve the job.

## Public Notify Monitoring

`notify_relay_setup` is public and can consume historian cycles through ledger/index calls even when a caller has not funded a valid setup account. After enablement, monitor Historian cycle balance and call volume. The deployment accepts this bounded operational risk for now rather than adding stable-state rate-limit data; revisit if public no-fund notify traffic becomes material.

## Factory-Enabled Production Deploys

Mainnet install args enable `relay_factory_enabled = opt true`.

Factory-enabled production Historian deploys must:

1. Build the artifact with `./tools/scripts/build-canister jupiter-historian-with-relay`.
2. Verify `release-artifacts/jupiter_historian_with_relay.reviewed-relay-wasm-raw.sha256`.
3. Verify `release-artifacts/jupiter_historian_with_relay.embedded-relay-wasm-gz.sha256`.
4. Install the reviewed historian-with-relay artifact in a non-mainnet test environment with `relay_factory_enabled=true`.
5. Confirm `get_relay_setup_view.relay_wasm_hash_hex` equals the recorded reviewed raw relay wasm hash.
6. Include the raw relay wasm hash, gzip relay wasm hash, historian-with-relay artifact hash, and validator output in the final pre-deploy report.
7. Use `release-artifacts/jupiter_historian_with_relay.wasm.gz` for the production deploy command.

For live enablement on an already-installed Historian, use a temporary `Option<UpgradeArgs>` file containing only:

```did
(opt record {
  relay_factory_enabled = opt true;
})
```

Do not pass `canisters/historian/mainnet-install-args.did` to an already-installed Historian upgrade.
