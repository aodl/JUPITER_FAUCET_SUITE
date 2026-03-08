use std::time::Duration;

const LOG_INTERVAL_SECS: u64 = 20 * 24 * 60 * 60;

fn log_cycles() {
    let cycles: u128 = ic_cdk::api::canister_cycle_balance();
    ic_cdk::println!("Cycles: {}", cycles);
}

fn install_timers() {
    ic_cdk_timers::set_timer_interval(Duration::from_secs(LOG_INTERVAL_SECS), || async {
        log_cycles();
    });
}

// Recovery code is not necessary unless a lifeline event occurs, which is unexpected.
// In that event this canister should be upgraded with recovery steps that match the
// specific failure case.
#[ic_cdk::init]
fn init() {
    install_timers();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    install_timers();
}

ic_cdk::export_candid!();

