use std::time::Duration;

use icrc_ledger_types::icrc1::account::Account;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{BlackholeClient, CmcClient, GovernanceClient, LedgerClient};
use crate::logic::{self, ResolvedSurplusRecipient};
use crate::scheduler::cycles_probe::probe_cycles;
use crate::scheduler::guards::MainGuard;
use crate::scheduler::logging::{log_cycles_and_config, log_error, log_summary};
use crate::scheduler::transfer::{
    drive_pending_faucet_commitment_transfer, drive_pending_transfer,
};
use crate::state::{
    self, ActiveRelayJob, ActiveRelayMode, CanisterBurnSample, PendingFaucetCommitmentTransfer,
    PendingTransfer, PendingTransferKind, PendingTransferPhase, RelayMode, RelaySummary,
    SurplusTarget,
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

pub(crate) fn schedule_startup_liveness_tick() {
    ic_cdk_timers::set_timer(Duration::from_secs(1), async {
        main_tick(false).await;
    });
}

#[cfg(feature = "debug_api")]
pub(crate) async fn debug_main_tick_impl() {
    main_tick(true).await;
}

async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let governance = NnsGovernanceCanister::new(cfg.governance_canister_id);
    let blackhole = BlackholeCanister::new(cfg.blackhole_canister_id);
    run_main_tick_with_clients(
        force,
        now_nanos,
        now_secs,
        &ledger,
        &cmc,
        &governance,
        &blackhole,
    )
    .await;
}

// The relay scheduler keeps clients explicit so integration tests can model each dependency.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_main_tick_with_clients<
    L: LedgerClient,
    C: CmcClient,
    G: GovernanceClient,
    B: BlackholeClient,
>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    cmc: &C,
    governance: &G,
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
    if !resume_or_start_faucet_commitment(now_nanos, ledger, governance).await {
        log_error("relay tick stopped after debug faucet commitment transfer injection");
        guard.finish(now_secs);
        return;
    }
    if !resume_or_start_job(now_nanos, ledger, cmc, governance, blackhole).await {
        log_error("relay tick stopped after debug transfer injection");
        guard.finish(now_secs);
        return;
    }
    guard.finish(now_secs);
}

async fn resume_or_start_job<
    L: LedgerClient,
    C: CmcClient,
    G: GovernanceClient,
    B: BlackholeClient,
>(
    now_nanos: u64,
    ledger: &L,
    cmc: &C,
    governance: &G,
    blackhole: &B,
) -> bool {
    if state::with_state(|st| st.active_job.is_none()) {
        start_job(now_nanos, ledger, cmc, blackhole).await;
    }
    drive_active_job(now_nanos, ledger, cmc, governance).await
}

async fn resume_or_start_faucet_commitment<L: LedgerClient, G: GovernanceClient>(
    now_nanos: u64,
    ledger: &L,
    governance: &G,
) -> bool {
    if state::with_state(|st| st.active_faucet_commitment_transfer.is_none()) {
        plan_faucet_commitment(now_nanos, ledger, governance).await;
    }
    drive_pending_faucet_commitment_transfer(ledger, now_nanos).await
}

async fn plan_faucet_commitment<L: LedgerClient, G: GovernanceClient>(
    now_nanos: u64,
    ledger: &L,
    governance: &G,
) {
    let cfg = state::with_state(|st| st.config.clone());
    let self_id = ic_cdk::api::canister_self();
    let source = logic::relay_subaccount_one_account(self_id);
    let fee = match ledger.fee_e8s().await {
        Ok(v) => v,
        Err(err) => {
            log_error(&format!("subaccount 1 fee read failed: {err}"));
            return;
        }
    };
    let balance = match ledger.balance_of_e8s(source).await {
        Ok(v) => v,
        Err(err) => {
            log_error(&format!("subaccount 1 balance read failed: {err}"));
            return;
        }
    };

    let probe_subaccount = [0u8; 32];
    let threshold_probe = logic::build_faucet_commitment_plan(
        self_id,
        cfg.governance_canister_id,
        probe_subaccount,
        balance,
        fee,
    );
    if threshold_probe.is_err() {
        return;
    }

    let staking_subaccount = match governance
        .neuron_staking_subaccount(logic::JUPITER_FAUCET_NEURON_ID)
        .await
    {
        Ok(v) => v,
        Err(err) => {
            log_error(&format!(
                "subaccount 1 Jupiter Faucet neuron resolution failed: {err}"
            ));
            return;
        }
    };
    let plan = match logic::build_faucet_commitment_plan(
        self_id,
        cfg.governance_canister_id,
        staking_subaccount,
        balance,
        fee,
    ) {
        Ok(plan) => plan,
        Err(_) => {
            return;
        }
    };

    state::with_state_mut(|st| {
        st.active_faucet_commitment_transfer = Some(PendingFaucetCommitmentTransfer {
            transfer: PendingTransfer {
                kind: PendingTransferKind::FaucetCommitment {
                    neuron_id: logic::JUPITER_FAUCET_NEURON_ID,
                    account: plan.destination_account,
                    from_subaccount: logic::relay_subaccount_one(),
                    memo: plan.memo,
                },
                gross_share_e8s: plan.balance_start_e8s,
                amount_e8s: plan.amount_e8s,
                created_at_time_nanos: now_nanos,
                phase: PendingTransferPhase::AwaitingTransfer,
            },
            fee_e8s: plan.fee_e8s,
            balance_start_e8s: plan.balance_start_e8s,
        });
    });
}

async fn start_job<L: LedgerClient, C: CmcClient, B: BlackholeClient>(
    now_nanos: u64,
    ledger: &L,
    cmc: &C,
    blackhole: &B,
) {
    let cfg = state::with_state(|st| st.config.clone());
    let self_id = ic_cdk::api::canister_self();
    let managed = logic::effective_managed_canisters(&cfg.managed_canisters, self_id);
    let (current_cycles, probe_failures) = probe_cycles(
        &managed,
        self_id,
        cfg.blackhole_canister_id,
        now_nanos,
        blackhole,
    )
    .await;
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
        summary.skipped_surplus_reason = Some("probe_failed".to_string());
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
        summary.skipped_surplus_reason = Some("missing_previous_sample".to_string());
        log_summary(&summary);
        state::with_state_mut(|st| {
            complete_baseline_sample(st, current_cycles, &managed, summary);
        });
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
            summary.skipped_surplus_reason = Some("probe_failed".to_string());
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
            summary.skipped_surplus_reason = Some("probe_failed".to_string());
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
        let has_raw_icp_recipients = !cfg.surplus_recipients.is_empty();
        let (previous, relay_minted, recovery_deficits) = state::with_state(|st| {
            (
                st.last_completed_cycles.clone(),
                st.relay_minted_cycles_since_sample.clone(),
                st.recovery_deficit_cycles.clone(),
            )
        });
        let summary = build_no_funds_summary(
            now_nanos,
            managed.len() as u32,
            min_cycles,
            balance,
            fee,
            has_raw_icp_recipients,
            &current_cycles,
            &previous,
            &relay_minted,
            &recovery_deficits,
        );
        log_summary(&summary);
        state::with_state_mut(|st| {
            complete_no_funds_sample(st, current_cycles, summary);
        });
        return;
    }

    let has_raw_icp_recipients = !cfg.surplus_recipients.is_empty();
    refresh_conversion_estimate_if_needed(has_raw_icp_recipients, cmc).await;

    let (previous, relay_minted, recovery_deficits, conversion_estimate) =
        state::with_state(|st| {
            (
                st.last_completed_cycles.clone(),
                st.relay_minted_cycles_since_sample.clone(),
                st.recovery_deficit_cycles.clone(),
                st.conversion_estimate.clone(),
            )
        });
    state::with_state_mut(|st| {
        for canister_id in relay_minted.keys() {
            st.relay_minted_cycles_since_sample.remove(canister_id);
        }
    });
    let allocation = if has_raw_icp_recipients {
        logic::build_allocation_plan(
            &current_cycles,
            &previous,
            &relay_minted,
            &recovery_deficits,
            balance,
            fee,
            conversion_estimate.as_ref(),
            now_nanos,
        )
    } else {
        logic::build_spend_all_cycles_plan(
            &current_cycles,
            &previous,
            &relay_minted,
            &recovery_deficits,
            balance,
            fee,
        )
    };
    let canisters = allocation
        .topups
        .iter()
        .map(CanisterBurnSample::from)
        .collect::<Vec<_>>();
    let total_burn_cycles = canisters.iter().map(|sample| sample.burn_cycles).sum();

    let id = state::with_state_mut(|st| {
        let id = st.next_job_id;
        st.next_job_id = st.next_job_id.saturating_add(1);
        id
    });

    let mut summary =
        RelaySummary::started(RelayMode::TopUpThenSurplus, now_nanos, managed.len() as u32);
    summary.default_account_balance_start_e8s = balance;
    summary.fee_e8s = fee;
    summary.min_cycles_balance = min_cycles;
    summary.total_burn_cycles = total_burn_cycles;
    summary.canisters = canisters.clone();
    summary.refresh_canister_totals();
    summary.conversion_estimate_used = if has_raw_icp_recipients {
        conversion_estimate
    } else {
        None
    };
    summary.skipped_surplus_reason = allocation.skipped_surplus_reason;
    summary.planned_retained_e8s = retained_e8s(balance, &canisters, &[]);
    summary.known_unspent_e8s = summary.planned_retained_e8s;

    let job = ActiveRelayJob {
        id,
        mode: ActiveRelayMode::TopUpThenSurplus,
        started_at_ts_nanos: now_nanos,
        fee_e8s: fee,
        balance_start_e8s: balance,
        current_cycles,
        canisters,
        surplus_transfers: Vec::new(),
        surplus_memos: Vec::new(),
        surplus_phase_planned: !has_raw_icp_recipients || !allocation.topup_phase_fully_funded,
        pending_transfer: None,
        next_transfer_index: 0,
        surplus_transfer_index: 0,
        next_created_at_time_nanos: now_nanos,
        summary,
    };
    state::with_state_mut(|st| st.active_job = Some(job));
}

#[allow(clippy::too_many_arguments)]
fn build_no_funds_summary(
    now_nanos: u64,
    managed_canister_count: u32,
    min_cycles: Option<u128>,
    balance: u64,
    fee: u64,
    has_raw_icp_recipients: bool,
    current_cycles: &std::collections::BTreeMap<candid::Principal, crate::state::CyclesSnapshot>,
    previous: &std::collections::BTreeMap<candid::Principal, crate::state::CyclesSnapshot>,
    relay_minted: &std::collections::BTreeMap<candid::Principal, u128>,
    recovery_deficits: &std::collections::BTreeMap<candid::Principal, u128>,
) -> RelaySummary {
    let allocation = if has_raw_icp_recipients {
        logic::build_allocation_plan(
            current_cycles,
            previous,
            relay_minted,
            recovery_deficits,
            balance,
            fee,
            None,
            now_nanos,
        )
    } else {
        logic::build_spend_all_cycles_plan(
            current_cycles,
            previous,
            relay_minted,
            recovery_deficits,
            balance,
            fee,
        )
    };
    let mut summary = RelaySummary::started(RelayMode::NoFunds, now_nanos, managed_canister_count);
    summary.completed_at_ts_nanos = Some(now_nanos);
    summary.default_account_balance_start_e8s = balance;
    summary.fee_e8s = fee;
    summary.min_cycles_balance = min_cycles;
    summary.skipped_surplus_reason = Some("no_surplus".to_string());
    summary.canisters = allocation
        .topups
        .iter()
        .map(CanisterBurnSample::from)
        .collect();
    summary.total_burn_cycles = summary
        .canisters
        .iter()
        .map(|sample| sample.burn_cycles)
        .sum();
    summary.refresh_canister_totals();
    summary.planned_retained_e8s = balance;
    summary.known_unspent_e8s = balance;
    summary
}

async fn refresh_conversion_estimate_if_needed<C: CmcClient>(
    has_raw_icp_recipients: bool,
    cmc: &C,
) {
    if has_raw_icp_recipients {
        refresh_conversion_estimate_from_cmc(cmc).await;
    }
}

async fn refresh_conversion_estimate_from_cmc<C: CmcClient>(cmc: &C) {
    match cmc.get_icp_xdr_conversion_rate().await {
        Ok(rate) => match logic::conversion_estimate_from_cmc_rate(
            rate.xdr_permyriad_per_icp,
            rate.timestamp_seconds,
        ) {
            Ok(estimate) => {
                state::with_state_mut(|st| st.conversion_estimate = Some(estimate));
            }
            Err(err) => {
                log_error(&format!(
                    "invalid CMC ICP/XDR conversion rate; using cached or bootstrap estimate: {err}"
                ));
            }
        },
        Err(err) => {
            log_error(&format!(
                "CMC ICP/XDR conversion rate refresh failed; using cached or bootstrap estimate: {err}"
            ));
        }
    }
}

async fn resolve_surplus_recipients<G: GovernanceClient>(
    cfg: &crate::state::Config,
    governance: &G,
) -> Result<Vec<ResolvedSurplusRecipient>, String> {
    let mut resolved = Vec::new();
    for recipient in &cfg.surplus_recipients {
        match recipient.target {
            SurplusTarget::Canister(_) => {
                if let Some(candidate) = logic::resolve_canister_surplus_recipient(recipient) {
                    resolved.push(candidate);
                }
            }
            SurplusTarget::Neuron(neuron_id) => {
                let subaccount = governance
                    .neuron_staking_subaccount(neuron_id)
                    .await
                    .map_err(|err| format!("neuron {neuron_id} is not publicly readable: {err}"))?;
                resolved.push(ResolvedSurplusRecipient {
                    target: recipient.target.clone(),
                    account: Account {
                        owner: cfg.governance_canister_id,
                        subaccount: Some(subaccount),
                    },
                    memo: recipient.memo.clone(),
                });
            }
        }
    }
    logic::reject_duplicate_resolved_destinations(&resolved)?;
    Ok(resolved)
}

async fn drive_active_job<L: LedgerClient, C: CmcClient, G: GovernanceClient>(
    now_nanos: u64,
    ledger: &L,
    cmc: &C,
    governance: &G,
) -> bool {
    let max_transfers_this_tick = state::with_state(|st| st.config.max_transfers_per_tick);
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
        if topup_phase_done_and_surplus_unplanned() {
            plan_surplus_phase(now_nanos, governance).await;
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
        if topup_phase_done_and_surplus_unplanned() && topup_phase_had_activity() {
            continue;
        }
        complete_job(now_nanos);
        return true;
    }
}

fn topup_phase_done_and_surplus_unplanned() -> bool {
    state::with_state(|st| {
        st.active_job
            .as_ref()
            .map(|job| {
                (job.next_transfer_index as usize) >= job.canisters.len()
                    && !job.surplus_phase_planned
            })
            .unwrap_or(false)
    })
}

fn topup_phase_had_activity() -> bool {
    state::with_state(|st| {
        st.active_job
            .as_ref()
            .map(|job| {
                job.summary.ledger_transfer_count > 0
                    || job.summary.failed_transfers > 0
                    || job.summary.ambiguous_transfers > 0
                    || job.summary.cmc_notify_success_count > 0
                    || job.summary.cmc_notify_failed_count > 0
                    || job.summary.cmc_notify_ambiguous_count > 0
            })
            .unwrap_or(false)
    })
}

async fn plan_surplus_phase<G: GovernanceClient>(now_nanos: u64, governance: &G) {
    let (cfg, clean_topup_phase, known_unspent_e8s, fee_e8s) = state::with_state(|st| {
        let job = st.active_job.as_ref().expect("active job");
        (
            st.config.clone(),
            surplus_allowed_after_topups(job),
            job.summary.known_unspent_e8s,
            job.fee_e8s,
        )
    });

    if let Err(reason) = clean_topup_phase {
        disable_surplus(reason);
        return;
    }

    let resolved_surplus = match resolve_surplus_recipients(&cfg, governance).await {
        Ok(recipients) => recipients,
        Err(err) => {
            log_error(&format!("surplus recipient resolution failed: {err}"));
            disable_surplus("recipient_resolution_failed");
            return;
        }
    };
    let conversion_estimate = state::with_state(|st| st.conversion_estimate.clone());
    let (surplus, surplus_e8s_before_fees, skipped_surplus_reason) = logic::build_surplus_plan(
        &resolved_surplus,
        known_unspent_e8s,
        fee_e8s,
        conversion_estimate.as_ref(),
        now_nanos,
    );
    let memos = resolved_surplus
        .into_iter()
        .map(|recipient| recipient.memo)
        .collect::<Vec<_>>();

    state::with_state_mut(|st| {
        let job = st.active_job.as_mut().expect("active job");
        job.surplus_transfers = surplus;
        job.surplus_memos = memos;
        job.surplus_transfer_index = 0;
        job.surplus_phase_planned = true;
        job.summary.surplus_e8s_before_fees = surplus_e8s_before_fees;
        job.summary.surplus_transfers = job.surplus_transfers.clone();
        job.summary.skipped_surplus_reason = skipped_surplus_reason;
        job.summary.planned_retained_e8s = retained_e8s(
            job.balance_start_e8s,
            &job.canisters,
            &job.surplus_transfers,
        );
        job.summary.known_unspent_e8s = job.summary.planned_retained_e8s;
        job.summary.refresh_canister_totals();
    });
}

fn surplus_allowed_after_topups(job: &ActiveRelayJob) -> Result<(), &'static str> {
    if job.summary.failed_transfers != 0 {
        return Err("topup_transfer_failed");
    }
    if job.summary.ambiguous_transfers != 0 {
        return Err("topup_transfer_ambiguous");
    }
    if job.summary.cmc_notify_failed_count != 0 {
        return Err("cmc_notify_failed");
    }
    if job.summary.cmc_notify_ambiguous_count != 0 {
        return Err("cmc_notify_ambiguous");
    }
    if job.summary.partial_tick_count != 0 {
        return Err("topup_phase_incomplete");
    }
    if job
        .canisters
        .iter()
        .any(|sample| sample.remaining_deficit_cycles > 0)
    {
        return Err(logic::SKIP_REASON_UNRECOVERED_CYCLE_DEFICIT);
    }
    Ok(())
}

fn disable_surplus(reason: &'static str) {
    state::with_state_mut(|st| {
        if let Some(job) = st.active_job.as_mut() {
            job.surplus_transfers.clear();
            job.surplus_memos.clear();
            job.surplus_transfer_index = 0;
            job.surplus_phase_planned = true;
            job.summary.surplus_e8s_before_fees = 0;
            job.summary.surplus_transfers.clear();
            job.summary.skipped_surplus_reason = Some(reason.to_string());
            job.summary.planned_retained_e8s =
                retained_e8s(job.balance_start_e8s, &job.canisters, &[]);
            job.summary.known_unspent_e8s = job.summary.planned_retained_e8s;
            job.summary.refresh_canister_totals();
        }
    });
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
        job.summary.skipped_surplus_reason = Some("topup_phase_incomplete".to_string());
        job.surplus_transfers.clear();
        job.surplus_memos.clear();
        job.surplus_transfer_index = 0;
        job.surplus_phase_planned = true;
        return TransferPlanStep::Paused;
    }
    if next_topup_pending(job).is_some() || next_surplus_pending(job).is_some() {
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
        let canister_id = sample.canister_id;
        let gross_share_e8s = sample.gross_share_e8s;
        let amount_e8s = sample.amount_e8s;
        let created_at = next_created_at(job);
        job.pending_transfer = Some(PendingTransfer {
            kind: PendingTransferKind::CmcTopUp { canister_id },
            gross_share_e8s,
            amount_e8s,
            created_at_time_nanos: created_at,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        return Some(());
    }
    None
}

fn next_surplus_pending(job: &mut ActiveRelayJob) -> Option<()> {
    if job.surplus_transfers.is_empty() {
        return None;
    }
    if let Err(reason) = surplus_allowed_after_topups(job) {
        job.surplus_transfers.clear();
        job.surplus_memos.clear();
        job.surplus_transfer_index = 0;
        job.summary.surplus_transfers.clear();
        job.summary.surplus_e8s_before_fees = 0;
        job.summary.skipped_surplus_reason = Some(reason.to_string());
        return None;
    }
    while (job.surplus_transfer_index as usize) < job.surplus_transfers.len() {
        let index = job.surplus_transfer_index as usize;
        job.surplus_transfer_index = job.surplus_transfer_index.saturating_add(1);
        let plan = job.surplus_transfers[index].clone();
        if plan.amount_e8s == 0 {
            continue;
        }
        let memo = job.surplus_memos.get(index).cloned().unwrap_or(None);
        let created_at = next_created_at(job);
        job.pending_transfer = Some(PendingTransfer {
            kind: PendingTransferKind::SurplusIcp {
                target: plan.target,
                account: plan.account,
                memo,
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

fn next_created_at(job: &mut ActiveRelayJob) -> u64 {
    let created_at = job.next_created_at_time_nanos;
    job.next_created_at_time_nanos = job.next_created_at_time_nanos.saturating_add(1);
    created_at
}

fn complete_job(now_nanos: u64) {
    state::with_state_mut(|st| {
        let Some(mut job) = st.active_job.take() else {
            return;
        };
        for sample in &mut job.canisters {
            sample.remaining_deficit_cycles = sample
                .target_topup_cycles
                .saturating_sub(sample.actual_minted_cycles);
        }
        for summary_sample in &mut job.summary.canisters {
            if let Some(sample) = job
                .canisters
                .iter()
                .find(|sample| sample.canister_id == summary_sample.canister_id)
            {
                summary_sample.actual_minted_cycles = sample.actual_minted_cycles;
                summary_sample.remaining_deficit_cycles = sample.remaining_deficit_cycles;
                summary_sample.sent_topup_e8s = sample.sent_topup_e8s;
            }
        }
        job.summary.completed_at_ts_nanos = Some(now_nanos);
        job.summary.refresh_canister_totals();
        log_summary(&job.summary);
        st.last_completed_cycles = job.current_cycles;
        persist_recovery_deficits_from_samples(st, &job.canisters);
        st.last_summary = Some(job.summary);
    });
}

fn complete_baseline_sample(
    st: &mut crate::state::State,
    current_cycles: std::collections::BTreeMap<candid::Principal, crate::state::CyclesSnapshot>,
    managed: &[candid::Principal],
    summary: RelaySummary,
) {
    st.last_completed_cycles = current_cycles;
    st.relay_minted_cycles_since_sample.clear();
    st.recovery_deficit_cycles
        .retain(|canister_id, _| managed.contains(canister_id));
    st.last_summary = Some(summary);
}

fn complete_no_funds_sample(
    st: &mut crate::state::State,
    current_cycles: std::collections::BTreeMap<candid::Principal, crate::state::CyclesSnapshot>,
    summary: RelaySummary,
) {
    st.last_completed_cycles = current_cycles;
    st.relay_minted_cycles_since_sample.clear();
    persist_recovery_deficits_from_samples(st, &summary.canisters);
    st.last_summary = Some(summary);
}

fn persist_recovery_deficits_from_samples(
    st: &mut crate::state::State,
    samples: &[CanisterBurnSample],
) {
    let sampled_canisters = samples
        .iter()
        .map(|sample| sample.canister_id)
        .collect::<std::collections::BTreeSet<_>>();
    for sample in samples {
        if sample.remaining_deficit_cycles > 0 {
            st.recovery_deficit_cycles
                .insert(sample.canister_id, sample.remaining_deficit_cycles);
        } else {
            st.recovery_deficit_cycles.remove(&sample.canister_id);
        }
    }
    st.recovery_deficit_cycles
        .retain(|canister_id, _| sampled_canisters.contains(canister_id));
}

fn log_active_job_summary() {
    state::with_state(|st| {
        if let Some(job) = &st.active_job {
            log_summary(&job.summary);
        }
    });
}

fn retained_e8s(
    balance: u64,
    canisters: &[CanisterBurnSample],
    surplus: &[crate::state::SurplusTransferSample],
) -> u64 {
    let topup_gross: u64 = canisters
        .iter()
        .filter(|sample| sample.amount_e8s > 0)
        .map(|sample| sample.gross_share_e8s)
        .sum();
    let surplus_gross: u64 = surplus
        .iter()
        .filter(|sample| sample.amount_e8s > 0)
        .map(|sample| sample.gross_share_e8s)
        .sum();
    balance.saturating_sub(topup_gross.saturating_add(surplus_gross))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use candid::Principal;

    use super::*;
    use crate::clients::ClientError;
    use crate::state::{Config, CyclesSampleSource, State};

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn base_config() -> Config {
        Config {
            managed_canisters: vec![principal("22255-zqaaa-aaaas-qf6uq-cai")],
            ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
            cmc_canister_id: principal("rkp4c-7iaaa-aaaaa-aaaca-cai"),
            governance_canister_id: principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
            blackhole_canister_id: principal("77deu-baaaa-aaaar-qb6za-cai"),
            main_interval_seconds: 60,
            max_transfers_per_tick: None,
            surplus_recipients: Vec::new(),
        }
    }

    fn block_on<F: Future>(mut future: F) -> F::Output {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut future = unsafe { Pin::new_unchecked(&mut future) };
        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn sample(canister_id: Principal, amount_e8s: u64) -> CanisterBurnSample {
        CanisterBurnSample {
            canister_id,
            previous_cycles: Some(1_000),
            current_cycles: 900,
            relay_minted_cycles: 0,
            burn_cycles: 100,
            carried_deficit_cycles: 0,
            target_topup_cycles: 101,
            gross_share_e8s: amount_e8s + 10,
            amount_e8s,
            sent_topup_e8s: 0,
            actual_minted_cycles: 0,
            remaining_deficit_cycles: 101,
            skipped_reason: None,
        }
    }

    fn snapshot(cycles: u128) -> crate::state::CyclesSnapshot {
        crate::state::CyclesSnapshot {
            cycles,
            timestamp_nanos: 1,
            source: CyclesSampleSource::BlackholeStatus,
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
        let canisters = vec![
            sample(canister_a, 100),
            sample(canister_b, 200),
            sample(canister_c, 300),
        ];
        let mut summary = RelaySummary::started(RelayMode::TopUpThenSurplus, 1, 3);
        summary.canisters = canisters.clone();
        ActiveRelayJob {
            id: 1,
            mode: ActiveRelayMode::TopUpThenSurplus,
            started_at_ts_nanos: 1,
            fee_e8s: 10,
            balance_start_e8s: 1_000,
            current_cycles,
            canisters,
            surplus_transfers: Vec::new(),
            surplus_memos: Vec::new(),
            surplus_phase_planned: false,
            pending_transfer: None,
            next_transfer_index: 0,
            surplus_transfer_index: 0,
            next_created_at_time_nanos: 10,
            summary,
        }
    }

    struct MockCmcConversionClient {
        calls: AtomicUsize,
    }

    impl MockCmcConversionClient {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl CmcClient for MockCmcConversionClient {
        async fn get_icp_xdr_conversion_rate(
            &self,
        ) -> Result<crate::clients::CmcIcpXdrConversionRate, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(crate::clients::CmcIcpXdrConversionRate {
                timestamp_seconds: 7,
                xdr_permyriad_per_icp: 42,
            })
        }

        async fn notify_top_up(
            &self,
            _canister_id: Principal,
            _block_index: u64,
        ) -> Result<u128, ClientError> {
            unreachable!("conversion refresh tests do not notify top-up")
        }
    }

    #[test]
    fn start_job_without_raw_recipients_does_not_query_cmc_conversion_rate() {
        let cmc = MockCmcConversionClient::new();

        block_on(refresh_conversion_estimate_if_needed(false, &cmc));

        assert_eq!(cmc.calls(), 0);
    }

    #[test]
    fn start_job_with_raw_recipients_queries_cmc_conversion_rate_before_planning() {
        let cmc = MockCmcConversionClient::new();
        state::set_state(State::new(base_config(), 0));

        block_on(refresh_conversion_estimate_if_needed(true, &cmc));

        assert_eq!(cmc.calls(), 1);
        let estimate = state::with_state(|st| st.conversion_estimate.clone()).unwrap();
        assert_eq!(estimate.cycles_per_e8, 42);
        assert_eq!(estimate.timestamp_nanos, 7_000_000_000);
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
        assert_eq!(
            job.summary.skipped_surplus_reason.as_deref(),
            Some("topup_phase_incomplete")
        );

        let mut next_tick_started = 0;
        assert_eq!(
            plan_next_transfer(&mut job, Some(1), &mut next_tick_started),
            TransferPlanStep::Planned
        );
        assert_eq!(job.next_transfer_index, 2);
    }

    #[test]
    fn surplus_is_blocked_after_ambiguous_topup_boundary() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        job.surplus_phase_planned = true;
        job.summary.ambiguous_transfers = 1;
        job.summary.cmc_notify_ambiguous_count = 1;
        job.surplus_transfers = vec![crate::state::SurplusTransferSample {
            target: SurplusTarget::Canister(principal("jufzc-caaaa-aaaar-qb5da-cai")),
            account: icrc_ledger_types::icrc1::account::Account {
                owner: principal("jufzc-caaaa-aaaar-qb5da-cai"),
                subaccount: None,
            },
            gross_share_e8s: 100,
            amount_e8s: 90,
            memo_len: None,
            skipped_reason: None,
        }];

        assert!(next_surplus_pending(&mut job).is_none());
        assert!(job.surplus_transfers.is_empty());
        assert_eq!(
            job.summary.skipped_surplus_reason.as_deref(),
            Some("topup_transfer_ambiguous")
        );
    }

    #[test]
    fn surplus_is_blocked_when_observed_conversion_covers_burn_but_not_target() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        for sample in &mut job.canisters {
            sample.actual_minted_cycles = sample.burn_cycles;
            sample.remaining_deficit_cycles = sample
                .target_topup_cycles
                .saturating_sub(sample.actual_minted_cycles);
        }

        assert_eq!(
            surplus_allowed_after_topups(&job),
            Err(logic::SKIP_REASON_UNRECOVERED_CYCLE_DEFICIT)
        );
    }

    #[test]
    fn surplus_is_allowed_when_actual_minted_covers_full_target() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        for sample in &mut job.canisters {
            sample.actual_minted_cycles = sample.target_topup_cycles;
            sample.remaining_deficit_cycles = 0;
        }

        assert_eq!(surplus_allowed_after_topups(&job), Ok(()));
    }

    #[test]
    fn surplus_is_blocked_when_observed_conversion_mints_less_than_burn() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        job.canisters[0].actual_minted_cycles = job.canisters[0].burn_cycles - 1;
        job.canisters[0].remaining_deficit_cycles = job.canisters[0]
            .target_topup_cycles
            .saturating_sub(job.canisters[0].actual_minted_cycles);

        assert_eq!(
            surplus_allowed_after_topups(&job),
            Err(logic::SKIP_REASON_UNRECOVERED_CYCLE_DEFICIT)
        );
    }

    #[test]
    fn completed_job_minted_cycles_remain_for_next_burn_calculation() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = State::new(base_config(), 0);
        let mut job = job_with_three_topups();
        job.current_cycles.clear();
        job.current_cycles.insert(
            canister,
            crate::state::CyclesSnapshot {
                cycles: 900,
                timestamp_nanos: 1,
                source: CyclesSampleSource::BlackholeStatus,
            },
        );
        st.active_job = Some(job);
        st.relay_minted_cycles_since_sample.insert(canister, 123);
        state::set_state(st);

        complete_job(2);

        let (previous, relay_minted) = state::with_state(|st| {
            (
                st.last_completed_cycles.clone(),
                st.relay_minted_cycles_since_sample.clone(),
            )
        });
        assert_eq!(relay_minted.get(&canister), Some(&123));

        let mut current = BTreeMap::new();
        current.insert(
            canister,
            crate::state::CyclesSnapshot {
                cycles: 850,
                timestamp_nanos: 3,
                source: CyclesSampleSource::BlackholeStatus,
            },
        );
        let estimate = crate::state::ConversionEstimate {
            cycles_per_e8: 10,
            timestamp_nanos: 3,
        };
        let allocation = logic::build_allocation_plan(
            &current,
            &previous,
            &relay_minted,
            &BTreeMap::new(),
            1_000,
            10,
            Some(&estimate),
            3,
        );

        assert_eq!(allocation.topups[0].relay_minted_cycles, 123);
        assert_eq!(allocation.topups[0].burn_cycles, 173);
    }

    #[test]
    fn new_baseline_clears_consumed_relay_minted_cycles_since_sample() {
        let managed = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let removed = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let mut st = State::new(base_config(), 0);
        st.relay_minted_cycles_since_sample.insert(managed, 123);
        st.relay_minted_cycles_since_sample.insert(removed, 456);

        complete_baseline_sample(
            &mut st,
            BTreeMap::from([(managed, snapshot(900))]),
            &[managed],
            RelaySummary::started(RelayMode::BaselineOnly, 1, 1),
        );

        assert!(st.relay_minted_cycles_since_sample.is_empty());
    }

    #[test]
    fn underfunded_completed_job_persists_recovery_deficit() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = State::new(base_config(), 0);
        let mut job = job_with_three_topups();
        for sample in &mut job.canisters {
            sample.actual_minted_cycles = sample.target_topup_cycles;
            sample.remaining_deficit_cycles = 0;
        }
        job.canisters[0].actual_minted_cycles = 60;
        job.canisters[0].remaining_deficit_cycles = 41;
        st.active_job = Some(job);
        state::set_state(st);

        complete_job(2);

        state::with_state(|st| {
            assert_eq!(st.recovery_deficit_cycles.get(&canister), Some(&41));
            assert_eq!(
                st.last_summary
                    .as_ref()
                    .unwrap()
                    .canisters
                    .iter()
                    .find(|sample| sample.canister_id == canister)
                    .unwrap()
                    .remaining_deficit_cycles,
                41
            );
        });
    }

    #[test]
    fn completed_job_prunes_recovery_deficits_absent_from_completed_samples() {
        let sampled = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let removed = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let mut st = State::new(base_config(), 0);
        st.recovery_deficit_cycles.insert(sampled, 25);
        st.recovery_deficit_cycles.insert(removed, 77);
        let mut job = job_with_three_topups();
        job.canisters.retain(|sample| sample.canister_id == sampled);
        job.summary.canisters = job.canisters.clone();
        for sample in &mut job.canisters {
            sample.actual_minted_cycles = sample.target_topup_cycles;
            sample.remaining_deficit_cycles = 0;
        }
        st.active_job = Some(job);
        state::set_state(st);

        complete_job(2);

        state::with_state(|st| {
            assert!(st.recovery_deficit_cycles.is_empty());
        });
    }

    #[test]
    fn full_completed_job_clears_existing_recovery_deficit() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = State::new(base_config(), 0);
        st.recovery_deficit_cycles.insert(canister, 25);
        let mut job = job_with_three_topups();
        for sample in &mut job.canisters {
            sample.actual_minted_cycles = sample.target_topup_cycles;
            sample.remaining_deficit_cycles = 0;
        }
        st.active_job = Some(job);
        state::set_state(st);

        complete_job(2);

        state::with_state(|st| {
            assert_eq!(st.recovery_deficit_cycles.get(&canister), None);
        });
    }

    #[test]
    fn over_topup_clears_deficit_without_storing_negative_credit() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut st = State::new(base_config(), 0);
        st.recovery_deficit_cycles.insert(canister, 25);
        let mut job = job_with_three_topups();
        for sample in &mut job.canisters {
            sample.actual_minted_cycles = sample.target_topup_cycles.saturating_add(500);
            sample.remaining_deficit_cycles = 0;
        }
        st.active_job = Some(job);
        state::set_state(st);

        complete_job(2);

        state::with_state(|st| {
            assert!(st.recovery_deficit_cycles.is_empty());
        });
    }

    #[test]
    fn failed_or_ambiguous_topup_boundaries_persist_full_target_deficit() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        for mark_failure in [
            crate::scheduler::transfer::mark_pending_failed as fn(),
            crate::scheduler::transfer::mark_pending_failed_after_acceptance as fn(),
            crate::scheduler::transfer::mark_pending_ambiguous_after_acceptance as fn(),
        ] {
            let mut st = State::new(base_config(), 0);
            st.active_job = Some(job_with_three_topups());
            state::set_state(st);
            mark_failure();
            complete_job(2);
            state::with_state(|st| {
                assert_eq!(st.recovery_deficit_cycles.get(&canister), Some(&101));
            });
        }
    }

    #[test]
    fn no_funds_summary_records_recovery_deficit_instead_of_forgetting_burn() {
        let canister = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let mut current = BTreeMap::new();
        let mut previous = BTreeMap::new();
        let mut relay_minted = BTreeMap::new();
        let mut deficits = BTreeMap::new();
        current.insert(canister, snapshot(900));
        previous.insert(canister, snapshot(1_000));
        relay_minted.insert(canister, 50);
        deficits.insert(canister, 25);

        let summary = build_no_funds_summary(
            10,
            1,
            Some(900),
            0,
            10,
            true,
            &current,
            &previous,
            &relay_minted,
            &deficits,
        );

        let sample = &summary.canisters[0];
        assert_eq!(sample.burn_cycles, 150);
        assert_eq!(sample.carried_deficit_cycles, 25);
        assert_eq!(sample.target_topup_cycles, 177);
        assert_eq!(sample.actual_minted_cycles, 0);
        assert_eq!(sample.remaining_deficit_cycles, 177);
    }

    #[test]
    fn no_funds_sample_persists_sampled_deficits_and_prunes_removed_deficits() {
        let sampled = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let removed = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let mut st = State::new(base_config(), 0);
        st.relay_minted_cycles_since_sample.insert(sampled, 50);
        st.relay_minted_cycles_since_sample.insert(removed, 75);
        st.recovery_deficit_cycles.insert(sampled, 25);
        st.recovery_deficit_cycles.insert(removed, 77);
        let mut summary = RelaySummary::started(RelayMode::NoFunds, 1, 1);
        summary.canisters = vec![CanisterBurnSample {
            canister_id: sampled,
            previous_cycles: Some(1_000),
            current_cycles: 900,
            relay_minted_cycles: 50,
            burn_cycles: 150,
            carried_deficit_cycles: 25,
            target_topup_cycles: 177,
            gross_share_e8s: 0,
            amount_e8s: 0,
            sent_topup_e8s: 0,
            actual_minted_cycles: 0,
            remaining_deficit_cycles: 177,
            skipped_reason: None,
        }];

        complete_no_funds_sample(&mut st, BTreeMap::from([(sampled, snapshot(900))]), summary);

        assert!(st.relay_minted_cycles_since_sample.is_empty());
        assert_eq!(st.recovery_deficit_cycles.get(&sampled), Some(&177));
        assert_eq!(st.recovery_deficit_cycles.get(&removed), None);
    }

    #[test]
    fn all_cycles_mode_does_not_run_raw_surplus_conversion_guard() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        job.surplus_phase_planned = true;
        job.summary.skipped_surplus_reason = Some("no_raw_icp_recipients".to_string());
        job.canisters[0].actual_minted_cycles = job.canisters[0].burn_cycles - 1;
        let mut started = 0;

        assert_eq!(
            plan_next_transfer(&mut job, None, &mut started),
            TransferPlanStep::Done
        );
        assert_eq!(
            job.summary.skipped_surplus_reason.as_deref(),
            Some("no_raw_icp_recipients")
        );
    }
}
