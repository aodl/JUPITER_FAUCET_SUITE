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
}

pub(super) fn log_raw_recipients(
    recipients: &[crate::state::RawIcpRecipient],
    self_id: candid::Principal,
    balance_start_e8s: u64,
    fee_e8s: u64,
) {
    let default_account = crate::logic::default_account(self_id);
    let plans = crate::logic::allocate_equal_raw_icp_shares(
        recipients,
        default_account,
        balance_start_e8s,
        fee_e8s,
    );
    for plan in plans {
        ic_cdk::println!("{}", crate::state::relay_raw_recipient_log_line(&plan));
    }
}
