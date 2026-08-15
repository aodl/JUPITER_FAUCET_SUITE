use std::borrow::Cow;
use std::collections::BTreeMap;

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{storable::Bound, StableCell, Storable};
use serde::Serialize;

use crate::logic::SplitterPlan;

pub(crate) const SPLITTER_STATE_MEMORY_ID: u8 = 1;

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SplitterLeg {
    DefaultAccount,
    SubaccountOne,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SplitterLegStatus {
    Ready,
    WaitingForFunds { observed_balance_e8s: u64 },
    WaitingForFeasibleFee { expected_fee_e8s: Nat },
    Accepted { block_index: Nat },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SplitterLegProgress {
    pub status: SplitterLegStatus,
    pub attempt_started: bool,
    pub uncertain_attempt_seen: bool,
}

impl Default for SplitterLegProgress {
    fn default() -> Self {
        Self {
            status: SplitterLegStatus::Ready,
            attempt_started: false,
            uncertain_attempt_seen: false,
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveSplitterJob {
    pub pinned_ledger_canister_id: Principal,
    pub plan: SplitterPlan,
    pub default_leg: SplitterLegProgress,
    pub subaccount_one_leg: SplitterLegProgress,
    /// Persisted fencing token for the async driver that most recently claimed this job.
    pub driver_revision: u64,
}

impl ActiveSplitterJob {
    pub(crate) fn new(pinned_ledger_canister_id: Principal, plan: SplitterPlan) -> Self {
        Self {
            pinned_ledger_canister_id,
            plan,
            default_leg: SplitterLegProgress::default(),
            subaccount_one_leg: SplitterLegProgress::default(),
            driver_revision: 0,
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuarantinedSplitterJob {
    pub job: ActiveSplitterJob,
    pub blocked_leg: SplitterLeg,
    pub quarantined_at_nanos: u64,
    pub reason: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SplitterState {
    pub active_job: Option<ActiveSplitterJob>,
    pub quarantined_jobs: BTreeMap<u8, QuarantinedSplitterJob>,
    /// Never reused. Each await-capable driver claim advances this revision.
    pub next_driver_revision: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum VersionedSplitterState {
    Uninitialized,
    V1(SplitterState),
}

impl Storable for VersionedSplitterState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("encode Relay splitter state"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("encode Relay splitter state")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("decode Relay splitter state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

thread_local! {
    static CELL: std::cell::RefCell<Option<StableCell<VersionedSplitterState, crate::stable_memory::Memory>>> =
        const { std::cell::RefCell::new(None) };
}

fn with_cell<R>(
    f: impl FnOnce(&mut StableCell<VersionedSplitterState, crate::stable_memory::Memory>) -> R,
) -> R {
    CELL.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(StableCell::init(
                crate::stable_memory::memory(SPLITTER_STATE_MEMORY_ID),
                VersionedSplitterState::Uninitialized,
            ));
        }
        f(cell
            .borrow_mut()
            .as_mut()
            .expect("Relay splitter stable cell"))
    })
}

pub(crate) fn initialize_if_uninitialized() {
    with_cell(|cell| {
        if matches!(cell.get(), VersionedSplitterState::Uninitialized) {
            cell.set(VersionedSplitterState::V1(SplitterState::default()));
        }
    });
}

pub(crate) fn get() -> SplitterState {
    initialize_if_uninitialized();
    with_cell(|cell| match cell.get().clone() {
        VersionedSplitterState::Uninitialized => unreachable!(),
        VersionedSplitterState::V1(state) => state,
    })
}

pub(crate) fn set(state: SplitterState) {
    with_cell(|cell| cell.set(VersionedSplitterState::V1(state)));
}

pub(crate) fn mutate<R>(f: impl FnOnce(&mut SplitterState) -> R) -> R {
    let mut state = get();
    let result = f(&mut state);
    set(state);
    result
}

pub(crate) fn active_job() -> Option<ActiveSplitterJob> {
    get().active_job
}

pub(crate) fn set_active_job(job: ActiveSplitterJob) {
    mutate(|state| state.active_job = Some(job));
}

/// Claims the current job for one await-capable step and durably advances its fencing token.
/// A late continuation can only mutate state if the complete claimed job still matches.
pub(crate) fn claim_active_job(
    expected: &ActiveSplitterJob,
    mark_attempt_started: Option<SplitterLeg>,
) -> Option<ActiveSplitterJob> {
    mutate(|state| {
        if state.active_job.as_ref() != Some(expected) {
            return None;
        }
        let revision = state
            .next_driver_revision
            .checked_add(1)
            .expect("Relay splitter driver revision exhausted");
        state.next_driver_revision = revision;
        let job = state
            .active_job
            .as_mut()
            .expect("matched active splitter job");
        job.driver_revision = revision;
        if let Some(leg) = mark_attempt_started {
            match leg {
                SplitterLeg::DefaultAccount => job.default_leg.attempt_started = true,
                SplitterLeg::SubaccountOne => job.subaccount_one_leg.attempt_started = true,
            }
        }
        Some(job.clone())
    })
}

pub(crate) fn active_job_if_claim_matches(
    claimed: &ActiveSplitterJob,
) -> Option<ActiveSplitterJob> {
    let active = active_job()?;
    (active == *claimed).then_some(active)
}

pub(crate) fn clear_active_job() -> Option<ActiveSplitterJob> {
    mutate(|state| state.active_job.take())
}

pub(crate) fn is_quarantined(splitter_number: u8) -> bool {
    get().quarantined_jobs.contains_key(&splitter_number)
}

pub(crate) fn quarantine_active_job(
    blocked_leg: SplitterLeg,
    quarantined_at_nanos: u64,
    reason: String,
) -> Option<QuarantinedSplitterJob> {
    mutate(|state| {
        let job = state.active_job.take()?;
        let splitter_number = job.plan.splitter_number;
        let quarantined = QuarantinedSplitterJob {
            job,
            blocked_leg,
            quarantined_at_nanos,
            reason,
        };
        state
            .quarantined_jobs
            .insert(splitter_number, quarantined.clone());
        Some(quarantined)
    })
}

pub(crate) fn validate_active_ledger(ledger_canister_id: Principal) -> Result<(), String> {
    let Some(active) = active_job() else {
        return Ok(());
    };
    if active.pinned_ledger_canister_id != ledger_canister_id {
        return Err(format!(
            "active splitter {} pins ledger {}; refusing config ledger {}",
            active.plan.splitter_number,
            active.pinned_ledger_canister_id.to_text(),
            ledger_canister_id.to_text(),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    set(SplitterState::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::{build_splitter_plan, SplitterDestination};
    use icrc_ledger_types::icrc1::account::Account;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn sample_job() -> ActiveSplitterJob {
        ActiveSplitterJob::new(
            principal(1),
            build_splitter_plan(30, 500_000_007, 10_000, 99).unwrap(),
        )
    }

    #[test]
    fn splitter_memory_id_is_distinct_from_reward_memory_id() {
        assert_eq!(crate::reward_state::REWARD_STATE_MEMORY_ID, 0);
        assert_eq!(SPLITTER_STATE_MEMORY_ID, 1);
        assert_ne!(
            crate::reward_state::REWARD_STATE_MEMORY_ID,
            SPLITTER_STATE_MEMORY_ID
        );
    }

    #[test]
    fn uninitialized_state_becomes_empty_v1_and_existing_v1_is_not_overwritten() {
        with_cell(|cell| cell.set(VersionedSplitterState::Uninitialized));
        initialize_if_uninitialized();
        assert_eq!(get(), SplitterState::default());

        set_active_job(sample_job());
        initialize_if_uninitialized();
        assert!(active_job().is_some());
    }

    #[test]
    fn active_accepted_uncertain_and_quarantined_states_roundtrip() {
        reset_for_test();
        let mut job = sample_job();
        job.default_leg.status = SplitterLegStatus::Accepted {
            block_index: Nat::from(7_u64),
        };
        job.default_leg.attempt_started = true;
        job.subaccount_one_leg.attempt_started = true;
        job.subaccount_one_leg.uncertain_attempt_seen = true;
        set_active_job(job.clone());
        assert_eq!(active_job(), Some(job.clone()));

        let quarantined = quarantine_active_job(
            SplitterLeg::SubaccountOne,
            123,
            "expired ambiguous identity".to_string(),
        )
        .unwrap();
        let state = get();
        assert!(state.active_job.is_none());
        assert_eq!(state.quarantined_jobs.get(&30), Some(&quarantined));

        set_active_job(sample_job());
        clear_active_job();
        assert_eq!(get().quarantined_jobs.get(&30), Some(&quarantined));
    }

    #[test]
    fn reward_and_splitter_cells_preserve_each_other() {
        reset_for_test();
        crate::reward_state::reset_for_test();
        crate::reward_state::mutate(|state| state.processed_through_commitment_tx_id = Some(41));
        set_active_job(sample_job());
        assert_eq!(
            crate::reward_state::get().processed_through_commitment_tx_id,
            Some(41)
        );

        crate::reward_state::mutate(|state| {
            state.carried_credit_start_tx_id = Some(40);
        });
        assert_eq!(active_job().unwrap().plan.splitter_number, 30);
        assert_eq!(
            crate::reward_state::get().carried_credit_start_tx_id,
            Some(40)
        );
    }

    #[test]
    fn active_job_pins_destinations_and_rejects_ledger_change() {
        reset_for_test();
        let job = sample_job();
        assert_eq!(
            job.plan.default_leg.destination,
            SplitterDestination::DefaultAccount
        );
        assert_eq!(
            job.plan.subaccount_one_leg.destination,
            SplitterDestination::SubaccountOne
        );
        set_active_job(job);
        assert!(validate_active_ledger(principal(1)).is_ok());
        assert!(validate_active_ledger(principal(2)).is_err());

        let relay = principal(9);
        assert_eq!(
            crate::logic::splitter_destination_account(relay, SplitterDestination::DefaultAccount),
            Account {
                owner: relay,
                subaccount: None
            }
        );
    }
}
