use super::*;
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
            relay_factory_enabled: Some(value.relay_factory_enabled),
            relay_setup_min_e8s: Some(value.relay_setup_min_e8s),
            relay_initial_cycles: Some(value.relay_initial_cycles),
            relay_cycle_safety_margin_e8s: Some(value.relay_cycle_safety_margin_e8s),
            relay_min_subaccount_one_seed_e8s: Some(value.relay_min_subaccount_one_seed_e8s),
            self_service_relay_interval_seconds: Some(value.self_service_relay_interval_seconds),
            canonical_relay_canister_id: Some(value.canonical_relay_canister_id),
            canonical_relay_targets: Some(value.canonical_relay_targets),
        }
    }
}

impl TryFrom<StableConfig> for Config {
    type Error = String;

    fn try_from(value: StableConfig) -> Result<Self, Self::Error> {
        let missing = |field| format!("historian restoration invariant: missing config.{field}");
        let interval = value
            .self_service_relay_interval_seconds
            .ok_or_else(|| missing("self_service_relay_interval_seconds"))?;
        if interval < 60 {
            return Err("historian restoration invariant: config.self_service_relay_interval_seconds must be at least 60".into());
        }
        Ok(Self {
            staking_account: value.staking_account,
            output_source_account: value
                .output_source_account
                .ok_or_else(|| missing("output_source_account"))?,
            output_account: value
                .output_account
                .ok_or_else(|| missing("output_account"))?,
            rewards_account: value
                .rewards_account
                .ok_or_else(|| missing("rewards_account"))?,
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            cmc_canister_id: Some(
                value
                    .cmc_canister_id
                    .ok_or_else(|| missing("cmc_canister_id"))?,
            ),
            faucet_canister_id: Some(
                value
                    .faucet_canister_id
                    .ok_or_else(|| missing("faucet_canister_id"))?,
            ),
            sns_wasm_canister_id: value.sns_wasm_canister_id,
            xrc_canister_id: value
                .xrc_canister_id
                .ok_or_else(|| missing("xrc_canister_id"))?,
            enable_sns_tracking: value.enable_sns_tracking,
            scan_interval_seconds: value.scan_interval_seconds,
            cycles_interval_seconds: value.cycles_interval_seconds,
            min_tx_e8s: value.min_tx_e8s,
            max_cycles_entries_per_canister: value.max_cycles_entries_per_canister,
            max_commitment_entries_per_canister: value.max_commitment_entries_per_canister,
            max_index_pages_per_tick: value.max_index_pages_per_tick,
            max_canisters_per_cycles_tick: value.max_canisters_per_cycles_tick,
            relay_factory_enabled: value
                .relay_factory_enabled
                .ok_or_else(|| missing("relay_factory_enabled"))?,
            relay_setup_min_e8s: value
                .relay_setup_min_e8s
                .ok_or_else(|| missing("relay_setup_min_e8s"))?,
            relay_initial_cycles: value
                .relay_initial_cycles
                .ok_or_else(|| missing("relay_initial_cycles"))?,
            relay_cycle_safety_margin_e8s: value
                .relay_cycle_safety_margin_e8s
                .ok_or_else(|| missing("relay_cycle_safety_margin_e8s"))?,
            relay_min_subaccount_one_seed_e8s: value
                .relay_min_subaccount_one_seed_e8s
                .ok_or_else(|| missing("relay_min_subaccount_one_seed_e8s"))?,
            self_service_relay_interval_seconds: interval,
            canonical_relay_canister_id: value
                .canonical_relay_canister_id
                .ok_or_else(|| missing("canonical_relay_canister_id"))?,
            canonical_relay_targets: value
                .canonical_relay_targets
                .ok_or_else(|| missing("canonical_relay_targets"))?,
        })
    }
}

impl From<CanisterMeta> for StableCanisterMeta {
    fn from(value: CanisterMeta) -> Self {
        Self {
            first_seen_ts: value.first_seen_ts,
            last_commitment_ts: value.last_commitment_ts,
            last_cycles_probe_ts: value.last_cycles_probe_ts,
            last_cycles_probe_result: value.last_cycles_probe_result,
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
        }
    }
}
