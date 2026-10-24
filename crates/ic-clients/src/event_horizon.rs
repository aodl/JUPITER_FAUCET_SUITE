use candid::{CandidType, Nat, Principal};
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
    pub subject: Principal,
    pub target: EventHorizonPokeTarget,
    /// Earliest qualifying observed block; scheduling hint, never an accounting cursor.
    pub first_block_index: u64,
    pub max_amount: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct EventHorizonPokeNotification {
    pub global_activity: bool,
    pub matches: Vec<EventHorizonPokeMatch>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent declarations pin the upstream structural Candid contract.
    #[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
    struct UpstreamMatch {
        subject: Principal,
        target: EventHorizonPokeTarget,
        first_block_index: u64,
        max_amount: Nat,
    }

    #[derive(Debug, CandidType, Deserialize, PartialEq, Eq)]
    struct UpstreamNotification {
        global_activity: bool,
        matches: Vec<UpstreamMatch>,
    }

    #[test]
    fn unified_contract_round_trips_exactly_without_narrowing_hints() {
        let huge: Nat = format!("1{}", "0".repeat(100)).parse().unwrap();
        assert!(huge > Nat::from(u128::MAX));
        for global_activity in [false, true] {
            for first_block_index in [0, (1_u64 << 53) + 1, u64::MAX] {
                let upstream = UpstreamNotification {
                    global_activity,
                    matches: vec![UpstreamMatch {
                        subject: Principal::from_slice(&[1, 2, 3]),
                        target: EventHorizonPokeTarget::NeuronNonce(0),
                        first_block_index,
                        max_amount: huge.clone(),
                    }],
                };
                let bytes = candid::encode_one(&upstream).unwrap();
                let decoded: EventHorizonPokeNotification = candid::decode_one(&bytes).unwrap();
                let returned = candid::encode_one(&decoded).unwrap();
                let round_trip: UpstreamNotification = candid::decode_one(&returned).unwrap();
                assert_eq!(round_trip, upstream);
                assert_eq!(
                    returned, bytes,
                    "callback must preserve the exact upstream record"
                );
            }
        }
    }

    #[test]
    fn empty_notifications_round_trip_with_either_global_flag() {
        for global_activity in [false, true] {
            let upstream = UpstreamNotification {
                global_activity,
                matches: Vec::new(),
            };
            let bytes = candid::encode_one(&upstream).unwrap();
            let decoded: EventHorizonPokeNotification = candid::decode_one(&bytes).unwrap();
            assert_eq!(decoded.global_activity, global_activity);
            assert!(decoded.matches.is_empty());
            assert_eq!(candid::encode_one(&decoded).unwrap(), bytes);
        }
    }

    #[test]
    fn obsolete_vector_is_not_a_notification() {
        let bytes = candid::encode_one(Vec::<EventHorizonPokeMatch>::new()).unwrap();
        assert!(candid::decode_one::<EventHorizonPokeNotification>(&bytes).is_err());
    }
}
