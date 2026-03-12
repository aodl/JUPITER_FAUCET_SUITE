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
    pub blackhole_armed: Option<bool>,

    pub main_interval_seconds: Option<u64>,
    pub rescue_interval_seconds: Option<u64>,
    pub min_tx_e8s: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct UpgradeArgs {
    pub blackhole_armed: Option<bool>,
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
        blackhole_armed: args.blackhole_armed,
        main_interval_seconds: args.main_interval_seconds.unwrap_or(7 * 24 * 60 * 60),
        rescue_interval_seconds: args.rescue_interval_seconds.unwrap_or(24 * 60 * 60),
        min_tx_e8s: args.min_tx_e8s.unwrap_or(10_000_000),
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

#[ic_cdk::post_upgrade]
fn post_upgrade(args: Option<UpgradeArgs>) {
    let (mut st,): (State,) = ic_cdk::storage::stable_restore().expect("stable_restore failed");
    if let Some(args) = args {
        if args.blackhole_armed.is_some() {
            st.config.blackhole_armed = args.blackhole_armed;
        }
    }
    crate::state::set_state(st);
    crate::scheduler::install_timers();
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub active_payout_job_present: bool,
    pub pending_notification_present: bool,
    pub last_summary_present: bool,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugAccounts {
    pub payout: Account,
    pub staking: Account,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    crate::state::with_state(|st| DebugState {
        last_successful_transfer_ts: st.last_successful_transfer_ts,
        last_rescue_check_ts: st.last_rescue_check_ts,
        rescue_triggered: st.rescue_triggered,
        active_payout_job_present: st.active_payout_job.is_some(),
        pending_notification_present: st.active_payout_job.as_ref().and_then(|j| j.pending_notification.as_ref()).is_some(),
        last_summary_present: st.last_summary.is_some(),
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
#[ic_cdk::update]
async fn debug_main_tick() {
    crate::scheduler::debug_main_tick_impl().await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_rescue_tick() {
    crate::scheduler::debug_rescue_tick_impl().await;
}

ic_cdk::export_candid!();
