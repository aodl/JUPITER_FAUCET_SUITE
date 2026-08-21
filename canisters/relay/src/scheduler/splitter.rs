use std::time::Duration;

use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};

use crate::clients::{ClientError, LedgerClient};
use crate::logic::{self, SplitterDestination, SplitterLegPlan};
use crate::scheduler::guards::SplitterGuard;
use crate::scheduler::ledger_fee::{
    record_bad_fee, resolve_icp_ledger_fee_e8s, LedgerFeeResolutionContext,
};
use crate::scheduler::logging::{emit_log_line, log_structured_error};
use crate::scheduler::transfer::{
    created_at_time_is_valid, debug_successful_transfer_injection, DebugSuccessfulTransferInjection,
};
use crate::splitter_state::{
    self, ActiveSplitterJob, SplitterLeg, SplitterLegProgress, SplitterLegStatus,
};
use crate::state;

pub(super) const SPLITTER_RETRY_INTERVAL_SECONDS: u64 = 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DriveResult {
    Completed,
    ClearedPreSpend,
    Unresolved,
    Quarantined,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MainStageResult {
    Ready,
    GuardBusy,
    Unresolved,
    Quarantined,
}

enum TransferOutcome {
    Accepted(Nat),
    BadFee(Nat),
    InsufficientFunds(Nat),
    DefinitiveRejection(String),
    TransportUncertain(ClientError),
}

pub(super) fn install_retry_timer() {
    ic_cdk_timers::set_timer_interval(
        Duration::from_secs(SPLITTER_RETRY_INTERVAL_SECONDS),
        || async {
            retry_active_splitter().await;
        },
    );
}

async fn retry_active_splitter() {
    if splitter_state::active_job().is_none() {
        return;
    }
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let ledger_id = state::with_state(|state| state.config.ledger_canister_id);
    let ledger = crate::clients::ledger::IcrcLedgerCanister::new(ledger_id);
    let _ = retry_active_splitter_with_client(
        now_nanos,
        now_secs,
        ic_cdk::api::canister_self(),
        &ledger,
    )
    .await;
}

pub(super) async fn retry_active_splitter_with_client<L: LedgerClient>(
    now_nanos: u64,
    now_secs: u64,
    relay_id: Principal,
    ledger: &L,
) -> Option<DriveResult> {
    splitter_state::active_job()?;
    let guard = SplitterGuard::acquire(now_secs)?;
    let result = drive_active_job(now_nanos, relay_id, ledger, Some(&guard)).await;
    drop(guard);
    if matches!(result, DriveResult::Completed | DriveResult::Quarantined) {
        super::tick::schedule_splitter_main_continuation_if_requested();
    }
    Some(result)
}

pub(super) async fn process_main_stage<L: LedgerClient>(
    now_nanos: u64,
    now_secs: u64,
    relay_id: Principal,
    ledger: &L,
) -> MainStageResult {
    let Some(guard) = SplitterGuard::acquire(now_secs) else {
        return MainStageResult::GuardBusy;
    };

    let mut resumed_splitter = None;
    if let Some(active) = splitter_state::active_job() {
        resumed_splitter = Some(active.plan.splitter_number);
        match drive_active_job(now_nanos, relay_id, ledger, Some(&guard)).await {
            DriveResult::Completed => {}
            DriveResult::ClearedPreSpend => return MainStageResult::Ready,
            DriveResult::Unresolved => return MainStageResult::Unresolved,
            DriveResult::Stale => return MainStageResult::GuardBusy,
            DriveResult::Quarantined => return MainStageResult::Quarantined,
        }
    }

    let fee_e8s = resolve_icp_ledger_fee_e8s(ledger, LedgerFeeResolutionContext::Splitter).await;
    if !guard.is_current() {
        return MainStageResult::GuardBusy;
    }
    let pinned_ledger_canister_id = state::with_state(|state| state.config.ledger_canister_id);
    for splitter_number in logic::SPLITTER_PERCENTAGES {
        if resumed_splitter == Some(splitter_number)
            || splitter_state::is_quarantined(splitter_number)
        {
            continue;
        }
        let source = logic::splitter_source_account(relay_id, splitter_number)
            .expect("fixed splitter definition");
        let balance_e8s = match ledger.balance_of_e8s(source).await {
            Ok(balance) => balance,
            Err(error) => {
                log_structured_error(
                    "splitter_balance_read_failed",
                    &[
                        ("splitter_number", splitter_number.to_string()),
                        ("error", error.to_string()),
                    ],
                );
                continue;
            }
        };
        if !guard.is_current() {
            return MainStageResult::GuardBusy;
        }
        let plan =
            match logic::build_splitter_plan(splitter_number, balance_e8s, fee_e8s, now_nanos) {
                Ok(plan) => plan,
                Err("splitter_no_funds" | "splitter_below_1_icp_net") => continue,
                Err(reason) => {
                    log_structured_error(
                        "splitter_plan_rejected",
                        &[
                            ("splitter_number", splitter_number.to_string()),
                            ("balance_e8s", balance_e8s.to_string()),
                            ("fee_e8s", fee_e8s.to_string()),
                            ("reason", reason.to_string()),
                        ],
                    );
                    continue;
                }
            };

        // The complete immutable plan is durable before the first ledger await.
        splitter_state::set_active_job(ActiveSplitterJob::new(pinned_ledger_canister_id, plan));
        match drive_active_job(now_nanos, relay_id, ledger, Some(&guard)).await {
            DriveResult::Completed => {}
            DriveResult::ClearedPreSpend => return MainStageResult::Ready,
            DriveResult::Unresolved => return MainStageResult::Unresolved,
            DriveResult::Stale => return MainStageResult::GuardBusy,
            DriveResult::Quarantined => return MainStageResult::Quarantined,
        }
    }
    MainStageResult::Ready
}

async fn transfer_once<L: LedgerClient>(ledger: &L, arg: TransferArg) -> TransferOutcome {
    match ledger.transfer(arg).await {
        Ok(Ok(block_index)) => TransferOutcome::Accepted(block_index),
        Ok(Err(TransferError::Duplicate { duplicate_of })) => {
            TransferOutcome::Accepted(duplicate_of)
        }
        Ok(Err(TransferError::BadFee { expected_fee })) => TransferOutcome::BadFee(expected_fee),
        Ok(Err(TransferError::InsufficientFunds { balance })) => {
            TransferOutcome::InsufficientFunds(balance)
        }
        Ok(Err(error)) => TransferOutcome::DefinitiveRejection(format!("{error:?}")),
        Err(error) => TransferOutcome::TransportUncertain(error),
    }
}

fn leg_plan(job: &ActiveSplitterJob, leg: SplitterLeg) -> &SplitterLegPlan {
    match leg {
        SplitterLeg::DefaultAccount => &job.plan.default_leg,
        SplitterLeg::SubaccountOne => &job.plan.subaccount_one_leg,
    }
}

fn leg_plan_mut(job: &mut ActiveSplitterJob, leg: SplitterLeg) -> &mut SplitterLegPlan {
    match leg {
        SplitterLeg::DefaultAccount => &mut job.plan.default_leg,
        SplitterLeg::SubaccountOne => &mut job.plan.subaccount_one_leg,
    }
}

fn leg_progress(job: &ActiveSplitterJob, leg: SplitterLeg) -> &SplitterLegProgress {
    match leg {
        SplitterLeg::DefaultAccount => &job.default_leg,
        SplitterLeg::SubaccountOne => &job.subaccount_one_leg,
    }
}

fn leg_progress_mut(job: &mut ActiveSplitterJob, leg: SplitterLeg) -> &mut SplitterLegProgress {
    match leg {
        SplitterLeg::DefaultAccount => &mut job.default_leg,
        SplitterLeg::SubaccountOne => &mut job.subaccount_one_leg,
    }
}

fn current_leg(job: &ActiveSplitterJob) -> Option<SplitterLeg> {
    if !matches!(job.default_leg.status, SplitterLegStatus::Accepted { .. }) {
        return Some(SplitterLeg::DefaultAccount);
    }
    if !matches!(
        job.subaccount_one_leg.status,
        SplitterLegStatus::Accepted { .. }
    ) {
        return Some(SplitterLeg::SubaccountOne);
    }
    None
}

fn transfer_arg(relay_id: Principal, job: &ActiveSplitterJob, leg: SplitterLeg) -> TransferArg {
    let plan = leg_plan(job, leg);
    TransferArg {
        from_subaccount: Some(job.plan.source_subaccount),
        to: logic::splitter_destination_account(relay_id, plan.destination),
        amount: Nat::from(plan.amount_e8s),
        fee: Some(Nat::from(plan.fee_e8s)),
        memo: None::<Memo>,
        created_at_time: Some(plan.created_at_time_nanos),
    }
}

fn active_plan_is_valid(job: &ActiveSplitterJob) -> bool {
    let Some(percentage) = logic::splitter_percentage(job.plan.splitter_number) else {
        return false;
    };
    let default = &job.plan.default_leg;
    let subaccount_one = &job.plan.subaccount_one_leg;
    let expected_default_gross =
        u128::from(job.plan.balance_start_e8s) * u128::from(percentage) / 100;
    percentage == job.plan.percentage_to_default
        && job.plan.source_subaccount == logic::relay_numbered_subaccount(job.plan.splitter_number)
        && default.destination == SplitterDestination::DefaultAccount
        && subaccount_one.destination == SplitterDestination::SubaccountOne
        && u128::from(default.gross_share_e8s) == expected_default_gross
        && default
            .gross_share_e8s
            .checked_add(subaccount_one.gross_share_e8s)
            == Some(job.plan.balance_start_e8s)
        && default.amount_e8s.checked_add(default.fee_e8s) == Some(default.gross_share_e8s)
        && subaccount_one
            .amount_e8s
            .checked_add(subaccount_one.fee_e8s)
            == Some(subaccount_one.gross_share_e8s)
        && default.gross_share_e8s > default.fee_e8s
        && subaccount_one.gross_share_e8s > subaccount_one.fee_e8s
        && default.created_at_time_nanos != subaccount_one.created_at_time_nanos
}

fn fresh_created_at(previous: u64, now_nanos: u64) -> Option<u64> {
    previous
        .checked_add(1)
        .map(|minimum| now_nanos.max(minimum))
}

fn repin_leg(job: &mut ActiveSplitterJob, leg: SplitterLeg, fee_e8s: u64, now_nanos: u64) -> bool {
    let plan = leg_plan_mut(job, leg);
    let Some(created_at_time_nanos) = fresh_created_at(plan.created_at_time_nanos, now_nanos)
    else {
        return false;
    };
    plan.fee_e8s = fee_e8s;
    plan.amount_e8s = plan.gross_share_e8s - fee_e8s;
    plan.created_at_time_nanos = created_at_time_nanos;
    *leg_progress_mut(job, leg) = SplitterLegProgress::default();
    true
}

fn leg_text(leg: SplitterLeg) -> &'static str {
    match leg {
        SplitterLeg::DefaultAccount => "default_account",
        SplitterLeg::SubaccountOne => "subaccount_1",
    }
}

fn quarantine(job: &ActiveSplitterJob, leg: SplitterLeg, now_nanos: u64, reason: &str) {
    let plan = leg_plan(job, leg);
    let quarantined = splitter_state::quarantine_active_job(leg, now_nanos, reason.to_string());
    if quarantined.is_none() {
        return;
    }
    log_structured_error(
        "splitter_quarantined",
        &[
            ("splitter_number", job.plan.splitter_number.to_string()),
            ("leg", leg_text(leg).to_string()),
            ("balance_start_e8s", job.plan.balance_start_e8s.to_string()),
            ("gross_share_e8s", plan.gross_share_e8s.to_string()),
            ("amount_e8s", plan.amount_e8s.to_string()),
            ("fee_e8s", plan.fee_e8s.to_string()),
            (
                "created_at_time_nanos",
                plan.created_at_time_nanos.to_string(),
            ),
            ("reason", reason.to_string()),
        ],
    );
}

fn log_completion(relay_id: Principal, job: &ActiveSplitterJob) {
    let SplitterLegStatus::Accepted {
        block_index: default_block,
    } = &job.default_leg.status
    else {
        return;
    };
    let SplitterLegStatus::Accepted {
        block_index: subaccount_one_block,
    } = &job.subaccount_one_leg.status
    else {
        return;
    };
    emit_log_line(format!(
        "RELAY_SPLITTER splitter_number={} source_owner={} source_subaccount={} balance_start_e8s={} percentage_to_default={} default_gross_e8s={} default_amount_e8s={} default_fee_e8s={} default_block_index={} subaccount_one_gross_e8s={} subaccount_one_amount_e8s={} subaccount_one_fee_e8s={} subaccount_one_block_index={} status=completed",
        job.plan.splitter_number,
        relay_id.to_text(),
        hex::encode(job.plan.source_subaccount),
        job.plan.balance_start_e8s,
        job.plan.percentage_to_default,
        job.plan.default_leg.gross_share_e8s,
        job.plan.default_leg.amount_e8s,
        job.plan.default_leg.fee_e8s,
        default_block,
        job.plan.subaccount_one_leg.gross_share_e8s,
        job.plan.subaccount_one_leg.amount_e8s,
        job.plan.subaccount_one_leg.fee_e8s,
        subaccount_one_block,
    ));
}

async fn drive_active_job<L: LedgerClient>(
    now_nanos: u64,
    relay_id: Principal,
    ledger: &L,
    guard: Option<&SplitterGuard>,
) -> DriveResult {
    loop {
        let Some(mut job) = splitter_state::active_job() else {
            return DriveResult::Completed;
        };
        let configured_ledger = state::with_state(|state| state.config.ledger_canister_id);
        if job.pinned_ledger_canister_id != configured_ledger {
            let leg = current_leg(&job).unwrap_or(SplitterLeg::DefaultAccount);
            quarantine(
                &job,
                leg,
                now_nanos,
                "pinned ledger differs from runtime config",
            );
            return DriveResult::Quarantined;
        }
        if !active_plan_is_valid(&job) {
            let leg = current_leg(&job).unwrap_or(SplitterLeg::DefaultAccount);
            quarantine(
                &job,
                leg,
                now_nanos,
                "stable splitter plan invariant failed",
            );
            return DriveResult::Quarantined;
        }
        let Some(leg) = current_leg(&job) else {
            log_completion(relay_id, &job);
            splitter_state::clear_active_job();
            return DriveResult::Completed;
        };

        match leg_progress(&job, leg).status.clone() {
            SplitterLegStatus::Accepted { .. } => unreachable!(),
            SplitterLegStatus::WaitingForFunds { .. } => {
                let Some(claimed) = splitter_state::claim_active_job(&job, None) else {
                    return DriveResult::Stale;
                };
                let source = Account {
                    owner: relay_id,
                    subaccount: Some(claimed.plan.source_subaccount),
                };
                let balance_result = ledger.balance_of_e8s(source).await;
                if guard.is_some_and(|guard| !guard.is_current()) {
                    return DriveResult::Stale;
                }
                let Some(mut job) = splitter_state::active_job_if_claim_matches(&claimed) else {
                    return DriveResult::Stale;
                };
                let balance = match balance_result {
                    Ok(balance) => balance,
                    Err(error) => {
                        log_structured_error(
                            "splitter_balance_read_failed",
                            &[
                                ("splitter_number", job.plan.splitter_number.to_string()),
                                ("leg", leg_text(leg).to_string()),
                                ("error", error.to_string()),
                            ],
                        );
                        return DriveResult::Unresolved;
                    }
                };
                if balance < leg_plan(&job, leg).gross_share_e8s {
                    leg_progress_mut(&mut job, leg).status = SplitterLegStatus::WaitingForFunds {
                        observed_balance_e8s: balance,
                    };
                    splitter_state::set_active_job(job);
                    return DriveResult::Unresolved;
                }
                let fee = leg_plan(&job, leg).fee_e8s;
                if !repin_leg(&mut job, leg, fee, now_nanos) {
                    quarantine(
                        &job,
                        leg,
                        now_nanos,
                        "replacement transfer timestamp overflow",
                    );
                    return DriveResult::Quarantined;
                }
                splitter_state::set_active_job(job);
                continue;
            }
            SplitterLegStatus::WaitingForFeasibleFee { .. } => {
                let Some(claimed) = splitter_state::claim_active_job(&job, None) else {
                    return DriveResult::Stale;
                };
                let fee =
                    resolve_icp_ledger_fee_e8s(ledger, LedgerFeeResolutionContext::Splitter).await;
                if guard.is_some_and(|guard| !guard.is_current()) {
                    return DriveResult::Stale;
                }
                let Some(mut job) = splitter_state::active_job_if_claim_matches(&claimed) else {
                    return DriveResult::Stale;
                };
                if leg_plan(&job, leg).gross_share_e8s <= fee {
                    leg_progress_mut(&mut job, leg).status =
                        SplitterLegStatus::WaitingForFeasibleFee {
                            expected_fee_e8s: Nat::from(fee),
                        };
                    splitter_state::set_active_job(job);
                    return DriveResult::Unresolved;
                }
                if !repin_leg(&mut job, leg, fee, now_nanos) {
                    quarantine(
                        &job,
                        leg,
                        now_nanos,
                        "replacement transfer timestamp overflow",
                    );
                    return DriveResult::Quarantined;
                }
                splitter_state::set_active_job(job);
                continue;
            }
            SplitterLegStatus::Ready => {}
        }

        let had_prior_uncertain_attempt = {
            let progress = leg_progress(&job, leg);
            progress.attempt_started || progress.uncertain_attempt_seen
        };
        let created_at_time_nanos = leg_plan(&job, leg).created_at_time_nanos;
        if !created_at_time_is_valid(created_at_time_nanos, now_nanos) {
            if had_prior_uncertain_attempt {
                quarantine(
                    &job,
                    leg,
                    now_nanos,
                    "expired transfer identity after a potentially accepted attempt",
                );
                return DriveResult::Quarantined;
            }
            if leg == SplitterLeg::DefaultAccount {
                splitter_state::clear_active_job();
                return DriveResult::ClearedPreSpend;
            }
            let fee = leg_plan(&job, leg).fee_e8s;
            if !repin_leg(&mut job, leg, fee, now_nanos) {
                quarantine(
                    &job,
                    leg,
                    now_nanos,
                    "replacement transfer timestamp overflow",
                );
                return DriveResult::Quarantined;
            }
            splitter_state::set_active_job(job);
            continue;
        }

        let Some(claimed) = splitter_state::claim_active_job(&job, Some(leg)) else {
            return DriveResult::Stale;
        };
        job = claimed.clone();
        let arg = transfer_arg(relay_id, &job, leg);
        let outcome = transfer_once(ledger, arg).await;
        if guard.is_some_and(|guard| !guard.is_current()) {
            return DriveResult::Stale;
        }
        let Some(mut job) = splitter_state::active_job_if_claim_matches(&claimed) else {
            return DriveResult::Stale;
        };
        match outcome {
            TransferOutcome::Accepted(block_index) => {
                match debug_successful_transfer_injection() {
                    DebugSuccessfulTransferInjection::None => {}
                    DebugSuccessfulTransferInjection::Abort => return DriveResult::Unresolved,
                    DebugSuccessfulTransferInjection::Trap => {
                        ic_cdk::trap("debug trap after successful relay splitter transfer")
                    }
                }
                leg_progress_mut(&mut job, leg).status =
                    SplitterLegStatus::Accepted { block_index };
                splitter_state::set_active_job(job);
                if super::transfer::debug_pause_after_persisted_splitter_leg() {
                    return DriveResult::Unresolved;
                }
            }
            TransferOutcome::TransportUncertain(error) => {
                leg_progress_mut(&mut job, leg).uncertain_attempt_seen = true;
                splitter_state::set_active_job(job.clone());
                log_structured_error(
                    "splitter_transfer_uncertain",
                    &[
                        ("splitter_number", job.plan.splitter_number.to_string()),
                        ("leg", leg_text(leg).to_string()),
                        ("error", error.to_string()),
                    ],
                );
                return DriveResult::Unresolved;
            }
            TransferOutcome::BadFee(expected_fee) => {
                let planned_fee = leg_plan(&job, leg).fee_e8s;
                record_bad_fee("splitter", planned_fee, &expected_fee);
                if had_prior_uncertain_attempt {
                    leg_progress_mut(&mut job, leg).uncertain_attempt_seen = true;
                    splitter_state::set_active_job(job);
                    return DriveResult::Unresolved;
                }
                if leg == SplitterLeg::DefaultAccount {
                    splitter_state::clear_active_job();
                    return DriveResult::ClearedPreSpend;
                }
                match u64::try_from(expected_fee.0.clone()) {
                    Ok(expected_fee_e8s)
                        if leg_plan(&job, leg).gross_share_e8s > expected_fee_e8s =>
                    {
                        if !repin_leg(&mut job, leg, expected_fee_e8s, now_nanos) {
                            quarantine(
                                &job,
                                leg,
                                now_nanos,
                                "replacement transfer timestamp overflow",
                            );
                            return DriveResult::Quarantined;
                        }
                    }
                    _ => {
                        let progress = leg_progress_mut(&mut job, leg);
                        progress.attempt_started = false;
                        progress.uncertain_attempt_seen = false;
                        progress.status = SplitterLegStatus::WaitingForFeasibleFee {
                            expected_fee_e8s: expected_fee,
                        };
                    }
                }
                splitter_state::set_active_job(job);
                return DriveResult::Unresolved;
            }
            TransferOutcome::InsufficientFunds(balance) => {
                if had_prior_uncertain_attempt {
                    leg_progress_mut(&mut job, leg).uncertain_attempt_seen = true;
                    splitter_state::set_active_job(job);
                    return DriveResult::Unresolved;
                }
                if leg == SplitterLeg::DefaultAccount {
                    splitter_state::clear_active_job();
                    return DriveResult::ClearedPreSpend;
                }
                let balance = u64::try_from(balance.0).unwrap_or(u64::MAX);
                *leg_progress_mut(&mut job, leg) = SplitterLegProgress {
                    status: SplitterLegStatus::WaitingForFunds {
                        observed_balance_e8s: balance,
                    },
                    attempt_started: false,
                    uncertain_attempt_seen: false,
                };
                splitter_state::set_active_job(job);
                return DriveResult::Unresolved;
            }
            TransferOutcome::DefinitiveRejection(reason) => {
                if had_prior_uncertain_attempt {
                    leg_progress_mut(&mut job, leg).uncertain_attempt_seen = true;
                    splitter_state::set_active_job(job);
                    return DriveResult::Unresolved;
                }
                if leg == SplitterLeg::DefaultAccount {
                    splitter_state::clear_active_job();
                    return DriveResult::ClearedPreSpend;
                }
                let fee = leg_plan(&job, leg).fee_e8s;
                if !repin_leg(&mut job, leg, fee, now_nanos) {
                    quarantine(
                        &job,
                        leg,
                        now_nanos,
                        "replacement transfer timestamp overflow",
                    );
                    return DriveResult::Quarantined;
                }
                splitter_state::set_active_job(job.clone());
                log_structured_error(
                    "splitter_second_leg_rejected",
                    &[
                        ("splitter_number", job.plan.splitter_number.to_string()),
                        ("reason", reason),
                    ],
                );
                return DriveResult::Unresolved;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use icrc_ledger_types::icrc1::transfer::BlockIndex;

    use super::*;
    use crate::state::{Config, State};
    use jupiter_ic_clients::cycles_probe::CyclesProbePolicy;

    #[derive(Clone)]
    enum ScriptedOutcome {
        Accepted(u64),
        Duplicate(u64),
        BadFee(u64),
        InsufficientFunds(u64),
        TooOld,
        Transport,
    }

    struct MockLedger {
        fee: Result<u64, ClientError>,
        balances: Mutex<BTreeMap<Option<[u8; 32]>, u64>>,
        outcomes: Mutex<VecDeque<ScriptedOutcome>>,
        transfers: Mutex<Vec<TransferArg>>,
    }

    impl MockLedger {
        fn new(fee: u64, outcomes: Vec<ScriptedOutcome>) -> Self {
            Self {
                fee: Ok(fee),
                balances: Mutex::new(BTreeMap::new()),
                outcomes: Mutex::new(outcomes.into()),
                transfers: Mutex::new(Vec::new()),
            }
        }

        fn set_balance(&self, subaccount: [u8; 32], balance: u64) {
            self.balances
                .lock()
                .unwrap()
                .insert(Some(subaccount), balance);
        }

        fn transfers(&self) -> Vec<TransferArg> {
            self.transfers.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl LedgerClient for MockLedger {
        async fn fee_e8s(&self) -> Result<u64, ClientError> {
            self.fee.clone()
        }

        async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
            Ok(*self
                .balances
                .lock()
                .unwrap()
                .get(&account.subaccount)
                .unwrap_or(&0))
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
            self.transfers.lock().unwrap().push(arg);
            match self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ScriptedOutcome::Accepted(99))
            {
                ScriptedOutcome::Accepted(block) => Ok(Ok(Nat::from(block))),
                ScriptedOutcome::Duplicate(block) => Ok(Err(TransferError::Duplicate {
                    duplicate_of: Nat::from(block),
                })),
                ScriptedOutcome::BadFee(fee) => Ok(Err(TransferError::BadFee {
                    expected_fee: Nat::from(fee),
                })),
                ScriptedOutcome::InsufficientFunds(balance) => {
                    Ok(Err(TransferError::InsufficientFunds {
                        balance: Nat::from(balance),
                    }))
                }
                ScriptedOutcome::TooOld => Ok(Err(TransferError::TooOld)),
                ScriptedOutcome::Transport => Err(ClientError::Call(
                    "scripted transport uncertainty".to_string(),
                )),
            }
        }
    }

    fn relay() -> Principal {
        Principal::from_slice(&[9])
    }

    fn ledger_id() -> Principal {
        Principal::from_slice(&[8])
    }

    fn config() -> Config {
        Config {
            managed_canisters: Vec::new(),
            ledger_canister_id: ledger_id(),
            cmc_canister_id: Principal::from_slice(&[2]),
            governance_canister_id: Principal::from_slice(&[3]),
            blackhole_canister_id: Principal::from_slice(&[4]),
            sns_rewards_canister_id: Principal::from_slice(&[5]),
            icp_index_canister_id: Principal::from_slice(&[6]),
            cycles_probe_policy: CyclesProbePolicy::Auto,
            main_interval_seconds: 86_400,
            max_transfers_per_tick: Some(1),
            surplus_recipients: Vec::new(),
        }
    }

    fn reset() {
        state::set_state(State::new(config(), 0));
        splitter_state::reset_for_test();
        crate::scheduler::logging::TEST_LOG_LINES.with(|lines| lines.borrow_mut().clear());
    }

    fn block_on<F: Future>(mut future: F) -> F::Output {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        let mut future = unsafe { Pin::new_unchecked(&mut future) };
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn active_job(number: u8, balance: u64, fee: u64, now: u64) -> ActiveSplitterJob {
        ActiveSplitterJob::new(
            ledger_id(),
            logic::build_splitter_plan(number, balance, fee, now).unwrap(),
        )
    }

    struct DeferredLedger {
        release_transfer: AtomicBool,
        transfer: Mutex<Option<TransferArg>>,
    }

    struct TakeoverLedger {
        release_first_transfer: AtomicBool,
        transfers: Mutex<Vec<TransferArg>>,
    }

    #[async_trait::async_trait]
    impl LedgerClient for TakeoverLedger {
        async fn fee_e8s(&self) -> Result<u64, ClientError> {
            Ok(10_000)
        }

        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, ClientError> {
            Ok(0)
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
            let call_index = {
                let mut transfers = self.transfers.lock().unwrap();
                let call_index = transfers.len();
                transfers.push(arg);
                call_index
            };
            if call_index == 0 {
                std::future::poll_fn(|_| {
                    if self.release_first_transfer.load(Ordering::SeqCst) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                return Ok(Ok(Nat::from(1_u64)));
            }
            if call_index == 1 {
                return Ok(Err(TransferError::Duplicate {
                    duplicate_of: Nat::from(1_u64),
                }));
            }
            Ok(Ok(Nat::from(2_u64)))
        }
    }

    #[async_trait::async_trait]
    impl LedgerClient for DeferredLedger {
        async fn fee_e8s(&self) -> Result<u64, ClientError> {
            Ok(10_000)
        }

        async fn balance_of_e8s(&self, account: Account) -> Result<u64, ClientError> {
            Ok(
                if account.subaccount == Some(logic::relay_numbered_subaccount(30)) {
                    500_000_007
                } else {
                    0
                },
            )
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
            *self.transfer.lock().unwrap() = Some(arg);
            std::future::poll_fn(|_| {
                if self.release_transfer.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            Ok(Ok(Nat::from(1_u64)))
        }
    }

    #[test]
    fn complete_two_leg_plan_is_stable_before_first_transfer_future_completes() {
        reset();
        let ledger = DeferredLedger {
            release_transfer: AtomicBool::new(false),
            transfer: Mutex::new(None),
        };
        let mut future = Box::pin(process_main_stage(100, 1, relay(), &ledger));
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);

        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        let active = splitter_state::active_job().expect("plan is stable before ledger response");
        assert_eq!(active.plan.splitter_number, 30);
        assert_eq!(active.plan.balance_start_e8s, 500_000_007);
        assert!(active.default_leg.attempt_started);
        assert!(!active.subaccount_one_leg.attempt_started);
        assert_eq!(active.plan.default_leg.created_at_time_nanos, 100);
        assert_eq!(active.plan.subaccount_one_leg.created_at_time_nanos, 101);
        assert_eq!(
            active.plan.default_leg.gross_share_e8s
                + active.plan.subaccount_one_leg.gross_share_e8s,
            active.plan.balance_start_e8s
        );
        assert_eq!(
            *ledger.transfer.lock().unwrap(),
            Some(transfer_arg(relay(), &active, SplitterLeg::DefaultAccount))
        );

        ledger.release_transfer.store(true, Ordering::SeqCst);
        assert_eq!(block_on(future), MainStageResult::Ready);
        assert!(splitter_state::active_job().is_none());
    }

    #[test]
    fn expired_driver_callback_cannot_regress_a_takeover_that_completed_the_job() {
        reset();
        let ledger = TakeoverLedger {
            release_first_transfer: AtomicBool::new(false),
            transfers: Mutex::new(Vec::new()),
        };
        splitter_state::set_active_job(active_job(30, 500_000_007, 10_000, 100));

        let mut driver_a = Box::pin(process_main_stage(100, 1, relay(), &ledger));
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            driver_a.as_mut().poll(&mut context),
            Poll::Pending
        ));
        let revision_a = splitter_state::active_job().unwrap().driver_revision;

        assert_eq!(
            block_on(process_main_stage(
                101,
                1 + super::super::guards::SPLITTER_LEASE_SECONDS + 1,
                relay(),
                &ledger,
            )),
            MainStageResult::Ready
        );
        assert!(splitter_state::active_job().is_none());

        ledger.release_first_transfer.store(true, Ordering::SeqCst);
        assert_eq!(block_on(driver_a), MainStageResult::GuardBusy);
        assert!(splitter_state::active_job().is_none());
        let transfers = ledger.transfers.lock().unwrap();
        assert_eq!(transfers.len(), 3);
        assert_eq!(
            transfers[0], transfers[1],
            "takeover retries exact identity"
        );
        assert_ne!(transfers[1].created_at_time, transfers[2].created_at_time);
        assert!(revision_a > 0);
        assert!(splitter_state::get().next_driver_revision > revision_a);
    }

    #[test]
    fn expired_driver_callback_cannot_resurrect_a_job_quarantined_by_takeover() {
        reset();
        let ledger = TakeoverLedger {
            release_first_transfer: AtomicBool::new(false),
            transfers: Mutex::new(Vec::new()),
        };
        splitter_state::set_active_job(active_job(30, 500_000_007, 10_000, 100));
        let mut driver_a = Box::pin(process_main_stage(100, 1, relay(), &ledger));
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            driver_a.as_mut().poll(&mut context),
            Poll::Pending
        ));

        let expired_nanos = 25 * 60 * 60 * 1_000_000_000;
        assert_eq!(
            block_on(process_main_stage(
                expired_nanos,
                1 + super::super::guards::SPLITTER_LEASE_SECONDS + 1,
                relay(),
                &ledger,
            )),
            MainStageResult::Quarantined
        );
        assert!(splitter_state::is_quarantined(30));

        ledger.release_first_transfer.store(true, Ordering::SeqCst);
        assert_eq!(block_on(driver_a), MainStageResult::GuardBusy);
        assert!(splitter_state::active_job().is_none());
        assert!(splitter_state::is_quarantined(30));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);
    }

    #[test]
    fn accepted_and_duplicate_paths_are_ordered_and_complete() {
        for outcomes in [
            vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Accepted(2)],
            vec![ScriptedOutcome::Duplicate(1), ScriptedOutcome::Accepted(2)],
            vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Duplicate(2)],
        ] {
            reset();
            let ledger = MockLedger::new(10_000, outcomes);
            splitter_state::set_active_job(active_job(30, 500_000_007, 10_000, 10));
            assert_eq!(
                block_on(drive_active_job(11, relay(), &ledger, None)),
                DriveResult::Completed
            );
            let transfers = ledger.transfers();
            assert_eq!(transfers.len(), 2);
            assert_eq!(transfers[0].to.subaccount, None);
            assert_eq!(
                transfers[1].to.subaccount,
                Some(logic::relay_subaccount_one())
            );
            assert!(splitter_state::active_job().is_none());
        }
    }

    #[test]
    fn transport_uncertainty_retries_byte_identical_identity_and_duplicate_advances() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![
                ScriptedOutcome::Transport,
                ScriptedOutcome::Duplicate(1),
                ScriptedOutcome::Accepted(2),
            ],
        );
        splitter_state::set_active_job(active_job(50, 500_000_007, 10_000, 10));
        assert_eq!(
            block_on(drive_active_job(11, relay(), &ledger, None)),
            DriveResult::Unresolved
        );
        let pinned = splitter_state::active_job().unwrap();
        assert!(pinned.default_leg.attempt_started);
        assert!(pinned.default_leg.uncertain_attempt_seen);
        assert_eq!(
            block_on(drive_active_job(12, relay(), &ledger, None)),
            DriveResult::Completed
        );
        let transfers = ledger.transfers();
        assert_eq!(transfers.len(), 3);
        assert_eq!(transfers[0], transfers[1]);
        assert_ne!(transfers[1].created_at_time, transfers[2].created_at_time);
    }

    #[test]
    fn first_bad_fee_clears_without_spend_and_second_bad_fee_repins_only_complement() {
        reset();
        let first_bad_fee = MockLedger::new(10_000, vec![ScriptedOutcome::BadFee(20_000)]);
        splitter_state::set_active_job(active_job(30, 500_000_007, 10_000, 10));
        assert_eq!(
            block_on(drive_active_job(11, relay(), &first_bad_fee, None)),
            DriveResult::ClearedPreSpend
        );
        assert!(splitter_state::active_job().is_none());

        reset();
        let second_bad_fee = MockLedger::new(
            10_000,
            vec![
                ScriptedOutcome::Accepted(1),
                ScriptedOutcome::BadFee(20_000),
            ],
        );
        splitter_state::set_active_job(active_job(30, 500_000_007, 10_000, 10));
        assert_eq!(
            block_on(drive_active_job(11, relay(), &second_bad_fee, None)),
            DriveResult::Unresolved
        );
        let active = splitter_state::active_job().unwrap();
        assert!(matches!(
            active.default_leg.status,
            SplitterLegStatus::Accepted { .. }
        ));
        assert_eq!(active.plan.default_leg.fee_e8s, 10_000);
        assert_eq!(active.plan.subaccount_one_leg.fee_e8s, 20_000);
        assert_eq!(
            active.plan.subaccount_one_leg.amount_e8s + 20_000,
            active.plan.subaccount_one_leg.gross_share_e8s
        );
        assert_eq!(second_bad_fee.transfers().len(), 2);
    }

    #[test]
    fn uncertain_bad_fee_never_repins_and_expired_identity_quarantines_only_that_splitter() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![ScriptedOutcome::Transport, ScriptedOutcome::BadFee(20_000)],
        );
        splitter_state::set_active_job(active_job(30, 500_000_007, 10_000, 10));
        assert_eq!(
            block_on(drive_active_job(11, relay(), &ledger, None)),
            DriveResult::Unresolved
        );
        let identity = splitter_state::active_job()
            .unwrap()
            .plan
            .default_leg
            .clone();
        assert_eq!(
            block_on(drive_active_job(12, relay(), &ledger, None)),
            DriveResult::Unresolved
        );
        assert_eq!(
            splitter_state::active_job().unwrap().plan.default_leg,
            identity
        );
        let expired = 10 + 24 * 60 * 60 * 1_000_000_000 + 1;
        assert_eq!(
            block_on(drive_active_job(expired, relay(), &ledger, None)),
            DriveResult::Quarantined
        );
        assert!(splitter_state::is_quarantined(30));
        assert!(!splitter_state::is_quarantined(40));
        assert!(splitter_state::active_job().is_none());
    }

    #[test]
    fn second_insufficient_funds_preserves_gross_and_uses_only_later_required_funds() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![
                ScriptedOutcome::Accepted(1),
                ScriptedOutcome::InsufficientFunds(1),
                ScriptedOutcome::Accepted(2),
            ],
        );
        let job = active_job(30, 500_000_007, 10_000, 10);
        let second_gross = job.plan.subaccount_one_leg.gross_share_e8s;
        splitter_state::set_active_job(job);
        assert_eq!(
            block_on(drive_active_job(11, relay(), &ledger, None)),
            DriveResult::Unresolved
        );
        let active = splitter_state::active_job().unwrap();
        assert_eq!(active.plan.subaccount_one_leg.gross_share_e8s, second_gross);
        ledger.set_balance(active.plan.source_subaccount, second_gross + 123_456);
        assert_eq!(
            block_on(drive_active_job(20, relay(), &ledger, None)),
            DriveResult::Completed
        );
        let transfers = ledger.transfers();
        let replacement = transfers.last().unwrap();
        assert_eq!(
            u64::try_from(replacement.amount.0.clone()).unwrap() + 10_000,
            second_gross
        );
    }

    #[test]
    fn definitive_second_rejection_never_recomputes_percentage_from_residual_balance() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![
                ScriptedOutcome::Accepted(1),
                ScriptedOutcome::TooOld,
                ScriptedOutcome::Accepted(2),
            ],
        );
        splitter_state::set_active_job(active_job(90, 500_000_007, 10_000, 10));
        assert_eq!(
            block_on(drive_active_job(11, relay(), &ledger, None)),
            DriveResult::Unresolved
        );
        let repinned = splitter_state::active_job().unwrap();
        let gross = repinned.plan.subaccount_one_leg.gross_share_e8s;
        assert_eq!(
            block_on(drive_active_job(12, relay(), &ledger, None)),
            DriveResult::Completed
        );
        let last = ledger.transfers().pop().unwrap();
        assert_eq!(u64::try_from(last.amount.0).unwrap() + 10_000, gross);
    }

    #[test]
    fn max_transfer_limit_does_not_limit_two_leg_splitter_stage() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Accepted(2)],
        );
        ledger.set_balance(logic::relay_numbered_subaccount(10), 200_000_000);
        assert_eq!(
            block_on(process_main_stage(10, 10, relay(), &ledger)),
            MainStageResult::Ready
        );
        assert_eq!(ledger.transfers().len(), 2);
        assert_eq!(
            state::with_state(|state| state.config.max_transfers_per_tick),
            Some(1)
        );
    }

    #[test]
    fn scans_all_fixed_splitters_in_order_and_skips_below_threshold_or_quarantine() {
        reset();
        let outcomes = (1_u64..=14).map(ScriptedOutcome::Accepted).collect();
        let ledger = MockLedger::new(10_000, outcomes);
        for number in logic::SPLITTER_PERCENTAGES {
            ledger.set_balance(
                logic::relay_numbered_subaccount(number),
                if number == 20 { 1 } else { 200_000_000 },
            );
        }
        let quarantined = active_job(40, 200_000_000, 10_000, 1);
        splitter_state::set_active_job(quarantined);
        splitter_state::quarantine_active_job(
            SplitterLeg::DefaultAccount,
            2,
            "test quarantine".to_string(),
        );

        assert_eq!(
            block_on(process_main_stage(10, 10, relay(), &ledger)),
            MainStageResult::Ready
        );
        let sources: Vec<u8> = ledger
            .transfers()
            .chunks_exact(2)
            .map(|pair| pair[0].from_subaccount.unwrap()[31])
            .collect();
        assert_eq!(sources, [10, 30, 50, 60, 70, 80, 90]);
    }

    #[test]
    fn every_healthy_splitter_completes_eighteen_transfers_in_one_scan_despite_limit_one() {
        reset();
        let outcomes = (1_u64..=18).map(ScriptedOutcome::Accepted).collect();
        let ledger = MockLedger::new(10_000, outcomes);
        for number in logic::SPLITTER_PERCENTAGES {
            ledger.set_balance(logic::relay_numbered_subaccount(number), 200_000_000);
        }

        assert_eq!(
            block_on(process_main_stage(10, 10, relay(), &ledger)),
            MainStageResult::Ready
        );
        let transfers = ledger.transfers();
        assert_eq!(transfers.len(), 18);
        let sources = transfers
            .chunks_exact(2)
            .map(|pair| pair[0].from_subaccount.unwrap()[31])
            .collect::<Vec<_>>();
        assert_eq!(sources, logic::SPLITTER_PERCENTAGES);
        assert_eq!(
            state::with_state(|state| state.config.max_transfers_per_tick),
            Some(1)
        );
    }

    #[test]
    fn held_splitter_lease_blocks_competing_driver_and_stale_lease_recovers() {
        reset();
        let guard = SplitterGuard::acquire(100).expect("first driver acquires lease");
        let ledger = MockLedger::new(10_000, Vec::new());
        assert_eq!(
            block_on(process_main_stage(10, 100, relay(), &ledger)),
            MainStageResult::GuardBusy
        );
        drop(guard);
        assert_eq!(
            block_on(process_main_stage(10, 100, relay(), &ledger)),
            MainStageResult::Ready
        );

        state::with_state_mut(|state| state.splitter_lock_state_ts = Some(100));
        assert_eq!(
            block_on(process_main_stage(10, 101, relay(), &ledger)),
            MainStageResult::Ready
        );
    }

    #[test]
    fn active_job_resumes_before_new_scan_and_unresolved_work_blocks_new_plans() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![
                ScriptedOutcome::Accepted(1),
                ScriptedOutcome::Accepted(2),
                ScriptedOutcome::Accepted(3),
                ScriptedOutcome::Accepted(4),
            ],
        );
        ledger.set_balance(logic::relay_numbered_subaccount(10), 200_000_000);
        splitter_state::set_active_job(active_job(50, 200_000_000, 10_000, 10));
        assert_eq!(
            block_on(process_main_stage(11, 11, relay(), &ledger)),
            MainStageResult::Ready
        );
        let sources: Vec<u8> = ledger
            .transfers()
            .chunks_exact(2)
            .map(|pair| pair[0].from_subaccount.unwrap()[31])
            .collect();
        assert_eq!(sources, [50, 10]);

        reset();
        let ledger = MockLedger::new(10_000, vec![ScriptedOutcome::Transport]);
        ledger.set_balance(logic::relay_numbered_subaccount(10), 200_000_000);
        splitter_state::set_active_job(active_job(50, 200_000_000, 10_000, 10));
        assert_eq!(
            block_on(process_main_stage(11, 11, relay(), &ledger)),
            MainStageResult::Unresolved
        );
        assert_eq!(ledger.transfers().len(), 1);
        assert_eq!(ledger.transfers()[0].from_subaccount.unwrap()[31], 50);
    }

    #[test]
    fn splitter_fee_resolution_uses_live_cached_and_bootstrap_sources_with_context() {
        reset();
        let live = MockLedger::new(
            20_000,
            vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Accepted(2)],
        );
        live.set_balance(logic::relay_numbered_subaccount(10), 200_000_000);
        assert_eq!(
            block_on(process_main_stage(10, 10, relay(), &live)),
            MainStageResult::Ready
        );
        assert_eq!(live.transfers()[0].fee, Some(Nat::from(20_000_u64)));
        assert_eq!(
            state::with_state(|state| state.last_known_ledger_fee_e8s),
            Some(20_000)
        );

        reset();
        state::with_state_mut(|state| state.last_known_ledger_fee_e8s = Some(30_000));
        let cached = MockLedger {
            fee: Err(ClientError::Call("fee unavailable".to_string())),
            balances: Mutex::new(BTreeMap::from([(
                Some(logic::relay_numbered_subaccount(10)),
                200_000_000,
            )])),
            outcomes: Mutex::new(
                vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Accepted(2)].into(),
            ),
            transfers: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(process_main_stage(10, 10, relay(), &cached)),
            MainStageResult::Ready
        );
        assert_eq!(cached.transfers()[0].fee, Some(Nat::from(30_000_u64)));
        let cached_logs =
            crate::scheduler::logging::TEST_LOG_LINES.with(|lines| lines.borrow().join("\n"));
        assert!(cached_logs.contains("context=splitter"));
        assert!(cached_logs.contains("fallback_source=cached"));

        reset();
        let bootstrap = MockLedger {
            fee: Err(ClientError::Call("fee unavailable".to_string())),
            balances: Mutex::new(BTreeMap::from([(
                Some(logic::relay_numbered_subaccount(10)),
                200_000_000,
            )])),
            outcomes: Mutex::new(
                vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Accepted(2)].into(),
            ),
            transfers: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(process_main_stage(10, 10, relay(), &bootstrap)),
            MainStageResult::Ready
        );
        assert_eq!(bootstrap.transfers()[0].fee, Some(Nat::from(10_000_u64)));
        let bootstrap_logs =
            crate::scheduler::logging::TEST_LOG_LINES.with(|lines| lines.borrow().join("\n"));
        assert!(bootstrap_logs.contains("context=splitter"));
        assert!(bootstrap_logs.contains("fallback_source=bootstrap"));
    }

    #[test]
    fn hourly_retry_is_narrow_and_noops_without_active_state() {
        reset();
        assert_eq!(SPLITTER_RETRY_INTERVAL_SECONDS, 3_600);
        block_on(retry_active_splitter());
        assert!(splitter_state::active_job().is_none());
    }

    #[test]
    fn hourly_retry_only_finishes_the_pinned_job_and_requests_normal_continuation() {
        reset();
        let ledger = MockLedger::new(
            10_000,
            vec![ScriptedOutcome::Accepted(1), ScriptedOutcome::Accepted(2)],
        );
        ledger.set_balance(logic::relay_numbered_subaccount(10), 500_000_007);
        splitter_state::set_active_job(active_job(50, 500_000_007, 10_000, 10));
        state::with_state_mut(|state| state.splitter_main_continuation_requested = true);

        assert_eq!(
            block_on(retry_active_splitter_with_client(11, 11, relay(), &ledger)),
            Some(DriveResult::Completed)
        );
        assert!(splitter_state::active_job().is_none());
        let transfers = ledger.transfers();
        assert_eq!(transfers.len(), 2);
        assert!(transfers
            .iter()
            .all(|transfer| transfer.from_subaccount.unwrap()[31] == 50));
        assert!(state::with_state(|state| {
            state.splitter_main_continuation_requested
                && state.splitter_main_continuation_timer_pending
        }));
    }
}
