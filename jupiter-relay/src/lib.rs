mod clients;
mod logic;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};

#[derive(CandidType, Deserialize, Clone)]
pub struct InitArgs {
    pub managed_canisters: Vec<Principal>,
    pub ledger_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,
    pub blackhole_canister_id: Option<Principal>,
    pub main_interval_seconds: Option<u64>,
    pub max_transfers_per_tick: Option<u32>,
    pub surplus_recipients: Vec<SurplusRecipient>,
}

#[derive(CandidType, Deserialize, Clone)]
pub struct SurplusRecipient {
    pub target: SurplusTarget,
    pub memo: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Clone)]
pub enum SurplusTarget {
    Canister(Principal),
    Neuron(u64),
}

#[derive(CandidType, Deserialize, Clone)]
pub struct UpgradeArgs {
    pub managed_canisters: Option<Vec<Principal>>,
    pub ledger_canister_id: Option<Principal>,
    pub cmc_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,
    pub blackhole_canister_id: Option<Principal>,
    pub main_interval_seconds: Option<u64>,
    pub max_transfers_per_tick: Option<Option<u32>>,
    pub surplus_recipients: Option<Vec<SurplusRecipient>>,
}

fn mainnet_ledger_id() -> Principal {
    Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid hardcoded ledger principal")
}

fn mainnet_cmc_id() -> Principal {
    Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").expect("invalid hardcoded cmc principal")
}

fn mainnet_governance_id() -> Principal {
    Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai")
        .expect("invalid hardcoded governance principal")
}

fn mainnet_blackhole_id() -> Principal {
    Principal::from_text("77deu-baaaa-aaaar-qb6za-cai")
        .expect("invalid hardcoded blackhole principal")
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

fn surplus_recipient_from_public(recipient: SurplusRecipient) -> crate::state::SurplusRecipient {
    crate::state::SurplusRecipient {
        target: match recipient.target {
            SurplusTarget::Canister(canister_id) => crate::state::SurplusTarget::Canister(canister_id),
            SurplusTarget::Neuron(neuron_id) => crate::state::SurplusTarget::Neuron(neuron_id),
        },
        memo: recipient.memo,
    }
}

fn apply_upgrade_args(st: &mut crate::state::State, args: UpgradeArgs) {
    st.config = crate::state::Config {
        managed_canisters: args
            .managed_canisters
            .unwrap_or_else(|| st.config.managed_canisters.clone()),
        ledger_canister_id: args
            .ledger_canister_id
            .unwrap_or(st.config.ledger_canister_id),
        cmc_canister_id: args.cmc_canister_id.unwrap_or(st.config.cmc_canister_id),
        governance_canister_id: args
            .governance_canister_id
            .unwrap_or(st.config.governance_canister_id),
        blackhole_canister_id: args
            .blackhole_canister_id
            .unwrap_or(st.config.blackhole_canister_id),
        main_interval_seconds: args
            .main_interval_seconds
            .unwrap_or(st.config.main_interval_seconds)
            .max(60),
        max_transfers_per_tick: args
            .max_transfers_per_tick
            .unwrap_or(st.config.max_transfers_per_tick),
        surplus_recipients: args
            .surplus_recipients
            .map(|recipients| {
                recipients
                    .into_iter()
                    .map(surplus_recipient_from_public)
                    .collect()
            })
            .unwrap_or_else(|| st.config.surplus_recipients.clone()),
    };
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    let cfg = crate::state::Config {
        managed_canisters: args.managed_canisters,
        ledger_canister_id: args.ledger_canister_id.unwrap_or_else(mainnet_ledger_id),
        cmc_canister_id: args.cmc_canister_id.unwrap_or_else(mainnet_cmc_id),
        governance_canister_id: args
            .governance_canister_id
            .unwrap_or_else(mainnet_governance_id),
        blackhole_canister_id: args
            .blackhole_canister_id
            .unwrap_or_else(mainnet_blackhole_id),
        main_interval_seconds: args
            .main_interval_seconds
            .unwrap_or(7 * 24 * 60 * 60)
            .max(60),
        max_transfers_per_tick: args.max_transfers_per_tick,
        surplus_recipients: args
            .surplus_recipients
            .into_iter()
            .map(surplus_recipient_from_public)
            .collect(),
    };
    crate::logic::validate_config(&cfg, self_canister_principal_for_validation())
        .expect("invalid relay config");
    crate::state::init_stable_storage();
    crate::state::set_state(crate::state::State::new(cfg, now_secs));
    crate::scheduler::install_timers();
}

#[ic_cdk::post_upgrade]
fn post_upgrade(args: Option<UpgradeArgs>) {
    let now_secs = (ic_cdk::api::time() / 1_000_000_000) as u64;
    crate::state::init_stable_storage();
    let mut st = crate::state::restore_state_from_stable()
        .expect("stable state missing during relay post_upgrade");
    if let Some(args) = args {
        apply_upgrade_args(&mut st, args);
    }
    crate::logic::validate_config(&st.config, self_canister_principal_for_validation())
        .expect("invalid relay config after upgrade");
    st.main_lock_state_ts = Some(0);
    if st.last_main_run_ts == 0 {
        st.last_main_run_ts = now_secs.saturating_sub(10 * 365 * 24 * 60 * 60);
    }
    crate::state::set_state(st);
    crate::scheduler::install_timers();
    crate::scheduler::schedule_immediate_resume_if_needed();
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub struct DebugState {
    pub last_main_run_ts: u64,
    pub main_lock_state_ts: Option<u64>,
    pub active_job_present: bool,
    pub last_summary_present: bool,
    pub next_job_id: u64,
    pub last_completed_cycles_count: u32,
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
    pub surplus_recipients: Vec<crate::state::SurplusRecipient>,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    guard_debug_api_not_production();
    crate::state::with_state(|st| DebugState {
        last_main_run_ts: st.last_main_run_ts,
        main_lock_state_ts: st.main_lock_state_ts,
        active_job_present: st.active_job.is_some(),
        last_summary_present: st.last_summary.is_some(),
        next_job_id: st.next_job_id,
        last_completed_cycles_count: st.last_completed_cycles.len() as u32,
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
        surplus_recipients: st.config.surplus_recipients.clone(),
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

    #[test]
    fn init_args_surplus_conversion_preserves_target_and_memo() {
        let owner = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let converted = surplus_recipient_from_public(SurplusRecipient {
            target: SurplusTarget::Canister(owner),
            memo: Some(vec![1, 2, 3]),
        });
        assert_eq!(
            converted.target,
            crate::state::SurplusTarget::Canister(owner)
        );
        assert_eq!(converted.memo, Some(vec![1, 2, 3]));
    }

    #[test]
    fn production_relay_principal_is_recognized() {
        let prod = Principal::from_text("u2qkp-aqaaa-aaaar-qb7ea-cai").unwrap();
        assert_eq!(production_canister_id(), prod);
        assert!(is_production_canister(prod));
        assert!(!is_production_canister(Principal::anonymous()));
    }

    #[test]
    fn upgrade_args_can_leave_set_or_clear_optional_config() {
        let owner = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = crate::state::Config {
            managed_canisters: vec![owner],
            ledger_canister_id: Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").unwrap(),
            cmc_canister_id: mainnet_cmc_id(),
            governance_canister_id: mainnet_governance_id(),
            blackhole_canister_id: mainnet_blackhole_id(),
            main_interval_seconds: 60,
            max_transfers_per_tick: Some(3),
            surplus_recipients: vec![crate::state::SurplusRecipient {
                target: crate::state::SurplusTarget::Canister(owner),
                memo: None,
            }],
        };
        let mut st = crate::state::State::new(cfg, 1);

        apply_upgrade_args(
            &mut st,
            UpgradeArgs {
                managed_canisters: None,
                ledger_canister_id: None,
                cmc_canister_id: None,
                governance_canister_id: None,
                blackhole_canister_id: None,
                main_interval_seconds: None,
                max_transfers_per_tick: None,
                surplus_recipients: None,
            },
        );
        assert_eq!(st.config.max_transfers_per_tick, Some(3));
        assert_eq!(st.config.surplus_recipients.len(), 1);

        apply_upgrade_args(
            &mut st,
            UpgradeArgs {
                managed_canisters: None,
                ledger_canister_id: None,
                cmc_canister_id: None,
                governance_canister_id: None,
                blackhole_canister_id: None,
                main_interval_seconds: None,
                max_transfers_per_tick: Some(None),
                surplus_recipients: Some(Vec::new()),
            },
        );
        assert_eq!(st.config.max_transfers_per_tick, None);
        assert!(st.config.surplus_recipients.is_empty());

        apply_upgrade_args(
            &mut st,
            UpgradeArgs {
                managed_canisters: None,
                ledger_canister_id: None,
                cmc_canister_id: None,
                governance_canister_id: None,
                blackhole_canister_id: None,
                main_interval_seconds: None,
                max_transfers_per_tick: Some(Some(1)),
                surplus_recipients: Some(vec![SurplusRecipient {
                    target: SurplusTarget::Neuron(42),
                    memo: Some(vec![4]),
                }]),
            },
        );
        assert_eq!(st.config.max_transfers_per_tick, Some(1));
        assert_eq!(st.config.surplus_recipients.len(), 1);
    }
}
