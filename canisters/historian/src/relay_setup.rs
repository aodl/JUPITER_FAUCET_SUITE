use crate::clients::index::{account_identifier_text_for_account, IndexOperation};
use crate::clients::{BlackholeClient, CmcCanister, CmcClient, IndexClient, LedgerClient};
use crate::state::{self, Config, RelayRegistryStatus, State};
use crate::*;
use candid::{Encode, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg};
use jupiter_ic_clients::account::{principal_to_subaccount, relay_setup_subaccount};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const RELAY_SUBACCOUNT_ONE: [u8; 32] = {
    let mut bytes = [0u8; 32];
    bytes[31] = 1;
    bytes
};

const TOP_UP_CANISTER_MEMO: u64 = 1_347_768_404;
const REFUND_MEMO: u64 = 0x4a525246;

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

#[cfg(not(test))]
fn now_nanos() -> u64 {
    ic_cdk::api::time()
}

#[cfg(test)]
fn now_nanos() -> u64 {
    0
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

pub(crate) fn setup_view_from_state(
    st: &State,
    target: Principal,
    historian: Principal,
) -> RelaySetupView {
    let setup_account = setup_account_for(historian, target);
    let setup_account_identifier = account_identifier_text_for_account(&setup_account);
    let setup_job = st.relay_setup_jobs.get(&target).cloned();
    RelaySetupView {
        target_canister_id: target,
        setup_account,
        setup_account_identifier,
        minimum_e8s: st.config.relay_setup_min_e8s,
        dust_e8s: st.config.relay_setup_dust_e8s,
        current_status: setup_job.as_ref().map(|job| job.status.clone()),
        existing_relay: st.relay_registry_by_target.get(&target).cloned(),
        setup_job: setup_job.map(Into::into),
        factory_enabled: st.config.relay_factory_enabled && approved_self_service_relay_wasm().is_some(),
        relay_wasm_hash_hex: relay_wasm_hash_hex(),
        warning_text: Some(
            "This relay can only be created for canisters whose cycle balance is visible through the Jupiter blackhole canister. In practice, the configured blackhole canister must be able to call canister_status for the target.".to_string(),
        ),
    }
}

pub(crate) fn get_relay_for_canister(target: Principal) -> Option<RelayRegistryEntry> {
    state::with_state(|st| st.relay_registry_by_target.get(&target).cloned())
}

pub(crate) fn get_relay_by_id(relay: Principal) -> Vec<RelayRegistryEntry> {
    state::with_state(|st| {
        st.relay_targets_by_relay
            .get(&relay)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|target| st.relay_registry_by_target.get(&target).cloned())
            .collect()
    })
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
            if items.len() >= limit {
                next = items
                    .last()
                    .map(|item: &RelayRegistryEntry| item.target_canister_id);
                break;
            }
            items.push(entry.clone());
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
) -> Result<Vec<RelaySetupPayment>, String> {
    let resp = index
        .get_account_identifier_transactions(setup_account_identifier.clone(), None, 100)
        .await
        .map_err(|err| err.to_string())?;
    let mut payments = Vec::new();
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
    Ok(payments)
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
        && job.setup_amount_processed_e8s == 0
}

fn refund_eligible_status(job: &RelaySetupJob) -> bool {
    matches!(
        job.status,
        RelaySetupStatus::BelowMinimum
            | RelaySetupStatus::TargetNotObservable
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
) -> TransferArg {
    TransferArg {
        from_subaccount,
        to,
        amount: amount.into(),
        fee: Some(fee.into()),
        memo: memo.map(Memo::from),
        created_at_time: Some(now_nanos()),
    }
}

async fn sweep_existing(
    target: Principal,
    relay: RelayRegistryEntry,
    balance: u64,
    fee: u64,
    ledger: &dyn LedgerClient,
) -> RelaySetupNotifyResult {
    if balance <= fee.saturating_add(state::with_state(|st| st.config.relay_setup_dust_e8s)) {
        return RelaySetupNotifyResult::SweepBelowDust {
            relay,
            current_balance_e8s: balance,
        };
    }
    let amount = balance.saturating_sub(fee);
    let arg = transfer_arg(
        Some(relay_setup_subaccount(target)),
        relay_subaccount_one(relay.relay_canister_id),
        amount,
        fee,
        None,
    );
    match ledger.icrc1_transfer(arg).await {
        Ok(Ok(block)) => RelaySetupNotifyResult::SweptToExistingRelay {
            relay,
            amount_e8s: amount,
            block_index: u64::try_from(block.0).unwrap_or(u64::MAX),
        },
        Ok(Err(err)) => RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedRetryable,
            message: format!("sweep transfer failed: {err:?}"),
        },
        Err(err) => RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedRetryable,
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
    if let Some(message) = invalid_target(target, &cfg, historian) {
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedTerminal,
            message,
        };
    }
    let setup_account = setup_account_for(historian, target);
    let setup_account_identifier = account_identifier_text_for_account(&setup_account);
    let existing_job = state::with_state(|st| st.relay_setup_jobs.get(&target).cloned());
    if let Some(job) = existing_job
        .as_ref()
        .filter(|job| in_flight_job(job) && !resumable_job(job))
    {
        return RelaySetupNotifyResult::Pending {
            job: job.clone().into(),
        };
    }
    let resume_job = existing_job.filter(resumable_job);
    let active_relay = state::with_state(|st| st.relay_registry_by_target.get(&target).cloned())
        .filter(|entry| entry.status == RelayRegistryStatus::Active);
    if active_relay.is_none() && resume_job.is_none() {
        state::with_root_and_relay_factory_state_mut(target, |st| {
            st.relay_setup_jobs.entry(target).or_insert_with(|| {
                reserve_job(target, setup_account, setup_account_identifier.clone())
            });
        });
    }
    let fee = match ledger.fee_e8s().await {
        Ok(fee) => fee,
        Err(err) => {
            set_job_failed_retryable(target, err.to_string());
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                message: err.to_string(),
            };
        }
    };
    let balance = match ledger.balance_of_e8s(setup_account).await {
        Ok(balance) => balance,
        Err(err) => {
            set_job_failed_retryable(target, err.to_string());
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                message: err.to_string(),
            };
        }
    };
    if let Some(relay) = active_relay {
        return sweep_existing(target, relay, balance, fee, ledger).await;
    }
    if let Some(job) = resume_job {
        let resume_point = relay_setup_resume_point(&job);
        if let RelaySetupResumePoint::NotifyCycleTopUp { block_index } = resume_point {
            let minted = match cmc.notify_top_up(historian, block_index).await {
                Ok(cycles) => cycles,
                Err(err) => {
                    set_job_failed_retryable(target, err.clone());
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::FailedRetryable,
                        message: err,
                    };
                }
            };
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.status = RelaySetupStatus::CycleNotifySucceeded;
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
        return create_and_activate_relay(target, relay_funding, fee).await;
    }
    if balance < cfg.relay_setup_min_e8s {
        set_job_status(target, RelaySetupStatus::BelowMinimum, None);
        return RelaySetupNotifyResult::BelowMinimum {
            minimum_e8s: cfg.relay_setup_min_e8s,
            current_balance_e8s: balance,
        };
    }
    let required = required_setup_e8s(&cfg, fee);
    if balance < required {
        set_job_status(
            target,
            RelaySetupStatus::InsufficientForCurrentRate,
            Some("setup balance is below current relay setup requirement".to_string()),
        );
        return RelaySetupNotifyResult::InsufficientForCurrentRate {
            required_e8s: required,
            current_balance_e8s: balance,
        };
    }
    if let Err(err) = blackhole.canister_status(target).await {
        state::with_root_and_relay_factory_state_mut(target, |st| {
            let mut job = st.relay_setup_jobs.remove(&target).unwrap_or_else(|| {
                reserve_job(target, setup_account, setup_account_identifier.clone())
            });
            job.status = RelaySetupStatus::TargetNotObservable;
            job.last_error = Some(err.to_string());
            job.updated_at_ts = now_secs();
            st.relay_setup_jobs.insert(target, job);
        });
        return RelaySetupNotifyResult::TargetNotObservable {
            message: "target is not observable through the configured blackhole canister"
                .to_string(),
        };
    }
    let payments = match index_setup_payments(target, setup_account_identifier.clone(), index).await
    {
        Ok(payments) => payments,
        Err(err) => {
            set_job_status(target, RelaySetupStatus::FailedRetryable, Some(err.clone()));
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                message: err,
            };
        }
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            merge_payments(job, payments);
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
        let message = "setup account balance is visible on ledger but ICP index has not caught up"
            .to_string();
        set_job_failed_retryable(target, message.clone());
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedRetryable,
            message,
        };
    }
    if !cfg.relay_factory_enabled || approved_self_service_relay_wasm().is_none() {
        set_job_status(
            target,
            RelaySetupStatus::FailedTerminal,
            Some("relay factory is disabled".to_string()),
        );
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedTerminal,
            message: "relay factory is disabled".to_string(),
        };
    }

    let Some(conversion_e8s) = cycle_conversion_e8s(&cfg, fee, balance) else {
        set_job_status(
            target,
            RelaySetupStatus::InsufficientForCurrentRate,
            Some("setup balance cannot leave useful relay subaccount-1 seed".to_string()),
        );
        return RelaySetupNotifyResult::InsufficientForCurrentRate {
            required_e8s: required,
            current_balance_e8s: balance,
        };
    };
    let cmc_id = cfg.cmc_canister_id.unwrap_or_else(mainnet_cmc_id);
    set_job_status(target, RelaySetupStatus::ConvertingCycles, None);
    let block_index = match ledger
        .icrc1_transfer(transfer_arg(
            Some(relay_setup_subaccount(target)),
            cmc_deposit_account(cmc_id, historian),
            conversion_e8s,
            fee,
            Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
        ))
        .await
    {
        Ok(Ok(block)) => u64::try_from(block.0).unwrap_or(u64::MAX),
        Ok(Err(err)) => {
            set_job_status(
                target,
                RelaySetupStatus::FailedRetryable,
                Some(format!("{err:?}")),
            );
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                message: format!("CMC transfer failed: {err:?}"),
            };
        }
        Err(err) => {
            set_job_status(target, RelaySetupStatus::Ambiguous, Some(err.to_string()));
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::Ambiguous,
                message: err.to_string(),
            };
        }
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = RelaySetupStatus::CycleTransferAccepted;
            job.cycle_conversion_e8s = Some(conversion_e8s);
            job.cycle_transfer_block_index = Some(block_index);
            job.updated_at_ts = now_secs();
        }
    });
    let minted = match cmc.notify_top_up(historian, block_index).await {
        Ok(cycles) => cycles,
        Err(err) => {
            set_job_failed_retryable(target, err.clone());
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                message: err,
            };
        }
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            job.status = RelaySetupStatus::CycleNotifySucceeded;
            job.cycles_minted = Some(minted);
            job.updated_at_ts = now_secs();
        }
    });
    let relay_funding = balance
        .saturating_sub(conversion_e8s)
        .saturating_sub(fee.saturating_mul(2));
    create_and_activate_relay(target, relay_funding, fee).await
}

async fn create_and_activate_relay(
    target: Principal,
    relay_funding: u64,
    fee: u64,
) -> RelaySetupNotifyResult {
    let cfg = state::with_state(|st| st.config.clone());
    let wasm = match approved_self_service_relay_wasm() {
        Some(wasm) => wasm.to_vec(),
        None => {
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedTerminal,
                message: "approved relay wasm is not embedded".to_string(),
            }
        }
    };
    let (relay_id, status, funding_already_recorded) = state::with_state(|st| {
        let job = st.relay_setup_jobs.get(&target);
        (
            job.and_then(|job| job.relay_canister_id),
            job.map(|job| job.status.clone())
                .unwrap_or(RelaySetupStatus::Pending),
            job.and_then(|job| job.relay_funding_block_index).is_some(),
        )
    });
    let relay_id = match relay_id {
        Some(relay_id) => relay_id,
        None => {
            let create_args = jupiter_ic_clients::management::CreateCanisterArgs {
                settings: Some(jupiter_ic_clients::management::CanisterSettings {
                    controllers: Some(vec![ic_cdk::api::canister_self()]),
                    log_visibility: Some(jupiter_ic_clients::management::LogVisibility::Public),
                }),
            };
            set_job_status(target, RelaySetupStatus::CreatingCanister, None);
            let relay_id = match jupiter_ic_clients::management::create_canister(
                &create_args,
                cfg.relay_initial_cycles,
            )
            .await
            {
                Ok(result) => result.canister_id,
                Err(err) => {
                    set_job_status(
                        target,
                        RelaySetupStatus::FailedRetryable,
                        Some(format!("{err:?}")),
                    );
                    return RelaySetupNotifyResult::Failed {
                        status: RelaySetupStatus::FailedRetryable,
                        message: format!("create_canister failed: {err:?}"),
                    };
                }
            };
            state::with_root_and_relay_factory_state_mut(target, |st| {
                if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                    job.status = RelaySetupStatus::CanisterCreated;
                    job.relay_canister_id = Some(relay_id);
                    job.relay_initial_cycles = Some(cfg.relay_initial_cycles);
                    job.updated_at_ts = now_secs();
                }
            });
            relay_id
        }
    };
    if !matches!(
        status,
        RelaySetupStatus::CodeInstalled
            | RelaySetupStatus::FundingRelaySubaccountOne
            | RelaySetupStatus::Blackholing
    ) {
        let relay_args = jupiter_relay_init_arg(&cfg, target);
        set_job_status(target, RelaySetupStatus::InstallingCode, None);
        if let Err(err) = jupiter_ic_clients::management::install_code(
            &jupiter_ic_clients::management::InstallCodeArgs {
                mode: jupiter_ic_clients::management::InstallMode::Install,
                canister_id: relay_id,
                wasm_module: wasm,
                arg: relay_args,
            },
        )
        .await
        {
            set_job_status(
                target,
                RelaySetupStatus::FailedRetryable,
                Some(format!("{err:?}")),
            );
            return RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                message: format!("install_code failed: {err:?}"),
            };
        }
    }
    set_job_status(target, RelaySetupStatus::CodeInstalled, None);
    let ledger = jupiter_ic_clients::ledger::IcrcLedgerCanister::new(cfg.ledger_canister_id);
    if !funding_already_recorded && relay_funding > cfg.relay_setup_dust_e8s {
        set_job_status(target, RelaySetupStatus::FundingRelaySubaccountOne, None);
        match crate::clients::LedgerClient::icrc1_transfer(
            &ledger,
            transfer_arg(
                Some(relay_setup_subaccount(target)),
                relay_subaccount_one(relay_id),
                relay_funding,
                fee,
                None,
            ),
        )
        .await
        {
            Ok(Ok(block)) => {
                state::with_root_and_relay_factory_state_mut(target, |st| {
                    if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
                        job.relay_funding_e8s = Some(relay_funding);
                        job.relay_funding_block_index =
                            Some(u64::try_from(block.0).unwrap_or(u64::MAX));
                        job.updated_at_ts = now_secs();
                    }
                });
            }
            Ok(Err(err)) => {
                set_job_status(
                    target,
                    RelaySetupStatus::FailedRetryable,
                    Some(format!("{err:?}")),
                );
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::FailedRetryable,
                    message: format!("relay funding failed: {err:?}"),
                };
            }
            Err(err) => {
                set_job_status(target, RelaySetupStatus::Ambiguous, Some(err.to_string()));
                return RelaySetupNotifyResult::Failed {
                    status: RelaySetupStatus::Ambiguous,
                    message: err.to_string(),
                };
            }
        }
    }
    set_job_status(target, RelaySetupStatus::Blackholing, None);
    if let Err(err) = jupiter_ic_clients::management::update_settings(
        &jupiter_ic_clients::management::UpdateSettingsArgs {
            canister_id: relay_id,
            settings: jupiter_ic_clients::management::CanisterSettings {
                controllers: Some(vec![cfg.blackhole_canister_id]),
                log_visibility: Some(jupiter_ic_clients::management::LogVisibility::Public),
            },
        },
    )
    .await
    {
        set_job_status(
            target,
            RelaySetupStatus::FailedRetryable,
            Some(format!("{err:?}")),
        );
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedRetryable,
            message: format!("blackhole update_settings failed: {err:?}"),
        };
    }
    let entry = RelayRegistryEntry {
        relay_canister_id: relay_id,
        target_canister_id: target,
        kind: RelayRegistryKind::SelfService,
        status: RelayRegistryStatus::Active,
        setup_account: Some(setup_account_for(self_canister_id(), target)),
        setup_account_identifier: Some(account_identifier_text_for_account(&setup_account_for(
            self_canister_id(),
            target,
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
            job.relay_canister_id = Some(relay_id);
            job.setup_amount_processed_e8s = job.setup_amount_seen_e8s;
            job.updated_at_ts = now_secs();
        }
        st.relay_registry_by_target.insert(target, entry.clone());
        crate::rebuild_relay_targets_by_relay(st);
    });
    RelaySetupNotifyResult::Active { relay: entry }
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
    let Some(cmc_id) = cfg.cmc_canister_id else {
        return RelaySetupNotifyResult::Failed {
            status: RelaySetupStatus::FailedTerminal,
            message: "CMC canister is not configured".to_string(),
        };
    };
    let cmc = CmcCanister::new(cmc_id);
    notify_relay_setup_with_clients(target, &ledger, &index, &blackhole, &cmc).await
}

pub(crate) async fn request_relay_setup_refund_with_clients(
    target: Principal,
    ledger: &dyn LedgerClient,
    index: &dyn IndexClient,
) -> RelaySetupRefundResult {
    request_relay_setup_refund_with_clients_for_historian(self_canister_id(), target, ledger, index)
        .await
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
    let payments = match index_setup_payments(target, setup_account_identifier, index).await {
        Ok(payments) => payments,
        Err(err) => return RelaySetupRefundResult::Failed { message: err },
    };
    state::with_root_and_relay_factory_state_mut(target, |st| {
        if let Some(job) = st.relay_setup_jobs.get_mut(&target) {
            merge_payments(job, payments);
            job.status = RelaySetupStatus::Refunding;
            job.last_refund_attempt_ts = Some(now);
            job.refund_attempt_count = job.refund_attempt_count.saturating_add(1);
            job.updated_at_ts = now;
        }
    });
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
        let result = ledger
            .legacy_transfer_to_account_identifier(
                setup_account.subaccount,
                account_identifier,
                refund_amount,
                fee,
                REFUND_MEMO,
                Some(now_nanos()),
            )
            .await;
        match result {
            Ok(Ok(block)) => {
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

pub(crate) async fn request_relay_setup_refund(target: Principal) -> RelaySetupRefundResult {
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = jupiter_ic_clients::ledger::IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = jupiter_ic_clients::index::IcpIndexCanister::new(cfg.index_canister_id);
    request_relay_setup_refund_with_clients(target, &ledger, &index).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::blackhole::{BlackholeCanisterStatus, BlackholeSettings};
    use crate::clients::index::GetAccountIdentifierTransactionsResponse;
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
        transfers: Arc<Mutex<Vec<TransferArg>>>,
        legacy_results:
            Arc<Mutex<Vec<Result<jupiter_ic_clients::ledger::LegacyTransferResult, String>>>>,
        legacy_calls: Arc<Mutex<Vec<(String, u64)>>>,
    }

    impl FakeLedger {
        fn healthy(balance: u64) -> Self {
            Self {
                fee: Ok(10_000),
                balance: Ok(balance),
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
            Ok(Ok(candid::Nat::from(77u64)))
        }

        async fn legacy_transfer_to_account_identifier(
            &self,
            _from_subaccount: Option<[u8; 32]>,
            to_account_identifier_hex: String,
            amount_e8s: u64,
            _fee_e8s: u64,
            _memo: u64,
            _created_at_time_nanos: Option<u64>,
        ) -> Result<crate::clients::LegacyTransferResult, ClientError> {
            self.legacy_calls
                .lock()
                .unwrap()
                .push((to_account_identifier_hex, amount_e8s));
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
    }

    #[async_trait::async_trait]
    impl IndexClient for FakeIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, ClientError> {
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

    struct FakeCmc;

    #[async_trait::async_trait]
    impl CmcClient for FakeCmc {
        async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
            Err(ClientError::Call("not used".to_string()))
        }

        async fn notify_top_up(
            &self,
            _canister_id: Principal,
            _block_index: u64,
        ) -> Result<u128, String> {
            Ok(1_000_000_000_000)
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
            RelaySetupStatus::FailedRetryable,
            RelaySetupStatus::FailedTerminal,
            RelaySetupStatus::Ambiguous,
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
        };

        let result = block_on(request_relay_setup_refund_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
        ));
        assert!(matches!(result, RelaySetupRefundResult::Refunded { .. }));
        let calls = ledger.legacy_calls.lock().unwrap().clone();
        assert_eq!(calls, vec![("capped".to_string(), 50_000)]);
    }

    #[test]
    fn early_ledger_fee_failure_marks_reserved_job_retryable() {
        let target = Principal::from_slice(&[23]);
        state::set_state(State::new(config(), 0));
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
        };
        let result = block_on(notify_relay_setup_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
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
        };
        let result = block_on(notify_relay_setup_with_clients_for_historian(
            Principal::from_slice(&[42]),
            target,
            &ledger,
            &index,
            &FakeBlackhole,
            &FakeCmc,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::Failed {
                status: RelaySetupStatus::FailedRetryable,
                ..
            }
        ));
        assert!(ledger.transfers.lock().unwrap().is_empty());
    }
}
