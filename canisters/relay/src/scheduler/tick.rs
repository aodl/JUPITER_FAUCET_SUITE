use std::time::Duration;

use icrc_ledger_types::icrc1::account::Account;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{
    BlackholeClient, CmcClient, ExchangeRateClient, GovernanceClient, LedgerClient,
};
use crate::logic::{self, ResolvedSurplusRecipient};
use crate::scheduler::cycles_probe::probe_cycles;
use crate::scheduler::guards::MainGuard;
use crate::scheduler::logging::{log_cycles_and_config, log_error, log_info, log_summary};
use crate::scheduler::transfer::{
    drive_pending_faucet_commitment_transfer, drive_pending_transfer,
};
use crate::state::{
    self, ActiveRelayJob, ActiveRelayMode, CanisterBurnSample, PendingFaucetCommitmentTransfer,
    PendingTransfer, PendingTransferKind, PendingTransferPhase, RelayMode, RelaySummary,
    SurplusTarget,
};
use jupiter_ic_clients::xrc::XrcCanister;

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
    let has_pending_work = state::with_state(|st| {
        st.active_job.is_some() || st.active_faucet_commitment_transfer.is_some()
    });
    if has_pending_work {
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
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let governance = NnsGovernanceCanister::new(cfg.governance_canister_id);
    let blackhole = BlackholeCanister::new(cfg.blackhole_canister_id);
    let xrc = XrcCanister::new();
    run_main_tick_with_clients(
        force,
        now_nanos,
        now_secs,
        &ledger,
        &cmc,
        &governance,
        &blackhole,
        &xrc,
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
    X: ExchangeRateClient,
>(
    force: bool,
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    cmc: &C,
    governance: &G,
    blackhole: &B,
    xrc: &X,
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
    if !resume_or_start_job(now_nanos, ledger, cmc, governance, blackhole, xrc).await {
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
    X: ExchangeRateClient,
>(
    now_nanos: u64,
    ledger: &L,
    cmc: &C,
    governance: &G,
    blackhole: &B,
    xrc: &X,
) -> bool {
    if state::with_state(|st| st.active_job.is_none()) {
        start_job(now_nanos, ledger, blackhole, xrc).await;
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
            log_faucet_commitment_skip(
                source,
                Account {
                    owner: cfg.governance_canister_id,
                    subaccount: None,
                },
                0,
                0,
                0,
                0,
                "subaccount_1_fee_read_failed",
            );
            return;
        }
    };
    let balance = match ledger.balance_of_e8s(source).await {
        Ok(v) => v,
        Err(err) => {
            log_error(&format!("subaccount 1 balance read failed: {err}"));
            log_faucet_commitment_skip(
                source,
                Account {
                    owner: cfg.governance_canister_id,
                    subaccount: None,
                },
                0,
                0,
                fee,
                0,
                "subaccount_1_balance_read_failed",
            );
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
    if let Err(reason) = threshold_probe {
        log_faucet_commitment_skip(
            source,
            Account {
                owner: cfg.governance_canister_id,
                subaccount: None,
            },
            balance,
            0,
            fee,
            logic::relay_faucet_commitment_memo(self_id)
                .map(|memo| memo.len() as u32)
                .unwrap_or(0),
            reason,
        );
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
            log_faucet_commitment_skip(
                source,
                Account {
                    owner: cfg.governance_canister_id,
                    subaccount: None,
                },
                balance,
                0,
                fee,
                logic::relay_faucet_commitment_memo(self_id)
                    .map(|memo| memo.len() as u32)
                    .unwrap_or(0),
                "subaccount_1_neuron_resolution_failed",
            );
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
        Err(reason) => {
            log_faucet_commitment_skip(
                source,
                Account {
                    owner: cfg.governance_canister_id,
                    subaccount: Some(staking_subaccount),
                },
                balance,
                0,
                fee,
                logic::relay_faucet_commitment_memo(self_id)
                    .map(|memo| memo.len() as u32)
                    .unwrap_or(0),
                reason,
            );
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

fn log_faucet_commitment_skip(
    source: Account,
    destination: Account,
    balance_start_e8s: u64,
    amount_e8s: u64,
    fee_e8s: u64,
    memo_len: u32,
    reason: &'static str,
) {
    ic_cdk::println!(
        "{}",
        state::relay_faucet_commitment_log_line(
            source,
            destination,
            balance_start_e8s,
            amount_e8s,
            fee_e8s,
            memo_len,
            Some(reason),
        )
    );
}

async fn start_job<L: LedgerClient, B: BlackholeClient, X: ExchangeRateClient>(
    now_nanos: u64,
    ledger: &L,
    blackhole: &B,
    xrc: &X,
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
            st.last_completed_cycles = current_cycles;
            st.relay_minted_cycles_since_sample.clear();
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
        let mut summary =
            RelaySummary::started(RelayMode::NoFunds, now_nanos, managed.len() as u32);
        summary.completed_at_ts_nanos = Some(now_nanos);
        summary.default_account_balance_start_e8s = balance;
        summary.fee_e8s = fee;
        summary.min_cycles_balance = min_cycles;
        summary.skipped_surplus_reason = Some("no_surplus".to_string());
        log_summary(&summary);
        state::with_state_mut(|st| {
            st.last_completed_cycles = current_cycles;
            st.relay_minted_cycles_since_sample.clear();
            st.last_summary = Some(summary);
        });
        return;
    }

    let has_raw_icp_recipients = !cfg.surplus_recipients.is_empty();
    refresh_conversion_estimate_if_needed(has_raw_icp_recipients, xrc).await;

    let (previous, relay_minted, conversion_estimate) = state::with_state(|st| {
        (
            st.last_completed_cycles.clone(),
            st.relay_minted_cycles_since_sample.clone(),
            st.conversion_estimate.clone(),
        )
    });
    let allocation = if has_raw_icp_recipients {
        logic::build_allocation_plan(
            &current_cycles,
            &previous,
            &relay_minted,
            balance,
            fee,
            conversion_estimate.as_ref(),
            now_nanos,
        )
    } else {
        logic::build_spend_all_cycles_plan(&current_cycles, &previous, &relay_minted, balance, fee)
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

async fn refresh_conversion_estimate_if_needed<X: ExchangeRateClient>(
    has_raw_icp_recipients: bool,
    xrc: &X,
) {
    if has_raw_icp_recipients {
        refresh_conversion_estimate_from_xrc(xrc).await;
    }
}

async fn refresh_conversion_estimate_from_xrc<X: ExchangeRateClient>(xrc: &X) {
    match xrc.get_icp_xdr_rate().await {
        Ok(rate) => match logic::conversion_estimate_from_icp_xdr_rate(
            rate.rate,
            rate.decimals,
            rate.timestamp,
        ) {
            Ok(estimate) => {
                log_info(&format!(
                    "refreshed ICP/XDR conversion estimate cycles_per_e8={} timestamp_nanos={}",
                    estimate.cycles_per_e8, estimate.timestamp_nanos
                ));
                state::with_state_mut(|st| st.conversion_estimate = Some(estimate));
            }
            Err(err) => {
                log_error(&format!(
                    "invalid XRC ICP/XDR rate; using cached or bootstrap estimate: {err}"
                ));
            }
        },
        Err(err) => {
            log_error(&format!(
                "XRC ICP/XDR conversion estimate refresh failed; using cached or bootstrap estimate: {err}"
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
    if job.canisters.iter().any(|sample| {
        sample.amount_e8s > 0 && sample.actual_minted_cycles < sample.target_topup_cycles
    }) {
        return Err("observed_conversion_worse_than_planned_topups");
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
        job.summary.completed_at_ts_nanos = Some(now_nanos);
        log_summary(&job.summary);
        st.last_completed_cycles = job.current_cycles;
        st.relay_minted_cycles_since_sample.clear();
        st.last_summary = Some(job.summary);
    });
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
    use crate::state::CyclesSampleSource;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
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
            target_topup_cycles: 101,
            gross_share_e8s: amount_e8s + 10,
            amount_e8s,
            actual_minted_cycles: 0,
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
            mode: ActiveRelayMode::TopUpThenSurplus,
            started_at_ts_nanos: 1,
            fee_e8s: 10,
            balance_start_e8s: 1_000,
            current_cycles,
            canisters: vec![
                sample(canister_a, 100),
                sample(canister_b, 200),
                sample(canister_c, 300),
            ],
            surplus_transfers: Vec::new(),
            surplus_memos: Vec::new(),
            surplus_phase_planned: false,
            pending_transfer: None,
            next_transfer_index: 0,
            surplus_transfer_index: 0,
            next_created_at_time_nanos: 10,
            summary: RelaySummary::started(RelayMode::TopUpThenSurplus, 1, 3),
        }
    }

    struct MockXrcClient {
        calls: AtomicUsize,
    }

    impl MockXrcClient {
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
    impl ExchangeRateClient for MockXrcClient {
        async fn get_icp_xdr_rate(&self) -> Result<crate::clients::IcpXdrRate, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(ClientError::Call("mock xrc failure".to_string()))
        }
    }

    #[test]
    fn start_job_without_raw_recipients_does_not_query_xrc() {
        let xrc = MockXrcClient::new();

        block_on(refresh_conversion_estimate_if_needed(false, &xrc));

        assert_eq!(xrc.calls(), 0);
    }

    #[test]
    fn start_job_with_raw_recipients_queries_xrc_before_planning() {
        let xrc = MockXrcClient::new();

        block_on(refresh_conversion_estimate_if_needed(true, &xrc));

        assert_eq!(xrc.calls(), 1);
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
    fn surplus_is_blocked_when_observed_conversion_mints_less_than_target() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        job.canisters[0].actual_minted_cycles = job.canisters[0].target_topup_cycles - 1;

        assert_eq!(
            surplus_allowed_after_topups(&job),
            Err("observed_conversion_worse_than_planned_topups")
        );
    }

    #[test]
    fn all_cycles_mode_does_not_run_raw_surplus_conversion_guard() {
        let mut job = job_with_three_topups();
        job.next_transfer_index = job.canisters.len() as u32;
        job.surplus_phase_planned = true;
        job.summary.skipped_surplus_reason = Some("no_raw_icp_recipients".to_string());
        job.canisters[0].actual_minted_cycles = job.canisters[0].target_topup_cycles - 1;
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
