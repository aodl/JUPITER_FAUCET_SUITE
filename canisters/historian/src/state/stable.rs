use super::*;
pub(super) type Memory = VirtualMemory<DefaultMemoryImpl>;

// Historian stable memory IDs:
// 0: root state
// 10: canister source registry
// 11: per-canister metadata
// 14: commitment history index
// 15: cycles history index
// 16: commitment history entries
// 17: cycles history entries
// 18: raw ICP commitment history index
// 19: raw ICP commitment entries
// 20: neuron commitment history index
// 21: neuron commitment entries
// 22: relay registry by target
// 23: reserved (formerly relay targets by relay)
// 24: relay setup jobs by target
thread_local! {
    pub(super) static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    pub(super) static STABLE_ROOT_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_CANISTER_SOURCES_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableTrackingReasonSet, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_CANISTER_META_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableCanisterMeta, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_COMMITMENT_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_CYCLES_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_COMMITMENT_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_CYCLES_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CyclesEntryKey, CyclesSample, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_RAW_ICP_COMMITMENT_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_RAW_ICP_COMMITMENT_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_NEURON_COMMITMENT_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<u64, StableU64List, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_NEURON_COMMITMENT_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<NeuronCommitmentEntryKey, CommitmentSample, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_RELAY_REGISTRY_BY_TARGET_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, RelayRegistryEntry, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STABLE_RELAY_SETUP_JOBS_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, RelaySetupJob, Memory>>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
    pub(super) static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    pub(super) static PERSISTENCE_DIRTY_SECTIONS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    pub(super) static DIRTY_REGISTRY_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = const { std::cell::RefCell::new(BTreeSet::new()) };
    pub(super) static DIRTY_COMMITMENT_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = const { std::cell::RefCell::new(BTreeSet::new()) };
    pub(super) static DIRTY_CYCLES_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = const { std::cell::RefCell::new(BTreeSet::new()) };
    pub(super) static DIRTY_RAW_ICP_COMMITMENT_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = const { std::cell::RefCell::new(BTreeSet::new()) };
    pub(super) static DIRTY_NEURON_COMMITMENT_IDS: std::cell::RefCell<BTreeSet<u64>> = const { std::cell::RefCell::new(BTreeSet::new()) };
    pub(super) static DIRTY_RELAY_TARGETS: std::cell::RefCell<BTreeSet<Principal>> = const { std::cell::RefCell::new(BTreeSet::new()) };
}

pub(crate) const DIRTY_ROOT: u8 = 1 << 0;
pub(crate) const DIRTY_REGISTRY: u8 = 1 << 1;
pub(crate) const DIRTY_COMMITMENTS: u8 = 1 << 2;
pub(crate) const DIRTY_CYCLES: u8 = 1 << 3;
pub(crate) const DIRTY_RAW_ICP_COMMITMENTS: u8 = 1 << 4;
pub(crate) const DIRTY_NEURON_COMMITMENTS: u8 = 1 << 5;
pub(crate) const DIRTY_RELAY_FACTORY: u8 = 1 << 6;
pub(crate) const DIRTY_ALL: u8 = DIRTY_ROOT
    | DIRTY_REGISTRY
    | DIRTY_COMMITMENTS
    | DIRTY_CYCLES
    | DIRTY_RAW_ICP_COMMITMENTS
    | DIRTY_NEURON_COMMITMENTS
    | DIRTY_RELAY_FACTORY;

pub(super) fn with_root_stable_cell<R>(
    f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R,
) -> R {
    STABLE_ROOT_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized);
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian root stable cell not initialized"))
    })
}

pub(super) fn with_canister_tracking_reasons_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableTrackingReasonSet, Memory>) -> R,
) -> R {
    STABLE_CANISTER_SOURCES_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(10));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian canister tracking-reasons stable map not initialized"))
    })
}

pub(super) fn with_canister_meta_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableCanisterMeta, Memory>) -> R,
) -> R {
    STABLE_CANISTER_META_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(11));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian canister-meta stable map not initialized"))
    })
}

pub(super) fn with_commitment_history_index_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableU64List, Memory>) -> R,
) -> R {
    STABLE_COMMITMENT_HISTORY_INDEX_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(14));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian commitment-history index map not initialized"))
    })
}

pub(super) fn with_cycles_history_index_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableU64List, Memory>) -> R,
) -> R {
    STABLE_CYCLES_HISTORY_INDEX_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(15));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian cycles-history index map not initialized"))
    })
}

pub(super) fn with_commitment_entry_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>) -> R,
) -> R {
    STABLE_COMMITMENT_ENTRY_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(16));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian commitment entry map not initialized"))
    })
}

pub(super) fn with_cycles_entry_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<CyclesEntryKey, CyclesSample, Memory>) -> R,
) -> R {
    STABLE_CYCLES_ENTRY_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(17));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian cycles entry map not initialized"))
    })
}

pub(super) fn with_raw_icp_commitment_history_index_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableU64List, Memory>) -> R,
) -> R {
    STABLE_RAW_ICP_COMMITMENT_HISTORY_INDEX_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(18));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian raw ICP commitment-history index map not initialized"))
    })
}

pub(super) fn with_raw_icp_commitment_entry_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>) -> R,
) -> R {
    STABLE_RAW_ICP_COMMITMENT_ENTRY_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(19));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian raw ICP commitment entry map not initialized"))
    })
}

pub(super) fn with_neuron_commitment_history_index_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<u64, StableU64List, Memory>) -> R,
) -> R {
    STABLE_NEURON_COMMITMENT_HISTORY_INDEX_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(20));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian neuron commitment-history index map not initialized"))
    })
}

pub(super) fn with_neuron_commitment_entry_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<NeuronCommitmentEntryKey, CommitmentSample, Memory>) -> R,
) -> R {
    STABLE_NEURON_COMMITMENT_ENTRY_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(21));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian neuron commitment entry map not initialized"))
    })
}

pub(super) fn with_relay_registry_by_target_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, RelayRegistryEntry, Memory>) -> R,
) -> R {
    STABLE_RELAY_REGISTRY_BY_TARGET_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(22));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian relay-registry stable map not initialized"))
    })
}

pub(super) fn with_relay_setup_jobs_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, RelaySetupJob, Memory>) -> R,
) -> R {
    STABLE_RELAY_SETUP_JOBS_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(24));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow
            .as_mut()
            .expect("historian relay setup jobs stable map not initialized"))
    })
}
