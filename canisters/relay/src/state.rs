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
    pub governance_canister_id: Principal,
    pub blackhole_canister_id: Principal,
    pub main_interval_seconds: u64,
    pub max_transfers_per_tick: Option<u32>,
    pub surplus_recipients: Vec<SurplusRecipient>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SurplusRecipient {
    pub target: SurplusTarget,
    pub memo: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SurplusTarget {
    Canister(Principal),
    Neuron(u64),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum RelayMode {
    BaselineOnly,
    TopUpThenSurplus,
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
    pub relay_minted_cycles: u128,
    pub burn_cycles: u128,
    pub target_topup_cycles: u128,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub actual_minted_cycles: u128,
    pub skipped_reason: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ConversionEstimate {
    pub cycles_per_e8: u128,
    pub timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SurplusTransferSample {
    pub target: SurplusTarget,
    pub account: Account,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub memo_len: Option<u32>,
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
    pub conversion_estimate_used: Option<ConversionEstimate>,
    pub surplus_e8s_before_fees: u64,
    pub surplus_transfers: Vec<SurplusTransferSample>,
    pub skipped_surplus_reason: Option<String>,
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
            conversion_estimate_used: None,
            surplus_e8s_before_fees: 0,
            surplus_transfers: Vec::new(),
            skipped_surplus_reason: None,
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ActiveRelayMode {
    TopUpThenSurplus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PendingTransferKind {
    CmcTopUp {
        canister_id: Principal,
    },
    SurplusIcp {
        target: SurplusTarget,
        account: Account,
        memo: Option<Vec<u8>>,
    },
    FaucetCommitment {
        neuron_id: u64,
        account: Account,
        from_subaccount: [u8; 32],
        memo: Vec<u8>,
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
    pub surplus_transfers: Vec<SurplusTransferSample>,
    pub surplus_memos: Vec<Option<Vec<u8>>>,
    pub surplus_phase_planned: bool,
    pub pending_transfer: Option<PendingTransfer>,
    pub next_transfer_index: u32,
    pub surplus_transfer_index: u32,
    pub next_created_at_time_nanos: u64,
    pub summary: RelaySummary,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingFaucetCommitmentTransfer {
    pub transfer: PendingTransfer,
    pub fee_e8s: u64,
    pub balance_start_e8s: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub(crate) struct State {
    pub config: Config,
    pub last_main_run_ts: u64,
    pub main_lock_state_ts: Option<u64>,
    pub last_completed_cycles: BTreeMap<Principal, CyclesSnapshot>,
    pub relay_minted_cycles_since_sample: BTreeMap<Principal, u128>,
    pub conversion_estimate: Option<ConversionEstimate>,
    pub active_job: Option<ActiveRelayJob>,
    pub active_faucet_commitment_transfer: Option<PendingFaucetCommitmentTransfer>,
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
            relay_minted_cycles_since_sample: BTreeMap::new(),
            conversion_estimate: None,
            active_job: None,
            active_faucet_commitment_transfer: None,
            last_summary: None,
            next_job_id: 1,
        }
    }
}

pub(crate) fn runtime_config_log_line(cfg: &Config, self_id: Principal) -> String {
    let effective = crate::logic::effective_managed_canisters(&cfg.managed_canisters, self_id);
    let surplus_recipients = cfg
        .surplus_recipients
        .iter()
        .map(|recipient| surplus_target_text(&recipient.target))
        .collect::<Vec<_>>()
        .join("|");
    let surplus_recipient_memos = cfg
        .surplus_recipients
        .iter()
        .map(|recipient| {
            recipient
                .memo
                .as_ref()
                .map(|memo| memo.len().to_string())
                .unwrap_or_else(|| "null".to_string())
        })
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "CONFIG relay_canister_id={}, managed_canisters={}, effective_managed_canisters={}, ledger_canister_id={}, cmc_canister_id={}, governance_canister_id={}, blackhole_canister_id={}, main_interval_seconds={}, max_transfers_per_tick={}, surplus_recipient_count={}, surplus_recipients={}, surplus_recipient_memo_lengths={}, production_managed_set_match={}",
        self_id.to_text(),
        principal_list(&cfg.managed_canisters),
        principal_list(&effective),
        cfg.ledger_canister_id.to_text(),
        cfg.cmc_canister_id.to_text(),
        cfg.governance_canister_id.to_text(),
        cfg.blackhole_canister_id.to_text(),
        cfg.main_interval_seconds,
        cfg.max_transfers_per_tick
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string()),
        cfg.surplus_recipients.len(),
        if surplus_recipients.is_empty() { "none" } else { &surplus_recipients },
        if surplus_recipient_memos.is_empty() { "none" } else { &surplus_recipient_memos },
        production_managed_set_matches(&cfg.managed_canisters),
    )
}

pub(crate) fn relay_summary_log_line(summary: &RelaySummary) -> String {
    format!(
        "RELAY_SUMMARY mode={:?} started_at_ts_nanos={} completed_at_ts_nanos={} min_cycles_balance={} total_burn_cycles={} balance_start_e8s={} fee_e8s={} transfer_count={} ledger_transfer_count={} ledger_sent_e8s={} ledger_fees_e8s={} cmc_notify_success_count={} cmc_notify_failed_count={} cmc_notify_ambiguous_count={} planned_retained_e8s={} known_unspent_e8s={} ambiguous_e8s={} failed_transfers={} ambiguous_transfers={} partial_tick_count={} conversion_cycles_per_e8={} surplus_e8s_before_fees={} skipped_surplus_reason={}",
        summary.mode,
        summary.started_at_ts_nanos,
        opt_u64(summary.completed_at_ts_nanos),
        opt_u128(summary.min_cycles_balance),
        summary.total_burn_cycles,
        summary.default_account_balance_start_e8s,
        summary.fee_e8s,
        summary.transfer_count,
        summary.ledger_transfer_count,
        summary.ledger_sent_e8s,
        summary.ledger_fees_e8s,
        summary.cmc_notify_success_count,
        summary.cmc_notify_failed_count,
        summary.cmc_notify_ambiguous_count,
        summary.planned_retained_e8s,
        summary.known_unspent_e8s,
        summary.ambiguous_e8s,
        summary.failed_transfers,
        summary.ambiguous_transfers,
        summary.partial_tick_count,
        summary
            .conversion_estimate_used
            .as_ref()
            .map(|estimate| estimate.cycles_per_e8.to_string())
            .unwrap_or_else(|| "null".to_string()),
        summary.surplus_e8s_before_fees,
        opt_text(summary.skipped_surplus_reason.as_deref()),
    )
}

pub(crate) fn relay_canister_log_line(sample: &CanisterBurnSample) -> String {
    format!(
        "RELAY_CANISTER canister_id={} previous_cycles={} current_cycles={} relay_minted_cycles={} burn_cycles={} target_topup_cycles={} planned_topup_e8s={} actual_topup_e8s={} actual_minted_cycles={} skipped_reason={}",
        sample.canister_id.to_text(),
        opt_u128(sample.previous_cycles),
        sample.current_cycles,
        sample.relay_minted_cycles,
        sample.burn_cycles,
        sample.target_topup_cycles,
        sample.gross_share_e8s,
        sample.amount_e8s,
        sample.actual_minted_cycles,
        opt_text(sample.skipped_reason.as_deref()),
    )
}

pub(crate) fn relay_probe_failure_log_line(failure: &ProbeFailure) -> String {
    format!(
        "RELAY_PROBE_FAILURE canister_id={} error={}",
        failure.canister_id.to_text(),
        escape_log_text(&failure.error),
    )
}

pub(crate) fn relay_surplus_transfer_log_line(plan: &SurplusTransferSample) -> String {
    format!(
        "RELAY_SURPLUS_TRANSFER target={} owner={} subaccount={} gross_share_e8s={} amount_e8s={} skipped_reason={} memo_len={}",
        surplus_target_text(&plan.target),
        plan.account.owner.to_text(),
        plan.account
            .subaccount
            .as_ref()
            .map(|subaccount| hex_bytes(subaccount))
            .unwrap_or_else(|| "null".to_string()),
        plan.gross_share_e8s,
        plan.amount_e8s,
        opt_text(plan.skipped_reason.as_deref()),
        plan.memo_len
            .map(|len| len.to_string())
            .unwrap_or_else(|| "null".to_string()),
    )
}

pub(crate) fn relay_faucet_commitment_log_line(
    source: Account,
    destination: Account,
    balance_start_e8s: u64,
    amount_e8s: u64,
    fee_e8s: u64,
    memo_len: u32,
    skipped_reason: Option<&str>,
) -> String {
    format!(
        "RELAY_FAUCET_COMMITMENT source_owner={} source_subaccount={} destination_owner={} destination_subaccount={} balance_start_e8s={} amount_e8s={} fee_e8s={} memo_len={} skipped_reason={}",
        source.owner.to_text(),
        source
            .subaccount
            .as_ref()
            .map(|subaccount| hex_bytes(subaccount))
            .unwrap_or_else(|| "null".to_string()),
        destination.owner.to_text(),
        destination
            .subaccount
            .as_ref()
            .map(|subaccount| hex_bytes(subaccount))
            .unwrap_or_else(|| "null".to_string()),
        balance_start_e8s,
        amount_e8s,
        fee_e8s,
        memo_len,
        opt_text(skipped_reason),
    )
}

fn surplus_target_text(target: &SurplusTarget) -> String {
    match target {
        SurplusTarget::Canister(canister_id) => format!("canister:{}", canister_id.to_text()),
        SurplusTarget::Neuron(neuron_id) => format!("neuron:{neuron_id}"),
    }
}

fn opt_u64(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn opt_u128(value: Option<u128>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn opt_text(value: Option<&str>) -> String {
    value
        .map(escape_log_text)
        .unwrap_or_else(|| "null".to_string())
}

fn escape_log_text(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' | b':' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
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

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// Stable-state enum shape is part of the upgrade contract; boxing V1 would change Candid.
#[allow(clippy::large_enum_variant)]
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
        const { std::cell::RefCell::new(None) };
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

    #[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
    struct OldStateWithoutFaucetCommitment {
        pub config: Config,
        pub last_main_run_ts: u64,
        pub main_lock_state_ts: Option<u64>,
        pub last_completed_cycles: BTreeMap<Principal, CyclesSnapshot>,
        pub relay_minted_cycles_since_sample: BTreeMap<Principal, u128>,
        pub conversion_estimate: Option<ConversionEstimate>,
        pub active_job: Option<ActiveRelayJob>,
        pub last_summary: Option<RelaySummary>,
        pub next_job_id: u64,
    }

    #[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
    enum OldVersionedStableStateWithoutFaucetCommitment {
        Uninitialized,
        V1(OldStateWithoutFaucetCommitment),
    }

    fn reset_test_storage() {
        with_stable_cell(|cell| {
            cell.set(VersionedStableState::Uninitialized)
                .expect("failed to reset relay stable state for test");
        });
        STATE.with(|s| *s.borrow_mut() = None);
    }

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
            governance_canister_id: principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
            blackhole_canister_id: principal("77deu-baaaa-aaaar-qb6za-cai"),
            main_interval_seconds: 86_400,
            max_transfers_per_tick: Some(10),
            surplus_recipients: Vec::new(),
        }
    }

    #[test]
    fn runtime_config_log_line_includes_all_fields() {
        let self_id = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        let line = runtime_config_log_line(&base_config(), self_id);

        assert!(line.starts_with("CONFIG "));
        assert!(line.contains("relay_canister_id=u2qkp-aqaaa-aaaar-qb7ea-cai"));
        assert!(line.contains("managed_canisters=uccpi-cqaaa-aaaar-qby3q-cai"));
        assert!(line.contains("effective_managed_canisters="));
        assert!(line.contains("u2qkp-aqaaa-aaaar-qb7ea-cai"));
        assert!(line.contains("ledger_canister_id=ryjl3-tyaaa-aaaaa-aaaba-cai"));
        assert!(line.contains("cmc_canister_id=rkp4c-7iaaa-aaaaa-aaaca-cai"));
        assert!(line.contains("governance_canister_id=rrkah-fqaaa-aaaaa-aaaaq-cai"));
        assert!(line.contains("blackhole_canister_id=77deu-baaaa-aaaar-qb6za-cai"));
        assert!(line.contains("main_interval_seconds=86400"));
        assert!(line.contains("max_transfers_per_tick=10"));
        assert!(line.contains("surplus_recipient_count=0"));
        assert!(line.contains("surplus_recipients=none"));
        assert!(line.contains("surplus_recipient_memo_lengths=none"));
        assert!(line.contains("production_managed_set_match=true"));
    }

    #[test]
    fn runtime_config_log_line_includes_surplus_recipients_and_memos() {
        let self_id = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        let mut cfg = base_config();
        cfg.surplus_recipients = vec![
            SurplusRecipient {
                target: SurplusTarget::Canister(principal("jufzc-caaaa-aaaar-qb5da-cai")),
                memo: Some(vec![1, 2]),
            },
            SurplusRecipient {
                target: SurplusTarget::Neuron(42),
                memo: None,
            },
        ];

        let line = runtime_config_log_line(&cfg, self_id);

        assert!(line.contains("surplus_recipient_count=2"));
        assert!(line.contains("surplus_recipients=canister:jufzc-caaaa-aaaar-qb5da-cai|neuron:42"));
        assert!(line.contains("surplus_recipient_memo_lengths=2|null"));
    }

    #[test]
    fn relay_summary_log_line_is_single_line_and_includes_public_counters() {
        let mut summary = RelaySummary::started(RelayMode::TopUpThenSurplus, 11, 2);
        summary.completed_at_ts_nanos = Some(22);
        summary.min_cycles_balance = Some(333);
        summary.total_burn_cycles = 444;
        summary.default_account_balance_start_e8s = 555;
        summary.fee_e8s = 10;
        summary.partial_tick_count = 1;

        let line = relay_summary_log_line(&summary);

        assert!(line.starts_with("RELAY_SUMMARY mode=TopUpThenSurplus"));
        assert!(line.contains("started_at_ts_nanos=11"));
        assert!(line.contains("completed_at_ts_nanos=22"));
        assert!(line.contains("min_cycles_balance=333"));
        assert!(line.contains("total_burn_cycles=444"));
        assert!(line.contains("balance_start_e8s=555"));
        assert!(line.contains("fee_e8s=10"));
        assert!(line.contains("partial_tick_count=1"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn relay_canister_and_probe_log_lines_escape_optional_text() {
        let sample = CanisterBurnSample {
            canister_id: principal("uccpi-cqaaa-aaaar-qby3q-cai"),
            previous_cycles: Some(1_000),
            current_cycles: 900,
            relay_minted_cycles: 0,
            burn_cycles: 100,
            target_topup_cycles: 101,
            gross_share_e8s: 50,
            amount_e8s: 40,
            actual_minted_cycles: 0,
            skipped_reason: Some("gross share <= fee".to_string()),
        };
        let canister_line = relay_canister_log_line(&sample);
        assert!(canister_line.starts_with("RELAY_CANISTER "));
        assert!(canister_line.contains("burn_cycles=100"));
        assert!(canister_line.contains("planned_topup_e8s=50"));
        assert!(canister_line.contains("actual_topup_e8s=40"));
        assert!(canister_line.contains("skipped_reason=gross%20share%20%3C%3D%20fee"));

        let failure = ProbeFailure {
            canister_id: principal("uccpi-cqaaa-aaaar-qby3q-cai"),
            error: "call failed\nretry".to_string(),
        };
        let failure_line = relay_probe_failure_log_line(&failure);
        assert!(failure_line.starts_with("RELAY_PROBE_FAILURE "));
        assert!(failure_line.contains("error=call%20failed%0Aretry"));
        assert!(!failure_line.contains('\n'));
    }

    #[test]
    fn relay_faucet_commitment_log_line_omits_raw_memo_and_includes_skip_reason() {
        let source = Account {
            owner: principal("u2qkp-aqaaa-aaaar-qb7ea-cai"),
            subaccount: Some(crate::logic::relay_subaccount_one()),
        };
        let destination = Account {
            owner: principal("rrkah-fqaaa-aaaaa-aaaaq-cai"),
            subaccount: Some([7u8; 32]),
        };

        let line = relay_faucet_commitment_log_line(
            source,
            destination,
            100_010_000,
            100_000_000,
            10_000,
            29,
            Some("subaccount_1_below_1_icp_net"),
        );

        assert!(line.starts_with("RELAY_FAUCET_COMMITMENT "));
        assert!(line.contains("source_owner=u2qkp-aqaaa-aaaar-qb7ea-cai"));
        assert!(line.contains(
            "source_subaccount=0000000000000000000000000000000000000000000000000000000000000001"
        ));
        assert!(line.contains("destination_owner=rrkah-fqaaa-aaaaa-aaaaq-cai"));
        assert!(line.contains("amount_e8s=100000000"));
        assert!(line.contains("memo_len=29"));
        assert!(line.contains("skipped_reason=subaccount_1_below_1_icp_net"));
        assert!(!line.contains("u2qkpaqaaaaaaarqb7eacai.Relay"));
    }

    #[test]
    fn current_relay_state_roundtrip_restores_current_shape() {
        reset_test_storage();
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let mut st = State::new(base_config(), 10_000);
        st.last_main_run_ts = 9_900;
        st.main_lock_state_ts = Some(0);
        st.last_completed_cycles.insert(
            canister_id,
            CyclesSnapshot {
                cycles: 1_000_000_000_000,
                timestamp_nanos: 123_000_000_000,
                source: CyclesSampleSource::BlackholeStatus,
            },
        );
        st.relay_minted_cycles_since_sample.insert(canister_id, 50_000);
        st.conversion_estimate = Some(ConversionEstimate {
            cycles_per_e8: 7_000_000_000,
            timestamp_nanos: 124_000_000_000,
        });
        st.next_job_id = 12;
        set_state(st.clone());

        let encoded_state = with_stable_cell(|cell| cell.get().clone());
        let VersionedStableState::V1(decoded_state) = encoded_state else {
            panic!("expected relay V1 state");
        };
        assert_eq!(decoded_state.last_main_run_ts, 9_900);
        assert_eq!(decoded_state.next_job_id, 12);
        assert_eq!(
            decoded_state.last_completed_cycles.get(&canister_id).map(|sample| sample.cycles),
            Some(1_000_000_000_000)
        );
        assert_eq!(decoded_state.relay_minted_cycles_since_sample.get(&canister_id), Some(&50_000));

        let restored = restore_state_from_stable().expect("expected restored relay state");
        assert_eq!(restored.config, st.config);
        assert_eq!(restored.active_job, None);
        assert_eq!(restored.last_summary, None);
        assert_eq!(restored.conversion_estimate, st.conversion_estimate);
        assert_eq!(
            restored.last_completed_cycles.get(&canister_id).map(|sample| sample.timestamp_nanos),
            Some(123_000_000_000)
        );
    }

    #[test]
    fn previous_stable_state_shape_decodes_with_no_active_faucet_commitment() {
        let old = OldStateWithoutFaucetCommitment {
            config: base_config(),
            last_main_run_ts: 9_900,
            main_lock_state_ts: Some(0),
            last_completed_cycles: BTreeMap::new(),
            relay_minted_cycles_since_sample: BTreeMap::new(),
            conversion_estimate: None,
            active_job: None,
            last_summary: None,
            next_job_id: 12,
        };
        let bytes = candid::encode_one(OldVersionedStableStateWithoutFaucetCommitment::V1(old))
            .expect("old relay stable state should encode");

        let decoded: VersionedStableState =
            candid::decode_one(&bytes).expect("current relay stable state should decode old shape");
        let VersionedStableState::V1(decoded_state) = decoded else {
            panic!("expected relay V1 state");
        };

        assert_eq!(decoded_state.next_job_id, 12);
        assert!(decoded_state.active_job.is_none());
        assert!(decoded_state.active_faucet_commitment_transfer.is_none());
    }
}
