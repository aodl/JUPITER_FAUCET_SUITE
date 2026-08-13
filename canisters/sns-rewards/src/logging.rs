use candid::Principal;

use crate::state::{self, OwnerScan, OwnerSnapshot};

fn principal(value: Option<Principal>) -> String {
    value
        .map(|p| p.to_text())
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn runtime_config_log_line(config: &state::Config) -> String {
    format!(
        "SNS_REWARDS_CONFIG reward_sns_root_canister_id={}",
        principal(config.reward_sns_root_canister_id)
    )
}

pub(crate) fn config() {
    let line = state::with_state(|st| runtime_config_log_line(&st.config));
    ic_cdk::println!("{}", line);
}

fn cursor(value: Option<&[u8]>) -> String {
    value.map(hex::encode).unwrap_or_else(|| "null".to_string())
}

pub(crate) fn scan(
    status: &str,
    scan: Option<&OwnerScan>,
    snapshot: Option<&OwnerSnapshot>,
    reason: Option<&str>,
) {
    let root = scan
        .map(|s| s.sns_root_canister_id)
        .or_else(|| snapshot.map(|s| s.sns_root_canister_id));
    let governance = scan
        .map(|s| s.sns_governance_canister_id)
        .or_else(|| snapshot.map(|s| s.sns_governance_canister_id));
    let ledger = scan
        .map(|s| s.sns_ledger_canister_id)
        .or_else(|| snapshot.map(|s| s.sns_ledger_canister_id));
    let page = scan.map(|s| s.pages_processed).unwrap_or(0);
    let neurons = scan
        .map(|s| s.neurons_processed)
        .or_else(|| snapshot.map(|s| s.neuron_count))
        .unwrap_or(0);
    let owners = scan
        .map(|s| state::slot_len(s.staging_slot))
        .or_else(|| snapshot.map(|s| s.owner_count))
        .unwrap_or(0);
    let snapshot_id = snapshot
        .map(|s| s.snapshot_id)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string());
    let reason = reason
        .map(jupiter_canister_logging::escape_value)
        .unwrap_or_else(|| "null".to_string());
    ic_cdk::println!(
        "SNS_REWARD_SCAN status={} root={} governance={} ledger={} snapshot_id={} page={} neurons_processed={} owners_indexed={} cursor={} reason={}",
        status,
        principal(root),
        principal(governance),
        principal(ledger),
        snapshot_id,
        page,
        neurons,
        owners,
        cursor(scan.and_then(|s| s.start_page_at.as_deref())),
        reason
    );
}

pub(crate) fn lifecycle(event: &str) {
    let root = state::with_state(|st| st.config.reward_sns_root_canister_id);
    ic_cdk::println!(
        "SNS_REWARDS_LIFECYCLE event={} reward_sns_root_canister_id={} timers_installed=true",
        event,
        principal(root)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_log_is_complete_for_configured_root() {
        let root = Principal::from_slice(&[1, 2, 3]);
        let config = state::Config {
            reward_sns_root_canister_id: Some(root),
        };

        assert_eq!(
            runtime_config_log_line(&config),
            format!(
                "SNS_REWARDS_CONFIG reward_sns_root_canister_id={}",
                root.to_text()
            )
        );
    }

    #[test]
    fn runtime_config_log_is_complete_for_unconfigured_root() {
        assert_eq!(
            runtime_config_log_line(&state::Config::default()),
            "SNS_REWARDS_CONFIG reward_sns_root_canister_id=null"
        );
    }
}
