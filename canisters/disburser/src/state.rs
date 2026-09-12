use candid::{CandidType, Deserialize, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::account::account_text;
use serde::Serialize;
use std::borrow::Cow;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) struct Config {
    pub neuron_id: u64,

    pub normal_recipient: Account,
    pub age_bonus_recipient_1: Account,
    pub age_bonus_recipient_2: Account,

    pub ledger_canister_id: Principal,
    pub governance_canister_id: Principal,

    pub rescue_controller: Principal,
    pub autonomous_rescue_armed: Option<bool>,

    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
}

fn opt_bool_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "none",
    }
}

pub(crate) fn runtime_config_log_line(cfg: &Config) -> String {
    format!(
        "CONFIG neuron_id={}, normal_recipient={}, age_bonus_recipient_1={}, age_bonus_recipient_2={}, ledger_canister_id={}, governance_canister_id={}, rescue_controller={}, autonomous_rescue_armed={}, main_interval_seconds={}, rescue_interval_seconds={}",
        cfg.neuron_id,
        account_text(&cfg.normal_recipient),
        account_text(&cfg.age_bonus_recipient_1),
        account_text(&cfg.age_bonus_recipient_2),
        cfg.ledger_canister_id.to_text(),
        cfg.governance_canister_id.to_text(),
        cfg.rescue_controller.to_text(),
        opt_bool_text(cfg.autonomous_rescue_armed),
        cfg.main_interval_seconds,
        cfg.rescue_interval_seconds
    )
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransferStatus {
    Pending,
    Sent { block_index: String },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedTransfer {
    pub to: Account,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub created_at_time_nanos: u64,
    pub memo: Vec<u8>,
    pub status: TransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PayoutPlan {
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
pub(crate) struct State {
    pub config: Config,
    pub prev_age_seconds: u64,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub autonomous_rescue_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
    pub main_lock_state_ts: Option<u64>,
    pub payout_nonce: u64,
    pub payout_plan: Option<PayoutPlan>,
    pub last_main_run_ts: u64,
}

impl State {
    pub(crate) fn new(config: Config, now_secs: u64) -> Self {
        let autonomous_rescue_armed_since_ts = config
            .autonomous_rescue_armed
            .unwrap_or(false)
            .then_some(now_secs);
        Self {
            config,
            prev_age_seconds: 0,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            autonomous_rescue_armed_since_ts,
            forced_rescue_reason: None,
            main_lock_state_ts: Some(0),
            payout_nonce: 1,
            payout_plan: None,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
        }
    }
}

// Stable-state enum shape is part of the upgrade contract; boxing V1 would change Candid.
#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum VersionedStableState {
    Uninitialized,
    V1(State),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode disburser stable state"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode disburser stable state")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        decode_versioned_stable_state(bytes.as_ref())
            .expect("failed to decode disburser stable state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

fn decode_versioned_stable_state(bytes: &[u8]) -> candid::Result<VersionedStableState> {
    let current = candid::decode_one::<VersionedStableState>(bytes)?;
    // Candid width compatibility can discard fields. Accept only the stable cell's
    // canonical current encoding so decoding cannot silently coerce another schema.
    if candid::encode_one(&current)? != bytes {
        return Err(candid::Error::msg(
            "current disburser stable state did not decode losslessly",
        ));
    }
    Ok(current)
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        const { std::cell::RefCell::new(None) };
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
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
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized);
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow
            .as_mut()
            .expect("disburser stable cell not initialized"))
    })
}

fn persist_snapshot(st: &State) {
    with_stable_cell(|cell| {
        cell.set(VersionedStableState::V1(st.clone()));
    });
}

pub(crate) fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub(crate) fn restore_state_from_stable() -> Option<State> {
    with_stable_cell(|cell| match cell.get().clone() {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::V1(st) => Some(st),
    })
}

pub(crate) fn set_state(st: State) {
    persist_snapshot(&st);
    clear_persistence_dirty();
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

#[cfg(any(test, feature = "debug_api"))]
pub(crate) fn get_state() -> State {
    STATE
        .with(|s| s.borrow().clone())
        .expect("state not initialized")
}

pub(crate) fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

#[cfg(test)]
fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| jupiter_persistence_batch::is_active(depth.get()))
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
pub(crate) fn persist_dirty_state() {
    let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
    if !dirty {
        return;
    }
    let snapshot = get_state();
    persist_snapshot(&snapshot);
    clear_persistence_dirty();
}

#[cfg(test)]
pub(crate) type PersistenceBatch = jupiter_persistence_batch::PersistenceBatch;

#[cfg(test)]
#[must_use]
pub(crate) fn begin_persistence_batch() -> PersistenceBatch {
    PERSISTENCE_BATCH_DEPTH
        .with(|depth| depth.set(jupiter_persistence_batch::begin_depth(depth.get())));
    PersistenceBatch::new(|| {
        let should_flush = PERSISTENCE_BATCH_DEPTH.with(|depth| {
            let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
            let (next_depth, should_flush) =
                jupiter_persistence_batch::finish_depth(depth.get(), dirty);
            depth.set(next_depth);
            should_flush
        });
        if should_flush {
            persist_dirty_state();
        }
    })
}

pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
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
            cell.set(VersionedStableState::Uninitialized);
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
            normal_recipient: Account {
                owner: principal(&[1]),
                subaccount: None,
            },
            age_bonus_recipient_1: Account {
                owner: principal(&[2]),
                subaccount: None,
            },
            age_bonus_recipient_2: Account {
                owner: principal(&[3]),
                subaccount: None,
            },
            ledger_canister_id: principal(&[14]),
            governance_canister_id: principal(&[5]),
            rescue_controller: principal(&[6]),
            autonomous_rescue_armed: Some(false),
            main_interval_seconds: 60,
            rescue_interval_seconds: 120,
        }
    }

    #[test]
    fn runtime_config_log_line_includes_all_config_fields() {
        let line = runtime_config_log_line(&sample_config());
        assert!(line.starts_with("CONFIG "));
        assert!(line.contains("neuron_id=42"));
        assert!(line.contains("normal_recipient="));
        assert!(line.contains("age_bonus_recipient_1="));
        assert!(line.contains("age_bonus_recipient_2="));
        assert!(line.contains("ledger_canister_id="));
        assert!(line.contains("governance_canister_id="));
        assert!(line.contains("rescue_controller="));
        assert!(line.contains("autonomous_rescue_armed=false"));
        assert!(line.contains("main_interval_seconds=60"));
        assert!(line.contains("rescue_interval_seconds=120"));
    }

    #[test]
    fn stable_restore_is_none_before_first_persist() {
        reset_test_storage();
        assert!(restore_state_from_stable().is_none());
    }

    #[test]
    fn current_v1_state_round_trips_through_stable_storage() {
        reset_test_storage();
        let mut st = State::new(sample_config(), 3_000);
        st.prev_age_seconds = 123;
        st.last_successful_transfer_ts = Some(2_950);
        st.main_lock_state_ts = Some(44);
        st.payout_nonce = 17;
        st.payout_plan = Some(PayoutPlan {
            id: 16,
            fee_e8s: 10_000,
            created_at_base_nanos: 2_900_000_000_000,
            transfers: vec![
                PlannedTransfer {
                    to: Account {
                        owner: principal(&[21]),
                        subaccount: Some([3; 32]),
                    },
                    gross_share_e8s: 75_000_000,
                    amount_e8s: 74_990_000,
                    created_at_time_nanos: 2_900_000_000_001,
                    memo: vec![1, 2, 3],
                    status: TransferStatus::Sent {
                        block_index: "12345".to_string(),
                    },
                },
                PlannedTransfer {
                    to: Account {
                        owner: principal(&[22]),
                        subaccount: None,
                    },
                    gross_share_e8s: 25_000_000,
                    amount_e8s: 24_990_000,
                    created_at_time_nanos: 2_900_000_000_002,
                    memo: vec![4, 5, 6],
                    status: TransferStatus::Pending,
                },
            ],
        });
        st.config.autonomous_rescue_armed = Some(true);
        st.rescue_triggered = true;
        st.autonomous_rescue_armed_since_ts = Some(2_999);
        set_state(st.clone());

        let restored = restore_state_from_stable().expect("expected persisted disburser state");
        assert_eq!(restored.prev_age_seconds, 123);
        assert_eq!(restored.last_successful_transfer_ts, Some(2_950));
        assert_eq!(restored.main_lock_state_ts, Some(44));
        assert_eq!(restored.payout_nonce, 17);
        assert_eq!(restored.payout_plan, st.payout_plan);
        assert_eq!(restored.config.autonomous_rescue_armed, Some(true));
        assert!(restored.rescue_triggered);
        assert_eq!(restored.autonomous_rescue_armed_since_ts, Some(2_999));
    }

    #[test]
    fn non_current_width_compatible_stable_payload_is_rejected() {
        #[derive(CandidType)]
        enum NarrowStableState {
            V1(State),
        }

        let bytes = candid::encode_one(NarrowStableState::V1(State::new(sample_config(), 3_000)))
            .expect("encode narrower variant type");
        assert!(candid::decode_one::<VersionedStableState>(&bytes).is_ok());
        let error = decode_versioned_stable_state(&bytes)
            .err()
            .expect("non-current encoding must fail closed");
        assert!(error.to_string().contains("did not decode losslessly"));
    }

    #[test]
    fn with_state_mut_persists_updates_to_stable_storage() {
        reset_test_storage();
        set_state(State::new(sample_config(), 4_000));

        with_state_mut(|st| {
            st.last_successful_transfer_ts = Some(888);
            st.main_lock_state_ts = Some(55);
        });

        let restored =
            restore_state_from_stable().expect("expected persisted disburser state after mutation");
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
            let restored_mid = restore_state_from_stable()
                .expect("expected persisted state before batch mutation");
            assert_ne!(restored_mid.last_successful_transfer_ts, Some(999));
            assert_ne!(restored_mid.main_lock_state_ts, Some(77));
            persist_dirty_state();
        }

        let restored =
            restore_state_from_stable().expect("expected persisted state after batch flush");
        assert_eq!(restored.last_successful_transfer_ts, Some(999));
        assert_eq!(restored.main_lock_state_ts, Some(77));
    }
}
