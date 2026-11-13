//! Disjoint programme memories; the deployed V1 layout in slots 0..=2 is retained.
use super::types::*;
use candid::Principal;
use ic_stable_structures::{
    memory_manager::{MemoryId, VirtualMemory},
    DefaultMemoryImpl, StableBTreeMap, StableCell,
};
use jupiter_ic_clients::account_identifier::account_identifier_bytes;
use std::cell::RefCell;

type Memory = VirtualMemory<DefaultMemoryImpl>;
type Owners = StableBTreeMap<[u8; 32], Vec<u8>, Memory>;
thread_local! {
    static META: RefCell<Option<StableCell<Vec<u8>, Memory>>> = const {RefCell::new(None)};
    static OWNERS_A: RefCell<Option<Owners>> = const {RefCell::new(None)};
    static OWNERS_C: RefCell<Option<Owners>> = const {RefCell::new(None)};
    static OWNERS_B: RefCell<Option<Owners>> = const {RefCell::new(None)};
    static RECORDS: RefCell<Option<StableBTreeMap<u64, Vec<u8>, Memory>>> = const {RefCell::new(None)};
}

fn memory(id: u8) -> Memory {
    crate::state::programme_memory(MemoryId::new(id))
}

pub fn read() -> Runtime {
    META.with_borrow_mut(|cell| {
        let cell = cell.get_or_insert_with(|| StableCell::init(memory(3), Vec::new()));
        if cell.get().is_empty() {
            Runtime::default()
        } else {
            candid::decode_one(cell.get()).expect("programme metadata")
        }
    })
}

pub fn mutate<R>(f: impl FnOnce(&mut Runtime) -> R) -> R {
    let mut state = read();
    let result = f(&mut state);
    state.last_error = state.last_error.as_deref().map(super::compact);
    META.with_borrow_mut(|cell| {
        cell.as_mut()
            .expect("initialized programme metadata")
            .set(candid::encode_one(&state).expect("encode programme metadata"));
    });
    result
}

fn owners<R>(slot: u8, f: impl FnOnce(&mut Owners) -> R) -> R {
    let (map, id) = match slot {
        0 => (&OWNERS_A, 4),
        1 => (&OWNERS_B, 5),
        2 => (&OWNERS_C, 21),
        _ => panic!("owner slot"),
    };
    map.with_borrow_mut(|map| f(map.get_or_insert_with(|| StableBTreeMap::init(memory(id)))))
}

pub fn clear(slot: u8) {
    owners(slot, |map| map.clear_new());
}
pub fn len(slot: u8) -> u64 {
    owners(slot, |map| map.len())
}
pub fn lookup(slot: u8, account: [u8; 32]) -> Option<Owner> {
    owners(slot, |map| {
        map.get(&account)
            .map(|v| candid::decode_one(&v).expect("owner record"))
    })
}
pub fn insert(slot: u8, principal: Principal, neuron: Option<Vec<u8>>) {
    let key = account_identifier_bytes(principal, None);
    owners(slot, |map| {
        let mut value: Owner = map
            .get(&key)
            .map(|v| candid::decode_one(&v).expect("owner record"))
            .unwrap_or(Owner {
                principal,
                compound_neuron: None,
            });
        if let Some(id) = neuron {
            if value.compound_neuron.as_ref().is_none_or(|old| &id < old) {
                value.compound_neuron = Some(id);
            }
        }
        map.insert(key, candid::encode_one(value).expect("encode owner"));
    });
}

pub fn record(block: u64) -> Option<PatronRecord> {
    RECORDS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(6)))
            .get(&block)
            .map(|v| candid::decode_one(&v).expect("patron record"))
    })
}

/// At most `limit` journal records, in ascending exclusive-key order. A caller
/// persists its cursor even when these records are all terminal.
pub(super) fn retry_page(after: Option<u64>, end: u64, limit: usize) -> Vec<PatronRecord> {
    use std::ops::Bound::{Excluded, Included, Unbounded};
    RECORDS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(6)))
            .range((after.map_or(Unbounded, Excluded), Included(end)))
            .take(limit)
            .map(|entry| candid::decode_one(&entry.value()).expect("patron record"))
            .collect()
    })
}

pub(super) fn last_record_block() -> Option<u64> {
    RECORDS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(6)))
            .last_key_value()
            .map(|(key, _)| key)
    })
}

pub(super) fn defer_relay(record: &PatronRecord) {
    assert!(matches!(
        record.outcome,
        PatronOutcome::AwaitingRelayMembership { .. }
    ));
    let already_deferred = self::record(record.contribution_block)
        .is_some_and(|r| matches!(r.outcome, PatronOutcome::AwaitingRelayMembership { .. }));
    put(record);
    if !already_deferred {
        mutate(|s| {
            let retry = s.relay_retries.get_or_insert_with(RelayRetries::default);
            retry.outstanding = retry
                .outstanding
                .checked_add(1)
                .expect("deferred count overflow");
        });
    }
}

pub fn put(record: &PatronRecord) {
    let mut record = record.clone();
    match &mut record.outcome {
        PatronOutcome::AssumedPaidUnverified { reason }
        | PatronOutcome::Skipped { reason }
        | PatronOutcome::AwaitingRelayMembership { reason, .. } => *reason = super::compact(reason),
        PatronOutcome::Failed { error } => *error = super::compact(error),
        _ => {}
    }
    record.refresh_result = record.refresh_result.as_deref().map(super::compact);
    RECORDS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(6)))
            .insert(
                record.contribution_block,
                candid::encode_one(record).expect("encode patron record"),
            );
    });
}

pub fn configure(config: ProgrammeConfig) -> Result<(), String> {
    config.validate()?;
    let current = read();
    if current.config.as_ref() == Some(&config) {
        return Ok(());
    }
    if current.batch.is_some()
        || current.pending_block.is_some()
        || super::treasury::has_requested_operation()
    {
        return Err(
            "finish the pinned patron batch before changing programme configuration".into(),
        );
    }
    if !super::automation::can_reconfigure() || !super::treasury::can_reconfigure() {
        return Err(
            "finish or explicitly resolve existing governance work before reconfiguration".into(),
        );
    }
    if current
        .config
        .as_ref()
        .is_some_and(|old| config.version <= old.version)
    {
        return Err("configuration version must increase".into());
    }
    if current
        .config
        .as_ref()
        .is_some_and(|old| !config.preserves_financial_bindings(old))
    {
        return Err("programme token, commitment and reserve identities are immutable".into());
    }
    clear(0);
    clear(1);
    clear(2);
    mutate(|state| {
        state.next_snapshot = state
            .next_snapshot
            .max(crate::state::with_state(|legacy| legacy.next_snapshot_id));
        state.config = Some(config);
        state.snapshot = None;
        state.scan = None;
    });
    super::bind_relay_root();
    Ok(())
}

#[cfg(test)]
pub(super) fn clear_records_for_test() {
    RECORDS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(6)))
            .clear_new();
    });
}
