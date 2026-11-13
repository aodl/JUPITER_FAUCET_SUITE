// Preserve the official generated DTO graph rather than hand-editing its variants.
#![allow(clippy::large_enum_variant)]
use candid::{CandidType, Deserialize, Principal};
use serde::Serialize;
include!("sns.rs");

mod permissions;
pub use permissions::{NeuronPermissionType, GOVERNANCE_ERROR_NOT_FOUND};
