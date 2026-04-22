use candid::{CandidType, Deserialize, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableBTreeMap, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;
use std::borrow::Cow;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Config {
    pub staking_account: Account,
    pub payout_subaccount: Option<[u8; 32]>,
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub cmc_canister_id: Principal,
    pub rescue_controller: Principal,
    pub blackhole_controller: Option<Principal>,
    pub blackhole_armed: Option<bool>,
    pub expected_first_staking_tx_id: Option<u64>,
    pub main_interval_seconds: u64,
    pub rescue_interval_seconds: u64,
    pub min_tx_e8s: u64,
    #[serde(default)]
    pub stake_recognition_delay_seconds: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Beneficiary,
    RemainderToSelf,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingNotification {
    pub kind: TransferKind,
    pub beneficiary: Principal,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub block_index: u64,
    pub next_start: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum PendingTransferPhase {
    AwaitingTransfer,
    TransferAccepted,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingTransfer {
    pub notification: PendingNotification,
    pub created_at_time_nanos: u64,
    pub phase: PendingTransferPhase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkipRange {
    pub start_tx_id: u64,
    pub end_tx_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct U64Key(u64);

impl U64Key {
    fn get(&self) -> u64 {
        self.0
    }
}

impl From<u64> for U64Key {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Storable for U64Key {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.to_be_bytes().to_vec())
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let slice = bytes.as_ref();
        assert_eq!(slice.len(), 8, "invalid faucet u64 key length");
        let mut raw = [0u8; 8];
        raw.copy_from_slice(slice);
        Self(u64::from_be_bytes(raw))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 8,
        is_fixed_size: true,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct U64Value(u64);

impl U64Value {
    fn get(&self) -> u64 {
        self.0
    }
}

impl From<u64> for U64Value {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Storable for U64Value {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.to_be_bytes().to_vec())
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let slice = bytes.as_ref();
        assert_eq!(slice.len(), 8, "invalid faucet u64 value length");
        let mut raw = [0u8; 8];
        raw.copy_from_slice(slice);
        Self(u64::from_be_bytes(raw))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 8,
        is_fixed_size: true,
    };
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum ForcedRescueReason {
    BootstrapNoSuccess,
    IndexAnchorMissing,
    IndexLatestInvariantBroken,
    IndexLatestUnreadable,
    CmcZeroSuccessRuns,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipRangeInsertError {
    InvalidRange,
    DuplicateStart,
    OverlapsOrAbutsPredecessor,
    OverlapsOrAbutsSuccessor,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub pot_start_e8s: u64,
    pub pot_remaining_e8s: u64,
    pub denom_staking_balance_e8s: u64,
    #[serde(default)]
    pub effective_denom_staking_balance_e8s: Option<u64>,
    pub topped_up_count: u64,
    pub topped_up_sum_e8s: u64,
    pub topped_up_min_e8s: Option<u64>,
    pub topped_up_max_e8s: Option<u64>,
    pub failed_topups: u64,
    #[serde(default)]
    pub ambiguous_topups: u64,
    pub ignored_under_threshold: u64,
    pub ignored_bad_memo: u64,
    pub remainder_to_self_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct ActivePayoutJob {
    pub id: u64,
    pub fee_e8s: u64,
    pub pot_start_e8s: u64,
    pub denom_staking_balance_e8s: u64,
    pub next_start: Option<u64>,
    pub scan_complete: bool,
    pub ignored_under_threshold: u64,
    pub ignored_bad_memo: u64,
    pub gross_outflow_e8s: u64,
    pub topped_up_count: u64,
    pub topped_up_sum_e8s: u64,
    pub topped_up_min_e8s: Option<u64>,
    pub topped_up_max_e8s: Option<u64>,
    pub failed_topups: u64,
    #[serde(default)]
    pub ambiguous_topups: u64,
    pub remainder_to_self_e8s: u64,
    #[serde(default)]
    pub pending_transfer: Option<PendingTransfer>,
    #[serde(default)]
    pub skip_candidate_start_tx_id: Option<u64>,
    #[serde(default)]
    pub skip_candidate_end_tx_id: Option<u64>,
    #[serde(default)]
    pub skip_candidate_tx_count: u64,
    pub next_created_at_time_nanos: u64,
    pub observed_oldest_tx_id: Option<u64>,
    pub observed_latest_tx_id: Option<u64>,
    pub cmc_attempt_count: Option<u64>,
    pub cmc_success_count: Option<u64>,
    #[serde(default)]
    pub cmc_attempted_beneficiaries: Option<Vec<Principal>>,
    #[serde(default)]
    pub round_start_time_nanos: Option<u64>,
    #[serde(default)]
    pub round_start_staking_balance_e8s: Option<u64>,
    #[serde(default)]
    pub round_start_latest_tx_id: Option<u64>,
    #[serde(default)]
    pub round_end_time_nanos: Option<u64>,
    #[serde(default)]
    pub round_end_latest_tx_id: Option<u64>,
    #[serde(default)]
    pub effective_denom_staking_balance_e8s: Option<u64>,
    #[serde(default)]
    pub effective_denom_scan_complete: Option<bool>,
}

impl ActivePayoutJob {
    pub fn new(id: u64, fee_e8s: u64, pot_start_e8s: u64, denom_staking_balance_e8s: u64, created_at_time_nanos: u64) -> Self {
        Self {
            id,
            fee_e8s,
            pot_start_e8s,
            denom_staking_balance_e8s,
            next_start: None,
            scan_complete: false,
            ignored_under_threshold: 0,
            ignored_bad_memo: 0,
            gross_outflow_e8s: 0,
            topped_up_count: 0,
            topped_up_sum_e8s: 0,
            topped_up_min_e8s: None,
            topped_up_max_e8s: None,
            failed_topups: 0,
            ambiguous_topups: 0,
            remainder_to_self_e8s: 0,
            pending_transfer: None,
            skip_candidate_start_tx_id: None,
            skip_candidate_end_tx_id: None,
            skip_candidate_tx_count: 0,
            next_created_at_time_nanos: created_at_time_nanos,
            observed_oldest_tx_id: None,
            observed_latest_tx_id: None,
            cmc_attempt_count: Some(0),
            cmc_success_count: Some(0),
            cmc_attempted_beneficiaries: Some(Vec::new()),
            round_start_time_nanos: None,
            round_start_staking_balance_e8s: None,
            round_start_latest_tx_id: None,
            round_end_time_nanos: None,
            round_end_latest_tx_id: None,
            effective_denom_staking_balance_e8s: None,
            effective_denom_scan_complete: None,
        }
    }

    pub fn configure_round_accounting(
        &mut self,
        round_start_time_nanos: Option<u64>,
        round_start_staking_balance_e8s: Option<u64>,
        round_start_latest_tx_id: Option<u64>,
        round_end_time_nanos: u64,
        round_end_latest_tx_id: Option<u64>,
        effective_denom_staking_balance_e8s: u64,
        effective_denom_scan_complete: bool,
    ) {
        self.round_start_time_nanos = round_start_time_nanos;
        self.round_start_staking_balance_e8s = round_start_staking_balance_e8s;
        self.round_start_latest_tx_id = round_start_latest_tx_id;
        self.round_end_time_nanos = Some(round_end_time_nanos);
        self.round_end_latest_tx_id = round_end_latest_tx_id;
        self.effective_denom_staking_balance_e8s = Some(effective_denom_staking_balance_e8s);
        self.effective_denom_scan_complete = Some(effective_denom_scan_complete);
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct State {
    pub config: Config,
    pub last_summary: Option<Summary>,
    pub last_successful_transfer_ts: Option<u64>,
    pub last_rescue_check_ts: u64,
    pub rescue_triggered: bool,
    pub blackhole_armed_since_ts: Option<u64>,
    pub forced_rescue_reason: Option<ForcedRescueReason>,
    #[serde(default)]
    pub skip_range_invariant_fault: Option<bool>,
    pub consecutive_index_anchor_failures: Option<u8>,
    pub consecutive_index_latest_invariant_failures: Option<u8>,
    #[serde(default)]
    pub consecutive_index_latest_unreadable_failures: Option<u8>,
    pub consecutive_cmc_zero_success_runs: Option<u8>,
    pub last_observed_staking_balance_e8s: Option<u64>,
    pub last_observed_latest_tx_id: Option<u64>,
    pub main_lock_state_ts: Option<u64>,
    pub payout_nonce: u64,
    pub active_payout_job: Option<ActivePayoutJob>,
    pub last_main_run_ts: u64,
    #[serde(default)]
    pub current_round_start_time_nanos: Option<u64>,
    #[serde(default)]
    pub current_round_start_staking_balance_e8s: Option<u64>,
    #[serde(default)]
    pub current_round_start_latest_tx_id: Option<u64>,
}

impl State {
    pub fn new(config: Config, now_secs: u64) -> Self {
        let blackhole_armed_since_ts = config.blackhole_armed.unwrap_or(false).then_some(now_secs);
        Self {
            config,
            last_summary: None,
            last_successful_transfer_ts: None,
            last_rescue_check_ts: 0,
            rescue_triggered: false,
            blackhole_armed_since_ts,
            forced_rescue_reason: None,
            skip_range_invariant_fault: Some(false),
            consecutive_index_anchor_failures: Some(0),
            consecutive_index_latest_invariant_failures: Some(0),
            consecutive_index_latest_unreadable_failures: Some(0),
            consecutive_cmc_zero_success_runs: Some(0),
            last_observed_staking_balance_e8s: None,
            last_observed_latest_tx_id: None,
            main_lock_state_ts: Some(0),
            payout_nonce: 1,
            active_payout_job: None,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
            current_round_start_time_nanos: None,
            current_round_start_staking_balance_e8s: None,
            current_round_start_latest_tx_id: None,
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
        Cow::Owned(candid::encode_one(self).expect("failed to encode faucet stable state"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode faucet stable state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        std::cell::RefCell::new(None);
    static STABLE_SKIP_RANGE_MAP: std::cell::RefCell<Option<StableBTreeMap<U64Key, U64Value, Memory>>> =
        std::cell::RefCell::new(None);
    static STATE: std::cell::RefCell<Option<State>> = std::cell::RefCell::new(None);
    static PERSISTENCE_BATCH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static PERSISTENCE_DIRTY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn with_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized)
                    .expect("failed to initialize faucet stable cell");
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("faucet stable cell not initialized"))
    })
}

fn persist_snapshot(st: &State) {
    with_stable_cell(|cell| {
        cell.set(VersionedStableState::V1(st.clone()))
            .expect("failed to persist faucet stable state");
    });
}

fn with_skip_range_map<R>(f: impl FnOnce(&mut StableBTreeMap<U64Key, U64Value, Memory>) -> R) -> R {
    STABLE_SKIP_RANGE_MAP.with(|map| {
        if map.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(1));
                let stable_map = StableBTreeMap::init(memory);
                *map.borrow_mut() = Some(stable_map);
            });
        }
        let mut borrow = map.borrow_mut();
        f(borrow.as_mut().expect("faucet skip-range stable map not initialized"))
    })
}

// Skip ranges are a durable replay-work cache for history spans that are known to be
// irrelevant under the current faucet attribution policy. Rescue upgrades conservatively
// clear the cache before the faucet resumes, and any future maintenance that bypasses that
// default must still clear the cache whenever contribution-validity rules change.
pub fn list_skip_ranges() -> Vec<SkipRange> {
    with_skip_range_map(|map| {
        map.iter()
            .map(|(start, end)| SkipRange {
                start_tx_id: start.get(),
                end_tx_id: end.get(),
            })
            .collect()
    })
}

pub(crate) fn validate_skip_range_insertion(existing: &[SkipRange], range: &SkipRange) -> Result<(), SkipRangeInsertError> {
    if range.start_tx_id > range.end_tx_id {
        return Err(SkipRangeInsertError::InvalidRange);
    }
    if existing.iter().any(|candidate| candidate.start_tx_id == range.start_tx_id) {
        return Err(SkipRangeInsertError::DuplicateStart);
    }
    if let Some(previous) = existing
        .iter()
        .rev()
        .find(|candidate| candidate.start_tx_id < range.start_tx_id)
    {
        if previous.end_tx_id.saturating_add(1) >= range.start_tx_id {
            return Err(SkipRangeInsertError::OverlapsOrAbutsPredecessor);
        }
    }
    if let Some(next) = existing
        .iter()
        .find(|candidate| candidate.start_tx_id > range.start_tx_id)
    {
        if range.end_tx_id.saturating_add(1) >= next.start_tx_id {
            return Err(SkipRangeInsertError::OverlapsOrAbutsSuccessor);
        }
    }
    Ok(())
}

pub fn insert_skip_range(range: SkipRange) -> Result<(), SkipRangeInsertError> {
    // This durable cache assumes the contribution-validity rules are unchanged since the
    // range was learned. Rescue upgrades clear the whole cache before resuming, and any
    // future maintenance path that changes contribution-validity rules must do the same
    // before relying on persisted skip ranges again.
    let existing = list_skip_ranges();
    validate_skip_range_insertion(&existing, &range)?;
    with_skip_range_map(|map| {
        map.insert(U64Key::from(range.start_tx_id), U64Value::from(range.end_tx_id));
    });
    Ok(())
}

#[cfg(test)]
pub fn latch_forced_rescue_reason(reason: ForcedRescueReason) {
    with_state_mut(|st| {
        if st.forced_rescue_reason.is_none() {
            st.forced_rescue_reason = Some(reason);
        }
    });
}

pub fn latch_skip_range_invariant_fault() {
    with_state_mut(|st| {
        st.skip_range_invariant_fault = Some(true);
    });
}

pub fn clear_skip_ranges() {
    with_skip_range_map(|map| {
        let keys: Vec<_> = map.iter().map(|(start, _)| start).collect();
        for key in keys {
            map.remove(&key);
        }
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

fn persistence_batch_active() -> bool {
    PERSISTENCE_BATCH_DEPTH.with(|depth| depth.get() > 0)
}

fn mark_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(true));
}

fn clear_persistence_dirty() {
    PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
}

pub fn persist_dirty_state() {
    let dirty = PERSISTENCE_DIRTY.with(|flag| flag.get());
    if !dirty {
        return;
    }
    let snapshot = get_state();
    persist_snapshot(&snapshot);
    clear_persistence_dirty();
}

/// A synchronous persistence-batch guard.
///
/// Do not hold this guard across an `await` point. While it is live, mutations are
/// only marked dirty and are not durably flushed until the batch ends or an
/// explicit `persist_dirty_state()` call occurs.
pub struct PersistenceBatch {
    active: bool,
}

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
                .expect("failed to reset faucet stable state for test");
        });
        clear_skip_ranges();
        PERSISTENCE_BATCH_DEPTH.with(|depth| depth.set(0));
        PERSISTENCE_DIRTY.with(|dirty| dirty.set(false));
        STATE.with(|s| *s.borrow_mut() = None);
    }

    fn principal(bytes: &[u8]) -> Principal {
        Principal::from_slice(bytes)
    }

    fn sample_config() -> Config {
        Config {
            staking_account: Account { owner: principal(&[1]), subaccount: None },
            payout_subaccount: Some([7; 32]),
            ledger_canister_id: principal(&[2]),
            index_canister_id: principal(&[3]),
            cmc_canister_id: principal(&[4]),
            rescue_controller: principal(&[5]),
            blackhole_controller: Some(principal(&[6])),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: Some(11),
            main_interval_seconds: 60,
            rescue_interval_seconds: 120,
            min_tx_e8s: 100_000_000,
            stake_recognition_delay_seconds: Some(24 * 60 * 60),
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
        let mut st = State::new(sample_config(), 1_000);
        st.last_successful_transfer_ts = Some(77);
        st.main_lock_state_ts = Some(33);
        set_state(st.clone());

        let restored = restore_state_from_stable().expect("expected persisted faucet state");
        assert_eq!(restored.last_successful_transfer_ts, Some(77));
        assert_eq!(restored.main_lock_state_ts, Some(33));
        assert_eq!(restored.payout_nonce, st.payout_nonce);
        assert_eq!(restored.config.min_tx_e8s, st.config.min_tx_e8s);
    }

    #[test]
    fn with_state_mut_persists_updates_to_stable_storage() {
        reset_test_storage();
        set_state(State::new(sample_config(), 2_000));

        with_state_mut(|st| {
            st.last_observed_staking_balance_e8s = Some(555);
            st.main_lock_state_ts = Some(99);
        });

        let restored = restore_state_from_stable().expect("expected persisted faucet state after mutation");
        assert_eq!(restored.last_observed_staking_balance_e8s, Some(555));
        assert_eq!(restored.main_lock_state_ts, Some(99));
    }

    #[test]
    fn persistence_batch_defers_writes_until_flush_boundary() {
        reset_test_storage();
        set_state(State::new(sample_config(), 3_000));

        {
            let _batch = begin_persistence_batch();
            with_state_mut(|st| {
                st.last_observed_staking_balance_e8s = Some(777);
                st.main_lock_state_ts = Some(123);
            });
            let restored_mid = restore_state_from_stable().expect("expected persisted state before batch mutation");
            assert_ne!(restored_mid.last_observed_staking_balance_e8s, Some(777));
            assert_ne!(restored_mid.main_lock_state_ts, Some(123));
            persist_dirty_state();
        }

        let restored = restore_state_from_stable().expect("expected persisted state after batch flush");
        assert_eq!(restored.last_observed_staking_balance_e8s, Some(777));
        assert_eq!(restored.main_lock_state_ts, Some(123));
    }

    #[test]
    fn skip_ranges_round_trip_through_dedicated_stable_map() {
        reset_test_storage();
        insert_skip_range(SkipRange { start_tx_id: 10, end_tx_id: 25 }).expect("first skip range should persist");
        insert_skip_range(SkipRange { start_tx_id: 40, end_tx_id: 60 }).expect("second skip range should persist");

        assert_eq!(
            list_skip_ranges(),
            vec![
                SkipRange { start_tx_id: 10, end_tx_id: 25 },
                SkipRange { start_tx_id: 40, end_tx_id: 60 },
            ]
        );
    }

    #[test]
    fn clear_skip_ranges_removes_all_entries() {
        reset_test_storage();
        insert_skip_range(SkipRange { start_tx_id: 100, end_tx_id: 200 }).expect("first skip range should persist");
        insert_skip_range(SkipRange { start_tx_id: 400, end_tx_id: 800 }).expect("second skip range should persist");

        clear_skip_ranges();

        assert!(list_skip_ranges().is_empty());
    }

    #[test]
    fn skip_range_insertion_rejects_adjacent_ranges() {
        reset_test_storage();
        insert_skip_range(SkipRange { start_tx_id: 10, end_tx_id: 20 }).expect("baseline range should persist");

        let err = insert_skip_range(SkipRange { start_tx_id: 21, end_tx_id: 30 }).expect_err("adjacent skip range should be rejected");
        assert_eq!(err, SkipRangeInsertError::OverlapsOrAbutsPredecessor);
    }

    #[test]
    fn skip_range_insertion_rejects_same_start_as_existing_range() {
        reset_test_storage();
        insert_skip_range(SkipRange { start_tx_id: 10, end_tx_id: 20 }).expect("baseline range should persist");

        let err = insert_skip_range(SkipRange { start_tx_id: 10, end_tx_id: 30 }).expect_err("duplicate-start skip range should be rejected");
        assert_eq!(err, SkipRangeInsertError::DuplicateStart);
    }

    #[test]
    fn latch_forced_rescue_reason_only_sets_the_first_reason() {
        reset_test_storage();
        set_state(State::new(sample_config(), 42));

        latch_forced_rescue_reason(ForcedRescueReason::IndexLatestInvariantBroken);
        latch_forced_rescue_reason(ForcedRescueReason::BootstrapNoSuccess);

        let forced = with_state(|st| st.forced_rescue_reason.clone());
        assert_eq!(forced, Some(ForcedRescueReason::IndexLatestInvariantBroken));
    }

    #[test]
    fn latch_skip_range_invariant_fault_sets_sticky_fault_flag() {
        reset_test_storage();
        set_state(State::new(sample_config(), 42));

        latch_skip_range_invariant_fault();
        latch_skip_range_invariant_fault();

        let fault = with_state(|st| st.skip_range_invariant_fault);
        assert_eq!(fault, Some(true));
    }

}
