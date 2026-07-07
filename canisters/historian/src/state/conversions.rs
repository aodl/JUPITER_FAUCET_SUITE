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
            relay_factory_enabled: Some(value.relay_factory_enabled),
            relay_setup_min_e8s: Some(value.relay_setup_min_e8s),
            relay_setup_dust_e8s: Some(value.relay_setup_dust_e8s),
            relay_setup_refund_cooldown_seconds: Some(value.relay_setup_refund_cooldown_seconds),
            relay_initial_cycles: Some(value.relay_initial_cycles),
            relay_cycle_safety_margin_e8s: Some(value.relay_cycle_safety_margin_e8s),
            relay_min_subaccount_one_seed_e8s: Some(value.relay_min_subaccount_one_seed_e8s),
            self_service_relay_interval_seconds: Some(value.self_service_relay_interval_seconds),
            self_service_relay_max_transfers_per_tick: Some(
                value.self_service_relay_max_transfers_per_tick,
            ),
            io_surplus_neuron_id: Some(value.io_surplus_neuron_id),
            canonical_relay_canister_id: Some(value.canonical_relay_canister_id),
            canonical_relay_targets: Some(value.canonical_relay_targets),
        }
    }
}

impl From<StableConfig> for Config {
    fn from(value: StableConfig) -> Self {
        Self {
            staking_account: value.staking_account,
            output_source_account: value
                .output_source_account
                .unwrap_or_else(crate::mainnet_disburser_staging_account),
            output_account: value
                .output_account
                .unwrap_or_else(crate::mainnet_output_account),
            rewards_account: value
                .rewards_account
                .unwrap_or_else(crate::mainnet_rewards_account),
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
            relay_factory_enabled: value.relay_factory_enabled.unwrap_or(false),
            relay_setup_min_e8s: value.relay_setup_min_e8s.unwrap_or(200_000_000),
            relay_setup_dust_e8s: value.relay_setup_dust_e8s.unwrap_or(10_000),
            relay_setup_refund_cooldown_seconds: value
                .relay_setup_refund_cooldown_seconds
                .unwrap_or(300),
            relay_initial_cycles: value.relay_initial_cycles.unwrap_or(1_000_000_000_000),
            relay_cycle_safety_margin_e8s: value.relay_cycle_safety_margin_e8s.unwrap_or(5_000_000),
            relay_min_subaccount_one_seed_e8s: value
                .relay_min_subaccount_one_seed_e8s
                .unwrap_or(100_020_000),
            self_service_relay_interval_seconds: value
                .self_service_relay_interval_seconds
                .unwrap_or(3600),
            self_service_relay_max_transfers_per_tick: value
                .self_service_relay_max_transfers_per_tick
                .unwrap_or(Some(10)),
            io_surplus_neuron_id: value
                .io_surplus_neuron_id
                .unwrap_or(crate::DEFAULT_IO_SURPLUS_NEURON_ID),
            canonical_relay_canister_id: value
                .canonical_relay_canister_id
                .unwrap_or_else(|| Some(crate::mainnet_relay_id())),
            canonical_relay_targets: value
                .canonical_relay_targets
                .unwrap_or_else(crate::mainnet_canonical_relay_targets),
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
