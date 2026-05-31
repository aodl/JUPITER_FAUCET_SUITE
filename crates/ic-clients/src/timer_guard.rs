//! Small reusable lease guard for timer reentrancy protection.
//!
//! The guard only models lease acquisition/release semantics. State mutation
//! and logging remain owned by each canister.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseFinish {
    Released,
    Unchanged,
}

#[derive(Debug)]
pub struct TimerLeaseGuard {
    active: bool,
    lease_expires_at_ts: u64,
}

impl TimerLeaseGuard {
    pub fn acquire(
        now_secs: u64,
        lease_seconds: u64,
        current_lock_expires_at_ts: Option<u64>,
    ) -> Option<Self> {
        if current_lock_expires_at_ts.unwrap_or(0) > now_secs {
            return None;
        }
        Some(Self {
            active: true,
            lease_expires_at_ts: now_secs.saturating_add(lease_seconds),
        })
    }

    pub fn lease_expires_at_ts(&self) -> u64 {
        self.lease_expires_at_ts
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn release(&mut self, current_lock_expires_at_ts: Option<u64>) -> LeaseFinish {
        if !self.active {
            return LeaseFinish::Unchanged;
        }
        self.active = false;
        if current_lock_expires_at_ts == Some(self.lease_expires_at_ts) {
            LeaseFinish::Released
        } else {
            LeaseFinish::Unchanged
        }
    }

    pub fn finish(mut self, current_lock_expires_at_ts: Option<u64>) -> LeaseFinish {
        self.release(current_lock_expires_at_ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_rejects_unexpired_lock() {
        assert!(TimerLeaseGuard::acquire(10, 30, Some(11)).is_none());
    }

    #[test]
    fn acquire_sets_saturating_lease() {
        let guard = TimerLeaseGuard::acquire(u64::MAX - 5, 30, Some(0)).unwrap();
        assert_eq!(guard.lease_expires_at_ts(), u64::MAX);
    }

    #[test]
    fn release_only_releases_matching_lease() {
        let mut guard = TimerLeaseGuard::acquire(10, 30, Some(0)).unwrap();
        assert_eq!(guard.release(Some(40)), LeaseFinish::Released);
        assert_eq!(guard.release(Some(40)), LeaseFinish::Unchanged);

        let mut guard = TimerLeaseGuard::acquire(10, 30, Some(0)).unwrap();
        assert_eq!(guard.release(Some(41)), LeaseFinish::Unchanged);
    }
}
