use super::*;
#[cfg(test)]
thread_local! {
    pub(super) static SKIP_JUMP_OBSERVATIONS: std::cell::RefCell<Option<Vec<ActivePayoutJob>>> = const { std::cell::RefCell::new(None) };
}
#[derive(Clone, Debug, Default)]
pub(super) struct LocalSkipCandidate {
    pub(super) start_tx_id: Option<u64>,
    pub(super) end_tx_id: Option<u64>,
    pub(super) tx_count: u64,
}

impl LocalSkipCandidate {
    pub(super) fn from_job(job: &ActivePayoutJob) -> Self {
        Self {
            start_tx_id: job.skip_candidate_start_tx_id,
            end_tx_id: job.skip_candidate_end_tx_id,
            tx_count: job.skip_candidate_tx_count,
        }
    }

    pub(super) fn note_skippable(&mut self, tx_id: u64) {
        if self.tx_count == 0 {
            self.start_tx_id = Some(tx_id);
            self.end_tx_id = Some(tx_id);
            self.tx_count = 1;
            return;
        }
        self.end_tx_id = Some(tx_id);
        self.tx_count = self.tx_count.saturating_add(1);
    }

    pub(super) fn finish_span(&mut self) -> Option<SkipRange> {
        // Calls cover one uninterrupted descending sequence of validated account
        // records. Count records, never global block-ID width.
        let range = (self.tx_count >= MIN_SKIP_RANGE_TX_COUNT).then(|| SkipRange {
            start_tx_id: self.end_tx_id.expect("skip span low missing"),
            end_tx_id: self.start_tx_id.expect("skip span high missing"),
        });
        *self = Self::default();
        range
    }
}

pub(super) fn record_completed_skip_range(
    skip_candidate: &mut LocalSkipCandidate,
    pending_skip_ranges: &mut Vec<SkipRange>,
) {
    if let Some(range) = skip_candidate.finish_span() {
        pending_skip_ranges.push(range);
    }
}

pub(super) fn persist_new_skip_ranges(
    pending_skip_ranges: &mut Vec<SkipRange>,
) -> Result<(), state::SkipRangeInsertError> {
    for range in pending_skip_ranges.drain(..) {
        state::insert_skip_range(range)?;
    }
    Ok(())
}

/// The cursor excludes itself. Only jump if its next possible unread ID is
/// inside validated exclusion evidence; never cross an unverified gap. The new
/// cursor is the inclusive low endpoint, NOT low - 1.
pub(super) fn skip_cached_history(
    job: &ActivePayoutJob,
    lease: MainLeaseToken,
) -> Result<bool, state::SkipRangeInsertError> {
    let Some(oldest) = job.observed_oldest_tx_id else {
        return Ok(false);
    };
    let Some(unread) = job.next_start.and_then(|id| id.checked_sub(1)) else {
        return Ok(false);
    };
    let Some(range) = state::skip_range_containing(unread)? else {
        return Ok(false);
    };
    if range.start_tx_id < oldest {
        return Err(state::SkipRangeInsertError::InvalidRange);
    }
    let complete = range.start_tx_id == oldest;
    #[cfg(test)]
    SKIP_JUMP_OBSERVATIONS.with(|observations| {
        if let Some(observations) = observations.borrow_mut().as_mut() {
            observations.push(job.clone());
        }
    });
    state::with_state_mut(|st| {
        if !lease.is_current_in(st) {
            return Ok(false);
        }
        let Some(active) = st.active_payout_job.as_mut().filter(|active| {
            active.id == job.id
                && active.next_start == job.next_start
                && active.effective_denom_scan_complete == job.effective_denom_scan_complete
                && active.scan_complete == job.scan_complete
                && active.pending_transfer.is_none()
        }) else {
            return Ok(false);
        };
        if !effective_denom_scan_complete(active) {
            // Finish only the observations already examined under this owner.
            // Work on a copy so an insertion error preserves the recoverable
            // partial span and cursor, including oldest-anchor completion.
            if let Some(learned) = LocalSkipCandidate::from_job(active).finish_span() {
                state::insert_skip_range(learned)?;
            }
        }
        active.next_start = Some(range.start_tx_id);
        // Never join newly read observations across a cached jump. The cache
        // proves exclusion but does not encode a record count for learning.
        active.skip_candidate_start_tx_id = None;
        active.skip_candidate_end_tx_id = None;
        active.skip_candidate_tx_count = 0;
        if complete {
            if effective_denom_scan_complete(active) {
                active.scan_complete = true;
            } else {
                active.effective_denom_scan_complete = Some(true);
                active.next_start = None;
            }
        }
        Ok(true)
    })
}

pub(super) fn latch_skip_range_invariant_rescue() {
    log_error(3111);
    state::latch_skip_range_invariant_fault();
}
pub(super) fn flush_scan_progress(
    ignored_under_threshold_delta: &mut u64,
    ignored_bad_memo_delta: &mut u64,
    next_start: Option<u64>,
    skip_candidate: &LocalSkipCandidate,
    lease: MainLeaseToken,
    job_id: u64,
    expected_cursor: Option<u64>,
) {
    if *ignored_under_threshold_delta == 0
        && *ignored_bad_memo_delta == 0
        && next_start.is_none()
        && skip_candidate.tx_count == 0
    {
        return;
    }
    state::with_state_mut(|st| {
        if !lease.is_current_in(st) {
            return;
        }
        if let Some(job) = st
            .active_payout_job
            .as_mut()
            .filter(|job| job.id == job_id && job.next_start == expected_cursor)
        {
            job.ignored_under_threshold = job
                .ignored_under_threshold
                .saturating_add(*ignored_under_threshold_delta);
            job.ignored_bad_memo = job.ignored_bad_memo.saturating_add(*ignored_bad_memo_delta);
            if next_start.is_some() {
                job.next_start = next_start;
            }
            job.skip_candidate_start_tx_id = skip_candidate.start_tx_id;
            job.skip_candidate_end_tx_id = skip_candidate.end_tx_id;
            job.skip_candidate_tx_count = skip_candidate.tx_count;
        }
    });
    *ignored_under_threshold_delta = 0;
    *ignored_bad_memo_delta = 0;
}
