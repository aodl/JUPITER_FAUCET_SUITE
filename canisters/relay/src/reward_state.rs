use std::borrow::Cow;

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{storable::Bound, StableCell, Storable};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

pub(crate) const REWARD_STATE_MEMORY_ID: u8 = 0;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingRewardRecipient {
    pub recipient: Account,
    pub amount: Nat,
    pub memo: Vec<u8>,
    pub created_at_time_nanos: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingRewardPayout {
    pub sns_ledger_canister_id: Principal,
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

// Keep the deployed V3 label. It is part of the stable Candid representation even though the
// retired V1/V2 decoders no longer exist.
#[derive(CandidType, Deserialize, Serialize, Clone)]
enum VersionedRewardState {
    Uninitialized,
    V3(RewardState),
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
    with_cell(|cell| {
        if matches!(cell.get(), VersionedRewardState::Uninitialized) {
            cell.set(VersionedRewardState::V3(RewardState::default()));
        }
    });
}

pub(crate) fn get() -> RewardState {
    initialize_if_uninitialized();
    with_cell(|cell| match cell.get().clone() {
        VersionedRewardState::Uninitialized => unreachable!(),
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

    // This freezes the deployed V3 wire shape. Candid record-width subtyping must let the current
    // implementation discard settlement fields which are no longer part of the live state machine.
    #[allow(dead_code)]
    #[derive(CandidType)]
    enum DeployedPendingRewardTransferStatus {
        AwaitingTransfer,
        Ambiguous,
        NeedsFreshIdentity,
        WaitingForBalance,
    }

    #[derive(CandidType)]
    struct DeployedPendingRewardRecipient {
        recipient: Account,
        observed_balance: Option<Nat>,
        amount: Nat,
        memo: Vec<u8>,
        created_at_time_nanos: u64,
        attempt_started: bool,
        uncertain_attempt_seen: bool,
        status: DeployedPendingRewardTransferStatus,
    }

    #[derive(CandidType)]
    struct DeployedPendingRewardPayout {
        sns_root_canister_id: Principal,
        sns_ledger_canister_id: Principal,
        snapshot_id: u64,
        attribution_commitment_tx_id: u64,
        fee: Nat,
        recipients: Vec<DeployedPendingRewardRecipient>,
        next_recipient_index: u32,
    }

    #[derive(CandidType)]
    struct DeployedRewardStateV3 {
        last_sweep_attempt_timestamp_seconds: u64,
        pending_payout: Option<DeployedPendingRewardPayout>,
    }

    #[allow(dead_code)]
    #[derive(CandidType)]
    enum DeployedVersionedRewardState {
        Uninitialized,
        V3(DeployedRewardStateV3),
    }

    fn current_state() -> RewardState {
        RewardState {
            last_sweep_attempt_timestamp_seconds: 10,
            pending_payout: Some(PendingRewardPayout {
                sns_ledger_canister_id: Principal::from_slice(&[2]),
                attribution_commitment_tx_id: 12,
                fee: Nat::from(10_u64),
                recipients: vec![PendingRewardRecipient {
                    recipient: Account {
                        owner: Principal::from_slice(&[4]),
                        subaccount: None,
                    },
                    amount: Nat::from(990_u64),
                    memo: b"JRS1".to_vec(),
                    created_at_time_nanos: 5,
                }],
                next_recipient_index: 0,
            }),
        }
    }

    #[test]
    fn versioned_reward_state_v3_roundtrips() {
        let state = current_state();
        let encoded = candid::encode_one(VersionedRewardState::V3(state.clone())).unwrap();
        let decoded: VersionedRewardState = candid::decode_one(&encoded).unwrap();
        assert!(matches!(decoded, VersionedRewardState::V3(decoded) if decoded == state));
    }

    #[test]
    fn deployed_v3_wire_shape_decodes_into_smaller_v3_record() {
        let deployed = DeployedVersionedRewardState::V3(DeployedRewardStateV3 {
            last_sweep_attempt_timestamp_seconds: 10,
            pending_payout: Some(DeployedPendingRewardPayout {
                sns_root_canister_id: Principal::from_slice(&[1]),
                sns_ledger_canister_id: Principal::from_slice(&[2]),
                snapshot_id: 3,
                attribution_commitment_tx_id: 12,
                fee: Nat::from(10_u64),
                recipients: vec![DeployedPendingRewardRecipient {
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
                    status: DeployedPendingRewardTransferStatus::Ambiguous,
                }],
                next_recipient_index: 0,
            }),
        });
        let encoded = candid::encode_one(deployed).unwrap();
        let decoded: VersionedRewardState = candid::decode_one(&encoded).unwrap();
        assert!(matches!(decoded, VersionedRewardState::V3(decoded) if decoded == current_state()));
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
