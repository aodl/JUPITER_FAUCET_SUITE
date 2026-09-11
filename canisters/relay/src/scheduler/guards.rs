use crate::state;
use jupiter_ic_clients::timer_guard::{LeaseFinish, TimerLeaseGuard};

pub(super) const MAIN_TICK_LEASE_SECONDS: u64 = 30 * 60;
pub(super) const SPLITTER_LEASE_SECONDS: u64 = 30 * 60;

pub(super) struct MainGuard {
    inner: TimerLeaseGuard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MainLeaseToken(u64);

impl MainLeaseToken {
    pub(super) fn is_current(self) -> bool {
        state::with_state(|st| st.main_lock_state_ts == Some(self.0))
    }

    pub(super) fn is_current_in(self, st: &state::State) -> bool {
        st.main_lock_state_ts == Some(self.0)
    }

    #[cfg(test)]
    pub(super) fn capture_for_test() -> Option<Self> {
        state::with_state(|st| st.main_lock_state_ts.map(Self))
    }
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
            if self.inner.release(st.main_lock_state_ts) == LeaseFinish::Released {
                st.last_main_run_ts = now_secs;
                st.main_lock_state_ts = Some(0);
            }
        });
    }

    #[cfg(test)]
    pub(super) fn lease_expires_at_ts(&self) -> u64 {
        self.inner.lease_expires_at_ts()
    }

    pub(super) fn lease_token(&self) -> MainLeaseToken {
        MainLeaseToken(self.inner.lease_expires_at_ts())
    }

    pub(super) fn is_current(&self) -> bool {
        state::with_state(|st| {
            self.inner.is_active()
                && st.main_lock_state_ts == Some(self.inner.lease_expires_at_ts())
        })
    }

    pub(super) fn release_without_finishing(mut self) {
        self.release();
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

pub(super) struct SplitterGuard {
    inner: TimerLeaseGuard,
}

impl SplitterGuard {
    pub(super) fn acquire(now_secs: u64) -> Option<Self> {
        state::with_state_mut(|st| {
            let inner = TimerLeaseGuard::acquire(
                now_secs,
                SPLITTER_LEASE_SECONDS,
                st.splitter_lock_state_ts,
            )?;
            st.splitter_lock_state_ts = Some(inner.lease_expires_at_ts());
            Some(Self { inner })
        })
    }

    pub(super) fn is_current(&self) -> bool {
        state::with_state(|st| {
            self.inner.is_active()
                && st.splitter_lock_state_ts == Some(self.inner.lease_expires_at_ts())
        })
    }

    fn release(&mut self) {
        if !self.inner.is_active() {
            return;
        }
        state::with_state_mut(|st| {
            if self.inner.release(st.splitter_lock_state_ts) == LeaseFinish::Released {
                st.splitter_lock_state_ts = Some(0);
            }
        });
    }
}

impl Drop for SplitterGuard {
    fn drop(&mut self) {
        self.release();
    }
}
