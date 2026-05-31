use candid::Principal;

#[allow(unused_imports)]
pub use jupiter_ic_clients::constants::{
    BLACKHOLE_CANISTER_ID, CYCLES_MINTING_CANISTER_ID, ICP_INDEX_ID, ICP_LEDGER_ID,
    NNS_GOVERNANCE_ID, NNS_ROOT_ID, SNS_WASM_ID,
};

pub fn icp_ledger() -> Principal {
    jupiter_ic_clients::constants::icp_ledger_id()
}

pub fn icp_index() -> Principal {
    jupiter_ic_clients::constants::icp_index_id()
}

pub fn nns_governance() -> Principal {
    jupiter_ic_clients::constants::nns_governance_id()
}

pub fn cycles_minting_canister() -> Principal {
    jupiter_ic_clients::constants::cycles_minting_canister_id()
}

pub fn blackhole_canister() -> Principal {
    jupiter_ic_clients::constants::blackhole_canister_id()
}

pub fn nns_root() -> Principal {
    jupiter_ic_clients::constants::nns_root_id()
}

pub fn sns_wasm() -> Principal {
    jupiter_ic_clients::constants::sns_wasm_id()
}

pub fn fixture_principal() -> Principal {
    Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("valid fixture principal")
}
