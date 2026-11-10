pub(crate) mod automation;
mod discovery;
mod governance_client;
mod patron;
pub(crate) mod refresh;
#[cfg(feature = "debug_api")]
pub(crate) use patron::trap_callback_once;
pub(crate) mod participation;
pub(crate) mod reserve;
mod storage;
#[cfg(test)]
mod tests;
pub(crate) mod treasury;
pub mod types;

use std::cell::Cell;
pub use storage::{configure, read, record};

/// Keep durable review evidence bounded even if a remote reject/decode message
/// contains a full response dump. Preserve a UTF-8 prefix and full-message hash.
fn compact(message: &str) -> String {
    if message.len() <= 2048 {
        return message.to_owned();
    }
    use sha2::{Digest, Sha256};
    let mut end = 1920;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{} [truncated; sha256={}]",
        &message[..end],
        hex::encode(Sha256::digest(message.as_bytes()))
    )
}
use types::*;

thread_local! { static BUSY: Cell<bool> = const { Cell::new(false) }; }
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.with(|busy| busy.set(false));
    }
}

pub fn install_timer() {
    ic_cdk_timers::set_timer_interval(std::time::Duration::from_secs(30), tick);
}

pub async fn tick() {
    if read().config.is_none() {
        return; // Existing installations do no new work or journal writes.
    }
    if BUSY.with(|busy| busy.replace(true)) {
        return;
    }
    let _guard = Guard;
    let now = ic_cdk::api::time() / 1_000_000_000;
    let result = async {
        let discovery = discovery::tick(now).await;
        let reserve = reserve::tick().await;
        let refresh = refresh::tick(now).await;
        let patron = patron::tick(now).await;
        let governance = automation::tick(now).await;
        let participation = participation::tick().await;
        discovery
            .and(reserve)
            .and(refresh)
            .and(patron)
            .and(governance)
            .and(participation)
    }
    .await;
    storage::mutate(|s| s.last_error = result.err());
}

pub fn recover_dispatch() {
    if let Some(block) = read().pending_block {
        if let Some(mut record) = record(block) {
            if matches!(record.outcome, PatronOutcome::Dispatched { .. }) {
                record.outcome = PatronOutcome::AssumedPaidUnverified {
                    reason:
                        "upgrade interrupted the dispatched patron callback; no automatic replay"
                            .into(),
                };
                storage::put(&record);
                patron::finish(&record);
            }
        }
    }
}

/// One monotonically increasing namespace for independent Relay and patron
/// snapshots. No await separates allocation from publication in either scanner.
pub(crate) fn allocate_snapshot_id() -> u64 {
    let id = crate::state::with_state(|s| s.next_snapshot_id).max(
        read()
            .next_snapshot
            .checked_add(1)
            .expect("snapshot generation exhausted"),
    );
    let next = id.checked_add(1).expect("snapshot generation exhausted");
    crate::state::with_state_mut(|s| s.next_snapshot_id = next);
    id
}

pub(crate) fn bind_relay_root() {
    if let Some(config) = read().config {
        if crate::state::with_state(|s| s.config.reward_sns_root_canister_id)
            != Some(config.jup_root)
        {
            crate::state::invalidate_for_root(Some(config.jup_root));
        }
    }
}
