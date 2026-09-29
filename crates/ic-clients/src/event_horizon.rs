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
    pub max_amount: Nat,
}
