use std::borrow::Cow;

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

pub(crate) const REWARD_STATE_MEMORY_ID: u8 = 0;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum PendingRewardTransferStatus {
    AwaitingTransfer,
    Ambiguous,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingRewardTransfer {
    pub sns_root_canister_id: Principal,
    pub sns_ledger_canister_id: Principal,
    pub snapshot_id: u64,
    pub through_commitment_tx_id: u64,
    pub recipient: Account,
    pub observed_balance: Nat,
    pub fee: Nat,
    pub amount: Nat,
    pub memo: Vec<u8>,
    pub created_at_time_nanos: u64,
    pub attempt_started: bool,
    pub uncertain_attempt_seen: bool,
    pub status: PendingRewardTransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RewardState {
    pub epoch_sns_root_canister_id: Option<Principal>,
    pub processed_through_commitment_tx_id: Option<u64>,
    pub last_sweep_attempt_timestamp_seconds: u64,
    pub pending_transfer: Option<PendingRewardTransfer>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
enum VersionedRewardState {
    Uninitialized,
    V1(RewardState),
}

impl Storable for VersionedRewardState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("encode Relay reward state"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("encode Relay reward state")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("decode Relay reward state")
    }
    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static CELL: std::cell::RefCell<Option<StableCell<VersionedRewardState, Memory>>> =
        const { std::cell::RefCell::new(None) };
}

fn with_cell<R>(f: impl FnOnce(&mut StableCell<VersionedRewardState, Memory>) -> R) -> R {
    CELL.with(|cell| {
        if cell.borrow().is_none() {
            let memory = MEMORY_MANAGER
                .with(|manager| manager.borrow().get(MemoryId::new(REWARD_STATE_MEMORY_ID)));
            *cell.borrow_mut() = Some(StableCell::init(
                memory,
                VersionedRewardState::Uninitialized,
            ));
        }
        f(cell
            .borrow_mut()
            .as_mut()
            .expect("Relay reward stable cell"))
    })
}

pub(crate) fn initialize_if_uninitialized() {
    with_cell(|cell| {
        if matches!(cell.get(), VersionedRewardState::Uninitialized) {
            cell.set(VersionedRewardState::V1(RewardState::default()));
        }
    });
}

pub(crate) fn get() -> RewardState {
    initialize_if_uninitialized();
    with_cell(|cell| match cell.get().clone() {
        VersionedRewardState::Uninitialized => unreachable!(),
        VersionedRewardState::V1(state) => state,
    })
}

pub(crate) fn set(state: RewardState) {
    with_cell(|cell| cell.set(VersionedRewardState::V1(state)));
}

pub(crate) fn mutate<R>(f: impl FnOnce(&mut RewardState) -> R) -> R {
    let mut state = get();
    let result = f(&mut state);
    set(state);
    result
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    set(RewardState::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_reward_state_roundtrips() {
        let state = RewardState {
            epoch_sns_root_canister_id: Some(Principal::from_slice(&[1])),
            processed_through_commitment_tx_id: Some(9),
            last_sweep_attempt_timestamp_seconds: 10,
            pending_transfer: None,
        };
        let encoded = candid::encode_one(VersionedRewardState::V1(state.clone())).unwrap();
        let decoded: VersionedRewardState = candid::decode_one(&encoded).unwrap();
        match decoded {
            VersionedRewardState::V1(decoded) => assert_eq!(decoded, state),
            VersionedRewardState::Uninitialized => panic!("unexpected uninitialized state"),
        }
    }

    #[test]
    fn initialization_does_not_overwrite_existing_state() {
        reset_for_test();
        mutate(|state| state.processed_through_commitment_tx_id = Some(42));
        initialize_if_uninitialized();
        assert_eq!(get().processed_through_commitment_tx_id, Some(42));
    }
}
