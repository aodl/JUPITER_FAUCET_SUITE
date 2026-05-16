impl From<Config> for StableConfig {
    fn from(value: Config) -> Self {
        Self {
            staking_account: value.staking_account,
            output_source_account: Some(value.output_source_account),
            output_account: Some(value.output_account),
            rewards_account: Some(value.rewards_account),
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: value.cmc_canister_id,
            faucet_canister_id: value.faucet_canister_id,
            blackhole_canister_id: value.blackhole_canister_id,
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            xrc_canister_id: Some(value.xrc_canister_id),
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_commitment_entries_per_canister: value.max_commitment_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
        }
    }
}

impl From<StableConfig> for Config {
    fn from(value: StableConfig) -> Self {
        Self {
            staking_account: value.staking_account,
            output_source_account: value.output_source_account.unwrap_or_else(crate::mainnet_disburser_staging_account),
            output_account: value.output_account.unwrap_or_else(crate::mainnet_output_account),
            rewards_account: value.rewards_account.unwrap_or_else(crate::mainnet_rewards_account),
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: value.cmc_canister_id,
            faucet_canister_id: value.faucet_canister_id,
            blackhole_canister_id: value.blackhole_canister_id,
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            xrc_canister_id: value.xrc_canister_id.unwrap_or_else(crate::mainnet_xrc_id),
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_commitment_entries_per_canister: value.max_commitment_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
        }
    }
}

impl From<CanisterMeta> for StableCanisterMeta {
    fn from(value: CanisterMeta) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_commitment_ts: value.last_commitment_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result,
            last_burn_tx_id: value.last_burn_tx_id,
            last_burn_scan_tx_id: value.last_burn_scan_tx_id,
            burned_e8s: Some(value.burned_e8s),
        }
    }
}

impl From<StableCanisterMeta> for CanisterMeta {
    fn from(value: StableCanisterMeta) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_commitment_ts: value.last_commitment_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result,
            last_burn_tx_id: value.last_burn_tx_id,
            last_burn_scan_tx_id: value.last_burn_scan_tx_id.or(value.last_burn_tx_id),
            burned_e8s: value.burned_e8s.unwrap_or(0),
        }
    }
}


