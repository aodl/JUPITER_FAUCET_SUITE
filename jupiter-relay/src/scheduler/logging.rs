pub(super) fn log_error(message: &str) {
    ic_cdk::println!("relay error: {message}");
}

pub(super) fn log_info(message: &str) {
    ic_cdk::println!("relay: {message}");
}

pub(super) fn log_cycles_and_config() {
    let cycles: u128 = ic_cdk::api::canister_cycle_balance();
    ic_cdk::println!("Cycles: {}", cycles);
    let line = crate::state::with_state(|st| {
        crate::state::runtime_config_log_line(&st.config, ic_cdk::api::canister_self())
    });
    ic_cdk::println!("{}", line);
}

pub(super) fn log_summary(summary: &crate::state::RelaySummary) {
    ic_cdk::println!("{}", crate::state::relay_summary_log_line(summary));
    for sample in &summary.canisters {
        ic_cdk::println!("{}", crate::state::relay_canister_log_line(sample));
    }
    for failure in &summary.probe_failures {
        ic_cdk::println!("{}", crate::state::relay_probe_failure_log_line(failure));
    }
    for plan in &summary.surplus_transfers {
        ic_cdk::println!("{}", crate::state::relay_surplus_transfer_log_line(plan));
    }
}
