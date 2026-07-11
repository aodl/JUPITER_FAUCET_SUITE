mod clients;
mod logic;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};
use jupiter_ic_clients::constants;

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub managed_canisters: Vec<Principal>,
    pub ledger_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,
    pub blackhole_canister_id: Option<Principal>,
    pub main_interval_seconds: Option<u64>,
    pub max_transfers_per_tick: Option<u32>,
    pub surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    pub surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SurplusCanisterRecipient {
    pub canister_id: Principal,
    pub memo: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SurplusNeuronRecipient {
    pub neuron_id: u64,
    pub memo: Vec<u8>,
}

fn mainnet_ledger_id() -> Principal {
    constants::icp_ledger_id()
}

fn mainnet_cmc_id() -> Principal {
    constants::cycles_minting_canister_id()
}

fn mainnet_governance_id() -> Principal {
    constants::nns_governance_id()
}

fn mainnet_blackhole_id() -> Principal {
    constants::blackhole_canister_id()
}

#[cfg(any(test, feature = "debug_api"))]
fn production_canister_id() -> Principal {
    Principal::from_text(env!("JUPITER_RELAY_PROD_CANISTER_ID"))
        .expect("invalid embedded production canister principal")
}

#[cfg(any(test, feature = "debug_api"))]
fn is_production_canister(principal: Principal) -> bool {
    principal == production_canister_id()
}

#[cfg(feature = "debug_api")]
fn guard_debug_api_not_production() {
    if is_production_canister(ic_cdk::api::canister_self()) {
        ic_cdk::trap("debug_api is disabled for the production canister");
    }
}

fn self_canister_principal_for_validation() -> Principal {
    #[cfg(test)]
    {
        Principal::from_text("2vxsx-fae").unwrap_or_else(|_| Principal::anonymous())
    }
    #[cfg(not(test))]
    {
        ic_cdk::api::canister_self()
    }
}

fn validate_public_canister_target(canister_id: Principal) -> Result<(), String> {
    if canister_id == Principal::anonymous() {
        return Err("surplus_canister_recipients.canister_id must not be anonymous".to_string());
    }
    if canister_id == Principal::management_canister() {
        return Err(
            "surplus_canister_recipients.canister_id must not be the management canister"
                .to_string(),
        );
    }
    Ok(())
}

fn public_memo_to_internal(memo: Vec<u8>) -> Option<Vec<u8>> {
    if memo.is_empty() {
        None
    } else {
        Some(memo)
    }
}

fn surplus_canister_recipient_from_public(
    recipient: SurplusCanisterRecipient,
) -> Result<crate::state::SurplusRecipient, String> {
    validate_public_canister_target(recipient.canister_id)?;
    Ok(crate::state::SurplusRecipient {
        target: crate::state::SurplusTarget::Canister(recipient.canister_id),
        memo: public_memo_to_internal(recipient.memo),
    })
}

fn surplus_neuron_recipient_from_public(
    recipient: SurplusNeuronRecipient,
) -> crate::state::SurplusRecipient {
    crate::state::SurplusRecipient {
        target: crate::state::SurplusTarget::Neuron(recipient.neuron_id),
        memo: public_memo_to_internal(recipient.memo),
    }
}

#[cfg(feature = "debug_api")]
fn internal_memo_to_public(memo: Option<Vec<u8>>) -> Vec<u8> {
    memo.unwrap_or_default()
}

fn public_surplus_recipients_from_args(
    canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    neuron_recipients: Vec<SurplusNeuronRecipient>,
) -> Result<Vec<crate::state::SurplusRecipient>, String> {
    let mut recipients = Vec::new();
    for recipient in canister_recipients.unwrap_or_default() {
        recipients.push(surplus_canister_recipient_from_public(recipient)?);
    }
    recipients.extend(
        neuron_recipients
            .into_iter()
            .map(surplus_neuron_recipient_from_public),
    );
    Ok(recipients)
}

fn config_from_init_args(args: InitArgs) -> crate::state::Config {
    crate::state::Config {
        managed_canisters: args.managed_canisters,
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        cmc_canister_id: args.cmc_canister_id.unwrap_or_else(mainnet_cmc_id),
        governance_canister_id: args
            .governance_canister_id
            .unwrap_or_else(mainnet_governance_id),
        blackhole_canister_id: args
            .blackhole_canister_id
            .unwrap_or_else(mainnet_blackhole_id),
        main_interval_seconds: args.main_interval_seconds.unwrap_or(24 * 60 * 60).max(60),
        max_transfers_per_tick: args.max_transfers_per_tick,
        surplus_recipients: public_surplus_recipients_from_args(
            args.surplus_canister_recipients,
            args.surplus_neuron_recipients,
        )
        .expect("invalid relay surplus recipients"),
    }
}

#[cfg(feature = "debug_api")]
fn surplus_canister_recipients_to_public(
    recipients: &[crate::state::SurplusRecipient],
) -> Option<Vec<SurplusCanisterRecipient>> {
    let canister_recipients = recipients
        .iter()
        .filter_map(|recipient| match recipient.target {
            crate::state::SurplusTarget::Canister(canister_id) => Some(SurplusCanisterRecipient {
                canister_id,
                memo: internal_memo_to_public(recipient.memo.clone()),
            }),
            crate::state::SurplusTarget::Neuron(_) => None,
        })
        .collect::<Vec<_>>();
    if canister_recipients.is_empty() {
        None
    } else {
        Some(canister_recipients)
    }
}

#[cfg(feature = "debug_api")]
fn surplus_neuron_recipients_to_public(
    recipients: &[crate::state::SurplusRecipient],
) -> Vec<SurplusNeuronRecipient> {
    recipients
        .iter()
        .filter_map(|recipient| match recipient.target {
            crate::state::SurplusTarget::Canister(_) => None,
            crate::state::SurplusTarget::Neuron(neuron_id) => Some(SurplusNeuronRecipient {
                neuron_id,
                memo: internal_memo_to_public(recipient.memo.clone()),
            }),
        })
        .collect()
}

fn initialize_from_config(cfg: crate::state::Config, lifecycle_event: &'static str) {
    let now_secs = ic_cdk::api::time() / 1_000_000_000;
    crate::logic::validate_config(&cfg, self_canister_principal_for_validation())
        .expect("invalid relay config");
    crate::state::set_state(crate::state::State::new(cfg, now_secs));
    crate::scheduler::install_timers();
    crate::scheduler::schedule_startup_liveness_tick();
    let main_interval_seconds = crate::state::with_state(|st| st.config.main_interval_seconds);
    crate::scheduler::log_lifecycle(lifecycle_event, main_interval_seconds, None, None);
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    initialize_from_args(args, "init_complete");
}

fn initialize_from_args(args: InitArgs, lifecycle_event: &'static str) {
    initialize_from_config(config_from_init_args(args), lifecycle_event);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(args: InitArgs) {
    initialize_from_args(args, "post_upgrade_complete");
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub last_main_run_ts: u64,
    pub main_lock_state_ts: Option<u64>,
    pub active_job_present: bool,
    pub active_job_pending_transfer_present: bool,
    pub active_faucet_commitment_transfer_present: bool,
    pub last_summary_present: bool,
    pub next_job_id: u64,
    pub last_completed_cycles_count: u32,
    pub relay_minted_cycles_since_sample_count: u32,
    pub recovery_deficit_cycles_count: u32,
    pub consecutive_probe_failures: Vec<(Principal, u32)>,
    pub conversion_estimate_present: bool,
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugConfig {
    pub managed_canisters: Vec<Principal>,
    pub effective_managed_canisters: Vec<Principal>,
    pub ledger_canister_id: Principal,
    pub cmc_canister_id: Principal,
    pub governance_canister_id: Principal,
    pub blackhole_canister_id: Principal,
    pub main_interval_seconds: u64,
    pub max_transfers_per_tick: Option<u32>,
    pub surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    pub surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    guard_debug_api_not_production();
    crate::state::with_state(|st| DebugState {
        last_main_run_ts: st.last_main_run_ts,
        main_lock_state_ts: st.main_lock_state_ts,
        active_job_present: st.active_job.is_some(),
        active_job_pending_transfer_present: st
            .active_job
            .as_ref()
            .and_then(|job| job.pending_transfer.as_ref())
            .is_some(),
        active_faucet_commitment_transfer_present: st.active_faucet_commitment_transfer.is_some(),
        last_summary_present: st.last_summary.is_some(),
        next_job_id: st.next_job_id,
        last_completed_cycles_count: st.last_completed_cycles.len() as u32,
        relay_minted_cycles_since_sample_count: st.relay_minted_cycles_since_sample.len() as u32,
        recovery_deficit_cycles_count: st.recovery_deficit_cycles.len() as u32,
        consecutive_probe_failures: st
            .consecutive_probe_failures
            .iter()
            .map(|(canister_id, count)| (*canister_id, *count))
            .collect(),
        conversion_estimate_present: st.conversion_estimate.is_some(),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_config() -> DebugConfig {
    guard_debug_api_not_production();
    crate::state::with_state(|st| DebugConfig {
        managed_canisters: st.config.managed_canisters.clone(),
        effective_managed_canisters: crate::logic::effective_managed_canisters(
            &st.config.managed_canisters,
            ic_cdk::api::canister_self(),
        ),
        ledger_canister_id: st.config.ledger_canister_id,
        cmc_canister_id: st.config.cmc_canister_id,
        governance_canister_id: st.config.governance_canister_id,
        blackhole_canister_id: st.config.blackhole_canister_id,
        main_interval_seconds: st.config.main_interval_seconds,
        max_transfers_per_tick: st.config.max_transfers_per_tick,
        surplus_canister_recipients: surplus_canister_recipients_to_public(
            &st.config.surplus_recipients,
        ),
        surplus_neuron_recipients: surplus_neuron_recipients_to_public(
            &st.config.surplus_recipients,
        ),
    })
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_main_tick() {
    guard_debug_api_not_production();
    crate::scheduler::debug_main_tick_impl().await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_last_summary() -> Option<crate::state::RelaySummary> {
    guard_debug_api_not_production();
    crate::state::with_state(|st| st.last_summary.clone())
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_set_cycles_sample(canister_id: Principal, cycles: u128) {
    guard_debug_api_not_production();
    let now = ic_cdk::api::time() as u64;
    crate::state::with_state_mut(|st| {
        st.last_completed_cycles.insert(
            canister_id,
            crate::state::CyclesSnapshot {
                cycles,
                timestamp_nanos: now,
                source: crate::logic::sample_source_for(canister_id, ic_cdk::api::canister_self()),
            },
        );
    });
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_force_clear_active_job() {
    guard_debug_api_not_production();
    crate::state::with_state_mut(|st| st.active_job = None);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_abort_after_successful_transfer(v: bool) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_abort_after_successful_transfer(v);
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
fn debug_trap_after_successful_transfer(v: bool) {
    guard_debug_api_not_production();
    crate::scheduler::debug_set_trap_after_successful_transfer(v);
}

ic_cdk::export_candid!();

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{decode_args, encode_args};
    use candid_parser::parse_idl_args;
    use candid_parser::utils::{instantiate_candid, service_equal, CandidSource};
    use std::path::Path;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn sample_init_args() -> InitArgs {
        InitArgs {
            managed_canisters: vec![principal("22255-zqaaa-aaaas-qf6uq-cai")],
            ledger_canister_id: None,
            cmc_canister_id: None,
            governance_canister_id: None,
            blackhole_canister_id: None,
            main_interval_seconds: Some(12),
            max_transfers_per_tick: Some(3),
            surplus_canister_recipients: None,
            surplus_neuron_recipients: vec![SurplusNeuronRecipient {
                neuron_id: 42,
                memo: Vec::new(),
            }],
        }
    }

    fn assert_committed_did_matches_rust_service(did_file: &str) {
        let did_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(did_file);
        service_equal(
            CandidSource::Text(&__export_service()),
            CandidSource::File(&did_path),
        )
        .unwrap_or_else(|err| {
            panic!("committed relay DID {did_file} diverged from Rust service: {err}")
        });
    }

    #[cfg(not(feature = "debug_api"))]
    #[test]
    fn committed_production_did_matches_rust_service() {
        assert_committed_did_matches_rust_service("jupiter_relay.did");
    }

    #[cfg(not(feature = "debug_api"))]
    #[test]
    fn production_did_exposes_empty_service() {
        let did = include_str!("../jupiter_relay.did");
        assert!(did.trim_end().ends_with("service : (InitArgs) -> {}"));
        assert!(!did.contains(concat!("Relay", "Status")));
        assert!(!did.contains(concat!("relay_", "status")));
        assert!(!did.contains("admin_schedule_main_tick_now"));
    }

    #[cfg(feature = "debug_api")]
    #[test]
    fn committed_debug_did_matches_rust_service() {
        assert_committed_did_matches_rust_service("jupiter_relay_debug.did");
    }

    #[cfg(feature = "debug_api")]
    #[test]
    fn debug_did_does_not_expose_status_or_admin_endpoint() {
        let did = include_str!("../jupiter_relay_debug.did");
        assert!(!did.contains(concat!("Relay", "Status")));
        assert!(!did.contains(concat!("relay_", "status")));
        assert!(!did.contains("admin_schedule_main_tick_now"));
    }

    #[test]
    fn mainnet_install_args_preflight_decodes_to_rust_init_args() {
        let did = include_str!("../jupiter_relay.did");
        let install_args = include_str!("../mainnet-install-args.did");
        let (init_types, (env, _service_type)) =
            instantiate_candid(CandidSource::Text(did)).expect("relay DID should expose init args");
        let parsed_args = parse_idl_args(install_args).expect("mainnet install args should parse");
        let bytes = parsed_args
            .to_bytes_with_types(&env, &init_types)
            .expect("mainnet install args should encode against relay DID");
        let (args,): (InitArgs,) =
            decode_args(&bytes).expect("mainnet install args should decode into Rust InitArgs");

        assert!(!args.managed_canisters.is_empty());
        assert!(args
            .managed_canisters
            .contains(&Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").unwrap()));
        assert!(args
            .managed_canisters
            .contains(&Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").unwrap()));
        assert!(args.surplus_canister_recipients.is_none());
        assert_eq!(args.surplus_neuron_recipients.len(), 2);
        assert_eq!(
            args.surplus_neuron_recipients[0].neuron_id,
            10_292_412_127_977_304_661
        );
        assert!(args.surplus_neuron_recipients[0].memo.is_empty());
        assert_eq!(
            args.surplus_neuron_recipients[1].neuron_id,
            11_614_578_985_374_291_210
        );
        assert_eq!(
            args.surplus_neuron_recipients[1].memo.as_slice(),
            b"10292412127977304661"
        );

        let roundtrip = encode_args((args,)).expect("decoded Rust InitArgs should re-encode");
        let (_decoded_again,): (InitArgs,) =
            decode_args(&roundtrip).expect("roundtripped Rust InitArgs should decode");
    }

    #[test]
    fn init_args_surplus_conversion_preserves_canister_target_and_memo() {
        let owner = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let converted = surplus_canister_recipient_from_public(SurplusCanisterRecipient {
            canister_id: owner,
            memo: vec![1, 2, 3],
        })
        .unwrap();
        assert_eq!(
            converted.target,
            crate::state::SurplusTarget::Canister(owner)
        );
        assert_eq!(converted.memo, Some(vec![1, 2, 3]));
    }

    #[test]
    fn public_surplus_conversion_preserves_neuron_target_and_memo() {
        let converted = surplus_neuron_recipient_from_public(SurplusNeuronRecipient {
            neuron_id: 42,
            memo: vec![4, 5],
        });
        assert_eq!(converted.target, crate::state::SurplusTarget::Neuron(42));
        assert_eq!(converted.memo, Some(vec![4, 5]));
    }

    #[test]
    fn public_surplus_conversion_maps_empty_memo_to_none() {
        let owner = principal("22255-zqaaa-aaaas-qf6uq-cai");
        let canister = surplus_canister_recipient_from_public(SurplusCanisterRecipient {
            canister_id: owner,
            memo: Vec::new(),
        })
        .unwrap();
        assert_eq!(canister.memo, None);

        let neuron = surplus_neuron_recipient_from_public(SurplusNeuronRecipient {
            neuron_id: 42,
            memo: Vec::new(),
        });
        assert_eq!(neuron.memo, None);
    }

    #[test]
    fn config_from_init_args_uses_full_init_args_for_fresh_state() {
        let cfg = config_from_init_args(sample_init_args());
        assert_eq!(
            cfg.managed_canisters,
            vec![principal("22255-zqaaa-aaaas-qf6uq-cai")]
        );
        assert_eq!(cfg.main_interval_seconds, 60);
        assert_eq!(cfg.max_transfers_per_tick, Some(3));
        assert_eq!(cfg.surplus_recipients.len(), 1);
        assert_eq!(
            cfg.surplus_recipients[0].target,
            crate::state::SurplusTarget::Neuron(42)
        );
    }

    #[test]
    fn fresh_state_from_full_init_args_has_empty_runtime_accounting() {
        let cfg = config_from_init_args(sample_init_args());
        let st = crate::state::State::new(cfg, 1_000);
        assert!(st.last_completed_cycles.is_empty());
        assert!(st.relay_minted_cycles_since_sample.is_empty());
        assert!(st.recovery_deficit_cycles.is_empty());
        assert!(st.consecutive_probe_failures.is_empty());
        assert!(st.conversion_estimate.is_none());
        assert!(st.active_job.is_none());
        assert!(st.active_faucet_commitment_transfer.is_none());
        assert!(st.last_summary.is_none());
        assert_eq!(st.next_job_id, 1);
    }

    #[test]
    fn public_surplus_conversion_rejects_invalid_canister_targets() {
        let anonymous = surplus_canister_recipient_from_public(SurplusCanisterRecipient {
            canister_id: Principal::anonymous(),
            memo: Vec::new(),
        });
        assert!(anonymous.unwrap_err().contains("anonymous"));

        let management = surplus_canister_recipient_from_public(SurplusCanisterRecipient {
            canister_id: Principal::management_canister(),
            memo: Vec::new(),
        });
        assert!(management.unwrap_err().contains("management canister"));
    }

    #[test]
    fn production_relay_principal_is_recognized() {
        let prod = Principal::from_text("u2qkp-aqaaa-aaaar-qb7ea-cai").unwrap();
        assert_eq!(production_canister_id(), prod);
        assert!(is_production_canister(prod));
        assert!(!is_production_canister(Principal::anonymous()));
    }
}
