use jupiter_canister_logging::{
    format_event_line, FIELD_EVENT, FIELD_MAIN_INTERVAL_SECONDS, FIELD_TIMERS_INSTALLED,
};

#[cfg(test)]
thread_local! {
    pub(super) static TEST_LOG_LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn emit_log_line(line: String) {
    #[cfg(test)]
    {
        TEST_LOG_LINES.with(|logs| logs.borrow_mut().push(line));
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("{}", line);
    }
}

pub(super) fn log_error(message: &str) {
    emit_log_line(format_event_line(
        "relay",
        "ERR",
        &[("message", message.to_string())],
    ));
}

pub(crate) fn log_lifecycle(
    event: &str,
    main_interval_seconds: u64,
    active_job_present: Option<bool>,
    active_faucet_commitment_transfer_present: Option<bool>,
) {
    let mut fields = vec![
        (FIELD_EVENT, event.to_string()),
        (FIELD_TIMERS_INSTALLED, true.to_string()),
        (
            FIELD_MAIN_INTERVAL_SECONDS,
            main_interval_seconds.to_string(),
        ),
    ];
    if let Some(value) = active_job_present {
        fields.push(("active_job_present", value.to_string()));
    }
    if let Some(value) = active_faucet_commitment_transfer_present {
        fields.push((
            "active_faucet_commitment_transfer_present",
            value.to_string(),
        ));
    }
    emit_log_line(format_event_line("relay", "LIFECYCLE", &fields));
}

pub(super) fn log_cycles_and_config() {
    let cycles: u128 = ic_cdk::api::canister_cycle_balance();
    emit_log_line(format!("Cycles: {}", cycles));
    let line = crate::state::with_state(|st| {
        crate::state::runtime_config_log_line(&st.config, ic_cdk::api::canister_self())
    });
    emit_log_line(line);
}

pub(super) fn log_summary(summary: &crate::state::RelaySummary) {
    emit_log_line(crate::state::relay_summary_log_line(summary));
    for sample in &summary.canisters {
        if should_log_canister_detail(summary, sample) {
            emit_log_line(crate::state::relay_canister_log_line(sample));
        }
    }
    for failure in &summary.probe_failures {
        emit_log_line(crate::state::relay_probe_failure_log_line(failure));
    }
    for status in &summary.target_probe_statuses {
        if should_log_target_probe_status(status) {
            emit_log_line(crate::state::relay_target_probe_status_log_line(status));
        }
    }
    for plan in &summary.surplus_transfers {
        emit_log_line(crate::state::relay_surplus_transfer_log_line(plan));
    }
}

fn should_log_canister_detail(
    summary: &crate::state::RelaySummary,
    sample: &crate::state::CanisterBurnSample,
) -> bool {
    if sample.sent_topup_e8s > 0 || sample.actual_minted_cycles > 0 {
        return true;
    }
    if transfer_problem_present(summary) && sample.amount_e8s > 0 {
        return true;
    }
    match sample.skipped_reason.as_deref() {
        None => sample.amount_e8s > 0,
        Some(reason) => !is_routine_canister_skip(reason),
    }
}

fn transfer_problem_present(summary: &crate::state::RelaySummary) -> bool {
    summary.failed_transfers > 0
        || summary.ambiguous_transfers > 0
        || summary.cmc_notify_failed_count > 0
        || summary.cmc_notify_ambiguous_count > 0
}

fn is_routine_canister_skip(reason: &str) -> bool {
    matches!(
        reason,
        crate::logic::SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE
            | crate::logic::SKIP_REASON_ZERO_BURN
            | crate::logic::SKIP_REASON_NO_POSITIVE_BURN
            | crate::logic::SKIP_REASON_INSUFFICIENT_BALANCE_FOR_TOPUPS
    )
}

fn should_log_target_probe_status(status: &crate::state::TargetProbeStatus) -> bool {
    !matches!(
        status.classification,
        crate::state::TargetProbeClassification::Observable
    )
}

#[cfg(test)]
mod tests {
    use candid::Principal;

    use super::{log_summary, TEST_LOG_LINES};
    use crate::logic;
    use crate::state::{
        CanisterBurnSample, RelayMode, RelaySummary, TargetProbeClassification, TargetProbeStatus,
    };

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).expect("principal")
    }

    fn sample(canister_id: Principal, skipped_reason: Option<&str>) -> CanisterBurnSample {
        CanisterBurnSample {
            canister_id,
            previous_cycles: Some(1_000),
            current_cycles: 900,
            relay_minted_cycles: 0,
            burn_cycles: 100,
            carried_deficit_cycles: 0,
            target_topup_cycles: 101,
            gross_share_e8s: 10_000,
            amount_e8s: 0,
            sent_topup_e8s: 0,
            actual_minted_cycles: 0,
            remaining_deficit_cycles: 101,
            skipped_reason: skipped_reason.map(str::to_string),
        }
    }

    fn take_logs() -> String {
        TEST_LOG_LINES.with(|lines| {
            let mut lines = lines.borrow_mut();
            let out = lines.join("\n");
            lines.clear();
            out
        })
    }

    #[test]
    fn routine_no_transfer_tick_logs_compact_summary_only() {
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let mut summary = RelaySummary::started(RelayMode::TopUpThenSurplus, 11, 1);
        summary.canisters = vec![sample(
            canister_id,
            Some(logic::SKIP_REASON_GROSS_SHARE_DOES_NOT_EXCEED_FEE),
        )];
        summary.refresh_canister_totals();
        summary.target_probe_statuses = vec![TargetProbeStatus {
            canister_id,
            consecutive_probe_failures: 0,
            classification: TargetProbeClassification::Observable,
            skipped_reason: None,
        }];

        log_summary(&summary);
        let logs = take_logs();

        assert!(logs.contains("RELAY_SUMMARY "));
        assert!(logs.contains("canister_skip_counts=gross_share_does_not_exceed_fee:1"));
        assert!(!logs.contains("RELAY_CANISTER "));
        assert!(!logs.contains("RELAY_TARGET_PROBE "));
    }

    #[test]
    fn actual_topup_logs_canister_detail() {
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let mut topup = sample(canister_id, None);
        topup.amount_e8s = 100_000;
        topup.sent_topup_e8s = 100_000;
        topup.actual_minted_cycles = 1_000_000;
        let mut summary = RelaySummary::started(RelayMode::TopUpThenSurplus, 11, 1);
        summary.canisters = vec![topup];

        log_summary(&summary);
        let logs = take_logs();

        assert!(logs.contains("RELAY_SUMMARY "));
        assert!(logs.contains("RELAY_CANISTER "));
        assert!(logs.contains("sent_topup_e8s=100000"));
        assert!(logs.contains("actual_minted_cycles=1000000"));
    }

    #[test]
    fn unavailable_target_logs_probe_detail() {
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let mut summary = RelaySummary::started(RelayMode::NoFunds, 11, 1);
        summary.target_probe_statuses = vec![TargetProbeStatus {
            canister_id,
            consecutive_probe_failures: 3,
            classification: TargetProbeClassification::UnavailableAfterConsecutiveFailures {
                consecutive_failures: 3,
            },
            skipped_reason: Some(
                logic::SKIP_REASON_TARGET_UNAVAILABLE_AFTER_CONSECUTIVE_PROBE_FAILURES.to_string(),
            ),
        }];

        log_summary(&summary);
        let logs = take_logs();

        assert!(logs.contains("RELAY_SUMMARY "));
        assert!(logs.contains("RELAY_TARGET_PROBE "));
        assert!(logs.contains("classification=target_unavailable_after_consecutive_probe_failures"));
    }
}
