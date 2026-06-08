use std::time::Duration;

use jupiter_canister_logging::{
    format_event_line, FIELD_EVENT, FIELD_MAIN_INTERVAL_SECONDS, FIELD_TIMERS_INSTALLED,
};

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
    log_lifecycle("init_complete");
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    install_timers();
    log_lifecycle("post_upgrade_complete");
}

fn log_lifecycle(event: &str) {
    ic_cdk::println!(
        "{}",
        format_event_line(
            "lifeline",
            "LIFECYCLE",
            &[
                (FIELD_EVENT, event.to_string()),
                (FIELD_TIMERS_INSTALLED, true.to_string()),
                (FIELD_MAIN_INTERVAL_SECONDS, LOG_INTERVAL_SECS.to_string()),
            ],
        )
    );
}

ic_cdk::export_candid!();
