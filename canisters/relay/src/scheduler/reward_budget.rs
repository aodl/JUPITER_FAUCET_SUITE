//! One heap-only allowance shared by every part of a reward attempt. Exhaustion
//! holds work; it never turns a partial history into authorized attribution.
use std::future::Future;
use std::sync::{Arc, Mutex};

use super::guards::MainLeaseToken;

pub(super) const MAX_CALLS: usize = 64;
pub(super) const MAX_RECORDS: usize = 16_384;
pub(super) const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_WORK: usize = 2_000_000;
const ATTEMPT_SECONDS: u64 = 120;
pub(super) const EXHAUSTED: &str = "reward_work_budget_exhausted";

#[cfg(feature = "debug_api")]
thread_local! { static TRAP_AFTER_CALL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[cfg(feature = "debug_api")]
pub(crate) fn debug_trap_after_reward_call(value: bool) {
    TRAP_AFTER_CALL.with(|flag| flag.set(value));
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Usage {
    pub calls: usize,
    pub records: usize,
    pub bytes: usize,
    pub work: usize,
}

#[derive(Clone, Debug)]
pub(super) struct WorkBudget {
    usage: Arc<Mutex<Usage>>,
    lease: Option<MainLeaseToken>,
    deadline_nanos: u64,
}

impl WorkBudget {
    pub(super) fn new(now_nanos: u64, lease: MainLeaseToken) -> Self {
        Self {
            usage: Arc::default(),
            lease: Some(lease),
            deadline_nanos: now_nanos.saturating_add(ATTEMPT_SECONDS * 1_000_000_000),
        }
    }

    #[cfg(test)]
    pub(super) fn for_test() -> Self {
        Self {
            usage: Arc::default(),
            lease: None,
            deadline_nanos: u64::MAX,
        }
    }

    pub(super) fn usage(&self) -> Usage {
        *self.usage.lock().expect("reward work budget")
    }

    pub(super) fn check(&self) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        let now = ic_cdk::api::time();
        #[cfg(not(target_arch = "wasm32"))]
        let now = 0;
        self.check_at(now)
    }

    fn check_at(&self, now: u64) -> Result<(), String> {
        if now >= self.deadline_nanos || self.lease.is_some_and(|lease| !lease.is_current()) {
            Err("reward_attempt_expired_or_stale".into())
        } else {
            Ok(())
        }
    }

    fn charge(
        &self,
        calls: usize,
        records: usize,
        bytes: usize,
        work: usize,
    ) -> Result<(), String> {
        self.check()?;
        let mut used = self.usage.lock().expect("reward work budget");
        let next = Usage {
            calls: used.calls.saturating_add(calls),
            records: used.records.saturating_add(records),
            bytes: used.bytes.saturating_add(bytes),
            work: used.work.saturating_add(work),
        };
        if next.calls > MAX_CALLS
            || next.records > MAX_RECORDS
            || next.bytes > MAX_HISTORY_BYTES
            || next.work > MAX_WORK
        {
            return Err(EXHAUSTED.into());
        }
        *used = next;
        Ok(())
    }

    pub(super) fn retain(&self, records: usize, bytes: usize) -> Result<(), String> {
        self.charge(0, records, bytes, records)
    }

    pub(super) fn work(&self, units: usize) -> Result<(), String> {
        self.charge(0, 0, 0, units)
    }

    pub(super) fn require_call_capacity(&self) -> Result<(), String> {
        self.check()?;
        if self.usage().calls >= MAX_CALLS {
            Err(EXHAUSTED.into())
        } else {
            Ok(())
        }
    }

    pub(super) async fn call<T>(&self, future: impl Future<Output = T>) -> Result<T, String> {
        // Charge attempted calls, including failures. Checking afterwards fences
        // a late reply, but cannot cancel work already dispatched to a callee.
        self.charge(1, 0, 0, 0)?;
        let result = future.await;
        #[cfg(feature = "debug_api")]
        if TRAP_AFTER_CALL.with(|flag| flag.get()) {
            ic_cdk::trap("debug reward callback interruption");
        }
        self.check()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn shared_allowance_checks_before_call_and_retention_at_exact_limits() {
        let budget = WorkBudget::for_test();
        let nested = budget.clone();
        let calls = AtomicUsize::new(0);
        for _ in 0..MAX_CALLS {
            block_on(nested.call(async {
                calls.fetch_add(1, Ordering::SeqCst);
            }))
            .unwrap();
        }
        assert_eq!(
            block_on(budget.call(async {
                calls.fetch_add(1, Ordering::SeqCst);
            }))
            .unwrap_err(),
            EXHAUSTED
        );
        assert_eq!(calls.load(Ordering::SeqCst), MAX_CALLS);
        assert_eq!(budget.require_call_capacity().unwrap_err(), EXHAUSTED);
        nested.retain(MAX_RECORDS, MAX_HISTORY_BYTES).unwrap();
        assert_eq!(budget.retain(1, 0).unwrap_err(), EXHAUSTED);
        assert_eq!(budget.retain(0, 1).unwrap_err(), EXHAUSTED);
        budget.work(MAX_WORK - MAX_RECORDS).unwrap();
        assert_eq!(nested.work(1).unwrap_err(), EXHAUSTED);
        assert_eq!(
            budget.usage(),
            Usage {
                calls: MAX_CALLS,
                records: MAX_RECORDS,
                bytes: MAX_HISTORY_BYTES,
                work: MAX_WORK
            }
        );
    }

    #[test]
    fn deadline_is_exclusive_and_does_not_refresh_for_nested_work() {
        let mut budget = WorkBudget::for_test();
        budget.deadline_nanos = 100;
        assert!(budget.clone().check_at(99).is_ok());
        assert!(budget.check_at(100).is_err());
    }
}
