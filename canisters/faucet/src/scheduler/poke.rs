use super::*;
use std::cell::Cell;

pub(super) const EVENT_HORIZON_POKE_INTERVAL_SECONDS: u64 = 10;
pub(super) const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct PokeRuntimeState {
    due_at_ns: Option<u64>,
    timer_pending: bool,
    next_attempt_allowed_at_ns: u64,
}

thread_local! {
    static POKE_RUNTIME: Cell<PokeRuntimeState> = const { Cell::new(PokeRuntimeState {
        due_at_ns: None, timer_pending: false, next_attempt_allowed_at_ns: 0,
    }) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferredTimerAction {
    Stop,
    Reschedule(Duration),
    Attempt,
}

fn record_hint(now_ns: u64) -> bool {
    POKE_RUNTIME.with(|cell| {
        let mut runtime = cell.get();
        runtime.due_at_ns =
            Some(now_ns.saturating_add(
                EVENT_HORIZON_POKE_INTERVAL_SECONDS.saturating_mul(NANOS_PER_SECOND),
            ));
        let should_schedule = !runtime.timer_pending;
        runtime.timer_pending = true;
        cell.set(runtime);
        should_schedule
    })
}

fn admit_attempt(now_ns: u64) -> bool {
    POKE_RUNTIME.with(|cell| {
        let mut runtime = cell.get();
        if runtime.next_attempt_allowed_at_ns > now_ns {
            return false;
        }
        runtime.next_attempt_allowed_at_ns = now_ns
            .saturating_add(EVENT_HORIZON_POKE_INTERVAL_SECONDS.saturating_mul(NANOS_PER_SECOND));
        cell.set(runtime);
        true
    })
}

fn deferred_timer_action(now_ns: u64) -> DeferredTimerAction {
    POKE_RUNTIME.with(|cell| {
        let mut runtime = cell.get();
        let action = match runtime.due_at_ns {
            Some(due_at_ns) if due_at_ns > now_ns => {
                DeferredTimerAction::Reschedule(Duration::from_nanos(due_at_ns - now_ns))
            }
            Some(_) => {
                runtime.due_at_ns = None;
                runtime.timer_pending = false;
                DeferredTimerAction::Attempt
            }
            None => {
                runtime.timer_pending = false;
                DeferredTimerAction::Stop
            }
        };
        cell.set(runtime);
        action
    })
}

async fn run_worker() {
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / NANOS_PER_SECOND;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let index = IcpIndexCanister::new(cfg.index_canister_id);
    let cmc = CyclesMintingCanister::new(cfg.cmc_canister_id);
    let governance = NnsGovernanceCanister::new(
        cfg.governance_canister_id
            .expect("governance_canister_id configured"),
    );
    let status_client = ManagementCanisterInfoClient;
    run_worker_with_clients(
        now_nanos,
        now_secs,
        &ledger,
        &index,
        &cmc,
        &governance,
        &status_client,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_worker_with_clients<
    L: LedgerClient,
    I: IndexClient,
    C: CmcClient,
    G: GovernanceClient,
    S: CanisterStatusClient,
>(
    now_nanos: u64,
    now_secs: u64,
    ledger: &L,
    index: &I,
    cmc: &C,
    governance: &G,
    status_client: &S,
) {
    let Some(guard) = MainGuard::acquire(now_secs) else {
        return;
    };
    let lease = guard.lease_token();
    debug_reset_successful_transfer_counter();
    let _ = process_payout_with_lease(
        ledger,
        index,
        cmc,
        governance,
        status_client,
        now_nanos,
        now_secs,
        lease,
    )
    .await;
    drop(guard);
}

fn schedule_deferred(delay: Duration) {
    ic_cdk_timers::set_timer(delay, async {
        match deferred_timer_action(ic_cdk::api::time()) {
            DeferredTimerAction::Stop => {}
            DeferredTimerAction::Reschedule(remaining) => schedule_deferred(remaining),
            DeferredTimerAction::Attempt => {
                if admit_attempt(ic_cdk::api::time()) {
                    run_worker().await;
                }
            }
        }
    });
}

pub(crate) async fn handle_event_horizon_poke() {
    let now_ns = ic_cdk::api::time();
    if record_hint(now_ns) {
        schedule_deferred(Duration::from_secs(EVENT_HORIZON_POKE_INTERVAL_SECONDS));
    }
    if admit_attempt(now_ns) {
        run_worker().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        POKE_RUNTIME.with(|cell| cell.set(PokeRuntimeState::default()));
    }

    #[test]
    fn trailing_edge_moves_without_a_second_timer_and_clears_before_work() {
        reset();
        assert!(record_hint(0));
        assert!(!record_hint(9_900_000_000));
        assert_eq!(
            deferred_timer_action(10_000_000_000),
            DeferredTimerAction::Reschedule(Duration::from_millis(9_900))
        );
        assert_eq!(
            deferred_timer_action(19_900_000_000),
            DeferredTimerAction::Attempt
        );
        assert!(record_hint(19_900_000_000));
    }

    #[test]
    fn admission_bounds_attempts_and_does_not_create_retries() {
        reset();
        assert!(admit_attempt(1));
        assert!(!admit_attempt(9_999_999_999));
        assert!(admit_attempt(10_000_000_001));
        assert_eq!(
            deferred_timer_action(10_000_000_001),
            DeferredTimerAction::Stop
        );
    }
}
