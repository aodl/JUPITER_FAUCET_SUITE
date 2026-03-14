use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Config {
    pub staking_account: Account,
    pub payout_subaccount: Option<[u8; 32]>,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub cmc_canister_id: Principal,
    pub rescue_controller: Principal,
    pub blackhole_armed: Option<bool>,
    pub expected_first_staking_tx_id: Option<u64>,
    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
    pub min_tx_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Beneficiary,
    RemainderToSelf,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingNotification {
    pub kind: TransferKind,
    pub beneficiary: Principal,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub block_index: u64,
    pub next_start: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum RetryStep {
    Transfer,
    Notify,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct RetryState {
    pub step: RetryStep,
    pub pending: PendingNotification,
    pub fee_e8s: u64,
    pub created_at_time_nanos: u64,
    pub retry_at_secs: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum ForcedRescueReason {
    BootstrapNoSuccess,
    IndexAnchorMissing,
    IndexLatestInvariantBroken,
    CmcZeroSuccessRuns,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub pot_start_e8s: u64,
    pub pot_remaining_e8s: u64,
    pub denom_staking_balance_e8s: u64,
    pub topped_up_count: u64,
    pub topped_up_sum_e8s: u64,
    pub topped_up_min_e8s: Option<u64>,
    pub topped_up_max_e8s: Option<u64>,
    pub failed_topups: u64,
    pub ignored_under_threshold: u64,
    pub ignored_bad_memo: u64,
    pub remainder_to_self_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct ActivePayoutJob {
    pub id: u64,
    pub fee_e8s: u64,
    pub pot_start_e8s: u64,
    pub denom_staking_balance_e8s: u64,
    pub next_start: Option<u64>,
    pub scan_complete: bool,
    pub ignored_under_threshold: u64,
    pub ignored_bad_memo: u64,
    pub gross_outflow_e8s: u64,
    pub topped_up_count: u64,
    pub topped_up_sum_e8s: u64,
    pub topped_up_min_e8s: Option<u64>,
    pub topped_up_max_e8s: Option<u64>,
    pub failed_topups: u64,
    pub remainder_to_self_e8s: u64,
    pub next_created_at_time_nanos: u64,
    pub retry_state: Option<RetryState>,
    pub observed_oldest_tx_id: Option<u64>,
    pub observed_latest_tx_id: Option<u64>,
    pub cmc_attempt_count: Option<u64>,
    pub cmc_success_count: Option<u64>,
}

impl ActivePayoutJob {
    pub fn new(id: u64, fee_e8s: u64, pot_start_e8s: u64, denom_staking_balance_e8s: u64, created_at_time_nanos: u64) -> Self {
        Self {
            id,
            fee_e8s,
            pot_start_e8s,
            denom_staking_balance_e8s,
            next_start: None,
            scan_complete: false,
            ignored_under_threshold: 0,
            ignored_bad_memo: 0,
            gross_outflow_e8s: 0,
            topped_up_count: 0,
            topped_up_sum_e8s: 0,
            topped_up_min_e8s: None,
            topped_up_max_e8s: None,
            failed_topups: 0,
            remainder_to_self_e8s: 0,
            next_created_at_time_nanos: created_at_time_nanos,
            retry_state: None,
            observed_oldest_tx_id: None,
            observed_latest_tx_id: None,
            cmc_attempt_count: Some(0),
            cmc_success_count: Some(0),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct State {
    pub config: Config,
    pub last_summary: Option<Summary>,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub blackhole_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
    pub consecutive_index_anchor_failures: Option<u8>,
    pub consecutive_index_latest_invariant_failures: Option<u8>,
    pub consecutive_cmc_zero_success_runs: Option<u8>,
    pub last_observed_staking_balance_e8s: Option<u64>,
    pub last_observed_latest_tx_id: Option<u64>,
    pub main_lock_expires_at_ts: Option<u64>,
    pub payout_nonce: u64,
    pub active_payout_job: Option<ActivePayoutJob>,
    pub last_main_run_ts: u64,
}

impl State {
    pub fn new(config: Config, now_secs: u64) -> Self {
        let blackhole_armed_since_ts = config.blackhole_armed.unwrap_or(false).then_some(now_secs);
        Self {
            config,
            last_summary: None,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            blackhole_armed_since_ts,
            forced_rescue_reason: None,
            consecutive_index_anchor_failures: Some(0),
            consecutive_index_latest_invariant_failures: Some(0),
            consecutive_cmc_zero_success_runs: Some(0),
            last_observed_staking_balance_e8s: None,
            last_observed_latest_tx_id: None,
            main_lock_expires_at_ts: Some(0),
            payout_nonce: 1,
            active_payout_job: None,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
        }
    }
}

thread_local! {
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
}

pub fn set_state(st: State) { STATE.with(|s| *s.borrow_mut() = Some(st)); }
pub fn get_state() -> State { STATE.with(|s| s.borrow().clone()).expect("state not initialized") }
pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R { STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized"))) }
pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R { STATE.with(|s| f(s.borrow_mut().as_mut().expect("state not initialized"))) }
