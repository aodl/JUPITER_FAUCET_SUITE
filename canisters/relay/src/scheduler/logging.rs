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
        emit_log_line(crate::state::relay_canister_log_line(sample));
    }
    for failure in &summary.probe_failures {
        emit_log_line(crate::state::relay_probe_failure_log_line(failure));
    }
    for status in &summary.target_probe_statuses {
        emit_log_line(crate::state::relay_target_probe_status_log_line(status));
    }
    for plan in &summary.surplus_transfers {
        emit_log_line(crate::state::relay_surplus_transfer_log_line(plan));
    }
}
