use super::*;
pub(super) fn enqueue_initial_cycles_probe(st: &mut crate::state::State, canister_id: candid::Principal) {
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

pub(super) fn should_probe_memo_registered_canister(st: &crate::state::State, canister_id: candid::Principal) -> bool {
    let Some(sources) = st.canister_sources.get(&canister_id) else {
        return false;
    };
    if logic::should_skip_blackhole_for_sources(sources) {
        return false;
    }
    crate::memo_source_is_registered(st, &canister_id, sources)
}

pub(super) fn build_cycles_sweep_canisters(
    st: &crate::state::State,
    self_id: candid::Principal,
) -> Vec<candid::Principal> {
    let mut canisters = vec![self_id];
    for canister_id in st.distinct_canisters.iter().copied() {
        let sources = st.canister_sources.get(&canister_id).cloned().unwrap_or_default();
        let memo_registered = crate::memo_source_is_registered(st, &canister_id, &sources);
        if !memo_registered && !sources.contains(&CanisterSource::SnsDiscovery) {
            continue;
        }
        if logic::should_skip_blackhole_for_sources(&sources) {
            continue;
        }
        canisters.push(canister_id);
    }
    canisters
}
