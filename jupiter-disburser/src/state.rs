use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Config {
    pub neuron_id: u64,

    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,

    pub ledger_canister_id: Principal,
    pub governance_canister_id: Principal,

    pub rescue_controller: Principal,
    pub blackhole_armed: Option<bool>,

    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Sent { block_index: String },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct PlannedTransfer {
    pub to: Account,
    pub gross_share_e8s: u64, // includes its own fee; used for accounting/logging
    pub amount_e8s: u64,      // net amount in TransferArg.amount (gross - fee)
    pub created_at_time_nanos: u64,
    pub memo: Vec<u8>,
    pub status: TransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct PayoutPlan {
    pub id: u64,
    pub fee_e8s: u64,
    pub created_at_base_nanos: u64,
    pub transfers: Vec<PlannedTransfer>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct State {
    pub config: Config,

    /// Stored at initiation time; used to split staging balance next cycle.
    pub prev_age_seconds: u64,

    /// Updated on Ok/Duplicate transfers.
    pub last_successful_transfer_ts: Option<u64>,

    /// Timestamp of the last successful controller update performed by rescue logic.
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,

    /// Legacy lock fields retained for stable-state compatibility.
    /// Execution no longer depends on either boolean.
    pub main_lock: bool,
    pub rescue_lock: bool,

    /// Expiring lease used to suppress overlapping main ticks without risking a
    /// permanent wedge if execution stops after an await boundary.
    pub main_lock_expires_at_ts: Option<u64>,

    /// Deterministic, persisted payout plan for idempotent retries.
    pub payout_nonce: u64,
    pub payout_plan: Option<PayoutPlan>,

    /// Last time main tick ran successfully (for duplicate timer calls).
    pub last_main_run_ts: u64,
}

impl State {
    pub fn new(config: Config, now_secs: u64) -> Self {
        Self {
            config,
            prev_age_seconds: 0,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            main_lock: false,
            rescue_lock: false,
            main_lock_expires_at_ts: Some(0),
            payout_nonce: 1,
            payout_plan: None,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60), // far in past
        }
    }
}

thread_local! {
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
}

pub fn set_state(st: State) {
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub fn get_state() -> State {
    STATE.with(|s| s.borrow().clone()).expect("state not initialized")
}

pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("state not initialized")))
}
