mod clients;
mod logic;
mod nns_types;
mod policy;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;

use crate::state::{ForcedRescueReason, State};

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub neuron_id: u64,

    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,

    pub ledger_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,

    pub rescue_controller: Principal,
    pub blackhole_armed: Option<bool>,

    pub main_interval_seconds: Option<u64>,
    pub rescue_interval_seconds: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct UpgradeArgs {
    pub blackhole_armed: Option<bool>,
    pub clear_forced_rescue: Option<bool>,
}

fn mainnet_ledger_id() -> Principal {
    Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid hardcoded ledger principal")
}

fn mainnet_governance_id() -> Principal {
    Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").expect("invalid hardcoded governance principal")
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;

    let cfg = crate::state::Config {
        neuron_id: args.neuron_id,
        normal_recipient: args.normal_recipient,
        age_bonus_recipient_1: args.age_bonus_recipient_1,
        age_bonus_recipient_2: args.age_bonus_recipient_2,
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        governance_canister_id: args.governance_canister_id.unwrap_or_else(mainnet_governance_id),
        rescue_controller: args.rescue_controller,
        blackhole_armed: args.blackhole_armed,
        main_interval_seconds: args.main_interval_seconds.unwrap_or(86_400),
        rescue_interval_seconds: args.rescue_interval_seconds.unwrap_or(86_400),
    };

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
        if let Some(armed) = args.blackhole_armed {
            st.config.blackhole_armed = Some(armed);
            st.blackhole_armed_since_ts = if armed { Some(now_secs) } else { None };
            if !armed {
                st.rescue_triggered = false;
            }
        }
        if args.clear_forced_rescue.unwrap_or(false) {
            st.forced_rescue_reason = None;
        }
    }
    st.main_lock_expires_at_ts = Some(0);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(args: Option<UpgradeArgs>) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let (mut st,): (State,) = ic_cdk::storage::stable_restore().expect("stable_restore failed");
    apply_upgrade_args_to_state(&mut st, args, now_secs);
    crate::state::set_state(st);
    crate::scheduler::install_timers();
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub prev_age_seconds: u64,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub payout_plan_present: bool,
    pub blackhole_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    crate::state::with_state(|st| DebugState {
        prev_age_seconds: st.prev_age_seconds,
        last_successful_transfer_ts: st.last_successful_transfer_ts,
        last_rescue_check_ts: st.last_rescue_check_ts,
        rescue_triggered: st.rescue_triggered,
        payout_plan_present: st.payout_plan.is_some(),
        blackhole_armed_since_ts: st.blackhole_armed_since_ts,
        forced_rescue_reason: st.forced_rescue_reason.clone(),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state_size_bytes() -> u64 {
    let st = crate::state::get_state();
    match candid::encode_one(st) {
        Ok(bytes) => bytes.len() as u64,
        Err(_) => 0,
    }
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

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_prev_age_seconds(age_seconds: u64) {
    crate::state::with_state_mut(|st| st.prev_age_seconds = age_seconds);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_successful_transfer_ts(ts: Option<u64>) {
    crate::state::with_state_mut(|st| st.last_successful_transfer_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_rescue_check_ts(ts: u64) {
    crate::state::with_state_mut(|st| st.last_rescue_check_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_blackhole_armed_since_ts(ts: Option<u64>) {
    crate::state::with_state_mut(|st| st.blackhole_armed_since_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_clear_forced_rescue() {
    crate::state::with_state_mut(|st| st.forced_rescue_reason = None);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_pause_after_planning(enabled: bool) {
    crate::scheduler::debug_set_pause_after_planning(enabled);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    crate::scheduler::debug_set_trap_after_successful_transfers(n);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_simulate_low_cycles(enabled: bool) {
    crate::scheduler::debug_set_simulate_low_cycles(enabled);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_skip_maturity_initiation(enabled: bool) {
    crate::scheduler::debug_set_skip_maturity_initiation(enabled);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_build_payout_plan() -> bool {
    crate::scheduler::debug_build_payout_plan_impl().await
}

ic_cdk::export_candid!();
