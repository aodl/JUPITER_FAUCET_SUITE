use candid::{CandidType, Nat};
use serde::Deserialize;

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum EventHorizonPokeTarget {
    #[serde(rename = "subaccount")]
    Subaccount(u64),
    #[serde(rename = "neuron_nonce")]
    NeuronNonce(u64),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct EventHorizonPokeMatch {
    pub target: EventHorizonPokeTarget,
    /// Earliest qualifying observed block; scheduling hint, never an accounting cursor.
    pub first_block_index: u64,
    pub max_amount: Nat,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independently declare the upstream required wire record. This catches a
    // receiver silently dropping a newly required field during Candid decoding.
    #[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
    struct UpstreamMatch {
        target: EventHorizonPokeTarget,
        first_block_index: u64,
        max_amount: Nat,
    }

    #[test]
    fn current_three_field_contract_round_trips_exactly() {
        for first_block_index in [0, (1_u64 << 53) + 1, u64::MAX] {
            let upstream = vec![UpstreamMatch {
                target: EventHorizonPokeTarget::NeuronNonce(0),
                first_block_index,
                max_amount: Nat::from(u128::MAX),
            }];
            let bytes = candid::encode_one(&upstream).unwrap();
            let decoded: Vec<EventHorizonPokeMatch> = candid::decode_one(&bytes).unwrap();
            let returned = candid::encode_one(&decoded).unwrap();
            let round_trip: Vec<UpstreamMatch> = candid::decode_one(&returned)
                .expect("production callback must preserve the required first_block_index");
            assert_eq!(round_trip, upstream);
            assert_eq!(
                returned, bytes,
                "callback ABI must have exactly three fields"
            );
        }
    }
}
