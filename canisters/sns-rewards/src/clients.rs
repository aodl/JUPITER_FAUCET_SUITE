use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub(crate) struct NeuronId {
    pub id: Vec<u8>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub(crate) struct NeuronPermission {
    pub principal: Option<Principal>,
    pub permission_type: Vec<i32>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub(crate) struct Neuron {
    pub id: Option<NeuronId>,
    pub permissions: Vec<NeuronPermission>,
    pub cached_neuron_stake_e8s: u64,
    pub neuron_fees_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub(crate) struct ListNeurons {
    pub of_principal: Option<Principal>,
    pub limit: u32,
    pub start_page_at: Option<NeuronId>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub(crate) struct ListNeuronsResponse {
    pub neurons: Vec<Neuron>,
}

pub(crate) async fn list_neurons(
    governance_canister_id: Principal,
    start_page_at: Option<Vec<u8>>,
) -> Result<ListNeuronsResponse, String> {
    let request = ListNeurons {
        of_principal: None,
        limit: crate::policy::SNS_NEURON_PAGE_SIZE,
        start_page_at: start_page_at.map(|id| NeuronId { id }),
    };
    let response = Call::bounded_wait(governance_canister_id, "list_neurons")
        .with_arg(request)
        .change_timeout(60)
        .await
        .map_err(|err| format!("list_neurons failed: {err:?}"))?;
    response
        .candid()
        .map_err(|err| format!("decode list_neurons failed: {err:?}"))
}
