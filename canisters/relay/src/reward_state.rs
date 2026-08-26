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
    NeedsFreshIdentity,
    WaitingForBalance,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingRewardRecipient {
    pub recipient: Account,
    /// Balance read immediately before this identity's first transfer attempt. Absent until the
    /// attempt is durably pinned; ambiguity reconciliation requires it to remain exact.
    pub observed_balance: Option<Nat>,
    pub amount: Nat,
    pub memo: Vec<u8>,
    pub created_at_time_nanos: u64,
    pub attempt_started: bool,
    pub uncertain_attempt_seen: bool,
    pub status: PendingRewardTransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingRewardPayout {
    pub sns_root_canister_id: Principal,
    pub sns_ledger_canister_id: Principal,
    pub snapshot_id: u64,
    pub attribution_commitment_tx_id: u64,
    pub fee: Nat,
    pub recipients: Vec<PendingRewardRecipient>,
    pub next_recipient_index: u32,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RewardState {
    /// Stable compatibility name; records the latest adjudication that consumed the weekly cadence.
    pub last_sweep_attempt_timestamp_seconds: u64,
    pub pending_payout: Option<PendingRewardPayout>,
}

// Frozen decoders for the two deployed reward-state schemas. They are migration inputs only;
// attribution cursors and splitter boundaries deliberately do not enter the live V3 state.
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
enum FrozenPendingRewardTransferStatus {
    AwaitingTransfer,
    Ambiguous,
}

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
    status: FrozenPendingRewardTransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
struct FrozenRewardStateV1 {
    epoch_sns_root_canister_id: Option<Principal>,
    processed_through_commitment_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
    last_sweep_attempt_timestamp_seconds: u64,
    pending_transfer: Option<FrozenPendingRewardTransferV1>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FrozenRewardHistoryBoundaryV2 {
    processed_through_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
struct FrozenPendingRewardTransferV2 {
    sns_root_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    through_commitment_tx_id: u64,
    next_carried_credit_start_tx_id: Option<u64>,
    proposed_splitter_boundaries: BTreeMap<u8, FrozenRewardHistoryBoundaryV2>,
    recipient: Account,
    observed_balance: Nat,
    fee: Nat,
    amount: Nat,
    memo: Vec<u8>,
    created_at_time_nanos: u64,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
    status: FrozenPendingRewardTransferStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
struct FrozenRewardStateV2 {
    epoch_sns_root_canister_id: Option<Principal>,
    processed_through_commitment_tx_id: Option<u64>,
    carried_credit_start_tx_id: Option<u64>,
    splitter_boundaries: BTreeMap<u8, FrozenRewardHistoryBoundaryV2>,
    last_sweep_attempt_timestamp_seconds: u64,
    pending_transfer: Option<FrozenPendingRewardTransferV2>,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
enum VersionedRewardState {
    Uninitialized,
    V1(FrozenRewardStateV1),
    V2(FrozenRewardStateV2),
    V3(RewardState),
}

#[allow(clippy::too_many_arguments)]
fn legacy_payout(
    sns_root_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    through_commitment_tx_id: u64,
    recipient: Account,
    observed_balance: Nat,
    fee: Nat,
    amount: Nat,
    memo: Vec<u8>,
    created_at_time_nanos: u64,
    attempt_started: bool,
    uncertain_attempt_seen: bool,
    status: PendingRewardTransferStatus,
) -> PendingRewardPayout {
    PendingRewardPayout {
        sns_root_canister_id,
        sns_ledger_canister_id,
        snapshot_id,
        attribution_commitment_tx_id: through_commitment_tx_id,
        fee,
        recipients: vec![PendingRewardRecipient {
            recipient,
            observed_balance: Some(observed_balance),
            amount,
            memo,
            created_at_time_nanos,
            attempt_started,
            uncertain_attempt_seen,
            status,
        }],
        next_recipient_index: 0,
    }
}

impl From<FrozenRewardStateV1> for RewardState {
    fn from(old: FrozenRewardStateV1) -> Self {
        Self {
            // V1 recorded any attempted sweep, including transient failures. V3 records only a
            // completed adjudication, so inheriting this timestamp could suppress the first valid
            // stateless adjudication for a week.
            last_sweep_attempt_timestamp_seconds: 0,
            pending_payout: old.pending_transfer.map(|pending| {
                legacy_payout(
                    pending.sns_root_canister_id,
                    pending.sns_ledger_canister_id,
                    pending.snapshot_id,
                    pending.through_commitment_tx_id,
                    pending.recipient,
                    pending.observed_balance,
                    pending.fee,
                    pending.amount,
                    pending.memo,
                    pending.created_at_time_nanos,
                    pending.attempt_started,
                    pending.uncertain_attempt_seen,
                    pending.status.into(),
                )
            }),
        }
    }
}

impl From<FrozenRewardStateV2> for RewardState {
    fn from(old: FrozenRewardStateV2) -> Self {
        Self {
            last_sweep_attempt_timestamp_seconds: 0,
            pending_payout: old.pending_transfer.map(|pending| {
                legacy_payout(
                    pending.sns_root_canister_id,
                    pending.sns_ledger_canister_id,
                    pending.snapshot_id,
                    pending.through_commitment_tx_id,
                    pending.recipient,
                    pending.observed_balance,
                    pending.fee,
                    pending.amount,
                    pending.memo,
                    pending.created_at_time_nanos,
                    pending.attempt_started,
                    pending.uncertain_attempt_seen,
                    pending.status.into(),
                )
            }),
        }
    }
}

impl From<FrozenPendingRewardTransferStatus> for PendingRewardTransferStatus {
    fn from(old: FrozenPendingRewardTransferStatus) -> Self {
        match old {
            FrozenPendingRewardTransferStatus::AwaitingTransfer => Self::AwaitingTransfer,
            FrozenPendingRewardTransferStatus::Ambiguous => Self::Ambiguous,
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
            cell.set(VersionedRewardState::V3(RewardState::default()));
        }
        VersionedRewardState::V1(old) => {
            cell.set(VersionedRewardState::V3(old.into()));
        }
        VersionedRewardState::V2(old) => {
            cell.set(VersionedRewardState::V3(old.into()));
        }
        VersionedRewardState::V3(_) => {}
    });
}

pub(crate) fn get() -> RewardState {
    initialize_if_uninitialized();
    with_cell(|cell| match cell.get().clone() {
        VersionedRewardState::Uninitialized
        | VersionedRewardState::V1(_)
        | VersionedRewardState::V2(_) => unreachable!(),
        VersionedRewardState::V3(state) => state,
    })
}

pub(crate) fn set(state: RewardState) {
    if let Some(payout) = &state.pending_payout {
        assert!(!payout.recipients.is_empty(), "pending payout is empty");
        assert!(
            usize::try_from(payout.next_recipient_index)
                .is_ok_and(|index| index < payout.recipients.len()),
            "pending payout progress is out of bounds"
        );
    }
    with_cell(|cell| cell.set(VersionedRewardState::V3(state)));
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

    fn old_pending() -> FrozenPendingRewardTransferV2 {
        FrozenPendingRewardTransferV2 {
            sns_root_canister_id: Principal::from_slice(&[1]),
            sns_ledger_canister_id: Principal::from_slice(&[2]),
            snapshot_id: 3,
            through_commitment_tx_id: 12,
            next_carried_credit_start_tx_id: Some(11),
            proposed_splitter_boundaries: BTreeMap::from([(
                50,
                FrozenRewardHistoryBoundaryV2 {
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
            status: FrozenPendingRewardTransferStatus::Ambiguous,
        }
    }

    #[test]
    fn versioned_reward_state_v3_roundtrips() {
        let state = RewardState {
            last_sweep_attempt_timestamp_seconds: 10,
            pending_payout: Some(PendingRewardPayout {
                sns_root_canister_id: Principal::from_slice(&[1]),
                sns_ledger_canister_id: Principal::from_slice(&[2]),
                snapshot_id: 3,
                attribution_commitment_tx_id: 12,
                fee: Nat::from(10_u64),
                recipients: vec![PendingRewardRecipient {
                    recipient: Account {
                        owner: Principal::from_slice(&[4]),
                        subaccount: None,
                    },
                    observed_balance: Some(Nat::from(1_000_u64)),
                    amount: Nat::from(990_u64),
                    memo: b"JRS1".to_vec(),
                    created_at_time_nanos: 5,
                    attempt_started: true,
                    uncertain_attempt_seen: true,
                    status: PendingRewardTransferStatus::Ambiguous,
                }],
                next_recipient_index: 0,
            }),
        };
        let encoded = candid::encode_one(VersionedRewardState::V3(state.clone())).unwrap();
        let decoded: VersionedRewardState = candid::decode_one(&encoded).unwrap();
        assert!(matches!(decoded, VersionedRewardState::V3(decoded) if decoded == state));
    }

    #[test]
    fn v2_migration_discards_attribution_cursors_and_preserves_pending_identity() {
        let old = FrozenRewardStateV2 {
            epoch_sns_root_canister_id: Some(Principal::from_slice(&[1])),
            processed_through_commitment_tx_id: Some(9),
            carried_credit_start_tx_id: Some(8),
            splitter_boundaries: BTreeMap::from([(
                50,
                FrozenRewardHistoryBoundaryV2 {
                    processed_through_tx_id: Some(7),
                    carried_credit_start_tx_id: Some(6),
                },
            )]),
            last_sweep_attempt_timestamp_seconds: 10,
            pending_transfer: Some(old_pending()),
        };
        with_cell(|cell| cell.set(VersionedRewardState::V2(old)));

        let migrated = get();
        assert_eq!(migrated.last_sweep_attempt_timestamp_seconds, 0);
        let payout = migrated.pending_payout.unwrap();
        assert_eq!(payout.attribution_commitment_tx_id, 12);
        assert_eq!(payout.fee, Nat::from(10_u64));
        assert_eq!(payout.recipients.len(), 1);
        let recipient = &payout.recipients[0];
        assert_eq!(recipient.amount, Nat::from(990_u64));
        assert_eq!(recipient.memo, b"JRS1");
        assert_eq!(recipient.created_at_time_nanos, 5);
        assert_eq!(recipient.status, PendingRewardTransferStatus::Ambiguous);
        with_cell(|cell| assert!(matches!(cell.get(), VersionedRewardState::V3(_))));
    }

    #[test]
    fn v1_migration_freezes_status_discards_cursor_and_resets_cadence() {
        let old = FrozenRewardStateV1 {
            epoch_sns_root_canister_id: Some(Principal::from_slice(&[1])),
            processed_through_commitment_tx_id: Some(9),
            carried_credit_start_tx_id: Some(8),
            last_sweep_attempt_timestamp_seconds: 777,
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
                memo: b"legacy-v1".to_vec(),
                created_at_time_nanos: 5,
                attempt_started: true,
                uncertain_attempt_seen: true,
                status: FrozenPendingRewardTransferStatus::Ambiguous,
            }),
        };
        with_cell(|cell| cell.set(VersionedRewardState::V1(old)));

        let migrated = get();
        assert_eq!(migrated.last_sweep_attempt_timestamp_seconds, 0);
        let recipient = &migrated.pending_payout.unwrap().recipients[0];
        assert_eq!(recipient.memo, b"legacy-v1");
        assert_eq!(recipient.observed_balance, Some(Nat::from(1_000_u64)));
        assert_eq!(recipient.status, PendingRewardTransferStatus::Ambiguous);
    }

    #[test]
    fn initialization_does_not_overwrite_existing_v3_state() {
        reset_for_test();
        mutate(|state| state.last_sweep_attempt_timestamp_seconds = 42);
        initialize_if_uninitialized();
        assert_eq!(get().last_sweep_attempt_timestamp_seconds, 42);
    }

    #[test]
    fn uninitialized_reward_state_initializes_to_v3() {
        with_cell(|cell| cell.set(VersionedRewardState::Uninitialized));
        initialize_if_uninitialized();
        assert_eq!(get(), RewardState::default());
        with_cell(|cell| assert!(matches!(cell.get(), VersionedRewardState::V3(_))));
    }
}
