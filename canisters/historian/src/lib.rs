mod clients;
mod logic;
mod scheduler;
mod state;
pub(crate) use state::*;
mod relay_setup;
use jupiter_ic_clients::event_horizon::EventHorizonPokeNotification;

mod normalization;
pub(crate) use normalization::*;
mod api;
pub use api::*;
mod lifecycle;
pub(crate) use lifecycle::*;
mod read_model;
#[cfg(test)]
pub(crate) use read_model::*;
#[cfg(any(test, feature = "debug_api"))]
mod debug;
#[cfg(feature = "debug_api")]
pub use debug::*;
#[cfg(test)]
mod lib_tests;

pub(crate) fn approved_self_service_relay_wasm() -> Option<&'static [u8]> {
    // The embedded install payload may be gzip-compressed. The management canister accepts
    // compressed Wasm in install_code and installs the decompressed module.
    #[cfg(test)]
    {
        Some(b"jupiter-historian-test-relay-wasm")
    }
    #[cfg(not(test))]
    {
        let bytes = include_bytes!(concat!(
            env!("OUT_DIR"),
            "/self_service_relay_install_payload.wasm"
        ));
        (!bytes.is_empty()).then_some(bytes.as_slice())
    }
}

pub(crate) fn approved_relay_raw_wasm_hash_hex() -> Option<String> {
    // Reviewed raw relay Wasm hash from release-artifacts/jupiter_relay.wasm.
    // This is review evidence. Runtime module-hash reconciliation uses the compressed
    // install payload hash because Historian passes release-artifacts/jupiter_relay.wasm.gz
    // bytes to install_code.
    #[cfg(test)]
    {
        use sha2::{Digest, Sha256};
        Some(hex::encode(Sha256::digest(
            b"jupiter-historian-test-relay-raw-wasm",
        )))
    }
    #[cfg(not(test))]
    {
        let hash = option_env!("JUPITER_RELAY_RAW_WASM_SHA256")?;
        approved_self_service_relay_wasm().and_then(|_| {
            (hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| hash.to_ascii_lowercase())
        })
    }
}

pub(crate) fn approved_relay_install_payload_hash_hex() -> Option<String> {
    // Reviewed compressed relay install payload hash from release-artifacts/jupiter_relay.wasm.gz.
    // This is the hash expected from canister_status.module_hash for self-service relays
    // installed from the compressed payload.
    #[cfg(test)]
    {
        use sha2::{Digest, Sha256};
        approved_self_service_relay_wasm().map(|bytes| hex::encode(Sha256::digest(bytes)))
    }
    #[cfg(not(test))]
    {
        let hash = option_env!("JUPITER_RELAY_GZ_WASM_SHA256")?;
        approved_self_service_relay_wasm().and_then(|_| {
            (hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| hash.to_ascii_lowercase())
        })
    }
}

pub(crate) fn approved_relay_onchain_module_hash() -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    approved_self_service_relay_wasm().map(|bytes| Sha256::digest(bytes).into())
}

// Exact predecessor payload retained for already-paid configurations. This is
// separately approved installation identity, never an arbitrary caller module.
pub(crate) fn relay_install_payload_for_hash(hash: &[u8]) -> Option<&'static [u8]> {
    if approved_relay_onchain_module_hash()
        .as_ref()
        .map(|h| h.as_slice())
        == Some(hash)
    {
        return approved_self_service_relay_wasm();
    }
    (hex::encode(hash) == "221765e5940e3d0721dac7677f0294e05f616a6d91385f984c59a34ceb71cde0")
        .then_some(include_bytes!("../relay-install/legacy-relay.wasm.gz").as_slice())
}

ic_cdk::export_candid!();
