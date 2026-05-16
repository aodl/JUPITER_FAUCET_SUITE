#[cfg(feature = "debug_api")]
use std::cell::RefCell;

#[cfg(feature = "debug_api")]
thread_local! {
    static DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK: RefCell<u32> = RefCell::new(0);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
pub fn debug_set_real_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
fn debug_reset_successful_transfer_counter() {
    DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|v| *v.borrow_mut() = 0);
}

#[cfg(feature = "debug_api")]
enum DebugSuccessfulTransferInjection {
    None,
    Abort,
    Trap,
}

#[cfg(feature = "debug_api")]
fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
    let abort_after_n = DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow());
    let trap_after_n = DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow());
    if abort_after_n.is_none() && trap_after_n.is_none() {
        return DebugSuccessfulTransferInjection::None;
    }

    let count = DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|c| {
        let mut c = c.borrow_mut();
        *c = c.saturating_add(1);
        *c
    });

    if trap_after_n == Some(count) {
        return DebugSuccessfulTransferInjection::Trap;
    }
    if abort_after_n == Some(count) {
        return DebugSuccessfulTransferInjection::Abort;
    }
    DebugSuccessfulTransferInjection::None
}

#[cfg(not(feature = "debug_api"))]
fn debug_reset_successful_transfer_counter() {}

#[cfg(not(feature = "debug_api"))]
enum DebugSuccessfulTransferInjection {
    None,
}

#[cfg(not(feature = "debug_api"))]
fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
    DebugSuccessfulTransferInjection::None
}

