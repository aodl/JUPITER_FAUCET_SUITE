use candid::{CandidType, Deserialize, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;
use std::borrow::Cow;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Config {
    pub neuron_id: u64,

    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,

    pub ledger_canister_id: Principal,
    pub governance_canister_id: Principal,

    pub rescue_controller: Principal,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,

    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Sent { block_index: String },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct PlannedTransfer {
    pub to: Account,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub created_at_time_nanos: u64,
    pub memo: Vec<u8>,
    pub status: TransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct PayoutPlan {
    pub id: u64,
    pub fee_e8s: u64,
    pub created_at_base_nanos: u64,
    pub transfers: Vec<PlannedTransfer>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum ForcedRescueReason {
    BootstrapNoSuccess,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct State {
    pub config: Config,
    pub prev_age_seconds: u64,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub blackhole_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
    pub main_lock_state_ts: Option<u64>,
    pub payout_nonce: u64,
    pub payout_plan: Option<PayoutPlan>,
    pub last_main_run_ts: u64,
}

impl State {
    pub fn new(config: Config, now_secs: u64) -> Self {
        let blackhole_armed_since_ts = config.blackhole_armed.unwrap_or(false).then_some(now_secs);
        Self {
            config,
            prev_age_seconds: 0,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            blackhole_armed_since_ts,
            forced_rescue_reason: None,
            main_lock_state_ts: Some(0),
            payout_nonce: 1,
            payout_plan: None,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub enum VersionedStableState {
    Uninitialized,
    V1(State),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode disburser stable state"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode disburser stable state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        std::cell::RefCell::new(None);
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
    #[cfg(test)]
    static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    #[cfg(test)]
    static PERSISTENCE_DIRTY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn with_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized)
                    .expect("failed to initialize disburser stable cell");
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("disburser stable cell not initialized"))
    })
}

fn persist_snapshot(st: &State) {
    with_stable_cell(|cell| {
        cell.set(VersionedStableState::V1(st.clone()))
            .expect("failed to persist disburser stable state");
    });
}

pub fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub fn restore_state_from_stable() -> Option<State> {
    with_stable_cell(|cell| match cell.get().clone() {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::V1(st) => Some(st),
    })
}

pub fn set_state(st: State) {
    persist_snapshot(&st);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub fn get_state() -> State {
    STATE.with(|s| s.borrow().clone()).expect("state not initialized")
}

pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

#[cfg(test)]
fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.get() > 0)
}

#[cfg(not(test))]
fn persistence_batch_active() -> bool {
    false
}

#[cfg(test)]
fn mark_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(true));
}

#[cfg(not(test))]
fn mark_persistence_dirty() {}

#[cfg(test)]
fn clear_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
}

#[cfg(not(test))]
fn clear_persistence_dirty() {}

#[cfg(test)]
pub fn persist_dirty_state() {
    let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
    if !dirty {
        return;
    }
    let snapshot = get_state();
    persist_snapshot(&snapshot);
    clear_persistence_dirty();
}

#[cfg(test)]
/// A synchronous persistence-batch guard.
///
/// Do not hold this guard across an `await` point. While it is live, mutations are
/// only marked dirty and are not durably flushed until the batch ends or an
/// explicit `persist_dirty_state()` call occurs.
pub struct PersistenceBatch {
    active: bool,
}

#[cfg(test)]
impl Drop for PersistenceBatch {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let should_flush = PERSISTENCE_BATCH_DEPTH.with(|depth| {
            let current = depth.get();
            assert!(current > 0, "persistence batch depth underflow");
            depth.set(current - 1);
            current == 1
        });
        if should_flush {
            persist_dirty_state();
        }
        self.active = false;
    }
}

#[cfg(test)]
#[must_use]
pub fn begin_persistence_batch() -> PersistenceBatch {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    PersistenceBatch { active: true }
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        let immediate_persist = !persistence_batch_active();
        let out = f(st);
        if immediate_persist {
            let snapshot = st.clone();
            drop(borrow);
            persist_snapshot(&snapshot);
            return out;
        }
        mark_persistence_dirty();
        drop(borrow);
        out
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    fn reset_test_storage() {
        with_stable_cell(|cell| {
            cell.set(VersionedStableState::Uninitialized)
                .expect("failed to reset disburser stable state for test");
        });
        PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(0));
        PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn principal(bytes: &[u8]) -> Principal {
        Principal::from_slice(bytes)
    }

    fn sample_config() -> Config {
        Config {
            neuron_id: 42,
            normal_recipient: Account { owner: principal(&[1]), subaccount: None },
            age_bonus_recipient_1: Account { owner: principal(&[2]), subaccount: None },
            age_bonus_recipient_2: Account { owner: principal(&[3]), subaccount: None },
            ledger_canister_id: principal(&[4]),
            governance_canister_id: principal(&[5]),
            rescue_controller: principal(&[6]),
            blackhole_controller: Some(principal(&[7])),
            blackhole_armed: Some(false),
            main_interval_seconds: 60,
            rescue_interval_seconds: 120,
        }
    }

    #[test]
    fn stable_restore_is_none_before_first_persist() {
        reset_test_storage();
        assert!(restore_state_from_stable().is_none());
    }

    #[test]
    fn set_state_round_trips_through_stable_storage() {
        reset_test_storage();
        let mut st = State::new(sample_config(), 3_000);
        st.prev_age_seconds = 123;
        st.main_lock_state_ts = Some(44);
        set_state(st.clone());

        let restored = restore_state_from_stable().expect("expected persisted disburser state");
        assert_eq!(restored.prev_age_seconds, 123);
        assert_eq!(restored.main_lock_state_ts, Some(44));
        assert_eq!(restored.payout_nonce, st.payout_nonce);
    }

    #[test]
    fn with_state_mut_persists_updates_to_stable_storage() {
        reset_test_storage();
        set_state(State::new(sample_config(), 4_000));

        with_state_mut(|st| {
            st.last_successful_transfer_ts = Some(888);
            st.main_lock_state_ts = Some(55);
        });

        let restored = restore_state_from_stable().expect("expected persisted disburser state after mutation");
        assert_eq!(restored.last_successful_transfer_ts, Some(888));
        assert_eq!(restored.main_lock_state_ts, Some(55));
    }

    #[test]
    fn persistence_batch_defers_writes_until_flush_boundary() {
        reset_test_storage();
        set_state(State::new(sample_config(), 5_000));

        {
            let _batch = begin_persistence_batch();
            with_state_mut(|st| {
                st.last_successful_transfer_ts = Some(999);
                st.main_lock_state_ts = Some(77);
            });
            let restored_mid = restore_state_from_stable().expect("expected persisted state before batch mutation");
            assert_ne!(restored_mid.last_successful_transfer_ts, Some(999));
            assert_ne!(restored_mid.main_lock_state_ts, Some(77));
            persist_dirty_state();
        }

        let restored = restore_state_from_stable().expect("expected persisted state after batch flush");
        assert_eq!(restored.last_successful_transfer_ts, Some(999));
        assert_eq!(restored.main_lock_state_ts, Some(77));
    }
}
