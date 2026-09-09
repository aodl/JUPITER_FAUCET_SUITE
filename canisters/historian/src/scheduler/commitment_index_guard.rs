use super::*;
use jupiter_ic_clients::timer_guard::FencedTimerLeaseGuard;

pub(super) const COMMITMENT_INDEX_LEASE_SECONDS: u64 = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CommitmentIndexLeaseToken {
    expires_at_ts: u64,
    generation: u64,
}

impl CommitmentIndexLeaseToken {
    pub(super) fn is_current(self) -> bool {
        state::with_state(|st| {
            st.commitment_index_lock_expires_at_ts == Some(self.expires_at_ts)
                && st.commitment_index_lock_generation == self.generation
        })
    }

    pub(super) fn renew(self, now_secs: u64) -> Result<Self, String> {
        state::with_root_state_mut(|st| {
            if st.commitment_index_lock_expires_at_ts != Some(self.expires_at_ts)
                || st.commitment_index_lock_generation != self.generation
            {
                return Err("commitment index lease was superseded".to_string());
            }
            let renewed = Self {
                expires_at_ts: now_secs.saturating_add(COMMITMENT_INDEX_LEASE_SECONDS),
                generation: self.generation,
            };
            st.commitment_index_lock_expires_at_ts = Some(renewed.expires_at_ts);
            Ok(renewed)
        })
    }
}

pub(super) struct CommitmentIndexGuard {
    inner: FencedTimerLeaseGuard,
}

impl CommitmentIndexGuard {
    pub(super) fn acquire(now_secs: u64, owner: state::CommitmentIndexLeaseOwner) -> Option<Self> {
        state::with_root_state_mut(|st| {
            let timer_preempts_endowment_refresh = owner
                == state::CommitmentIndexLeaseOwner::Scheduled
                && st.commitment_index_lock_owner
                    == Some(state::CommitmentIndexLeaseOwner::EndowmentRefresh);
            let inner = FencedTimerLeaseGuard::acquire(
                now_secs,
                COMMITMENT_INDEX_LEASE_SECONDS,
                st.commitment_index_lock_expires_at_ts,
                st.commitment_index_lock_generation,
                timer_preempts_endowment_refresh,
            )?;
            st.commitment_index_lock_expires_at_ts = Some(inner.lease_expires_at_ts());
            st.commitment_index_lock_generation = inner.generation();
            st.commitment_index_lock_owner = Some(owner);
            Some(Self { inner })
        })
    }

    pub(super) fn token(&self) -> CommitmentIndexLeaseToken {
        CommitmentIndexLeaseToken {
            expires_at_ts: self.inner.lease_expires_at_ts(),
            generation: self.inner.generation(),
        }
    }

    fn release(&mut self) {
        state::with_root_state_mut(|st| {
            // Renewal changes the expiry while retaining the fenced generation.
            // Release by generation so the owning guard can clear its renewed
            // lease, but can never clear a successor that acquired a new one.
            if st.commitment_index_lock_generation == self.inner.generation() {
                st.commitment_index_lock_expires_at_ts = Some(0);
                st.commitment_index_lock_owner = None;
            }
        });
    }
}

pub(super) fn renew_current_commitment_index_lease(
    token: &mut Option<CommitmentIndexLeaseToken>,
    now_secs: u64,
) -> Result<(), String> {
    if let Some(current) = token.as_mut() {
        *current = current.renew(now_secs)?;
    }
    Ok(())
}

impl Drop for CommitmentIndexGuard {
    fn drop(&mut self) {
        self.release();
    }
}

pub(super) fn require_current_commitment_index_lease(
    token: Option<CommitmentIndexLeaseToken>,
) -> Result<(), String> {
    if token.is_none_or(CommitmentIndexLeaseToken::is_current) {
        Ok(())
    } else {
        Err("commitment index lease was superseded".to_string())
    }
}
