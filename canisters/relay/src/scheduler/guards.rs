use crate::state;

pub(super) const MAIN_TICK_LEASE_SECONDS: u64 = 30 * 60;

pub(super) struct MainGuard {
    active: bool,
    lease_expires_at_ts: u64,
}

impl MainGuard {
    pub(super) fn acquire(now_secs: u64) -> Option<Self> {
        state::with_state_mut(|st| {
            let lock_expires_at_ts = st.main_lock_state_ts.unwrap_or(0);
            if lock_expires_at_ts > now_secs {
                return None;
            }
            let lease_expires_at_ts = now_secs.saturating_add(MAIN_TICK_LEASE_SECONDS);
            st.main_lock_state_ts = Some(lease_expires_at_ts);
            Some(Self {
                active: true,
                lease_expires_at_ts,
            })
        })
    }

    pub(super) fn finish(mut self, now_secs: u64) {
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_state_mut(|st| {
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        self.release();
    }
}
