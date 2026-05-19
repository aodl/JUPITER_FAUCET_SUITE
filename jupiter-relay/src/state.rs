use std::borrow::Cow;
use std::collections::BTreeMap;

use candid::{CandidType, Deserialize, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::Bound,
    DefaultMemoryImpl, StableCell, Storable,
};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

const PRODUCTION_JUPITER_MANAGED_CANISTERS: [&str; 7] = [
    "uccpi-cqaaa-aaaar-qby3q-cai",
    "afisn-gqaaa-aaaar-qb4qa-cai",
    "acjuz-liaaa-aaaar-qb4qq-cai",
    "alk7f-5aaaa-aaaar-qb4ra-cai",
    "jufzc-caaaa-aaaar-qb5da-cai",
    "j5gs6-uiaaa-aaaar-qb5cq-cai",
    "77deu-baaaa-aaaar-qb6za-cai",
];

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub managed_canisters: Vec<Principal>,
    pub ledger_canister_id: Principal,
    pub cmc_canister_id: Principal,
    pub blackhole_canister_id: Principal,
    pub main_interval_seconds: u64,
    pub max_transfers_per_tick: Option<u32>,
    pub raw_icp_mode: Option<RawIcpModeConfig>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RawIcpModeConfig {
    pub min_cycles_threshold: u128,
    pub recipients: Vec<RawIcpRecipient>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawIcpRecipient {
    pub account: Account,
    pub memo: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum RelayMode {
    BaselineOnly,
    CyclesTopUp,
    RawIcp,
    Degraded,
    NoFunds,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum CyclesSampleSource {
    SelfCanister,
    BlackholeStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CyclesSnapshot {
    pub cycles: u128,
    pub timestamp_nanos: u64,
    pub source: CyclesSampleSource,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ProbeFailure {
    pub canister_id: Principal,
    pub error: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CanisterBurnSample {
    pub canister_id: Principal,
    pub previous_cycles: Option<u128>,
    pub current_cycles: u128,
    pub burn_cycles: u128,
    pub weight: u128,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub skipped_reason: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RelaySummary {
    pub mode: RelayMode,
    pub started_at_ts_nanos: u64,
    pub completed_at_ts_nanos: Option<u64>,
    pub default_account_balance_start_e8s: u64,
    pub fee_e8s: u64,
    pub managed_canister_count: u32,
    pub min_cycles_balance: Option<u128>,
    pub total_burn_cycles: u128,
    pub transfer_count: u32,
    pub ledger_transfer_count: u32,
    pub ledger_sent_e8s: u64,
    pub ledger_fees_e8s: u64,
    pub cmc_notify_success_count: u32,
    pub cmc_notify_failed_count: u32,
    pub cmc_notify_ambiguous_count: u32,
    pub planned_retained_e8s: u64,
    pub known_unspent_e8s: u64,
    pub ambiguous_e8s: u64,
    pub failed_transfers: u32,
    pub ambiguous_transfers: u32,
    pub partial_tick_count: u32,
    pub probe_failures: Vec<ProbeFailure>,
    pub canisters: Vec<CanisterBurnSample>,
}

impl RelaySummary {
    pub(crate) fn started(
        mode: RelayMode,
        started_at_ts_nanos: u64,
        managed_canister_count: u32,
    ) -> Self {
        Self {
            mode,
            started_at_ts_nanos,
            completed_at_ts_nanos: None,
            default_account_balance_start_e8s: 0,
            fee_e8s: 0,
            managed_canister_count,
            min_cycles_balance: None,
            total_burn_cycles: 0,
            transfer_count: 0,
            ledger_transfer_count: 0,
            ledger_sent_e8s: 0,
            ledger_fees_e8s: 0,
            cmc_notify_success_count: 0,
            cmc_notify_failed_count: 0,
            cmc_notify_ambiguous_count: 0,
            planned_retained_e8s: 0,
            known_unspent_e8s: 0,
            ambiguous_e8s: 0,
            failed_transfers: 0,
            ambiguous_transfers: 0,
            partial_tick_count: 0,
            probe_failures: Vec::new(),
            canisters: Vec::new(),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ActiveRelayMode {
    CyclesTopUp,
    RawIcp,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PendingTransferKind {
    CmcTopUp {
        canister_id: Principal,
    },
    RawIcp {
        account: Account,
        memo: Option<Vec<u8>>,
    },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PendingTransferPhase {
    AwaitingTransfer,
    TransferAccepted { block_index: u64 },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingTransfer {
    pub kind: PendingTransferKind,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub created_at_time_nanos: u64,
    pub phase: PendingTransferPhase,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveRelayJob {
    pub id: u64,
    pub mode: ActiveRelayMode,
    pub started_at_ts_nanos: u64,
    pub fee_e8s: u64,
    pub balance_start_e8s: u64,
    pub current_cycles: BTreeMap<Principal, CyclesSnapshot>,
    pub canisters: Vec<CanisterBurnSample>,
    pub raw_recipients: Vec<RawIcpRecipient>,
    pub pending_transfer: Option<PendingTransfer>,
    pub next_transfer_index: u32,
    pub next_created_at_time_nanos: u64,
    pub summary: RelaySummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub(crate) struct State {
    pub config: Config,
    pub last_main_run_ts: u64,
    pub main_lock_state_ts: Option<u64>,
    pub last_completed_cycles: BTreeMap<Principal, CyclesSnapshot>,
    pub active_job: Option<ActiveRelayJob>,
    pub last_summary: Option<RelaySummary>,
    pub next_job_id: u64,
}

impl State {
    pub(crate) fn new(config: Config, now_secs: u64) -> Self {
        Self {
            config,
            last_main_run_ts: now_secs.saturating_sub(10 * 365 * 24 * 60 * 60),
            main_lock_state_ts: Some(0),
            last_completed_cycles: BTreeMap::new(),
            active_job: None,
            last_summary: None,
            next_job_id: 1,
        }
    }
}

pub(crate) fn runtime_config_log_line(cfg: &Config, self_id: Principal) -> String {
    let effective = crate::logic::effective_managed_canisters(&cfg.managed_canisters, self_id);
    let raw_icp_min_cycles_threshold = cfg
        .raw_icp_mode
        .as_ref()
        .map(|raw| raw.min_cycles_threshold.to_string())
        .unwrap_or_else(|| "null".to_string());
    let raw_icp_recipients = cfg
        .raw_icp_mode
        .as_ref()
        .map(|raw| {
            raw.recipients
                .iter()
                .map(|recipient| account_log_text(&recipient.account))
                .collect::<Vec<_>>()
                .join("|")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "none".to_string());
    let raw_icp_recipient_memos = cfg
        .raw_icp_mode
        .as_ref()
        .map(|raw| {
            raw.recipients
                .iter()
                .map(|recipient| {
                    recipient
                        .memo
                        .as_ref()
                        .map(|memo| hex_bytes(memo))
                        .unwrap_or_else(|| "null".to_string())
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "none".to_string());
    format!(
        "CONFIG relay_canister_id={}, managed_canisters={}, effective_managed_canisters={}, ledger_canister_id={}, cmc_canister_id={}, blackhole_canister_id={}, main_interval_seconds={}, max_transfers_per_tick={}, raw_icp_mode_present={}, raw_icp_min_cycles_threshold={}, raw_icp_recipients={}, raw_icp_recipient_memos={}, production_managed_set_match={}",
        self_id.to_text(),
        principal_list(&cfg.managed_canisters),
        principal_list(&effective),
        cfg.ledger_canister_id.to_text(),
        cfg.cmc_canister_id.to_text(),
        cfg.blackhole_canister_id.to_text(),
        cfg.main_interval_seconds,
        cfg.max_transfers_per_tick
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string()),
        cfg.raw_icp_mode.is_some(),
        raw_icp_min_cycles_threshold,
        raw_icp_recipients,
        raw_icp_recipient_memos,
        production_managed_set_matches(&cfg.managed_canisters),
    )
}

fn production_managed_set_matches(managed: &[Principal]) -> bool {
    let expected = PRODUCTION_JUPITER_MANAGED_CANISTERS
        .iter()
        .map(|id| Principal::from_text(id).expect("invalid hardcoded production canister id"))
        .collect::<std::collections::BTreeSet<_>>();
    managed
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        == expected
}

fn principal_list(principals: &[Principal]) -> String {
    principals
        .iter()
        .map(Principal::to_text)
        .collect::<Vec<_>>()
        .join("|")
}

fn account_log_text(account: &Account) -> String {
    format!(
        "{}:{}",
        account.owner.to_text(),
        account
            .subaccount
            .as_ref()
            .map(|subaccount| hex_bytes(subaccount))
            .unwrap_or_else(|| "null".to_string())
    )
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub(crate) enum VersionedStableState {
    Uninitialized,
    V1(State),
}

impl Storable for VersionedStableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode relay stable state"))
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode relay stable state")
    }

    const BOUND: Bound = Bound::Unbounded;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: std::cell::RefCell<MemoryManager<DefaultMemoryImpl>> =
        std::cell::RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static STABLE_STATE: std::cell::RefCell<Option<StableCell<VersionedStableState, Memory>>> =
        std::cell::RefCell::new(None);
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
}

fn with_stable_cell<R>(f: impl FnOnce(&mut StableCell<VersionedStableState, Memory>) -> R) -> R {
    STABLE_STATE.with(|cell| {
        if cell.borrow().is_none() {
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(0));
                let stable_cell = StableCell::init(memory, VersionedStableState::Uninitialized)
                    .expect("failed to initialize relay stable cell");
                *cell.borrow_mut() = Some(stable_cell);
            });
        }
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().expect("relay stable cell not initialized"))
    })
}

fn persist_snapshot(st: &State) {
    with_stable_cell(|cell| {
        cell.set(VersionedStableState::V1(st.clone()))
            .expect("failed to persist relay stable state");
    });
}

pub(crate) fn init_stable_storage() {
    let _ = restore_state_from_stable();
}

pub(crate) fn restore_state_from_stable() -> Option<State> {
    with_stable_cell(|cell| match cell.get().clone() {
        VersionedStableState::Uninitialized => None,
        VersionedStableState::V1(st) => Some(st),
    })
}

pub(crate) fn set_state(st: State) {
    persist_snapshot(&st);
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub(crate) fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        let out = f(st);
        persist_snapshot(st);
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    fn base_config() -> Config {
        Config {
            managed_canisters: vec![
                principal("uccpi-cqaaa-aaaar-qby3q-cai"),
                principal("afisn-gqaaa-aaaar-qb4qa-cai"),
                principal("acjuz-liaaa-aaaar-qb4qq-cai"),
                principal("alk7f-5aaaa-aaaar-qb4ra-cai"),
                principal("jufzc-caaaa-aaaar-qb5da-cai"),
                principal("j5gs6-uiaaa-aaaar-qb5cq-cai"),
                principal("77deu-baaaa-aaaar-qb6za-cai"),
            ],
            ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
            cmc_canister_id: principal("rkp4c-7iaaa-aaaaa-aaaca-cai"),
            blackhole_canister_id: principal("77deu-baaaa-aaaar-qb6za-cai"),
            main_interval_seconds: 604_800,
            max_transfers_per_tick: Some(10),
            raw_icp_mode: None,
        }
    }

    #[test]
    fn runtime_config_log_line_includes_all_fields() {
        let self_id = principal("cm5kl-iiaaa-aaaac-be6za-cai");
        let line = runtime_config_log_line(&base_config(), self_id);

        assert!(line.starts_with("CONFIG "));
        assert!(line.contains("relay_canister_id=cm5kl-iiaaa-aaaac-be6za-cai"));
        assert!(line.contains("managed_canisters=uccpi-cqaaa-aaaar-qby3q-cai"));
        assert!(line.contains("effective_managed_canisters="));
        assert!(line.contains("cm5kl-iiaaa-aaaac-be6za-cai"));
        assert!(line.contains("ledger_canister_id=ryjl3-tyaaa-aaaaa-aaaba-cai"));
        assert!(line.contains("cmc_canister_id=rkp4c-7iaaa-aaaaa-aaaca-cai"));
        assert!(line.contains("blackhole_canister_id=77deu-baaaa-aaaar-qb6za-cai"));
        assert!(line.contains("main_interval_seconds=604800"));
        assert!(line.contains("max_transfers_per_tick=10"));
        assert!(line.contains("raw_icp_mode_present=false"));
        assert!(line.contains("raw_icp_min_cycles_threshold=null"));
        assert!(line.contains("raw_icp_recipients=none"));
        assert!(line.contains("raw_icp_recipient_memos=none"));
        assert!(line.contains("production_managed_set_match=true"));
    }

    #[test]
    fn runtime_config_log_line_includes_raw_icp_recipients_and_memos() {
        let self_id = principal("cm5kl-iiaaa-aaaac-be6za-cai");
        let mut cfg = base_config();
        cfg.raw_icp_mode = Some(RawIcpModeConfig {
            min_cycles_threshold: 5_000_000_000_000,
            recipients: vec![
                RawIcpRecipient {
                    account: Account {
                        owner: principal("jufzc-caaaa-aaaar-qb5da-cai"),
                        subaccount: None,
                    },
                    memo: Some(vec![1, 2]),
                },
                RawIcpRecipient {
                    account: Account {
                        owner: self_id,
                        subaccount: Some([9; 32]),
                    },
                    memo: None,
                },
            ],
        });

        let line = runtime_config_log_line(&cfg, self_id);

        assert!(line.contains("raw_icp_mode_present=true"));
        assert!(line.contains("raw_icp_min_cycles_threshold=5000000000000"));
        assert!(line.contains("raw_icp_recipients=jufzc-caaaa-aaaar-qb5da-cai:null|cm5kl-iiaaa-aaaac-be6za-cai:0909090909090909090909090909090909090909090909090909090909090909"));
        assert!(line.contains("raw_icp_recipient_memos=0102|null"));
    }
}
