use std::borrow::Cow;

use candid::{CandidType, Deserialize, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
use jupiter_ic_clients::account_identifier::account_identifier_bytes;
use serde::Serialize;

pub(crate) const METADATA_MEMORY_ID: u8 = 0;
pub(crate) const OWNER_INDEX_A_MEMORY_ID: u8 = 1;
pub(crate) const OWNER_INDEX_B_MEMORY_ID: u8 = 2;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Config {
    pub reward_sns_root_canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnerIndexSlot {
    A,
    B,
}

impl OwnerIndexSlot {
    pub(crate) fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnerSnapshot {
    pub snapshot_id: u64,
    pub active_slot: OwnerIndexSlot,
    pub sns_root_canister_id: Principal,
    pub sns_governance_canister_id: Principal,
    pub sns_ledger_canister_id: Principal,
    pub scan_started_at_timestamp_nanos: u64,
    pub scan_completed_at_timestamp_nanos: u64,
    pub neuron_count: u64,
    pub owner_count: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnerScan {
    pub staging_slot: OwnerIndexSlot,
    pub sns_root_canister_id: Principal,
    pub sns_governance_canister_id: Principal,
    pub sns_ledger_canister_id: Principal,
    pub scan_started_at_timestamp_nanos: u64,
    pub start_page_at: Option<Vec<u8>>,
    pub pages_processed: u32,
    pub neurons_processed: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnsRewardsState {
    pub config: Config,
    pub next_snapshot_id: u64,
    pub active_snapshot: Option<OwnerSnapshot>,
    pub scan: Option<OwnerScan>,
    pub last_scan_started_at_timestamp_nanos: u64,
    pub scan_lock_state_ts: Option<u64>,
}

impl SnsRewardsState {
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            next_snapshot_id: 1,
            active_snapshot: None,
            scan: None,
            last_scan_started_at_timestamp_nanos: 0,
            scan_lock_state_ts: Some(0),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
enum VersionedStableState {
    Uninitialized,
    V1(SnsRewardsState),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("encode SNS rewards stable state"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("encode SNS rewards stable state")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("decode SNS rewards stable state")
    }
    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct AccountIdentifierKey([u8; 32]);

impl Storable for AccountIdentifierKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(
            bytes
                .as_ref()
                .try_into()
                .expect("32-byte account identifier"),
        )
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: true,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrincipalValue(Principal);

impl Storable for PrincipalValue {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.0.as_slice())
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0.as_slice().to_vec()
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(Principal::from_slice(bytes.as_ref()))
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 29,
        is_fixed_size: false,
    };
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        const { std::cell::RefCell::new(None) };
    static OWNERS_A: std::cell::RefCell<Option<StableBTreeMap<AccountIdentifierKey, PrincipalValue, Memory>>> =
        const { std::cell::RefCell::new(None) };
    static OWNERS_B: std::cell::RefCell<Option<StableBTreeMap<AccountIdentifierKey, PrincipalValue, Memory>>> =
        const { std::cell::RefCell::new(None) };
    static STATE: std::cell::RefCell<Option<SnsRewardsState>> = const { std::cell::RefCell::new(None) };
}

fn with_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_STATE.with(|cell| {
        if cell.borrow().is_none() {
            let memory = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(METADATA_MEMORY_ID)));
            *cell.borrow_mut() = Some(StableCell::init(
                memory,
                VersionedStableState::Uninitialized,
            ));
        }
        f(cell.borrow_mut().as_mut().expect("SNS rewards stable cell"))
    })
}

fn with_map<R>(
    slot: OwnerIndexSlot,
    f: impl FnOnce(&mut StableBTreeMap<AccountIdentifierKey, PrincipalValue, Memory>) -> R,
) -> R {
    let (storage, memory_id) = match slot {
        OwnerIndexSlot::A => (&OWNERS_A, OWNER_INDEX_A_MEMORY_ID),
        OwnerIndexSlot::B => (&OWNERS_B, OWNER_INDEX_B_MEMORY_ID),
    };
    storage.with(|map| {
        if map.borrow().is_none() {
            let memory = MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(memory_id)));
            *map.borrow_mut() = Some(StableBTreeMap::init(memory));
        }
        f(map.borrow_mut().as_mut().expect("SNS rewards owner map"))
    })
}

fn persist(st: &SnsRewardsState) {
    with_stable_cell(|cell| cell.set(VersionedStableState::V1(st.clone())));
}

pub(crate) fn initialize(config: Config) {
    set_state(SnsRewardsState::new(config));
}

pub(crate) fn restore() -> Option<SnsRewardsState> {
    with_stable_cell(|cell| match cell.get().clone() {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::V1(st) => Some(st),
    })
}

pub(crate) fn set_state(st: SnsRewardsState) {
    persist(&st);
    STATE.with(|state| *state.borrow_mut() = Some(st));
}

pub(crate) fn with_state<R>(f: impl FnOnce(&SnsRewardsState) -> R) -> R {
    STATE.with(|state| f(state.borrow().as_ref().expect("SNS rewards state")))
}

pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut SnsRewardsState) -> R) -> R {
    STATE.with(|state| {
        let mut borrow = state.borrow_mut();
        let st = borrow.as_mut().expect("SNS rewards state");
        let result = f(st);
        persist(st);
        result
    })
}

pub(crate) fn clear_slot(slot: OwnerIndexSlot) {
    with_map(slot, |map| map.clear_new());
}

pub(crate) fn clear_all_owners() {
    clear_slot(OwnerIndexSlot::A);
    clear_slot(OwnerIndexSlot::B);
}

pub(crate) fn insert_owner(slot: OwnerIndexSlot, owner: Principal) {
    let key = AccountIdentifierKey(account_identifier_bytes(owner, None));
    with_map(slot, |map| {
        map.insert(key, PrincipalValue(owner));
    });
}

pub(crate) fn lookup(slot: OwnerIndexSlot, account_identifier: [u8; 32]) -> Option<Principal> {
    with_map(slot, |map| {
        map.get(&AccountIdentifierKey(account_identifier))
            .map(|value| value.0)
    })
}

pub(crate) fn slot_len(slot: OwnerIndexSlot) -> u64 {
    with_map(slot, |map| map.len())
}

pub(crate) fn invalidate_for_root(root: Option<Principal>) {
    clear_all_owners();
    with_state_mut(|st| {
        st.config.reward_sns_root_canister_id = root;
        st.active_snapshot = None;
        st.scan = None;
        st.last_scan_started_at_timestamp_nanos = 0;
        st.scan_lock_state_ts = Some(0);
    });
}

#[cfg(test)]
pub(crate) fn reset_for_test(config: Config) {
    clear_all_owners();
    initialize(config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_metadata_roundtrips_active_snapshot_and_scan_cursor() {
        let root = Principal::from_slice(&[1]);
        let governance = Principal::from_slice(&[2]);
        let ledger = Principal::from_slice(&[3]);
        let mut state = SnsRewardsState::new(Config {
            reward_sns_root_canister_id: Some(root),
        });
        state.next_snapshot_id = 8;
        state.active_snapshot = Some(OwnerSnapshot {
            snapshot_id: 7,
            active_slot: OwnerIndexSlot::A,
            sns_root_canister_id: root,
            sns_governance_canister_id: governance,
            sns_ledger_canister_id: ledger,
            scan_started_at_timestamp_nanos: 10,
            scan_completed_at_timestamp_nanos: 20,
            neuron_count: 30,
            owner_count: 4,
        });
        state.scan = Some(OwnerScan {
            staging_slot: OwnerIndexSlot::B,
            sns_root_canister_id: root,
            sns_governance_canister_id: governance,
            sns_ledger_canister_id: ledger,
            scan_started_at_timestamp_nanos: 30,
            start_page_at: Some(vec![9; 32]),
            pages_processed: 2,
            neurons_processed: 200,
        });
        let bytes = candid::encode_one(VersionedStableState::V1(state.clone())).unwrap();
        let decoded: VersionedStableState = candid::decode_one(&bytes).unwrap();
        match decoded {
            VersionedStableState::V1(decoded) => assert_eq!(decoded, state),
            VersionedStableState::Uninitialized => panic!("unexpected uninitialized metadata"),
        }
    }

    #[test]
    fn uninitialized_version_supports_upgrade_from_empty_placeholder() {
        let bytes = candid::encode_one(VersionedStableState::Uninitialized).unwrap();
        let decoded: VersionedStableState = candid::decode_one(&bytes).unwrap();
        assert!(matches!(decoded, VersionedStableState::Uninitialized));
    }
}
