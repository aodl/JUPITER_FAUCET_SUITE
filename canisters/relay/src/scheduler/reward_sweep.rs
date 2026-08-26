#![cfg_attr(test, allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use async_trait::async_trait;
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, Memo, TransferArg, TransferError};
use jupiter_ic_clients::account_identifier::{account_identifier_bytes, account_identifier_text};
use jupiter_ic_clients::icrc_index::IcrcIndexCanister;
use jupiter_ic_clients::index::{IcpIndexCanister, IndexOperation, IndexTransactionWithId};
use jupiter_ic_clients::ledger::IcrcLedgerCanister;
use jupiter_ic_clients::sns::{ListSnsCanistersResponse, SnsRootCanister};

use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::GovernanceClient;
use crate::reward_state::{
    self, PendingRewardPayout, PendingRewardRecipient, PendingRewardTransferStatus,
};
use crate::scheduler::reward_history;
use crate::scheduler::reward_splitter::{self, SplitterFundingCredit};
use crate::scheduler::reward_token_history;
use crate::scheduler::transfer::created_at_time_is_valid;
use crate::{logic, state};

pub(crate) const REWARD_SWEEP_INTERVAL_SECONDS: u64 = 7 * 24 * 60 * 60;
const REWARD_CONTEXT_MAX_AGE_NANOS: u64 = 48 * 60 * 60 * 1_000_000_000;
const OWNER_RESOLUTION_CHUNK_SIZE: usize = 128;
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
    attribution_commitment: Option<u64>,
    scanned_transactions: usize,
    splitters_scanned: usize,
    distinct_sources: usize,
    eligible_principals: usize,
    eligible_e8s: Option<u64>,
    owner_mismatch_count: usize,
    ineligible_e8s: Option<u64>,
    recipient_count: usize,
    token_balance: Option<Nat>,
    token_fee: Option<Nat>,
}

impl RewardLog {
    fn emit(&self) {
        fn opt<T: ToString>(value: Option<T>) -> String {
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string())
        }
        ic_cdk::println!(
            "RELAY_SNS_REWARD status={} reason={} sns_root_canister_id={} sns_ledger_canister_id={} snapshot_id={} attribution_cutoff_ts_nanos={} attribution_commitment_tx_id={} scanned_transactions={} splitters_scanned={} distinct_sources={} eligible_principals={} eligible_icp_e8s={} ineligible_icp_e8s={} recipient_count={} owner_mismatch_count={} token_balance={} token_fee={}",
            self.status,
            self.reason.as_deref().map(jupiter_canister_logging::escape_value).unwrap_or_else(|| "null".to_string()),
            self.root.map(|v| v.to_text()).unwrap_or_else(|| "null".to_string()),
            self.ledger.map(|v| v.to_text()).unwrap_or_else(|| "null".to_string()),
            opt(self.snapshot_id), opt(self.cutoff), opt(self.attribution_commitment),
            self.scanned_transactions, self.splitters_scanned, self.distinct_sources,
            self.eligible_principals, opt(self.eligible_e8s), opt(self.ineligible_e8s),
            self.recipient_count, self.owner_mismatch_count,
            opt(self.token_balance.as_ref()), opt(self.token_fee.as_ref())
        );
    }
}

fn validate_reward_index_components(
    context: &RelayRewardContext,
    components: ListSnsCanistersResponse,
) -> Result<Principal, String> {
    if components.root != Some(context.sns_root_canister_id)
        || components.ledger != Some(context.sns_ledger_canister_id)
    {
        return Err("reward_index_component_mismatch".to_string());
    }
    components
        .index
        .ok_or_else(|| "reward_index_unavailable".to_string())
}

fn effective_attribution_cutoff(snapshot_cutoff: u64, reward_pot_cutoff: u64) -> u64 {
    snapshot_cutoff.min(reward_pot_cutoff)
}

fn validate_icp_index_tip(
    ledger_chain_length: u64,
    index_blocks_synced: u64,
) -> Result<(), String> {
    if index_blocks_synced < ledger_chain_length {
        Err("icp_index_not_caught_up".to_string())
    } else {
        Ok(())
    }
}

async fn resolve_reward_index(context: &RelayRewardContext) -> Result<IcrcIndexCanister, String> {
    let components = SnsRootCanister
        .list_sns_canisters(context.sns_root_canister_id)
        .await
        .map_err(|_| "reward_index_unavailable".to_string())?;
    let index_id = validate_reward_index_components(context, components)?;
    let index = IcrcIndexCanister::new(index_id);
    let indexed_ledger = index
        .ledger_id()
        .await
        .map_err(|_| "reward_index_unavailable".to_string())?;
    if indexed_ledger != context.sns_ledger_canister_id {
        return Err("reward_index_component_mismatch".to_string());
    }
    Ok(index)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SweepDisposition {
    Completed,
    RetryNextDailyTick,
}

fn record_sweep_disposition(disposition: SweepDisposition, now_secs: u64) {
    if disposition == SweepDisposition::Completed {
        reward_state::mutate(|reward| {
            reward.last_sweep_attempt_timestamp_seconds = now_secs;
        });
    }
}

fn sweep_is_due(last_completed_sweep_secs: u64, now_secs: u64, force: bool) -> bool {
    force
        || last_completed_sweep_secs == 0
        || now_secs.saturating_sub(last_completed_sweep_secs) >= REWARD_SWEEP_INTERVAL_SECONDS
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

#[async_trait]
trait OwnerResolverClient: Send + Sync {
    async fn resolve(
        &self,
        snapshot_id: u64,
        accounts: Vec<Vec<u8>>,
    ) -> Result<ResolveDefaultIcpAccountsResult, String>;
}

#[async_trait]
impl OwnerResolverClient for Principal {
    async fn resolve(
        &self,
        snapshot_id: u64,
        accounts: Vec<Vec<u8>>,
    ) -> Result<ResolveDefaultIcpAccountsResult, String> {
        resolve_accounts(*self, snapshot_id, accounts).await
    }
}

async fn resolve_accounts_chunked<R: OwnerResolverClient>(
    resolver: &R,
    snapshot_id: u64,
    accounts: &BTreeMap<[u8; 32], u64>,
) -> Result<Vec<Option<Principal>>, String> {
    let ordered = accounts.keys().copied().collect::<Vec<_>>();
    let mut owners = Vec::with_capacity(ordered.len());
    for chunk in ordered.chunks(OWNER_RESOLUTION_CHUNK_SIZE) {
        let response = resolver
            .resolve(
                snapshot_id,
                chunk.iter().map(|account| account.to_vec()).collect(),
            )
            .await
            .map_err(|_| "owner_lookup_failed".to_string())?;
        match response {
            ResolveDefaultIcpAccountsResult::Ok(mut resolved) => {
                if resolved.len() != chunk.len() {
                    return Err("owner_lookup_length_mismatch".to_string());
                }
                owners.append(&mut resolved);
            }
            ResolveDefaultIcpAccountsResult::SnapshotChanged => {
                return Err("reward_context_changed".to_string());
            }
            ResolveDefaultIcpAccountsResult::TooManyAccounts => {
                return Err("owner_lookup_chunk_rejected".to_string());
            }
            ResolveDefaultIcpAccountsResult::InvalidAccountIdentifier { index } => {
                return Err(format!("invalid_source_account_identifier_{index}"));
            }
        }
    }
    Ok(owners)
}

pub(crate) async fn process(now_nanos: u64, now_secs: u64, force: bool) {
    if reward_state::get().pending_payout.is_some() {
        let result = drive_pending(now_nanos).await;
        RewardLog {
            status: result.status(),
            reason: result.reason().map(str::to_string),
            ..Default::default()
        }
        .emit();
        return;
    }
    if !sweep_is_due(
        reward_state::get().last_sweep_attempt_timestamp_seconds,
        now_secs,
        force,
    ) {
        return;
    }
    let mut log = RewardLog {
        status: "held",
        ..Default::default()
    };
    let disposition = match adjudicate(now_nanos, &mut log).await {
        Ok(disposition) => disposition,
        Err(reason) => {
            log.status = "failed";
            log.reason = Some(reason);
            SweepDisposition::RetryNextDailyTick
        }
    };
    record_sweep_disposition(disposition, now_secs);
    log.emit();
}

async fn adjudicate(now_nanos: u64, log: &mut RewardLog) -> Result<SweepDisposition, String> {
    let cfg = state::with_state(|st| st.config.clone());
    let Some(context) = reward_context(cfg.sns_rewards_canister_id)
        .await
        .map_err(|_| "reward_context_unavailable".to_string())?
    else {
        return Err("reward_context_unavailable".to_string());
    };
    log.root = Some(context.sns_root_canister_id);
    log.ledger = Some(context.sns_ledger_canister_id);
    log.snapshot_id = Some(context.snapshot_id);
    if now_nanos.saturating_sub(context.scan_completed_at_timestamp_nanos)
        > REWARD_CONTEXT_MAX_AGE_NANOS
    {
        return Err("reward_context_stale".to_string());
    }
    let reward_ledger = IcrcLedgerCanister::new(context.sns_ledger_canister_id);
    let reward_account = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: None,
    };
    let balance = match reward_ledger.balance_of(reward_account).await {
        Ok(value) => value,
        Err(_) => return Err("reward_balance_read_failed".to_string()),
    };
    log.token_balance = Some(balance.clone());
    let fee = match reward_ledger.fee().await {
        Ok(value) => value,
        Err(_) => return Err("reward_fee_read_failed".to_string()),
    };
    log.token_fee = Some(fee.clone());
    if let Some(reason) = terminal_reward_pot_hold_reason(&balance, &fee) {
        log.reason = Some(reason.to_string());
        return Ok(SweepDisposition::Completed);
    }

    let reward_index = resolve_reward_index(&context).await?;
    let reward_pot_cutoff =
        reward_token_history::reward_pot_cutoff(&reward_index, reward_account, balance.clone())
            .await?;
    // This is a conservative ledger-time recency heuristic across two independent ledgers, not
    // a cryptographic causal ordering. A Faucet commitment must be recorded strictly before both
    // the owner-snapshot scan and the oldest unspent reward credit in the current pot.
    let attribution_cutoff =
        effective_attribution_cutoff(context.scan_started_at_timestamp_nanos, reward_pot_cutoff);
    log.cutoff = Some(attribution_cutoff);

    // Observe the authoritative Ledger tip first, then require the Index to contain that entire
    // prefix. Blocks created after this Ledger read cannot carry a pre-existing attribution-cutoff
    // timestamp, while paying from a stale ICP account history could select the wrong funder.
    let icp_ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    let ledger_chain_length = icp_ledger
        .chain_length()
        .await
        .map_err(|_| "icp_history_sync_unavailable".to_string())?;
    let index = IcpIndexCanister::new(cfg.icp_index_canister_id);
    let index_blocks_synced = index
        .status()
        .await
        .map_err(|_| "icp_history_sync_unavailable".to_string())?
        .num_blocks_synced;
    validate_icp_index_tip(ledger_chain_length, index_blocks_synced)?;

    let relay = ic_cdk::api::canister_self();
    let relay_account_identifier =
        account_identifier_text(relay, Some(logic::relay_subaccount_one()));
    let mut history = reward_history::BackwardHistory::new(relay_account_identifier);
    let staking_subaccount = NnsGovernanceCanister::new(cfg.governance_canister_id)
        .neuron_staking_subaccount(logic::JUPITER_FAUCET_NEURON_ID)
        .await
        .map_err(|_| "faucet_staking_account_unavailable".to_string())?;
    let faucet_account_identifier =
        account_identifier_text(cfg.governance_canister_id, Some(staking_subaccount));
    let intrinsic_splitters = reward_splitter::intrinsic_splitter_accounts(relay);
    let commitment_memo = logic::relay_faucet_commitment_memo(relay)
        .map_err(|_| "commitment_memo_invalid".to_string())?;
    let relay_account_identifier =
        account_identifier_text(relay, Some(logic::relay_subaccount_one()));
    let mut examined_commitments = BTreeSet::new();
    let mut splitter_cache = reward_splitter::SplitterHistoryCache::new();
    let mut splitter_transactions_scanned = 0_usize;

    loop {
        if history.transactions().is_empty() && !history.exhausted() {
            history.extend(&index).await?;
            log.scanned_transactions = history.transactions().len();
        }
        let batches = match reconstruct_batches_with_splitters(
            history.authoritative_transactions(),
            history.authoritative(),
            attribution_cutoff,
            &relay_account_identifier,
            &faucet_account_identifier,
            &commitment_memo,
            &intrinsic_splitters,
        ) {
            reward_history::HistoricalReconstruction::Complete(batches) => batches,
            reward_history::HistoricalReconstruction::NeedOlderHistory => {
                if history.exhausted() {
                    return Err("commitment_reconciliation_failed".to_string());
                }
                history.extend(&index).await?;
                log.scanned_transactions = history
                    .transactions()
                    .len()
                    .checked_add(splitter_transactions_scanned)
                    .ok_or_else(|| "history_count_overflow".to_string())?;
                continue;
            }
            reward_history::HistoricalReconstruction::Malformed(error) => return Err(error),
        };

        for batch in batches.into_iter().rev() {
            if !examined_commitments.insert(batch.commitment_tx_id) {
                continue;
            }
            let expanded = splitter_cache
                .expand(&index, relay, &batch.splitter_credits)
                .await?;
            splitter_transactions_scanned = splitter_transactions_scanned
                .checked_add(expanded.scanned_transactions)
                .ok_or_else(|| "history_count_overflow".to_string())?;
            log.scanned_transactions = log
                .scanned_transactions
                .checked_add(expanded.scanned_transactions)
                .ok_or_else(|| "history_count_overflow".to_string())?;
            log.splitters_scanned = log
                .splitters_scanned
                .checked_add(expanded.splitters_scanned)
                .ok_or_else(|| "splitter_count_overflow".to_string())?;
            let (final_sources, final_ineligible) =
                merge_attribution(batch.sources, batch.ineligible_e8s, &expanded)?;
            log.distinct_sources = final_sources.len();
            let owners = resolve_accounts_chunked(
                &cfg.sns_rewards_canister_id,
                context.snapshot_id,
                &final_sources,
            )
            .await?;
            let (eligible, ineligible, owner_mismatch_count) =
                classify_sources(final_sources, final_ineligible, owners)?;
            log.ineligible_e8s = Some(ineligible);
            log.owner_mismatch_count = owner_mismatch_count;
            if eligible.is_empty() {
                continue;
            }

            let allocations = match proportional_reward_allocations(&balance, &fee, &eligible) {
                Ok(allocations) => allocations,
                Err(reason) => {
                    log.attribution_commitment = Some(batch.commitment_tx_id);
                    log.eligible_principals = eligible.len();
                    log.eligible_e8s = Some(sum_eligible_weight(&eligible)?);
                    log.reason = Some(reason.to_string());
                    return Ok(SweepDisposition::Completed);
                }
            };
            log.attribution_commitment = Some(batch.commitment_tx_id);
            log.eligible_principals = eligible.len();
            log.eligible_e8s = Some(sum_eligible_weight(&eligible)?);
            log.recipient_count = allocations.len();
            let pending = build_pending_payout(
                &context,
                batch.commitment_tx_id,
                fee,
                allocations,
                now_nanos,
            )?;
            reward_state::mutate(|state| state.pending_payout = Some(pending));
            log.status = "pending";
            let result = drive_pending(now_nanos).await;
            log.status = result.status();
            log.reason = result.reason().map(str::to_string);
            return Ok(result.sweep_disposition());
        }

        if history.exhausted() {
            log.reason = Some("no_eligible_historical_commitment".to_string());
            return Ok(SweepDisposition::Completed);
        }
        history.extend(&index).await?;
        // Main-account reads replace the prior main count; splitter reads are accumulated above.
        log.scanned_transactions = history
            .transactions()
            .len()
            .checked_add(splitter_transactions_scanned)
            .ok_or_else(|| "history_count_overflow".to_string())?;
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ContributionBatch {
    commitment_tx_id: u64,
    sources: BTreeMap<[u8; 32], u64>,
    splitter_credits: Vec<SplitterFundingCredit>,
    ineligible_e8s: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct FundingCredit {
    tx_id: u64,
    origin: FundingOrigin,
    amount_e8s: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum FundingOrigin {
    External([u8; 32]),
    Splitter(u8),
    MintOrInvalid,
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

fn merge_attribution(
    mut direct_sources: BTreeMap<[u8; 32], u64>,
    mut ineligible_e8s: u64,
    expanded: &reward_splitter::ExpandedSplitterProvenance,
) -> Result<(BTreeMap<[u8; 32], u64>, u64), String> {
    for (source, amount) in &expanded.sources {
        add_checked(direct_sources.entry(*source).or_default(), *amount)?;
    }
    add_checked(&mut ineligible_e8s, expanded.ineligible_e8s)?;
    Ok((direct_sources, ineligible_e8s))
}

fn sum_eligible_weight(eligible: &BTreeMap<Principal, u64>) -> Result<u64, String> {
    eligible.values().try_fold(0_u64, |total, weight| {
        total
            .checked_add(*weight)
            .ok_or_else(|| "contribution_overflow".to_string())
    })
}

fn terminal_reward_pot_hold_reason(balance: &Nat, fee: &Nat) -> Option<&'static str> {
    if balance.0 == 0u8.into() {
        return Some("zero_reward_balance");
    }
    if balance.0 <= fee.0 {
        return Some("balance_not_above_plan_fees");
    }
    let single_recipient_amount = balance.0.clone() - fee.0.clone();
    if fee.0.clone() * 10u8 > single_recipient_amount {
        return Some("plan_fees_over_10_percent");
    }
    None
}

fn proportional_reward_allocations(
    balance: &Nat,
    fee: &Nat,
    eligible: &BTreeMap<Principal, u64>,
) -> Result<Vec<(Principal, Nat)>, &'static str> {
    if eligible.is_empty() {
        return Err("no_eligible_owner");
    }
    let total_weight = eligible
        .values()
        .try_fold(0_u128, |total, weight| {
            total.checked_add(u128::from(*weight))
        })
        .ok_or("contribution_overflow")?;
    if total_weight == 0 {
        return Err("no_eligible_owner");
    }
    let mut prefix = 0_u128;
    let mut previous_floor = 0u8.into();
    let mut allocations = Vec::with_capacity(eligible.len());
    for (principal, weight) in eligible {
        prefix = prefix
            .checked_add(u128::from(*weight))
            .ok_or("contribution_overflow")?;
        let floor = candid::Nat::from(prefix).0 * balance.0.clone() / total_weight;
        let gross_entitlement = floor.clone() - previous_floor;
        if gross_entitlement > fee.0 {
            let transfer_amount = gross_entitlement - fee.0.clone();
            if fee.0.clone() * 10u8 <= transfer_amount {
                allocations.push((*principal, Nat(transfer_amount)));
            }
        }
        previous_floor = floor;
    }
    if allocations.is_empty() {
        Err("no_economical_reward_recipient")
    } else {
        Ok(allocations)
    }
}

fn build_pending_payout(
    context: &RelayRewardContext,
    commitment_tx_id: u64,
    fee: Nat,
    allocations: Vec<(Principal, Nat)>,
    now_nanos: u64,
) -> Result<PendingRewardPayout, String> {
    let mut recipients = Vec::with_capacity(allocations.len());
    for (index, (principal, amount)) in allocations.into_iter().enumerate() {
        let memo = reward_memo(commitment_tx_id, index)?;
        recipients.push(PendingRewardRecipient {
            recipient: Account {
                owner: principal,
                subaccount: None,
            },
            observed_balance: None,
            amount: amount.clone(),
            memo,
            created_at_time_nanos: now_nanos,
            attempt_started: false,
            uncertain_attempt_seen: false,
            status: PendingRewardTransferStatus::AwaitingTransfer,
        });
    }
    if recipients.is_empty() {
        return Err("reward_plan_has_no_transferable_recipient".to_string());
    }
    Ok(PendingRewardPayout {
        sns_root_canister_id: context.sns_root_canister_id,
        sns_ledger_canister_id: context.sns_ledger_canister_id,
        snapshot_id: context.snapshot_id,
        attribution_commitment_tx_id: commitment_tx_id,
        fee,
        recipients,
        next_recipient_index: 0,
    })
}

type ClassifiedSources = (BTreeMap<Principal, u64>, u64, usize);

fn classify_sources(
    sources: BTreeMap<[u8; 32], u64>,
    initial_ineligible: u64,
    owners: Vec<Option<Principal>>,
) -> Result<ClassifiedSources, String> {
    if owners.len() != sources.len() {
        return Err("owner_lookup_length_mismatch".to_string());
    }
    let mut eligible = BTreeMap::<Principal, u64>::new();
    let mut ineligible = initial_ineligible;
    let mut owner_mismatch_count = 0;
    for ((account, value), owner) in sources.into_iter().zip(owners) {
        match owner {
            Some(principal) if account_identifier_bytes(principal, None) == account => {
                add_checked(eligible.entry(principal).or_default(), value)?;
            }
            Some(_) => {
                add_checked(&mut ineligible, value)?;
                owner_mismatch_count += 1;
            }
            None => add_checked(&mut ineligible, value)?,
        }
    }
    Ok((eligible, ineligible, owner_mismatch_count))
}

fn reconstruct_batches_with_splitters(
    transactions: &[IndexTransactionWithId],
    history_authoritative: bool,
    cutoff_nanos: u64,
    relay_account_identifier: &str,
    faucet_account_identifier: &str,
    commitment_memo: &[u8],
    intrinsic_splitters: &BTreeMap<[u8; 32], u8>,
) -> reward_history::HistoricalReconstruction<Vec<ContributionBatch>> {
    if !history_authoritative {
        return reward_history::HistoricalReconstruction::NeedOlderHistory;
    }
    let mut chronological = transactions.to_vec();
    chronological.sort_by_key(|transaction| transaction.id);
    let mut funding = VecDeque::<FundingCredit>::new();
    let mut batches = Vec::new();
    for entry in chronological {
        let Some(timestamp) = entry.transaction.timestamp.as_ref() else {
            return reward_history::HistoricalReconstruction::Malformed(
                "history_timestamp_missing".to_string(),
            );
        };
        let timestamp = timestamp.timestamp_nanos;
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
                        return reward_history::HistoricalReconstruction::Malformed(
                            "unexpected_subaccount_debit".to_string(),
                        );
                    }
                    let Some(gross) = amount.e8s().checked_add(fee.e8s()) else {
                        return reward_history::HistoricalReconstruction::Malformed(
                            "commitment_reconciliation_failed".to_string(),
                        );
                    };
                    let mut remaining = gross;
                    let mut sources = BTreeMap::<[u8; 32], u64>::new();
                    let mut splitter_credits = Vec::new();
                    let mut ineligible_e8s = 0_u64;
                    while remaining > 0 {
                        let Some(credit) = funding.pop_front() else {
                            return reward_history::HistoricalReconstruction::Malformed(
                                "commitment_reconciliation_failed".to_string(),
                            );
                        };
                        if credit.amount_e8s > remaining {
                            return reward_history::HistoricalReconstruction::Malformed(
                                "commitment_reconciliation_failed".to_string(),
                            );
                        }
                        remaining -= credit.amount_e8s;
                        match credit.origin {
                            FundingOrigin::External(source) => {
                                if let Err(error) = add_checked(
                                    sources.entry(source).or_default(),
                                    credit.amount_e8s,
                                ) {
                                    return reward_history::HistoricalReconstruction::Malformed(
                                        error,
                                    );
                                }
                            }
                            FundingOrigin::Splitter(splitter_number) => {
                                splitter_credits.push(SplitterFundingCredit {
                                    splitter_number,
                                    tx_id: credit.tx_id,
                                    amount_e8s: credit.amount_e8s,
                                });
                            }
                            FundingOrigin::MintOrInvalid => {
                                if let Err(error) =
                                    add_checked(&mut ineligible_e8s, credit.amount_e8s)
                                {
                                    return reward_history::HistoricalReconstruction::Malformed(
                                        error,
                                    );
                                }
                            }
                        }
                    }
                    batches.push(ContributionBatch {
                        commitment_tx_id: entry.id,
                        sources,
                        splitter_credits,
                        ineligible_e8s,
                    });
                } else if to == relay_account_identifier {
                    funding.push_back(FundingCredit {
                        tx_id: entry.id,
                        origin: decode_account_identifier(from).map_or(
                            FundingOrigin::MintOrInvalid,
                            |source| {
                                intrinsic_splitters
                                    .get(&source)
                                    .copied()
                                    .map(FundingOrigin::Splitter)
                                    .unwrap_or(FundingOrigin::External(source))
                            },
                        ),
                        amount_e8s: amount.e8s(),
                    });
                }
            }
            IndexOperation::TransferFrom {
                to, from, amount, ..
            } => {
                if from == relay_account_identifier {
                    return reward_history::HistoricalReconstruction::Malformed(
                        "unexpected_subaccount_debit".to_string(),
                    );
                } else if to == relay_account_identifier {
                    funding.push_back(FundingCredit {
                        tx_id: entry.id,
                        origin: decode_account_identifier(from).map_or(
                            FundingOrigin::MintOrInvalid,
                            |source| {
                                intrinsic_splitters
                                    .get(&source)
                                    .copied()
                                    .map(FundingOrigin::Splitter)
                                    .unwrap_or(FundingOrigin::External(source))
                            },
                        ),
                        amount_e8s: amount.e8s(),
                    });
                }
            }
            IndexOperation::Mint { to, amount } if to == relay_account_identifier => {
                funding.push_back(FundingCredit {
                    tx_id: entry.id,
                    origin: FundingOrigin::MintOrInvalid,
                    amount_e8s: amount.e8s(),
                });
            }
            IndexOperation::Burn { from, .. } if from == relay_account_identifier => {
                return reward_history::HistoricalReconstruction::Malformed(
                    "unexpected_subaccount_debit".to_string(),
                );
            }
            IndexOperation::Approve { from, fee, .. }
                if from == relay_account_identifier && fee.e8s() > 0 =>
            {
                return reward_history::HistoricalReconstruction::Malformed(
                    "unexpected_subaccount_debit".to_string(),
                );
            }
            _ => {}
        }
    }
    reward_history::HistoricalReconstruction::Complete(batches)
}

fn reward_memo(commitment_tx_id: u64, recipient_index: usize) -> Result<Vec<u8>, String> {
    let mut memo = REWARD_MEMO_PREFIX.to_vec();
    memo.extend_from_slice(&commitment_tx_id.to_be_bytes());
    memo.extend_from_slice(
        &u32::try_from(recipient_index)
            .map_err(|_| "reward_recipient_index_overflow".to_string())?
            .to_be_bytes(),
    );
    Ok(memo)
}

enum AttemptOutcome {
    Accepted,
    RetryableExplicit,
    DefinitiveRejected(&'static str),
    Uncertain,
}

#[async_trait]
trait RewardLedgerClient: Send + Sync {
    async fn balance_of(&self, account: Account) -> Result<Nat, ()>;
    async fn fee(&self) -> Result<Nat, ()>;
    async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, ()>;
}

#[async_trait]
impl RewardLedgerClient for IcrcLedgerCanister {
    async fn balance_of(&self, account: Account) -> Result<Nat, ()> {
        self.balance_of(account).await.map_err(|_| ())
    }

    async fn fee(&self) -> Result<Nat, ()> {
        self.fee().await.map_err(|_| ())
    }

    async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, ()> {
        self.transfer(arg).await.map_err(|_| ())
    }
}

fn reward_transfer_arg(recipient: &PendingRewardRecipient, fee: &Nat) -> TransferArg {
    TransferArg {
        from_subaccount: None,
        to: recipient.recipient,
        fee: Some(fee.clone()),
        created_at_time: Some(recipient.created_at_time_nanos),
        memo: Some(Memo::from(recipient.memo.clone())),
        amount: recipient.amount.clone(),
    }
}

async fn transfer_once<L: RewardLedgerClient>(
    ledger: &L,
    recipient: &PendingRewardRecipient,
    fee: &Nat,
) -> AttemptOutcome {
    match ledger.transfer(reward_transfer_arg(recipient, fee)).await {
        Ok(Ok(_)) | Ok(Err(TransferError::Duplicate { .. })) => AttemptOutcome::Accepted,
        Ok(Err(TransferError::BadFee { .. })) => AttemptOutcome::DefinitiveRejected("bad_fee"),
        Ok(Err(
            TransferError::TemporarilyUnavailable
            | TransferError::CreatedInFuture { .. }
            | TransferError::GenericError { .. },
        )) => AttemptOutcome::RetryableExplicit,
        Ok(Err(
            TransferError::BadBurn { .. }
            | TransferError::InsufficientFunds { .. }
            | TransferError::TooOld,
        )) => AttemptOutcome::DefinitiveRejected("definitive_rejection"),
        Err(_) => AttemptOutcome::Uncertain,
    }
}

fn accept_current_recipient() -> bool {
    reward_state::mutate(|state| {
        let Some(payout) = state.pending_payout.as_mut() else {
            return true;
        };
        let next = usize::try_from(payout.next_recipient_index)
            .expect("validated reward recipient index")
            + 1;
        if next == payout.recipients.len() {
            state.pending_payout = None;
            true
        } else {
            payout.next_recipient_index = u32::try_from(next).expect("bounded reward recipients");
            false
        }
    })
}

fn clear_rejected() {
    reward_state::mutate(|state| state.pending_payout = None);
}

fn mark_ambiguous() {
    reward_state::mutate(|state| {
        if let Some(payout) = state.pending_payout.as_mut() {
            let index = usize::try_from(payout.next_recipient_index)
                .expect("validated reward recipient index");
            let recipient = &mut payout.recipients[index];
            recipient.uncertain_attempt_seen = true;
            recipient.status = PendingRewardTransferStatus::Ambiguous;
        }
    });
}

fn mark_retryable_explicit() {
    reward_state::mutate(|state| {
        if let Some(payout) = state.pending_payout.as_mut() {
            let index = usize::try_from(payout.next_recipient_index)
                .expect("validated reward recipient index");
            let recipient = &mut payout.recipients[index];
            recipient.observed_balance = None;
            recipient.attempt_started = false;
            recipient.status = PendingRewardTransferStatus::AwaitingTransfer;
        }
    });
}

fn reject_current(reason: &'static str) -> PendingDriveResult {
    let completed_recipients = reward_state::get()
        .pending_payout
        .as_ref()
        .map_or(0, |payout| payout.next_recipient_index);
    if completed_recipients == 0 {
        clear_rejected();
        PendingDriveResult::Rejected(reason)
    } else {
        reward_state::mutate(|state| {
            let payout = state
                .pending_payout
                .as_mut()
                .expect("pending reward payout");
            let index = usize::try_from(payout.next_recipient_index)
                .expect("validated reward recipient index");
            let recipient = &mut payout.recipients[index];
            recipient.observed_balance = None;
            recipient.attempt_started = false;
            recipient.status = PendingRewardTransferStatus::NeedsFreshIdentity;
        });
        PendingDriveResult::Repricing
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingDriveResult {
    Accepted,
    Rejected(&'static str),
    Ambiguous,
    Repricing,
    WaitingForBalance,
    Held,
}

impl PendingDriveResult {
    fn status(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected(_) => "failed",
            Self::Ambiguous => "ambiguous",
            Self::Repricing | Self::WaitingForBalance | Self::Held => "held",
        }
    }

    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Rejected(reason) => Some(reason),
            Self::Ambiguous => Some("reward_payout_ambiguous"),
            Self::Repricing => Some("reward_payout_repricing"),
            Self::WaitingForBalance => Some("reward_payout_waiting_for_balance"),
            Self::Held => Some("reward_payout_pending"),
            Self::Accepted => None,
        }
    }

    fn sweep_disposition(self) -> SweepDisposition {
        match self {
            Self::Accepted
            | Self::Ambiguous
            | Self::Repricing
            | Self::WaitingForBalance
            | Self::Held => SweepDisposition::Completed,
            Self::Rejected(_) => SweepDisposition::RetryNextDailyTick,
        }
    }
}

async fn reprice_unpaid_remainder<L: RewardLedgerClient>(
    now_nanos: u64,
    relay_account_owner: Principal,
    ledger: &L,
) -> PendingDriveResult {
    let Some(payout) = reward_state::get().pending_payout else {
        return PendingDriveResult::Accepted;
    };
    let fee = match ledger.fee().await {
        Ok(fee) => fee,
        Err(()) => return PendingDriveResult::Repricing,
    };
    let balance = match ledger
        .balance_of(Account {
            owner: relay_account_owner,
            subaccount: None,
        })
        .await
    {
        Ok(balance) => balance,
        Err(()) => return PendingDriveResult::Repricing,
    };
    let start =
        usize::try_from(payout.next_recipient_index).expect("validated reward recipient index");
    let remaining_amounts = payout.recipients[start..]
        .iter()
        .fold(Nat::from(0u8).0, |total, recipient| {
            total + recipient.amount.0.clone()
        });
    let remaining_fees = fee.0.clone() * (payout.recipients.len() - start);
    let enough_for_promises = balance.0 >= remaining_amounts.clone() + remaining_fees.clone();
    // A fresh plan still uses the strict fee-to-distributable guard. Once some recipients have
    // been paid, their fixed entitlements cannot be rebuilt. Repricing therefore waits until the
    // live balance supplies both the promised remainder and fee headroom, and until the remaining
    // fees are at most ten percent of the currently spendable post-fee balance. Extra accrual is
    // headroom only; it is not redistributed into this already-fixed payout.
    let economical_with_current_balance = balance.0 > remaining_fees
        && remaining_fees.clone() * 10u8 <= balance.0.clone() - remaining_fees.clone();
    if !enough_for_promises || !economical_with_current_balance {
        reward_state::mutate(|state| {
            if let Some(payout) = state.pending_payout.as_mut() {
                let index = usize::try_from(payout.next_recipient_index)
                    .expect("validated reward recipient index");
                payout.recipients[index].status = PendingRewardTransferStatus::WaitingForBalance;
            }
        });
        return PendingDriveResult::WaitingForBalance;
    }

    reward_state::mutate(|state| {
        let payout = state
            .pending_payout
            .as_mut()
            .expect("pending reward payout");
        payout.fee = fee;
        let start =
            usize::try_from(payout.next_recipient_index).expect("validated reward recipient index");
        for (offset, recipient) in payout.recipients[start..].iter_mut().enumerate() {
            let offset = u64::try_from(offset).expect("bounded reward recipients");
            recipient.created_at_time_nanos = now_nanos
                .saturating_add(offset)
                .max(recipient.created_at_time_nanos.saturating_add(1));
            recipient.observed_balance = None;
            recipient.attempt_started = false;
            recipient.uncertain_attempt_seen = false;
            recipient.status = PendingRewardTransferStatus::AwaitingTransfer;
        }
    });
    PendingDriveResult::Held
}

async fn drive_pending(now_nanos: u64) -> PendingDriveResult {
    let Some(payout) = reward_state::get().pending_payout else {
        return PendingDriveResult::Held;
    };
    let ledger = IcrcLedgerCanister::new(payout.sns_ledger_canister_id);
    drive_pending_with_ledger(now_nanos, ic_cdk::api::canister_self(), &ledger).await
}

async fn drive_pending_with_ledger<L: RewardLedgerClient>(
    now_nanos: u64,
    relay_account_owner: Principal,
    ledger: &L,
) -> PendingDriveResult {
    loop {
        let Some(payout) = reward_state::get().pending_payout else {
            return PendingDriveResult::Accepted;
        };
        let index =
            usize::try_from(payout.next_recipient_index).expect("validated reward recipient index");
        let recipient = payout.recipients[index].clone();
        if matches!(
            recipient.status,
            PendingRewardTransferStatus::NeedsFreshIdentity
                | PendingRewardTransferStatus::WaitingForBalance
        ) {
            match reprice_unpaid_remainder(now_nanos, relay_account_owner, ledger).await {
                PendingDriveResult::Held => continue,
                result => return result,
            }
        }

        if recipient.status == PendingRewardTransferStatus::Ambiguous || recipient.attempt_started {
            if recipient.status != PendingRewardTransferStatus::Ambiguous {
                mark_ambiguous();
            }
            let relay_account = Account {
                owner: relay_account_owner,
                subaccount: None,
            };
            if let (Some(observed), Ok(current)) = (
                recipient.observed_balance.as_ref(),
                ledger.balance_of(relay_account).await,
            ) {
                if current.0 < observed.0 {
                    if accept_current_recipient() {
                        return PendingDriveResult::Accepted;
                    }
                    continue;
                }
            }
            if !created_at_time_is_valid(recipient.created_at_time_nanos, now_nanos) {
                return PendingDriveResult::Ambiguous;
            }
            match transfer_once(ledger, &recipient, &payout.fee).await {
                AttemptOutcome::Accepted => {
                    if accept_current_recipient() {
                        return PendingDriveResult::Accepted;
                    }
                    continue;
                }
                _ => return PendingDriveResult::Ambiguous,
            }
        }

        let observed_balance = match ledger
            .balance_of(Account {
                owner: relay_account_owner,
                subaccount: None,
            })
            .await
        {
            Ok(balance) => balance,
            Err(()) => return PendingDriveResult::Held,
        };
        reward_state::mutate(|state| {
            if let Some(payout) = state.pending_payout.as_mut() {
                let index = usize::try_from(payout.next_recipient_index)
                    .expect("validated reward recipient index");
                let recipient = &mut payout.recipients[index];
                recipient.observed_balance = Some(observed_balance);
                recipient.attempt_started = true;
            }
        });
        let first = transfer_once(ledger, &recipient, &payout.fee).await;
        match first {
            AttemptOutcome::Accepted => {
                if accept_current_recipient() {
                    return PendingDriveResult::Accepted;
                }
            }
            AttemptOutcome::DefinitiveRejected(reason) => return reject_current(reason),
            AttemptOutcome::RetryableExplicit => {
                mark_retryable_explicit();
                return PendingDriveResult::Held;
            }
            AttemptOutcome::Uncertain => {
                mark_ambiguous();
                return PendingDriveResult::Ambiguous;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupiter_ic_clients::index::{
        GetAccountIdentifierTransactionsResponse, IndexTimeStamp, IndexTransaction, Tokens,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn tx(id: u64, timestamp: u64, operation: IndexOperation) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation,
                created_at_time: None,
                timestamp: Some(IndexTimeStamp {
                    timestamp_nanos: timestamp,
                }),
            },
        }
    }

    fn transfer(
        id: u64,
        from: String,
        to: String,
        amount: u64,
        fee: u64,
        memo: Option<Vec<u8>>,
    ) -> IndexTransactionWithId {
        let mut transaction = tx(
            id,
            id,
            IndexOperation::Transfer {
                from,
                to,
                amount: Tokens::new(amount),
                fee: Tokens::new(fee),
                spender: None,
            },
        );
        transaction.transaction.icrc1_memo = memo;
        transaction
    }

    fn transfer_from(id: u64, from: String, to: String, amount: u64) -> IndexTransactionWithId {
        tx(
            id,
            id,
            IndexOperation::TransferFrom {
                from,
                to,
                spender: "spender".to_string(),
                amount: Tokens::new(amount),
                fee: Tokens::new(10_000),
            },
        )
    }

    fn reconstruct(history: &[IndexTransactionWithId]) -> Vec<ContributionBatch> {
        reconstruct_batches_with_splitters(
            history,
            true,
            100,
            "relay",
            "faucet",
            b"commit",
            &BTreeMap::new(),
        )
        .unwrap_complete()
    }

    #[test]
    fn exact_commitments_exclude_post_staging_credit_and_consume_all_whole_credits() {
        const E8S: u64 = 100_000_000;
        let alice = hex::encode([1; 32]);
        let bob = hex::encode([2; 32]);
        let batches = reconstruct(&[
            transfer(1, alice, "relay".to_string(), 150 * E8S / 100, 0, None),
            transfer(2, bob, "relay".to_string(), 3 * E8S, 0, None),
            transfer(
                3,
                "relay".to_string(),
                "faucet".to_string(),
                140 * E8S / 100,
                10 * E8S / 100,
                Some(b"commit".to_vec()),
            ),
            transfer(
                4,
                "relay".to_string(),
                "faucet".to_string(),
                290 * E8S / 100,
                10 * E8S / 100,
                Some(b"commit".to_vec()),
            ),
        ]);
        assert_eq!(batches.len(), 2);
        assert_eq!(
            batches[0].sources,
            BTreeMap::from([([1; 32], 150 * E8S / 100)])
        );
        assert_eq!(batches[1].sources, BTreeMap::from([([2; 32], 3 * E8S)]));

        let combined = reconstruct(&[
            transfer(
                1,
                hex::encode([1; 32]),
                "relay".to_string(),
                4 * E8S,
                0,
                None,
            ),
            transfer(
                2,
                hex::encode([2; 32]),
                "relay".to_string(),
                6 * E8S,
                0,
                None,
            ),
            transfer(
                3,
                "relay".to_string(),
                "faucet".to_string(),
                990 * E8S / 100,
                10 * E8S / 100,
                Some(b"commit".to_vec()),
            ),
        ]);
        assert_eq!(combined[0].sources.len(), 2);
        assert_eq!(combined[0].sources.values().sum::<u64>(), 10 * E8S);
    }

    #[test]
    fn incremental_replay_extends_past_post_pin_credit_and_recovers_carried_fifo() {
        const E8S: u64 = 100_000_000;
        let alice = hex::encode([1; 32]);
        let bob = hex::encode([2; 32]);
        let recent_suffix = vec![
            transfer(2, bob.clone(), "relay".to_string(), 3 * E8S, 0, None),
            transfer(
                3,
                "relay".to_string(),
                "faucet".to_string(),
                140 * E8S / 100,
                10 * E8S / 100,
                Some(b"commit".to_vec()),
            ),
        ];
        assert_eq!(
            reconstruct_batches_with_splitters(
                &recent_suffix,
                false,
                100,
                "relay",
                "faucet",
                b"commit",
                &BTreeMap::new(),
            ),
            reward_history::HistoricalReconstruction::NeedOlderHistory
        );

        let complete = reconstruct(&[
            transfer(1, alice, "relay".to_string(), 150 * E8S / 100, 0, None),
            recent_suffix[0].clone(),
            recent_suffix[1].clone(),
            transfer(
                4,
                "relay".to_string(),
                "faucet".to_string(),
                290 * E8S / 100,
                10 * E8S / 100,
                Some(b"commit".to_vec()),
            ),
        ]);
        assert_eq!(
            complete[0].sources,
            BTreeMap::from([([1; 32], 150 * E8S / 100)])
        );
        assert_eq!(complete[1].sources, BTreeMap::from([([2; 32], 3 * E8S)]));
    }

    #[test]
    fn transfer_from_mint_exact_fee_and_trailing_deposits_are_classified_exactly() {
        let source = hex::encode([3; 32]);
        let batches = reconstruct(&[
            transfer_from(1, source, "relay".to_string(), 60_000_000),
            tx(
                2,
                2,
                IndexOperation::Mint {
                    to: "relay".to_string(),
                    amount: Tokens::new(40_010_000),
                },
            ),
            transfer(
                3,
                "relay".to_string(),
                "faucet".to_string(),
                100_000_000,
                10_000,
                Some(b"commit".to_vec()),
            ),
            transfer(
                4,
                hex::encode([4; 32]),
                "relay".to_string(),
                50_000_000,
                0,
                None,
            ),
        ]);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].sources, BTreeMap::from([([3; 32], 60_000_000)]));
        assert_eq!(batches[0].ineligible_e8s, 40_010_000);
    }

    #[test]
    fn whole_credit_under_overshoot_and_commitment_shape_fail_closed() {
        let source = hex::encode([3; 32]);
        for credited in [100_009_999, 100_010_001] {
            let history = vec![
                transfer(1, source.clone(), "relay".to_string(), credited, 0, None),
                transfer(
                    2,
                    "relay".to_string(),
                    "faucet".to_string(),
                    100_000_000,
                    10_000,
                    Some(b"commit".to_vec()),
                ),
            ];
            assert_eq!(
                reconstruct_batches_with_splitters(
                    &history,
                    true,
                    100,
                    "relay",
                    "faucet",
                    b"commit",
                    &BTreeMap::new(),
                )
                .unwrap_malformed(),
                "commitment_reconciliation_failed"
            );
        }

        for (to, memo, amount) in [
            ("wrong", Some(b"commit".to_vec()), 100_000_000),
            ("faucet", Some(b"wrong".to_vec()), 100_000_000),
            ("faucet", Some(b"commit".to_vec()), 99_999_999),
        ] {
            let history = vec![transfer(
                1,
                "relay".to_string(),
                to.to_string(),
                amount,
                10_000,
                memo,
            )];
            assert_eq!(
                reconstruct_batches_with_splitters(
                    &history,
                    true,
                    100,
                    "relay",
                    "faucet",
                    b"commit",
                    &BTreeMap::new(),
                )
                .unwrap_malformed(),
                "unexpected_subaccount_debit"
            );
        }
    }

    #[test]
    fn unsupported_debits_self_transfer_and_arithmetic_overflow_fail_closed() {
        let source = hex::encode([3; 32]);
        let operations = [
            IndexOperation::Burn {
                from: "relay".to_string(),
                amount: Tokens::new(1),
                spender: None,
            },
            IndexOperation::Approve {
                from: "relay".to_string(),
                spender: source.clone(),
                allowance: Tokens::new(1),
                fee: Tokens::new(1),
                expires_at: None,
                expected_allowance: None,
            },
            IndexOperation::Transfer {
                from: "relay".to_string(),
                to: "relay".to_string(),
                amount: Tokens::new(1),
                fee: Tokens::new(1),
                spender: None,
            },
        ];
        for operation in operations {
            assert_eq!(
                reconstruct_batches_with_splitters(
                    &[tx(1, 1, operation)],
                    true,
                    100,
                    "relay",
                    "faucet",
                    b"commit",
                    &BTreeMap::new(),
                )
                .unwrap_malformed(),
                "unexpected_subaccount_debit"
            );
        }
        let overflow = vec![transfer(
            1,
            "relay".to_string(),
            "faucet".to_string(),
            u64::MAX,
            1,
            Some(b"commit".to_vec()),
        )];
        assert_eq!(
            reconstruct_batches_with_splitters(
                &overflow,
                true,
                100,
                "relay",
                "faucet",
                b"commit",
                &BTreeMap::new(),
            )
            .unwrap_malformed(),
            "commitment_reconciliation_failed"
        );
    }

    #[test]
    fn snapshot_cutoff_is_exclusive_and_uncommitted_trailing_deposit_is_not_attributed() {
        let mut deposit = transfer(
            1,
            hex::encode([1; 32]),
            "relay".to_string(),
            100_010_000,
            0,
            None,
        );
        deposit
            .transaction
            .timestamp
            .as_mut()
            .unwrap()
            .timestamp_nanos = 9;
        let mut commitment = transfer(
            2,
            "relay".to_string(),
            "faucet".to_string(),
            100_000_000,
            10_000,
            Some(b"commit".to_vec()),
        );
        commitment
            .transaction
            .timestamp
            .as_mut()
            .unwrap()
            .timestamp_nanos = 10;
        let trailing = transfer(
            3,
            hex::encode([2; 32]),
            "relay".to_string(),
            50_000_000,
            0,
            None,
        );
        assert!(reconstruct_batches_with_splitters(
            &[deposit.clone(), commitment.clone(), trailing],
            true,
            10,
            "relay",
            "faucet",
            b"commit",
            &BTreeMap::new(),
        )
        .unwrap_complete()
        .is_empty());
        assert_eq!(
            reconstruct_batches_with_splitters(
                &[deposit, commitment],
                true,
                11,
                "relay",
                "faucet",
                b"commit",
                &BTreeMap::new(),
            )
            .unwrap_complete()
            .len(),
            1
        );
    }

    #[test]
    fn reward_arrival_cutoff_excludes_later_commitment_and_allows_earlier_one() {
        let mut alice_deposit = transfer(
            1,
            hex::encode([1; 32]),
            "relay".to_string(),
            100_010_000,
            0,
            None,
        );
        let mut alice_commitment = transfer(
            2,
            "relay".to_string(),
            "faucet".to_string(),
            100_000_000,
            10_000,
            Some(b"commit".to_vec()),
        );
        let mut bob_deposit = transfer(
            3,
            hex::encode([2; 32]),
            "relay".to_string(),
            100_010_000,
            0,
            None,
        );
        let mut bob_commitment = transfer(
            4,
            "relay".to_string(),
            "faucet".to_string(),
            100_000_000,
            10_000,
            Some(b"commit".to_vec()),
        );
        for (entry, timestamp) in [
            (&mut alice_deposit, 5),
            (&mut alice_commitment, 10),
            (&mut bob_deposit, 15),
            (&mut bob_commitment, 20),
        ] {
            entry
                .transaction
                .timestamp
                .as_mut()
                .unwrap()
                .timestamp_nanos = timestamp;
        }
        let history = [alice_deposit, alice_commitment, bob_deposit, bob_commitment];

        let before_reward_arrival = reconstruct_batches_with_splitters(
            &history,
            true,
            15,
            "relay",
            "faucet",
            b"commit",
            &BTreeMap::new(),
        )
        .unwrap_complete();
        assert_eq!(before_reward_arrival.last().unwrap().commitment_tx_id, 2);

        let after_bob_commitment = reconstruct_batches_with_splitters(
            &history,
            true,
            25,
            "relay",
            "faucet",
            b"commit",
            &BTreeMap::new(),
        )
        .unwrap_complete();
        assert_eq!(after_bob_commitment.last().unwrap().commitment_tx_id, 4);
        assert_eq!(
            effective_attribution_cutoff(12, 15),
            12,
            "owner snapshot remains the tighter cutoff"
        );
        assert_eq!(
            effective_attribution_cutoff(25, 15),
            15,
            "oldest reward credit becomes the tighter cutoff"
        );
    }

    #[test]
    fn root_resolved_reward_index_components_are_exactly_pinned() {
        let context = RelayRewardContext {
            sns_root_canister_id: principal(1),
            sns_governance_canister_id: principal(2),
            sns_ledger_canister_id: principal(3),
            snapshot_id: 1,
            scan_started_at_timestamp_nanos: 10,
            scan_completed_at_timestamp_nanos: 11,
        };
        let valid = ListSnsCanistersResponse {
            root: Some(principal(1)),
            ledger: Some(principal(3)),
            index: Some(principal(4)),
            ..Default::default()
        };
        assert_eq!(
            validate_reward_index_components(&context, valid.clone()).unwrap(),
            principal(4)
        );
        for invalid in [
            ListSnsCanistersResponse {
                root: Some(principal(9)),
                ..valid.clone()
            },
            ListSnsCanistersResponse {
                ledger: Some(principal(9)),
                ..valid.clone()
            },
        ] {
            assert_eq!(
                validate_reward_index_components(&context, invalid).unwrap_err(),
                "reward_index_component_mismatch"
            );
        }
        assert_eq!(
            validate_reward_index_components(
                &context,
                ListSnsCanistersResponse {
                    index: None,
                    ..valid
                }
            )
            .unwrap_err(),
            "reward_index_unavailable"
        );
    }

    struct MockHistory {
        pages: Mutex<VecDeque<Result<GetAccountIdentifierTransactionsResponse, String>>>,
        starts: Mutex<Vec<Option<u64>>>,
    }

    #[async_trait]
    impl reward_history::RewardHistoryClient for MockHistory {
        async fn get_transactions(
            &self,
            _account_identifier: String,
            start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, String> {
            self.starts.lock().unwrap().push(start);
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected_history_call".to_string()))
        }
    }

    fn pages_from_history(
        mut chronological: Vec<IndexTransactionWithId>,
    ) -> Vec<GetAccountIdentifierTransactionsResponse> {
        chronological.sort_by_key(|entry| entry.id);
        let oldest = chronological.first().map_or(0, |entry| entry.id);
        chronological.reverse();
        chronological
            .chunks(reward_history::ICP_HISTORY_PAGE_SIZE as usize)
            .map(|chunk| GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: chunk.to_vec(),
                oldest_tx_id: Some(oldest),
            })
            .collect()
    }

    fn scanner_history(total: u64, eligible_oldest: bool) -> Vec<IndexTransactionWithId> {
        let mut history = (1..=total)
            .map(|id| {
                tx(
                    id,
                    id,
                    IndexOperation::Transfer {
                        from: "unrelated-a".to_string(),
                        to: "unrelated-b".to_string(),
                        amount: Tokens::new(1),
                        fee: Tokens::new(0),
                        spender: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        for page in 0..(total / reward_history::ICP_HISTORY_PAGE_SIZE) {
            let deposit_id = page * reward_history::ICP_HISTORY_PAGE_SIZE + 100;
            let commitment_id = deposit_id + 1;
            let source = if page == 0 && eligible_oldest {
                hex::encode([1; 32])
            } else {
                "invalid-source".to_string()
            };
            history[usize::try_from(deposit_id - 1).unwrap()] = transfer(
                deposit_id,
                source,
                "relay".to_string(),
                100_010_000,
                0,
                None,
            );
            history[usize::try_from(commitment_id - 1).unwrap()] = transfer(
                commitment_id,
                "relay".to_string(),
                "faucet".to_string(),
                100_000_000,
                10_000,
                Some(b"commit".to_vec()),
            );
        }
        history
    }

    async fn scan_until_eligible(index: &MockHistory) -> Result<Option<ContributionBatch>, String> {
        let mut history = reward_history::BackwardHistory::new("relay".to_string());
        let mut examined = BTreeSet::new();
        loop {
            if history.transactions().is_empty() {
                history.extend(index).await?;
            }
            let batches = match reconstruct_batches_with_splitters(
                history.authoritative_transactions(),
                history.authoritative(),
                u64::MAX,
                "relay",
                "faucet",
                b"commit",
                &BTreeMap::new(),
            ) {
                reward_history::HistoricalReconstruction::Complete(batches) => batches,
                reward_history::HistoricalReconstruction::NeedOlderHistory => Vec::new(),
                reward_history::HistoricalReconstruction::Malformed(error) => return Err(error),
            };
            for batch in batches.into_iter().rev() {
                if examined.insert(batch.commitment_tx_id) && batch.sources.contains_key(&[1; 32]) {
                    return Ok(Some(batch));
                }
            }
            if history.exhausted() {
                return Ok(None);
            }
            history.extend(index).await?;
        }
    }

    #[test]
    fn balance_proven_suffix_prevents_post_pin_credit_from_rebinding_later_commitment() {
        const PINNED: u64 = 100_010_000;
        let alice = hex::encode([1; 32]);
        let bob = hex::encode([2; 32]);
        let carol = hex::encode([3; 32]);
        let commitment = |id| {
            transfer(
                id,
                "relay".to_string(),
                "faucet".to_string(),
                100_000_000,
                10_000,
                Some(b"commit".to_vec()),
            )
        };
        let recent = vec![
            commitment(5),
            transfer(4, carol, "relay".to_string(), PINNED, 0, None),
            commitment(3),
            transfer(2, bob, "relay".to_string(), PINNED, 0, None),
        ];
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: PINNED,
                    transactions: recent,
                    oldest_tx_id: Some(1),
                }),
                Ok(GetAccountIdentifierTransactionsResponse {
                    balance: PINNED,
                    transactions: vec![transfer(1, alice, "relay".to_string(), PINNED, 0, None)],
                    oldest_tx_id: Some(1),
                }),
            ])),
            starts: Mutex::new(Vec::new()),
        };
        let mut history = reward_history::BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        assert!(!history.authoritative());
        assert_eq!(
            reconstruct_batches_with_splitters(
                history.authoritative_transactions(),
                history.authoritative(),
                u64::MAX,
                "relay",
                "faucet",
                b"commit",
                &BTreeMap::new(),
            ),
            reward_history::HistoricalReconstruction::NeedOlderHistory
        );

        block_on(history.extend(&index)).unwrap();
        assert!(history.authoritative());
        let batches = reconstruct_batches_with_splitters(
            history.authoritative_transactions(),
            history.authoritative(),
            u64::MAX,
            "relay",
            "faucet",
            b"commit",
            &BTreeMap::new(),
        )
        .unwrap_complete();
        assert_eq!(batches[0].sources, BTreeMap::from([([1; 32], PINNED)]));
        assert_eq!(batches[1].sources, BTreeMap::from([([2; 32], PINNED)]));
    }

    #[test]
    fn icp_index_tip_must_cover_the_ledger_tip_before_attribution() {
        assert_eq!(
            validate_icp_index_tip(42, 41),
            Err("icp_index_not_caught_up".to_string())
        );
        assert_eq!(validate_icp_index_tip(42, 42), Ok(()));
        assert_eq!(validate_icp_index_tip(42, 43), Ok(()));
    }

    #[test]
    fn hidden_net_zero_newer_commitment_cannot_pay_older_funder_before_index_catches_up() {
        const PINNED: u64 = 100_010_000;
        let all_history = vec![
            transfer(
                4,
                "relay".to_string(),
                "faucet".to_string(),
                100_000_000,
                10_000,
                Some(b"commit".to_vec()),
            ),
            transfer(
                3,
                hex::encode([2; 32]),
                "relay".to_string(),
                PINNED,
                0,
                None,
            ),
            transfer(
                2,
                "relay".to_string(),
                "faucet".to_string(),
                100_000_000,
                10_000,
                Some(b"commit".to_vec()),
            ),
            transfer(
                1,
                hex::encode([1; 32]),
                "relay".to_string(),
                PINNED,
                0,
                None,
            ),
        ];
        let index = MockHistory {
            pages: Mutex::new(VecDeque::from([Ok(
                GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: all_history,
                    oldest_tx_id: Some(1),
                },
            )])),
            starts: Mutex::new(Vec::new()),
        };

        assert_eq!(
            validate_icp_index_tip(4, 2),
            Err("icp_index_not_caught_up".to_string())
        );
        assert!(index.starts.lock().unwrap().is_empty());

        validate_icp_index_tip(4, 4).unwrap();
        let mut history = reward_history::BackwardHistory::new("relay".to_string());
        block_on(history.extend(&index)).unwrap();
        let batches = reconstruct_batches_with_splitters(
            history.authoritative_transactions(),
            history.authoritative(),
            u64::MAX,
            "relay",
            "faucet",
            b"commit",
            &BTreeMap::new(),
        )
        .unwrap_complete();
        assert_eq!(
            batches.last().unwrap().sources,
            BTreeMap::from([([2; 32], PINNED)])
        );
    }

    #[test]
    fn recent_commitment_does_not_read_huge_older_history() {
        let mut history = scanner_history(12_000, false);
        history[11_898] = transfer(
            11_899,
            hex::encode([1; 32]),
            "relay".to_string(),
            100_010_000,
            0,
            None,
        );
        history[11_899] = transfer(
            11_900,
            "relay".to_string(),
            "faucet".to_string(),
            100_000_000,
            10_000,
            Some(b"commit".to_vec()),
        );
        let index = MockHistory {
            pages: Mutex::new(pages_from_history(history).into_iter().map(Ok).collect()),
            starts: Mutex::new(Vec::new()),
        };
        let selected = block_on(scan_until_eligible(&index)).unwrap().unwrap();
        assert_eq!(selected.commitment_tx_id, 11_900);
        assert_eq!(index.starts.lock().unwrap().len(), 1);
    }

    #[test]
    fn eligible_commitment_beyond_former_limit_is_found_without_depth_veto() {
        let index = MockHistory {
            pages: Mutex::new(
                pages_from_history(scanner_history(12_000, true))
                    .into_iter()
                    .map(Ok)
                    .collect(),
            ),
            starts: Mutex::new(Vec::new()),
        };
        let selected = block_on(scan_until_eligible(&index)).unwrap().unwrap();
        assert_eq!(selected.commitment_tx_id, 101);
        assert_eq!(index.starts.lock().unwrap().len(), 12);
    }

    #[test]
    fn genuine_history_exhaustion_returns_no_eligible_commitment() {
        let index = MockHistory {
            pages: Mutex::new(
                pages_from_history(scanner_history(12_000, false))
                    .into_iter()
                    .map(Ok)
                    .collect(),
            ),
            starts: Mutex::new(Vec::new()),
        };
        assert!(block_on(scan_until_eligible(&index)).unwrap().is_none());
        assert_eq!(index.starts.lock().unwrap().len(), 12);
    }

    #[test]
    fn pro_rata_plan_is_deterministic_conserved_and_ignores_ineligible_weight() {
        let alice = principal(1);
        let bob = principal(2);
        let eligible = BTreeMap::from([(alice, 4), (bob, 6)]);
        let allocations =
            proportional_reward_allocations(&Nat::from(1_020_u64), &Nat::from(10_u64), &eligible)
                .unwrap();
        assert_eq!(
            allocations,
            vec![(alice, Nat::from(398_u64)), (bob, Nat::from(602_u64))]
        );
        let transfer_total = allocations
            .iter()
            .fold(Nat::from(0_u8), |sum, (_, amount)| {
                Nat(sum.0 + amount.0.clone())
            });
        assert_eq!(transfer_total, Nat::from(1_000_u64));
        assert_eq!(
            transfer_total.0 + Nat::from(20_u64).0,
            Nat::from(1_020_u64).0
        );

        let sources = BTreeMap::from([
            (account_identifier_bytes(alice, None), 4),
            (account_identifier_bytes(bob, None), 6),
            ([9; 32], 90),
            ([10; 32], 10),
        ]);
        let owners = sources
            .keys()
            .map(|account| {
                if *account == account_identifier_bytes(alice, None) {
                    Some(alice)
                } else if *account == account_identifier_bytes(bob, None) {
                    Some(bob)
                } else if *account == [10; 32] {
                    Some(principal(10))
                } else {
                    None
                }
            })
            .collect();
        let (classified, ineligible, mismatches) = classify_sources(sources, 0, owners).unwrap();
        assert_eq!(classified, eligible);
        assert_eq!(ineligible, 100);
        assert_eq!(mismatches, 1);
    }

    #[test]
    fn gross_entitlements_charge_only_actual_transfer_fees_and_retain_dust() {
        let tiny = principal(1);
        let large = principal(2);
        let allocations = proportional_reward_allocations(
            &Nat::from(1_002_u64),
            &Nat::from(1_u64),
            &BTreeMap::from([(tiny, 1), (large, 10_000)]),
        )
        .unwrap();
        assert_eq!(allocations, vec![(large, Nat::from(1_001_u64))]);
        assert_eq!(
            proportional_reward_allocations(
                &Nat::from(111_u64),
                &Nat::from(11_u64),
                &BTreeMap::from([(large, 1)]),
            ),
            Err("no_economical_reward_recipient")
        );

        let allocations = proportional_reward_allocations(
            &Nat::from(12_100_u64),
            &Nat::from(1_000_u64),
            &BTreeMap::from([(tiny, 1_000), (large, 10_000)]),
        )
        .unwrap();
        assert_eq!(allocations, vec![(large, Nat::from(10_000_u64))]);
        // The tiny owner's gross 1,100-unit entitlement remains in Relay. It is neither charged
        // a fee nor redistributed to the large owner.
        assert_eq!(12_100_u64 - 10_000_u64 - 1_000_u64, 1_100_u64);
    }

    #[test]
    fn one_ten_and_hundreds_of_dust_contributors_cannot_veto_large_owner() {
        for dust_count in [1_u8, 10, 200] {
            let large = principal(254);
            let mut eligible = (1..=dust_count)
                .map(|id| (principal(id), 1_u64))
                .collect::<BTreeMap<_, _>>();
            // Even all 200 dust weights together round to a zero gross entitlement, so none of
            // them can reserve a phantom fee or reduce the large owner's exact 11,000-unit gross.
            eligible.insert(large, 10_000_000);
            assert_eq!(
                proportional_reward_allocations(
                    &Nat::from(11_000_u64),
                    &Nat::from(1_000_u64),
                    &eligible,
                )
                .unwrap(),
                vec![(large, Nat::from(10_000_u64))],
                "dust_count={dust_count}"
            );
        }
    }

    #[test]
    fn universally_uneconomical_reward_pots_are_terminal_before_attribution() {
        assert_eq!(
            terminal_reward_pot_hold_reason(&Nat::from(0_u8), &Nat::from(10_u8)),
            Some("zero_reward_balance")
        );
        assert_eq!(
            terminal_reward_pot_hold_reason(&Nat::from(10_u8), &Nat::from(10_u8)),
            Some("balance_not_above_plan_fees")
        );
        assert_eq!(
            terminal_reward_pot_hold_reason(&Nat::from(110_u8), &Nat::from(11_u8)),
            Some("plan_fees_over_10_percent")
        );
        assert_eq!(
            terminal_reward_pot_hold_reason(&Nat::from(111_u8), &Nat::from(10_u8)),
            None
        );
    }

    struct MockResolver {
        responses: Mutex<VecDeque<Result<ResolveDefaultIcpAccountsResult, String>>>,
        requests: Mutex<Vec<(u64, Vec<Vec<u8>>)>>,
    }

    #[async_trait]
    impl OwnerResolverClient for MockResolver {
        async fn resolve(
            &self,
            snapshot_id: u64,
            accounts: Vec<Vec<u8>>,
        ) -> Result<ResolveDefaultIcpAccountsResult, String> {
            self.requests.lock().unwrap().push((snapshot_id, accounts));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected_owner_lookup".to_string()))
        }
    }

    #[test]
    fn owner_resolution_chunks_large_candidates_with_one_snapshot_and_order() {
        let sources = (0..300_u16)
            .map(|value| {
                let mut account = [0_u8; 32];
                account[..2].copy_from_slice(&value.to_be_bytes());
                (account, 1)
            })
            .collect::<BTreeMap<_, _>>();
        let resolver = MockResolver {
            responses: Mutex::new(VecDeque::from([
                Ok(ResolveDefaultIcpAccountsResult::Ok(vec![
                    Some(principal(1));
                    128
                ])),
                Ok(ResolveDefaultIcpAccountsResult::Ok(vec![
                    Some(principal(2));
                    128
                ])),
                Ok(ResolveDefaultIcpAccountsResult::Ok(vec![
                    Some(principal(3));
                    44
                ])),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        let owners = block_on(resolve_accounts_chunked(&resolver, 77, &sources)).unwrap();
        assert_eq!(owners.len(), 300);
        assert_eq!(owners[0], Some(principal(1)));
        assert_eq!(owners[128], Some(principal(2)));
        assert_eq!(owners[256], Some(principal(3)));
        let requests = resolver.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .map(|(_, values)| values.len())
                .collect::<Vec<_>>(),
            [128, 128, 44]
        );
        assert!(requests.iter().all(|(snapshot, _)| *snapshot == 77));
    }

    #[test]
    fn snapshot_change_in_any_owner_chunk_fails_the_whole_candidate() {
        let sources = (0..129_u16)
            .map(|value| {
                let mut account = [0_u8; 32];
                account[..2].copy_from_slice(&value.to_be_bytes());
                (account, 1)
            })
            .collect::<BTreeMap<_, _>>();
        let resolver = MockResolver {
            responses: Mutex::new(VecDeque::from([
                Ok(ResolveDefaultIcpAccountsResult::Ok(vec![None; 128])),
                Ok(ResolveDefaultIcpAccountsResult::SnapshotChanged),
            ])),
            requests: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(resolve_accounts_chunked(&resolver, 77, &sources)).unwrap_err(),
            "reward_context_changed"
        );
    }

    struct MockLedger {
        balances: Mutex<VecDeque<Result<Nat, ()>>>,
        fees: Mutex<VecDeque<Result<Nat, ()>>>,
        transfers: Mutex<VecDeque<Result<Result<BlockIndex, TransferError>, ()>>>,
        args: Mutex<Vec<TransferArg>>,
    }

    #[async_trait]
    impl RewardLedgerClient for MockLedger {
        async fn balance_of(&self, _account: Account) -> Result<Nat, ()> {
            self.balances.lock().unwrap().pop_front().unwrap_or(Err(()))
        }

        async fn fee(&self) -> Result<Nat, ()> {
            self.fees.lock().unwrap().pop_front().unwrap_or(Err(()))
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, ()> {
            self.args.lock().unwrap().push(arg);
            self.transfers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(()))
        }
    }

    fn payout() -> PendingRewardPayout {
        PendingRewardPayout {
            sns_root_canister_id: principal(7),
            sns_ledger_canister_id: principal(8),
            snapshot_id: 9,
            attribution_commitment_tx_id: 10,
            fee: Nat::from(1_u64),
            recipients: (0..3)
                .map(|index| PendingRewardRecipient {
                    recipient: Account {
                        owner: principal(index + 1),
                        subaccount: None,
                    },
                    observed_balance: None,
                    amount: Nat::from(19_u64),
                    memo: reward_memo(10, usize::from(index)).unwrap(),
                    created_at_time_nanos: 55,
                    attempt_started: false,
                    uncertain_attempt_seen: false,
                    status: PendingRewardTransferStatus::AwaitingTransfer,
                })
                .collect(),
            next_recipient_index: 0,
        }
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn multi_recipient_progress_survives_ambiguity_and_duplicate_without_double_pay() {
        reward_state::reset_for_test();
        reward_state::mutate(|state| state.pending_payout = Some(payout()));
        let first = MockLedger {
            balances: Mutex::new(VecDeque::from([
                Ok(Nat::from(100_u64)),
                Ok(Nat::from(80_u64)),
            ])),
            fees: Mutex::new(VecDeque::new()),
            transfers: Mutex::new(VecDeque::from([Ok(Ok(Nat::from(1_u64))), Err(())])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(56, principal(99), &first)),
            PendingDriveResult::Ambiguous
        );
        let durable = reward_state::get().pending_payout.unwrap();
        assert_eq!(durable.next_recipient_index, 1);
        assert_eq!(
            durable.recipients[1].status,
            PendingRewardTransferStatus::Ambiguous
        );

        let second = MockLedger {
            balances: Mutex::new(VecDeque::from([
                Ok(Nat::from(100_u64)),
                Ok(Nat::from(60_u64)),
            ])),
            fees: Mutex::new(VecDeque::new()),
            transfers: Mutex::new(VecDeque::from([
                Ok(Err(TransferError::Duplicate {
                    duplicate_of: Nat::from(2_u64),
                })),
                Ok(Ok(Nat::from(3_u64))),
            ])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(57, principal(99), &second)),
            PendingDriveResult::Accepted
        );
        assert!(reward_state::get().pending_payout.is_none());
        assert_eq!(first.args.lock().unwrap().len(), 2);
        assert_eq!(second.args.lock().unwrap().len(), 2);
    }

    #[test]
    fn successful_multi_recipient_payout_pins_actual_balances_and_pays_once() {
        reward_state::reset_for_test();
        reward_state::mutate(|state| state.pending_payout = Some(payout()));
        let ledger = MockLedger {
            balances: Mutex::new(VecDeque::from([
                Ok(Nat::from(100_u64)),
                Ok(Nat::from(80_u64)),
                Ok(Nat::from(60_u64)),
            ])),
            fees: Mutex::new(VecDeque::new()),
            transfers: Mutex::new(VecDeque::from([
                Ok(Ok(Nat::from(1_u64))),
                Ok(Ok(Nat::from(2_u64))),
                Ok(Ok(Nat::from(3_u64))),
            ])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(56, principal(99), &ledger)),
            PendingDriveResult::Accepted
        );
        assert!(reward_state::get().pending_payout.is_none());
        let args = ledger.args.lock().unwrap();
        assert_eq!(args.len(), 3);
        assert_eq!(
            args.iter().map(|arg| arg.to.owner).collect::<Vec<_>>(),
            [principal(1), principal(2), principal(3),]
        );
        assert!(args.iter().all(|arg| arg.amount == Nat::from(19_u64)));
    }

    #[test]
    fn bad_fee_before_first_recipient_clears_fresh_plan_without_uncertainty() {
        reward_state::reset_for_test();
        reward_state::mutate(|state| state.pending_payout = Some(payout()));
        let ledger = MockLedger {
            balances: Mutex::new(VecDeque::from([Ok(Nat::from(100_u64))])),
            fees: Mutex::new(VecDeque::new()),
            transfers: Mutex::new(VecDeque::from([Ok(Err(TransferError::BadFee {
                expected_fee: Nat::from(2_u64),
            }))])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(56, principal(99), &ledger)),
            PendingDriveResult::Rejected("bad_fee")
        );
        assert!(reward_state::get().pending_payout.is_none());
    }

    #[test]
    fn partial_bad_fee_reprices_only_unpaid_recipients_and_completes() {
        reward_state::reset_for_test();
        let mut pending = payout();
        pending.next_recipient_index = 1;
        let completed = pending.recipients[0].clone();
        reward_state::mutate(|state| state.pending_payout = Some(pending));
        let rejected = MockLedger {
            balances: Mutex::new(VecDeque::from([Ok(Nat::from(80_u64))])),
            fees: Mutex::new(VecDeque::new()),
            transfers: Mutex::new(VecDeque::from([Ok(Err(TransferError::BadFee {
                expected_fee: Nat::from(2_u64),
            }))])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(56, principal(99), &rejected)),
            PendingDriveResult::Repricing
        );

        let resumed = MockLedger {
            balances: Mutex::new(VecDeque::from([
                Ok(Nat::from(60_u64)),
                Ok(Nat::from(60_u64)),
                Ok(Nat::from(39_u64)),
            ])),
            fees: Mutex::new(VecDeque::from([Ok(Nat::from(2_u64))])),
            transfers: Mutex::new(VecDeque::from([
                Ok(Ok(Nat::from(2_u64))),
                Ok(Ok(Nat::from(3_u64))),
            ])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(100, principal(99), &resumed)),
            PendingDriveResult::Accepted
        );
        assert!(reward_state::get().pending_payout.is_none());
        let args = resumed.args.lock().unwrap();
        assert_eq!(
            args.iter().map(|arg| arg.to.owner).collect::<Vec<_>>(),
            [principal(2), principal(3),]
        );
        assert!(args.iter().all(|arg| arg.fee == Some(Nat::from(2_u64))));
        assert!(!args.iter().any(|arg| arg.to == completed.recipient));
        assert!(args
            .iter()
            .all(|arg| arg.created_at_time.is_some_and(|created| created >= 100)));
    }

    #[test]
    fn partial_fee_increase_waits_for_accrual_then_resumes_same_entitlements() {
        reward_state::reset_for_test();
        let mut pending = payout();
        pending.next_recipient_index = 1;
        pending.recipients[1].status = PendingRewardTransferStatus::NeedsFreshIdentity;
        let promised = pending.recipients[1..]
            .iter()
            .map(|recipient| recipient.amount.clone())
            .collect::<Vec<_>>();
        reward_state::mutate(|state| state.pending_payout = Some(pending));

        let insufficient = MockLedger {
            balances: Mutex::new(VecDeque::from([Ok(Nat::from(50_u64))])),
            fees: Mutex::new(VecDeque::from([Ok(Nat::from(10_u64))])),
            transfers: Mutex::new(VecDeque::new()),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(100, principal(99), &insufficient)),
            PendingDriveResult::WaitingForBalance
        );
        let waiting = reward_state::get().pending_payout.unwrap();
        assert_eq!(waiting.next_recipient_index, 1);
        assert_eq!(
            waiting.recipients[1..]
                .iter()
                .map(|recipient| recipient.amount.clone())
                .collect::<Vec<_>>(),
            promised
        );

        let funded = MockLedger {
            balances: Mutex::new(VecDeque::from([
                Ok(Nat::from(300_u64)),
                Ok(Nat::from(300_u64)),
                Ok(Nat::from(271_u64)),
            ])),
            fees: Mutex::new(VecDeque::from([Ok(Nat::from(10_u64))])),
            transfers: Mutex::new(VecDeque::from([
                Ok(Ok(Nat::from(2_u64))),
                Ok(Ok(Nat::from(3_u64))),
            ])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(200, principal(99), &funded)),
            PendingDriveResult::Accepted
        );
        let args = funded.args.lock().unwrap();
        assert_eq!(
            args.iter()
                .map(|arg| arg.amount.clone())
                .collect::<Vec<_>>(),
            promised
        );
    }

    #[test]
    fn lower_fee_and_expired_definitive_identity_can_be_safely_repinned() {
        reward_state::reset_for_test();
        let mut pending = payout();
        pending.next_recipient_index = 2;
        pending.fee = Nat::from(10_u64);
        pending.recipients[2].status = PendingRewardTransferStatus::WaitingForBalance;
        pending.recipients[2].created_at_time_nanos = 1;
        reward_state::mutate(|state| state.pending_payout = Some(pending));
        let ledger = MockLedger {
            balances: Mutex::new(VecDeque::from([
                Ok(Nat::from(30_u64)),
                Ok(Nat::from(30_u64)),
            ])),
            fees: Mutex::new(VecDeque::from([Ok(Nat::from(1_u64))])),
            transfers: Mutex::new(VecDeque::from([Ok(Ok(Nat::from(3_u64)))])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(
                1_000_000_000_000,
                principal(99),
                &ledger
            )),
            PendingDriveResult::Accepted
        );
        let args = ledger.args.lock().unwrap();
        assert_eq!(args[0].fee, Some(Nat::from(1_u64)));
        assert_eq!(args[0].created_at_time, Some(1_000_000_000_000));
    }

    #[test]
    fn expired_ambiguous_identity_is_never_replaced() {
        reward_state::reset_for_test();
        let mut pending = payout();
        pending.next_recipient_index = 1;
        pending.recipients[1].status = PendingRewardTransferStatus::Ambiguous;
        pending.recipients[1].attempt_started = true;
        pending.recipients[1].uncertain_attempt_seen = true;
        pending.recipients[1].observed_balance = Some(Nat::from(80_u64));
        pending.recipients[1].created_at_time_nanos = 1;
        let identity = pending.recipients[1].clone();
        reward_state::mutate(|state| state.pending_payout = Some(pending));
        let ledger = MockLedger {
            balances: Mutex::new(VecDeque::from([Ok(Nat::from(80_u64))])),
            fees: Mutex::new(VecDeque::from([Ok(Nat::from(1_u64))])),
            transfers: Mutex::new(VecDeque::new()),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(u64::MAX, principal(99), &ledger)),
            PendingDriveResult::Ambiguous
        );
        let durable = reward_state::get().pending_payout.unwrap();
        assert_eq!(durable.recipients[1], identity);
        assert!(ledger.args.lock().unwrap().is_empty());
    }

    #[test]
    fn rejection_after_partial_completion_retains_the_exact_remaining_plan() {
        reward_state::reset_for_test();
        let mut pinned = payout();
        pinned.next_recipient_index = 1;
        let expected = pinned.clone();
        reward_state::mutate(|state| state.pending_payout = Some(pinned));
        let rejected = MockLedger {
            balances: Mutex::new(VecDeque::from([Ok(Nat::from(80_u64))])),
            fees: Mutex::new(VecDeque::new()),
            transfers: Mutex::new(VecDeque::from([Ok(Err(TransferError::BadFee {
                expected_fee: Nat::from(2_u64),
            }))])),
            args: Mutex::new(Vec::new()),
        };
        assert_eq!(
            block_on(drive_pending_with_ledger(56, principal(99), &rejected)),
            PendingDriveResult::Repricing
        );
        let pending = reward_state::get().pending_payout.unwrap();
        assert_eq!(pending.next_recipient_index, 1);
        assert_eq!(pending.recipients[0], expected.recipients[0]);
        assert_eq!(pending.recipients[1].amount, expected.recipients[1].amount);
        assert_eq!(pending.recipients[1].memo, expected.recipients[1].memo);
        assert_eq!(
            pending.recipients[1].created_at_time_nanos,
            expected.recipients[1].created_at_time_nanos
        );
        assert_eq!(
            pending.recipients[1].status,
            PendingRewardTransferStatus::NeedsFreshIdentity
        );
        assert!(!pending.recipients[1].uncertain_attempt_seen);
    }

    #[test]
    fn cadence_records_only_completed_or_durable_adjudication() {
        reward_state::reset_for_test();
        reward_state::mutate(|state| state.last_sweep_attempt_timestamp_seconds = 7);
        record_sweep_disposition(SweepDisposition::RetryNextDailyTick, 100);
        assert_eq!(reward_state::get().last_sweep_attempt_timestamp_seconds, 7);
        record_sweep_disposition(SweepDisposition::Completed, 100);
        assert_eq!(
            reward_state::get().last_sweep_attempt_timestamp_seconds,
            100
        );
        assert_eq!(
            PendingDriveResult::Ambiguous.sweep_disposition(),
            SweepDisposition::Completed
        );
        assert_eq!(
            PendingDriveResult::Rejected("bad_fee").sweep_disposition(),
            SweepDisposition::RetryNextDailyTick
        );
        assert!(sweep_is_due(99, 100, true));
    }
}
