use candid::Nat;
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::{cell::RefCell, time::Duration};

use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::index::{account_identifier_text, IcpIndexCanister};
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{CmcClient, IndexClient, LedgerClient};
use crate::state::{ActivePayoutJob, PendingNotification, TransferKind};
use crate::{logic, policy, state};

thread_local! { static LAST_ERR_CODE: RefCell<Option<u32>> = RefCell::new(None); }
const PAGE_SIZE: u64 = 500;
const MAIN_LOCK_LEASE_SECONDS: u64 = 15 * 60;

fn log_error(code: u32) {
    LAST_ERR_CODE.with(|c| {
        let mut c = c.borrow_mut();
        if *c == Some(code) { return; }
        *c = Some(code);
        ic_cdk::println!("ERR:{}", code);
    });
}
fn log_cycles() { let cycles: u128 = ic_cdk::api::canister_cycle_balance(); ic_cdk::println!("Cycles: {}", cycles); }
fn try_acquire_main_lease(now_secs: u64) -> bool {
    state::with_state_mut(|st| {
        st.main_lock = false;
        let expires_at = st.main_lock_expires_at_ts.unwrap_or(0);
        if expires_at > now_secs { return false; }
        st.main_lock_expires_at_ts = Some(now_secs.saturating_add(MAIN_LOCK_LEASE_SECONDS));
        true
    })
}

pub fn install_timers() {
    let (main_s, rescue_s) = state::with_state(|st| (st.config.main_interval_seconds, st.config.rescue_interval_seconds));
    ic_cdk_timers::set_timer_interval(Duration::from_secs(main_s.max(60)), || async { main_tick().await; });
    ic_cdk_timers::set_timer_interval(Duration::from_secs(rescue_s.max(60)), || async { rescue_tick().await; });
}

async fn main_tick() {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    if !try_acquire_main_lease(now_secs) { return; }
    let min_gap = state::with_state(|st| st.config.main_interval_seconds.saturating_sub(60));
    let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
    if recently_ran { finish_main(now_secs, None); return; }
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = IcpIndexCanister::new(cfg.index_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let ok = process_payout(&ledger, &index, &cmc, now_nanos, now_secs).await;
    finish_main(now_secs, if ok { None } else { Some(3001) });
}

fn finish_main(now_secs: u64, err: Option<u32>) {
    state::with_state_mut(|st| { st.last_main_run_ts = now_secs; st.main_lock = false; st.main_lock_expires_at_ts = Some(0); });
    if let Some(code) = err { log_error(code); }
    log_cycles();
}

fn payout_account() -> Account {
    let payout_subaccount = state::with_state(|st| st.config.payout_subaccount);
    Account { owner: ic_cdk::api::canister_self(), subaccount: payout_subaccount }
}

fn next_created_at_time_nanos() -> u64 {
    state::with_state(|st| st.active_payout_job.as_ref().expect("active payout job missing").next_created_at_time_nanos)
}
fn advance_created_at_time_nanos() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.next_created_at_time_nanos = job.next_created_at_time_nanos.saturating_add(1); }); }
fn increment_failed_topups() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.failed_topups = job.failed_topups.saturating_add(1); }); }
fn set_pending_notification(pending: PendingNotification) { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.pending_notification = Some(pending); }); }
fn record_successful_notification(now_secs: u64, pending: &PendingNotification) {
    state::with_state_mut(|st| {
        st.last_successful_transfer_ts = Some(now_secs);
        if let Some(job) = st.active_payout_job.as_mut() { logic::apply_notified_transfer(job, pending); }
    });
}
fn finalize_completed_job() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.take() { st.last_summary = Some(logic::summary_from_job(&job)); }); }

fn transfer_arg(to: Account, amount_e8s: u64, fee_e8s: u64, created_at_time_nanos: u64) -> TransferArg {
    let memo_bytes = logic::MEMO_TOP_UP_CANISTER_U64.to_be_bytes().to_vec();
    TransferArg {
        from_subaccount: state::with_state(|st| st.config.payout_subaccount),
        to,
        fee: Some(Nat::from(fee_e8s)),
        created_at_time: Some(created_at_time_nanos),
        memo: Some(Memo::from(memo_bytes)),
        amount: Nat::from(amount_e8s),
    }
}

async fn send_and_notify(ledger: &impl LedgerClient, cmc: &impl CmcClient, pending: PendingNotification, to: Account, fee_e8s: u64, now_secs: u64) -> bool {
    let created_at_time_nanos = next_created_at_time_nanos();
    let arg = transfer_arg(to, pending.amount_e8s, fee_e8s, created_at_time_nanos);
    let ledger_res = match ledger.transfer(arg).await { Ok(r) => r, Err(_) => { increment_failed_topups(); return false; } };
    let block_index = match ledger_res {
        Ok(block) => block.to_string().parse::<u64>().unwrap_or(0),
        Err(TransferError::Duplicate { duplicate_of }) => duplicate_of.to_string().parse::<u64>().unwrap_or(0),
        Err(_) => { increment_failed_topups(); return false; }
    };
    advance_created_at_time_nanos();
    let pending = PendingNotification { block_index, ..pending };
    if cmc.notify_top_up(pending.beneficiary, pending.block_index).await.is_err() {
        increment_failed_topups();
        set_pending_notification(pending);
        return false;
    }
    record_successful_notification(now_secs, &pending);
    true
}

async fn retry_pending_notification(cmc: &impl CmcClient, now_secs: u64) -> bool {
    let pending = state::with_state(|st| st.active_payout_job.as_ref().and_then(|j| j.pending_notification.clone()));
    let Some(pending) = pending else { return true; };
    if cmc.notify_top_up(pending.beneficiary, pending.block_index).await.is_err() { increment_failed_topups(); return false; }
    record_successful_notification(now_secs, &pending);
    true
}

fn ensure_active_job(now_nanos: u64, fee_e8s: u64, pot_start_e8s: u64, denom_e8s: u64) {
    state::with_state_mut(|st| {
        if st.active_payout_job.is_some() { return; }
        let id = st.payout_nonce;
        st.payout_nonce = st.payout_nonce.saturating_add(1);
        st.active_payout_job = Some(ActivePayoutJob::new(id, fee_e8s, pot_start_e8s, denom_e8s, now_nanos));
    });
}

async fn process_payout(ledger: &impl LedgerClient, index: &impl IndexClient, cmc: &impl CmcClient, now_nanos: u64, now_secs: u64) -> bool {
    if state::with_state(|st| st.active_payout_job.is_none()) {
        let cfg = state::with_state(|st| st.config.clone());
        let fee_e8s = match ledger.fee_e8s().await { Ok(v) => v, Err(_) => return false };
        let pot_start_e8s = match ledger.balance_of_e8s(payout_account()).await { Ok(v) => v, Err(_) => return false };
        if pot_start_e8s <= fee_e8s { return true; }
        let denom_e8s = match ledger.balance_of_e8s(cfg.staking_account.clone()).await { Ok(v) => v, Err(_) => return false };
        if denom_e8s == 0 { return true; }
        ensure_active_job(now_nanos, fee_e8s, pot_start_e8s, denom_e8s);
    }

    if !retry_pending_notification(cmc, now_secs).await { return false; }
    let cfg = state::with_state(|st| st.config.clone());
    let staking_id = account_identifier_text(&cfg.staking_account);

    loop {
        let job = state::with_state(|st| st.active_payout_job.clone());
        let Some(job) = job else { return true; };
        if job.pending_notification.is_some() {
            if !retry_pending_notification(cmc, now_secs).await { return false; }
            continue;
        }
        if job.scan_complete {
            if job.remainder_to_self_e8s > 0 { finalize_completed_job(); return true; }
            let remainder_gross_e8s = job.pot_start_e8s.saturating_sub(job.beneficiary_gross_sent_sum_e8s);
            if remainder_gross_e8s <= job.fee_e8s { finalize_completed_job(); return true; }
            let self_id = ic_cdk::api::canister_self();
            let pending = PendingNotification { kind: TransferKind::RemainderToSelf, beneficiary: self_id, gross_share_e8s: remainder_gross_e8s, amount_e8s: remainder_gross_e8s.saturating_sub(job.fee_e8s), block_index: 0, next_start: None };
            let to = logic::cmc_deposit_account(cfg.cmc_canister_id, self_id);
            if !send_and_notify(ledger, cmc, pending, to, job.fee_e8s, now_secs).await { return false; }
            finalize_completed_job();
            return true;
        }

        let resp = match index.get_account_identifier_transactions(staking_id.clone(), job.next_start, PAGE_SIZE).await { Ok(v) => v, Err(_) => return false };
        if resp.transactions.is_empty() { state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.scan_complete = true; }); continue; }

        for tx in &resp.transactions {
            let Some(contribution) = logic::memo_bytes_from_index_tx(tx, &staking_id) else {
                state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.next_start = Some(tx.id); });
                continue;
            };
            let snapshot = state::with_state(|st| {
                let job = st.active_payout_job.as_ref().expect("active payout job missing");
                (job.pot_start_e8s, job.denom_staking_balance_e8s, job.fee_e8s, st.config.min_tx_e8s)
            });
            match logic::evaluate_contribution(snapshot.0, snapshot.1, snapshot.2, snapshot.3, &contribution) {
                logic::ContributionDecision::IgnoreUnderThreshold => state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.ignored_under_threshold = job.ignored_under_threshold.saturating_add(1); job.next_start = Some(tx.id); }),
                logic::ContributionDecision::IgnoreBadMemo => state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.ignored_bad_memo = job.ignored_bad_memo.saturating_add(1); job.next_start = Some(tx.id); }),
                logic::ContributionDecision::NoTransfer => state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { job.next_start = Some(tx.id); }),
                logic::ContributionDecision::Eligible { beneficiary, gross_share_e8s, amount_e8s } => {
                    let pending = PendingNotification { kind: TransferKind::Beneficiary, beneficiary, gross_share_e8s, amount_e8s, block_index: 0, next_start: Some(tx.id) };
                    let to = logic::cmc_deposit_account(cfg.cmc_canister_id, beneficiary);
                    if !send_and_notify(ledger, cmc, pending, to, snapshot.2, now_secs).await { return false; }
                }
            }
        }
        let last_id = resp.transactions.last().map(|t| t.id);
        state::with_state_mut(|st| if let Some(job) = st.active_payout_job.as_mut() { if resp.transactions.len() < PAGE_SIZE as usize || last_id.is_none() { job.scan_complete = true; } else { job.next_start = last_id; } });
    }
}

async fn rescue_tick() {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let (blackhole_armed, last_xfer_opt, rescue_controller, rescue_triggered) = state::with_state(|st| (st.config.blackhole_armed.unwrap_or(false), st.last_successful_transfer_ts, st.config.rescue_controller, st.rescue_triggered));
    if !blackhole_armed { return; }
    let self_id = ic_cdk::api::canister_self();
    let desired_opt = policy::desired_controllers(now_secs, last_xfer_opt, self_id, rescue_controller);
    let Some(mut desired) = desired_opt else { return; };
    desired.sort_by(|a, b| a.to_text().cmp(&b.to_text()));
    desired.dedup();
    let rescue_active = desired.iter().any(|p| *p == rescue_controller);
    if !rescue_active && !rescue_triggered { return; }
    let arg = UpdateSettingsArgs { canister_id: self_id, settings: CanisterSettings { controllers: Some(desired), ..Default::default() } };
    if update_settings(&arg).await.is_err() { log_error(3101); return; }
    state::with_state_mut(|st| { st.last_rescue_check_ts = now_secs; st.rescue_triggered = true; });
}

#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() { main_tick().await; }
#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() { rescue_tick().await; }
