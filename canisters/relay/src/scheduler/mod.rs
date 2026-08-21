pub(crate) mod cycles_probe;
mod guards;
mod ledger_fee;
mod logging;
mod reward_history;
mod reward_splitter;
mod reward_sweep;
mod splitter;
mod tick;
mod transfer;

pub(crate) use logging::log_lifecycle;
#[cfg(feature = "debug_api")]
pub(crate) use tick::debug_main_tick_impl;
pub(crate) use tick::{install_timers, schedule_startup_liveness_tick};

#[cfg(feature = "debug_api")]
pub(crate) async fn debug_reward_sweep_impl() {
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let Some(guard) = guards::MainGuard::acquire(now_secs) else {
        return;
    };
    reward_sweep::process(now_nanos, now_secs, true).await;
    guard.release_without_finishing();
}

#[cfg(feature = "debug_api")]
pub(crate) use transfer::{
    debug_set_abort_after_successful_transfer, debug_set_pause_after_persisted_splitter_leg,
    debug_set_trap_after_successful_transfer,
};
