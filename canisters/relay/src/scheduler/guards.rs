use crate::state;
use jupiter_ic_clients::timer_guard::{LeaseFinish, TimerLeaseGuard};

pub(super) const MAIN_TICK_LEASE_SECONDS: u64 = 30 * 60;

pub(super) struct MainGuard {
    inner: TimerLeaseGuard,
}

impl MainGuard {
    pub(super) fn acquire(now_secs: u64) -> Option<Self> {
        state::with_state_mut(|st| {
            let inner =
                TimerLeaseGuard::acquire(now_secs, MAIN_TICK_LEASE_SECONDS, st.main_lock_state_ts)?;
            let lease_expires_at_ts = inner.lease_expires_at_ts();
            st.main_lock_state_ts = Some(lease_expires_at_ts);
            Some(Self { inner })
        })
    }

    pub(super) fn finish(mut self, now_secs: u64) {
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            if self.inner.release(st.main_lock_state_ts) == LeaseFinish::Released {
                st.main_lock_state_ts = Some(0);
            }
        });
    }

    fn release(&mut self) {
        if !self.inner.is_active() {
            return;
        }
        state::with_state_mut(|st| {
            if self.inner.release(st.main_lock_state_ts) == LeaseFinish::Released {
                st.main_lock_state_ts = Some(0);
            }
        });
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        self.release();
    }
}
