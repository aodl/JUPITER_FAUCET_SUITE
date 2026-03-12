use candid::{CandidType, Deserialize, Nat, Principal};
use std::cell::RefCell;

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct NotifyTopUpArg {
    pub canister_id: Principal,
    pub block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct NotifyRecord {
    pub canister_id: Principal,
    pub block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum NotifyTopUpResult {
    Ok(Nat),
    Err(NotifyError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum NotifyError {
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    Processing,
    TransactionTooOld(u64),
    InvalidTransaction(String),
    Other {
        error_code: u64,
        error_message: String,
    },
}

#[derive(Default)]
struct State {
    fail: bool,
    notifications: Vec<NotifyRecord>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn notify_top_up(arg: NotifyTopUpArg) -> NotifyTopUpResult {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        if st.fail {
            return NotifyTopUpResult::Err(NotifyError::Processing);
        }
        st.notifications.push(NotifyRecord {
            canister_id: arg.canister_id,
            block_index: arg.block_index,
        });
        NotifyTopUpResult::Ok(Nat::from(0_u8))
    })
}

#[ic_cdk::update]
fn debug_reset() {
    ST.with(|s| *s.borrow_mut() = State::default());
}

#[ic_cdk::update]
fn debug_set_fail(v: bool) {
    ST.with(|s| s.borrow_mut().fail = v);
}

#[ic_cdk::query]
fn debug_notifications() -> Vec<NotifyRecord> {
    ST.with(|s| s.borrow().notifications.clone())
}

ic_cdk::export_candid!();
