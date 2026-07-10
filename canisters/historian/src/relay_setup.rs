use crate::clients::index::{account_identifier_text_for_account, IndexOperation};
use crate::clients::{
    BlackholeClient, ClientError, CmcCanister, CmcClient, IcpXdrConversionRate, IndexClient,
    LedgerClient,
};
use crate::state::{self, Config, RelayRegistryStatus, State};
use crate::*;
use candid::{Encode, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::TransferError;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, Memo, TransferArg};
use jupiter_ic_clients::account::{principal_to_subaccount, relay_setup_subaccount};
use jupiter_ic_clients::cmc::NotifyTopUpError;
use jupiter_ic_clients::ledger::LegacyTransferError;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const RELAY_SUBACCOUNT_ONE: [u8; 32] = {
    let mut bytes = [0u8; 32];
    bytes[31] = 1;
    bytes
};

const TOP_UP_CANISTER_MEMO: u64 = 1_347_768_404;
const REFUND_MEMO: u64 = 0x4a525246;
const INDEX_PAGE_LIMIT: usize = 20;
const INDEX_PAGE_SIZE: u64 = 100;
const LEDGER_DUPLICATE_WINDOW_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;

struct IndexedSetupPayments {
    payments: Vec<RelaySetupPayment>,
    hit_page_cap: bool,
}

pub(crate) fn setup_account_for(historian: Principal, target: Principal) -> Account {
    Account {
        owner: historian,
        subaccount: Some(relay_setup_subaccount(target)),
    }
}

pub(crate) fn cmc_deposit_account(cmc_id: Principal, canister_id: Principal) -> Account {
    Account {
        owner: cmc_id,
        subaccount: Some(principal_to_subaccount(canister_id)),
    }
}

pub(crate) fn relay_subaccount_one(relay_id: Principal) -> Account {
    Account {
        owner: relay_id,
        subaccount: Some(RELAY_SUBACCOUNT_ONE),
    }
}

pub(crate) fn relay_wasm_hash_hex() -> Option<String> {
    approved_self_service_relay_wasm_hash_hex()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ManagementClientError {
    Ambiguous(String),
    Failed(String),
}

impl std::fmt::Display for ManagementClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ambiguous(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

impl From<ic_cdk::call::Error> for ManagementClientError {
    fn from(value: ic_cdk::call::Error) -> Self {
        if let ic_cdk::call::Error::CallRejected(rejected) = &value {
            if rejected.reject_code() == Ok(ic_cdk::call::RejectCode::SysUnknown) {
                return Self::Ambiguous(format!("{value:?}"));
            }
        }
        Self::Failed(format!("{value:?}"))
    }
}

#[async_trait::async_trait]
trait ManagementClient: Send + Sync {
    async fn create_canister(
        &self,
        arg: &jupiter_ic_clients::management::CreateCanisterArgs,
        cycles_to_attach: u128,
    ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, ManagementClientError>;
    async fn install_code(
        &self,
        arg: &jupiter_ic_clients::management::InstallCodeArgs,
    ) -> Result<(), ManagementClientError>;
    async fn canister_status(
        &self,
        arg: &jupiter_ic_clients::management::CanisterStatusArgs,
    ) -> Result<jupiter_ic_clients::management::CanisterStatusResult, ManagementClientError>;
    async fn update_settings(
        &self,
        arg: &jupiter_ic_clients::management::UpdateSettingsArgs,
    ) -> Result<(), ManagementClientError>;
}

struct IcManagementClient;

#[async_trait::async_trait]
impl ManagementClient for IcManagementClient {
    async fn create_canister(
        &self,
        arg: &jupiter_ic_clients::management::CreateCanisterArgs,
        cycles_to_attach: u128,
    ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, ManagementClientError> {
        jupiter_ic_clients::management::create_canister(arg, cycles_to_attach)
            .await
            .map_err(Into::into)
    }

    async fn install_code(
        &self,
        arg: &jupiter_ic_clients::management::InstallCodeArgs,
    ) -> Result<(), ManagementClientError> {
        jupiter_ic_clients::management::install_code(arg)
            .await
            .map_err(Into::into)
    }

    async fn canister_status(
        &self,
        arg: &jupiter_ic_clients::management::CanisterStatusArgs,
    ) -> Result<jupiter_ic_clients::management::CanisterStatusResult, ManagementClientError> {
        jupiter_ic_clients::management::canister_status(arg)
            .await
            .map_err(Into::into)
    }

    async fn update_settings(
        &self,
        arg: &jupiter_ic_clients::management::UpdateSettingsArgs,
    ) -> Result<(), ManagementClientError> {
        jupiter_ic_clients::management::update_settings(arg)
            .await
            .map_err(Into::into)
    }
}

#[cfg(not(test))]
fn now_nanos() -> u64 {
    ic_cdk::api::time()
}

#[cfg(test)]
static TEST_NOW_NANOS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1_000_000_000);

#[cfg(test)]
fn now_nanos() -> u64 {
    TEST_NOW_NANOS.fetch_add(1_000_000_000, std::sync::atomic::Ordering::SeqCst)
}

fn now_secs() -> u64 {
    now_nanos() / 1_000_000_000
}

#[cfg(not(test))]
fn self_canister_id() -> Principal {
    ic_cdk::api::canister_self()
}

#[cfg(test)]
fn self_canister_id() -> Principal {
    Principal::from_slice(&[42])
}

pub(crate) fn setup_view(target: Principal) -> RelaySetupView {
    state::with_state(|st| setup_view_from_state(st, target, self_canister_id()))
}

fn redacted_transfer(record: &RelaySetupTransferRecord) -> RedactedTransferRecord {
    RedactedTransferRecord {
        kind: record.kind.clone(),
        from_account_identifier: record.from_account_identifier.clone(),
        to_account_identifier: record.to_account_identifier.clone(),
        amount_e8s: record.amount_e8s,
        fee_e8s: record.fee_e8s,
        created_at_time_nanos: record.created_at_time_nanos,
        block_index: record.block_index,
        completed: record.completed,
    }
}

pub(crate) fn setup_recovery_view(target: Principal) -> RelaySetupRecoveryView {
    state::with_state(|st| {
        let setup_account = setup_account_for(self_canister_id(), target);
        let setup_account_identifier = account_identifier_text_for_account(&setup_account);
        if let Some(job) = st.relay_setup_jobs.get(&target) {
            return RelaySetupRecoveryView {
                target_canister_id: target,
                status: RelaySetupPublicStatus::from(job.status.clone()),
                last_error: job.last_error.clone(),
                relay_canister_id: job.relay_canister_id,
                setup_account_identifier: job.setup_account_identifier.clone(),
                setup_amount_seen_e8s: job.setup_amount_seen_e8s,
                setup_amount_processed_e8s: job.setup_amount_processed_e8s,
                cycle_transfer: job.cycle_transfer.as_ref().map(redacted_transfer),
                relay_funding_transfer: job.relay_funding_transfer.as_ref().map(redacted_transfer),
                existing_relay_sweep_transfer: job
                    .existing_relay_sweep_transfer
                    .as_ref()
                    .map(redacted_transfer),
                refund_transfer_count: job.refund_transfers.len() as u32,
                relay_create_attempt: job.relay_create_attempt.as_ref().map(|attempt| {
                    RelayCreateAttemptView {
                        target_canister_id: attempt.target_canister_id,
                        created_at_ts: attempt.created_at_ts,
                        initial_cycles: attempt.initial_cycles,
                        relay_wasm_hash_hex: attempt.relay_wasm_hash_hex.clone(),
                    }
                }),
                created_at_ts: job.created_at_ts,
                updated_at_ts: job.updated_at_ts,
            };
        }
        RelaySetupRecoveryView {
            target_canister_id: target,
            status: setup_view_from_state(st, target, self_canister_id()).status,
            last_error: None,
            relay_canister_id: None,
            setup_account_identifier,
            setup_amount_seen_e8s: 0,
            setup_amount_processed_e8s: 0,
            cycle_transfer: None,
            relay_funding_transfer: None,
            existing_relay_sweep_transfer: None,
            refund_transfer_count: 0,
            relay_create_attempt: None,
            created_at_ts: 0,
            updated_at_ts: 0,
        }
    })
}

pub(crate) fn setup_view_from_state(
    st: &State,
    target: Principal,
    historian: Principal,
) -> RelaySetupView {
    let setup_account = setup_account_for(historian, target);
    let setup_account_identifier = account_identifier_text_for_account(&setup_account);
    let setup_job = st.relay_setup_jobs.get(&target).cloned();
    let existing_relay = st
        .relay_registry_by_target
        .get(&target)
        .filter(|entry| entry.status == RelayRegistryStatus::Active)
        .cloned();
    let factory_available =
        st.config.relay_factory_enabled && approved_self_service_relay_wasm().is_some();
    let payment_blocked_reason = if existing_relay.is_some() {
        None
    } else if let Some(message) = invalid_target(target, &st.config, historian) {
        Some(message)
    } else if !st.config.relay_factory_enabled {
        Some("relay factory is disabled".to_string())
    } else if st.config.cmc_canister_id.is_none() {
        Some("CMC canister is not configured".to_string())
    } else if approved_self_service_relay_wasm().is_none() {
        Some("approved relay wasm is not embedded".to_string())
    } else {
        None
    };
    let payment_allowed = existing_relay.is_some() || payment_blocked_reason.is_none();
    let status = if existing_relay.is_some() {
        RelaySetupPublicStatus::Active
    } else {
        setup_job
            .as_ref()
            .map(|job| RelaySetupPublicStatus::from(job.status.clone()))
            .unwrap_or_else(|| {
                if payment_allowed {
                    RelaySetupPublicStatus::NotFunded
                } else {
                    RelaySetupPublicStatus::PaymentNotAllowed
                }
            })
    };
    RelaySetupView {
        target_canister_id: target,
        setup_account,
        setup_account_identifier,
        minimum_e8s: st.config.relay_setup_min_e8s,
        payment_allowed,
        payment_blocked_reason,
        existing_relay: existing_relay.map(Into::into),
        status,
        factory_available,
        relay_wasm_hash_hex: relay_wasm_hash_hex(),
        warning_text: Some(
            "This relay can only be created for canisters whose cycle balance is visible through the Jupiter blackhole canister. In practice, the configured blackhole canister must be able to call canister_status for the target.".to_string(),
        ),
    }
}

pub(crate) fn list_relay_registrations(
    args: ListRelayRegistrationsArgs,
) -> ListRelayRegistrationsResponse {
    state::with_state(|st| {
        let limit = clamp_public_limit(args.limit, 100);
        let mut items = Vec::new();
        let mut next = None;
        for (target, entry) in st.relay_registry_by_target.iter() {
            if args
                .start_after
                .map(|start_after| *target <= start_after)
                .unwrap_or(false)
            {
                continue;
            }
            if entry.status != RelayRegistryStatus::Active {
                continue;
            }
            if items.len() >= limit {
                next = items
                    .last()
                    .map(|item: &RelayRegistration| item.target_canister_id);
                break;
            }
            items.push(entry.clone().into());
        }
        ListRelayRegistrationsResponse {
            items,
            next_start_after: next,
        }
    })
}

fn invalid_target(target: Principal, cfg: &Config, historian: Principal) -> Option<String> {
    if target == Principal::anonymous() {
        return Some("target must not be anonymous".to_string());
    }
    if target == Principal::management_canister() {
        return Some("target must not be the management canister".to_string());
    }
    if target == historian {
        return Some("target must not be the historian canister".to_string());
    }
    if target == cfg.blackhole_canister_id
        || target == cfg.ledger_canister_id
        || target == cfg.index_canister_id
        || Some(target) == cfg.cmc_canister_id
    {
        return Some("target must not be a configured protocol dependency".to_string());
    }
    None
}

fn reserve_job(
    target: Principal,
    setup_account: Account,
    setup_account_identifier: String,
) -> RelaySetupJob {
    let ts = now_secs();
    RelaySetupJob {
        target_canister_id: target,
        setup_account,
        setup_account_identifier,
        status: RelaySetupStatus::Pending,
        relay_canister_id: None,
        last_indexed_setup_tx_id: None,
        setup_tx_ids: Vec::new(),
        setup_amount_seen_e8s: 0,
        setup_amount_processed_e8s: 0,
        payments: Vec::new(),
        cycle_conversion_e8s: None,
        cycle_transfer_block_index: None,
        cycles_minted: None,
        relay_initial_cycles: None,
        relay_funding_e8s: None,
        relay_funding_block_index: None,
        phase: Some(RelaySetupPhase::PreSpend),
        cycle_transfer: None,
        relay_funding_transfer: None,
        existing_relay_sweep_transfer: None,
        refund_transfers: Vec::new(),
        relay_create_attempt: None,
        code_installed: false,
        relay_funding_accepted: false,
        blackhole_update_attempted: false,
        blackhole_confirmed: false,
        refund_attempt_count: 0,
        last_refund_attempt_ts: None,
        refund_blocks: Vec::new(),
        created_at_ts: ts,
        updated_at_ts: ts,
        last_error: None,
    }
}

fn set_job_status(target: Principal, status: RelaySetupStatus, error: Option<String>) {
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = status;
            job.updated_at_ts = now_secs();
            job.last_error = error;
        }
    });
}

fn set_job_failed_retryable(target: Principal, error: String) {
    set_job_status(target, RelaySetupStatus::FailedRetryable, Some(error));
}

async fn index_setup_payments(
    target: Principal,
    setup_account_identifier: String,
    index: &dyn IndexClient,
) -> Result<IndexedSetupPayments, String> {
    let mut payments = Vec::new();
    let mut start = None;
    let mut hit_page_cap = true;
    for _ in 0..INDEX_PAGE_LIMIT {
        let resp = index
            .get_account_identifier_transactions(
                setup_account_identifier.clone(),
                start,
                INDEX_PAGE_SIZE,
            )
            .await
            .map_err(|err| err.to_string())?;
        let transaction_count = resp.transactions.len();
        for tx in resp.transactions {
            let IndexOperation::Transfer {
                from, to, amount, ..
            } = &tx.transaction.operation
            else {
                continue;
            };
            if to != &setup_account_identifier {
                continue;
            }
            payments.push(RelaySetupPayment {
                target_canister_id: target,
                tx_id: tx.id,
                from_account_identifier: from.clone(),
                amount_e8s: amount.e8s(),
                timestamp_nanos: tx
                    .transaction
                    .timestamp
                    .as_ref()
                    .map(|ts| ts.timestamp_nanos)
                    .or_else(|| {
                        tx.transaction
                            .created_at_time
                            .as_ref()
                            .map(|ts| ts.timestamp_nanos)
                    }),
                processed: false,
                refunded: false,
            });
        }
        let Some(oldest) = resp.oldest_tx_id else {
            hit_page_cap = false;
            break;
        };
        if transaction_count < INDEX_PAGE_SIZE as usize {
            hit_page_cap = false;
            break;
        }
        start = Some(oldest);
    }
    Ok(IndexedSetupPayments {
        payments,
        hit_page_cap,
    })
}

fn merge_payments(job: &mut RelaySetupJob, payments: Vec<RelaySetupPayment>) {
    let mut seen: BTreeSet<u64> = job.payments.iter().map(|payment| payment.tx_id).collect();
    for payment in payments {
        if seen.insert(payment.tx_id) {
            job.last_indexed_setup_tx_id = job.last_indexed_setup_tx_id.max(Some(payment.tx_id));
            job.setup_tx_ids.push(payment.tx_id);
            job.setup_amount_seen_e8s =
                job.setup_amount_seen_e8s.saturating_add(payment.amount_e8s);
            job.payments.push(payment);
        }
    }
}

fn in_flight_job(job: &RelaySetupJob) -> bool {
    matches!(
        job.status,
        RelaySetupStatus::Pending
            | RelaySetupStatus::ConvertingCycles
            | RelaySetupStatus::CycleTransferAccepted
            | RelaySetupStatus::CycleNotifySucceeded
            | RelaySetupStatus::CreatingCanister
            | RelaySetupStatus::CanisterCreated
            | RelaySetupStatus::InstallingCode
            | RelaySetupStatus::CodeInstalled
            | RelaySetupStatus::SettingPublicLogs
            | RelaySetupStatus::FundingRelaySubaccountOne
            | RelaySetupStatus::Blackholing
            | RelaySetupStatus::Refunding
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelaySetupResumePoint {
    PreSpend,
    NotifyCycleTopUp { block_index: u64 },
    CreateRelayCanister,
    InstallRelayCode { relay_id: Principal },
    FundRelaySubaccountOne { relay_id: Principal },
    BlackholeRelay { relay_id: Principal },
    RegisterActive { relay_id: Principal },
    ReconcileCycleTransfer,
}

fn relay_setup_resume_point(job: &RelaySetupJob) -> RelaySetupResumePoint {
    if let Some(relay_id) = job.relay_canister_id {
        if job.relay_funding_block_index.is_some() {
            return if matches!(job.status, RelaySetupStatus::Blackholing) {
                RelaySetupResumePoint::BlackholeRelay { relay_id }
            } else {
                RelaySetupResumePoint::RegisterActive { relay_id }
            };
        }
        return if matches!(
            job.status,
            RelaySetupStatus::CodeInstalled | RelaySetupStatus::FundingRelaySubaccountOne
        ) {
            RelaySetupResumePoint::FundRelaySubaccountOne { relay_id }
        } else {
            RelaySetupResumePoint::InstallRelayCode { relay_id }
        };
    }
    if job.cycles_minted.is_some() {
        RelaySetupResumePoint::CreateRelayCanister
    } else if let Some(block_index) = job.cycle_transfer_block_index {
        RelaySetupResumePoint::NotifyCycleTopUp { block_index }
    } else if job.cycle_transfer.is_some() {
        RelaySetupResumePoint::ReconcileCycleTransfer
    } else {
        RelaySetupResumePoint::PreSpend
    }
}

fn resumable_job(job: &RelaySetupJob) -> bool {
    !matches!(
        relay_setup_resume_point(job),
        RelaySetupResumePoint::PreSpend
    )
}

fn indexed_inbound_total_for_job(job: &RelaySetupJob) -> u64 {
    job.payments
        .iter()
        .filter(|payment| !payment.refunded)
        .fold(0u64, |acc, payment| acc.saturating_add(payment.amount_e8s))
}

fn refund_allowed_before_spend(job: &RelaySetupJob) -> bool {
    job.cycle_transfer_block_index.is_none()
        && job.cycles_minted.is_none()
        && job.relay_canister_id.is_none()
        && job.relay_funding_block_index.is_none()
        && job.cycle_transfer.is_none()
        && job.relay_funding_transfer.is_none()
        && job.setup_amount_processed_e8s == 0
}

fn refund_eligible_status(job: &RelaySetupJob) -> bool {
    matches!(
        job.status,
        RelaySetupStatus::BelowMinimum
            | RelaySetupStatus::TargetNotObservable
            | RelaySetupStatus::InsufficientForCurrentRate
            | RelaySetupStatus::IndexNotReady
            | RelaySetupStatus::RefundAvailable
    ) || (matches!(job.status, RelaySetupStatus::FailedRetryable)
        && refund_allowed_before_spend(job))
}

pub(crate) fn required_setup_e8s(cfg: &Config, fee_e8s: u64) -> u64 {
    cfg.relay_setup_min_e8s.max(
        fee_e8s
            .saturating_mul(4)
            .saturating_add(cfg.relay_cycle_safety_margin_e8s)
            .saturating_add(cfg.relay_min_subaccount_one_seed_e8s),
    )
}

fn cycle_conversion_e8s(cfg: &Config, fee_e8s: u64, balance: u64) -> Option<u64> {
    let keep = cfg
        .relay_min_subaccount_one_seed_e8s
        .saturating_add(cfg.relay_cycle_safety_margin_e8s)
        .saturating_add(fee_e8s.saturating_mul(3));
    balance.checked_sub(keep).map(|amount| {
        amount
            .min(cfg.relay_setup_min_e8s / 2)
            .max(fee_e8s.saturating_mul(2))
    })
}

fn transfer_arg(
    from_subaccount: Option<[u8; 32]>,
    to: Account,
    amount: u64,
    fee: u64,
    memo: Option<Vec<u8>>,
    created_at_time_nanos: u64,
) -> TransferArg {
    TransferArg {
        from_subaccount,
        to,
        amount: amount.into(),
        fee: Some(fee.into()),
        memo: memo.map(Memo::from),
        created_at_time: Some(created_at_time_nanos),
    }
}

fn transfer_record(
    kind: RelaySetupTransferKind,
    from_subaccount: Option<[u8; 32]>,
    from_account: Account,
    to: Account,
    amount_e8s: u64,
    fee_e8s: u64,
    memo: Option<Vec<u8>>,
) -> RelaySetupTransferRecord {
    RelaySetupTransferRecord {
        kind,
        from_subaccount,
        from_account_identifier: account_identifier_text_for_account(&from_account),
        to_account_identifier: account_identifier_text_for_account(&to),
        to,
        amount_e8s,
        fee_e8s,
        memo,
        created_at_time_nanos: now_nanos(),
        block_index: None,
        completed: false,
    }
}

fn record_to_transfer_arg(record: &RelaySetupTransferRecord) -> TransferArg {
    transfer_arg(
        record.from_subaccount,
        record.to,
        record.amount_e8s,
        record.fee_e8s,
        record.memo.clone(),
        record.created_at_time_nanos,
    )
}

fn record_block_index(block: BlockIndex) -> u64 {
    u64::try_from(block.0).unwrap_or(u64::MAX)
}

fn transfer_error_duplicate_block(err: &TransferError) -> Option<u64> {
    match err {
        TransferError::Duplicate { duplicate_of } => {
            Some(u64::try_from(duplicate_of.0.clone()).unwrap_or(u64::MAX))
        }
        _ => None,
    }
}

fn classify_transfer_response(
    result: Result<Result<BlockIndex, TransferError>, ClientError>,
) -> Result<u64, Result<TransferError, ClientError>> {
    match result {
        Ok(Ok(block)) => Ok(record_block_index(block)),
        Ok(Err(err)) => transfer_error_duplicate_block(&err).ok_or(Ok(err)),
        Err(err) => Err(Err(err)),
    }
}

async fn find_recorded_transfer_in_index(
    record: &RelaySetupTransferRecord,
    index: &dyn IndexClient,
) -> Result<Option<u64>, String> {
    let mut start = None;
    for _ in 0..20 {
        let resp = index
            .get_account_identifier_transactions(record.from_account_identifier.clone(), start, 100)
            .await
            .map_err(|err| err.to_string())?;
        for tx in &resp.transactions {
            let IndexOperation::Transfer {
                from, to, amount, ..
            } = &tx.transaction.operation
            else {
                continue;
            };
            if from == &record.from_account_identifier
                && to == &record.to_account_identifier
                && amount.e8s() == record.amount_e8s
                && tx
                    .transaction
                    .created_at_time
                    .as_ref()
                    .map(|ts| ts.timestamp_nanos)
                    .or_else(|| {
                        tx.transaction
                            .timestamp
                            .as_ref()
                            .map(|ts| ts.timestamp_nanos)
                    })
                    == Some(record.created_at_time_nanos)
            {
                return Ok(Some(tx.id));
            }
        }
        let Some(oldest) = resp.oldest_tx_id else {
            break;
        };
        if resp.transactions.is_empty() {
            break;
        }
        start = Some(oldest);
    }
    Ok(None)
}

fn note_transfer_success(record: &mut RelaySetupTransferRecord, block_index: u64) {
    record.block_index = Some(block_index);
    record.completed = true;
}

fn pending_transfer_is_stale(record: &RelaySetupTransferRecord) -> bool {
    !record.completed
        && record.block_index.is_none()
        && now_nanos().saturating_sub(record.created_at_time_nanos) > LEDGER_DUPLICATE_WINDOW_NANOS
}

fn mark_manual_recovery_required(target: Principal, message: String) -> RelaySetupNotifyResult {
    set_job_status(
        target,
        RelaySetupStatus::ManualRecoveryRequired,
        Some(message.clone()),
    );
    RelaySetupNotifyResult::Failed {
        status: RelaySetupStatus::ManualRecoveryRequired.into(),
        message,
    }
}

fn stale_transfer_manual_recovery(
    target: Principal,
    record: &RelaySetupTransferRecord,
) -> RelaySetupNotifyResult {
    mark_manual_recovery_required(
        target,
        format!(
            "{:?} transfer created at {} is older than the ledger duplicate window and was not found in the index",
            record.kind, record.created_at_time_nanos
        ),
    )
}

fn handle_cmc_notify_error(target: Principal, err: NotifyTopUpError) -> RelaySetupNotifyResult {
    match err {
        NotifyTopUpError::Retryable(retryable) => {
            let message = format!("{retryable:?}");
            set_job_failed_retryable(target, message.clone());
            RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message,
            }
        }
        NotifyTopUpError::Transport(message)
        | NotifyTopUpError::Decode(message)
        | NotifyTopUpError::Convert(message) => {
            set_job_failed_retryable(target, message.clone());
            RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message,
            }
        }
        NotifyTopUpError::Terminal(terminal) => {
            let message = format!("{terminal:?}");
            set_job_status(
                target,
                RelaySetupStatus::FailedTerminal,
                Some(message.clone()),
            );
            RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedTerminal.into(),
                message,
            }
        }
    }
}

fn refund_result_to_notify(result: RelaySetupRefundResult) -> RelaySetupNotifyResult {
    match result {
        RelaySetupRefundResult::Refunded { blocks } => RelaySetupNotifyResult::Refunded { blocks },
        RelaySetupRefundResult::NoRefundableAmount => RelaySetupNotifyResult::RefundPending {
            reason: "no refundable payment amount was found".to_string(),
        },
        RelaySetupRefundResult::Cooldown {
            retry_after_seconds,
        } => RelaySetupNotifyResult::RefundPending {
            reason: format!("refund retry is cooling down for {retry_after_seconds} seconds"),
        },
        RelaySetupRefundResult::NotEligible { status } => RelaySetupNotifyResult::RefundPending {
            reason: status
                .map(|status| format!("setup status is not refundable: {status:?}"))
                .unwrap_or_else(|| "setup job is not refundable".to_string()),
        },
        RelaySetupRefundResult::Failed { message } => {
            RelaySetupNotifyResult::RefundPending { reason: message }
        }
    }
}

async fn auto_refund_pre_spend(
    historian: Principal,
    target: Principal,
    reason: String,
    ledger: &dyn LedgerClient,
    index: &dyn IndexClient,
) -> RelaySetupNotifyResult {
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = RelaySetupStatus::RefundAvailable;
            job.last_error = Some(reason);
            job.updated_at_ts = now_secs();
        }
    });
    refund_result_to_notify(
        request_relay_setup_refund_with_clients_for_historian(historian, target, ledger, index)
            .await,
    )
}

async fn sweep_existing(
    target: Principal,
    relay: RelayRegistryEntry,
    balance: u64,
    fee: u64,
    ledger: &dyn LedgerClient,
    index: &dyn IndexClient,
    historian: Principal,
) -> RelaySetupNotifyResult {
    let from_subaccount = Some(relay_setup_subaccount(target));
    let setup_account = setup_account_for(historian, target);
    let pending_record = state::with_state(|st| {
        st.relay_setup_jobs
            .get(&target)
            .and_then(|job| job.existing_relay_sweep_transfer.clone())
            .filter(|record| {
                !record.completed && record.to == relay_subaccount_one(relay.relay_canister_id)
            })
    });
    if pending_record.is_none()
        && balance <= fee.saturating_add(state::with_state(|st| st.config.relay_setup_dust_e8s))
    {
        return RelaySetupNotifyResult::SweepBelowDust {
            relay: relay.into(),
            current_balance_e8s: balance,
        };
    }
    let amount = balance.saturating_sub(fee);
    let mut record = state::with_root_and_relay_factory_state_mut(target, |st| {
        let setup_account_identifier = account_identifier_text_for_account(&setup_account);
        let job = st
            .relay_setup_jobs
            .entry(target)
            .or_insert_with(|| reserve_job(target, setup_account, setup_account_identifier));
        let record = pending_record.unwrap_or_else(|| {
            transfer_record(
                RelaySetupTransferKind::ExistingRelaySweep,
                from_subaccount,
                setup_account,
                relay_subaccount_one(relay.relay_canister_id),
                amount,
                fee,
                None,
            )
        });
        job.existing_relay_sweep_transfer = Some(record.clone());
        job.status = RelaySetupStatus::SweepingToExistingRelay;
        job.updated_at_ts = now_secs();
        record
    });
    if let Some(block_index) = record.block_index {
        return RelaySetupNotifyResult::SweptToExistingRelay {
            relay: relay.into(),
            amount_e8s: record.amount_e8s,
            block_index,
        };
    }
    match find_recorded_transfer_in_index(&record, index).await {
        Ok(Some(block_index)) => {
            note_transfer_success(&mut record, block_index);
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.existing_relay_sweep_transfer = Some(record.clone());
                    job.status = RelaySetupStatus::SweptToExistingRelay;
                    job.updated_at_ts = now_secs();
                }
            });
            return RelaySetupNotifyResult::SweptToExistingRelay {
                relay: relay.into(),
                amount_e8s: record.amount_e8s,
                block_index,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message: err,
            }
        }
    }
    if pending_transfer_is_stale(&record) {
        return stale_transfer_manual_recovery(target, &record);
    }
    match classify_transfer_response(ledger.icrc1_transfer(record_to_transfer_arg(&record)).await) {
        Ok(block_index) => {
            note_transfer_success(&mut record, block_index);
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.existing_relay_sweep_transfer = Some(record.clone());
                    job.status = RelaySetupStatus::SweptToExistingRelay;
                    job.updated_at_ts = now_secs();
                }
            });
            RelaySetupNotifyResult::SweptToExistingRelay {
                relay: relay.into(),
                amount_e8s: record.amount_e8s,
                block_index,
            }
        }
        Err(Ok(err)) => RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedRetryable.into(),
            message: format!("sweep transfer failed: {err:?}"),
        },
        Err(Err(err)) => RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::Ambiguous.into(),
            message: err.to_string(),
        },
    }
}

pub(crate) async fn notify_relay_setup_with_clients(
    target: Principal,
    ledger: &dyn LedgerClient,
    index: &dyn IndexClient,
    blackhole: &dyn BlackholeClient,
    cmc: &dyn CmcClient,
) -> RelaySetupNotifyResult {
    notify_relay_setup_with_clients_for_historian(
        self_canister_id(),
        target,
        ledger,
        index,
        blackhole,
        cmc,
    )
    .await
}

async fn notify_relay_setup_with_clients_for_historian(
    historian: Principal,
    target: Principal,
    ledger: &dyn LedgerClient,
    index: &dyn IndexClient,
    blackhole: &dyn BlackholeClient,
    cmc: &dyn CmcClient,
) -> RelaySetupNotifyResult {
    let cfg = state::with_state(|st| st.config.clone());
    let setup_account = setup_account_for(historian, target);
    let setup_account_identifier = account_identifier_text_for_account(&setup_account);
    let existing_job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned());
    let had_existing_job = existing_job.is_some();
    if let Some(job) = existing_job
        .as_ref()
        .filter(|job| job.status == RelaySetupStatus::ManualRecoveryRequired)
    {
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupPublicStatus::ManualRecoveryRequired,
            message: job
                .last_error
                .clone()
                .unwrap_or_else(|| "relay setup requires manual recovery before retry".to_string()),
        };
    }
    if let Some(job) = existing_job
        .as_ref()
        .filter(|job| in_flight_job(job) && !resumable_job(job))
    {
        return RelaySetupNotifyResult::Pending {
            status: RelaySetupPublicStatus::from(job.status.clone()),
        };
    }
    let resume_job = existing_job.filter(resumable_job);
    let has_resume_job = resume_job.is_some();
    let active_relay = state::with_state(|st| st.relay_registry_by_target.get(&target).cloned())
        .filter(|entry| entry.status == RelayRegistryStatus::Active);
    let fee = match ledger.fee_e8s().await {
        Ok(fee) => fee,
        Err(err) => {
            if had_existing_job {
                set_job_failed_retryable(target, err.to_string());
            }
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message: err.to_string(),
            };
        }
    };
    let balance = match ledger.balance_of_e8s(setup_account).await {
        Ok(balance) => balance,
        Err(err) => {
            if had_existing_job {
                set_job_failed_retryable(target, err.to_string());
            }
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message: err.to_string(),
            };
        }
    };
    if let Some(relay) = active_relay {
        return sweep_existing(target, relay, balance, fee, ledger, index, historian).await;
    }
    if let Some(job) = resume_job {
        let resume_point = relay_setup_resume_point(&job);
        let block_index = if let RelaySetupResumePoint::ReconcileCycleTransfer = resume_point {
            let mut cycle_record = job
                .cycle_transfer
                .clone()
                .expect("reconcile cycle transfer requires durable record");
            let block_index = if let Some(block_index) =
                match find_recorded_transfer_in_index(&cycle_record, index).await {
                    Ok(block_index) => block_index,
                    Err(err) => {
                        set_job_failed_retryable(target, err.clone());
                        return RelaySetupNotifyResult::Failed {
                            status: RelaySetupStatus::FailedRetryable.into(),
                            message: err,
                        };
                    }
                } {
                block_index
            } else {
                if pending_transfer_is_stale(&cycle_record) {
                    return stale_transfer_manual_recovery(target, &cycle_record);
                }
                match classify_transfer_response(
                    ledger
                        .icrc1_transfer(record_to_transfer_arg(&cycle_record))
                        .await,
                ) {
                    Ok(block_index) => block_index,
                    Err(Ok(err)) => {
                        set_job_status(
                            target,
                            RelaySetupStatus::FailedRetryable,
                            Some(format!("{err:?}")),
                        );
                        return RelaySetupNotifyResult::Failed {
                            status: RelaySetupStatus::FailedRetryable.into(),
                            message: format!("CMC transfer failed: {err:?}"),
                        };
                    }
                    Err(Err(err)) => {
                        set_job_status(target, RelaySetupStatus::Ambiguous, Some(err.to_string()));
                        return RelaySetupNotifyResult::Failed {
                            status: RelaySetupStatus::Ambiguous.into(),
                            message: err.to_string(),
                        };
                    }
                }
            };
            note_transfer_success(&mut cycle_record, block_index);
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.status = RelaySetupStatus::CycleTransferAccepted;
                    job.phase = Some(RelaySetupPhase::CycleTransferAccepted);
                    job.cycle_transfer = Some(cycle_record);
                    job.cycle_transfer_block_index = Some(block_index);
                    job.updated_at_ts = now_secs();
                }
            });
            Some(block_index)
        } else if let RelaySetupResumePoint::NotifyCycleTopUp { block_index } = resume_point {
            Some(block_index)
        } else {
            None
        };
        if let Some(block_index) = block_index {
            let minted = match cmc.notify_top_up(historian, block_index).await {
                Ok(cycles) => cycles,
                Err(err) => return handle_cmc_notify_error(target, err),
            };
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.status = RelaySetupStatus::CycleNotifySucceeded;
                    job.phase = Some(RelaySetupPhase::CycleNotifySucceeded);
                    job.cycles_minted = Some(minted);
                    job.updated_at_ts = now_secs();
                }
            });
        }
        let relay_funding = if job.relay_funding_block_index.is_some() {
            0
        } else {
            balance.saturating_sub(fee)
        };
        return create_and_activate_relay(
            target,
            relay_funding,
            fee,
            index,
            blackhole,
            &IcManagementClient,
            historian,
        )
        .await;
    }
    if let Some(message) = invalid_target(target, &cfg, historian) {
        if balance > cfg.relay_setup_dust_e8s {
            state::with_root_and_relay_factory_state_mut(target, |st| {
                let job = st.relay_setup_jobs.entry(target).or_insert_with(|| {
                    reserve_job(target, setup_account, setup_account_identifier.clone())
                });
                job.status = RelaySetupStatus::RefundAvailable;
                job.last_error = Some(message.clone());
                job.updated_at_ts = now_secs();
            });
            return auto_refund_pre_spend(historian, target, message, ledger, index).await;
        }
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedTerminal.into(),
            message,
        };
    }
    if balance < cfg.relay_setup_min_e8s {
        if balance > cfg.relay_setup_dust_e8s {
            state::with_root_and_relay_factory_state_mut(target, |st| {
                st.relay_setup_jobs.entry(target).or_insert_with(|| {
                    reserve_job(target, setup_account, setup_account_identifier.clone())
                });
            });
            set_job_status(target, RelaySetupStatus::BelowMinimum, None);
            return auto_refund_pre_spend(
                historian,
                target,
                "setup balance is below the minimum".to_string(),
                ledger,
                index,
            )
            .await;
        }
        return RelaySetupNotifyResult::BelowMinimum {
            minimum_e8s: cfg.relay_setup_min_e8s,
            current_balance_e8s: balance,
        };
    }
    if !has_resume_job {
        state::with_root_and_relay_factory_state_mut(target, |st| {
            st.relay_setup_jobs.entry(target).or_insert_with(|| {
                reserve_job(target, setup_account, setup_account_identifier.clone())
            });
        });
    }
    let required = required_setup_e8s(&cfg, fee);
    if balance < required {
        set_job_status(
            target,
            RelaySetupStatus::InsufficientForCurrentRate,
            Some("setup balance is below current relay setup requirement".to_string()),
        );
        return auto_refund_pre_spend(
            historian,
            target,
            "setup balance is below current relay setup requirement".to_string(),
            ledger,
            index,
        )
        .await;
    }
    if let Err(err) = blackhole.canister_status(target).await {
        state::with_root_and_relay_factory_state_mut(target, |st| {
            let mut job = st.relay_setup_jobs.remove(&target).unwrap_or_else(|| {
                reserve_job(target, setup_account, setup_account_identifier.clone())
            });
            job.status = RelaySetupStatus::RefundAvailable;
            job.last_error = Some(err.to_string());
            job.updated_at_ts = now_secs();
            st.relay_setup_jobs.insert(target, job);
        });
        return auto_refund_pre_spend(
            historian,
            target,
            "target is not observable through the configured blackhole canister".to_string(),
            ledger,
            index,
        )
        .await;
    }
    let indexed = match index_setup_payments(target, setup_account_identifier.clone(), index).await
    {
        Ok(indexed) => indexed,
        Err(err) => {
            set_job_status(target, RelaySetupStatus::FailedRetryable, Some(err.clone()));
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message: err,
            };
        }
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            merge_payments(job, indexed.payments);
            job.updated_at_ts = now_secs();
        }
    });
    let indexed_total = state::with_state(|st| {
        st.relay_setup_jobs
            .get(&target)
            .map(indexed_inbound_total_for_job)
            .unwrap_or(0)
    });
    if indexed_total.saturating_add(cfg.relay_setup_dust_e8s) < balance {
        set_job_status(
            target,
            RelaySetupStatus::IndexNotReady,
            Some(
                "setup account balance is visible on ledger but ICP index has not caught up"
                    .to_string(),
            ),
        );
        if indexed.hit_page_cap {
            return RelaySetupNotifyResult::Pending {
                status: RelaySetupPublicStatus::IndexNotReady,
            };
        }
        return RelaySetupNotifyResult::Pending {
            status: RelaySetupPublicStatus::IndexNotReady,
        };
    }
    if !cfg.relay_factory_enabled
        || approved_self_service_relay_wasm().is_none()
        || cfg.cmc_canister_id.is_none()
    {
        return auto_refund_pre_spend(
            historian,
            target,
            "relay factory is disabled, approved relay wasm is not embedded, or CMC canister is not configured"
                .to_string(),
            ledger,
            index,
        )
        .await;
    }

    let Some(conversion_e8s) = cycle_conversion_e8s(&cfg, fee, balance) else {
        set_job_status(
            target,
            RelaySetupStatus::InsufficientForCurrentRate,
            Some("setup balance cannot leave useful relay subaccount-1 seed".to_string()),
        );
        return auto_refund_pre_spend(
            historian,
            target,
            "setup balance cannot leave useful relay subaccount-1 seed".to_string(),
            ledger,
            index,
        )
        .await;
    };
    let cmc_id = cfg
        .cmc_canister_id
        .expect("CMC canister id must be configured before conversion");
    let mut cycle_record = state::with_root_and_relay_factory_state_mut(target, |st| {
        let job = st
            .relay_setup_jobs
            .get_mut(&target)
            .expect("funded setup job must exist before CMC conversion");
        job.status = RelaySetupStatus::ConvertingCycles;
        job.cycle_transfer.clone().unwrap_or_else(|| {
            let record = transfer_record(
                RelaySetupTransferKind::CmcConversion,
                Some(relay_setup_subaccount(target)),
                setup_account,
                cmc_deposit_account(cmc_id, historian),
                conversion_e8s,
                fee,
                Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
            );
            job.cycle_transfer = Some(record.clone());
            job.cycle_conversion_e8s = Some(conversion_e8s);
            job.updated_at_ts = now_secs();
            record
        })
    });
    let block_index = if let Some(block_index) = cycle_record.block_index {
        block_index
    } else if let Some(block_index) =
        match find_recorded_transfer_in_index(&cycle_record, index).await {
            Ok(block_index) => block_index,
            Err(err) => {
                set_job_failed_retryable(target, err.clone());
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::FailedRetryable.into(),
                    message: err,
                };
            }
        }
    {
        block_index
    } else {
        if pending_transfer_is_stale(&cycle_record) {
            return stale_transfer_manual_recovery(target, &cycle_record);
        }
        match classify_transfer_response(
            ledger
                .icrc1_transfer(record_to_transfer_arg(&cycle_record))
                .await,
        ) {
            Ok(block_index) => block_index,
            Err(Ok(err)) => {
                set_job_status(
                    target,
                    RelaySetupStatus::FailedRetryable,
                    Some(format!("{err:?}")),
                );
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::FailedRetryable.into(),
                    message: format!("CMC transfer failed: {err:?}"),
                };
            }
            Err(Err(err)) => {
                set_job_status(target, RelaySetupStatus::Ambiguous, Some(err.to_string()));
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::Ambiguous.into(),
                    message: err.to_string(),
                };
            }
        }
    };
    note_transfer_success(&mut cycle_record, block_index);
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = RelaySetupStatus::CycleTransferAccepted;
            job.phase = Some(RelaySetupPhase::CycleTransferAccepted);
            job.cycle_transfer = Some(cycle_record);
            job.cycle_conversion_e8s = Some(conversion_e8s);
            job.cycle_transfer_block_index = Some(block_index);
            job.updated_at_ts = now_secs();
        }
    });
    let minted = match cmc.notify_top_up(historian, block_index).await {
        Ok(cycles) => cycles,
        Err(err) => return handle_cmc_notify_error(target, err),
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = RelaySetupStatus::CycleNotifySucceeded;
            job.phase = Some(RelaySetupPhase::CycleNotifySucceeded);
            job.cycles_minted = Some(minted);
            job.updated_at_ts = now_secs();
        }
    });
    let relay_funding = balance
        .saturating_sub(conversion_e8s)
        .saturating_sub(fee.saturating_mul(2));
    create_and_activate_relay(
        target,
        relay_funding,
        fee,
        index,
        blackhole,
        &IcManagementClient,
        historian,
    )
    .await
}

enum RelayCodeInstallReconciliation {
    ExistingApprovedModule,
    EmptyCanister,
    ManualRecoveryRequired(RelaySetupNotifyResult),
}

async fn reconcile_relay_code_installed(
    target: Principal,
    relay_id: Principal,
    expected_wasm_hash: [u8; 32],
    management: &dyn ManagementClient,
) -> Result<RelayCodeInstallReconciliation, String> {
    let status = management
        .canister_status(&jupiter_ic_clients::management::CanisterStatusArgs {
            canister_id: relay_id,
        })
        .await
        .map_err(|err| format!("relay canister_status failed before install retry: {err}"))?;
    match status.module_hash.as_deref() {
        Some(hash) if hash == expected_wasm_hash.as_slice() => {
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.code_installed = true;
                    job.status = RelaySetupStatus::CodeInstalled;
                    job.phase = Some(RelaySetupPhase::RelayCodeInstalled);
                    job.updated_at_ts = now_secs();
                }
            });
            Ok(RelayCodeInstallReconciliation::ExistingApprovedModule)
        }
        Some(_) => Ok(RelayCodeInstallReconciliation::ManualRecoveryRequired(
            mark_manual_recovery_required(
                target,
                "relay canister already has an unexpected module hash".to_string(),
            ),
        )),
        None => Ok(RelayCodeInstallReconciliation::EmptyCanister),
    }
}

async fn create_and_activate_relay(
    target: Principal,
    relay_funding: u64,
    fee: u64,
    index: &dyn IndexClient,
    blackhole: &dyn BlackholeClient,
    management: &dyn ManagementClient,
    historian: Principal,
) -> RelaySetupNotifyResult {
    let cfg = state::with_state(|st| st.config.clone());
    let wasm = match approved_self_service_relay_wasm() {
        Some(wasm) => wasm.to_vec(),
        None => {
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedTerminal.into(),
                message: "approved relay wasm is not embedded".to_string(),
            }
        }
    };
    let (relay_id, code_installed, funding_already_recorded) = state::with_state(|st| {
        let job = st.relay_setup_jobs.get(&target);
        (
            job.and_then(|job| job.relay_canister_id),
            job.map(|job| job.code_installed).unwrap_or(false),
            job.map(|job| job.relay_funding_accepted).unwrap_or(false)
                || job.and_then(|job| job.relay_funding_block_index).is_some(),
        )
    });
    let relay_id = match relay_id {
        Some(relay_id) => relay_id,
        None => {
            let create_args = jupiter_ic_clients::management::CreateCanisterArgs {
                settings: Some(jupiter_ic_clients::management::CanisterSettings {
                    controllers: Some(vec![self_canister_id()]),
                    log_visibility: Some(jupiter_ic_clients::management::LogVisibility::Public),
                }),
            };
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.status = RelaySetupStatus::CreatingCanister;
                    job.relay_create_attempt = Some(RelayCreateAttempt {
                        target_canister_id: target,
                        created_at_ts: now_secs(),
                        initial_cycles: cfg.relay_initial_cycles,
                        relay_wasm_hash_hex: relay_wasm_hash_hex(),
                    });
                    job.updated_at_ts = now_secs();
                }
            });
            let relay_id = match management
                .create_canister(&create_args, cfg.relay_initial_cycles)
                .await
            {
                Ok(result) => result.canister_id,
                Err(ManagementClientError::Ambiguous(err)) => {
                    return mark_manual_recovery_required(
                        target,
                        format!(
                            "create_canister may have succeeded but relay_canister_id was not recorded: {err}"
                        ),
                    );
                }
                Err(ManagementClientError::Failed(err)) => {
                    set_job_status(target, RelaySetupStatus::FailedRetryable, Some(err.clone()));
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::FailedRetryable.into(),
                        message: format!("create_canister failed: {err}"),
                    };
                }
            };
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.status = RelaySetupStatus::CanisterCreated;
                    job.phase = Some(RelaySetupPhase::RelayCanisterCreated);
                    job.relay_canister_id = Some(relay_id);
                    job.relay_initial_cycles = Some(cfg.relay_initial_cycles);
                    job.updated_at_ts = now_secs();
                }
            });
            relay_id
        }
    };
    if !code_installed {
        let expected_wasm_hash =
            approved_relay_wasm_hash().expect("approved relay wasm exists when installing");
        match reconcile_relay_code_installed(target, relay_id, expected_wasm_hash, management).await
        {
            Ok(RelayCodeInstallReconciliation::ExistingApprovedModule) => {}
            Ok(RelayCodeInstallReconciliation::EmptyCanister) => {
                let relay_args = jupiter_relay_init_arg(&cfg, target);
                set_job_status(target, RelaySetupStatus::InstallingCode, None);
                if let Err(err) = management
                    .install_code(&jupiter_ic_clients::management::InstallCodeArgs {
                        mode: jupiter_ic_clients::management::InstallMode::Install,
                        canister_id: relay_id,
                        wasm_module: wasm,
                        arg: relay_args,
                    })
                    .await
                {
                    set_job_status(
                        target,
                        RelaySetupStatus::FailedRetryable,
                        Some(err.to_string()),
                    );
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::FailedRetryable.into(),
                        message: format!("install_code failed: {err}"),
                    };
                }
                state::with_root_and_relay_factory_state_mut(target, |st| {
                    if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                        job.code_installed = true;
                        job.phase = Some(RelaySetupPhase::RelayCodeInstalled);
                        job.updated_at_ts = now_secs();
                    }
                });
            }
            Ok(RelayCodeInstallReconciliation::ManualRecoveryRequired(result)) => return result,
            Err(err) => {
                set_job_status(target, RelaySetupStatus::FailedRetryable, Some(err.clone()));
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::FailedRetryable.into(),
                    message: err,
                };
            }
        }
    }
    set_job_status(target, RelaySetupStatus::CodeInstalled, None);
    let ledger = jupiter_ic_clients::ledger::IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let pending_funding_transfer = state::with_state(|st| {
        st.relay_setup_jobs
            .get(&target)
            .and_then(|job| job.relay_funding_transfer.as_ref())
            .map(|record| !record.completed)
            .unwrap_or(false)
    });
    if pending_funding_transfer
        || (!funding_already_recorded && relay_funding > cfg.relay_setup_dust_e8s)
    {
        let setup_account = setup_account_for(historian, target);
        let mut record = state::with_root_and_relay_factory_state_mut(target, |st| {
            let job = st
                .relay_setup_jobs
                .get_mut(&target)
                .expect("relay funding requires setup job");
            job.status = RelaySetupStatus::FundingRelaySubaccountOne;
            job.relay_funding_transfer.clone().unwrap_or_else(|| {
                let record = transfer_record(
                    RelaySetupTransferKind::RelayFunding,
                    Some(relay_setup_subaccount(target)),
                    setup_account,
                    relay_subaccount_one(relay_id),
                    relay_funding,
                    fee,
                    None,
                );
                job.relay_funding_transfer = Some(record.clone());
                job.relay_funding_e8s = Some(relay_funding);
                job.updated_at_ts = now_secs();
                record
            })
        });
        let block_index = if let Some(block_index) = record.block_index {
            block_index
        } else if let Some(block_index) =
            match find_recorded_transfer_in_index(&record, index).await {
                Ok(block_index) => block_index,
                Err(err) => {
                    set_job_failed_retryable(target, err.clone());
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::FailedRetryable.into(),
                        message: err,
                    };
                }
            }
        {
            block_index
        } else {
            match classify_transfer_response(
                crate::clients::LedgerClient::icrc1_transfer(
                    &ledger,
                    record_to_transfer_arg(&record),
                )
                .await,
            ) {
                Ok(block_index) => block_index,
                Err(Ok(err)) => {
                    set_job_status(
                        target,
                        RelaySetupStatus::FailedRetryable,
                        Some(format!("{err:?}")),
                    );
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::FailedRetryable.into(),
                        message: format!("relay funding failed: {err:?}"),
                    };
                }
                Err(Err(err)) => {
                    set_job_status(target, RelaySetupStatus::Ambiguous, Some(err.to_string()));
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::Ambiguous.into(),
                        message: err.to_string(),
                    };
                }
            }
        };
        note_transfer_success(&mut record, block_index);
        state::with_root_and_relay_factory_state_mut(target, |st| {
            if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                let funding_amount = record.amount_e8s;
                job.relay_funding_transfer = Some(record);
                job.relay_funding_e8s = Some(funding_amount);
                job.relay_funding_block_index = Some(block_index);
                job.relay_funding_accepted = true;
                job.phase = Some(RelaySetupPhase::RelayFundingAccepted);
                job.updated_at_ts = now_secs();
            }
        });
    }
    set_job_status(target, RelaySetupStatus::Blackholing, None);
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.blackhole_update_attempted = true;
            job.phase = Some(RelaySetupPhase::BlackholeUpdateAttempted);
            job.updated_at_ts = now_secs();
        }
    });
    if let Err(err) = management
        .update_settings(&jupiter_ic_clients::management::UpdateSettingsArgs {
            canister_id: relay_id,
            settings: jupiter_ic_clients::management::CanisterSettings {
                controllers: Some(vec![cfg.blackhole_canister_id]),
                log_visibility: Some(jupiter_ic_clients::management::LogVisibility::Public),
            },
        })
        .await
    {
        if let Ok(status) = blackhole.canister_status(relay_id).await {
            if status.settings.controllers == vec![cfg.blackhole_canister_id] {
                state::with_root_and_relay_factory_state_mut(target, |st| {
                    if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                        job.blackhole_confirmed = true;
                        job.updated_at_ts = now_secs();
                    }
                });
            } else {
                set_job_status(
                    target,
                    RelaySetupStatus::FailedRetryable,
                    Some(err.to_string()),
                );
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::FailedRetryable.into(),
                    message: format!("blackhole update_settings failed: {err}"),
                };
            }
        } else {
            set_job_status(
                target,
                RelaySetupStatus::FailedRetryable,
                Some(err.to_string()),
            );
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable.into(),
                message: format!("blackhole update_settings failed: {err}"),
            };
        }
    } else {
        state::with_root_and_relay_factory_state_mut(target, |st| {
            if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                job.blackhole_confirmed = true;
                job.updated_at_ts = now_secs();
            }
        });
    }
    let entry = RelayRegistryEntry {
        relay_canister_id: relay_id,
        target_canister_id: target,
        kind: RelayRegistryKind::SelfService,
        status: RelayRegistryStatus::Active,
        setup_account: Some(setup_account_for(historian, target)),
        setup_account_identifier: Some(account_identifier_text_for_account(&setup_account_for(
            historian, target,
        ))),
        setup_amount_e8s: state::with_state(|st| {
            st.relay_setup_jobs
                .get(&target)
                .map(|job| job.setup_amount_seen_e8s)
        }),
        setup_tx_ids: state::with_state(|st| {
            st.relay_setup_jobs
                .get(&target)
                .map(|job| job.setup_tx_ids.clone())
                .unwrap_or_default()
        }),
        relay_wasm_hash_hex: relay_wasm_hash_hex(),
        final_controllers: Some(vec![cfg.blackhole_canister_id]),
        log_visibility_public: Some(true),
        created_at_ts: Some(now_secs()),
        activated_at_ts: Some(now_secs()),
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = RelaySetupStatus::Active;
            job.phase = Some(RelaySetupPhase::Active);
            job.relay_canister_id = Some(relay_id);
            job.setup_amount_processed_e8s = job.setup_amount_seen_e8s;
            job.updated_at_ts = now_secs();
        }
        st.relay_registry_by_target.insert(target, entry.clone());
    });
    RelaySetupNotifyResult::Active {
        relay: entry.into(),
    }
}

fn jupiter_relay_init_arg(cfg: &Config, target: Principal) -> Vec<u8> {
    #[derive(candid::CandidType)]
    struct SurplusNeuronRecipient {
        neuron_id: u64,
        memo: Vec<u8>,
    }
    #[derive(candid::CandidType)]
    struct InitArgs {
        managed_canisters: Vec<Principal>,
        ledger_canister_id: Option<Principal>,
        cmc_canister_id: Option<Principal>,
        governance_canister_id: Option<Principal>,
        blackhole_canister_id: Option<Principal>,
        main_interval_seconds: Option<u64>,
        max_transfers_per_tick: Option<u32>,
        surplus_canister_recipients: Option<Vec<()>>,
        surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
    }
    let args = InitArgs {
        managed_canisters: vec![target],
        ledger_canister_id: Some(cfg.ledger_canister_id),
        cmc_canister_id: cfg.cmc_canister_id,
        governance_canister_id: Some(jupiter_ic_clients::constants::nns_governance_id()),
        blackhole_canister_id: Some(cfg.blackhole_canister_id),
        main_interval_seconds: Some(cfg.self_service_relay_interval_seconds),
        max_transfers_per_tick: cfg.self_service_relay_max_transfers_per_tick,
        surplus_canister_recipients: None,
        surplus_neuron_recipients: vec![SurplusNeuronRecipient {
            neuron_id: cfg.io_surplus_neuron_id,
            memo: Vec::new(),
        }],
    };
    Encode!(&args).expect("relay init args should encode")
}

pub(crate) async fn notify_relay_setup(target: Principal) -> RelaySetupNotifyResult {
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = jupiter_ic_clients::ledger::IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = jupiter_ic_clients::index::IcpIndexCanister::new(cfg.index_canister_id);
    let blackhole = clients::blackhole::BlackholeCanister::new(cfg.blackhole_canister_id);
    let cmc = cfg.cmc_canister_id.map(CmcCanister::new);
    let missing_cmc = MissingCmcClient;
    match cmc.as_ref() {
        Some(cmc) => {
            notify_relay_setup_with_clients(target, &ledger, &index, &blackhole, cmc).await
        }
        None => {
            notify_relay_setup_with_clients(target, &ledger, &index, &blackhole, &missing_cmc).await
        }
    }
}

struct MissingCmcClient;

#[async_trait::async_trait]
impl CmcClient for MissingCmcClient {
    async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
        Err(ClientError::Call(
            "CMC canister is not configured".to_string(),
        ))
    }

    async fn notify_top_up(
        &self,
        _canister_id: Principal,
        _block_index: u64,
    ) -> Result<u128, NotifyTopUpError> {
        Err(NotifyTopUpError::Terminal(
            jupiter_ic_clients::cmc::NotifyTerminalError::InvalidTransaction(
                "CMC canister is not configured".to_string(),
            ),
        ))
    }
}

async fn request_relay_setup_refund_with_clients_for_historian(
    historian: Principal,
    target: Principal,
    ledger: &dyn LedgerClient,
    index: &dyn IndexClient,
) -> RelaySetupRefundResult {
    let (job, status, cooldown, last_attempt, setup_account, setup_account_identifier) =
        state::with_state(|st| {
            let job = st.relay_setup_jobs.get(&target);
            (
                job.cloned(),
                job.map(|job| job.status.clone()),
                st.config.relay_setup_refund_cooldown_seconds,
                job.and_then(|job| job.last_refund_attempt_ts),
                setup_account_for(historian, target),
                job.map(|job| job.setup_account_identifier.clone())
                    .unwrap_or_else(|| {
                        account_identifier_text_for_account(&setup_account_for(historian, target))
                    }),
            )
        });
    let Some(job) = job else {
        return RelaySetupRefundResult::NotEligible { status };
    };
    if !refund_eligible_status(&job) {
        return RelaySetupRefundResult::NotEligible { status };
    }
    let now = now_secs();
    if let Some(last) = last_attempt {
        let elapsed = now.saturating_sub(last);
        if elapsed < cooldown {
            return RelaySetupRefundResult::Cooldown {
                retry_after_seconds: cooldown.saturating_sub(elapsed),
            };
        }
    }
    let fee = match ledger.fee_e8s().await {
        Ok(fee) => fee,
        Err(err) => {
            return RelaySetupRefundResult::Failed {
                message: err.to_string(),
            }
        }
    };
    let balance = match ledger.balance_of_e8s(setup_account).await {
        Ok(balance) => balance,
        Err(err) => {
            return RelaySetupRefundResult::Failed {
                message: err.to_string(),
            }
        }
    };
    let indexed = match index_setup_payments(target, setup_account_identifier, index).await {
        Ok(indexed) => indexed,
        Err(err) => return RelaySetupRefundResult::Failed { message: err },
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            merge_payments(job, indexed.payments);
            job.status = RelaySetupStatus::Refunding;
            job.last_refund_attempt_ts = Some(now);
            job.refund_attempt_count = job.refund_attempt_count.saturating_add(1);
            job.updated_at_ts = now;
        }
    });
    let indexed_total = state::with_state(|st| {
        st.relay_setup_jobs
            .get(&target)
            .map(indexed_inbound_total_for_job)
            .unwrap_or(0)
    });
    if indexed.hit_page_cap && indexed_total.saturating_add(1) < balance {
        set_job_status(
            target,
            RelaySetupStatus::IndexNotReady,
            Some(
                "setup payment indexing reached the page cap before explaining the ledger balance"
                    .to_string(),
            ),
        );
        return RelaySetupRefundResult::Failed {
            message:
                "setup payment indexing reached the page cap before explaining the ledger balance"
                    .to_string(),
        };
    }
    let grouped = state::with_state(|st| {
        let mut grouped = BTreeMap::<String, (u64, Vec<u64>)>::new();
        if let Some(job) = st.relay_setup_jobs.get(&target) {
            for payment in job
                .payments
                .iter()
                .filter(|payment| !payment.processed && !payment.refunded)
            {
                let entry = grouped
                    .entry(payment.from_account_identifier.clone())
                    .or_default();
                entry.0 = entry.0.saturating_add(payment.amount_e8s);
                entry.1.push(payment.tx_id);
            }
        }
        grouped
    });
    let mut blocks = Vec::new();
    let mut refundable_balance = balance;
    for (account_identifier, (amount, tx_ids)) in grouped {
        if refundable_balance <= fee || amount <= fee {
            continue;
        }
        let gross = amount.min(refundable_balance);
        if gross <= fee {
            continue;
        }
        let refund_amount = gross.saturating_sub(fee);
        let created_at_time = state::with_root_and_relay_factory_state_mut(target, |st| {
            if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                if let Some(record) = job.refund_transfers.iter().find(|record| {
                    !record.completed
                        && record.kind == RelaySetupTransferKind::Refund
                        && record.from_subaccount == setup_account.subaccount
                        && record.to_account_identifier == account_identifier
                        && record.amount_e8s == refund_amount
                        && record.fee_e8s == fee
                }) {
                    return record.created_at_time_nanos;
                }
                let mut record = transfer_record(
                    RelaySetupTransferKind::Refund,
                    setup_account.subaccount,
                    setup_account,
                    Account {
                        owner: Principal::anonymous(),
                        subaccount: None,
                    },
                    refund_amount,
                    fee,
                    Some(REFUND_MEMO.to_le_bytes().to_vec()),
                );
                record.to_account_identifier = account_identifier.clone();
                let created_at_time = record.created_at_time_nanos;
                job.refund_transfers.push(record);
                job.updated_at_ts = now;
                return created_at_time;
            }
            now_nanos()
        });
        let stale = state::with_state(|st| {
            st.relay_setup_jobs
                .get(&target)
                .and_then(|job| {
                    job.refund_transfers.iter().find(|record| {
                        !record.completed
                            && record.kind == RelaySetupTransferKind::Refund
                            && record.to_account_identifier == account_identifier
                            && record.amount_e8s == refund_amount
                            && record.created_at_time_nanos == created_at_time
                    })
                })
                .map(pending_transfer_is_stale)
                .unwrap_or(false)
        });
        if stale {
            set_job_status(
                target,
                RelaySetupStatus::ManualRecoveryRequired,
                Some("pending refund transfer is older than the ledger duplicate window and was not found in the index".to_string()),
            );
            return RelaySetupRefundResult::Failed {
                message: "pending refund transfer is older than the ledger duplicate window and was not found in the index".to_string(),
            };
        }
        let result = ledger
            .legacy_transfer_to_account_identifier(
                setup_account.subaccount,
                account_identifier.clone(),
                refund_amount,
                fee,
                REFUND_MEMO,
                Some(created_at_time),
            )
            .await;
        match result {
            Ok(Ok(block))
            | Ok(Err(LegacyTransferError::TxDuplicate {
                duplicate_of: block,
            })) => {
                refundable_balance = refundable_balance.saturating_sub(gross);
                blocks.push(block);
                state::with_root_and_relay_factory_state_mut(target, |st| {
                    if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                        for payment in &mut job.payments {
                            if tx_ids.contains(&payment.tx_id) {
                                payment.refunded = true;
                            }
                        }
                        job.refund_blocks.push(block);
                        if let Some(record) = job.refund_transfers.iter_mut().rev().find(|record| {
                            record.to_account_identifier == account_identifier
                                && record.amount_e8s == refund_amount
                                && record.created_at_time_nanos == created_at_time
                        }) {
                            note_transfer_success(record, block);
                        }
                        job.updated_at_ts = now;
                    }
                });
            }
            Ok(Err(err)) => {
                set_job_status(
                    target,
                    RelaySetupStatus::RefundAvailable,
                    Some(format!("{err:?}")),
                );
                return RelaySetupRefundResult::Failed {
                    message: format!("{err:?}"),
                };
            }
            Err(err) => {
                set_job_status(
                    target,
                    RelaySetupStatus::RefundAvailable,
                    Some(err.to_string()),
                );
                return RelaySetupRefundResult::Failed {
                    message: err.to_string(),
                };
            }
        }
    }
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = if blocks.is_empty() {
                RelaySetupStatus::RefundAvailable
            } else {
                RelaySetupStatus::Refunded
            };
            job.updated_at_ts = now;
        }
    });
    if blocks.is_empty() {
        RelaySetupRefundResult::NoRefundableAmount
    } else {
        RelaySetupRefundResult::Refunded { blocks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::blackhole::{BlackholeCanisterStatus, BlackholeSettings};
    use crate::clients::index::{
        GetAccountIdentifierTransactionsResponse, IndexTimeStamp, IndexTransaction,
        IndexTransactionWithId, Tokens,
    };
    use crate::clients::{ClientError, IcpXdrConversionRate};
    use futures::executor::block_on;
    use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferError};
    use std::sync::{Arc, Mutex};

    fn job_with_status(status: RelaySetupStatus) -> RelaySetupJob {
        RelaySetupJob {
            target_canister_id: Principal::from_slice(&[1]),
            setup_account: Account {
                owner: Principal::from_slice(&[2]),
                subaccount: Some([3; 32]),
            },
            setup_account_identifier: "setup-account".to_string(),
            status,
            relay_canister_id: None,
            last_indexed_setup_tx_id: None,
            setup_tx_ids: Vec::new(),
            setup_amount_seen_e8s: 0,
            setup_amount_processed_e8s: 0,
            payments: Vec::new(),
            cycle_conversion_e8s: None,
            cycle_transfer_block_index: None,
            cycles_minted: None,
            relay_initial_cycles: None,
            relay_funding_e8s: None,
            relay_funding_block_index: None,
            phase: Some(RelaySetupPhase::PreSpend),
            cycle_transfer: None,
            relay_funding_transfer: None,
            existing_relay_sweep_transfer: None,
            refund_transfers: Vec::new(),
            relay_create_attempt: None,
            code_installed: false,
            relay_funding_accepted: false,
            blackhole_update_attempted: false,
            blackhole_confirmed: false,
            refund_attempt_count: 0,
            last_refund_attempt_ts: None,
            refund_blocks: Vec::new(),
            created_at_ts: 0,
            updated_at_ts: 0,
            last_error: None,
        }
    }

    fn config() -> Config {
        Config {
            staking_account: Account {
                owner: Principal::from_slice(&[1]),
                subaccount: None,
            },
            output_source_account: Account {
                owner: Principal::from_slice(&[11]),
                subaccount: None,
            },
            output_account: Account {
                owner: Principal::from_slice(&[12]),
                subaccount: Some([3; 32]),
            },
            rewards_account: Account {
                owner: Principal::from_slice(&[13]),
                subaccount: None,
            },
            ledger_canister_id: Principal::from_slice(&[2]),
            index_canister_id: Principal::from_slice(&[3]),
            cmc_canister_id: Some(Principal::from_slice(&[4])),
            faucet_canister_id: Some(Principal::from_slice(&[5])),
            blackhole_canister_id: Principal::from_slice(&[6]),
            sns_wasm_canister_id: Principal::from_slice(&[7]),
            xrc_canister_id: Principal::from_slice(&[8]),
            enable_sns_tracking: true,
            scan_interval_seconds: 60,
            cycles_interval_seconds: 120,
            min_tx_e8s: 100_000_000,
            max_cycles_entries_per_canister: 100,
            max_commitment_entries_per_canister: 100,
            max_index_pages_per_tick: 10,
            max_canisters_per_cycles_tick: 10,
            relay_factory_enabled: true,
            relay_setup_min_e8s: 200_000_000,
            relay_setup_dust_e8s: 10_000,
            relay_setup_refund_cooldown_seconds: 0,
            relay_initial_cycles: 1_000_000_000_000,
            relay_cycle_safety_margin_e8s: 5_000_000,
            relay_min_subaccount_one_seed_e8s: 100_020_000,
            self_service_relay_interval_seconds: 3600,
            self_service_relay_max_transfers_per_tick: Some(10),
            io_surplus_neuron_id: crate::DEFAULT_IO_SURPLUS_NEURON_ID,
            canonical_relay_canister_id: Some(crate::mainnet_relay_id()),
            canonical_relay_targets: crate::mainnet_canonical_relay_targets(),
        }
    }

    fn install_state_with_job(target: Principal, job: RelaySetupJob) {
        let mut st = State::new(config(), 0);
        st.relay_setup_jobs.insert(target, job);
        state::set_state(st);
    }

    #[derive(Clone)]
    struct FakeLedger {
        fee: Result<u64, String>,
        balance: Result<u64, String>,
        transfer_results: Arc<Mutex<Vec<Result<Result<BlockIndex, TransferError>, String>>>>,
        transfers: Arc<Mutex<Vec<TransferArg>>>,
        legacy_results:
            Arc<Mutex<Vec<Result<jupiter_ic_clients::ledger::LegacyTransferResult, String>>>>,
        legacy_calls: Arc<Mutex<Vec<(String, u64, Option<u64>)>>>,
    }

    impl FakeLedger {
        fn healthy(balance: u64) -> Self {
            Self {
                fee: Ok(10_000),
                balance: Ok(balance),
                transfer_results: Arc::new(Mutex::new(Vec::new())),
                transfers: Arc::new(Mutex::new(Vec::new())),
                legacy_results: Arc::new(Mutex::new(Vec::new())),
                legacy_calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl LedgerClient for FakeLedger {
        async fn fee_e8s(&self) -> Result<u64, ClientError> {
            self.fee.clone().map_err(ClientError::Call)
        }

        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, ClientError> {
            self.balance.clone().map_err(ClientError::Call)
        }

        async fn icrc1_transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
            self.transfers.lock().unwrap().push(arg);
            let mut results = self.transfer_results.lock().unwrap();
            if results.is_empty() {
                return Ok(Ok(candid::Nat::from(77u64)));
            }
            results.remove(0).map_err(ClientError::Call)
        }

        async fn legacy_transfer_to_account_identifier(
            &self,
            _from_subaccount: Option<[u8; 32]>,
            to_account_identifier_hex: String,
            amount_e8s: u64,
            _fee_e8s: u64,
            _memo: u64,
            created_at_time_nanos: Option<u64>,
        ) -> Result<crate::clients::LegacyTransferResult, ClientError> {
            self.legacy_calls.lock().unwrap().push((
                to_account_identifier_hex,
                amount_e8s,
                created_at_time_nanos,
            ));
            let result = self
                .legacy_results
                .lock()
                .unwrap()
                .remove(0)
                .map_err(ClientError::Call)?;
            Ok(result)
        }
    }

    struct FakeIndex {
        response: GetAccountIdentifierTransactionsResponse,
        pages: Arc<Mutex<Vec<GetAccountIdentifierTransactionsResponse>>>,
    }

    #[async_trait::async_trait]
    impl IndexClient for FakeIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, ClientError> {
            let mut pages = self.pages.lock().unwrap();
            if !pages.is_empty() {
                return Ok(pages.remove(0));
            }
            Ok(self.response.clone())
        }
    }

    struct FakeBlackhole;

    #[async_trait::async_trait]
    impl BlackholeClient for FakeBlackhole {
        async fn canister_status(
            &self,
            _canister_id: Principal,
        ) -> Result<BlackholeCanisterStatus, ClientError> {
            Ok(BlackholeCanisterStatus {
                cycles: candid::Nat::from(1u64),
                settings: BlackholeSettings {
                    controllers: Vec::new(),
                },
                memory_size: None,
                memory_metrics: None,
            })
        }
    }

    struct FakeCmc {
        notify_results: Arc<Mutex<Vec<Result<u128, NotifyTopUpError>>>>,
        notify_calls: Arc<Mutex<Vec<u64>>>,
    }

    impl FakeCmc {
        fn healthy() -> Self {
            Self {
                notify_results: Arc::new(Mutex::new(Vec::new())),
                notify_calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl CmcClient for FakeCmc {
        async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
            Err(ClientError::Call("not used".to_string()))
        }

        async fn notify_top_up(
            &self,
            _canister_id: Principal,
            block_index: u64,
        ) -> Result<u128, NotifyTopUpError> {
            self.notify_calls.lock().unwrap().push(block_index);
            let mut results = self.notify_results.lock().unwrap();
            if results.is_empty() {
                return Ok(1_000_000_000_000);
            }
            results.remove(0)
        }
    }

    #[derive(Clone)]
    struct FakeManagement {
        create_results: Arc<Mutex<Vec<Result<Principal, ManagementClientError>>>>,
        create_calls: Arc<Mutex<u32>>,
        install_calls: Arc<Mutex<u32>>,
        update_calls: Arc<Mutex<u32>>,
        status_hashes: Arc<Mutex<Vec<Option<Vec<u8>>>>>,
    }

    impl FakeManagement {
        fn healthy(relay_id: Principal, module_hash: Option<Vec<u8>>) -> Self {
            Self {
                create_results: Arc::new(Mutex::new(vec![Ok(relay_id)])),
                create_calls: Arc::new(Mutex::new(0)),
                install_calls: Arc::new(Mutex::new(0)),
                update_calls: Arc::new(Mutex::new(0)),
                status_hashes: Arc::new(Mutex::new(vec![module_hash])),
            }
        }
    }

    #[async_trait::async_trait]
    impl ManagementClient for FakeManagement {
        async fn create_canister(
            &self,
            _arg: &jupiter_ic_clients::management::CreateCanisterArgs,
            _cycles_to_attach: u128,
        ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, ManagementClientError>
        {
            *self.create_calls.lock().unwrap() += 1;
            let result = self.create_results.lock().unwrap().remove(0)?;
            Ok(jupiter_ic_clients::management::CreateCanisterResult {
                canister_id: result,
            })
        }

        async fn install_code(
            &self,
            _arg: &jupiter_ic_clients::management::InstallCodeArgs,
        ) -> Result<(), ManagementClientError> {
            *self.install_calls.lock().unwrap() += 1;
            Ok(())
        }

        async fn canister_status(
            &self,
            _arg: &jupiter_ic_clients::management::CanisterStatusArgs,
        ) -> Result<jupiter_ic_clients::management::CanisterStatusResult, ManagementClientError>
        {
            let mut hashes = self.status_hashes.lock().unwrap();
            let module_hash = if hashes.is_empty() {
                None
            } else {
                hashes.remove(0)
            };
            Ok(jupiter_ic_clients::management::CanisterStatusResult {
                module_hash,
                settings: jupiter_ic_clients::management::CanisterSettings::default(),
            })
        }

        async fn update_settings(
            &self,
            _arg: &jupiter_ic_clients::management::UpdateSettingsArgs,
        ) -> Result<(), ManagementClientError> {
            *self.update_calls.lock().unwrap() += 1;
            Ok(())
        }
    }

    fn payment(tx_id: u64, from: &str, amount_e8s: u64) -> RelaySetupPayment {
        RelaySetupPayment {
            target_canister_id: Principal::from_slice(&[1]),
            tx_id,
            from_account_identifier: from.to_string(),
            amount_e8s,
            timestamp_nanos: None,
            processed: false,
            refunded: false,
        }
    }

    fn index_transfer(tx_id: u64, from: &str, to: &str, amount_e8s: u64) -> IndexTransactionWithId {
        index_transfer_with_created_at(tx_id, from, to, amount_e8s, tx_id)
    }

    fn index_transfer_with_created_at(
        tx_id: u64,
        from: &str,
        to: &str,
        amount_e8s: u64,
        created_at_time_nanos: u64,
    ) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id: tx_id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    from: from.to_string(),
                    to: to.to_string(),
                    amount: Tokens::new(amount_e8s),
                    fee: Tokens::new(10_000),
                    spender: None,
                },
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: created_at_time_nanos,
                }),
                timestamp: None,
            },
        }
    }

    fn active_relay(target: Principal, relay_id: Principal) -> RelayRegistryEntry {
        RelayRegistryEntry {
            relay_canister_id: relay_id,
            target_canister_id: target,
            kind: RelayRegistryKind::SelfService,
            status: RelayRegistryStatus::Active,
            setup_account: None,
            setup_account_identifier: None,
            setup_amount_e8s: None,
            setup_tx_ids: Vec::new(),
            relay_wasm_hash_hex: None,
            final_controllers: None,
            log_visibility_public: None,
            created_at_ts: None,
            activated_at_ts: None,
        }
    }

    #[test]
    fn in_flight_job_covers_creation_and_refund_phases_only() {
        for status in [
            RelaySetupStatus::Pending,
            RelaySetupStatus::ConvertingCycles,
            RelaySetupStatus::CycleTransferAccepted,
            RelaySetupStatus::CycleNotifySucceeded,
            RelaySetupStatus::CreatingCanister,
            RelaySetupStatus::CanisterCreated,
            RelaySetupStatus::InstallingCode,
            RelaySetupStatus::CodeInstalled,
            RelaySetupStatus::SettingPublicLogs,
            RelaySetupStatus::FundingRelaySubaccountOne,
            RelaySetupStatus::Blackholing,
            RelaySetupStatus::Refunding,
        ] {
            assert!(in_flight_job(&job_with_status(status)));
        }

        for status in [
            RelaySetupStatus::BelowMinimum,
            RelaySetupStatus::InsufficientForCurrentRate,
            RelaySetupStatus::TargetNotObservable,
            RelaySetupStatus::Active,
            RelaySetupStatus::RefundAvailable,
            RelaySetupStatus::Refunded,
            RelaySetupStatus::IndexNotReady,
            RelaySetupStatus::FailedRetryable,
            RelaySetupStatus::FailedTerminal,
            RelaySetupStatus::Ambiguous,
            RelaySetupStatus::ManualRecoveryRequired,
        ] {
            assert!(!in_flight_job(&job_with_status(status)));
        }
    }

    #[test]
    fn resume_point_derives_from_durable_fields() {
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::PreSpend
        );
        assert!(!resumable_job(&job));

        job.cycle_transfer_block_index = Some(42);
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::NotifyCycleTopUp { block_index: 42 }
        );
        assert!(resumable_job(&job));

        job.cycles_minted = Some(1_000);
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::CreateRelayCanister
        );

        let relay_id = Principal::from_slice(&[9]);
        job.relay_canister_id = Some(relay_id);
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::InstallRelayCode { relay_id }
        );

        job.status = RelaySetupStatus::CodeInstalled;
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::FundRelaySubaccountOne { relay_id }
        );

        job.relay_funding_block_index = Some(7);
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::RegisterActive { relay_id }
        );

        job.status = RelaySetupStatus::Blackholing;
        assert_eq!(
            relay_setup_resume_point(&job),
            RelaySetupResumePoint::BlackholeRelay { relay_id }
        );
    }

    #[test]
    fn refund_eligibility_requires_pre_spend_fields() {
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        assert!(refund_eligible_status(&job));

        job.cycle_transfer_block_index = Some(1);
        assert!(!refund_eligible_status(&job));

        job.cycle_transfer_block_index = None;
        job.relay_canister_id = Some(Principal::from_slice(&[9]));
        assert!(!refund_eligible_status(&job));

        job = job_with_status(RelaySetupStatus::Active);
        assert!(!refund_eligible_status(&job));
    }

    #[test]
    fn refund_eligibility_rejects_durable_irreversible_transfer_records() {
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        job.cycle_transfer = Some(transfer_record(
            RelaySetupTransferKind::CmcConversion,
            Some([1; 32]),
            Account {
                owner: Principal::from_slice(&[2]),
                subaccount: Some([1; 32]),
            },
            Account {
                owner: Principal::from_slice(&[3]),
                subaccount: None,
            },
            100_000,
            10_000,
            None,
        ));
        assert!(!refund_eligible_status(&job));

        job.cycle_transfer = None;
        job.relay_funding_transfer = Some(transfer_record(
            RelaySetupTransferKind::RelayFunding,
            Some([1; 32]),
            Account {
                owner: Principal::from_slice(&[2]),
                subaccount: Some([1; 32]),
            },
            Account {
                owner: Principal::from_slice(&[4]),
                subaccount: Some([0; 32]),
            },
            100_000,
            10_000,
            None,
        ));
        assert!(!refund_eligible_status(&job));
    }

    #[test]
    fn indexed_inbound_total_ignores_refunded_payments() {
        let mut job = job_with_status(RelaySetupStatus::BelowMinimum);
        job.payments = vec![
            payment(1, "a", 100),
            RelaySetupPayment {
                refunded: true,
                ..payment(2, "b", 200)
            },
        ];
        assert_eq!(indexed_inbound_total_for_job(&job), 100);
    }

    #[test]
    fn setup_requirement_preserves_relay_seed_under_default_policy() {
        let cfg = config();
        let fee = 10_000;
        let required = required_setup_e8s(&cfg, fee);
        let cycle_conversion = required
            .saturating_sub(cfg.relay_min_subaccount_one_seed_e8s)
            .saturating_sub(fee);

        assert_eq!(required, 200_000_000);
        assert!(required >= cfg.relay_setup_min_e8s);
        assert!(cycle_conversion >= cfg.relay_cycle_safety_margin_e8s + fee);
        assert!(
            required
                .saturating_sub(cycle_conversion)
                .saturating_sub(fee)
                >= cfg.relay_min_subaccount_one_seed_e8s
        );
    }

    #[test]
    fn refund_persists_successful_payment_before_later_failure() {
        let target = Principal::from_slice(&[21]);
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        job.target_canister_id = target;
        job.setup_account_identifier = "setup".to_string();
        job.payments = vec![
            payment(1, "source-a", 100_000),
            payment(2, "source-b", 100_000),
        ];
        install_state_with_job(target, job);
        let ledger = FakeLedger {
            legacy_results: Arc::new(Mutex::new(vec![
                Ok(Ok(11)),
                Err("temporary transfer failure".to_string()),
            ])),
            ..FakeLedger::healthy(200_000)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 200_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(result, RelaySetupRefundResult::Failed { .. }));
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert!(job.payments[0].refunded);
        assert!(!job.payments[1].refunded);
        assert_eq!(job.refund_blocks, vec![11]);

        ledger.legacy_results.lock().unwrap().push(Ok(Ok(12)));
        let result = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(result, RelaySetupRefundResult::Refunded { .. }));
        let calls = ledger.legacy_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "source-a");
        assert_eq!(calls[1].0, "source-b");
        assert_eq!(calls[2].0, "source-b");
    }

    #[test]
    fn automatic_refund_reuses_incomplete_record_and_duplicate_success() {
        let target = Principal::from_slice(&[31]);
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        job.target_canister_id = target;
        job.setup_account_identifier = "setup".to_string();
        job.payments = vec![payment(1, "source-a", 100_000)];
        install_state_with_job(target, job);
        let ledger = FakeLedger {
            legacy_results: Arc::new(Mutex::new(vec![
                Err("reply lost after ledger accepted refund".to_string()),
                Ok(Err(LegacyTransferError::TxDuplicate { duplicate_of: 44 })),
            ])),
            ..FakeLedger::healthy(100_000)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 100_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let first = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(first, RelaySetupRefundResult::Failed { .. }));
        let first_call = ledger.legacy_calls.lock().unwrap()[0].clone();
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.refund_transfers.len(), 1);
        assert!(!job.refund_transfers[0].completed);

        let second = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(
            second,
            RelaySetupRefundResult::Refunded { ref blocks } if blocks == &vec![44]
        ));
        let calls = ledger.legacy_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].2, first_call.2);
        assert_eq!(calls[1].2, first_call.2);
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.refund_transfers.len(), 1);
        assert!(job.refund_transfers[0].completed);
        assert_eq!(job.refund_transfers[0].block_index, Some(44));
    }

    #[test]
    fn refund_skips_already_refunded_and_dust_and_caps_balance() {
        let target = Principal::from_slice(&[22]);
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        job.target_canister_id = target;
        job.setup_account_identifier = "setup".to_string();
        job.payments = vec![
            RelaySetupPayment {
                refunded: true,
                ..payment(1, "already", 100_000)
            },
            payment(2, "dust", 9_000),
            payment(3, "capped", 500_000),
        ];
        install_state_with_job(target, job);
        let ledger = FakeLedger {
            legacy_results: Arc::new(Mutex::new(vec![Ok(Ok(20))])),
            ..FakeLedger::healthy(60_000)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 60_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(result, RelaySetupRefundResult::Refunded { .. }));
        let calls = ledger.legacy_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "capped");
        assert_eq!(calls[0].1, 50_000);
    }

    #[test]
    fn early_ledger_fee_failure_marks_reserved_job_retryable() {
        let target = Principal::from_slice(&[23]);
        install_state_with_job(target, job_with_status(RelaySetupStatus::FailedRetryable));
        let ledger = FakeLedger {
            fee: Err("fee unavailable".to_string()),
            ..FakeLedger::healthy(0)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };
        let result = block_on(notify_relay_setup_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::FailedRetryable,
                ..
            }
        ));
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.status, RelaySetupStatus::FailedRetryable);
        assert_eq!(
            job.last_error.as_deref(),
            Some("inter-canister call failed: fee unavailable")
        );
    }

    #[test]
    fn index_catchup_blocks_pre_spend_conversion() {
        let target = Principal::from_slice(&[24]);
        state::set_state(State::new(config(), 0));
        let ledger = FakeLedger::healthy(250_000_000);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };
        let result = block_on(notify_relay_setup_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::Pending {
                status: RelaySetupPublicStatus::IndexNotReady,
            }
        ));
        assert!(ledger.transfers.lock().unwrap().is_empty());
    }

    #[test]
    fn funded_setup_below_required_amount_auto_refunds() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[25]);
        state::set_state(State::new(config(), 0));
        let setup_account = setup_account_for(historian, target);
        let setup_account_identifier = account_identifier_text_for_account(&setup_account);
        let ledger = FakeLedger {
            legacy_results: Arc::new(Mutex::new(vec![Ok(Ok(88))])),
            ..FakeLedger::healthy(150_000_000)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 150_000_000,
                transactions: vec![index_transfer(
                    1,
                    "source-a",
                    &setup_account_identifier,
                    150_000_000,
                )],
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::Refunded { ref blocks } if blocks == &vec![88]
        ));
        assert!(ledger.transfers.lock().unwrap().is_empty());
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.status, RelaySetupStatus::Refunded);
    }

    #[test]
    fn missing_cmc_hides_payment_and_auto_refunds_funded_setup() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[26]);
        let mut cfg = config();
        cfg.cmc_canister_id = None;
        state::set_state(State::new(cfg, 0));

        let view = state::with_state(|st| setup_view_from_state(st, target, historian));
        assert!(!view.payment_allowed);
        assert_eq!(
            view.payment_blocked_reason.as_deref(),
            Some("CMC canister is not configured")
        );

        let setup_account = setup_account_for(historian, target);
        let setup_account_identifier = account_identifier_text_for_account(&setup_account);
        let ledger = FakeLedger {
            legacy_results: Arc::new(Mutex::new(vec![Ok(Ok(89))])),
            ..FakeLedger::healthy(250_000_000)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 250_000_000,
                transactions: vec![index_transfer(
                    2,
                    "source-a",
                    &setup_account_identifier,
                    250_000_000,
                )],
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::Refunded { ref blocks } if blocks == &vec![89]
        ));
        assert!(ledger.transfers.lock().unwrap().is_empty());
    }

    #[test]
    fn pagination_cap_does_not_mark_refunded_while_balance_unexplained() {
        let target = Principal::from_slice(&[27]);
        let mut job = job_with_status(RelaySetupStatus::RefundAvailable);
        job.target_canister_id = target;
        job.setup_account_identifier = "setup".to_string();
        install_state_with_job(target, job);

        let mut pages = Vec::new();
        for page in 0..INDEX_PAGE_LIMIT {
            let mut transactions = Vec::new();
            for offset in 0..INDEX_PAGE_SIZE {
                let tx_id = (page as u64) * INDEX_PAGE_SIZE + offset;
                transactions.push(index_transfer(tx_id, "source-a", "setup", 1_000));
            }
            pages.push(GetAccountIdentifierTransactionsResponse {
                balance: 3_000_000,
                transactions,
                oldest_tx_id: Some((page as u64 + 1) * INDEX_PAGE_SIZE),
            });
        }
        let ledger = FakeLedger::healthy(3_000_000);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 3_000_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(pages)),
        };

        let result = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));

        assert!(matches!(result, RelaySetupRefundResult::Failed { .. }));
        assert!(ledger.legacy_calls.lock().unwrap().is_empty());
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.status, RelaySetupStatus::IndexNotReady);
    }

    #[test]
    fn stale_pending_transfer_requires_manual_recovery_after_duplicate_window() {
        TEST_NOW_NANOS.store(
            LEDGER_DUPLICATE_WINDOW_NANOS + 10_000_000_000,
            std::sync::atomic::Ordering::SeqCst,
        );
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[28]);
        let setup_account = setup_account_for(historian, target);
        let mut job = job_with_status(RelaySetupStatus::Ambiguous);
        job.target_canister_id = target;
        job.setup_account = setup_account;
        job.setup_account_identifier = account_identifier_text_for_account(&setup_account);
        let mut record = transfer_record(
            RelaySetupTransferKind::CmcConversion,
            setup_account.subaccount,
            setup_account,
            cmc_deposit_account(Principal::from_slice(&[4]), historian),
            50_000,
            10_000,
            Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
        );
        record.created_at_time_nanos = 1;
        job.cycle_transfer = Some(record);
        install_state_with_job(target, job);
        let ledger = FakeLedger::healthy(60_000);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 60_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::ManualRecoveryRequired,
                ..
            }
        ));
        assert!(ledger.transfers.lock().unwrap().is_empty());
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.status, RelaySetupStatus::ManualRecoveryRequired);
    }

    #[test]
    fn cmc_reply_loss_resumes_topup_without_auto_refund_or_new_transfer() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[32]);
        let setup_account = setup_account_for(historian, target);
        let mut job = job_with_status(RelaySetupStatus::Ambiguous);
        job.target_canister_id = target;
        job.setup_account = setup_account;
        job.setup_account_identifier = account_identifier_text_for_account(&setup_account);
        job.cycle_conversion_e8s = Some(50_000);
        job.cycle_transfer = Some(transfer_record(
            RelaySetupTransferKind::CmcConversion,
            setup_account.subaccount,
            setup_account,
            cmc_deposit_account(Principal::from_slice(&[4]), historian),
            50_000,
            10_000,
            Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
        ));
        let original_created_at = job.cycle_transfer.as_ref().unwrap().created_at_time_nanos;
        install_state_with_job(target, job);
        let ledger = FakeLedger::healthy(20_000);
        let cmc = FakeCmc {
            notify_results: Arc::new(Mutex::new(vec![Err(NotifyTopUpError::Transport(
                "reply still lost".to_string(),
            ))])),
            notify_calls: Arc::new(Mutex::new(Vec::new())),
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 20_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &cmc,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::FailedRetryable,
                ..
            }
        ));
        let transfers = ledger.transfers.lock().unwrap().clone();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].created_at_time, Some(original_created_at));
        assert!(state::with_state(|st| st
            .relay_setup_jobs
            .get(&target)
            .unwrap()
            .refund_transfers
            .is_empty()));
        assert_eq!(cmc.notify_calls.lock().unwrap().as_slice(), &[77]);
    }

    #[test]
    fn existing_relay_sweep_reconciles_pending_record_before_dust_check() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[33]);
        let relay_id = Principal::from_slice(&[34]);
        state::set_state(State::new(config(), 0));
        let ledger = FakeLedger {
            transfer_results: Arc::new(Mutex::new(vec![Err(
                "reply lost after ledger accepted sweep".to_string(),
            )])),
            ..FakeLedger::healthy(200_000)
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 200_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let first = block_on(sweep_existing(
            target,
            active_relay(target, relay_id),
            200_000,
            10_000,
            &ledger,
            &index,
            historian,
        ));
        assert!(matches!(
            first,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::FailedRetryable,
                ..
            }
        ));
        let record = state::with_state(|st| {
            st.relay_setup_jobs
                .get(&target)
                .unwrap()
                .existing_relay_sweep_transfer
                .clone()
                .unwrap()
        });
        assert!(!record.completed);
        let index_after_accept = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 10_000,
                transactions: vec![index_transfer_with_created_at(
                    88,
                    &record.from_account_identifier,
                    &record.to_account_identifier,
                    record.amount_e8s,
                    record.created_at_time_nanos,
                )],
                oldest_tx_id: Some(88),
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let second = block_on(sweep_existing(
            target,
            active_relay(target, relay_id),
            1,
            10_000,
            &ledger,
            &index_after_accept,
            historian,
        ));
        assert!(matches!(
            second,
            RelaySetupNotifyResult::SweptToExistingRelay {
                amount_e8s: 190_000,
                block_index: 88,
                ..
            }
        ));
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert!(job.existing_relay_sweep_transfer.unwrap().completed);
    }

    #[test]
    fn active_relay_second_same_amount_deposit_creates_distinct_sweep() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[35]);
        let relay_id = Principal::from_slice(&[36]);
        state::set_state(State::new(config(), 0));
        let ledger = FakeLedger::healthy(200_000);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 200_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let first = block_on(sweep_existing(
            target,
            active_relay(target, relay_id),
            200_000,
            10_000,
            &ledger,
            &index,
            historian,
        ));
        assert!(matches!(
            first,
            RelaySetupNotifyResult::SweptToExistingRelay { .. }
        ));
        let first_created_at = ledger.transfers.lock().unwrap()[0].created_at_time;

        let second = block_on(sweep_existing(
            target,
            active_relay(target, relay_id),
            200_000,
            10_000,
            &ledger,
            &index,
            historian,
        ));
        assert!(matches!(
            second,
            RelaySetupNotifyResult::SweptToExistingRelay { .. }
        ));
        let transfers = ledger.transfers.lock().unwrap().clone();
        assert_eq!(transfers.len(), 2);
        assert_eq!(transfers[0].amount, transfers[1].amount);
        assert_eq!(transfers[0].created_at_time, first_created_at);
        assert_ne!(transfers[0].created_at_time, transfers[1].created_at_time);
    }

    #[test]
    fn relay_setup_install_code_lost_reply_detects_existing_module_hash() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[37]);
        let relay_id = Principal::from_slice(&[38]);
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        job.target_canister_id = target;
        job.relay_canister_id = Some(relay_id);
        job.cycles_minted = Some(1_000_000_000_000);
        job.code_installed = false;
        install_state_with_job(target, job);
        let expected_hash = approved_relay_wasm_hash().unwrap().to_vec();
        let management = FakeManagement::healthy(relay_id, Some(expected_hash));
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(create_and_activate_relay(
            target,
            0,
            10_000,
            &index,
            &FakeBlackhole,
            &management,
            historian,
        ));

        assert!(matches!(result, RelaySetupNotifyResult::Active { .. }));
        assert_eq!(*management.create_calls.lock().unwrap(), 0);
        assert_eq!(*management.install_calls.lock().unwrap(), 0);
        assert_eq!(*management.update_calls.lock().unwrap(), 1);
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert!(job.code_installed);
        assert_eq!(job.phase, Some(RelaySetupPhase::Active));
        assert_eq!(job.relay_canister_id, Some(relay_id));
    }

    #[test]
    fn relay_setup_install_code_unexpected_module_hash_enters_manual_recovery() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[41]);
        let relay_id = Principal::from_slice(&[42, 1]);
        let mut job = job_with_status(RelaySetupStatus::FailedRetryable);
        job.target_canister_id = target;
        job.relay_canister_id = Some(relay_id);
        job.cycles_minted = Some(1_000_000_000_000);
        job.code_installed = false;
        install_state_with_job(target, job);
        let management = FakeManagement::healthy(relay_id, Some(vec![0xAA; 32]));
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(create_and_activate_relay(
            target,
            0,
            10_000,
            &index,
            &FakeBlackhole,
            &management,
            historian,
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::ManualRecoveryRequired,
                ..
            }
        ));
        assert_eq!(*management.create_calls.lock().unwrap(), 0);
        assert_eq!(*management.install_calls.lock().unwrap(), 0);
        assert_eq!(*management.update_calls.lock().unwrap(), 0);
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.relay_canister_id, Some(relay_id));
        assert!(!job.code_installed);
        assert_eq!(job.status, RelaySetupStatus::ManualRecoveryRequired);
        assert_eq!(
            job.last_error.as_deref(),
            Some("relay canister already has an unexpected module hash")
        );
        assert!(state::with_state(|st| st
            .relay_registry_by_target
            .get(&target)
            .is_none()));
    }

    #[test]
    fn relay_setup_create_canister_ambiguous_enters_manual_recovery_without_duplicate_create() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[39]);
        let mut job = job_with_status(RelaySetupStatus::CycleNotifySucceeded);
        job.target_canister_id = target;
        job.cycles_minted = Some(1_000_000_000_000);
        install_state_with_job(target, job);
        let management = FakeManagement {
            create_results: Arc::new(Mutex::new(vec![Err(ManagementClientError::Ambiguous(
                "SYS_UNKNOWN".to_string(),
            ))])),
            create_calls: Arc::new(Mutex::new(0)),
            install_calls: Arc::new(Mutex::new(0)),
            update_calls: Arc::new(Mutex::new(0)),
            status_hashes: Arc::new(Mutex::new(Vec::new())),
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(create_and_activate_relay(
            target,
            0,
            10_000,
            &index,
            &FakeBlackhole,
            &management,
            historian,
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::ManualRecoveryRequired,
                ..
            }
        ));
        assert_eq!(*management.create_calls.lock().unwrap(), 1);
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.status, RelaySetupStatus::ManualRecoveryRequired);
        assert!(job.relay_create_attempt.is_some());
        assert!(job.relay_canister_id.is_none());
        assert!(state::with_state(|st| st
            .relay_registry_by_target
            .get(&target)
            .is_none()));

        let ledger = FakeLedger::healthy(250_000_000);
        let retry = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));
        assert!(matches!(
            retry,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupPublicStatus::ManualRecoveryRequired,
                ..
            }
        ));
        assert_eq!(*management.create_calls.lock().unwrap(), 1);
    }

    #[test]
    fn relay_setup_manual_recovery_view_exposes_stuck_job() {
        let target = Principal::from_slice(&[40]);
        let mut job = job_with_status(RelaySetupStatus::ManualRecoveryRequired);
        job.target_canister_id = target;
        job.last_error = Some(
            "create_canister may have succeeded but relay_canister_id was not recorded".to_string(),
        );
        job.setup_amount_seen_e8s = 250_000_000;
        job.cycle_transfer = Some(transfer_record(
            RelaySetupTransferKind::CmcConversion,
            Some([1; 32]),
            Account {
                owner: Principal::from_slice(&[2]),
                subaccount: Some([1; 32]),
            },
            Account {
                owner: Principal::from_slice(&[4]),
                subaccount: None,
            },
            100_000_000,
            10_000,
            Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
        ));
        install_state_with_job(target, job);

        let view = setup_recovery_view(target);

        assert_eq!(view.target_canister_id, target);
        assert_eq!(view.status, RelaySetupPublicStatus::ManualRecoveryRequired);
        assert_eq!(view.setup_amount_seen_e8s, 250_000_000);
        assert_eq!(view.refund_transfer_count, 0);
        assert!(view.cycle_transfer.is_some());
        assert!(view
            .last_error
            .as_deref()
            .unwrap()
            .contains("relay_canister_id was not recorded"));
    }

    #[test]
    fn relay_setup_manual_recovery_view_does_not_mutate_state() {
        let target = Principal::from_slice(&[41]);
        install_state_with_job(
            target,
            job_with_status(RelaySetupStatus::ManualRecoveryRequired),
        );
        let before = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned());

        let first = setup_recovery_view(target);
        let second = setup_recovery_view(target);

        assert_eq!(first.updated_at_ts, second.updated_at_ts);
        assert_eq!(
            before,
            state::with_state(|st| st.relay_setup_jobs.get(&target).cloned())
        );
    }

    #[test]
    fn relay_setup_index_cap_then_catchup_refunds() {
        let target = Principal::from_slice(&[42, 1]);
        let mut job = job_with_status(RelaySetupStatus::RefundAvailable);
        job.target_canister_id = target;
        job.setup_account_identifier = "setup".to_string();
        install_state_with_job(target, job);
        let ledger = FakeLedger {
            legacy_results: Arc::new(Mutex::new(vec![Ok(Ok(91))])),
            ..FakeLedger::healthy(3_000_000)
        };
        let capped_pages = (0..INDEX_PAGE_LIMIT)
            .map(|page| GetAccountIdentifierTransactionsResponse {
                balance: 3_000_000,
                transactions: (0..INDEX_PAGE_SIZE)
                    .map(|offset| {
                        index_transfer(
                            (page as u64) * INDEX_PAGE_SIZE + offset,
                            "source-a",
                            "setup",
                            1_000,
                        )
                    })
                    .collect(),
                oldest_tx_id: Some((page as u64 + 1) * INDEX_PAGE_SIZE),
            })
            .collect();
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 3_000_000,
                transactions: vec![index_transfer(5000, "source-a", "setup", 3_000_000)],
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(capped_pages)),
        };

        let first = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(first, RelaySetupRefundResult::Failed { .. }));
        let second = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));

        assert!(matches!(
            second,
            RelaySetupRefundResult::Refunded { ref blocks } if blocks == &vec![91]
        ));
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned().unwrap());
        assert_eq!(job.status, RelaySetupStatus::Refunded);
    }

    #[test]
    fn relay_setup_existing_active_relay_allows_sweep_when_factory_disabled() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[43]);
        let relay_id = Principal::from_slice(&[44]);
        let mut cfg = config();
        cfg.relay_factory_enabled = false;
        let mut st = State::new(cfg, 0);
        st.relay_registry_by_target
            .insert(target, active_relay(target, relay_id));
        state::set_state(st);
        let ledger = FakeLedger::healthy(200_000);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 200_000,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let view = setup_view_from_state(&state::with_state(|st| st.clone()), target, historian);
        assert!(view.payment_allowed);

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::SweptToExistingRelay {
                amount_e8s: 190_000,
                ..
            }
        ));
    }

    fn stable_roundtrip(job: RelaySetupJob) -> RelaySetupJob {
        candid::decode_one(&candid::encode_one(job).unwrap()).unwrap()
    }

    #[test]
    fn relay_setup_upgrade_preserves_cycle_transfer_incomplete() {
        let target = Principal::from_slice(&[45]);
        let setup_account = setup_account_for(Principal::from_slice(&[42]), target);
        let mut job = job_with_status(RelaySetupStatus::Ambiguous);
        job.target_canister_id = target;
        job.setup_account = setup_account;
        job.setup_account_identifier = account_identifier_text_for_account(&setup_account);
        job.cycle_transfer = Some(transfer_record(
            RelaySetupTransferKind::CmcConversion,
            setup_account.subaccount,
            setup_account,
            cmc_deposit_account(Principal::from_slice(&[4]), Principal::from_slice(&[42])),
            50_000,
            10_000,
            Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
        ));
        let restored = stable_roundtrip(job);

        assert_eq!(
            relay_setup_resume_point(&restored),
            RelaySetupResumePoint::ReconcileCycleTransfer
        );
        install_state_with_job(target, restored);
        assert!(setup_recovery_view(target).cycle_transfer.is_some());
    }

    #[test]
    fn relay_setup_upgrade_preserves_relay_canister_id_before_install() {
        let target = Principal::from_slice(&[46]);
        let relay_id = Principal::from_slice(&[47]);
        let mut job = job_with_status(RelaySetupStatus::CanisterCreated);
        job.target_canister_id = target;
        job.cycles_minted = Some(1_000_000_000_000);
        job.relay_canister_id = Some(relay_id);
        job.relay_initial_cycles = Some(1_000_000_000_000);
        let restored = stable_roundtrip(job);

        assert_eq!(
            relay_setup_resume_point(&restored),
            RelaySetupResumePoint::InstallRelayCode { relay_id }
        );
        install_state_with_job(target, restored);
        assert_eq!(
            setup_recovery_view(target).relay_canister_id,
            Some(relay_id)
        );
    }

    #[test]
    fn relay_setup_upgrade_preserves_code_installed_before_funding() {
        let target = Principal::from_slice(&[48]);
        let relay_id = Principal::from_slice(&[49]);
        let mut job = job_with_status(RelaySetupStatus::CodeInstalled);
        job.target_canister_id = target;
        job.relay_canister_id = Some(relay_id);
        job.code_installed = true;
        let restored = stable_roundtrip(job);

        assert_eq!(
            relay_setup_resume_point(&restored),
            RelaySetupResumePoint::FundRelaySubaccountOne { relay_id }
        );
        install_state_with_job(target, restored);
        assert_eq!(
            setup_recovery_view(target).relay_canister_id,
            Some(relay_id)
        );
    }

    #[test]
    fn relay_setup_upgrade_preserves_relay_funding_incomplete() {
        let target = Principal::from_slice(&[50]);
        let relay_id = Principal::from_slice(&[51]);
        let setup_account = setup_account_for(Principal::from_slice(&[42]), target);
        let mut job = job_with_status(RelaySetupStatus::FundingRelaySubaccountOne);
        job.target_canister_id = target;
        job.relay_canister_id = Some(relay_id);
        job.code_installed = true;
        job.relay_funding_transfer = Some(transfer_record(
            RelaySetupTransferKind::RelayFunding,
            setup_account.subaccount,
            setup_account,
            relay_subaccount_one(relay_id),
            100_000,
            10_000,
            None,
        ));
        let restored = stable_roundtrip(job);

        assert_eq!(
            relay_setup_resume_point(&restored),
            RelaySetupResumePoint::FundRelaySubaccountOne { relay_id }
        );
        install_state_with_job(target, restored);
        assert!(setup_recovery_view(target).relay_funding_transfer.is_some());
    }

    #[test]
    fn relay_setup_upgrade_preserves_manual_recovery_required() {
        let target = Principal::from_slice(&[52]);
        let mut job = job_with_status(RelaySetupStatus::ManualRecoveryRequired);
        job.target_canister_id = target;
        job.last_error = Some(
            "create_canister may have succeeded but relay_canister_id was not recorded".to_string(),
        );
        job.relay_create_attempt = Some(RelayCreateAttempt {
            target_canister_id: target,
            created_at_ts: 99,
            initial_cycles: 1_000_000_000_000,
            relay_wasm_hash_hex: relay_wasm_hash_hex(),
        });
        let restored = stable_roundtrip(job);

        assert!(!resumable_job(&restored));
        install_state_with_job(target, restored);
        let view = setup_recovery_view(target);
        assert_eq!(view.status, RelaySetupPublicStatus::ManualRecoveryRequired);
        assert!(view.relay_create_attempt.is_some());
    }

    #[test]
    fn list_relay_registrations_returns_only_active_entries() {
        let target_active = Principal::from_slice(&[25, 1]);
        let target_pending = Principal::from_slice(&[25, 2]);
        let target_failed = Principal::from_slice(&[25, 3]);
        let target_superseded = Principal::from_slice(&[25, 4]);
        let mut st = State::new(config(), 0);
        st.relay_registry_by_target.insert(
            target_active,
            active_relay(target_active, Principal::from_slice(&[35, 1])),
        );
        let mut pending = active_relay(target_pending, Principal::from_slice(&[35, 2]));
        pending.status = RelayRegistryStatus::Pending;
        st.relay_registry_by_target.insert(target_pending, pending);
        let mut failed = active_relay(target_failed, Principal::from_slice(&[35, 3]));
        failed.status = RelayRegistryStatus::Failed;
        st.relay_registry_by_target.insert(target_failed, failed);
        let mut superseded = active_relay(target_superseded, Principal::from_slice(&[35, 4]));
        superseded.status = RelayRegistryStatus::Superseded;
        st.relay_registry_by_target
            .insert(target_superseded, superseded);
        state::set_state(st);

        let listed = list_relay_registrations(ListRelayRegistrationsArgs::default());

        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].target_canister_id, target_active);
        assert_eq!(listed.next_start_after, None);
    }

    #[test]
    fn relay_setup_zero_balance_notify_does_not_persist_jobs() {
        state::set_state(State::new(config(), 0));
        let ledger = FakeLedger::healthy(0);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        for target_id in 0..50u8 {
            let result = block_on(notify_relay_setup_with_clients_for_historian(
                Principal::from_slice(&[42]),
                Principal::from_slice(&[26, target_id]),
                &ledger,
                &index,
                &FakeBlackhole,
                &FakeCmc::healthy(),
            ));

            assert!(matches!(
                result,
                RelaySetupNotifyResult::BelowMinimum { .. }
            ));
        }
        assert_eq!(state::with_state(|st| st.relay_setup_jobs.len()), 0);
        assert_eq!(state::with_state(|st| st.relay_registry_by_target.len()), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert!(ledger.legacy_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn relay_setup_view_is_pure_query() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[25]);
        state::set_state(State::new(config(), 0));

        let first = setup_view_from_state(&state::with_state(|st| st.clone()), target, historian);
        let second = setup_view_from_state(&state::with_state(|st| st.clone()), target, historian);

        assert_eq!(first.setup_account, second.setup_account);
        assert_eq!(
            first.setup_account_identifier,
            second.setup_account_identifier
        );
        assert_eq!(state::with_state(|st| st.relay_setup_jobs.len()), 0);
        assert_eq!(state::with_state(|st| st.relay_registry_by_target.len()), 0);
    }

    #[test]
    fn funded_invalid_target_auto_refund_is_attempted() {
        let historian = Principal::from_slice(&[42]);
        state::set_state(State::new(config(), 0));
        let ledger = FakeLedger::healthy(250_000_000);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            Principal::anonymous(),
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::RefundPending { .. }
        ));
        let job = state::with_state(|st| st.relay_setup_jobs.get(&Principal::anonymous()).cloned())
            .unwrap();
        assert_eq!(job.status, RelaySetupStatus::RefundAvailable);
        assert!(refund_eligible_status(&job));

        let view = setup_view_from_state(
            &state::with_state(|st| st.clone()),
            Principal::anonymous(),
            historian,
        );
        assert!(!view.payment_allowed);
        assert!(view.payment_blocked_reason.is_some());
        assert_eq!(view.status, RelaySetupPublicStatus::Refunding);
    }

    #[test]
    fn funded_factory_disabled_setup_auto_refunds() {
        let historian = Principal::from_slice(&[42]);
        let target = Principal::from_slice(&[26]);
        let mut cfg = config();
        cfg.relay_factory_enabled = false;
        state::set_state(State::new(cfg, 0));
        let ledger = FakeLedger::healthy(250_000_000);
        ledger.legacy_results.lock().unwrap().push(Ok(Ok(88)));
        let setup_account = setup_account_for(historian, target);
        let setup_account_identifier = account_identifier_text_for_account(&setup_account);
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 250_000_000,
                transactions: vec![index_transfer(
                    1,
                    "source",
                    &setup_account_identifier,
                    250_000_000,
                )],
                oldest_tx_id: Some(1),
            },
            pages: Arc::new(Mutex::new(Vec::new())),
        };

        let result = block_on(notify_relay_setup_with_clients_for_historian(
            historian,
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc::healthy(),
        ));

        assert!(matches!(result, RelaySetupNotifyResult::Refunded { .. }));
        let job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned()).unwrap();
        assert_eq!(job.status, RelaySetupStatus::Refunded);
        let view = setup_view_from_state(&state::with_state(|st| st.clone()), target, historian);
        assert!(!view.payment_allowed);
        assert_eq!(view.status, RelaySetupPublicStatus::Refunded);
    }

    #[test]
    fn setup_payment_indexing_paginates_beyond_first_hundred() {
        let target = Principal::from_slice(&[27]);
        let setup = "setup-account";
        let first_page = GetAccountIdentifierTransactionsResponse {
            balance: 105_000,
            transactions: (0..100)
                .map(|idx| index_transfer(200 - idx, "dust", setup, 1))
                .collect(),
            oldest_tx_id: Some(101),
        };
        let second_page = GetAccountIdentifierTransactionsResponse {
            balance: 105_000,
            transactions: (0..5)
                .map(|idx| index_transfer(100 - idx, "funded", setup, 10_000))
                .collect(),
            oldest_tx_id: Some(96),
        };
        let index = FakeIndex {
            response: GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: Vec::new(),
                oldest_tx_id: None,
            },
            pages: Arc::new(Mutex::new(vec![first_page, second_page])),
        };

        let payments = block_on(index_setup_payments(target, setup.to_string(), &index)).unwrap();

        assert!(!payments.hit_page_cap);
        assert_eq!(payments.payments.len(), 105);
        assert!(payments.payments.iter().any(|payment| payment.tx_id == 100));
        assert_eq!(
            payments
                .payments
                .iter()
                .fold(0u64, |acc, payment| acc.saturating_add(payment.amount_e8s)),
            50_100
        );
    }
}
