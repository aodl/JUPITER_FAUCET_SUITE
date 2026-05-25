use super::*;
pub(super) fn commitment_sort_key(item: &RecentCommitment) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

pub(super) fn push_recent_commitment(recent: &mut Vec<RecentCommitment>, item: RecentCommitment, max_entries: usize) -> bool {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return false;
    }
    recent.push(item);
    recent.sort_by_key(|item| std::cmp::Reverse(commitment_sort_key(item)));
    if recent.len() > max_entries {
        recent.truncate(max_entries);
    }
    true
}

pub(super) fn push_recent_neuron_commitment(recent: &mut Vec<RecentNeuronCommitment>, item: RecentNeuronCommitment, max_entries: usize) -> bool {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return false;
    }
    recent.push(item);
    recent.sort_by_key(|item| std::cmp::Reverse((item.timestamp_nanos.unwrap_or(0), item.tx_id)));
    if recent.len() > max_entries {
        recent.truncate(max_entries);
    }
    true
}

pub(super) fn push_recent_invalid_commitment(recent: &mut Vec<InvalidCommitment>, item: InvalidCommitment) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| (b.timestamp_nanos.unwrap_or(0), b.tx_id).cmp(&(a.timestamp_nanos.unwrap_or(0), a.tx_id)));
    if recent.len() > MAX_RECENT_INVALID_COMMITMENTS {
        recent.truncate(MAX_RECENT_INVALID_COMMITMENTS);
    }
}
