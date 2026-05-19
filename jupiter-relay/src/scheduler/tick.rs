use std::time::Duration;

use icrc_ledger_types::icrc1::account::Account;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{BlackholeClient, CmcClient, LedgerClient};
use crate::logic::{self, RawIcpPlan};
use crate::scheduler::cycles_probe::probe_cycles;
use crate::scheduler::guards::MainGuard;
use crate::scheduler::logging::{
    log_cycles_and_config, log_error, log_info, log_raw_recipients, log_summary,
};
use crate::scheduler::transfer::drive_pending_transfer;
use crate::state::{
    self, ActiveRelayJob, ActiveRelayMode, CanisterBurnSample, PendingTransfer,
    PendingTransferKind, PendingTransferPhase, RelayMode, RelaySummary,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferPlanStep {
    Planned,
    Paused,
    Done,
}

pub(crate) fn install_timers() {
    let main_s = state::with_state(|st| st.config.main_interval_seconds.max(60));
    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s), || async {
        main_tick(false).await;
    });
}

pub(crate) fn schedule_immediate_resume_if_needed() {
    let has_active_job = state::with_state(|st| st.active_job.is_some());
    if has_active_job {
        ic_cdk_timers::set_timer(Duration::from_secs(1), async {
            main_tick(true).await;
        });
    }
}

#[cfg(feature = "debug_api")]
pub(crate) async fn debug_main_tick_impl() {
    main_tick(true).await;
}

async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let blackhole = BlackholeCanister::new(cfg.blackhole_canister_id);
    run_main_tick_with_clients(force, now_nanos, now_secs, &ledger, &cmc, &blackhole).await;
}

pub(crate) async fn run_main_tick_with_clients<
    L: LedgerClient,
    C: CmcClient,
    B: BlackholeClient,
>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    cmc: &C,
    blackhole: &B,
) {
    let Some(guard) = MainGuard::acquire(now_secs) else {
        return;
    };
    if !force {
        let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
        let recently_ran =
            state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            guard.finish(now_secs);
            return;
        }
    }

    log_cycles_and_config();
    if !resume_or_start_job(now_nanos, ledger, cmc, blackhole).await {
        log_error("relay tick stopped after debug transfer injection");
        guard.finish(now_secs);
        return;
    }
    guard.finish(now_secs);
}

async fn resume_or_start_job<L: LedgerClient, C: CmcClient, B: BlackholeClient>(
    now_nanos: u64,
    ledger: &L,
    cmc: &C,
    blackhole: &B,
) -> bool {
    if state::with_state(|st| st.active_job.is_none()) {
        start_job(now_nanos, ledger, blackhole).await;
    }
    drive_active_job(now_nanos, ledger, cmc).await
}

async fn start_job<L: LedgerClient, B: BlackholeClient>(now_nanos: u64, ledger: &L, blackhole: &B) {
    let cfg = state::with_state(|st| st.config.clone());
    let self_id = ic_cdk::api::canister_self();
    let managed = logic::effective_managed_canisters(&cfg.managed_canisters, self_id);
    let (current_cycles, probe_failures) =
        probe_cycles(&managed, self_id, now_nanos, blackhole).await;
    let min_cycles = current_cycles
        .values()
        .map(|snapshot| snapshot.cycles)
        .min();

    if !probe_failures.is_empty() {
        let mut summary =
            RelaySummary::started(RelayMode::Degraded, now_nanos, managed.len() as u32);
        summary.completed_at_ts_nanos = Some(now_nanos);
        summary.min_cycles_balance = min_cycles;
        summary.probe_failures = probe_failures;
        log_summary(&summary);
        state::with_state_mut(|st| st.last_summary = Some(summary));
        return;
    }

    let has_complete_previous = state::with_state(|st| {
        managed
            .iter()
            .all(|id| st.last_completed_cycles.contains_key(id))
    });
    if !has_complete_previous {
        let mut summary =
            RelaySummary::started(RelayMode::BaselineOnly, now_nanos, managed.len() as u32);
        summary.completed_at_ts_nanos = Some(now_nanos);
        summary.min_cycles_balance = min_cycles;
        log_summary(&summary);
        state::with_state_mut(|st| {
            st.last_completed_cycles = current_cycles;
            st.last_summary = Some(summary);
        });
        log_info("stored baseline cycles sample");
        return;
    }

    let default_account = logic::default_account(self_id);
    let balance = match ledger.balance_of_e8s(default_account).await {
        Ok(v) => v,
        Err(err) => {
            let mut summary =
                RelaySummary::started(RelayMode::Degraded, now_nanos, managed.len() as u32);
            summary.completed_at_ts_nanos = Some(now_nanos);
            summary.min_cycles_balance = min_cycles;
            summary.probe_failures.push(crate::state::ProbeFailure {
                canister_id: cfg.ledger_canister_id,
                error: format!("balance read failed: {err}"),
            });
            log_summary(&summary);
            state::with_state_mut(|st| st.last_summary = Some(summary));
            return;
        }
    };
    let fee = match ledger.fee_e8s().await {
        Ok(v) => v,
        Err(err) => {
            let mut summary =
                RelaySummary::started(RelayMode::Degraded, now_nanos, managed.len() as u32);
            summary.completed_at_ts_nanos = Some(now_nanos);
            summary.default_account_balance_start_e8s = balance;
            summary.min_cycles_balance = min_cycles;
            summary.probe_failures.push(crate::state::ProbeFailure {
                canister_id: cfg.ledger_canister_id,
                error: format!("fee read failed: {err}"),
            });
            log_summary(&summary);
            state::with_state_mut(|st| st.last_summary = Some(summary));
            return;
        }
    };

    if balance == 0 {
        let mut summary =
            RelaySummary::started(RelayMode::NoFunds, now_nanos, managed.len() as u32);
        summary.completed_at_ts_nanos = Some(now_nanos);
        summary.default_account_balance_start_e8s = balance;
        summary.fee_e8s = fee;
        summary.min_cycles_balance = min_cycles;
        log_summary(&summary);
        state::with_state_mut(|st| {
            st.last_completed_cycles = current_cycles;
            st.last_summary = Some(summary);
        });
        return;
    }

    let raw_active = cfg
        .raw_icp_mode
        .as_ref()
        .map(|raw| logic::raw_mode_active(min_cycles, raw.min_cycles_threshold, true, false))
        .unwrap_or(false);

    let previous = state::with_state(|st| st.last_completed_cycles.clone());
    let burn_plan = logic::build_burn_plan(&current_cycles, &previous, balance, fee);
    let canisters = burn_plan
        .iter()
        .map(CanisterBurnSample::from)
        .collect::<Vec<_>>();
    let total_burn_cycles = canisters.iter().map(|sample| sample.burn_cycles).sum();

    let id = state::with_state_mut(|st| {
        let id = st.next_job_id;
        st.next_job_id = st.next_job_id.saturating_add(1);
        id
    });

    if raw_active {
        let raw = cfg.raw_icp_mode.expect("raw mode configured when active");
        let raw_plans =
            logic::allocate_equal_raw_icp_shares(&raw.recipients, default_account, balance, fee);
        let mut summary = RelaySummary::started(RelayMode::RawIcp, now_nanos, managed.len() as u32);
        summary.default_account_balance_start_e8s = balance;
        summary.fee_e8s = fee;
        summary.min_cycles_balance = min_cycles;
        summary.total_burn_cycles = total_burn_cycles;
        summary.canisters = canisters.clone();
        summary.planned_retained_e8s = retained_raw_e8s(balance, &raw_plans);
        summary.known_unspent_e8s = summary.planned_retained_e8s;
        let job = ActiveRelayJob {
            id,
            mode: ActiveRelayMode::RawIcp,
            started_at_ts_nanos: now_nanos,
            fee_e8s: fee,
            balance_start_e8s: balance,
            current_cycles,
            canisters,
            raw_recipients: raw.recipients,
            pending_transfer: None,
            next_transfer_index: 0,
            next_created_at_time_nanos: now_nanos,
            summary,
        };
        state::with_state_mut(|st| st.active_job = Some(job));
    } else {
        let mut summary =
            RelaySummary::started(RelayMode::CyclesTopUp, now_nanos, managed.len() as u32);
        summary.default_account_balance_start_e8s = balance;
        summary.fee_e8s = fee;
        summary.min_cycles_balance = min_cycles;
        summary.total_burn_cycles = total_burn_cycles;
        summary.canisters = canisters.clone();
        summary.planned_retained_e8s = retained_topup_e8s(balance, &canisters);
        summary.known_unspent_e8s = summary.planned_retained_e8s;
        let job = ActiveRelayJob {
            id,
            mode: ActiveRelayMode::CyclesTopUp,
            started_at_ts_nanos: now_nanos,
            fee_e8s: fee,
            balance_start_e8s: balance,
            current_cycles,
            canisters,
            raw_recipients: Vec::new(),
            pending_transfer: None,
            next_transfer_index: 0,
            next_created_at_time_nanos: now_nanos,
            summary,
        };
        state::with_state_mut(|st| st.active_job = Some(job));
    }
}

async fn drive_active_job<L: LedgerClient, C: CmcClient>(
    now_nanos: u64,
    ledger: &L,
    cmc: &C,
) -> bool {
    let max_transfers_this_tick =
        state::with_state(|st| st.config.max_transfers_per_tick.map(u32::from));
    let mut transfers_started_this_tick = 0_u32;
    loop {
        if state::with_state(|st| st.active_job.is_none()) {
            return true;
        }
        if state::with_state(|st| st.active_job.as_ref().unwrap().pending_transfer.is_some()) {
            let cmc_id = state::with_state(|st| st.config.cmc_canister_id);
            if !drive_pending_transfer(ledger, cmc, cmc_id, now_nanos).await {
                return false;
            }
            continue;
        }
        let plan_step = state::with_state_mut(|st| {
            let job = st.active_job.as_mut().expect("active job");
            plan_next_transfer(
                job,
                max_transfers_this_tick,
                &mut transfers_started_this_tick,
            )
        });
        match plan_step {
            TransferPlanStep::Planned => continue,
            TransferPlanStep::Paused => {
                log_active_job_summary();
                return true;
            }
            TransferPlanStep::Done => {}
        }
        complete_job(now_nanos);
        return true;
    }
}

fn plan_next_transfer(
    job: &mut ActiveRelayJob,
    max_transfers_this_tick: Option<u32>,
    transfers_started_this_tick: &mut u32,
) -> TransferPlanStep {
    if max_transfers_this_tick
        .map(|limit| *transfers_started_this_tick >= limit)
        .unwrap_or(false)
    {
        job.summary.partial_tick_count = job.summary.partial_tick_count.saturating_add(1);
        return TransferPlanStep::Paused;
    }
    let planned = match job.mode {
        ActiveRelayMode::CyclesTopUp => next_topup_pending(job),
        ActiveRelayMode::RawIcp => next_raw_pending(job),
    };
    if planned.is_some() {
        *transfers_started_this_tick = transfers_started_this_tick.saturating_add(1);
        TransferPlanStep::Planned
    } else {
        TransferPlanStep::Done
    }
}

fn next_topup_pending(job: &mut ActiveRelayJob) -> Option<()> {
    while (job.next_transfer_index as usize) < job.canisters.len() {
        let index = job.next_transfer_index as usize;
        job.next_transfer_index = job.next_transfer_index.saturating_add(1);
        let sample = &job.canisters[index];
        if sample.amount_e8s == 0 {
            continue;
        }
        let created_at = job.next_created_at_time_nanos;
        job.next_created_at_time_nanos = job.next_created_at_time_nanos.saturating_add(1);
        job.pending_transfer = Some(PendingTransfer {
            kind: PendingTransferKind::CmcTopUp {
                canister_id: sample.canister_id,
            },
            gross_share_e8s: sample.gross_share_e8s,
            amount_e8s: sample.amount_e8s,
            created_at_time_nanos: created_at,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        return Some(());
    }
    None
}

fn next_raw_pending(job: &mut ActiveRelayJob) -> Option<()> {
    let default_account = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: None,
    };
    let plans = logic::allocate_equal_raw_icp_shares(
        &job.raw_recipients,
        default_account,
        job.balance_start_e8s,
        job.fee_e8s,
    );
    while (job.next_transfer_index as usize) < plans.len() {
        let index = job.next_transfer_index as usize;
        job.next_transfer_index = job.next_transfer_index.saturating_add(1);
        let plan = &plans[index];
        if plan.amount_e8s == 0 {
            continue;
        }
        let created_at = job.next_created_at_time_nanos;
        job.next_created_at_time_nanos = job.next_created_at_time_nanos.saturating_add(1);
        job.pending_transfer = Some(PendingTransfer {
            kind: PendingTransferKind::RawIcp {
                account: plan.recipient.account,
                memo: plan.recipient.memo.clone(),
            },
            gross_share_e8s: plan.gross_share_e8s,
            amount_e8s: plan.amount_e8s,
            created_at_time_nanos: created_at,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        return Some(());
    }
    None
}

fn complete_job(now_nanos: u64) {
    state::with_state_mut(|st| {
        let Some(mut job) = st.active_job.take() else {
            return;
        };
        job.summary.completed_at_ts_nanos = Some(now_nanos);
        log_summary(&job.summary);
        if matches!(job.mode, ActiveRelayMode::RawIcp) {
            log_raw_recipients(
                &job.raw_recipients,
                ic_cdk::api::canister_self(),
                job.balance_start_e8s,
                job.fee_e8s,
            );
        }
        st.last_completed_cycles = job.current_cycles;
        st.last_summary = Some(job.summary);
    });
}

fn log_active_job_summary() {
    state::with_state(|st| {
        if let Some(job) = &st.active_job {
            log_summary(&job.summary);
            if matches!(job.mode, ActiveRelayMode::RawIcp) {
                log_raw_recipients(
                    &job.raw_recipients,
                    ic_cdk::api::canister_self(),
                    job.balance_start_e8s,
                    job.fee_e8s,
                );
            }
        }
    });
}

fn retained_topup_e8s(balance: u64, canisters: &[CanisterBurnSample]) -> u64 {
    let gross: u64 = canisters
        .iter()
        .filter(|sample| sample.amount_e8s > 0)
        .map(|sample| sample.gross_share_e8s)
        .sum();
    balance.saturating_sub(gross)
}

fn retained_raw_e8s(balance: u64, plans: &[RawIcpPlan]) -> u64 {
    let gross_sent: u64 = plans
        .iter()
        .filter(|plan| plan.amount_e8s > 0)
        .map(|plan| plan.gross_share_e8s)
        .sum();
    balance.saturating_sub(gross_sent)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use candid::Principal;

    use super::*;
    use crate::state::CyclesSampleSource;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn sample(canister_id: Principal, amount_e8s: u64) -> CanisterBurnSample {
        CanisterBurnSample {
            canister_id,
            previous_cycles: Some(1_000),
            current_cycles: 900,
            burn_cycles: 100,
            weight: 100,
            gross_share_e8s: amount_e8s + 10,
            amount_e8s,
            skipped_reason: None,
        }
    }

    fn job_with_three_topups() -> ActiveRelayJob {
        let canister_a = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let canister_b = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let canister_c = principal("rkp4c-7iaaa-aaaaa-aaaca-cai");
        let mut current_cycles = BTreeMap::new();
        current_cycles.insert(
            canister_a,
            crate::state::CyclesSnapshot {
                cycles: 900,
                timestamp_nanos: 1,
                source: CyclesSampleSource::BlackholeStatus,
            },
        );
        ActiveRelayJob {
            id: 1,
            mode: ActiveRelayMode::CyclesTopUp,
            started_at_ts_nanos: 1,
            fee_e8s: 10,
            balance_start_e8s: 1_000,
            current_cycles,
            canisters: vec![
                sample(canister_a, 100),
                sample(canister_b, 200),
                sample(canister_c, 300),
            ],
            raw_recipients: Vec::new(),
            pending_transfer: None,
            next_transfer_index: 0,
            next_created_at_time_nanos: 10,
            summary: RelaySummary::started(RelayMode::CyclesTopUp, 1, 3),
        }
    }

    #[test]
    fn max_transfers_per_tick_chunks_transfer_planning_and_preserves_cursor() {
        let mut job = job_with_three_topups();
        let mut started = 0;

        assert_eq!(
            plan_next_transfer(&mut job, Some(1), &mut started),
            TransferPlanStep::Planned
        );
        assert_eq!(started, 1);
        assert_eq!(job.next_transfer_index, 1);
        assert!(job.pending_transfer.is_some());

        job.pending_transfer = None;
        assert_eq!(
            plan_next_transfer(&mut job, Some(1), &mut started),
            TransferPlanStep::Paused
        );
        assert_eq!(job.summary.partial_tick_count, 1);
        assert_eq!(job.next_transfer_index, 1);

        let mut next_tick_started = 0;
        assert_eq!(
            plan_next_transfer(&mut job, Some(1), &mut next_tick_started),
            TransferPlanStep::Planned
        );
        assert_eq!(job.next_transfer_index, 2);
    }
}
