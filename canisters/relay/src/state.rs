use std::collections::BTreeMap;

use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
#[cfg(test)]
use jupiter_ic_clients::account_identifier::account_identifier_text;
use serde::Serialize;

const PRODUCTION_JUPITER_MANAGED_CANISTERS: [&str; 8] = [
    "uccpi-cqaaa-aaaar-qby3q-cai",
    "afisn-gqaaa-aaaar-qb4qa-cai",
    "acjuz-liaaa-aaaar-qb4qq-cai",
    "alk7f-5aaaa-aaaar-qb4ra-cai",
    "jufzc-caaaa-aaaar-qb5da-cai",
    "j5gs6-uiaaa-aaaar-qb5cq-cai",
    "77deu-baaaa-aaaar-qb6za-cai",
    "e3mmv-5qaaa-aaaah-aadma-cai",
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
    pub consecutive_failures: u32,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum TargetProbeClassification {
    Observable,
    TransientProbeFailure { consecutive_failures: u32 },
    UnavailableAfterConsecutiveFailures { consecutive_failures: u32 },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct TargetProbeStatus {
    pub canister_id: Principal,
    pub consecutive_probe_failures: u32,
    pub classification: TargetProbeClassification,
    pub skipped_reason: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CanisterBurnSample {
    pub canister_id: Principal,
    pub previous_cycles: Option<u128>,
    pub current_cycles: u128,
    pub relay_minted_cycles: u128,
    pub burn_cycles: u128,
    pub carried_deficit_cycles: u128,
    pub target_topup_cycles: u128,
    pub gross_share_e8s: u64,
    pub amount_e8s: u64,
    pub sent_topup_e8s: u64,
    pub actual_minted_cycles: u128,
    pub remaining_deficit_cycles: u128,
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
    pub total_target_topup_cycles: u128,
    pub total_actual_minted_cycles: u128,
    pub total_carried_deficit_cycles: u128,
    pub total_remaining_deficit_cycles: u128,
    pub deficit_canister_count: u32,
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
    pub target_probe_statuses: Vec<TargetProbeStatus>,
    pub canisters: Vec<CanisterBurnSample>,
    pub conversion_estimate_used: Option<ConversionEstimate>,
    pub surplus_e8s_before_fees: u64,
    pub surplus_transfers: Vec<SurplusTransferSample>,
    pub skipped_surplus_reason: Option<String>,
    pub surplus_allowed_despite_unavailable_targets: bool,
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
            total_target_topup_cycles: 0,
            total_actual_minted_cycles: 0,
            total_carried_deficit_cycles: 0,
            total_remaining_deficit_cycles: 0,
            deficit_canister_count: 0,
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
            target_probe_statuses: Vec::new(),
            canisters: Vec::new(),
            conversion_estimate_used: None,
            surplus_e8s_before_fees: 0,
            surplus_transfers: Vec::new(),
            skipped_surplus_reason: None,
            surplus_allowed_despite_unavailable_targets: false,
        }
    }

    pub(crate) fn refresh_canister_totals(&mut self) {
        self.total_target_topup_cycles = self
            .canisters
            .iter()
            .map(|sample| sample.target_topup_cycles)
            .sum();
        self.total_actual_minted_cycles = self
            .canisters
            .iter()
            .map(|sample| sample.actual_minted_cycles)
            .sum();
        self.total_carried_deficit_cycles = self
            .canisters
            .iter()
            .map(|sample| sample.carried_deficit_cycles)
            .sum();
        self.total_remaining_deficit_cycles = self
            .canisters
            .iter()
            .map(|sample| sample.remaining_deficit_cycles)
            .sum();
        self.deficit_canister_count = self
            .canisters
            .iter()
            .filter(|sample| sample.remaining_deficit_cycles > 0)
            .count() as u32;
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
    pub recovery_deficit_cycles: BTreeMap<Principal, u128>,
    pub consecutive_probe_failures: BTreeMap<Principal, u32>,
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
            recovery_deficit_cycles: BTreeMap::new(),
            consecutive_probe_failures: BTreeMap::new(),
            conversion_estimate: None,
            active_job: None,
            active_faucet_commitment_transfer: None,
            last_summary: None,
            next_job_id: 1,
        }
    }
}

#[cfg(test)]
pub(crate) fn relay_subaccount_one_hex() -> String {
    hex_bytes(&crate::logic::relay_subaccount_one())
}

#[cfg(test)]
pub(crate) fn relay_subaccount_one_account_identifier(self_id: Principal) -> String {
    account_identifier_text(self_id, Some(crate::logic::relay_subaccount_one()))
}

#[cfg(test)]
pub(crate) fn relay_subaccount_one_icrc_text(self_id: Principal) -> String {
    Account {
        owner: self_id,
        subaccount: Some(crate::logic::relay_subaccount_one()),
    }
    .to_string()
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
    let (max_remaining_deficit_canister_id, max_remaining_deficit_cycles) =
        max_remaining_deficit(summary);
    let (min_cycles_canister_id, min_cycles_balance) = min_cycles_sample(summary);
    format!(
        "RELAY_SUMMARY mode={:?} started_at_ts_nanos={} completed_at_ts_nanos={} min_cycles_balance={} min_cycles_canister_id={} min_cycles_sample={} total_burn_cycles={} total_target_topup_cycles={} total_actual_minted_cycles={} total_carried_deficit_cycles={} total_remaining_deficit_cycles={} deficit_canister_count={} max_remaining_deficit_canister_id={} max_remaining_deficit_cycles={} balance_start_e8s={} fee_e8s={} transfer_count={} ledger_transfer_count={} ledger_sent_e8s={} ledger_fees_e8s={} cmc_notify_success_count={} cmc_notify_failed_count={} cmc_notify_ambiguous_count={} planned_retained_e8s={} known_unspent_e8s={} ambiguous_e8s={} failed_transfers={} ambiguous_transfers={} partial_tick_count={} conversion_cycles_per_e8={} surplus_e8s_before_fees={} skipped_surplus_reason={} canister_skip_counts={} surplus_allowed_despite_unavailable_targets={}",
        summary.mode,
        summary.started_at_ts_nanos,
        opt_u64(summary.completed_at_ts_nanos),
        opt_u128(summary.min_cycles_balance),
        opt_principal(min_cycles_canister_id),
        opt_u128(min_cycles_balance),
        summary.total_burn_cycles,
        summary.total_target_topup_cycles,
        summary.total_actual_minted_cycles,
        summary.total_carried_deficit_cycles,
        summary.total_remaining_deficit_cycles,
        summary.deficit_canister_count,
        opt_principal(max_remaining_deficit_canister_id),
        max_remaining_deficit_cycles
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string()),
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
        canister_skip_counts(summary),
        summary.surplus_allowed_despite_unavailable_targets,
    )
}

fn max_remaining_deficit(summary: &RelaySummary) -> (Option<Principal>, Option<u128>) {
    summary
        .canisters
        .iter()
        .filter(|sample| sample.remaining_deficit_cycles > 0)
        .max_by_key(|sample| sample.remaining_deficit_cycles)
        .map(|sample| {
            (
                Some(sample.canister_id),
                Some(sample.remaining_deficit_cycles),
            )
        })
        .unwrap_or((None, None))
}

fn min_cycles_sample(summary: &RelaySummary) -> (Option<Principal>, Option<u128>) {
    summary
        .canisters
        .iter()
        .min_by_key(|sample| sample.current_cycles)
        .map(|sample| (Some(sample.canister_id), Some(sample.current_cycles)))
        .unwrap_or((None, None))
}

fn canister_skip_counts(summary: &RelaySummary) -> String {
    let mut counts = BTreeMap::<&str, u32>::new();
    for sample in &summary.canisters {
        if let Some(reason) = sample.skipped_reason.as_deref() {
            *counts.entry(reason).or_insert(0) += 1;
        }
    }
    if counts.is_empty() {
        return "none".to_string();
    }
    counts
        .into_iter()
        .map(|(reason, count)| format!("{}:{}", escape_log_text(reason), count))
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn relay_canister_log_line(sample: &CanisterBurnSample) -> String {
    format!(
        "RELAY_CANISTER canister_id={} previous_cycles={} current_cycles={} relay_minted_cycles={} burn_cycles={} carried_deficit_cycles={} target_topup_cycles={} planned_topup_e8s={} sent_topup_e8s={} actual_minted_cycles={} remaining_deficit_cycles={} skipped_reason={}",
        sample.canister_id.to_text(),
        opt_u128(sample.previous_cycles),
        sample.current_cycles,
        sample.relay_minted_cycles,
        sample.burn_cycles,
        sample.carried_deficit_cycles,
        sample.target_topup_cycles,
        sample.amount_e8s,
        sample.sent_topup_e8s,
        sample.actual_minted_cycles,
        sample.remaining_deficit_cycles,
        opt_text(sample.skipped_reason.as_deref()),
    )
}

pub(crate) fn relay_probe_failure_log_line(failure: &ProbeFailure) -> String {
    format!(
        "RELAY_PROBE_FAILURE canister_id={} consecutive_failures={} error={}",
        failure.canister_id.to_text(),
        failure.consecutive_failures,
        escape_log_text(&failure.error),
    )
}

pub(crate) fn relay_target_probe_status_log_line(status: &TargetProbeStatus) -> String {
    format!(
        "RELAY_TARGET_PROBE canister_id={} consecutive_probe_failures={} classification={} skipped_reason={}",
        status.canister_id.to_text(),
        status.consecutive_probe_failures,
        target_probe_classification_text(&status.classification),
        opt_text(status.skipped_reason.as_deref()),
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

fn opt_principal(value: Option<Principal>) -> String {
    value
        .map(|v| v.to_text())
        .unwrap_or_else(|| "null".to_string())
}

fn opt_text(value: Option<&str>) -> String {
    value
        .map(escape_log_text)
        .unwrap_or_else(|| "null".to_string())
}

fn target_probe_classification_text(classification: &TargetProbeClassification) -> &'static str {
    match classification {
        TargetProbeClassification::Observable => "observable",
        TargetProbeClassification::TransientProbeFailure { .. } => "transient_probe_failure",
        TargetProbeClassification::UnavailableAfterConsecutiveFailures { .. } => {
            "target_unavailable_after_consecutive_probe_failures"
        }
    }
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

thread_local! {
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_state(st: State) {
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

pub(crate) fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("state not initialized")))
}

pub(crate) fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let st = borrow.as_mut().expect("state not initialized");
        f(st)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_test_state() {
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
                principal("e3mmv-5qaaa-aaaah-aadma-cai"),
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
        summary.total_target_topup_cycles = 555;
        summary.total_actual_minted_cycles = 222;
        summary.total_carried_deficit_cycles = 25;
        summary.total_remaining_deficit_cycles = 333;
        summary.deficit_canister_count = 2;
        summary.default_account_balance_start_e8s = 555;
        summary.fee_e8s = 10;
        summary.partial_tick_count = 1;

        let line = relay_summary_log_line(&summary);

        assert!(line.starts_with("RELAY_SUMMARY mode=TopUpThenSurplus"));
        assert!(line.contains("started_at_ts_nanos=11"));
        assert!(line.contains("completed_at_ts_nanos=22"));
        assert!(line.contains("min_cycles_balance=333"));
        assert!(line.contains("total_burn_cycles=444"));
        assert!(line.contains("total_target_topup_cycles=555"));
        assert!(line.contains("total_actual_minted_cycles=222"));
        assert!(line.contains("total_carried_deficit_cycles=25"));
        assert!(line.contains("total_remaining_deficit_cycles=333"));
        assert!(line.contains("deficit_canister_count=2"));
        assert!(line.contains("balance_start_e8s=555"));
        assert!(line.contains("fee_e8s=10"));
        assert!(line.contains("partial_tick_count=1"));
        assert!(line.contains("surplus_allowed_despite_unavailable_targets=false"));
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
            carried_deficit_cycles: 25,
            target_topup_cycles: 101,
            gross_share_e8s: 50,
            amount_e8s: 40,
            sent_topup_e8s: 0,
            actual_minted_cycles: 0,
            remaining_deficit_cycles: 101,
            skipped_reason: Some("gross share <= fee".to_string()),
        };
        let canister_line = relay_canister_log_line(&sample);
        assert!(canister_line.starts_with("RELAY_CANISTER "));
        assert!(canister_line.contains("burn_cycles=100"));
        assert!(canister_line.contains("carried_deficit_cycles=25"));
        assert!(canister_line.contains("planned_topup_e8s=40"));
        assert!(canister_line.contains("sent_topup_e8s=0"));
        assert!(canister_line.contains("remaining_deficit_cycles=101"));
        assert!(canister_line.contains("skipped_reason=gross%20share%20%3C%3D%20fee"));

        let failure = ProbeFailure {
            canister_id: principal("uccpi-cqaaa-aaaar-qby3q-cai"),
            error: "call failed\nretry".to_string(),
            consecutive_failures: 2,
        };
        let failure_line = relay_probe_failure_log_line(&failure);
        assert!(failure_line.starts_with("RELAY_PROBE_FAILURE "));
        assert!(failure_line.contains("consecutive_failures=2"));
        assert!(failure_line.contains("error=call%20failed%0Aretry"));
        assert!(!failure_line.contains('\n'));

        let status = TargetProbeStatus {
            canister_id: principal("uccpi-cqaaa-aaaar-qby3q-cai"),
            consecutive_probe_failures: 3,
            classification: TargetProbeClassification::UnavailableAfterConsecutiveFailures {
                consecutive_failures: 3,
            },
            skipped_reason: Some("target_unavailable_after_consecutive_probe_failures".to_string()),
        };
        let status_line = relay_target_probe_status_log_line(&status);
        assert!(status_line.starts_with("RELAY_TARGET_PROBE "));
        assert!(status_line.contains("consecutive_probe_failures=3"));
        assert!(status_line
            .contains("classification=target_unavailable_after_consecutive_probe_failures"));
        assert!(status_line
            .contains("skipped_reason=target_unavailable_after_consecutive_probe_failures"));
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
    fn production_subaccount_one_addresses_are_stable() {
        let relay = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        assert_eq!(
            relay_subaccount_one_hex(),
            "0000000000000000000000000000000000000000000000000000000000000001"
        );
        assert_eq!(
            relay_subaccount_one_account_identifier(relay),
            "9fffa5e0762fd8be8e4c3078d4101926fb8d3c15aa3fa077b981ea779ded42ee"
        );
        assert_eq!(
            relay_subaccount_one_icrc_text(relay),
            "u2qkp-aqaaa-aaaar-qb7ea-cai-66ym2xq.1"
        );
    }

    #[test]
    fn non_production_subaccount_one_icrc_text_uses_valid_textual_encoding() {
        let relay = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let text = relay_subaccount_one_icrc_text(relay);
        let parsed: Account = text.parse().expect("expected valid ICRC textual account");

        assert_eq!(parsed.owner, relay);
        assert_eq!(
            parsed.subaccount,
            Some(crate::logic::relay_subaccount_one())
        );
        assert!(!text.contains(':'));
    }

    #[test]
    fn set_state_initializes_fresh_heap_accounting_state() {
        reset_test_state();
        let canister_id = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let mut st = State::new(base_config(), 10_000);
        st.last_completed_cycles.insert(
            canister_id,
            CyclesSnapshot {
                cycles: 1_000_000_000_000,
                timestamp_nanos: 123_000_000_000,
                source: CyclesSampleSource::BlackholeStatus,
            },
        );
        set_state(st);

        with_state(|stored| {
            assert_eq!(
                stored
                    .last_completed_cycles
                    .get(&canister_id)
                    .map(|sample| sample.cycles),
                Some(1_000_000_000_000)
            );
            assert!(stored.relay_minted_cycles_since_sample.is_empty());
            assert!(stored.recovery_deficit_cycles.is_empty());
            assert!(stored.consecutive_probe_failures.is_empty());
            assert!(stored.last_summary.is_none());
            assert!(stored.active_job.is_none());
            assert!(stored.active_faucet_commitment_transfer.is_none());
            assert!(stored.conversion_estimate.is_none());
            assert_eq!(stored.next_job_id, 1);
        });
    }
}
