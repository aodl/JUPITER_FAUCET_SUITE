use candid::{
    types::value::{IDLArgs, IDLValue},
    CandidType, Deserialize, Principal,
};
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

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub(crate) struct PlannedTransfer {
    pub to: Account,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub created_at_time_nanos: u64,
    pub memo: Vec<u8>,
    pub status: TransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
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

fn idl_value_contains_field(value: &IDLValue, field_id: u32) -> bool {
    match value {
        IDLValue::Opt(value) => idl_value_contains_field(value, field_id),
        IDLValue::Vec(values) => values
            .iter()
            .any(|value| idl_value_contains_field(value, field_id)),
        IDLValue::Record(fields) => fields.iter().any(|field| {
            field.id.get_id() == field_id || idl_value_contains_field(&field.val, field_id)
        }),
        IDLValue::Variant(value) => {
            value.0.id.get_id() == field_id || idl_value_contains_field(&value.0.val, field_id)
        }
        _ => false,
    }
}

// Upgrade-compatibility boundary only: retired blackhole-policy fields identify an old
// stable record. Durable payout/failure evidence decodes through the current shape, while
// the old controller-authority fields and reconciliation latch are deliberately neutralized.
fn decode_versioned_stable_state(bytes: &[u8]) -> candid::Result<VersionedStableState> {
    let idl = IDLArgs::from_bytes(bytes)?;
    let legacy_controller_state = [
        "blackhole_controller",
        "blackhole_armed",
        "blackhole_armed_since_ts",
    ]
    .iter()
    .map(|name| candid::idl_hash(name))
    .any(|field_id| {
        idl.args
            .iter()
            .any(|value| idl_value_contains_field(value, field_id))
    });

    let mut decoded = candid::decode_one::<VersionedStableState>(bytes)?;
    if legacy_controller_state {
        if let VersionedStableState::V1(state) = &mut decoded {
            state.config.autonomous_rescue_armed = None;
            state.autonomous_rescue_armed_since_ts = None;
            state.rescue_triggered = false;
        }
    }
    Ok(decoded)
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

    // Exact pre-change wire fixture. Ordinary typed Candid evolution ignores retired fields
    // and supplies `None` for the new optional fields. The production compatibility decoder
    // additionally detects this legacy wire shape so retired blackhole arming, its timestamp,
    // and the controller-reconciliation latch are explicitly neutralized.
    #[derive(CandidType)]
    struct FrozenControllerConfig {
        neuron_id: u64,
        normal_recipient: Account,
        age_bonus_recipient_1: Account,
        age_bonus_recipient_2: Account,
        ledger_canister_id: Principal,
        governance_canister_id: Principal,
        rescue_controller: Principal,
        blackhole_controller: Option<Principal>,
        blackhole_armed: Option<bool>,
        main_interval_seconds: u64,
        rescue_interval_seconds: u64,
    }

    #[derive(CandidType)]
    struct FrozenControllerState {
        config: FrozenControllerConfig,
        prev_age_seconds: u64,
        last_successful_transfer_ts: Option<u64>,
        last_rescue_check_ts: u64,
        rescue_triggered: bool,
        blackhole_armed_since_ts: Option<u64>,
        forced_rescue_reason: Option<ForcedRescueReason>,
        main_lock_state_ts: Option<u64>,
        payout_nonce: u64,
        payout_plan: Option<PayoutPlan>,
        last_main_run_ts: u64,
    }

    #[allow(clippy::large_enum_variant)]
    #[allow(dead_code)]
    #[derive(CandidType)]
    enum FrozenControllerVersionedStableState {
        Uninitialized,
        V1(FrozenControllerState),
    }

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

    fn frozen_controller_config(blackhole_armed: Option<bool>) -> FrozenControllerConfig {
        let current = sample_config();
        FrozenControllerConfig {
            neuron_id: current.neuron_id,
            normal_recipient: current.normal_recipient,
            age_bonus_recipient_1: current.age_bonus_recipient_1,
            age_bonus_recipient_2: current.age_bonus_recipient_2,
            ledger_canister_id: current.ledger_canister_id,
            governance_canister_id: current.governance_canister_id,
            rescue_controller: current.rescue_controller,
            blackhole_controller: Some(principal(&[7])),
            blackhole_armed,
            main_interval_seconds: current.main_interval_seconds,
            rescue_interval_seconds: current.rescue_interval_seconds,
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
    fn set_state_round_trips_through_stable_storage() {
        reset_test_storage();
        let mut st = State::new(sample_config(), 3_000);
        st.prev_age_seconds = 123;
        st.main_lock_state_ts = Some(44);
        st.config.autonomous_rescue_armed = Some(true);
        st.rescue_triggered = true;
        st.autonomous_rescue_armed_since_ts = Some(2_999);
        set_state(st.clone());

        let restored = restore_state_from_stable().expect("expected persisted disburser state");
        assert_eq!(restored.prev_age_seconds, 123);
        assert_eq!(restored.main_lock_state_ts, Some(44));
        assert_eq!(restored.payout_nonce, st.payout_nonce);
        assert_eq!(restored.config.autonomous_rescue_armed, Some(true));
        assert!(restored.rescue_triggered);
        assert_eq!(restored.autonomous_rescue_armed_since_ts, Some(2_999));
    }

    #[test]
    fn deployed_unarmed_controller_state_upgrades_without_restoring_blackhole_policy() {
        let payout_plan = PayoutPlan {
            id: 41,
            fee_e8s: 10_000,
            created_at_base_nanos: 5_000,
            transfers: vec![
                PlannedTransfer {
                    to: sample_config().normal_recipient,
                    gross_share_e8s: 600_000_000,
                    amount_e8s: 599_990_000,
                    created_at_time_nanos: 5_001,
                    memo: vec![1, 2, 3],
                    status: TransferStatus::Sent {
                        block_index: "123".to_string(),
                    },
                },
                PlannedTransfer {
                    to: sample_config().age_bonus_recipient_1,
                    gross_share_e8s: 200_000_000,
                    amount_e8s: 199_990_000,
                    created_at_time_nanos: 5_002,
                    memo: vec![4, 5, 6],
                    status: TransferStatus::Pending,
                },
            ],
        };
        let legacy = FrozenControllerVersionedStableState::V1(FrozenControllerState {
            config: frozen_controller_config(Some(false)),
            prev_age_seconds: 31_536_000,
            last_successful_transfer_ts: Some(1_000),
            last_rescue_check_ts: 2_000,
            rescue_triggered: true,
            blackhole_armed_since_ts: None,
            forced_rescue_reason: Some(ForcedRescueReason::BootstrapNoSuccess),
            main_lock_state_ts: Some(2_001),
            payout_nonce: 42,
            payout_plan: Some(payout_plan),
            last_main_run_ts: 2_002,
        });
        let bytes = candid::encode_one(legacy).expect("encode deployed disburser controller state");

        let VersionedStableState::V1(restored) = decode_versioned_stable_state(&bytes)
            .expect("decode deployed disburser controller state")
        else {
            panic!("expected restored V1 disburser state");
        };

        assert_eq!(restored.config.autonomous_rescue_armed, None);
        assert_eq!(restored.autonomous_rescue_armed_since_ts, None);
        assert!(!restored.rescue_triggered);
        assert_eq!(restored.prev_age_seconds, 31_536_000);
        assert_eq!(restored.payout_nonce, 42);
        assert_eq!(
            restored.forced_rescue_reason,
            Some(ForcedRescueReason::BootstrapNoSuccess)
        );
        let restored_plan = restored.payout_plan.expect("payout plan should survive");
        assert_eq!(restored_plan.id, 41);
        assert_eq!(restored_plan.transfers.len(), 2);
        assert_eq!(
            restored_plan.transfers[0].status,
            TransferStatus::Sent {
                block_index: "123".to_string()
            }
        );
        assert_eq!(restored_plan.transfers[1].status, TransferStatus::Pending);

        let blackhole = principal(&[7]);
        let self_id = principal(&[10]);
        let desired = crate::policy::desired_controllers(
            1_000 + 15 * 86_400,
            restored.last_successful_transfer_ts,
            self_id,
            restored.config.rescue_controller,
        )
        .expect("broken state should request recovery controllers");
        assert_eq!(desired, vec![restored.config.rescue_controller, self_id]);
        assert!(!desired.contains(&blackhole));
    }

    #[test]
    fn deployed_armed_controller_state_cannot_authorize_autonomous_rescue_after_upgrade() {
        let payout_plan = PayoutPlan {
            id: 51,
            fee_e8s: 10_000,
            created_at_base_nanos: 6_000,
            transfers: vec![
                PlannedTransfer {
                    to: sample_config().normal_recipient,
                    gross_share_e8s: 700_000_000,
                    amount_e8s: 699_990_000,
                    created_at_time_nanos: 6_001,
                    memo: vec![7, 8, 9],
                    status: TransferStatus::Sent {
                        block_index: "456".to_string(),
                    },
                },
                PlannedTransfer {
                    to: sample_config().age_bonus_recipient_1,
                    gross_share_e8s: 300_000_000,
                    amount_e8s: 299_990_000,
                    created_at_time_nanos: 6_002,
                    memo: vec![10, 11, 12],
                    status: TransferStatus::Pending,
                },
            ],
        };
        let legacy = FrozenControllerVersionedStableState::V1(FrozenControllerState {
            config: frozen_controller_config(Some(true)),
            prev_age_seconds: 63_072_000,
            last_successful_transfer_ts: Some(1_000),
            last_rescue_check_ts: 2_000,
            rescue_triggered: true,
            blackhole_armed_since_ts: Some(500),
            forced_rescue_reason: Some(ForcedRescueReason::BootstrapNoSuccess),
            main_lock_state_ts: Some(2_001),
            payout_nonce: 52,
            payout_plan: Some(payout_plan),
            last_main_run_ts: 2_002,
        });
        let bytes = candid::encode_one(legacy).expect("encode armed legacy disburser state");

        let VersionedStableState::V1(mut restored) =
            VersionedStableState::from_bytes(Cow::Owned(bytes))
        else {
            panic!("expected restored V1 disburser state");
        };

        assert_eq!(restored.config.autonomous_rescue_armed, None);
        assert_eq!(restored.autonomous_rescue_armed_since_ts, None);
        assert!(!restored.rescue_triggered);
        assert_eq!(restored.prev_age_seconds, 63_072_000);
        assert_eq!(restored.payout_nonce, 52);
        assert_eq!(
            restored.forced_rescue_reason,
            Some(ForcedRescueReason::BootstrapNoSuccess)
        );
        let restored_plan = restored
            .payout_plan
            .as_ref()
            .expect("payout plan should survive");
        assert_eq!(restored_plan.id, 51);
        assert_eq!(restored_plan.transfers.len(), 2);
        assert_eq!(
            restored_plan.transfers[0].status,
            TransferStatus::Sent {
                block_index: "456".to_string()
            }
        );
        assert_eq!(restored_plan.transfers[1].status, TransferStatus::Pending);

        let actions = crate::apply_upgrade_args_to_state(&mut restored, None, 3_000);
        assert_eq!(actions, crate::PostUpgradeActions::default());

        let blackhole = principal(&[7]);
        let self_id = principal(&[10]);
        let desired = crate::policy::desired_controllers(
            1_000 + 15 * 86_400,
            restored.last_successful_transfer_ts,
            self_id,
            restored.config.rescue_controller,
        )
        .expect("broken state should request recovery controllers");
        assert_eq!(desired, vec![restored.config.rescue_controller, self_id]);
        assert!(!desired.contains(&blackhole));
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
