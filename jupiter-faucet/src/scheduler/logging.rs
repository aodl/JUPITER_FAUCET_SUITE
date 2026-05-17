use super::*;
#[cfg(test)]
thread_local! {
    pub(super) static TEST_LOG_LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn emit_log_line(line: String) {
    #[cfg(test)]
    {
        TEST_LOG_LINES.with(|logs| logs.borrow_mut().push(line));
        return;
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("{}", line);
    }
}

pub(super) fn log_error(code: u32) {
    emit_log_line(format!("ERR:{}", code));
}
pub(super) fn log_cycles() {
    #[cfg(test)]
    {
        return;
    }
    #[cfg(not(test))]
    {
        let cycles: u128 = ic_cdk::api::canister_cycle_balance();
        emit_log_line(format!("Cycles: {}", cycles));
    }
}

pub(super) fn log_summary(summary: &state::Summary) {
    emit_log_line(format!(
        "SUMMARY:topped_up_count={} failed_topups={} ambiguous_topups={} ignored_under_threshold={} ignored_bad_memo={} remainder_to_self_e8s={} pot_remaining_e8s={} effective_denom_e8s={}",
        summary.topped_up_count,
        summary.failed_topups,
        summary.ambiguous_topups,
        summary.ignored_under_threshold,
        summary.ignored_bad_memo,
        summary.remainder_to_self_e8s,
        summary.pot_remaining_e8s,
        summary.effective_denom_staking_balance_e8s.unwrap_or(summary.denom_staking_balance_e8s)
    ));
}

pub(super) fn log_current_config() {
    let line = state::with_state(|st| state::runtime_config_log_line(&st.config));
    emit_log_line(line);
}
