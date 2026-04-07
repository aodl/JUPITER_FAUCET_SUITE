mod clients;
mod logic;
mod nns_types;
mod policy;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;

use crate::state::State;
#[cfg(feature = "debug_api")]
use crate::state::ForcedRescueReason;

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub neuron_id: u64,

    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,

    pub ledger_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,

    pub rescue_controller: Principal,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,

    pub main_interval_seconds: Option<u64>,
    pub rescue_interval_seconds: Option<u64>,
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

fn mainnet_governance_id() -> Principal {
    Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").expect("invalid hardcoded governance principal")
}

fn mainnet_blackhole_id() -> Principal {
    Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").expect("invalid hardcoded blackhole principal")
}

#[cfg(any(test, feature = "debug_api"))]
fn production_canister_id() -> Principal {
    Principal::from_text(env!("JUPITER_DISBURSER_PROD_CANISTER_ID")).expect("invalid embedded production canister principal")
}

#[cfg(any(test, feature = "debug_api"))]
fn is_production_canister(principal: Principal) -> bool {
    principal == production_canister_id()
}

#[cfg(feature = "debug_api")]
fn guard_debug_api_not_production() {
    if is_production_canister(ic_cdk::api::canister_self()) {
        ic_cdk::trap("debug_api is disabled for the production canister");
    }
}

fn self_canister_principal_for_validation() -> Principal {
    #[cfg(test)]
    {
        // Unit tests do not run inside a real canister context, so use a stable non-anonymous
        // stand-in principal to model the disburser's staging account for validation-only checks.
        Principal::management_canister()
    }
    #[cfg(not(test))]
    {
        ic_cdk::api::canister_self()
    }
}

fn assert_non_anonymous_principal(name: &str, principal: Principal) {
    assert!(principal != Principal::anonymous(), "{name} must not be the anonymous principal");
}

fn validate_config(cfg: &crate::state::Config) {
    assert!(cfg.neuron_id != 0, "neuron_id must be non-zero");
    assert_non_anonymous_principal("ledger_canister_id", cfg.ledger_canister_id);
    assert_non_anonymous_principal("governance_canister_id", cfg.governance_canister_id);
    assert_non_anonymous_principal("rescue_controller", cfg.rescue_controller);
    if let Some(blackhole_controller) = cfg.blackhole_controller {
        assert_non_anonymous_principal("blackhole_controller", blackhole_controller);
    }
    assert!(cfg.main_interval_seconds > 0, "main_interval_seconds must be greater than 0");
    assert!(cfg.rescue_interval_seconds > 0, "rescue_interval_seconds must be greater than 0");

    let staging_account = Account {
        owner: self_canister_principal_for_validation(),
        subaccount: None,
    };
    assert!(cfg.normal_recipient != staging_account, "normal_recipient must not equal the disburser staging account");
    assert!(cfg.age_bonus_recipient_1 != staging_account, "age_bonus_recipient_1 must not equal the disburser staging account");
    assert!(cfg.age_bonus_recipient_2 != staging_account, "age_bonus_recipient_2 must not equal the disburser staging account");

    assert!(cfg.normal_recipient != cfg.age_bonus_recipient_1, "normal_recipient and age_bonus_recipient_1 must be distinct");
    assert!(cfg.normal_recipient != cfg.age_bonus_recipient_2, "normal_recipient and age_bonus_recipient_2 must be distinct");
    assert!(cfg.age_bonus_recipient_1 != cfg.age_bonus_recipient_2, "age_bonus_recipient_1 and age_bonus_recipient_2 must be distinct");
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
        blackhole_controller: Some(args.blackhole_controller.unwrap_or_else(mainnet_blackhole_id)),
        blackhole_armed: args.blackhole_armed,
        main_interval_seconds: args.main_interval_seconds.unwrap_or(86_400),
        rescue_interval_seconds: args.rescue_interval_seconds.unwrap_or(86_400),
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
    pub prev_age_seconds: u64,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub payout_plan_present: bool,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugConfig {
    pub neuron_id: u64,
    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,
    pub ledger_canister_id: Principal,
    pub governance_canister_id: Principal,
    pub rescue_controller: Principal,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,
    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    guard_debug_api_not_production();
    crate::state::with_state(|st| DebugState {
        prev_age_seconds: st.prev_age_seconds,
        last_successful_transfer_ts: st.last_successful_transfer_ts,
        last_rescue_check_ts: st.last_rescue_check_ts,
        rescue_triggered: st.rescue_triggered,
        payout_plan_present: st.payout_plan.is_some(),
        blackhole_controller: st.config.blackhole_controller,
        blackhole_armed_since_ts: st.blackhole_armed_since_ts,
        forced_rescue_reason: st.forced_rescue_reason.clone(),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_config() -> DebugConfig {
    guard_debug_api_not_production();
    crate::state::with_state(|st| DebugConfig {
        neuron_id: st.config.neuron_id,
        normal_recipient: st.config.normal_recipient.clone(),
        age_bonus_recipient_1: st.config.age_bonus_recipient_1.clone(),
        age_bonus_recipient_2: st.config.age_bonus_recipient_2.clone(),
        ledger_canister_id: st.config.ledger_canister_id,
        governance_canister_id: st.config.governance_canister_id,
        rescue_controller: st.config.rescue_controller,
        blackhole_controller: st.config.blackhole_controller,
        blackhole_armed: st.config.blackhole_armed,
        main_interval_seconds: st.config.main_interval_seconds,
        rescue_interval_seconds: st.config.rescue_interval_seconds,
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state_size_bytes() -> u64 {
    guard_debug_api_not_production();
    let st = crate::state::get_state();
    match candid::encode_one(st) {
        Ok(bytes) => bytes.len() as u64,
        Err(_) => 0,
    }
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_main_tick() {
    guard_debug_api_not_production();
    crate::scheduler::debug_main_tick_impl().await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_rescue_tick() {
    guard_debug_api_not_production();
    crate::scheduler::debug_rescue_tick_impl().await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_prev_age_seconds(age_seconds: u64) {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.prev_age_seconds = age_seconds);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_successful_transfer_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.last_successful_transfer_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_last_rescue_check_ts(ts: u64) {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.last_rescue_check_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_blackhole_armed_since_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.blackhole_armed_since_ts = ts);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_main_lock_expires_at_ts(ts: Option<u64>) {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.main_lock_expires_at_ts = Some(ts.unwrap_or(0)));
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_clear_forced_rescue() {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.forced_rescue_reason = None);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_pause_after_planning(enabled: bool) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_pause_after_planning(enabled);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_trap_after_successful_transfers(n);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_real_trap_after_successful_transfers(n: Option<u32>) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_real_trap_after_successful_transfers(n);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_simulate_low_cycles(enabled: bool) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_simulate_low_cycles(enabled);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_skip_maturity_initiation(enabled: bool) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_skip_maturity_initiation(enabled);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_build_payout_plan() -> bool {
    guard_debug_api_not_production();
    crate::scheduler::debug_build_payout_plan_impl().await
}


#[cfg(test)]
mod tests {
    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn account(owner: Principal, subaccount: Option<[u8; 32]>) -> Account {
        Account { owner, subaccount }
    }

    fn sample_config() -> crate::state::Config {
        crate::state::Config {
            neuron_id: 1,
            normal_recipient: account(principal("ryjl3-tyaaa-aaaaa-aaaba-cai"), None),
            age_bonus_recipient_1: account(principal("qhbym-qaaaa-aaaaa-aaafq-cai"), None),
            age_bonus_recipient_2: account(principal("rrkah-fqaaa-aaaaa-aaaaq-cai"), Some([7u8; 32])),
            ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
            governance_canister_id: principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
            rescue_controller: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
            blackhole_controller: Some(principal("e3mmv-5qaaa-aaaah-aadma-cai")),
            blackhole_armed: Some(false),
            main_interval_seconds: 60,
            rescue_interval_seconds: 60,
        }
    }

    #[test]
    fn validate_config_accepts_distinct_recipients() {
        validate_config(&sample_config());
    }

    #[test]
    #[should_panic(expected = "must be distinct")]
    fn validate_config_rejects_duplicate_recipients() {
        let mut cfg = sample_config();
        cfg.age_bonus_recipient_1 = cfg.normal_recipient;
        validate_config(&cfg);
    }

    #[test]
    #[should_panic(expected = "must not equal the disburser staging account")]
    fn validate_config_rejects_staging_account_recipient() {
        let mut cfg = sample_config();
        cfg.normal_recipient = Account { owner: Principal::management_canister(), subaccount: None };
        validate_config(&cfg);
    }

    #[test]
    fn apply_upgrade_args_revalidates_config() {
        let now_secs = 99;
        let mut st = State::new(sample_config(), now_secs);
        apply_upgrade_args_to_state(
            &mut st,
            Some(UpgradeArgs {
                blackhole_controller: Some(principal("qhbym-qaaaa-aaaaa-aaafq-cai")),
                blackhole_armed: Some(true),
                clear_forced_rescue: Some(true),
            }),
            now_secs,
        );
        assert_eq!(st.config.blackhole_controller, Some(principal("qhbym-qaaaa-aaaaa-aaafq-cai")));
        assert_eq!(st.blackhole_armed_since_ts, Some(now_secs));
        assert_eq!(st.main_lock_expires_at_ts, Some(0));
    }


    #[test]
    fn production_canister_detection_matches_expected_id() {
        assert!(is_production_canister(production_canister_id()));
        assert!(!is_production_canister(principal("aaaaa-aa")));
    }
}

ic_cdk::export_candid!();
