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
