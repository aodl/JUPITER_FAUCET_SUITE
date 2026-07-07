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
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/self_service_relay.wasm"));
    (!bytes.is_empty()).then_some(bytes.as_slice())
}

pub(crate) fn approved_self_service_relay_wasm_hash_hex() -> Option<String> {
    use sha2::{Digest, Sha256};
    approved_self_service_relay_wasm().map(|bytes| hex::encode(Sha256::digest(bytes)))
}
