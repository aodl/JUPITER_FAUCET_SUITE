mod clients;
mod logic;
mod nns_types;
mod policy;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;

use crate::state::State;

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub neuron_id: u64,

    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,

    pub ledger_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,

    pub rescue_controller: Principal,

    pub main_interval_seconds: Option<u64>,
    pub rescue_interval_seconds: Option<u64>,
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
    // On upgrade, it's better to abort upgrade than continue with lost state.
    ic_cdk::storage::stable_save((st,)).expect("stable_save failed");
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    // On upgrade, we expect stable state to exist.
    let (st,): (State,) = ic_cdk::storage::stable_restore().expect("stable_restore failed");
    crate::state::set_state(st);
    crate::scheduler::install_timers();
}

#[derive(CandidType, Deserialize)]
pub struct Metrics {
    pub prev_age_seconds: u64,
    pub last_successful_transfer_ts: Option<u64>,
    pub rescue_triggered: bool,
}

#[ic_cdk::query]
fn metrics() -> Metrics {
    crate::state::with_state(|st| Metrics {
        prev_age_seconds: st.prev_age_seconds,
        last_successful_transfer_ts: st.last_successful_transfer_ts,
        rescue_triggered: st.rescue_triggered,
    })
}

// ---------------- Debug-only API (feature-gated) ----------------

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub prev_age_seconds: u64,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub payout_plan_present: bool,
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
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state_size_bytes() -> u64 {
    // stable_save uses candid encoding; this approximates stable state size.
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



