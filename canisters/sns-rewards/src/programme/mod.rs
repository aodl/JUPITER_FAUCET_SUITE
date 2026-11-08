pub(crate) mod automation;
mod discovery;
mod governance_client;
mod patron;
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
        let patron = patron::tick(now).await;
        let governance = automation::tick(now).await;
        let participation = participation::tick().await;
        discovery
            .and(reserve)
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

pub fn context() -> Option<crate::RelayRewardContext> {
    let state = read();
    let config = state.config?;
    let snapshot = state.snapshot?;
    if !snapshot.valid_for(&config, ic_cdk::api::time() / 1_000_000_000) {
        return None;
    }
    let source = &snapshot.sources[0]; // Always jUP, even for an IO-only owner.
    Some(crate::RelayRewardContext {
        sns_root_canister_id: source.root,
        sns_governance_canister_id: source.governance,
        sns_ledger_canister_id: source.ledger,
        snapshot_id: snapshot.id,
        scan_started_at_timestamp_nanos: snapshot.started_at_seconds.saturating_mul(1_000_000_000),
        scan_completed_at_timestamp_nanos: snapshot
            .completed_at_seconds
            .saturating_mul(1_000_000_000),
    })
}

pub fn resolve(
    args: crate::ResolveDefaultIcpAccountsArgs,
) -> crate::ResolveDefaultIcpAccountsResult {
    use crate::ResolveDefaultIcpAccountsResult as Reply;
    if args.account_identifiers.len() > crate::policy::MAX_ACCOUNT_LOOKUPS_PER_CALL {
        return Reply::TooManyAccounts;
    }
    for (index, account) in args.account_identifiers.iter().enumerate() {
        if account.len() != 32 {
            return Reply::InvalidAccountIdentifier {
                index: index as u32,
            };
        }
    }
    let state = read();
    let Some(snapshot) = state.snapshot else {
        return Reply::SnapshotChanged;
    };
    let Some(config) = state.config else {
        return Reply::SnapshotChanged;
    };
    if snapshot.id != args.snapshot_id
        || !snapshot.valid_for(&config, ic_cdk::api::time() / 1_000_000_000)
    {
        return Reply::SnapshotChanged;
    }
    Reply::Ok(
        args.account_identifiers
            .into_iter()
            .map(|account| {
                storage::lookup(
                    snapshot.slot,
                    account.try_into().expect("validated account"),
                )
                .map(|owner| owner.principal)
            })
            .collect(),
    )
}
