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

#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum DebugNotifyBehavior {
    Ok,
    Processing,
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
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
    scripted_behaviors: Vec<DebugNotifyBehavior>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

#[ic_cdk::init]
fn init() {}


#[ic_cdk::update]
async fn notify_top_up(arg: NotifyTopUpArg) -> NotifyTopUpResult {
    let scripted = ST.with(|s| {
        let mut st = s.borrow_mut();
        st.scripted_behaviors.first().cloned().map(|behavior| {
            st.scripted_behaviors.remove(0);
            behavior
        })
    });

    if let Some(behavior) = scripted {
        return ST.with(|s| {
            let mut st = s.borrow_mut();
            match behavior {
                DebugNotifyBehavior::Ok => {
                    st.notifications.push(NotifyRecord {
                        canister_id: arg.canister_id,
                        block_index: arg.block_index,
                    });
                    NotifyTopUpResult::Ok(Nat::from(0_u8))
                }
                DebugNotifyBehavior::Processing => NotifyTopUpResult::Err(NotifyError::Processing),
                DebugNotifyBehavior::Refunded { reason, block_index } => {
                    NotifyTopUpResult::Err(NotifyError::Refunded { reason, block_index })
                }
                DebugNotifyBehavior::TransactionTooOld(v) => {
                    NotifyTopUpResult::Err(NotifyError::TransactionTooOld(v))
                }
                DebugNotifyBehavior::InvalidTransaction(msg) => {
                    NotifyTopUpResult::Err(NotifyError::InvalidTransaction(msg))
                }
                DebugNotifyBehavior::Other { error_code, error_message } => {
                    NotifyTopUpResult::Err(NotifyError::Other {
                        error_code,
                        error_message,
                    })
                }
            }
        });
    }

    if ST.with(|s| s.borrow().fail) {
        return NotifyTopUpResult::Err(NotifyError::Processing);
    }


    ST.with(|s| {
        let mut st = s.borrow_mut();
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

#[ic_cdk::update]
fn debug_set_script(behaviors: Vec<DebugNotifyBehavior>) {
    ST.with(|s| s.borrow_mut().scripted_behaviors = behaviors);
}

#[ic_cdk::query]
fn debug_notifications() -> Vec<NotifyRecord> {
    ST.with(|s| s.borrow().notifications.clone())
}

ic_cdk::export_candid!();
