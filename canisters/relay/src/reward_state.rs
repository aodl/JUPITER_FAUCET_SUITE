use std::borrow::Cow;
use std::collections::BTreeMap;

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{storable::Bound, StableCell, Storable};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

pub(crate) const REWARD_STATE_MEMORY_ID: u8 = 0;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum PendingRewardTransferStatus {
    AwaitingTransfer,
    Ambiguous,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RewardHistoryBoundary {
    pub processed_through_tx_id: Option<u64>,
    pub carried_credit_start_tx_id: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingRewardTransfer {
    pub sns_root_canister_id: Principal,
    pub sns_ledger_canister_id: Principal,
    pub snapshot_id: u64,
    pub through_commitment_tx_id: u64,
    pub next_carried_credit_start_tx_id: Option<u64>,
    pub proposed_splitter_boundaries: BTreeMap<u8, RewardHistoryBoundary>,
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
    pub carried_credit_start_tx_id: Option<u64>,
    pub splitter_boundaries: BTreeMap<u8, RewardHistoryBoundary>,
    pub last_sweep_attempt_timestamp_seconds: u64,
    pub pending_transfer: Option<PendingRewardTransfer>,
}

// Frozen decoder for the reward state committed before splitter provenance was added.
// These field types and their order-independent Candid labels must remain unchanged.
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct FrozenPendingRewardTransferV1 {
    sns_root_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    through_commitment_tx_id: u64,
    next_carried_credit_start_tx_id: Option<u64>,
    recipient: Account,
    observed_balance: Nat,
    fee: Nat,
    amount: Nat,
    memo: Vec<u8>,
    created_at_time_nanos: u64,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
    status: PendingRewardTransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
struct FrozenRewardStateV1 {
    epoch_sns_root_canister_id: Option<Principal>,
    processed_through_commitment_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
    last_sweep_attempt_timestamp_seconds: u64,
    pending_transfer: Option<FrozenPendingRewardTransferV1>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
enum VersionedRewardState {
    Uninitialized,
    V1(FrozenRewardStateV1),
    V2(RewardState),
}

impl From<FrozenPendingRewardTransferV1> for PendingRewardTransfer {
    fn from(old: FrozenPendingRewardTransferV1) -> Self {
        Self {
            sns_root_canister_id: old.sns_root_canister_id,
            sns_ledger_canister_id: old.sns_ledger_canister_id,
            snapshot_id: old.snapshot_id,
            through_commitment_tx_id: old.through_commitment_tx_id,
            next_carried_credit_start_tx_id: old.next_carried_credit_start_tx_id,
            proposed_splitter_boundaries: BTreeMap::new(),
            recipient: old.recipient,
            observed_balance: old.observed_balance,
            fee: old.fee,
            amount: old.amount,
            memo: old.memo,
            created_at_time_nanos: old.created_at_time_nanos,
            attempt_started: old.attempt_started,
            uncertain_attempt_seen: old.uncertain_attempt_seen,
            status: old.status,
        }
    }
}

impl From<FrozenRewardStateV1> for RewardState {
    fn from(old: FrozenRewardStateV1) -> Self {
        Self {
            epoch_sns_root_canister_id: old.epoch_sns_root_canister_id,
            processed_through_commitment_tx_id: old.processed_through_commitment_tx_id,
            carried_credit_start_tx_id: old.carried_credit_start_tx_id,
            splitter_boundaries: BTreeMap::new(),
            last_sweep_attempt_timestamp_seconds: old.last_sweep_attempt_timestamp_seconds,
            pending_transfer: old.pending_transfer.map(Into::into),
        }
    }
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

thread_local! {
    static CELL: std::cell::RefCell<Option<StableCell<VersionedRewardState, crate::stable_memory::Memory>>> =
        const { std::cell::RefCell::new(None) };
}

fn with_cell<R>(
    f: impl FnOnce(&mut StableCell<VersionedRewardState, crate::stable_memory::Memory>) -> R,
) -> R {
    CELL.with(|cell| {
        if cell.borrow().is_none() {
            let memory = crate::stable_memory::memory(REWARD_STATE_MEMORY_ID);
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
    with_cell(|cell| match cell.get().clone() {
        VersionedRewardState::Uninitialized => {
            cell.set(VersionedRewardState::V2(RewardState::default()));
        }
        VersionedRewardState::V1(old) => {
            cell.set(VersionedRewardState::V2(old.into()));
        }
        VersionedRewardState::V2(_) => {}
    });
}

pub(crate) fn get() -> RewardState {
    initialize_if_uninitialized();
    with_cell(|cell| match cell.get().clone() {
        VersionedRewardState::Uninitialized => unreachable!(),
        VersionedRewardState::V1(_) => unreachable!(),
        VersionedRewardState::V2(state) => state,
    })
}

pub(crate) fn set(state: RewardState) {
    assert!(
        state
            .splitter_boundaries
            .keys()
            .chain(
                state
                    .pending_transfer
                    .iter()
                    .flat_map(|pending| pending.proposed_splitter_boundaries.keys())
            )
            .all(|number| matches!(number, 10 | 20 | 30 | 40 | 50 | 60 | 70 | 80 | 90)),
        "Relay reward state contains a non-protocol splitter boundary"
    );
    with_cell(|cell| cell.set(VersionedRewardState::V2(state)));
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
    fn versioned_reward_state_v2_roundtrips() {
        let state = RewardState {
            epoch_sns_root_canister_id: Some(Principal::from_slice(&[1])),
            processed_through_commitment_tx_id: Some(9),
            carried_credit_start_tx_id: Some(8),
            splitter_boundaries: BTreeMap::from([
                (
                    10,
                    RewardHistoryBoundary {
                        processed_through_tx_id: Some(6),
                        carried_credit_start_tx_id: Some(5),
                    },
                ),
                (
                    90,
                    RewardHistoryBoundary {
                        processed_through_tx_id: Some(7),
                        carried_credit_start_tx_id: None,
                    },
                ),
            ]),
            last_sweep_attempt_timestamp_seconds: 10,
            pending_transfer: Some(PendingRewardTransfer {
                sns_root_canister_id: Principal::from_slice(&[1]),
                sns_ledger_canister_id: Principal::from_slice(&[2]),
                snapshot_id: 3,
                through_commitment_tx_id: 12,
                next_carried_credit_start_tx_id: Some(11),
                proposed_splitter_boundaries: BTreeMap::from([(
                    50,
                    RewardHistoryBoundary {
                        processed_through_tx_id: Some(14),
                        carried_credit_start_tx_id: Some(13),
                    },
                )]),
                recipient: Account {
                    owner: Principal::from_slice(&[4]),
                    subaccount: None,
                },
                observed_balance: Nat::from(1_000_u64),
                fee: Nat::from(10_u64),
                amount: Nat::from(990_u64),
                memo: b"JRS1".to_vec(),
                created_at_time_nanos: 5,
                attempt_started: true,
                uncertain_attempt_seen: true,
                status: PendingRewardTransferStatus::Ambiguous,
            }),
        };
        let encoded = candid::encode_one(VersionedRewardState::V2(state.clone())).unwrap();
        let decoded: VersionedRewardState = candid::decode_one(&encoded).unwrap();
        match decoded {
            VersionedRewardState::V2(decoded) => assert_eq!(decoded, state),
            VersionedRewardState::Uninitialized => panic!("unexpected uninitialized state"),
            VersionedRewardState::V1(_) => panic!("unexpected V1 state"),
        }
    }

    #[test]
    fn v1_migrates_once_without_changing_pending_transfer_identity() {
        let old = FrozenRewardStateV1 {
            epoch_sns_root_canister_id: Some(Principal::from_slice(&[1])),
            processed_through_commitment_tx_id: Some(9),
            carried_credit_start_tx_id: Some(8),
            last_sweep_attempt_timestamp_seconds: 10,
            pending_transfer: Some(FrozenPendingRewardTransferV1 {
                sns_root_canister_id: Principal::from_slice(&[1]),
                sns_ledger_canister_id: Principal::from_slice(&[2]),
                snapshot_id: 3,
                through_commitment_tx_id: 12,
                next_carried_credit_start_tx_id: Some(11),
                recipient: Account {
                    owner: Principal::from_slice(&[4]),
                    subaccount: None,
                },
                observed_balance: Nat::from(1_000_u64),
                fee: Nat::from(10_u64),
                amount: Nat::from(990_u64),
                memo: b"JRS1".to_vec(),
                created_at_time_nanos: 5,
                attempt_started: true,
                uncertain_attempt_seen: true,
                status: PendingRewardTransferStatus::Ambiguous,
            }),
        };
        let expected: RewardState = old.clone().into();
        with_cell(|cell| cell.set(VersionedRewardState::V1(old.clone())));

        let migrated = get();
        assert_eq!(migrated, expected);
        assert_eq!(
            migrated.epoch_sns_root_canister_id,
            old.epoch_sns_root_canister_id
        );
        assert_eq!(migrated.processed_through_commitment_tx_id, Some(9));
        assert_eq!(migrated.carried_credit_start_tx_id, Some(8));
        assert!(migrated.splitter_boundaries.is_empty());
        let pending = migrated.pending_transfer.unwrap();
        let old_pending = old.pending_transfer.unwrap();
        assert_eq!(
            pending.sns_ledger_canister_id,
            old_pending.sns_ledger_canister_id
        );
        assert_eq!(pending.recipient, old_pending.recipient);
        assert_eq!(pending.amount, old_pending.amount);
        assert_eq!(pending.fee, old_pending.fee);
        assert_eq!(pending.memo, old_pending.memo);
        assert_eq!(
            pending.created_at_time_nanos,
            old_pending.created_at_time_nanos
        );
        assert!(pending.proposed_splitter_boundaries.is_empty());
        with_cell(|cell| assert!(matches!(cell.get(), VersionedRewardState::V2(_))));
        initialize_if_uninitialized();
        with_cell(|cell| assert!(matches!(cell.get(), VersionedRewardState::V2(_))));
    }

    #[test]
    fn initialization_does_not_overwrite_existing_state() {
        reset_for_test();
        mutate(|state| state.processed_through_commitment_tx_id = Some(42));
        initialize_if_uninitialized();
        assert_eq!(get().processed_through_commitment_tx_id, Some(42));
    }

    #[test]
    fn uninitialized_reward_state_initializes_to_final_schema() {
        with_cell(|cell| cell.set(VersionedRewardState::Uninitialized));
        initialize_if_uninitialized();
        assert_eq!(get(), RewardState::default());
    }
}
