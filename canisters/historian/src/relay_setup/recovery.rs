//! User-funded continuation of the existing paid setup journal. Reads never
//! dispatch value. A conversion belongs to this key and one recorded destination.
use super::*;

const MAX_CONVERSIONS: usize = 4;
const RETRY_BACKOFF_NANOS: u64 = 30 * 1_000_000_000;
// ICP Ledger's 24-hour transaction window, with a one-hour conservative buffer.
const SAFE_TRANSFER_AGE_NANOS: u64 = 23 * 60 * 60 * 1_000_000_000;
const MIN_SUPPORTED_CREATION_COST: u128 = 500_000_000_000;
const MAX_SUPPORTED_CREATION_COST: u128 = 1_307_692_307_692;

fn creation_cost() -> u128 {
    #[cfg(test)]
    {
        500_000_000_000
    }
    #[cfg(not(test))]
    {
        ic_cdk::api::cost_create_canister()
    }
}

// Retain 1T operating cycles plus twice the subnet-scaled creation charge for
// installation, freezing and initialization. See the
// cost/reservation derivation and supported settings in relay-setup-recovery.md.
pub(super) fn creation_budget(config: &Config) -> Result<u128, String> {
    creation_budget_at_cost(config, creation_cost())
}

fn creation_budget_at_cost(config: &Config, cost: u128) -> Result<u128, String> {
    if !(MIN_SUPPORTED_CREATION_COST..=MAX_SUPPORTED_CREATION_COST).contains(&cost) {
        return Err("Relay provisioning is not supported at this subnet creation cost".into());
    }
    let floor = cost
        .checked_mul(3)
        .and_then(|n| n.checked_add(1_000_000_000_000))
        .ok_or("Relay provisioning budget overflow")?;
    Ok(config.relay_initial_cycles.max(floor))
}

pub(super) fn child_budget(config: &Config) -> Result<u128, String> {
    creation_budget(config)?
        .checked_sub(creation_cost())
        .ok_or("invalid child budget".into())
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelayInstallationIdentity {
    pub module_hash: Vec<u8>,
    pub init_arg: Vec<u8>,
    pub ledger_id: Principal,
    pub cmc_id: Principal,
    pub final_reserve_e8s: u64,
    pub margin_e8s: u64,
    pub original_create_cycles: u128,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecoveryConversion {
    pub transfer: RelayTransferRecord,
    pub destination: Principal,
    pub for_creation: bool,
    pub to: Account,
    pub memo: Vec<u8>,
    pub notifications: u8,
    pub cycles_minted: Option<u128>,
    pub rejected: bool,
    pub dispatches: u8,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelayContinuation {
    pub identity: RelayInstallationIdentity,
    pub conversions: Vec<RecoveryConversion>,
    pub install_retried: bool,
    pub funding_pause: bool,
    pub terminal: bool,
    pub attempt: u64,
    // Optional so already-persisted continuation records remain readable.
    pub retry_after_nanos: Option<u64>,
}

impl RelayContinuation {
    pub(super) fn new(config: &Config, setup: &CanonicalRelaySetup) -> Self {
        Self {
            identity: RelayInstallationIdentity {
                module_hash: approved_relay_onchain_module_hash()
                    .expect("approved Relay hash")
                    .to_vec(),
                init_arg: relay_init_arg(config, setup),
                ledger_id: config.ledger_canister_id,
                cmc_id: config.cmc_canister_id.expect("configured CMC"),
                final_reserve_e8s: minimum_final_relay_funding_e8s(config, setup.target_count())
                    .expect("validated reserve"),
                margin_e8s: config.relay_cycle_safety_margin_e8s,
                original_create_cycles: creation_budget(config)
                    .expect("validated provisioning budget"),
            },
            conversions: Vec::new(),
            install_retried: false,
            funding_pause: false,
            terminal: false,
            attempt: 0,
            retry_after_nanos: None,
        }
    }
}

pub(super) fn pause(
    key: RelaySetupKey,
    phase: RelayCreationPhase,
    message: impl Into<String>,
) -> RelaySetupNotifyResult {
    match require_phase(key, phase) {
        Ok(mut progress) => {
            progress.last_error = Some(bounded_message(message.into()));
            if let Some(continuation) = progress.continuation.as_mut() {
                continuation.funding_pause = true;
            }
            let result = notify_for_entry(RelaySetupEntry::Paused(progress.clone()));
            insert_entry(key, RelaySetupEntry::Paused(progress));
            result
        }
        Err(result) => result,
    }
}

// A safe retry is delayed, never made financially unsafe by a retry counter.
pub(super) fn pause_retry(
    key: RelaySetupKey,
    phase: RelayCreationPhase,
    message: impl Into<String>,
) -> RelaySetupNotifyResult {
    if let Err(result) = update_progress(key, phase, |progress| {
        if let Some(continuation) = progress.continuation.as_mut() {
            continuation.retry_after_nanos = Some(now_nanos().saturating_add(RETRY_BACKOFF_NANOS));
        }
    }) {
        return result;
    }
    pause(key, phase, message)
}

fn progress_for_recovery(key: RelaySetupKey) -> Option<RelayCreationProgress> {
    match get_entry(key)? {
        RelaySetupEntry::Paused(p) | RelaySetupEntry::ManualRecoveryRequired(p) => Some(p),
        _ => None,
    }
}

fn paid_evidence(progress: &RelayCreationProgress) -> Result<(), String> {
    if progress
        .cmc_transfer
        .as_ref()
        .and_then(|t| t.block_index)
        .is_none()
        || progress.cycles_minted.is_none()
    {
        return Err("Missing original accepted conversion evidence".into());
    }
    match progress.phase {
        RelayCreationPhase::CmcTransferAccepted | RelayCreationPhase::CmcNotifySucceeded
            if progress.create_dispatched_at_ts.is_none()
                && progress.relay_canister_id.is_none()
                && progress.relay_funding_transfer.is_none() =>
        {
            Ok(())
        }
        RelayCreationPhase::ChildCreated | RelayCreationPhase::CodeInstalled
            if progress.create_dispatched_at_ts.is_some()
                && progress.relay_canister_id.is_some()
                && progress.relay_funding_transfer.is_none() =>
        {
            Ok(())
        }
        RelayCreationPhase::RelayFunded | RelayCreationPhase::FinalizationAttempted
            if progress.create_dispatched_at_ts.is_some()
                && progress.relay_canister_id.is_some()
                && progress
                    .relay_funding_transfer
                    .as_ref()
                    .and_then(|t| t.block_index)
                    .is_some() =>
        {
            Ok(())
        }
        _ => Err(
            "This setup has an unknown child identity or an unreconciled spending operation".into(),
        ),
    }
}

pub(super) struct ChildStatus {
    pub status: CanisterStatusResult,
    pub budget_settings_supported: bool,
}

pub(super) async fn live_child_status(canister_id: Principal) -> Result<ChildStatus, String> {
    #[derive(CandidType, Deserialize)]
    struct Settings {
        controllers: Vec<Principal>,
        log_visibility: LogVisibility,
        status_visibility: StatusVisibility,
        compute_allocation: candid::Nat,
        memory_allocation: candid::Nat,
        freezing_threshold: candid::Nat,
    }
    #[derive(CandidType, Deserialize)]
    struct Status {
        status: CanisterStatusKind,
        cycles: candid::Nat,
        module_hash: Option<Vec<u8>>,
        settings: Settings,
        memory_size: candid::Nat,
    }
    let status: Status =
        ic_cdk::call::Call::bounded_wait(Principal::management_canister(), "canister_status")
            .with_arg(&jupiter_ic_clients::management::CanisterStatusArgs { canister_id })
            .change_timeout(60)
            .await
            .map_err(|e| format!("{e:?}"))?
            .candid()
            .map_err(|e| format!("{e:?}"))?;
    let supported = status.settings.compute_allocation == 0u8
        && status.settings.memory_allocation == 0u8
        && status.settings.freezing_threshold <= 2_592_000u64
        && status.memory_size <= 256 * 1024 * 1024u64;
    Ok(ChildStatus {
        budget_settings_supported: supported,
        status: CanisterStatusResult {
            status: status.status,
            cycles: status.cycles,
            module_hash: status.module_hash,
            settings: jupiter_ic_clients::management::DefiniteCanisterSettings {
                controllers: status.settings.controllers,
                log_visibility: status.settings.log_visibility,
                status_visibility: status.settings.status_visibility,
            },
        },
    })
}

struct Plan {
    progress: RelayCreationProgress,
    continuation: RelayContinuation,
    quote: RelayRecoveryQuote,
    installed: bool,
}

enum CheckError {
    Unsafe(String),
    Pending(String),
}

impl From<String> for CheckError {
    fn from(message: String) -> Self {
        Self::Unsafe(message)
    }
}

fn pending(message: impl Into<String>) -> CheckError {
    CheckError::Pending(message.into())
}

async fn inspect(
    key: RelaySetupKey,
    config: &Config,
    historian: Principal,
    ledger: &dyn LedgerClient,
    cmc: &dyn CmcClient,
    management: &dyn ManagementClient,
    ticket: admission::Ticket,
) -> Result<Plan, CheckError> {
    let progress =
        progress_for_recovery(key).ok_or_else(|| pending("Setup state changed; check again"))?;
    paid_evidence(&progress)?;
    let continuation = progress
        .continuation
        .clone()
        .ok_or_else(|| "The paid setup has no pinned installation identity".to_string())?;
    if continuation.identity.ledger_id != config.ledger_canister_id
        || Some(continuation.identity.cmc_id) != config.cmc_canister_id
        || continuation.identity.module_hash.len() != 32
        || (progress.relay_canister_id.is_none()
            && relay_install_payload_for_hash(&continuation.identity.module_hash).is_none())
    {
        return Err(CheckError::Unsafe(
            "The recorded Ledger, CMC or Relay payload differs from this release".into(),
        ));
    }
    if progress.relay_canister_id.is_some()
        && creation_credit(&progress)
            .is_none_or(|credit| credit < continuation.identity.original_create_cycles)
    {
        return Err(
            "Contradictory original creation funding evidence; do not pay"
                .to_string()
                .into(),
        );
    }
    let mut installed = false;
    let deficit = if let Some(child) = progress.relay_canister_id {
        let status = ticket
            .call(management.recovery_canister_status(child))
            .await
            .map_err(|_| pending("Recovery check is busy; check again"))?
            .map_err(|error| {
                pending(format!("Child status is unavailable; do not pay: {error}"))
            })?;
        if !status.budget_settings_supported {
            return Err(
                "Child settings exceed the supported provisioning budget; do not pay"
                    .to_string()
                    .into(),
            );
        }
        let status = status.status;
        if status.module_hash.as_deref() == Some(continuation.identity.module_hash.as_slice()) {
            classify_relay_finalization_state(
                &status,
                continuation
                    .identity
                    .module_hash
                    .as_slice()
                    .try_into()
                    .map_err(|_| "invalid pinned hash".to_string())?,
                historian,
            )?;
            if status.settings.controllers.is_empty()
                && !matches!(
                    progress.phase,
                    RelayCreationPhase::RelayFunded | RelayCreationPhase::FinalizationAttempted
                )
            {
                return Err("Controllers were removed before recorded funding"
                    .to_string()
                    .into());
            }
            installed = true;
            0
        } else if status.module_hash.is_none()
            && status.status == CanisterStatusKind::Running
            && status.settings.controllers == vec![historian]
            && progress.phase == RelayCreationPhase::ChildCreated
            && continuation.funding_pause
            && !continuation.terminal
        {
            if relay_install_payload_for_hash(&continuation.identity.module_hash).is_none() {
                return Err(
                    "The pinned Relay installation payload is unavailable; do not pay"
                        .to_string()
                        .into(),
                );
            }
            let cycles =
                u128::try_from(status.cycles.0).map_err(|_| "child cycles overflow".to_string())?;
            child_budget(config)?.saturating_sub(cycles)
        } else {
            return Err(
                "Unexpected child code, controllers or installation outcome; do not pay"
                    .to_string()
                    .into(),
            );
        }
    } else {
        if continuation.terminal || progress.create_dispatched_at_ts.is_some() {
            return Err("The child creation outcome cannot be safely repeated"
                .to_string()
                .into());
        }
        let credited = continuation
            .conversions
            .iter()
            .filter(|op| op.for_creation)
            .filter_map(|c| c.cycles_minted)
            .try_fold(progress.cycles_minted.unwrap_or(0), |n, value| {
                n.checked_add(value)
            })
            .ok_or("minted cycles overflow".to_string())?;
        creation_budget(config)?.saturating_sub(credited)
    };
    if continuation.conversions.len() > MAX_CONVERSIONS {
        return Err("Recovery journal exceeds its bound; do not pay"
            .to_string()
            .into());
    }
    let mut accepted_blocks = std::collections::BTreeSet::new();
    if let Some(block) = progress.cmc_transfer.as_ref().and_then(|op| op.block_index) {
        accepted_blocks.insert(block);
    }
    for (index, op) in continuation.conversions.iter().enumerate() {
        let bad_destination = if op.for_creation {
            op.destination != historian
                || (progress.relay_canister_id.is_some()
                    && op.cycles_minted.is_none()
                    && !op.rejected)
        } else {
            Some(op.destination) != progress.relay_canister_id
        };
        if bad_destination
            || op.to != cmc_deposit_account(continuation.identity.cmc_id, op.destination)
            || op.memo != TOP_UP_CANISTER_MEMO.to_le_bytes()
            || op.transfer.amount_e8s == 0
            || (op.cycles_minted.is_some() && op.transfer.block_index.is_none())
            || (op.rejected && (op.cycles_minted.is_some() || op.transfer.block_index.is_some()))
            || (index + 1 < continuation.conversions.len()
                && !op.rejected
                && op.cycles_minted.is_none())
            || op
                .transfer
                .block_index
                .is_some_and(|block| !accepted_blocks.insert(block))
        {
            return Err(
                "Contradictory recovery operation identity or result; do not pay"
                    .to_string()
                    .into(),
            );
        }
    }
    let outstanding = continuation
        .conversions
        .last()
        .is_some_and(|c| !c.rejected && c.cycles_minted.is_none());
    if outstanding
        && continuation.conversions.last().is_some_and(|c| {
            c.transfer.block_index.is_none()
                && now_nanos().saturating_sub(c.transfer.created_at_time_nanos)
                    > SAFE_TRANSFER_AGE_NANOS
        })
    {
        return Err(
            "Recovery transfer deduplication guarantee expired; do not pay"
                .to_string()
                .into(),
        );
    }
    if continuation.terminal && (!installed || outstanding) {
        return Err("Recovery spending cannot be safely reconciled; do not pay"
            .to_string()
            .into());
    }
    if continuation
        .retry_after_nanos
        .is_some_and(|deadline| now_nanos() < deadline)
    {
        return Err(pending(
            "Recovery retry is temporarily delayed; check again. Do not send a payment",
        ));
    }
    let account = setup_account_for(historian, key);
    let balance = ticket
        .call(ledger.balance_of_e8s(account))
        .await
        .map_err(|_| pending("Recovery check is busy; check again"))?
        .map_err(|e| pending(format!("Setup balance unavailable; do not pay: {e}")))?;
    let mut required = 0u64;
    if !outstanding
        && !matches!(
            progress.phase,
            RelayCreationPhase::RelayFunded | RelayCreationPhase::FinalizationAttempted
        )
    {
        let fee = ticket
            .call(ledger.fee_e8s())
            .await
            .map_err(|_| pending("Recovery check is busy; check again"))?
            .map_err(|e| pending(format!("Ledger fee unavailable; do not pay: {e}")))?;
        required = continuation
            .identity
            .final_reserve_e8s
            .checked_add(fee)
            .ok_or("reserve overflow".to_string())?;
        if deficit > 0 {
            if continuation
                .conversions
                .iter()
                .filter(|op| !op.rejected)
                .count()
                >= MAX_CONVERSIONS
            {
                return Err("Recovery conversion limit reached; do not pay"
                    .to_string()
                    .into());
            }
            let rate = ticket
                .call(cmc.get_icp_xdr_conversion_rate())
                .await
                .map_err(|_| pending("Recovery check is busy; check again"))?
                .map_err(|e| pending(format!("CMC rate unavailable; do not pay: {e}")))?;
            let (_, total) = recovery_conversion_quote(
                deficit,
                &rate,
                continuation.identity.margin_e8s,
                balance,
                continuation.identity.final_reserve_e8s,
                fee,
            )?;
            required = total;
        }
    }
    ticket
        .check()
        .map_err(|_| pending("Recovery check expired; check again"))?;
    if progress_for_recovery(key).as_ref() != Some(&progress) {
        return Err(pending("Setup state changed; check again"));
    }
    Ok(Plan {
        progress,
        continuation,
        installed,
        quote: RelayRecoveryQuote {
            setup_account: account,
            setup_account_identifier: crate::clients::index::account_identifier_text_for_account(
                &account,
            ),
            relay_canister_id: progress_for_recovery(key).and_then(|p| p.relay_canister_id),
            balance_e8s: balance,
            required_e8s: required,
            additional_e8s: required.saturating_sub(balance),
            cycles_deficit: deficit,
        },
    })
}

fn conversion_amount(
    deficit: u128,
    rate: &IcpXdrConversionRate,
    margin: u64,
) -> Result<u64, String> {
    ceil_div(deficit, u128::from(rate.xdr_permyriad_per_icp))
        .and_then(|n| u64::try_from(n).ok())
        .and_then(|n| n.checked_add(margin))
        .ok_or("Invalid CMC rate or conversion overflow".into())
}

// Prefer the configured exchange margin. Existing funds that already buy the
// full conservative child budget can use the available part of that margin;
// tiny idle-cycle drift after a quote must not demand another dust payment.
fn recovery_conversion_quote(
    deficit: u128,
    rate: &IcpXdrConversionRate,
    margin: u64,
    balance: u64,
    reserve: u64,
    fee: u64,
) -> Result<(u64, u64), String> {
    let fixed = fee
        .checked_mul(2)
        .and_then(|n| n.checked_add(reserve))
        .ok_or("recovery fee overflow")?;
    let bare = conversion_amount(deficit, rate, 0)?;
    let preferred = bare.checked_add(margin).ok_or("recovery margin overflow")?;
    let available = balance.saturating_sub(fixed);
    let amount = if available >= bare {
        preferred.min(available)
    } else {
        preferred
    };
    Ok((
        amount,
        fixed.checked_add(amount).ok_or("recovery quote overflow")?,
    ))
}

pub(crate) async fn view(args: RelaySetupArgs) -> RelaySetupViewResult {
    let mut view = match setup_view(args.clone()) {
        RelaySetupViewResult::Ok(v) => v,
        error => return error,
    };
    let setup = match CanonicalRelaySetup::canonicalize(
        args.target_canister_ids,
        args.surplus_recipients,
    ) {
        Ok(s) => s,
        Err(e) => return RelaySetupViewResult::Err(e),
    };
    let key = setup.key();
    if progress_for_recovery(key).is_none() {
        return RelaySetupViewResult::Ok(view);
    }
    let Some(guard) = admission::Guard::acquire(key, admission::Lane::Assessment) else {
        view.recovery = Some(RelayRecoveryView::CheckPending(
            "Recovery check is busy; check again".into(),
        ));
        return RelaySetupViewResult::Ok(view);
    };
    let config = state::with_state(|s| s.config.clone());
    let ledger = jupiter_ic_clients::ledger::IcrcLedgerCanister::new(config.ledger_canister_id);
    let cmc = CmcCanister::new(
        config
            .cmc_canister_id
            .unwrap_or_else(Principal::management_canister),
    );
    view.recovery = Some(
        match inspect(
            key,
            &config,
            self_canister_id(),
            &ledger,
            &cmc,
            &IcManagementClient,
            guard.ticket(),
        )
        .await
        {
            Ok(plan) if plan.quote.additional_e8s > 0 => {
                RelayRecoveryView::FundingRequired(plan.quote)
            }
            Ok(plan) => RelayRecoveryView::Ready(plan.quote),
            Err(CheckError::Pending(e)) => RelayRecoveryView::CheckPending(e),
            Err(CheckError::Unsafe(e)) => RelayRecoveryView::Unsafe(e),
        },
    );
    RelaySetupViewResult::Ok(view)
}

pub(super) async fn continue_setup(
    key: RelaySetupKey,
    setup: &CanonicalRelaySetup,
    config: &Config,
    historian: Principal,
    ledger: &dyn LedgerClient,
    cmc: &dyn CmcClient,
    management: &dyn ManagementClient,
) -> RelaySetupNotifyResult {
    // Cheap refusals precede admission/outcalls, including ambiguous old spending.
    if let Some(progress) = progress_for_recovery(key) {
        if paid_evidence(&progress).is_err() || progress.continuation.is_none() {
            return notify_for_entry(get_entry(key).expect("existing entry"));
        }
    }
    let Some(guard) = admission::Guard::acquire(key, admission::Lane::Completion) else {
        return RelaySetupNotifyResult::Busy;
    };
    let ticket = guard.ticket();
    for _ in 0..=MAX_CONVERSIONS {
        let plan = match inspect(key, config, historian, ledger, cmc, management, ticket).await {
            Ok(plan) => plan,
            Err(CheckError::Pending(_)) => {
                return notify_for_entry(get_entry(key).expect("existing setup"))
            }
            Err(CheckError::Unsafe(message)) => {
                return RelaySetupNotifyResult::ManualRecoveryRequired {
                    phase: progress_for_recovery(key)
                        .map(|p| p.phase)
                        .unwrap_or(RelayCreationPhase::ChildCreated),
                    relay_canister_id: progress_for_recovery(key).and_then(|p| p.relay_canister_id),
                    message,
                }
            }
        };
        if plan.quote.additional_e8s > 0 {
            return RelaySetupNotifyResult::BelowCurrentRequirement {
                balance_e8s: plan.quote.balance_e8s,
                required_e8s: plan.quote.required_e8s,
                shortfall_e8s: plan.quote.additional_e8s,
            };
        }
        if state::relay_creating_keys().len() >= MAX_CONCURRENT_FUNDED_RELAY_SETUPS {
            return RelaySetupNotifyResult::Busy;
        }
        let mut progress = plan.progress;
        let mut continuation = plan.continuation;
        let outstanding = continuation
            .conversions
            .last()
            .is_some_and(|c| !c.rejected && c.cycles_minted.is_none());
        if outstanding || plan.quote.cycles_deficit > 0 {
            if !outstanding {
                // Re-read fee and rate for the actual immutable dispatch identity.
                let fee = match ticket.call(ledger.fee_e8s()).await {
                    Ok(Ok(v)) => v,
                    _ => return notify_for_entry(get_entry(key).unwrap()),
                };
                let rate = match ticket.call(cmc.get_icp_xdr_conversion_rate()).await {
                    Ok(Ok(v)) => v,
                    _ => return notify_for_entry(get_entry(key).unwrap()),
                };
                let (amount, required) = match recovery_conversion_quote(
                    plan.quote.cycles_deficit,
                    &rate,
                    continuation.identity.margin_e8s,
                    plan.quote.balance_e8s,
                    continuation.identity.final_reserve_e8s,
                    fee,
                ) {
                    Ok(value) => value,
                    Err(_) => return notify_for_entry(get_entry(key).unwrap()),
                };
                if required > plan.quote.balance_e8s {
                    return RelaySetupNotifyResult::BelowCurrentRequirement {
                        balance_e8s: plan.quote.balance_e8s,
                        required_e8s: required,
                        shortfall_e8s: required - plan.quote.balance_e8s,
                    };
                }
                let previous_time = continuation
                    .conversions
                    .iter()
                    .map(|op| op.transfer.created_at_time_nanos)
                    .chain(
                        progress
                            .cmc_transfer
                            .iter()
                            .map(|op| op.created_at_time_nanos),
                    )
                    .max()
                    .unwrap_or(0);
                let Some(next_time) = previous_time.checked_add(1) else {
                    return RelaySetupNotifyResult::ManualRecoveryRequired {
                        phase: progress.phase,
                        relay_canister_id: progress.relay_canister_id,
                        message: "Transfer identity timestamp exhausted; do not pay".into(),
                    };
                };
                // Only definitive no-spend rejections may be compacted. Accepted
                // or uncertain identities and every minted result stay intact.
                // Compute next_time before compaction, so even a rejected identity
                // is never reassigned to different transfer parameters.
                continuation.conversions.retain(|op| !op.rejected);
                continuation.conversions.push(RecoveryConversion {
                    transfer: RelayTransferRecord {
                        amount_e8s: amount,
                        fee_e8s: fee,
                        created_at_time_nanos: now_nanos().max(next_time),
                        block_index: None,
                    },
                    destination: progress.relay_canister_id.unwrap_or(historian),
                    for_creation: progress.relay_canister_id.is_none(),
                    to: cmc_deposit_account(
                        continuation.identity.cmc_id,
                        progress.relay_canister_id.unwrap_or(historian),
                    ),
                    memo: TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec(),
                    notifications: 0,
                    cycles_minted: None,
                    rejected: false,
                    dispatches: 0,
                });
            }
            if ticket.check().is_err() || progress_for_recovery(key).as_ref() != Some(&progress) {
                return RelaySetupNotifyResult::Busy;
            }
            continuation.attempt = continuation
                .attempt
                .checked_add(1)
                .expect("attempt overflow");
            progress.continuation = Some(continuation.clone());
            let operation = continuation.conversions.last().expect("conversion").clone();
            if operation.transfer.block_index.is_none() {
                continuation.conversions.last_mut().unwrap().dispatches =
                    operation.dispatches.saturating_add(1);
                progress.continuation = Some(continuation.clone());
            }
            // The same key and journal fence all callbacks; live Creating phases
            // are never admitted to this continuation route.
            insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
            let mut block = operation.transfer.block_index;
            if block.is_none() {
                if now_nanos().saturating_sub(operation.transfer.created_at_time_nanos)
                    > SAFE_TRANSFER_AGE_NANOS
                {
                    return manual_recovery(
                        key,
                        progress.phase,
                        "Recovery transfer deduplication guarantee expired; do not pay",
                    );
                }
                let reply = creation_call(ledger.icrc1_transfer(transfer_arg(
                    key,
                    operation.to,
                    &operation.transfer,
                    Some(operation.memo.clone()),
                )))
                .await;
                if require_phase(key, progress.phase).ok().as_ref() != Some(&progress) {
                    return notify_for_entry(get_entry(key).unwrap());
                }
                match accepted_transfer(reply) {
                    Ok(accepted) => {
                        block = Some(accepted);
                        continuation
                            .conversions
                            .last_mut()
                            .unwrap()
                            .transfer
                            .block_index = block;
                    }
                    Err(Err(error)) => {
                        return pause_retry(
                            key,
                            progress.phase,
                            format!("Recovery Ledger reply unknown: {error}"),
                        )
                    }
                    Err(Ok(TransferError::TooOld)) => {
                        return manual_recovery(
                            key,
                            progress.phase,
                            "Recovery transfer is too old to reconcile; do not pay",
                        )
                    }
                    Err(Ok(TransferError::TemporarilyUnavailable)) if operation.dispatches > 0 => {
                        return pause_retry(
                            key,
                            progress.phase,
                            "Recovery Ledger temporarily unavailable; the transfer outcome is still unknown",
                        )
                    }
                    Err(Ok(error)) if operation.dispatches > 0 => {
                        return manual_recovery(key, progress.phase, format!("Previously uncertain recovery transfer cannot be replaced: {error:?}"));
                    }
                    Err(Ok(error)) => {
                        continuation.conversions.last_mut().unwrap().rejected = true;
                        progress.continuation = Some(continuation);
                        insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
                        return pause_retry(
                            key,
                            progress.phase,
                            format!("Recovery transfer rejected: {error:?}"),
                        );
                    }
                }
                progress.continuation = Some(continuation.clone());
                insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
                if progress.cmc_transfer.as_ref().and_then(|op| op.block_index) == block
                    || continuation.conversions[..continuation.conversions.len() - 1]
                        .iter()
                        .any(|op| op.transfer.block_index == block)
                {
                    return manual_recovery(
                        key,
                        progress.phase,
                        "Recovery accepted block duplicates an earlier operation; do not pay",
                    );
                }
            }
            let notifications = continuation.conversions.last().unwrap().notifications;
            continuation.conversions.last_mut().unwrap().notifications =
                notifications.saturating_add(1);
            progress.continuation = Some(continuation.clone());
            insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
            let reply = creation_call(
                cmc.notify_top_up(operation.destination, block.expect("accepted block")),
            )
            .await;
            if require_phase(key, progress.phase).ok().as_ref() != Some(&progress) {
                return notify_for_entry(get_entry(key).unwrap());
            }
            match reply {
                Ok(minted) => {
                    continuation.conversions.last_mut().unwrap().cycles_minted = Some(minted);
                }
                Err(
                    jupiter_ic_clients::cmc::NotifyTopUpError::Retryable(_)
                    | jupiter_ic_clients::cmc::NotifyTopUpError::Transport(_),
                ) => return pause_retry(
                    key,
                    progress.phase,
                    "Recovery CMC notification pending; continue checks the same accepted block",
                ),
                Err(error) => {
                    return manual_recovery(
                        key,
                        progress.phase,
                        format!("Recovery CMC result cannot be retried: {error:?}"),
                    )
                }
            }
            progress.continuation = Some(continuation);
            insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
            pause(
                key,
                progress.phase,
                "Recovery conversion recorded; rechecking remaining work",
            );
            continue;
        }
        continuation.attempt = continuation
            .attempt
            .checked_add(1)
            .expect("attempt overflow");
        if plan.installed && progress.phase == RelayCreationPhase::ChildCreated {
            progress.phase = RelayCreationPhase::CodeInstalled;
        }
        if progress.phase == RelayCreationPhase::ChildCreated {
            continuation.install_retried = true;
            continuation.funding_pause = false;
        }
        if progress.relay_canister_id.is_none() {
            progress.phase = RelayCreationPhase::CmcNotifySucceeded;
        }
        progress.continuation = Some(continuation);
        insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
        return match progress.phase {
            RelayCreationPhase::CmcNotifySucceeded => {
                continue_paid_creation(
                    key, setup, config, historian, ledger, cmc, management, ticket,
                )
                .await
            }
            RelayCreationPhase::ChildCreated => {
                install_and_fund(key, setup, config, historian, ledger, management, ticket).await
            }
            RelayCreationPhase::CodeInstalled => {
                fund_installed_relay(key, setup, config, historian, ledger, management, ticket)
                    .await
            }
            RelayCreationPhase::RelayFunded | RelayCreationPhase::FinalizationAttempted => {
                drive_relay_finalization(key, setup, historian, management, ticket).await
            }
            _ => manual_recovery(key, progress.phase, "Unexpected continuation phase"),
        };
    }
    notify_for_entry(get_entry(key).expect("existing setup"))
}

pub(super) fn creation_credit(progress: &RelayCreationProgress) -> Option<u128> {
    progress
        .continuation
        .as_ref()?
        .conversions
        .iter()
        .filter(|op| op.for_creation)
        .filter_map(|op| op.cycles_minted)
        .try_fold(progress.cycles_minted?, |n, amount| n.checked_add(amount))
}

#[cfg(test)]
pub(super) async fn quote_for_test(
    key: RelaySetupKey,
    _setup: &CanonicalRelaySetup,
    config: &Config,
    ledger: &dyn LedgerClient,
    cmc: &dyn CmcClient,
    management: &dyn ManagementClient,
) -> RelayRecoveryView {
    let Some(guard) = admission::Guard::acquire(key, admission::Lane::Assessment) else {
        return RelayRecoveryView::CheckPending("Recovery check is busy; check again".into());
    };
    match inspect(
        key,
        config,
        self_canister_id(),
        ledger,
        cmc,
        management,
        guard.ticket(),
    )
    .await
    {
        Ok(plan) if plan.quote.additional_e8s > 0 => RelayRecoveryView::FundingRequired(plan.quote),
        Ok(plan) => RelayRecoveryView::Ready(plan.quote),
        Err(CheckError::Pending(e)) => RelayRecoveryView::CheckPending(e),
        Err(CheckError::Unsafe(e)) => RelayRecoveryView::Unsafe(e),
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    #[test]
    fn budget_covers_upfront_reservation_and_supported_memory_at_34_nodes() {
        let config = super::super::tests::config();
        let amount = creation_budget_at_cost(&config, MAX_SUPPORTED_CREATION_COST).unwrap();
        assert_eq!(amount, 4_923_076_923_076);
        for nodes in 13..=34 {
            let cost = 500_000_000_000u128 * nodes / 13;
            let child = creation_budget_at_cost(&config, cost).unwrap() - cost;
            let upfront_execution = 300_000_000_000u128 * nodes / 13 + 20_000_000;
            let freezing =
                317_500u128 * 2_592_000 * nodes / (13 * 4) + 10_000u128 * 2_592_000 * nodes / 13; // 256 MiB plus base fee, 30 days.
            let initialization = 300_000_000_000u128;
            let operating = 1_000_000_000_000u128;
            assert!(child > upfront_execution + freezing + initialization + operating);
        }
        assert!(creation_budget_at_cost(&config, MAX_SUPPORTED_CREATION_COST + 1).is_err());
        assert!(creation_budget_at_cost(&config, MIN_SUPPORTED_CREATION_COST - 1).is_err());
        assert!(creation_budget_at_cost(&config, 0).is_err());
    }
}
