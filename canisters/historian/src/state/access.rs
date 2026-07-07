use super::*;
pub(crate) fn set_state(st: State) {
    persist_snapshot(&st);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub(crate) fn set_state_root_only(st: State) {
    persist_snapshot_sections(&st, DIRTY_ROOT);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub(crate) fn get_state() -> State {
    STATE
        .with(|s| s.borrow().clone())
        .expect("state not initialized")
}

pub(crate) fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

pub(super) fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| jupiter_persistence_batch::is_active(depth.get()))
}

pub(super) fn mark_persistence_dirty(dirty_sections: u8) {
    PERSISTENCE_DIRTY_SECTIONS.with(|dirty| dirty.set(dirty.get() | dirty_sections));
}

pub(super) fn clear_persistence_dirty() {
    PERSISTENCE_DIRTY_SECTIONS.with(|dirty| dirty.set(0));
    DIRTY_REGISTRY_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_COMMITMENT_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_CYCLES_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_RAW_ICP_COMMITMENT_PRINCIPALS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_NEURON_COMMITMENT_IDS.with(|dirty| dirty.borrow_mut().clear());
    DIRTY_RELAY_TARGETS.with(|dirty| dirty.borrow_mut().clear());
}

pub(crate) fn persist_dirty_state() {
    let dirty_sections = PERSISTENCE_DIRTY_SECTIONS.with(|flag| flag.get());
    if dirty_sections == 0 {
        return;
    }
    let registry_scope = dirty_registry_principals();
    let commitment_scope = dirty_commitment_principals();
    let cycles_scope = dirty_cycles_principals();
    let raw_icp_commitment_scope = dirty_raw_icp_commitment_principals();
    let neuron_commitment_scope = dirty_neuron_commitment_ids();
    let relay_target_scope = dirty_relay_targets();
    let snapshot = get_state();
    persist_snapshot_sections_scoped(
        &snapshot,
        dirty_sections,
        (!registry_scope.is_empty()).then_some(&registry_scope),
        (!commitment_scope.is_empty()).then_some(&commitment_scope),
        (!cycles_scope.is_empty()).then_some(&cycles_scope),
        (!raw_icp_commitment_scope.is_empty()).then_some(&raw_icp_commitment_scope),
        (!neuron_commitment_scope.is_empty()).then_some(&neuron_commitment_scope),
        (!relay_target_scope.is_empty()).then_some(&relay_target_scope),
    );
    clear_persistence_dirty();
}

pub(crate) type PersistenceBatch = jupiter_persistence_batch::PersistenceBatch;

#[must_use]
pub(crate) fn begin_persistence_batch() -> PersistenceBatch {
    PERSISTENCE_BATCH_DEPTH
        .with(|depth| depth.set(jupiter_persistence_batch::begin_depth(depth.get())));
    PersistenceBatch::new(|| {
        let should_flush = PERSISTENCE_BATCH_DEPTH.with(|depth| {
            let dirty_sections = PERSISTENCE_DIRTY_SECTIONS.with(|flag| flag.get());
            let (next_depth, should_flush) =
                jupiter_persistence_batch::finish_depth(depth.get(), dirty_sections != 0);
            depth.set(next_depth);
            should_flush
        });
        if should_flush {
            persist_dirty_state();
        }
    })
}

pub(super) fn with_state_mut_sections_scoped<R>(
    dirty_sections: u8,
    registry_principal: Option<Principal>,
    commitment_principal: Option<Principal>,
    cycles_principal: Option<Principal>,
    raw_icp_commitment_principal: Option<Principal>,
    neuron_commitment_id: Option<u64>,
    relay_target: Option<Principal>,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        let immediate_persist = !persistence_batch_active();
        let out = f(st);
        if immediate_persist {
            let snapshot = st.clone();
            drop(borrow);
            let registry_scope = registry_principal.into_iter().collect::<BTreeSet<_>>();
            let commitment_scope = commitment_principal.into_iter().collect::<BTreeSet<_>>();
            let cycles_scope = cycles_principal.into_iter().collect::<BTreeSet<_>>();
            let raw_icp_commitment_scope = raw_icp_commitment_principal
                .into_iter()
                .collect::<BTreeSet<_>>();
            let neuron_commitment_scope = neuron_commitment_id.into_iter().collect::<BTreeSet<_>>();
            let relay_target_scope = relay_target.into_iter().collect::<BTreeSet<_>>();
            persist_snapshot_sections_scoped(
                &snapshot,
                dirty_sections,
                (!registry_scope.is_empty()).then_some(&registry_scope),
                (!commitment_scope.is_empty()).then_some(&commitment_scope),
                (!cycles_scope.is_empty()).then_some(&cycles_scope),
                (!raw_icp_commitment_scope.is_empty()).then_some(&raw_icp_commitment_scope),
                (!neuron_commitment_scope.is_empty()).then_some(&neuron_commitment_scope),
                (!relay_target_scope.is_empty()).then_some(&relay_target_scope),
            );
            return out;
        }
        if let Some(canister_id) = registry_principal {
            mark_registry_principal_dirty(canister_id);
        }
        if let Some(canister_id) = commitment_principal {
            mark_commitment_principal_dirty(canister_id);
        }
        if let Some(canister_id) = cycles_principal {
            mark_cycles_principal_dirty(canister_id);
        }
        if let Some(canister_id) = raw_icp_commitment_principal {
            mark_raw_icp_commitment_principal_dirty(canister_id);
        }
        if let Some(neuron_id) = neuron_commitment_id {
            mark_neuron_commitment_id_dirty(neuron_id);
        }
        if let Some(target) = relay_target {
            mark_relay_target_dirty(target);
        }
        mark_persistence_dirty(dirty_sections);
        drop(borrow);
        out
    })
}

pub(crate) fn with_state_mut_sections<R>(dirty_sections: u8, f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections_scoped(dirty_sections, None, None, None, None, None, None, f)
}

#[cfg(any(test, feature = "debug_api"))]
pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections(DIRTY_ALL, f)
}

pub(crate) fn with_root_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    with_state_mut_sections(DIRTY_ROOT, f)
}

pub(crate) fn clear_loaded_history_caches_after_flush() {
    let batch_depth = PERSISTENCE_BATCH_DEPTH.with(|depth| depth.get());
    assert_eq!(
        batch_depth, 0,
        "cannot clear loaded history caches during persistence batch"
    );
    let dirty_sections = PERSISTENCE_DIRTY_SECTIONS.with(|dirty| dirty.get());
    assert_eq!(
        dirty_sections, 0,
        "cannot clear loaded history caches while persistence sections are dirty"
    );

    with_state_mut_sections(DIRTY_ROOT, |st| {
        st.commitment_history.clear();
        st.cycles_history.clear();
        st.raw_icp_commitment_history.clear();
        st.neuron_commitment_history.clear();
    });
}

pub(crate) fn stable_commitment_history_keys() -> BTreeSet<Principal> {
    stable_commitment_history_keys_internal()
}

pub(crate) fn stable_cycles_history_keys() -> BTreeSet<Principal> {
    stable_cycles_history_keys_internal()
}

pub(crate) fn stable_raw_icp_commitment_history_keys() -> BTreeSet<Principal> {
    stable_raw_icp_commitment_history_keys_internal()
}

pub(crate) fn stable_neuron_commitment_history_keys() -> BTreeSet<u64> {
    stable_neuron_commitment_history_keys_internal()
}

pub(crate) fn stable_commitment_history_for(canister_id: Principal) -> Vec<CommitmentSample> {
    load_stable_commitment_history_internal(canister_id)
}

pub(crate) fn stable_cycles_history_for(canister_id: Principal) -> Vec<CyclesSample> {
    load_stable_cycles_history_internal(canister_id)
}

pub(crate) fn stable_raw_icp_commitment_history_for(
    canister_id: Principal,
) -> Vec<CommitmentSample> {
    load_stable_raw_icp_commitment_history_internal(canister_id)
}

pub(crate) fn stable_neuron_commitment_history_for(neuron_id: u64) -> Vec<CommitmentSample> {
    load_stable_neuron_commitment_history_internal(neuron_id)
}

pub(crate) fn ensure_commitment_history_loaded(st: &mut State, canister_id: Principal) {
    if st.commitment_history.contains_key(&canister_id) {
        return;
    }
    let history = load_stable_commitment_history_internal(canister_id);
    if !history.is_empty() {
        st.commitment_history.insert(canister_id, history);
    }
}

pub(crate) fn ensure_raw_icp_commitment_history_loaded(st: &mut State, canister_id: Principal) {
    if st.raw_icp_commitment_history.contains_key(&canister_id) {
        return;
    }
    let history = load_stable_raw_icp_commitment_history_internal(canister_id);
    if !history.is_empty() {
        st.raw_icp_commitment_history.insert(canister_id, history);
    }
}

pub(crate) fn ensure_neuron_commitment_history_loaded(st: &mut State, neuron_id: u64) {
    if st.neuron_commitment_history.contains_key(&neuron_id) {
        return;
    }
    let history = load_stable_neuron_commitment_history_internal(neuron_id);
    if !history.is_empty() {
        st.neuron_commitment_history.insert(neuron_id, history);
    }
}

pub(crate) fn ensure_cycles_history_loaded(st: &mut State, canister_id: Principal) {
    if st.cycles_history.contains_key(&canister_id) {
        return;
    }
    let history = load_stable_cycles_history_internal(canister_id);
    if !history.is_empty() {
        st.cycles_history.insert(canister_id, history);
    }
}

pub(crate) fn with_root_and_registry_canister_state_mut<R>(
    canister_id: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_REGISTRY,
        Some(canister_id),
        None,
        None,
        None,
        None,
        None,
        f,
    )
}

pub(crate) fn with_root_and_raw_icp_commitments_state_mut<R>(
    canister_id: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_RAW_ICP_COMMITMENTS,
        None,
        None,
        None,
        Some(canister_id),
        None,
        None,
        f,
    )
}

pub(crate) fn with_root_and_neuron_commitments_state_mut<R>(
    neuron_id: u64,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_NEURON_COMMITMENTS,
        None,
        None,
        None,
        None,
        Some(neuron_id),
        None,
        f,
    )
}

pub(crate) fn with_root_registry_and_commitments_canister_state_mut<R>(
    canister_id: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_REGISTRY | DIRTY_COMMITMENTS,
        Some(canister_id),
        Some(canister_id),
        None,
        None,
        None,
        None,
        f,
    )
}

pub(crate) fn with_root_registry_and_cycles_canister_state_mut<R>(
    canister_id: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_REGISTRY | DIRTY_CYCLES,
        Some(canister_id),
        None,
        Some(canister_id),
        None,
        None,
        None,
        f,
    )
}

pub(crate) fn with_root_and_relay_factory_state_mut<R>(
    target: Principal,
    f: impl FnOnce(&mut State) -> R,
) -> R {
    with_state_mut_sections_scoped(
        DIRTY_ROOT | DIRTY_RELAY_FACTORY,
        None,
        None,
        None,
        None,
        None,
        Some(target),
        f,
    )
}
