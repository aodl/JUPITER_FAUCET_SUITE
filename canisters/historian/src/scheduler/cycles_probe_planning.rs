use super::*;
pub(crate) fn enqueue_initial_cycles_probe(
    st: &mut crate::state::State,
    canister_id: candid::Principal,
) {
    if st.initial_cycles_probe_queue.contains(&canister_id) {
        return;
    }
    if st.initial_cycles_probe_queue.len() >= MAX_INITIAL_CYCLES_PROBE_QUEUE {
        // The normal periodic sweep still covers this canister. Refusing to
        // grow the targeted queue further keeps a burst of paid commitments from
        // turning into unbounded historian work or state growth.
        return;
    }
    st.initial_cycles_probe_queue.push(canister_id);
}

pub(super) fn should_probe_memo_registered_canister(
    st: &crate::state::State,
    canister_id: candid::Principal,
) -> bool {
    crate::visible_tracking_reasons_for_canister(st, &canister_id).is_some()
}

pub(super) fn build_cycles_sweep_canisters(
    st: &crate::state::State,
    self_id: candid::Principal,
) -> Vec<candid::Principal> {
    let mut canisters = vec![self_id];
    let mut seen = std::collections::BTreeSet::from([self_id]);
    for canister_id in st.distinct_canisters.iter().copied() {
        if crate::visible_tracking_reasons_for_canister(st, &canister_id).is_none() {
            continue;
        }
        if seen.insert(canister_id) {
            canisters.push(canister_id);
        }
    }
    canisters
}
