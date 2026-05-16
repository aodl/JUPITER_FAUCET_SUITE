type Memory = VirtualMemory<DefaultMemoryImpl>;

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
thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_ROOT_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CANISTER_SOURCES_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableSourceSet, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CANISTER_META_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableCanisterMeta, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_COMMITMENT_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CYCLES_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_COMMITMENT_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_CYCLES_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CyclesEntryKey, CyclesSample, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_RAW_ICP_COMMITMENT_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<PrincipalKey, StableU64List, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_RAW_ICP_COMMITMENT_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<CommitmentEntryKey, CommitmentSample, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_NEURON_COMMITMENT_HISTORY_INDEX_MAP: std::cell::RefCell<Option<StableBTreeMap<u64, StableU64List, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_NEURON_COMMITMENT_ENTRY_MAP: std::cell::RefCell<Option<StableBTreeMap<NeuronCommitmentEntryKey, CommitmentSample, Memory>>> =
        std::cell::RefCell::new(None);
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
    static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static PERSISTENCE_DIRTY_SECTIONS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static DIRTY_REGISTRY_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
    static DIRTY_COMMITMENT_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
    static DIRTY_CYCLES_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
    static DIRTY_RAW_ICP_COMMITMENT_PRINCIPALS: std::cell::RefCell<BTreeSet<Principal>> = std::cell::RefCell::new(BTreeSet::new());
    static DIRTY_NEURON_COMMITMENT_IDS: std::cell::RefCell<BTreeSet<u64>> = std::cell::RefCell::new(BTreeSet::new());
}

pub const DIRTY_ROOT: u8 = 1 << 0;
pub const DIRTY_REGISTRY: u8 = 1 << 1;
pub const DIRTY_COMMITMENTS: u8 = 1 << 2;
pub const DIRTY_CYCLES: u8 = 1 << 3;
pub const DIRTY_RAW_ICP_COMMITMENTS: u8 = 1 << 4;
pub const DIRTY_NEURON_COMMITMENTS: u8 = 1 << 5;
pub const DIRTY_ALL: u8 = DIRTY_ROOT
    | DIRTY_REGISTRY
    | DIRTY_COMMITMENTS
    | DIRTY_CYCLES
    | DIRTY_RAW_ICP_COMMITMENTS
    | DIRTY_NEURON_COMMITMENTS;

fn with_root_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_ROOT_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized)
                    .expect("failed to initialize historian root stable cell");
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("historian root stable cell not initialized"))
    })
}





fn with_canister_sources_map<R>(
    f: impl FnOnce(&mut StableBTreeMap<PrincipalKey, StableSourceSet, Memory>) -> R,
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
        f(borrow.as_mut().expect("historian canister-sources stable map not initialized"))
    })
}

fn with_canister_meta_map<R>(
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
        f(borrow.as_mut().expect("historian canister-meta stable map not initialized"))
    })
}



fn with_commitment_history_index_map<R>(
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
        f(borrow.as_mut().expect("historian commitment-history index map not initialized"))
    })
}

fn with_cycles_history_index_map<R>(
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
        f(borrow.as_mut().expect("historian cycles-history index map not initialized"))
    })
}

fn with_commitment_entry_map<R>(
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
        f(borrow.as_mut().expect("historian commitment entry map not initialized"))
    })
}

fn with_cycles_entry_map<R>(
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
        f(borrow.as_mut().expect("historian cycles entry map not initialized"))
    })
}

fn with_raw_icp_commitment_history_index_map<R>(
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
        f(borrow.as_mut().expect("historian raw ICP commitment-history index map not initialized"))
    })
}

fn with_raw_icp_commitment_entry_map<R>(
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
        f(borrow.as_mut().expect("historian raw ICP commitment entry map not initialized"))
    })
}

fn with_neuron_commitment_history_index_map<R>(
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
        f(borrow.as_mut().expect("historian neuron commitment-history index map not initialized"))
    })
}

fn with_neuron_commitment_entry_map<R>(
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
        f(borrow.as_mut().expect("historian neuron commitment entry map not initialized"))
    })
}

