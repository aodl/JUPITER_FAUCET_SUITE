mod types;
use types::*;
pub(crate) use types::*;
mod stable;
use stable::*;
pub(crate) use stable::{with_relay_setup_entries_map, DIRTY_REGISTRY, DIRTY_ROOT};
mod commitments;
use commitments::*;
mod commitment_route_rollups;
pub(crate) use commitment_route_rollups::*;
mod routes;
use routes::*;
mod cycles;
use cycles::*;
mod snapshots;
use snapshots::*;
mod restore;
pub(crate) use restore::*;
mod access;
pub(crate) use access::*;
mod conversions;
#[cfg(test)]
mod tests;

pub(crate) fn commitment_index_is_complete(st: &State) -> bool {
    st.commitment_route_rollups_complete_from_genesis == Some(true)
        && st.staking_index_descending != Some(false)
        && st.active_staking_catch_up.is_none()
        && st.commitment_index_fault.is_none()
}
