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

/// Lease guard with an ownership generation suitable for fencing async callbacks.
///
/// Matching only an expiry timestamp protects release, but it does not prevent an
/// expired owner from resuming and writing after a successor has acquired the
/// same logical operation. Callers must check `owns` after every await and before
/// every state mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FencedTimerLeaseGuard {
    active: bool,
    lease_expires_at_ts: u64,
    generation: u64,
}

impl FencedTimerLeaseGuard {
    pub fn acquire(
        now_secs: u64,
        lease_seconds: u64,
        current_lock_expires_at_ts: Option<u64>,
        current_generation: u64,
        preempt_unexpired: bool,
    ) -> Option<Self> {
        if !preempt_unexpired && current_lock_expires_at_ts.unwrap_or(0) > now_secs {
            return None;
        }
        Some(Self {
            active: true,
            lease_expires_at_ts: now_secs.saturating_add(lease_seconds),
            generation: current_generation.checked_add(1)?,
        })
    }

    pub fn lease_expires_at_ts(&self) -> u64 {
        self.lease_expires_at_ts
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn owns(&self, current_lock_expires_at_ts: Option<u64>, current_generation: u64) -> bool {
        self.active
            && current_lock_expires_at_ts == Some(self.lease_expires_at_ts)
            && current_generation == self.generation
    }

    pub fn release(
        &mut self,
        current_lock_expires_at_ts: Option<u64>,
        current_generation: u64,
    ) -> LeaseFinish {
        if !self.active {
            return LeaseFinish::Unchanged;
        }
        let owns = self.owns(current_lock_expires_at_ts, current_generation);
        self.active = false;
        if owns {
            LeaseFinish::Released
        } else {
            LeaseFinish::Unchanged
        }
    }
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

    #[test]
    fn fenced_lease_rejects_late_owner_after_expiry_and_reacquisition() {
        let mut old = FencedTimerLeaseGuard::acquire(10, 30, Some(0), 7, false).unwrap();
        let newer = FencedTimerLeaseGuard::acquire(
            40,
            30,
            Some(old.lease_expires_at_ts()),
            old.generation(),
            false,
        )
        .unwrap();

        assert!(!old.owns(Some(newer.lease_expires_at_ts()), newer.generation()));
        assert_eq!(
            old.release(Some(newer.lease_expires_at_ts()), newer.generation()),
            LeaseFinish::Unchanged
        );
        assert!(newer.owns(Some(newer.lease_expires_at_ts()), newer.generation()));
    }

    #[test]
    fn fenced_lease_can_be_preempted_by_a_priority_owner() {
        let public = FencedTimerLeaseGuard::acquire(10, 60, Some(0), 1, false).unwrap();
        assert!(FencedTimerLeaseGuard::acquire(
            11,
            60,
            Some(public.lease_expires_at_ts()),
            public.generation(),
            false,
        )
        .is_none());
        let timer = FencedTimerLeaseGuard::acquire(
            11,
            60,
            Some(public.lease_expires_at_ts()),
            public.generation(),
            true,
        )
        .unwrap();
        assert!(!public.owns(Some(timer.lease_expires_at_ts()), timer.generation()));
    }
}
