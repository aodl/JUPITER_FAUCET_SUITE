use std::{cell::RefCell, time::Duration};

use jupiter_ic_clients::sns::{ListSnsCanistersResponse, SnsRootCanister};
use jupiter_ic_clients::timer_guard::{LeaseFinish, TimerLeaseGuard};

use crate::clients::Neuron;
use crate::policy::{
    owner_for_neuron, SCAN_LEASE_SECONDS, SNS_NEURON_PAGE_SIZE, SNS_OWNER_SCAN_INTERVAL_SECONDS,
    SNS_OWNER_SCAN_MAX_PAGES,
};
use crate::state::{self, OwnerIndexSlot, OwnerScan, OwnerSnapshot};

const SCAN_START_RETRY_SECONDS: u64 = 60;
const SCAN_BUSY_RECHECK_SECONDS: u64 = 60;
const SCAN_PAGE_FAILURE_RECHECK_SECONDS: u64 = 5 * 60;
pub(crate) const CONFIG_LOG_INTERVAL_SECONDS: u64 = 24 * 60 * 60;

thread_local! {
    static NEXT_SCAN_TIMER: RefCell<Option<ic_cdk_timers::TimerId>> = const { RefCell::new(None) };
}

struct ScanGuard {
    inner: TimerLeaseGuard,
}

impl ScanGuard {
    fn acquire(now_secs: u64) -> Option<Self> {
        state::with_state_mut(|st| {
            let inner =
                TimerLeaseGuard::acquire(now_secs, SCAN_LEASE_SECONDS, st.scan_lock_state_ts)?;
            st.scan_lock_state_ts = Some(inner.lease_expires_at_ts());
            Some(Self { inner })
        })
    }
}

impl Drop for ScanGuard {
    fn drop(&mut self) {
        if !self.inner.is_active() {
            return;
        }
        state::with_state_mut(|st| {
            if self.inner.release(st.scan_lock_state_ts) == LeaseFinish::Released {
                st.scan_lock_state_ts = Some(0);
            }
        });
    }
}

pub(crate) fn install_timers() {
    install_config_log_timer();
    schedule_scan_check(Duration::ZERO, cfg!(feature = "debug_api"));
}

// Configuration observability is intentionally independent of the durable,
// accepted-start-derived owner-scan scheduler and never invokes scan work.
fn install_config_log_timer() {
    ic_cdk_timers::set_timer_interval(Duration::from_secs(CONFIG_LOG_INTERVAL_SECONDS), || async {
        crate::logging::config();
    });
}

fn schedule_scan_check(delay: Duration, force: bool) {
    let timer_id = ic_cdk_timers::set_timer(delay, async move {
        NEXT_SCAN_TIMER.with_borrow_mut(|timer| {
            timer.take();
        });
        scan_tick(force).await;
    });
    NEXT_SCAN_TIMER.with_borrow_mut(|timer| {
        if let Some(previous_timer_id) = timer.replace(timer_id) {
            ic_cdk_timers::clear_timer(previous_timer_id);
        }
    });
}

fn next_due_delay(last_started_at_nanos: u64, now_nanos: u64) -> Duration {
    if last_started_at_nanos == 0 {
        return Duration::ZERO;
    }
    let due_at = last_started_at_nanos
        .saturating_add(SNS_OWNER_SCAN_INTERVAL_SECONDS.saturating_mul(1_000_000_000));
    Duration::from_nanos(due_at.saturating_sub(now_nanos))
}

fn schedule_next_due_check(now_nanos: u64) {
    let last_started_at_nanos = state::with_state(|st| st.last_scan_started_at_timestamp_nanos);
    schedule_scan_check(next_due_delay(last_started_at_nanos, now_nanos), false);
}

fn schedule_start_retry() {
    schedule_scan_check(Duration::from_secs(SCAN_START_RETRY_SECONDS), false);
}

fn schedule_continuation() {
    ic_cdk_timers::set_timer(Duration::ZERO, async {
        scan_tick(false).await;
    });
}

pub(crate) async fn scan_tick(force: bool) {
    let now_nanos = ic_cdk::api::time();
    let now_secs = now_nanos / 1_000_000_000;
    let Some(_guard) = ScanGuard::acquire(now_secs) else {
        if !force {
            schedule_scan_check(Duration::from_secs(SCAN_BUSY_RECHECK_SECONDS), false);
        }
        return;
    };

    if state::with_state(|st| st.scan.is_none()) {
        let due = state::with_state(|st| {
            force
                || st.last_scan_started_at_timestamp_nanos == 0
                || now_nanos.saturating_sub(st.last_scan_started_at_timestamp_nanos)
                    >= SNS_OWNER_SCAN_INTERVAL_SECONDS * 1_000_000_000
        });
        if !due {
            schedule_next_due_check(now_nanos);
            return;
        }
        match start_scan().await {
            StartScanResult::Started(started_at_nanos) => {
                schedule_next_due_check(started_at_nanos);
            }
            StartScanResult::NotConfigured => return,
            StartScanResult::Failed => {
                schedule_start_retry();
                return;
            }
        }
    }
    match process_one_page(ic_cdk::api::time(), !force).await {
        PageResult::InProgress => {}
        PageResult::Completed => schedule_next_due_check(ic_cdk::api::time()),
        PageResult::Failed => schedule_scan_check(
            Duration::from_secs(SCAN_PAGE_FAILURE_RECHECK_SECONDS),
            false,
        ),
    }
}

enum StartScanResult {
    Started(u64),
    NotConfigured,
    Failed,
}

async fn start_scan() -> StartScanResult {
    let Some(root) = state::with_state(|st| st.config.reward_sns_root_canister_id) else {
        return StartScanResult::NotConfigured;
    };
    let resolved = match SnsRootCanister.list_sns_canisters(root).await {
        Ok(value) => value,
        Err(err) => {
            crate::logging::scan("failed", None, None, Some(&err.to_string()));
            return StartScanResult::Failed;
        }
    };
    let (governance, ledger) = match resolve_components(root, &resolved) {
        Ok(components) => components,
        Err(reason) => {
            crate::logging::scan("failed", None, None, Some(reason));
            return StartScanResult::Failed;
        }
    };
    let staging_slot = state::with_state(|st| {
        st.active_snapshot
            .as_ref()
            .map(|snapshot| snapshot.active_slot.other())
            .unwrap_or(OwnerIndexSlot::A)
    });
    state::clear_slot(staging_slot);
    let now_nanos = ic_cdk::api::time();
    let scan = OwnerScan {
        staging_slot,
        sns_root_canister_id: root,
        sns_governance_canister_id: governance,
        sns_ledger_canister_id: ledger,
        scan_started_at_timestamp_nanos: now_nanos,
        start_page_at: None,
        pages_processed: 0,
        neurons_processed: 0,
    };
    state::with_state_mut(|st| {
        st.last_scan_started_at_timestamp_nanos = now_nanos;
        st.scan = Some(scan.clone());
    });
    crate::logging::scan("started", Some(&scan), None, None);
    StartScanResult::Started(now_nanos)
}

fn resolve_components(
    configured_root: candid::Principal,
    resolved: &ListSnsCanistersResponse,
) -> Result<(candid::Principal, candid::Principal), &'static str> {
    if resolved.root != Some(configured_root) {
        return Err("resolved_root_mismatch");
    }
    let governance = resolved.governance.ok_or("missing_governance")?;
    let ledger = resolved.ledger.ok_or("missing_ledger")?;
    Ok((governance, ledger))
}

fn validate_page(scan: &OwnerScan, neurons: &[Neuron]) -> Result<Option<Vec<u8>>, String> {
    let mut previous = scan.start_page_at.as_deref();
    for neuron in neurons {
        let id = neuron
            .id
            .as_ref()
            .ok_or_else(|| "missing_neuron_id".to_string())?;
        if id.id.len() != 32 {
            return Err("malformed_neuron_id".to_string());
        }
        if previous.is_some_and(|cursor| id.id.as_slice() <= cursor) {
            return Err("non_progressing_cursor".to_string());
        }
        previous = Some(id.id.as_slice());
    }
    Ok(neurons
        .last()
        .and_then(|neuron| neuron.id.as_ref())
        .map(|id| id.id.clone()))
}

fn fail_scan(reason: &str) {
    let scan = state::with_state(|st| st.scan.clone());
    crate::logging::scan("failed", scan.as_ref(), None, Some(reason));
    state::with_state_mut(|st| st.scan = None);
}

enum PageResult {
    InProgress,
    Completed,
    Failed,
}

async fn process_one_page(now_nanos: u64, schedule_next_page: bool) -> PageResult {
    let Some(scan) = state::with_state(|st| st.scan.clone()) else {
        return PageResult::Failed;
    };
    if scan.pages_processed >= SNS_OWNER_SCAN_MAX_PAGES {
        fail_scan("page_cap_reached");
        return PageResult::Failed;
    }
    let response = match crate::clients::list_neurons(
        scan.sns_governance_canister_id,
        scan.start_page_at.clone(),
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            fail_scan(&err);
            return PageResult::Failed;
        }
    };
    let next_cursor = match validate_page(&scan, &response.neurons) {
        Ok(value) => value,
        Err(err) => {
            fail_scan(&err);
            return PageResult::Failed;
        }
    };
    for neuron in &response.neurons {
        if let Some(owner) = owner_for_neuron(neuron) {
            state::insert_owner(scan.staging_slot, owner);
        }
    }
    let is_final = response.neurons.len() < SNS_NEURON_PAGE_SIZE as usize;
    let updated = state::with_state_mut(|st| {
        let current = st.scan.as_mut().expect("active owner scan");
        current.pages_processed += 1;
        current.neurons_processed = current
            .neurons_processed
            .saturating_add(response.neurons.len() as u64);
        current.start_page_at = next_cursor;
        current.clone()
    });
    crate::logging::scan("page", Some(&updated), None, None);
    if !is_final {
        if updated.pages_processed >= SNS_OWNER_SCAN_MAX_PAGES {
            fail_scan("page_cap_reached");
            return PageResult::Failed;
        } else if schedule_next_page {
            schedule_continuation();
        }
        return PageResult::InProgress;
    }
    let snapshot = state::with_state_mut(|st| {
        let completed = st.scan.take().expect("completed owner scan");
        let snapshot = OwnerSnapshot {
            snapshot_id: st.next_snapshot_id,
            active_slot: completed.staging_slot,
            sns_root_canister_id: completed.sns_root_canister_id,
            sns_governance_canister_id: completed.sns_governance_canister_id,
            sns_ledger_canister_id: completed.sns_ledger_canister_id,
            scan_started_at_timestamp_nanos: completed.scan_started_at_timestamp_nanos,
            scan_completed_at_timestamp_nanos: now_nanos,
            neuron_count: completed.neurons_processed,
            owner_count: state::slot_len(completed.staging_slot),
        };
        st.active_snapshot = Some(snapshot.clone());
        st.next_snapshot_id = st.next_snapshot_id.saturating_add(1);
        snapshot
    });
    crate::logging::scan("completed", None, Some(&snapshot), None);
    PageResult::Completed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::{NeuronId, NeuronPermission};
    use candid::Principal;

    fn neuron(id: u8) -> Neuron {
        Neuron {
            id: Some(NeuronId { id: vec![id; 32] }),
            permissions: vec![NeuronPermission {
                principal: Some(Principal::from_slice(&[id])),
                permission_type: vec![1],
            }],
            cached_neuron_stake_e8s: 1,
            neuron_fees_e8s: 0,
        }
    }

    #[test]
    fn pagination_is_exclusive_and_strictly_progressing() {
        let scan = OwnerScan {
            staging_slot: OwnerIndexSlot::A,
            sns_root_canister_id: Principal::anonymous(),
            sns_governance_canister_id: Principal::anonymous(),
            sns_ledger_canister_id: Principal::anonymous(),
            scan_started_at_timestamp_nanos: 0,
            start_page_at: Some(vec![1; 32]),
            pages_processed: 0,
            neurons_processed: 0,
        };
        assert_eq!(
            validate_page(&scan, &[neuron(2)]).unwrap(),
            Some(vec![2; 32])
        );
        assert_eq!(
            validate_page(&scan, &[neuron(1)]).unwrap_err(),
            "non_progressing_cursor"
        );
        assert_eq!(
            validate_page(&scan, &[neuron(0)]).unwrap_err(),
            "non_progressing_cursor"
        );
    }

    #[test]
    fn malformed_or_missing_ids_fail() {
        let scan = OwnerScan {
            staging_slot: OwnerIndexSlot::A,
            sns_root_canister_id: Principal::anonymous(),
            sns_governance_canister_id: Principal::anonymous(),
            sns_ledger_canister_id: Principal::anonymous(),
            scan_started_at_timestamp_nanos: 0,
            start_page_at: None,
            pages_processed: 0,
            neurons_processed: 0,
        };
        let mut malformed = neuron(2);
        malformed.id = Some(NeuronId { id: vec![2] });
        assert_eq!(
            validate_page(&scan, &[malformed]).unwrap_err(),
            "malformed_neuron_id"
        );
        let mut missing = neuron(2);
        missing.id = None;
        assert_eq!(
            validate_page(&scan, &[missing]).unwrap_err(),
            "missing_neuron_id"
        );
    }

    #[test]
    fn root_resolution_requires_matching_root_governance_and_ledger() {
        let root = Principal::from_slice(&[1]);
        let governance = Principal::from_slice(&[2]);
        let ledger = Principal::from_slice(&[3]);
        let response = |resolved_root, governance, ledger| ListSnsCanistersResponse {
            root: resolved_root,
            governance,
            ledger,
            swap: None,
            index: None,
            dapps: vec![],
            archives: vec![],
            extensions: None,
        };
        assert_eq!(
            resolve_components(root, &response(Some(root), Some(governance), Some(ledger))),
            Ok((governance, ledger))
        );
        assert_eq!(
            resolve_components(
                root,
                &response(Some(ledger), Some(governance), Some(ledger))
            ),
            Err("resolved_root_mismatch")
        );
        assert_eq!(
            resolve_components(root, &response(Some(root), None, Some(ledger))),
            Err("missing_governance")
        );
        assert_eq!(
            resolve_components(root, &response(Some(root), Some(governance), None)),
            Err("missing_ledger")
        );
    }

    #[test]
    fn next_due_delay_is_derived_from_the_accepted_scan_start() {
        let accepted_at = 123_456_789_000_u64;
        let almost_due = accepted_at + SNS_OWNER_SCAN_INTERVAL_SECONDS * 1_000_000_000 - 17;
        assert_eq!(
            next_due_delay(accepted_at, almost_due),
            Duration::from_nanos(17)
        );
        assert_eq!(
            next_due_delay(
                accepted_at,
                accepted_at + SNS_OWNER_SCAN_INTERVAL_SECONDS * 1_000_000_000
            ),
            Duration::ZERO
        );
        assert_eq!(next_due_delay(0, accepted_at), Duration::ZERO);
    }
}
