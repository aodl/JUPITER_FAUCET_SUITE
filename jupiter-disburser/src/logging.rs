/// Minimal, non-chatty logging.
///
/// The canister log buffer is small; log only key transitions.
/// Logs are intended to be made publicly visible via log_visibility=public.
pub fn log_boot(now_secs: u64) {
    ic_cdk::println!("BOOT t={}", now_secs);
}

pub fn log_init(pending_id: u64, pct: u32, age_s: u64, amt_opt: Option<u64>) {
    match amt_opt {
        Some(a) => ic_cdk::println!("INIT id={} pct={} age_s={} amt={}", pending_id, pct, age_s, a),
        None => ic_cdk::println!("INIT id={} pct={} age_s={} amt=?", pending_id, pct, age_s),
    }
}

pub fn log_ready(pending_id: u64, bal: u64) {
    ic_cdk::println!("READY id={} bal={}", pending_id, bal);
}

pub fn log_plan(pending_id: u64, bal: u64, fee: u64, k: u64, base: u64, b80: u64, b20: u64) {
    ic_cdk::println!(
        "PLAN id={} bal={} fee={} k={} base={} b80={} b20={}",
        pending_id, bal, fee, k, base, b80, b20
    );
}

pub fn log_recon(pending_id: u64, newly_assumed: u32) {
    ic_cdk::println!("RECON id={} n={}", pending_id, newly_assumed);
}

pub fn log_paid(pending_id: u64, total: u64, fee: u64, k: u64) {
    ic_cdk::println!("PAID id={} total={} fee={} k={}", pending_id, total, fee, k);
}

pub fn log_err(pending_id: u64, code: u32) {
    ic_cdk::println!("ERR id={} code={}", pending_id, code);
}
