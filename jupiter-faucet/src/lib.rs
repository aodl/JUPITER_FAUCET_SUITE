mod clients;
mod logic;
mod policy;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;

use crate::state::State;

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub staking_account: Account,
    pub payout_subaccount: Option<Vec<u8>>,

    pub ledger_canister_id: Option<Principal>,
    pub index_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,

    pub rescue_controller: Principal,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,
    pub expected_first_staking_tx_id: Option<u64>,

    pub main_interval_seconds: Option<u64>,
    pub rescue_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct UpgradeArgs {
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,
    pub clear_forced_rescue: Option<bool>,
}

fn mainnet_ledger_id() -> Principal {
    Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid hardcoded ledger principal")
}

fn mainnet_index_id() -> Principal {
    Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").expect("invalid hardcoded index principal")
}

fn mainnet_cmc_id() -> Principal {
    Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").expect("invalid hardcoded cmc principal")
}

fn mainnet_blackhole_id() -> Principal {
    Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").expect("invalid hardcoded blackhole principal")
}


pub(crate) const MIN_MIN_TX_E8S: u64 = 10_000_000;

fn assert_non_anonymous_principal(name: &str, principal: Principal) {
    assert!(principal != Principal::anonymous(), "{name} must not be the anonymous principal");
}

fn validate_config(cfg: &crate::state::Config) {
    assert_non_anonymous_principal("staking_account.owner", cfg.staking_account.owner);
    assert_non_anonymous_principal("ledger_canister_id", cfg.ledger_canister_id);
    assert_non_anonymous_principal("index_canister_id", cfg.index_canister_id);
    assert_non_anonymous_principal("cmc_canister_id", cfg.cmc_canister_id);
    assert_non_anonymous_principal("rescue_controller", cfg.rescue_controller);
    if let Some(blackhole_controller) = cfg.blackhole_controller {
        assert_non_anonymous_principal("blackhole_controller", blackhole_controller);
    }
    assert!(cfg.main_interval_seconds > 0, "main_interval_seconds must be greater than 0");
    assert!(cfg.rescue_interval_seconds > 0, "rescue_interval_seconds must be greater than 0");
    assert!(
        cfg.min_tx_e8s >= MIN_MIN_TX_E8S,
        "min_tx_e8s must be at least {MIN_MIN_TX_E8S} e8s (0.1 ICP)"
    );
}

fn decode_subaccount_opt(v: Option<Vec<u8>>) -> Result<Option<[u8; 32]>, String> {
    match v {
        None => Ok(None),
        Some(bytes) => {
            if bytes.len() != 32 {
                return Err(format!("expected 32-byte subaccount, got {} bytes", bytes.len()));
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            Ok(Some(out))
        }
    }
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;

    let cfg = crate::state::Config {
        staking_account: args.staking_account,
        payout_subaccount: decode_subaccount_opt(args.payout_subaccount).expect("invalid payout_subaccount"),
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        index_canister_id: args.index_canister_id.unwrap_or_else(mainnet_index_id),
        cmc_canister_id: args.cmc_canister_id.unwrap_or_else(mainnet_cmc_id),
        rescue_controller: args.rescue_controller,
        blackhole_controller: Some(args.blackhole_controller.unwrap_or_else(mainnet_blackhole_id)),
        blackhole_armed: args.blackhole_armed,
        expected_first_staking_tx_id: args.expected_first_staking_tx_id,
        main_interval_seconds: args.main_interval_seconds.unwrap_or(7 * 24 * 60 * 60),
        rescue_interval_seconds: args.rescue_interval_seconds.unwrap_or(24 * 60 * 60),
        min_tx_e8s: args.min_tx_e8s.unwrap_or(100_000_000),
    };

    validate_config(&cfg);
    let st = State::new(cfg, now_secs);
    crate::state::set_state(st);
    crate::scheduler::install_timers();
}

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    let st = crate::state::get_state();
    ic_cdk::storage::stable_save((st,)).expect("stable_save failed");
}

pub(crate) fn apply_upgrade_args_to_state(st: &mut State, args: Option<UpgradeArgs>, now_secs: u64) {
    if let Some(args) = args {
        if let Some(blackhole_controller) = args.blackhole_controller {
            st.config.blackhole_controller = Some(blackhole_controller);
        }
        if let Some(armed) = args.blackhole_armed {
            st.config.blackhole_armed = Some(armed);
            st.blackhole_armed_since_ts = if armed { Some(now_secs) } else { None };
            if !armed {
                st.rescue_triggered = false;
            }
        }
        if args.clear_forced_rescue.unwrap_or(false) {
            // Clearing forced rescue is a DAO acknowledgement that the prior latch
            // is no longer authoritative after recovery and upgrade.
            // We intentionally do not force an immediate controller rewrite here;
            // the next rescue evaluation recomputes controller posture from current
            // state and current policy inputs.
            st.forced_rescue_reason = None;
            st.consecutive_index_anchor_failures = Some(0);
            st.consecutive_index_latest_invariant_failures = Some(0);
            st.consecutive_index_latest_unreadable_failures = Some(0);
            st.consecutive_cmc_zero_success_runs = Some(0);
        }
    }
    validate_config(&st.config);
    st.main_lock_expires_at_ts = Some(0);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(args: Option<UpgradeArgs>) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let (mut st,): (State,) = ic_cdk::storage::stable_restore().expect("stable_restore failed");
    apply_upgrade_args_to_state(&mut st, args, now_secs);
    crate::state::set_state(st);
    crate::scheduler::install_timers();
    crate::scheduler::schedule_immediate_resume_if_needed();
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub active_payout_job_present: bool,
    pub last_summary_present: bool,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<crate::state::ForcedRescueReason>,
    pub consecutive_index_anchor_failures: u8,
    pub consecutive_index_latest_invariant_failures: u8,
    pub consecutive_index_latest_unreadable_failures: u8,
    pub consecutive_cmc_zero_success_runs: u8,
    pub last_observed_staking_balance_e8s: Option<u64>,
    pub last_observed_latest_tx_id: Option<u64>,
    pub expected_first_staking_tx_id: Option<u64>,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugAccounts {
    pub payout: Account,
    pub staking: Account,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugFootprint {
    pub state_candid_bytes: u64,
    pub active_payout_job_candid_bytes: u64,
    pub last_summary_candid_bytes: u64,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    crate::state::with_state(|st| DebugState {
        last_successful_transfer_ts: st.last_successful_transfer_ts,
        last_rescue_check_ts: st.last_rescue_check_ts,
        rescue_triggered: st.rescue_triggered,
        active_payout_job_present: st.active_payout_job.is_some(),
        last_summary_present: st.last_summary.is_some(),
        blackhole_controller: st.config.blackhole_controller,
        blackhole_armed_since_ts: st.blackhole_armed_since_ts,
        forced_rescue_reason: st.forced_rescue_reason.clone(),
        consecutive_index_anchor_failures: st.consecutive_index_anchor_failures.unwrap_or(0),
        consecutive_index_latest_invariant_failures: st.consecutive_index_latest_invariant_failures.unwrap_or(0),
        consecutive_index_latest_unreadable_failures: st.consecutive_index_latest_unreadable_failures.unwrap_or(0),
        consecutive_cmc_zero_success_runs: st.consecutive_cmc_zero_success_runs.unwrap_or(0),
        last_observed_staking_balance_e8s: st.last_observed_staking_balance_e8s,
        last_observed_latest_tx_id: st.last_observed_latest_tx_id,
        expected_first_staking_tx_id: st.config.expected_first_staking_tx_id,
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_last_summary() -> Option<crate::state::Summary> {
    crate::state::with_state(|st| st.last_summary.clone())
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_accounts() -> DebugAccounts {
    crate::state::with_state(|st| DebugAccounts {
        payout: Account {
            owner: ic_cdk::api::canister_self(),
            subaccount: st.config.payout_subaccount,
        },
        staking: st.config.staking_account.clone(),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_footprint() -> DebugFootprint {
    crate::state::with_state(|st| DebugFootprint {
        state_candid_bytes: candid::encode_one(st).expect("encode state").len() as u64,
        active_payout_job_candid_bytes: st
            .active_payout_job
            .as_ref()
            .map(|job| candid::encode_one(job).expect("encode active payout job").len() as u64)
            .unwrap_or(0),
        last_summary_candid_bytes: st
            .last_summary
            .as_ref()
            .map(|summary| candid::encode_one(summary).expect("encode summary").len() as u64)
            .unwrap_or(0),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_reset_runtime_state() {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    crate::state::with_state_mut(|st| {
        st.last_summary = None;
        st.last_successful_transfer_ts = None;
        st.last_rescue_check_ts = 0;
        st.rescue_triggered = false;
        st.blackhole_armed_since_ts = st.config.blackhole_armed.unwrap_or(false).then_some(now_secs);
        st.forced_rescue_reason = None;
        st.consecutive_index_anchor_failures = Some(0);
        st.consecutive_index_latest_invariant_failures = Some(0);
        st.consecutive_index_latest_unreadable_failures = Some(0);
        st.consecutive_cmc_zero_success_runs = Some(0);
        st.last_observed_staking_balance_e8s = None;
        st.last_observed_latest_tx_id = None;
        validate_config(&st.config);
        st.main_lock_expires_at_ts = Some(0);
        st.active_payout_job = None;
        st.last_main_run_ts = now_secs.saturating_sub(10 * 365 * 24 * 60 * 60);
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_successful_transfer_ts(ts: Option<u64>) {
    crate::state::with_state_mut(|st| st.last_successful_transfer_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_blackhole_armed(v: Option<bool>) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    crate::state::with_state_mut(|st| {
        st.config.blackhole_armed = v;
        st.blackhole_armed_since_ts = v.unwrap_or(false).then_some(now_secs);
        if !v.unwrap_or(false) {
            st.rescue_triggered = false;
        }
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_blackhole_armed_since_ts(ts: Option<u64>) {
    crate::state::with_state_mut(|st| st.blackhole_armed_since_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_expected_first_staking_tx_id(v: Option<u64>) {
    crate::state::with_state_mut(|st| st.config.expected_first_staking_tx_id = v);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_main_lock_expires_at_ts(ts: Option<u64>) {
    crate::state::with_state_mut(|st| st.main_lock_expires_at_ts = Some(ts.unwrap_or(0)));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_clear_forced_rescue() {
    crate::state::with_state_mut(|st| {
        st.forced_rescue_reason = None;
        st.consecutive_index_anchor_failures = Some(0);
        st.consecutive_index_latest_invariant_failures = Some(0);
        st.consecutive_index_latest_unreadable_failures = Some(0);
        st.consecutive_cmc_zero_success_runs = Some(0);
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    crate::scheduler::debug_set_trap_after_successful_transfers(n);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_real_trap_after_successful_transfers(n: Option<u32>) {
    crate::scheduler::debug_set_real_trap_after_successful_transfers(n);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_main_tick() {
    crate::scheduler::debug_main_tick_impl().await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_rescue_tick() {
    crate::scheduler::debug_rescue_tick_impl().await;
}


#[cfg(test)]
mod tests {
    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn sample_account() -> Account {
        Account {
            owner: principal("22255-zqaaa-aaaas-qf6uq-cai"),
            subaccount: None,
        }
    }

    fn sample_config() -> crate::state::Config {
        crate::state::Config {
            staking_account: sample_account(),
            payout_subaccount: None,
            ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
            index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
            cmc_canister_id: principal("rkp4c-7iaaa-aaaaa-aaaca-cai"),
            rescue_controller: principal("acjuz-liaaa-aaaar-qb4qq-cai"),
            blackhole_controller: Some(principal("e3mmv-5qaaa-aaaah-aadma-cai")),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: None,
            main_interval_seconds: 60,
            rescue_interval_seconds: 60,
            min_tx_e8s: 100_000_000,
        }
    }

    #[test]
    fn validate_config_accepts_minimum_supported_threshold() {
        let mut cfg = sample_config();
        cfg.min_tx_e8s = MIN_MIN_TX_E8S;
        validate_config(&cfg);
    }

    #[test]
    #[should_panic(expected = "min_tx_e8s must be at least")]
    fn validate_config_rejects_threshold_below_minimum() {
        let mut cfg = sample_config();
        cfg.min_tx_e8s = MIN_MIN_TX_E8S - 1;
        validate_config(&cfg);
    }

    #[test]
    #[should_panic(expected = "main_interval_seconds must be greater than 0")]
    fn validate_config_rejects_zero_main_interval() {
        let mut cfg = sample_config();
        cfg.main_interval_seconds = 0;
        validate_config(&cfg);
    }

    #[test]
    fn apply_upgrade_args_keeps_runtime_state_and_revalidates() {
        let now_secs = 123;
        let mut st = State::new(sample_config(), now_secs);
        st.main_lock_expires_at_ts = Some(99);
        apply_upgrade_args_to_state(
            &mut st,
            Some(UpgradeArgs {
                blackhole_controller: Some(principal("qoctq-giaaa-aaaaa-aaaea-cai")),
                blackhole_armed: Some(true),
                clear_forced_rescue: Some(true),
            }),
            now_secs,
        );
        assert_eq!(st.config.blackhole_controller, Some(principal("qoctq-giaaa-aaaaa-aaaea-cai")));
        assert_eq!(st.config.blackhole_armed, Some(true));
        assert_eq!(st.blackhole_armed_since_ts, Some(now_secs));
        assert_eq!(st.main_lock_expires_at_ts, Some(0));
    }
}

ic_cdk::export_candid!();
