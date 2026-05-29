use candid::Principal;

pub const ICP_LEDGER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const ICP_INDEX_ID: &str = "qhbym-qaaaa-aaaaa-aaafq-cai";
pub const NNS_GOVERNANCE_ID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";
pub const CYCLES_MINTING_CANISTER_ID: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";

pub fn icp_ledger() -> Principal {
    Principal::from_text(ICP_LEDGER_ID).expect("valid ICP ledger principal")
}

pub fn icp_index() -> Principal {
    Principal::from_text(ICP_INDEX_ID).expect("valid ICP index principal")
}

pub fn nns_governance() -> Principal {
    Principal::from_text(NNS_GOVERNANCE_ID).expect("valid NNS governance principal")
}

pub fn fixture_principal() -> Principal {
    Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("valid fixture principal")
}
