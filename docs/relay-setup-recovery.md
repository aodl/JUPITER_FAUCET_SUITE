# Relay Setup Recovery Runbook

This runbook covers self-service relay setup jobs that reach `ManualRecoveryRequired`. Self-service relays use the canonical Relay daily cadence; their configuration differs from the canonical production Relay only by target canister and surplus recipient settings.

`ManualRecoveryRequired` means the historian saw an operation whose outcome can no longer be recovered safely by automatic retry. Operators must inspect public ledger/index records and canister state before deciding whether to refund, register a relay, or leave the job blocked.

## Inspection Entry Points

Use `get_relay_setup_view` for the user-facing status and setup account. Use `get_relay_setup_recovery_view` for recovery details:

- target canister ID
- setup account identifier
- setup amount seen and processed
- last error
- relay canister ID, if known
- cycle conversion amount and minted cycles, if CMC conversion has happened
- redacted transfer records for CMC conversion, relay funding, existing-relay sweep, and refund count
- create-canister intent and attached cycles, if one was persisted before a possibly ambiguous create

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

The nominal self-service setup policy minimum is now 3 ICP. The canister does not rely only on that static policy floor. Before any CMC transfer, it fetches the current CMC ICP/XDR conversion rate and computes the current required setup amount from:

- configured `create_canister` attachment cycles,
- relay subaccount-1 seed requirement,
- relay cycle safety margin held in ICP,
- required ICP ledger fees.

If current CMC/XDR conditions mean the setup account cannot mint the configured create attachment while preserving the seed, safety margin, and fees, setup fails before spending ICP. The user-facing view should display the actionable required amount, which is at least the 3 ICP nominal minimum and can be higher under adverse conversion conditions.

For a stale CMC conversion transfer, compare the recovery view's `cycle_transfer` with the ICP index:

- `to_account_identifier` should be the CMC deposit account for the historian canister.
- the memo should be the top-up memo recorded by protocol code, when visible in index tooling.
- if the transfer block exists but CMC notify is unresolved, inspect whether cycles were minted to the historian.
- if the transfer is not indexed and is older than the duplicate window, keep the job in manual recovery until ledger/index evidence is clear.

## Relay Canister Creation

For create-canister ambiguity, the historian persists a create intent before calling `create_canister`. If the management-canister reply is lost with no relay ID recorded, the recovery view shows the intent but `relay_canister_id` remains empty.

This is the explicit create_canister ambiguous relay ID loss case. The created canister ID may be unrecoverable from historian state, so automatic retry must not create another relay. Do not mark the job solved unless operators independently identify the created relay canister or decide to refund/replace through governance-reviewed operations.

`create_canister` consumes a creation fee from the attached cycles. Remaining attached cycles become the new relay canister's starting balance. A deterministic management-canister rejection saying the attached cycles are insufficient for `create_canister` is an operator-recovery condition, not a user-retry condition. Do not advise users to repeatedly call `notify_relay_setup` against the same bad config.

## Install Code

If `relay_canister_id` is known and `code_installed` was not recorded, automatic retry first checks the relay canister module hash. The compressed Relay install payload hash is the on-chain reconciliation hash because Historian passes `release-artifacts/jupiter_relay.wasm.gz` bytes to `install_code`. If `canister_status.module_hash` matches that install payload hash, the historian marks code installed and resumes relay funding. This install_code module-hash reconciliation prevents a second `install_code` call in `Install` mode after a lost reply.

The canonical historian build embeds `release-artifacts/jupiter_relay.wasm.gz` corresponding to the reviewed raw Relay Wasm from `./tools/scripts/docker-build`. Release artifacts are generated review evidence and are not checked into source control, so their absence from a source archive is not a source-level failure. `install_code` receives those compressed bytes directly; the IC accepts gzip-compressed Wasm modules and installs the decompressed module. Operators should keep these hashes distinct:

- reviewed reproducible raw relay wasm hash: `sha256sum release-artifacts/jupiter_relay.wasm`
- compressed Relay install payload hash: `sha256sum release-artifacts/jupiter_relay.wasm.gz`
- installed module hash: `canister_status.module_hash`, compared against the compressed Relay install payload hash

The reviewed raw relay wasm hash is reviewer verification evidence and must come from the Docker/reproducible release artifact, not an arbitrary local build. The gzip payload must decompress to that reviewed raw Wasm. The compressed relay wasm hash is the install payload hash and the runtime module-hash reconciliation value. Release notes must also record the `release-artifacts/jupiter_historian.wasm.gz` hash, which is the production Historian install package hash.

If the module hash is missing, automatic setup may retry install while the historian is still controller.

If the module hash exists but differs from the reviewed compressed Relay install payload hash, the job enters `ManualRecoveryRequired`. Operators must inspect the relay canister before any governance action.

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

## Existing Failed Target 2lo52-kiaaa-aaaar-qaqta-cai

The live failed job for `2lo52-kiaaa-aaaar-qaqta-cai` recorded:

- CMC conversion completed for `94_950_000` e8s at block `37_414_364`.
- `relay_canister_id = null`, so canister creation did not succeed or at least was not recorded.
- `relay_create_attempt.initial_cycles = 1_000_000_000_000`.
- The management canister rejected `create_canister` because it required `1_307_692_307_692` cycles while only `1_000_000_000_000` cycles were attached.

After this patch, automatic retry is safe only after the upgraded Historian config is live and the recovery view confirms `cycles_minted` is at least the new configured create attachment. If the existing CMC conversion minted only `1_000_000_000_000` cycles, retrying cannot safely call `create_canister` with the new 2T-cycle attachment without subsidizing from Historian cycles; the job should remain `ManualRecoveryRequired`.

Recovery recommendation for that target:

1. Query `get_relay_setup_recovery_view` and confirm `cycles_minted`, `cycle_conversion_e8s`, and all transfer records.
2. If `cycles_minted < 2_000_000_000_000` and no relay ID exists, do not call `notify_relay_setup` again expecting automatic completion.
3. Reconcile the setup account balance and the cycles minted to Historian. Because ICP was already spent on CMC conversion, an additional user payment may be required only if operators choose to complete the relay through a reviewed manual recovery path; otherwise operator refund/reconciliation is required because the converted ICP cannot be automatically returned from the setup account.
4. If operators can prove enough cycles are available and decide to complete manually, create/install/fund only with the reviewed reproducible Relay artifact and record the action in release evidence.

## Public Notify Monitoring

`notify_relay_setup` is public and can consume historian cycles through ledger/index calls even when a caller has not funded a valid setup account. After enablement, monitor Historian cycle balance and call volume. The deployment accepts this bounded operational risk for now rather than adding stable-state rate-limit data; revisit if public no-fund notify traffic becomes material.

## Factory-Enabled Production Deploys

Mainnet install args enable `relay_factory_enabled = opt true`.

Factory-enabled production Historian deploys must:

1. Build the canonical artifacts with `./tools/scripts/docker-build`.
2. Verify `release-artifacts/jupiter_historian.reviewed-relay-wasm-raw.sha256`.
3. Verify `release-artifacts/jupiter_historian.embedded-relay-wasm-gz.sha256`.
4. Install the reviewed canonical historian artifact in a non-mainnet test environment with `relay_factory_enabled=true`.
5. Confirm `get_relay_setup_view.relay_raw_wasm_hash_hex` equals the recorded reviewed raw Relay Wasm hash and `get_relay_setup_view.relay_install_payload_hash_hex` equals the compressed Relay install payload hash.
6. Include the raw relay wasm hash, compressed relay install payload hash, canonical historian artifact hash, and validator output in the final pre-deploy report.
7. Use `release-artifacts/jupiter_historian.wasm.gz` for the production deploy command.

For live enablement on an already-installed Historian, use a temporary `Option<UpgradeArgs>` file containing only:

```did
(opt record {
  relay_factory_enabled = opt true;
  cycles_probe_policy = opt variant { Auto };
})
```

The explicit Auto policy is required for an existing Historian upgrade. Omitting `cycles_probe_policy` intentionally preserves the legacy fixed-proxy policy already stored by the canister.

Do not pass `canisters/historian/mainnet-install-args.did` to an already-installed Historian upgrade.
