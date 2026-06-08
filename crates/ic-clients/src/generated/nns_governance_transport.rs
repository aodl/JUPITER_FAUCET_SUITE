// Generated from candid/nns-governance/governance.subset.did.
// Upstream source: dfinity/ic rs/nns/governance/canister/governance.did.
// Upstream commit: 0c7c8b83144844e1a598633585b3ee1beebe338b.
// Generator: nns-bindgen-check using candid_parser = 0.2.4
// emit_bindgen(...).methods rendered through Jupiter raw transport template.
//
// Do not edit manually. Run:
//   cargo run -p nns-bindgen-check -- --update
// Then review the generated diff.

use candid::Principal;
use ic_cdk::call::{Call, CallFailed, Response};
use jupiter_nns_types::{ListNeurons, ManageNeuronRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernanceCallWait {
    Bounded { timeout_seconds: Option<u32> },
}

impl GovernanceCallWait {
    pub const fn bounded_default() -> Self {
        Self::Bounded {
            timeout_seconds: None,
        }
    }

    pub const fn bounded_seconds(timeout_seconds: u32) -> Self {
        Self::Bounded {
            timeout_seconds: Some(timeout_seconds),
        }
    }
}

pub const GET_FULL_NEURON_METHOD: &str = "get_full_neuron";

pub async fn get_full_neuron(
    canister_id: Principal,
    arg0: &u64,
    wait: GovernanceCallWait,
) -> Result<Response, CallFailed> {
    let call = match wait {
        GovernanceCallWait::Bounded { timeout_seconds } => {
            let call = Call::bounded_wait(canister_id, GET_FULL_NEURON_METHOD);
            match timeout_seconds {
                Some(timeout_seconds) => call.change_timeout(timeout_seconds),
                None => call,
            }
        }
    };
    call.with_arg(arg0).await
}

pub const LIST_NEURONS_METHOD: &str = "list_neurons";

pub async fn list_neurons(
    canister_id: Principal,
    arg0: &ListNeurons,
    wait: GovernanceCallWait,
) -> Result<Response, CallFailed> {
    let call = match wait {
        GovernanceCallWait::Bounded { timeout_seconds } => {
            let call = Call::bounded_wait(canister_id, LIST_NEURONS_METHOD);
            match timeout_seconds {
                Some(timeout_seconds) => call.change_timeout(timeout_seconds),
                None => call,
            }
        }
    };
    call.with_arg(arg0).await
}

pub const MANAGE_NEURON_METHOD: &str = "manage_neuron";

pub async fn manage_neuron(
    canister_id: Principal,
    arg0: &ManageNeuronRequest,
    wait: GovernanceCallWait,
) -> Result<Response, CallFailed> {
    let call = match wait {
        GovernanceCallWait::Bounded { timeout_seconds } => {
            let call = Call::bounded_wait(canister_id, MANAGE_NEURON_METHOD);
            match timeout_seconds {
                Some(timeout_seconds) => call.change_timeout(timeout_seconds),
                None => call,
            }
        }
    };
    call.with_arg(arg0).await
}
