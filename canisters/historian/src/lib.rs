mod clients;
mod logic;
mod scheduler;
mod state;
pub(crate) use state::*;
mod relay_setup;

mod normalization;
pub(crate) use normalization::*;
mod api;
pub use api::*;
mod lifecycle;
pub(crate) use lifecycle::*;
mod read_model;
#[cfg(test)]
pub(crate) use read_model::*;
#[cfg(feature = "debug_api")]
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
        return Some(b"jupiter-historian-test-relay-wasm");
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

pub(crate) fn approved_self_service_relay_wasm_hash_hex() -> Option<String> {
    // This is the reviewed raw relay Wasm hash, not the hash of the embedded install payload.
    // Module-hash reconciliation compares canister_status.module_hash with this raw hash.
    #[cfg(test)]
    {
        use sha2::{Digest, Sha256};
        return approved_self_service_relay_wasm().map(|bytes| hex::encode(Sha256::digest(bytes)));
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

pub(crate) fn approved_relay_wasm_hash() -> Option<[u8; 32]> {
    #[cfg(test)]
    {
        use sha2::{Digest, Sha256};
        return approved_self_service_relay_wasm().map(|wasm| Sha256::digest(wasm).into());
    }
    #[cfg(not(test))]
    {
        let hash = approved_self_service_relay_wasm_hash_hex()?;
        let bytes = hex::decode(hash).ok()?;
        bytes.try_into().ok()
    }
}
