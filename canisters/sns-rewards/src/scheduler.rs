use std::time::Duration;

use jupiter_ic_clients::sns::{ListSnsCanistersResponse, SnsRootCanister};
use jupiter_ic_clients::timer_guard::{LeaseFinish, TimerLeaseGuard};

use crate::clients::Neuron;
use crate::policy::{
    owner_for_neuron, SCAN_LEASE_SECONDS, SNS_NEURON_PAGE_SIZE, SNS_OWNER_SCAN_INTERVAL_SECONDS,
    SNS_OWNER_SCAN_MAX_PAGES,
};
use crate::state::{self, OwnerIndexSlot, OwnerScan, OwnerSnapshot};

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
    ic_cdk_timers::set_timer_interval(
        Duration::from_secs(SNS_OWNER_SCAN_INTERVAL_SECONDS),
        || async {
            scan_tick(false).await;
        },
    );
}

pub(crate) fn schedule_immediate_scan_check() {
    ic_cdk_timers::set_timer(Duration::ZERO, async {
        // Debug builds retain one-step control for deterministic staging and upgrade tests.
        scan_tick(cfg!(feature = "debug_api")).await;
    });
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
        return;
    };

    if state::with_state(|st| st.scan.is_none()) {
        let due = state::with_state(|st| {
            force
                || st.last_scan_started_at_timestamp_nanos == 0
                || now_nanos.saturating_sub(st.last_scan_started_at_timestamp_nanos)
                    >= SNS_OWNER_SCAN_INTERVAL_SECONDS * 1_000_000_000
        });
        if !due || !start_scan(now_nanos).await {
            return;
        }
    }
    process_one_page(now_nanos, !force).await;
}

async fn start_scan(now_nanos: u64) -> bool {
    let Some(root) = state::with_state(|st| st.config.reward_sns_root_canister_id) else {
        return false;
    };
    state::with_state_mut(|st| st.last_scan_started_at_timestamp_nanos = now_nanos);
    let resolved = match SnsRootCanister.list_sns_canisters(root).await {
        Ok(value) => value,
        Err(err) => {
            crate::logging::scan("failed", None, None, Some(&err.to_string()));
            return false;
        }
    };
    let (governance, ledger) = match resolve_components(root, &resolved) {
        Ok(components) => components,
        Err(reason) => {
            crate::logging::scan("failed", None, None, Some(reason));
            return false;
        }
    };
    let staging_slot = state::with_state(|st| {
        st.active_snapshot
            .as_ref()
            .map(|snapshot| snapshot.active_slot.other())
            .unwrap_or(OwnerIndexSlot::A)
    });
    state::clear_slot(staging_slot);
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
    state::with_state_mut(|st| st.scan = Some(scan.clone()));
    crate::logging::scan("started", Some(&scan), None, None);
    true
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

async fn process_one_page(now_nanos: u64, schedule_next_page: bool) {
    let Some(scan) = state::with_state(|st| st.scan.clone()) else {
        return;
    };
    if scan.pages_processed >= SNS_OWNER_SCAN_MAX_PAGES {
        fail_scan("page_cap_reached");
        return;
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
            return;
        }
    };
    let next_cursor = match validate_page(&scan, &response.neurons) {
        Ok(value) => value,
        Err(err) => {
            fail_scan(&err);
            return;
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
        } else if schedule_next_page {
            schedule_continuation();
        }
        return;
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
}
