#![cfg_attr(test, allow(dead_code))]

use std::collections::BTreeMap;

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use jupiter_ic_clients::account_identifier::{account_identifier_bytes, account_identifier_text};
use jupiter_ic_clients::index::{IcpIndexCanister, IndexOperation, IndexTransactionWithId};
use jupiter_ic_clients::ledger::IcrcLedgerCanister;

use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::GovernanceClient;
use crate::reward_state::{self, PendingRewardTransfer, PendingRewardTransferStatus};
use crate::{logic, state};

pub(crate) const REWARD_SWEEP_INTERVAL_SECONDS: u64 = 7 * 24 * 60 * 60;
const REWARD_CONTEXT_MAX_AGE_NANOS: u64 = 48 * 60 * 60 * 1_000_000_000;
const ICP_HISTORY_PAGE_SIZE: u64 = 1_000;
const ICP_HISTORY_MAX_PAGES: usize = 10;
const ICP_HISTORY_MAX_TRANSACTIONS: usize = 10_000;
const ICP_HISTORY_MAX_DISTINCT_SOURCES: usize = 128;
const REWARD_MEMO_PREFIX: &[u8; 4] = b"JRS1";

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
struct RelayRewardContext {
    sns_root_canister_id: Principal,
    sns_governance_canister_id: Principal,
    sns_ledger_canister_id: Principal,
    snapshot_id: u64,
    scan_started_at_timestamp_nanos: u64,
    scan_completed_at_timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize)]
struct ResolveDefaultIcpAccountsArgs {
    snapshot_id: u64,
    account_identifiers: Vec<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Debug)]
enum ResolveDefaultIcpAccountsResult {
    Ok(Vec<Option<Principal>>),
    SnapshotChanged,
    TooManyAccounts,
    InvalidAccountIdentifier { index: u32 },
}

#[derive(Default)]
struct RewardLog {
    status: &'static str,
    reason: Option<String>,
    root: Option<Principal>,
    ledger: Option<Principal>,
    snapshot_id: Option<u64>,
    cutoff: Option<u64>,
    processed_from: Option<u64>,
    processed_through: Option<u64>,
    scanned_transactions: usize,
    completed_commitments: usize,
    distinct_sources: usize,
    eligible_principals: usize,
    winner_e8s: Option<u64>,
    ineligible_e8s: Option<u64>,
    token_balance: Option<Nat>,
    token_fee: Option<Nat>,
    token_amount: Option<Nat>,
    recipient: Option<Principal>,
}

impl RewardLog {
    fn emit(&self) {
        fn opt<T: ToString>(value: Option<T>) -> String {
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string())
        }
        ic_cdk::println!(
            "RELAY_SNS_REWARD status={} reason={} sns_root_canister_id={} sns_ledger_canister_id={} snapshot_id={} snapshot_cutoff_ts_nanos={} processed_from_commitment_tx_id={} processed_through_commitment_tx_id={} scanned_transactions={} completed_commitments={} distinct_sources={} eligible_principals={} eligible_winner_icp_e8s={} ineligible_icp_e8s={} token_balance={} token_fee={} token_amount={} recipient={}",
            self.status,
            self.reason.as_deref().map(jupiter_canister_logging::escape_value).unwrap_or_else(|| "null".to_string()),
            self.root.map(|v| v.to_text()).unwrap_or_else(|| "null".to_string()),
            self.ledger.map(|v| v.to_text()).unwrap_or_else(|| "null".to_string()),
            opt(self.snapshot_id), opt(self.cutoff), opt(self.processed_from), opt(self.processed_through),
            self.scanned_transactions, self.completed_commitments, self.distinct_sources,
            self.eligible_principals, opt(self.winner_e8s), opt(self.ineligible_e8s),
            opt(self.token_balance.as_ref()), opt(self.token_fee.as_ref()), opt(self.token_amount.as_ref()),
            self.recipient.map(|v| v.to_text()).unwrap_or_else(|| "null".to_string())
        );
    }
}

async fn reward_context(canister_id: Principal) -> Result<Option<RelayRewardContext>, String> {
    let response = Call::bounded_wait(canister_id, "get_relay_reward_context")
        .change_timeout(30)
        .await
        .map_err(|err| format!("reward context call failed: {err:?}"))?;
    response
        .candid()
        .map_err(|err| format!("reward context decode failed: {err:?}"))
}

async fn resolve_accounts(
    canister_id: Principal,
    snapshot_id: u64,
    accounts: Vec<Vec<u8>>,
) -> Result<ResolveDefaultIcpAccountsResult, String> {
    let response = Call::bounded_wait(canister_id, "resolve_default_icp_accounts")
        .with_arg(ResolveDefaultIcpAccountsArgs {
            snapshot_id,
            account_identifiers: accounts,
        })
        .change_timeout(30)
        .await
        .map_err(|err| format!("owner lookup call failed: {err:?}"))?;
    response
        .candid()
        .map_err(|err| format!("owner lookup decode failed: {err:?}"))
}

pub(crate) async fn process(now_nanos: u64, now_secs: u64, force: bool) {
    if reward_state::get().pending_transfer.is_some() {
        let result = drive_pending(now_nanos).await;
        RewardLog {
            status: result.status(),
            reason: result.reason().map(str::to_string),
            ..Default::default()
        }
        .emit();
        return;
    }
    let due = reward_state::get().last_sweep_attempt_timestamp_seconds == 0
        || now_secs.saturating_sub(reward_state::get().last_sweep_attempt_timestamp_seconds)
            >= REWARD_SWEEP_INTERVAL_SECONDS;
    if !force && !due {
        return;
    }
    reward_state::mutate(|reward| reward.last_sweep_attempt_timestamp_seconds = now_secs);
    let mut log = RewardLog {
        status: "held",
        ..Default::default()
    };
    if let Err(reason) = adjudicate(now_nanos, now_secs, &mut log).await {
        log.status = "failed";
        log.reason = Some(reason);
    }
    log.emit();
}

async fn adjudicate(now_nanos: u64, now_secs: u64, log: &mut RewardLog) -> Result<(), String> {
    let cfg = state::with_state(|st| st.config.clone());
    let Some(context) = reward_context(cfg.sns_rewards_canister_id)
        .await
        .map_err(|_| "reward_context_unavailable".to_string())?
    else {
        log.reason = Some("reward_context_unavailable".to_string());
        return Ok(());
    };
    log.root = Some(context.sns_root_canister_id);
    log.ledger = Some(context.sns_ledger_canister_id);
    log.snapshot_id = Some(context.snapshot_id);
    log.cutoff = Some(context.scan_started_at_timestamp_nanos);
    if now_nanos.saturating_sub(context.scan_completed_at_timestamp_nanos)
        > REWARD_CONTEXT_MAX_AGE_NANOS
    {
        log.reason = Some("reward_context_stale".to_string());
        return Ok(());
    }
    if let Err(reason) = apply_reward_epoch(context.sns_root_canister_id, now_secs) {
        log.reason = Some(reason.to_string());
        return Ok(());
    }
    let processed_from = reward_state::get().processed_through_commitment_tx_id;
    log.processed_from = processed_from;

    let reward_ledger = IcrcLedgerCanister::new(context.sns_ledger_canister_id);
    let reward_account = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: None,
    };
    let balance = match reward_ledger.balance_of(reward_account).await {
        Ok(value) => value,
        Err(_) => {
            log.reason = Some("reward_balance_read_failed".to_string());
            return Ok(());
        }
    };
    log.token_balance = Some(balance.clone());
    let fee = match reward_ledger.fee().await {
        Ok(value) => value,
        Err(_) => {
            log.reason = Some("reward_fee_read_failed".to_string());
            return Ok(());
        }
    };
    log.token_fee = Some(fee.clone());
    let amount = match economical_reward_amount(&balance, &fee) {
        Ok(amount) => amount,
        Err(reason) => {
            log.reason = Some(reason.to_string());
            return Ok(());
        }
    };
    log.token_amount = Some(amount.clone());

    let relay = ic_cdk::api::canister_self();
    let relay_account_identifier =
        account_identifier_text(relay, Some(logic::relay_subaccount_one()));
    let transactions = scan_history(
        cfg.icp_index_canister_id,
        relay_account_identifier,
        processed_from,
    )
    .await?;
    log.scanned_transactions = transactions.len();
    let staking_subaccount = NnsGovernanceCanister::new(cfg.governance_canister_id)
        .neuron_staking_subaccount(logic::JUPITER_FAUCET_NEURON_ID)
        .await
        .map_err(|_| "faucet_staking_account_unavailable".to_string())?;
    let faucet_account_identifier =
        account_identifier_text(cfg.governance_canister_id, Some(staking_subaccount));
    let batch = reconstruct_batch(
        &transactions,
        processed_from,
        context.scan_started_at_timestamp_nanos,
        &account_identifier_text(relay, Some(logic::relay_subaccount_one())),
        &faucet_account_identifier,
        &logic::relay_faucet_commitment_memo(relay)
            .map_err(|_| "commitment_memo_invalid".to_string())?,
    )?;
    let Some(batch) = batch else {
        log.reason = Some("no_new_completed_commitment".to_string());
        return Ok(());
    };
    log.processed_through = Some(batch.through_commitment_tx_id);
    log.completed_commitments = batch.completed_commitments;
    log.distinct_sources = batch.sources.len();
    if batch.sources.len() > ICP_HISTORY_MAX_DISTINCT_SOURCES {
        log.reason = Some("too_many_distinct_sources".to_string());
        return Ok(());
    }
    let requested = batch
        .sources
        .keys()
        .map(|account| account.to_vec())
        .collect::<Vec<_>>();
    let owners = match resolve_accounts(cfg.sns_rewards_canister_id, context.snapshot_id, requested)
        .await?
    {
        ResolveDefaultIcpAccountsResult::Ok(value) => value,
        ResolveDefaultIcpAccountsResult::SnapshotChanged => {
            log.reason = Some("reward_context_changed".to_string());
            return Ok(());
        }
        ResolveDefaultIcpAccountsResult::TooManyAccounts => {
            log.reason = Some("too_many_distinct_sources".to_string());
            return Ok(());
        }
        ResolveDefaultIcpAccountsResult::InvalidAccountIdentifier { index } => {
            return Err(format!("invalid_source_account_identifier_{index}"));
        }
    };
    if owners.len() != batch.sources.len() {
        return Err("owner_lookup_length_mismatch".to_string());
    }
    let (eligible, ineligible, mismatches) =
        classify_sources(batch.sources, batch.ineligible_e8s, owners)?;
    for principal in mismatches {
        ic_cdk::println!(
            "RELAY_SNS_REWARD_OWNER_MISMATCH principal={}",
            principal.to_text()
        );
    }
    log.eligible_principals = eligible.len();
    log.ineligible_e8s = Some(ineligible);
    let winner = select_winner(&eligible, ineligible);
    let Some((winner, winning_e8s)) = winner else {
        log.status = "no_winner";
        log.reason = Some(no_winner_reason(&eligible, ineligible).to_string());
        reward_state::mutate(|state| {
            state.processed_through_commitment_tx_id = Some(batch.through_commitment_tx_id)
        });
        return Ok(());
    };
    log.winner_e8s = Some(winning_e8s);
    log.recipient = Some(winner);
    let pending = PendingRewardTransfer {
        sns_root_canister_id: context.sns_root_canister_id,
        sns_ledger_canister_id: context.sns_ledger_canister_id,
        snapshot_id: context.snapshot_id,
        through_commitment_tx_id: batch.through_commitment_tx_id,
        recipient: Account {
            owner: winner,
            subaccount: None,
        },
        observed_balance: balance,
        fee,
        amount,
        memo: reward_memo(batch.through_commitment_tx_id),
        created_at_time_nanos: now_nanos,
        attempt_started: false,
        uncertain_attempt_seen: false,
        status: PendingRewardTransferStatus::AwaitingTransfer,
    };
    reward_state::mutate(|state| state.pending_transfer = Some(pending));
    log.status = "pending";
    let result = drive_pending(now_nanos).await;
    log.status = result.status();
    log.reason = result.reason().map(str::to_string);
    Ok(())
}

async fn scan_history(
    index_id: Principal,
    account_identifier: String,
    prior_cursor: Option<u64>,
) -> Result<Vec<IndexTransactionWithId>, String> {
    let index = IcpIndexCanister::new(index_id);
    let mut start = None;
    let mut transactions = Vec::new();
    let mut boundary_found = false;
    for _ in 0..ICP_HISTORY_MAX_PAGES {
        let page = index
            .get_account_identifier_transactions(
                account_identifier.clone(),
                start,
                ICP_HISTORY_PAGE_SIZE,
            )
            .await
            .map_err(|_| "history_read_failed".to_string())?;
        if page.transactions.is_empty() {
            boundary_found = prior_cursor.is_none();
            break;
        }
        for transaction in &page.transactions {
            if Some(transaction.id) == prior_cursor {
                boundary_found = true;
                break;
            }
            if prior_cursor.is_some_and(|cursor| transaction.id < cursor) {
                return Err("history_cursor_not_found".to_string());
            }
            transactions.push(transaction.clone());
            if transactions.len() >= ICP_HISTORY_MAX_TRANSACTIONS {
                return Err("history_limit_exceeded".to_string());
            }
        }
        if boundary_found {
            break;
        }
        let oldest_in_page = page.transactions.last().expect("nonempty page").id;
        if prior_cursor.is_none()
            && (page.transactions.len() < ICP_HISTORY_PAGE_SIZE as usize
                || page.oldest_tx_id == Some(oldest_in_page))
        {
            boundary_found = true;
            break;
        }
        if start == Some(oldest_in_page) {
            return Err("history_limit_exceeded".to_string());
        }
        start = Some(oldest_in_page);
    }
    if !boundary_found {
        return Err(if prior_cursor.is_some() {
            "history_cursor_not_found"
        } else {
            "history_limit_exceeded"
        }
        .to_string());
    }
    Ok(transactions)
}

#[derive(Debug, PartialEq, Eq)]
struct ContributionBatch {
    through_commitment_tx_id: u64,
    completed_commitments: usize,
    sources: BTreeMap<[u8; 32], u64>,
    ineligible_e8s: u64,
}

fn decode_account_identifier(text: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(text).ok()?;
    bytes.try_into().ok()
}

fn add_checked(target: &mut u64, amount: u64) -> Result<(), String> {
    *target = target
        .checked_add(amount)
        .ok_or_else(|| "contribution_overflow".to_string())?;
    Ok(())
}

fn economical_reward_amount(balance: &Nat, fee: &Nat) -> Result<Nat, &'static str> {
    if balance.0 == 0u8.into() {
        return Err("zero_reward_balance");
    }
    if balance.0 <= fee.0 {
        return Err("balance_not_above_fee");
    }
    let amount = Nat(balance.0.clone() - fee.0.clone());
    if fee.0.clone() * 10u8 > amount.0 {
        return Err("fee_over_10_percent");
    }
    Ok(amount)
}

fn apply_reward_epoch(root: Principal, now_secs: u64) -> Result<(), &'static str> {
    let current = reward_state::get();
    if current.epoch_sns_root_canister_id == Some(root) {
        return Ok(());
    }
    if current.pending_transfer.is_some() {
        return Err("pending_from_previous_sns");
    }
    reward_state::mutate(|state| {
        state.epoch_sns_root_canister_id = Some(root);
        state.processed_through_commitment_tx_id = None;
        state.last_sweep_attempt_timestamp_seconds = 0;
    });
    reward_state::mutate(|state| state.last_sweep_attempt_timestamp_seconds = now_secs);
    Ok(())
}

fn classify_sources(
    sources: BTreeMap<[u8; 32], u64>,
    initial_ineligible: u64,
    owners: Vec<Option<Principal>>,
) -> Result<(BTreeMap<Principal, u64>, u64, Vec<Principal>), String> {
    if owners.len() != sources.len() {
        return Err("owner_lookup_length_mismatch".to_string());
    }
    let mut eligible = BTreeMap::<Principal, u64>::new();
    let mut ineligible = initial_ineligible;
    let mut mismatches = Vec::new();
    for ((account, value), owner) in sources.into_iter().zip(owners) {
        match owner {
            Some(principal) if account_identifier_bytes(principal, None) == account => {
                add_checked(eligible.entry(principal).or_default(), value)?;
            }
            Some(principal) => {
                add_checked(&mut ineligible, value)?;
                mismatches.push(principal);
            }
            None => add_checked(&mut ineligible, value)?,
        }
    }
    Ok((eligible, ineligible, mismatches))
}

fn reconstruct_batch(
    transactions: &[IndexTransactionWithId],
    prior_cursor: Option<u64>,
    cutoff_nanos: u64,
    relay_account_identifier: &str,
    faucet_account_identifier: &str,
    commitment_memo: &[u8],
) -> Result<Option<ContributionBatch>, String> {
    let mut chronological = transactions.to_vec();
    chronological.sort_by_key(|transaction| transaction.id);
    let mut open_total = 0_u64;
    let mut open_sources = BTreeMap::<[u8; 32], u64>::new();
    let mut open_ineligible = 0_u64;
    let mut batch_sources = BTreeMap::<[u8; 32], u64>::new();
    let mut batch_ineligible = 0_u64;
    let mut through = None;
    let mut completed = 0usize;
    for entry in chronological {
        if prior_cursor.is_some_and(|cursor| entry.id <= cursor) {
            continue;
        }
        let timestamp = entry
            .transaction
            .timestamp
            .as_ref()
            .ok_or_else(|| "history_timestamp_missing".to_string())?
            .timestamp_nanos;
        if timestamp >= cutoff_nanos {
            continue;
        }
        match &entry.transaction.operation {
            IndexOperation::Transfer {
                to,
                from,
                amount,
                fee,
                ..
            } => {
                if from == relay_account_identifier {
                    let expected = to == faucet_account_identifier
                        && entry.transaction.icrc1_memo.as_deref() == Some(commitment_memo)
                        && amount.e8s() >= logic::MIN_RAW_ICP_RECIPIENT_AMOUNT_E8S;
                    if !expected {
                        return Err("unexpected_subaccount_debit".to_string());
                    }
                    let gross = amount
                        .e8s()
                        .checked_add(fee.e8s())
                        .ok_or_else(|| "commitment_reconciliation_failed".to_string())?;
                    if open_total != gross {
                        return Err("commitment_reconciliation_failed".to_string());
                    }
                    for (source, value) in std::mem::take(&mut open_sources) {
                        add_checked(batch_sources.entry(source).or_default(), value)?;
                    }
                    add_checked(&mut batch_ineligible, open_ineligible)?;
                    open_total = 0;
                    open_ineligible = 0;
                    through = Some(entry.id);
                    completed += 1;
                } else if to == relay_account_identifier {
                    add_checked(&mut open_total, amount.e8s())?;
                    if let Some(source) = decode_account_identifier(from) {
                        add_checked(open_sources.entry(source).or_default(), amount.e8s())?;
                    } else {
                        add_checked(&mut open_ineligible, amount.e8s())?;
                    }
                }
            }
            IndexOperation::TransferFrom {
                to, from, amount, ..
            } => {
                if from == relay_account_identifier {
                    return Err("unexpected_subaccount_debit".to_string());
                } else if to == relay_account_identifier {
                    add_checked(&mut open_total, amount.e8s())?;
                    if let Some(source) = decode_account_identifier(from) {
                        add_checked(open_sources.entry(source).or_default(), amount.e8s())?;
                    } else {
                        add_checked(&mut open_ineligible, amount.e8s())?;
                    }
                }
            }
            IndexOperation::Mint { to, amount } if to == relay_account_identifier => {
                add_checked(&mut open_total, amount.e8s())?;
                add_checked(&mut open_ineligible, amount.e8s())?;
            }
            IndexOperation::Burn { from, .. } if from == relay_account_identifier => {
                return Err("unexpected_subaccount_debit".to_string());
            }
            IndexOperation::Approve { from, fee, .. }
                if from == relay_account_identifier && fee.e8s() > 0 =>
            {
                return Err("unexpected_subaccount_debit".to_string());
            }
            _ => {}
        }
    }
    Ok(through.map(|through_commitment_tx_id| ContributionBatch {
        through_commitment_tx_id,
        completed_commitments: completed,
        sources: batch_sources,
        ineligible_e8s: batch_ineligible,
    }))
}

fn select_winner(eligible: &BTreeMap<Principal, u64>, ineligible: u64) -> Option<(Principal, u64)> {
    let mut ranked = eligible
        .iter()
        .map(|(principal, amount)| (*principal, *amount))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1));
    let (winner, amount) = *ranked.first()?;
    if amount <= ineligible || ranked.get(1).is_some_and(|second| second.1 == amount) {
        return None;
    }
    Some((winner, amount))
}

fn no_winner_reason(eligible: &BTreeMap<Principal, u64>, ineligible: u64) -> &'static str {
    if eligible.is_empty() {
        return "no_eligible_owner";
    }
    let max = eligible.values().copied().max().unwrap_or(0);
    if eligible.values().filter(|amount| **amount == max).count() > 1 {
        "contributor_tie"
    } else if ineligible >= max {
        "ineligible_not_smaller"
    } else {
        "no_eligible_owner"
    }
}

fn reward_memo(through_commitment_tx_id: u64) -> Vec<u8> {
    let mut memo = REWARD_MEMO_PREFIX.to_vec();
    memo.extend_from_slice(&through_commitment_tx_id.to_be_bytes());
    memo
}

enum AttemptOutcome {
    Accepted,
    RetryableExplicit,
    DefinitiveRejected,
    BadFee(Nat),
    Uncertain,
}

async fn transfer_once(
    ledger: &IcrcLedgerCanister,
    pending: &PendingRewardTransfer,
) -> AttemptOutcome {
    let arg = TransferArg {
        from_subaccount: None,
        to: pending.recipient,
        fee: Some(pending.fee.clone()),
        created_at_time: Some(pending.created_at_time_nanos),
        memo: Some(Memo::from(pending.memo.clone())),
        amount: pending.amount.clone(),
    };
    match ledger.transfer(arg).await {
        Ok(Ok(_)) | Ok(Err(TransferError::Duplicate { .. })) => AttemptOutcome::Accepted,
        Ok(Err(TransferError::BadFee { expected_fee })) => AttemptOutcome::BadFee(expected_fee),
        Ok(Err(
            TransferError::TemporarilyUnavailable
            | TransferError::CreatedInFuture { .. }
            | TransferError::GenericError { .. },
        )) => AttemptOutcome::RetryableExplicit,
        Ok(Err(
            TransferError::BadBurn { .. }
            | TransferError::InsufficientFunds { .. }
            | TransferError::TooOld,
        )) => AttemptOutcome::DefinitiveRejected,
        Err(_) => AttemptOutcome::Uncertain,
    }
}

fn accept_pending(reason: &str) {
    reward_state::mutate(|state| {
        if let Some(pending) = state.pending_transfer.take() {
            state.processed_through_commitment_tx_id = Some(pending.through_commitment_tx_id);
        }
    });
    ic_cdk::println!("RELAY_SNS_REWARD_TRANSFER status=accepted reason={reason}");
}

fn clear_rejected(reason: &str) {
    reward_state::mutate(|state| state.pending_transfer = None);
    ic_cdk::println!("RELAY_SNS_REWARD_TRANSFER status=rejected reason={reason}");
}

fn mark_ambiguous() {
    reward_state::mutate(|state| {
        if let Some(pending) = state.pending_transfer.as_mut() {
            pending.uncertain_attempt_seen = true;
            pending.status = PendingRewardTransferStatus::Ambiguous;
        }
    });
    ic_cdk::println!("RELAY_SNS_REWARD_TRANSFER status=ambiguous reason=reward_transfer_ambiguous");
}

#[derive(Clone, Copy)]
enum PendingDriveResult {
    Accepted,
    Rejected,
    Ambiguous,
    Held,
}

impl PendingDriveResult {
    fn status(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "failed",
            Self::Ambiguous => "ambiguous",
            Self::Held => "held",
        }
    }
    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Rejected => Some("reward_transfer_rejected"),
            Self::Ambiguous | Self::Held => Some("reward_transfer_ambiguous"),
            Self::Accepted => None,
        }
    }
}

async fn drive_pending(_now_nanos: u64) -> PendingDriveResult {
    let Some(pending) = reward_state::get().pending_transfer else {
        return PendingDriveResult::Held;
    };
    let ledger = IcrcLedgerCanister::new(pending.sns_ledger_canister_id);
    if pending.status == PendingRewardTransferStatus::Ambiguous {
        let relay_account = Account {
            owner: ic_cdk::api::canister_self(),
            subaccount: None,
        };
        if let Ok(current) = ledger.balance_of(relay_account).await {
            if current.0 < pending.observed_balance.0 {
                accept_pending("accepted_by_balance_reconciliation");
                return PendingDriveResult::Accepted;
            }
        }
        return PendingDriveResult::Held;
    }
    let restored_started_attempt = pending.attempt_started;
    reward_state::mutate(|state| {
        if let Some(pending) = state.pending_transfer.as_mut() {
            pending.attempt_started = true;
            if restored_started_attempt {
                pending.uncertain_attempt_seen = true;
            }
        }
    });
    let first = transfer_once(&ledger, &pending).await;
    match first {
        AttemptOutcome::Accepted => {
            accept_pending("accepted_or_duplicate");
            PendingDriveResult::Accepted
        }
        AttemptOutcome::BadFee(expected) if !restored_started_attempt => {
            ic_cdk::println!("RELAY_SNS_REWARD_TRANSFER status=rejected reason=bad_fee planned_fee={} expected_fee={}", pending.fee, expected);
            clear_rejected("bad_fee");
            PendingDriveResult::Rejected
        }
        AttemptOutcome::DefinitiveRejected if !restored_started_attempt => {
            clear_rejected("definitive_rejection");
            PendingDriveResult::Rejected
        }
        AttemptOutcome::RetryableExplicit | AttemptOutcome::Uncertain
            if !restored_started_attempt =>
        {
            let uncertainty = matches!(first, AttemptOutcome::Uncertain);
            if uncertainty {
                reward_state::mutate(|state| {
                    if let Some(pending) = state.pending_transfer.as_mut() {
                        pending.uncertain_attempt_seen = true;
                    }
                });
            }
            reward_state::mutate(|state| {
                if let Some(pending) = state.pending_transfer.as_mut() {
                    pending.attempt_started = true;
                }
            });
            match transfer_once(&ledger, &pending).await {
                AttemptOutcome::Accepted => {
                    accept_pending("accepted_or_duplicate_after_retry");
                    PendingDriveResult::Accepted
                }
                _ if uncertainty => {
                    mark_ambiguous();
                    PendingDriveResult::Ambiguous
                }
                AttemptOutcome::Uncertain => {
                    mark_ambiguous();
                    PendingDriveResult::Ambiguous
                }
                _ => {
                    clear_rejected("retry_rejected");
                    PendingDriveResult::Rejected
                }
            }
        }
        _ => {
            mark_ambiguous();
            PendingDriveResult::Ambiguous
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_ic_clients::index::{IndexTimeStamp, IndexTransaction, Tokens};

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }
    fn pending(root: Principal) -> PendingRewardTransfer {
        PendingRewardTransfer {
            sns_root_canister_id: root,
            sns_ledger_canister_id: principal(9),
            snapshot_id: 3,
            through_commitment_tx_id: 44,
            recipient: Account {
                owner: principal(8),
                subaccount: None,
            },
            observed_balance: Nat::from(1_000_u64),
            fee: Nat::from(10_u64),
            amount: Nat::from(990_u64),
            memo: reward_memo(44),
            created_at_time_nanos: 55,
            attempt_started: true,
            uncertain_attempt_seen: false,
            status: PendingRewardTransferStatus::AwaitingTransfer,
        }
    }
    fn tx(
        id: u64,
        timestamp: u64,
        operation: IndexOperation,
        memo: Option<Vec<u8>>,
    ) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: memo,
                operation,
                created_at_time: Some(IndexTimeStamp {
                    timestamp_nanos: 999_999,
                }),
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: timestamp,
                }),
            },
        }
    }
    fn transfer(
        id: u64,
        timestamp: u64,
        from: String,
        to: String,
        amount: u64,
        fee: u64,
        memo: Option<Vec<u8>>,
    ) -> IndexTransactionWithId {
        tx(
            id,
            timestamp,
            IndexOperation::Transfer {
                from,
                to,
                amount: Tokens::new(amount),
                fee: Tokens::new(fee),
                spender: None,
            },
            memo,
        )
    }

    fn transfer_from(
        id: u64,
        timestamp: u64,
        from: String,
        to: String,
        spender: String,
        amount: u64,
    ) -> IndexTransactionWithId {
        tx(
            id,
            timestamp,
            IndexOperation::TransferFrom {
                from,
                to,
                spender,
                amount: Tokens::new(amount),
                fee: Tokens::new(10_000),
            },
            None,
        )
    }

    #[test]
    fn reconstructs_multiple_completed_segments_and_ignores_trailing_deposit() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let source = "33".repeat(32);
        let memo = b"relay".to_vec();
        let history = vec![
            transfer(
                1,
                1,
                source.clone(),
                relay.clone(),
                100_010_000,
                10_000,
                None,
            ),
            transfer(
                2,
                2,
                relay.clone(),
                faucet.clone(),
                100_000_000,
                10_000,
                Some(memo.clone()),
            ),
            transfer(
                3,
                3,
                source.clone(),
                relay.clone(),
                200_010_000,
                10_000,
                None,
            ),
            transfer(
                4,
                4,
                relay.clone(),
                faucet.clone(),
                200_000_000,
                10_000,
                Some(memo.clone()),
            ),
            transfer(5, 5, source, relay.clone(), 50_000_000, 10_000, None),
        ];
        let batch = reconstruct_batch(&history, None, 10, &relay, &faucet, &memo)
            .unwrap()
            .unwrap();
        assert_eq!(batch.through_commitment_tx_id, 4);
        assert_eq!(batch.completed_commitments, 2);
        assert_eq!(batch.sources.values().sum::<u64>(), 300_020_000);
    }

    #[test]
    fn cutoff_is_exclusive_and_uses_ledger_timestamp() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let source = "33".repeat(32);
        let memo = b"relay".to_vec();
        let history = vec![
            transfer(1, 9, source.clone(), relay.clone(), 100_010_000, 0, None),
            transfer(
                2,
                10,
                relay.clone(),
                faucet.clone(),
                100_000_000,
                10_000,
                Some(memo.clone()),
            ),
        ];
        assert!(
            reconstruct_batch(&history, None, 10, &relay, &faucet, &memo)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prior_commitment_boundary_is_exclusive() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let first = "33".repeat(32);
        let second = "44".repeat(32);
        let memo = b"relay".to_vec();
        let history = vec![
            transfer(1, 1, first, relay.clone(), 100_010_000, 0, None),
            transfer(
                2,
                2,
                relay.clone(),
                faucet.clone(),
                100_000_000,
                10_000,
                Some(memo.clone()),
            ),
            transfer(3, 3, second.clone(), relay.clone(), 200_010_000, 0, None),
            transfer(
                4,
                4,
                relay.clone(),
                faucet.clone(),
                200_000_000,
                10_000,
                Some(memo.clone()),
            ),
        ];
        let batch = reconstruct_batch(&history, Some(2), 10, &relay, &faucet, &memo)
            .unwrap()
            .unwrap();
        assert_eq!(batch.through_commitment_tx_id, 4);
        assert_eq!(batch.completed_commitments, 1);
        assert_eq!(
            batch.sources,
            BTreeMap::from([(decode_account_identifier(&second).unwrap(), 200_010_000)])
        );
    }

    #[test]
    fn no_completed_commitment_ignores_open_deposits() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let source = "33".repeat(32);
        assert!(reconstruct_batch(
            &[transfer(1, 1, source, relay.clone(), 100_000_000, 0, None)],
            None,
            10,
            &relay,
            &faucet,
            b"relay",
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn mint_and_transfer_from_reconcile_and_use_from_account() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let source = "33".repeat(32);
        let spender = "44".repeat(32);
        let memo = b"relay".to_vec();
        let history = vec![
            transfer_from(1, 1, source.clone(), relay.clone(), spender, 60_000_000),
            tx(
                2,
                2,
                IndexOperation::Mint {
                    to: relay.clone(),
                    amount: Tokens::new(40_010_000),
                },
                None,
            ),
            transfer(
                3,
                3,
                relay.clone(),
                faucet.clone(),
                100_000_000,
                10_000,
                Some(memo.clone()),
            ),
        ];
        let batch = reconstruct_batch(&history, None, 10, &relay, &faucet, &memo)
            .unwrap()
            .unwrap();
        assert_eq!(
            batch.sources[&decode_account_identifier(&source).unwrap()],
            60_000_000
        );
        assert_eq!(batch.ineligible_e8s, 40_010_000);
    }

    #[test]
    fn reconciliation_rejects_under_over_and_unexpected_debits() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let source = "33".repeat(32);
        let memo = b"relay".to_vec();
        for credited in [100_009_999, 100_010_001] {
            let history = vec![
                transfer(1, 1, source.clone(), relay.clone(), credited, 0, None),
                transfer(
                    2,
                    2,
                    relay.clone(),
                    faucet.clone(),
                    100_000_000,
                    10_000,
                    Some(memo.clone()),
                ),
            ];
            assert_eq!(
                reconstruct_batch(&history, None, 10, &relay, &faucet, &memo).unwrap_err(),
                "commitment_reconciliation_failed"
            );
        }
        let history = vec![transfer(1, 1, relay.clone(), source, 1, 10_000, None)];
        assert_eq!(
            reconstruct_batch(&history, None, 10, &relay, &faucet, &memo).unwrap_err(),
            "unexpected_subaccount_debit"
        );
    }

    #[test]
    fn unsupported_debits_and_arithmetic_overflow_fail_closed() {
        let relay = "11".repeat(32);
        let faucet = "22".repeat(32);
        let source = "33".repeat(32);
        for operation in [
            IndexOperation::Burn {
                from: relay.clone(),
                amount: Tokens::new(1),
                spender: None,
            },
            IndexOperation::Approve {
                from: relay.clone(),
                spender: source.clone(),
                allowance: Tokens::new(1),
                fee: Tokens::new(1),
                expires_at: None,
                expected_allowance: None,
            },
        ] {
            assert_eq!(
                reconstruct_batch(
                    &[tx(1, 1, operation, None)],
                    None,
                    10,
                    &relay,
                    &faucet,
                    b"relay"
                )
                .unwrap_err(),
                "unexpected_subaccount_debit"
            );
        }
        let overflow = vec![
            transfer(1, 1, source.clone(), relay.clone(), u64::MAX, 0, None),
            transfer(2, 2, source, relay.clone(), 1, 0, None),
        ];
        assert_eq!(
            reconstruct_batch(&overflow, None, 10, &relay, &faucet, b"relay").unwrap_err(),
            "contribution_overflow"
        );
        let self_transfer = transfer(1, 1, relay.clone(), relay.clone(), 1, 1, None);
        assert_eq!(
            reconstruct_batch(&[self_transfer], None, 10, &relay, &faucet, b"relay").unwrap_err(),
            "unexpected_subaccount_debit"
        );
    }

    #[test]
    fn winner_requires_unique_largest_and_more_than_ineligible() {
        let a = principal(1);
        let b = principal(2);
        assert_eq!(
            select_winner(&BTreeMap::from([(a, 11), (b, 10)]), 10),
            Some((a, 11))
        );
        assert_eq!(select_winner(&BTreeMap::from([(a, 10), (b, 10)]), 0), None);
        assert_eq!(select_winner(&BTreeMap::from([(a, 10)]), 10), None);
        assert_eq!(select_winner(&BTreeMap::from([(a, 10)]), 11), None);
        assert_eq!(select_winner(&BTreeMap::new(), 0), None);
    }

    #[test]
    fn dynamic_fee_rule_allows_exactly_ten_percent_of_net() {
        assert_eq!(
            economical_reward_amount(&Nat::from(110_u64), &Nat::from(10_u64)),
            Ok(Nat::from(100_u64))
        );
        assert_eq!(
            economical_reward_amount(&Nat::from(111_u64), &Nat::from(11_u64)),
            Err("fee_over_10_percent")
        );
        assert_eq!(
            economical_reward_amount(&Nat::from(0_u64), &Nat::from(0_u64)),
            Err("zero_reward_balance")
        );
        assert_eq!(
            economical_reward_amount(&Nat::from(10_u64), &Nat::from(10_u64)),
            Err("balance_not_above_fee")
        );
    }

    #[test]
    fn source_classification_rederives_default_accounts_and_buckets_unknowns() {
        let owner = principal(1);
        let wrong_owner = principal(2);
        let default = account_identifier_bytes(owner, None);
        let explicit = account_identifier_bytes(owner, Some([1; 32]));
        let unknown = [9; 32];
        let sources = BTreeMap::from([(default, 50), (explicit, 30), (unknown, 20)]);
        let owners = sources
            .keys()
            .map(|account| {
                if *account == default {
                    Some(owner)
                } else if *account == explicit {
                    None
                } else {
                    Some(wrong_owner)
                }
            })
            .collect();
        let (eligible, ineligible, mismatches) = classify_sources(sources, 10, owners).unwrap();
        assert_eq!(eligible, BTreeMap::from([(owner, 50)]));
        assert_eq!(ineligible, 60);
        assert_eq!(mismatches, vec![wrong_owner]);
    }

    #[test]
    fn source_classification_rejects_lookup_length_mismatch_and_overflow() {
        assert_eq!(
            classify_sources(BTreeMap::from([([1; 32], 1)]), 0, vec![]).unwrap_err(),
            "owner_lookup_length_mismatch"
        );
        assert_eq!(
            classify_sources(BTreeMap::from([([1; 32], 1)]), u64::MAX, vec![None]).unwrap_err(),
            "contribution_overflow"
        );
    }

    #[test]
    fn reward_memo_is_versioned_and_pins_commitment() {
        assert_eq!(
            reward_memo(7),
            [b"JRS1".as_slice(), &7_u64.to_be_bytes()].concat()
        );
    }

    #[test]
    fn root_epoch_change_resets_cursor_but_same_root_preserves_it() {
        reward_state::reset_for_test();
        let first = principal(1);
        let second = principal(2);
        reward_state::mutate(|state| {
            state.epoch_sns_root_canister_id = Some(first);
            state.processed_through_commitment_tx_id = Some(7);
            state.last_sweep_attempt_timestamp_seconds = 8;
        });
        apply_reward_epoch(first, 100).unwrap();
        assert_eq!(
            reward_state::get().processed_through_commitment_tx_id,
            Some(7)
        );
        assert_eq!(reward_state::get().last_sweep_attempt_timestamp_seconds, 8);

        apply_reward_epoch(second, 100).unwrap();
        let changed = reward_state::get();
        assert_eq!(changed.epoch_sns_root_canister_id, Some(second));
        assert_eq!(changed.processed_through_commitment_tx_id, None);
        assert_eq!(changed.last_sweep_attempt_timestamp_seconds, 100);
    }

    #[test]
    fn unresolved_pending_transfer_blocks_root_change_without_mutating_identity() {
        reward_state::reset_for_test();
        let old_root = principal(1);
        let staged = pending(old_root);
        reward_state::mutate(|state| {
            state.epoch_sns_root_canister_id = Some(old_root);
            state.processed_through_commitment_tx_id = Some(7);
            state.pending_transfer = Some(staged.clone());
        });
        assert_eq!(
            apply_reward_epoch(principal(2), 100),
            Err("pending_from_previous_sns")
        );
        let held = reward_state::get();
        assert_eq!(held.epoch_sns_root_canister_id, Some(old_root));
        assert_eq!(held.processed_through_commitment_tx_id, Some(7));
        assert_eq!(held.pending_transfer, Some(staged));
    }

    #[test]
    fn accepted_rejected_and_ambiguous_transitions_are_cursor_safe() {
        reward_state::reset_for_test();
        let staged = pending(principal(1));
        reward_state::mutate(|state| state.pending_transfer = Some(staged.clone()));
        mark_ambiguous();
        let ambiguous = reward_state::get().pending_transfer.unwrap();
        assert_eq!(ambiguous.recipient, staged.recipient);
        assert_eq!(ambiguous.amount, staged.amount);
        assert_eq!(ambiguous.fee, staged.fee);
        assert_eq!(ambiguous.memo, staged.memo);
        assert_eq!(
            ambiguous.created_at_time_nanos,
            staged.created_at_time_nanos
        );
        assert!(ambiguous.uncertain_attempt_seen);
        assert_eq!(ambiguous.status, PendingRewardTransferStatus::Ambiguous);
        assert_eq!(reward_state::get().processed_through_commitment_tx_id, None);

        clear_rejected("test");
        assert!(reward_state::get().pending_transfer.is_none());
        assert_eq!(reward_state::get().processed_through_commitment_tx_id, None);

        reward_state::mutate(|state| state.pending_transfer = Some(staged));
        accept_pending("test");
        assert!(reward_state::get().pending_transfer.is_none());
        assert_eq!(
            reward_state::get().processed_through_commitment_tx_id,
            Some(44)
        );
    }
}
