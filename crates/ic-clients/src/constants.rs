//! Canonical mainnet canister principal constants.
//!
//! This module owns shared principal text and constructors. Canister-specific
//! install defaults and validation policy should stay with each canister.

use candid::Principal;

pub const ICP_LEDGER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const ICP_INDEX_ID: &str = "qhbym-qaaaa-aaaaa-aaafq-cai";
pub const NNS_GOVERNANCE_ID: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";
pub const CYCLES_MINTING_CANISTER_ID: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";
pub const BLACKHOLE_CANISTER_ID: &str = "77deu-baaaa-aaaar-qb6za-cai";
pub const NNS_ROOT_ID: &str = "r7inp-6aaaa-aaaaa-aaabq-cai";
pub const SNS_WASM_ID: &str = "qaa6y-5yaaa-aaaaa-aaafa-cai";

fn principal_from_text(text: &str, label: &str) -> Principal {
    Principal::from_text(text).unwrap_or_else(|_| panic!("invalid hardcoded {label} principal"))
}

pub fn icp_ledger_id() -> Principal {
    principal_from_text(ICP_LEDGER_ID, "ICP ledger")
}

pub fn icp_index_id() -> Principal {
    principal_from_text(ICP_INDEX_ID, "ICP index")
}

pub fn nns_governance_id() -> Principal {
    principal_from_text(NNS_GOVERNANCE_ID, "NNS governance")
}

pub fn cycles_minting_canister_id() -> Principal {
    principal_from_text(CYCLES_MINTING_CANISTER_ID, "cycles minting canister")
}

pub fn blackhole_canister_id() -> Principal {
    principal_from_text(BLACKHOLE_CANISTER_ID, "blackhole canister")
}

pub fn nns_root_id() -> Principal {
    principal_from_text(NNS_ROOT_ID, "NNS root")
}

pub fn sns_wasm_id() -> Principal {
    principal_from_text(SNS_WASM_ID, "SNS-WASM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_principal_constants_preserve_external_ids() {
        assert_eq!(ICP_LEDGER_ID, "ryjl3-tyaaa-aaaaa-aaaba-cai");
        assert_eq!(ICP_INDEX_ID, "qhbym-qaaaa-aaaaa-aaafq-cai");
        assert_eq!(NNS_GOVERNANCE_ID, "rrkah-fqaaa-aaaaa-aaaaq-cai");
        assert_eq!(CYCLES_MINTING_CANISTER_ID, "rkp4c-7iaaa-aaaaa-aaaca-cai");
        assert_eq!(BLACKHOLE_CANISTER_ID, "77deu-baaaa-aaaar-qb6za-cai");
        assert_eq!(NNS_ROOT_ID, "r7inp-6aaaa-aaaaa-aaabq-cai");
        assert_eq!(SNS_WASM_ID, "qaa6y-5yaaa-aaaaa-aaafa-cai");
    }

    #[test]
    fn mainnet_principal_constructors_parse_constants() {
        assert_eq!(icp_ledger_id().to_text(), ICP_LEDGER_ID);
        assert_eq!(icp_index_id().to_text(), ICP_INDEX_ID);
        assert_eq!(nns_governance_id().to_text(), NNS_GOVERNANCE_ID);
        assert_eq!(
            cycles_minting_canister_id().to_text(),
            CYCLES_MINTING_CANISTER_ID
        );
        assert_eq!(blackhole_canister_id().to_text(), BLACKHOLE_CANISTER_ID);
        assert_eq!(nns_root_id().to_text(), NNS_ROOT_ID);
        assert_eq!(sns_wasm_id().to_text(), SNS_WASM_ID);
    }
}
