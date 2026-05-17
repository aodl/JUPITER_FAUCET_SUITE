#[cfg(feature = "debug_api")]
use std::cell::RefCell;

#[cfg(feature = "debug_api")]
thread_local! {
    // Debug-only fault injection used by PocketIC E2E tests.
    // These are intentionally *not* persisted in stable memory.
    static DEBUG_PAUSE_AFTER_PLANNING: RefCell<bool> = RefCell::new(false);
    static DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS: RefCell<Option<u32>> = RefCell::new(None);
    static DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK: RefCell<u32> = RefCell::new(0);

    // Simulates "canister too low on cycles" without depending on PocketIC cycle accounting.
    // When enabled, main tick will refuse to perform any external calls.
    static DEBUG_SIMULATE_LOW_CYCLES: RefCell<bool> = RefCell::new(false);

    // Allows payout-only cycles in PocketIC without constantly initiating new maturity disbursements.
    // Useful for state-size regression tests.
    static DEBUG_SKIP_MATURITY_INITIATION: RefCell<bool> = RefCell::new(false);
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_pause_after_planning(enabled: bool) {
    DEBUG_PAUSE_AFTER_PLANNING.with(|v| *v.borrow_mut() = enabled);
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_real_trap_after_successful_transfers(n: Option<u32>) {
    DEBUG_REAL_TRAP_AFTER_SUCCESSFUL_TRANSFERS.with(|v| *v.borrow_mut() = n);
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_simulate_low_cycles(enabled: bool) {
    DEBUG_SIMULATE_LOW_CYCLES.with(|v| *v.borrow_mut() = enabled);
}

#[cfg(feature = "debug_api")]
pub(crate) fn debug_set_skip_maturity_initiation(enabled: bool) {
    DEBUG_SKIP_MATURITY_INITIATION.with(|v| *v.borrow_mut() = enabled);
}

#[cfg(feature = "debug_api")]
pub(super) fn debug_pause_after_planning() -> bool {
    DEBUG_PAUSE_AFTER_PLANNING.with(|v| *v.borrow())
}

#[cfg(feature = "debug_api")]
pub(super) fn debug_simulate_low_cycles() -> bool {
    DEBUG_SIMULATE_LOW_CYCLES.with(|v| *v.borrow())
}

#[cfg(feature = "debug_api")]
pub(super) fn debug_skip_maturity_initiation() -> bool {
    DEBUG_SKIP_MATURITY_INITIATION.with(|v| *v.borrow())
}

#[cfg(feature = "debug_api")]
pub(super) fn debug_reset_successful_transfer_counter() {
    DEBUG_SUCCESSFUL_TRANSFERS_THIS_TICK.with(|v| *v.borrow_mut() = 0);
}

#[cfg(not(feature = "debug_api"))]
pub(super) fn debug_reset_successful_transfer_counter() {}

#[cfg(feature = "debug_api")]
pub(super) enum DebugSuccessfulTransferInjection {
    None,
    Abort,
    Trap,
}

#[cfg(feature = "debug_api")]
pub(super) fn debug_successful_transfer_injection() -> DebugSuccessfulTransferInjection {
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

    // The abort path intentionally leaves the persisted plan untouched so the next run can observe
    // Duplicate and complete deterministically. The trap path is stricter and is used by PocketIC
    // tests to exercise the actual post-await rollback boundary.
    if trap_after_n == Some(count) {
        return DebugSuccessfulTransferInjection::Trap;
    }
    if abort_after_n == Some(count) {
        return DebugSuccessfulTransferInjection::Abort;
    }
    DebugSuccessfulTransferInjection::None
}
