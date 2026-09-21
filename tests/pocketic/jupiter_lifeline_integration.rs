use anyhow::{Context, Result};
use candid::{encode_args, Principal};
use std::sync::OnceLock;
use std::time::Duration;

#[path = "support/pocketic.rs"]
#[allow(dead_code)]
mod pocketic_support;
#[path = "support/wasm.rs"]
mod wasm_support;

static LIFELINE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
const LOG_INTERVAL: Duration = Duration::from_secs(20 * 24 * 60 * 60);

fn lifeline_wasm() -> Result<Vec<u8>> {
    wasm_support::build_wasm_cached_for_test(&LIFELINE_WASM, "jupiter-lifeline", None)
}

fn logs(pic: &pocket_ic::PocketIc, canister: Principal) -> Result<String> {
    let records = pic
        .fetch_canister_logs(canister, Principal::anonymous())
        .map_err(|error| anyhow::anyhow!("fetch lifeline logs failed: {error:?}"))?;
    Ok(records
        .iter()
        .map(|record| String::from_utf8_lossy(&record.content))
        .collect::<Vec<_>>()
        .join("\n"))
}

#[test]
#[ignore = "PocketIC lifecycle integration"]
fn lifeline_init_upgrade_and_natural_timer_reinstall_are_operational() -> Result<()> {
    let pic = pocketic_support::builder()
        .with_application_subnet()
        .build();
    let lifeline = pic.create_canister();
    pic.add_cycles(lifeline, 5_000_000_000_000);
    let wasm = lifeline_wasm()?;
    pic.install_canister(lifeline, wasm.clone(), encode_args(())?, None);
    for _ in 0..5 {
        pic.tick();
    }
    let installed_logs = logs(&pic, lifeline)?;
    anyhow::ensure!(installed_logs.contains("event=init_complete"));
    anyhow::ensure!(installed_logs.contains("timers_installed=true"));

    pic.advance_time(LOG_INTERVAL + Duration::from_secs(1));
    for _ in 0..10 {
        pic.tick();
    }
    let first_timer_count = logs(&pic, lifeline)?.matches("Cycles:").count();
    anyhow::ensure!(
        first_timer_count >= 1,
        "init timer did not produce a cycles record"
    );

    let sender = pic
        .get_controllers(lifeline)
        .first()
        .copied()
        .context("lifeline controller")?;
    pic.upgrade_canister(lifeline, wasm, encode_args(())?, Some(sender))
        .map_err(|error| anyhow::anyhow!("lifeline upgrade failed: {error:?}"))?;
    for _ in 0..5 {
        pic.tick();
    }
    let upgraded_logs = logs(&pic, lifeline)?;
    anyhow::ensure!(upgraded_logs.contains("event=post_upgrade_complete"));
    anyhow::ensure!(upgraded_logs.contains("main_interval_seconds=1728000"));
    let before_reinstalled_timer = upgraded_logs.matches("Cycles:").count();

    pic.advance_time(LOG_INTERVAL + Duration::from_secs(1));
    for _ in 0..10 {
        pic.tick();
    }
    let final_logs = logs(&pic, lifeline)?;
    anyhow::ensure!(
        final_logs.matches("Cycles:").count() > before_reinstalled_timer,
        "post-upgrade timer did not produce a fresh cycles record"
    );
    Ok(())
}
