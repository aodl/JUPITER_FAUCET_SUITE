#[derive(Clone, Debug, Default)]
struct LocalSkipCandidate {
    start_tx_id: Option<u64>,
    end_tx_id: Option<u64>,
    tx_count: u64,
}

impl LocalSkipCandidate {
    fn from_job(job: &ActivePayoutJob) -> Self {
        Self {
            start_tx_id: job.skip_candidate_start_tx_id,
            end_tx_id: job.skip_candidate_end_tx_id,
            tx_count: job.skip_candidate_tx_count,
        }
    }

    fn note_skippable(&mut self, tx_id: u64) {
        if self.tx_count == 0 {
            self.start_tx_id = Some(tx_id);
            self.end_tx_id = Some(tx_id);
            self.tx_count = 1;
            return;
        }
        self.end_tx_id = Some(tx_id);
        self.tx_count = self.tx_count.saturating_add(1);
    }

    fn finish_span(&mut self) -> Option<SkipRange> {
        let range = if self.tx_count >= MIN_SKIP_RANGE_TX_COUNT {
            Some(SkipRange {
                start_tx_id: self.start_tx_id.expect("skip span start missing"),
                end_tx_id: self.end_tx_id.expect("skip span end missing"),
            })
        } else {
            None
        };
        *self = Self::default();
        range
    }
}

fn initial_skip_range_index(skip_ranges: &[SkipRange], cursor: Option<u64>) -> usize {
    let Some(last_seen) = cursor else { return 0; };
    for (idx, range) in skip_ranges.iter().enumerate() {
        if range.end_tx_id > last_seen {
            return idx;
        }
    }
    skip_ranges.len()
}

fn next_skip_jump_target(cursor: Option<u64>, skip_ranges: &[SkipRange], skip_range_idx: &mut usize) -> Option<u64> {
    let Some(last_seen) = cursor else { return None; };
    while let Some(range) = skip_ranges.get(*skip_range_idx) {
        if last_seen >= range.end_tx_id {
            *skip_range_idx += 1;
            continue;
        }
        let next_unread = last_seen.saturating_add(1);
        if next_unread >= range.start_tx_id && next_unread <= range.end_tx_id {
            return Some(range.end_tx_id);
        }
        return None;
    }
    None
}

fn record_completed_skip_range(
    skip_candidate: &mut LocalSkipCandidate,
    pending_skip_ranges: &mut Vec<SkipRange>,
) {
    if let Some(range) = skip_candidate.finish_span() {
        pending_skip_ranges.push(range);
    }
}

fn persist_new_skip_ranges(
    skip_ranges: &mut Vec<SkipRange>,
    pending_skip_ranges: &mut Vec<SkipRange>,
) -> Result<(), state::SkipRangeInsertError> {
    let mut simulated = skip_ranges.clone();
    for range in pending_skip_ranges.iter() {
        state::validate_skip_range_insertion(&simulated, range)?;
        let insert_pos = simulated.partition_point(|candidate| candidate.start_tx_id < range.start_tx_id);
        simulated.insert(insert_pos, range.clone());
    }
    for range in pending_skip_ranges.drain(..) {
        state::insert_skip_range(range.clone())?;
        let insert_pos = skip_ranges.partition_point(|candidate| candidate.start_tx_id < range.start_tx_id);
        skip_ranges.insert(insert_pos, range);
    }
    Ok(())
}

fn latch_skip_range_invariant_rescue() {
    log_error(3111);
    state::latch_skip_range_invariant_fault();
}
fn flush_scan_progress(
    ignored_under_threshold_delta: &mut u64,
    ignored_bad_memo_delta: &mut u64,
    next_start: Option<u64>,
    skip_candidate: &LocalSkipCandidate,
) {
    if *ignored_under_threshold_delta == 0
        && *ignored_bad_memo_delta == 0
        && next_start.is_none()
        && skip_candidate.tx_count == 0
    {
        return;
    }
    state::with_state_mut(|st| {
        if let Some(job) = st.active_payout_job.as_mut() {
            job.ignored_under_threshold = job
                .ignored_under_threshold
                .saturating_add(*ignored_under_threshold_delta);
            job.ignored_bad_memo = job
                .ignored_bad_memo
                .saturating_add(*ignored_bad_memo_delta);
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

