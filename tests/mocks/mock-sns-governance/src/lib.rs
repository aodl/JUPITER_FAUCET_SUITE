use std::cell::RefCell;

use candid::{CandidType, Deserialize, Principal};

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct NeuronId {
    id: Vec<u8>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct NeuronPermission {
    principal: Option<Principal>,
    permission_type: Vec<i32>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct Neuron {
    id: Option<NeuronId>,
    permissions: Vec<NeuronPermission>,
    cached_neuron_stake_e8s: u64,
    neuron_fees_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct ListNeurons {
    of_principal: Option<Principal>,
    limit: u32,
    start_page_at: Option<NeuronId>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct ListNeuronsResponse {
    neurons: Vec<Neuron>,
}

thread_local! {
    static NEURONS: RefCell<Vec<Neuron>> = const { RefCell::new(Vec::new()) };
    static CALLS: RefCell<Vec<ListNeurons>> = const { RefCell::new(Vec::new()) };
    static FAIL_NEXT: RefCell<bool> = const { RefCell::new(false) };
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn list_neurons(args: ListNeurons) -> ListNeuronsResponse {
    CALLS.with(|calls| calls.borrow_mut().push(args.clone()));
    if FAIL_NEXT.with(|flag| flag.replace(false)) {
        ic_cdk::trap("debug injected list_neurons failure");
    }
    let mut neurons = NEURONS.with(|value| value.borrow().clone());
    neurons.sort_by(|left, right| {
        left.id
            .as_ref()
            .map(|id| id.id.as_slice())
            .cmp(&right.id.as_ref().map(|id| id.id.as_slice()))
    });
    if let Some(cursor) = args.start_page_at {
        neurons.retain(|neuron| neuron.id.as_ref().is_some_and(|id| id.id > cursor.id));
    }
    neurons.truncate(args.limit as usize);
    ListNeuronsResponse { neurons }
}

#[ic_cdk::update]
fn debug_set_neurons(neurons: Vec<Neuron>) {
    NEURONS.with(|value| *value.borrow_mut() = neurons);
}

#[ic_cdk::update]
fn debug_fail_next_call() {
    FAIL_NEXT.with(|flag| *flag.borrow_mut() = true);
}

#[ic_cdk::query]
fn debug_calls() -> Vec<ListNeurons> {
    CALLS.with(|calls| calls.borrow().clone())
}

#[ic_cdk::update]
fn debug_reset_calls() {
    CALLS.with(|calls| calls.borrow_mut().clear());
}

ic_cdk::export_candid!();
