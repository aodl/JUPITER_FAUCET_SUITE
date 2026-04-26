use candid::Nat;
use std::time::Duration;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::index::{account_identifier_text, IcpIndexCanister};
use crate::clients::sns_root::{SnsCanisterSummary, SnsRootCanister};
use crate::clients::sns_wasm::SnsWasmCanister;
use crate::clients::{BlackholeClient, IndexClient, SnsRootClient, SnsWasmClient};
use crate::{
    logic, MAX_RECENT_INVALID_COMMITMENTS, MAX_RECENT_QUALIFYING_COMMITMENTS,
    MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS,
};
use crate::state::{self, ActiveCyclesSweep, ActiveRouteSweep, ActiveSnsDiscovery, CanisterMeta, CanisterSource, CommitmentIndexFault, CyclesProbeResult, CyclesSampleSource, IndexedRouteKind, InvalidCommitment, RecentCommitment};

const PAGE_SIZE: u64 = 500;
const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;


fn indexed_route_kinds() -> [IndexedRouteKind; 2] {
    [IndexedRouteKind::Output, IndexedRouteKind::Rewards]
}

fn indexed_route_name(kind: &IndexedRouteKind) -> &'static str {
    match kind {
        IndexedRouteKind::Output => "output",
        IndexedRouteKind::Rewards => "rewards",
    }
}

fn indexed_route_account(cfg: &state::Config, kind: &IndexedRouteKind) -> icrc_ledger_types::icrc1::account::Account {
    match kind {
        IndexedRouteKind::Output => cfg.output_account.clone(),
        IndexedRouteKind::Rewards => cfg.rewards_account.clone(),
    }
}

fn indexed_route_cursor(st: &state::State, kind: &IndexedRouteKind) -> Option<u64> {
    match kind {
        IndexedRouteKind::Output => st.last_indexed_output_tx_id,
        IndexedRouteKind::Rewards => st.last_indexed_rewards_tx_id,
    }
}

fn set_indexed_route_cursor(st: &mut state::State, kind: &IndexedRouteKind, cursor: Option<u64>) {
    match kind {
        IndexedRouteKind::Output => st.last_indexed_output_tx_id = cursor,
        IndexedRouteKind::Rewards => st.last_indexed_rewards_tx_id = cursor,
    }
}

fn indexed_route_oldest_cursor(st: &state::State, kind: &IndexedRouteKind) -> Option<u64> {
    match kind {
        IndexedRouteKind::Output => st.oldest_indexed_output_tx_id,
        IndexedRouteKind::Rewards => st.oldest_indexed_rewards_tx_id,
    }
}

fn indexed_route_order_descending(st: &state::State, kind: &IndexedRouteKind) -> Option<bool> {
    match kind {
        IndexedRouteKind::Output => st.output_route_index_descending,
        IndexedRouteKind::Rewards => st.rewards_route_index_descending,
    }
}

fn indexed_route_backfill_complete(st: &state::State, kind: &IndexedRouteKind) -> bool {
    match kind {
        IndexedRouteKind::Output => st.output_route_backfill_complete.unwrap_or(false),
        IndexedRouteKind::Rewards => st.rewards_route_backfill_complete.unwrap_or(false),
    }
}

fn set_indexed_route_descending_progress(
    st: &mut state::State,
    kind: &IndexedRouteKind,
    latest: Option<u64>,
    oldest: Option<u64>,
    backfill_complete: bool,
) {
    match kind {
        IndexedRouteKind::Output => {
            st.last_indexed_output_tx_id = latest;
            st.oldest_indexed_output_tx_id = oldest;
            st.output_route_index_descending = Some(true);
            st.output_route_backfill_complete = Some(backfill_complete);
        }
        IndexedRouteKind::Rewards => {
            st.last_indexed_rewards_tx_id = latest;
            st.oldest_indexed_rewards_tx_id = oldest;
            st.rewards_route_index_descending = Some(true);
            st.rewards_route_backfill_complete = Some(backfill_complete);
        }
    }
}

fn add_indexed_route_amount(st: &mut state::State, kind: &IndexedRouteKind, amount_e8s: u64) {
    match kind {
        IndexedRouteKind::Output => {
            let total = st.total_output_e8s.get_or_insert(0);
            *total = total.saturating_add(amount_e8s);
        }
        IndexedRouteKind::Rewards => {
            let total = st.total_rewards_e8s.get_or_insert(0);
            *total = total.saturating_add(amount_e8s);
        }
    }
}

fn indexed_route_amount_from_tx(
    tx: &crate::clients::index::IndexTransactionWithId,
    expected_from: &str,
    expected_to: &str,
) -> Option<u64> {
    match &tx.transaction.operation {
        crate::clients::index::IndexOperation::Transfer { from, to, amount, .. }
            if from == expected_from && to == expected_to => Some(amount.e8s()),
        crate::clients::index::IndexOperation::TransferFrom { from, to, amount, .. }
            if from == expected_from && to == expected_to => Some(amount.e8s()),
        _ => None,
    }
}

fn commitment_sort_key(item: &RecentCommitment) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn push_recent_commitment(recent: &mut Vec<RecentCommitment>, item: RecentCommitment, max_entries: usize) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| commitment_sort_key(b).cmp(&commitment_sort_key(a)));
    if recent.len() > max_entries {
        recent.truncate(max_entries);
    }
}

fn push_recent_invalid_commitment(recent: &mut Vec<InvalidCommitment>, item: InvalidCommitment) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| (b.timestamp_nanos.unwrap_or(0), b.tx_id).cmp(&(a.timestamp_nanos.unwrap_or(0), a.tx_id)));
    if recent.len() > MAX_RECENT_INVALID_COMMITMENTS {
        recent.truncate(MAX_RECENT_INVALID_COMMITMENTS);
    }
}

fn nat_to_u128(n: &Nat) -> Option<u128> {
    use num_traits::ToPrimitive;
    n.0.to_u128()
}

fn log_cycles_once_per_week(cycles: u128) {
    #[cfg(test)]
    {
        let _ = cycles;
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("Cycles: {}", cycles);
    }
}

fn log_error(message: &str) {
    #[cfg(test)]
    {
        let _ = message;
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("ERR:{}", message);
    }
}

fn latch_commitment_index_fault(now_secs: u64, last_cursor_tx_id: Option<u64>, offending_tx_id: u64, message: String) -> String {
    state::with_root_state_mut(|st| {
        match st.commitment_index_fault.as_mut() {
            Some(existing) => {
                existing.last_cursor_tx_id = last_cursor_tx_id;
                existing.offending_tx_id = offending_tx_id;
                existing.message = message.clone();
            }
            None => {
                st.commitment_index_fault = Some(CommitmentIndexFault {
                    observed_at_ts: now_secs,
                    last_cursor_tx_id,
                    offending_tx_id,
                    message: message.clone(),
                });
            }
        }
    });
    message
}

struct MainGuard {
    active: bool,
    lease_expires_at_ts: u64,
}

impl MainGuard {
    fn acquire(now_secs: u64) -> Option<Self> {
        state::with_root_state_mut(|st| {
            let lock_expires_at_ts = st.main_lock_state_ts.unwrap_or(0);
            if lock_expires_at_ts > now_secs {
                return None;
            }
            let lease_expires_at_ts = now_secs.saturating_add(MAIN_TICK_LEASE_SECONDS);
            st.main_lock_state_ts = Some(lease_expires_at_ts);
            Some(Self {
                active: true,
                lease_expires_at_ts,
            })
        })
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_root_state_mut(|st| {
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;
    }

    fn finish(mut self, now_secs: u64) {
        let lease_expires_at_ts = self.lease_expires_at_ts;
        state::with_root_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            if st.main_lock_state_ts == Some(lease_expires_at_ts) {
                st.main_lock_state_ts = Some(0);
            }
        });
        self.active = false;
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) { self.release(); }
}

pub fn install_timers() {
    let interval_s = state::with_state(|st| st.config.scan_interval_seconds);
    ic_cdk_timers::set_timer(Duration::from_secs(1), async { main_tick(true).await; });
    ic_cdk_timers::set_timer_interval(Duration::from_secs(interval_s.max(60)), || async { main_tick(false).await; });
}

pub async fn main_tick(force: bool) {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    let Some(guard) = MainGuard::acquire(now_secs) else { return; };
    if !force {
        let min_gap = state::with_state(|st| st.config.scan_interval_seconds.saturating_sub(5));
        let recently_ran = state::with_state(|st| now_secs.saturating_sub(st.last_main_run_ts) < min_gap);
        if recently_ran {
            guard.finish(now_secs);
            return;
        }
    }

    let (index_id, blackhole_id, sns_wasm_id) = state::with_state(|st| (
        st.config.index_canister_id,
        st.config.blackhole_canister_id,
        st.config.sns_wasm_canister_id,
    ));
    let index = IcpIndexCanister::new(index_id);
    let blackhole = BlackholeCanister::new(blackhole_id);
    let sns_wasm = SnsWasmCanister::new(sns_wasm_id);
    let sns_root = SnsRootCanister;
    if let Err(err) = run_main_tick_with_clients(now_nanos, now_secs, &index, &blackhole, &sns_wasm, &sns_root).await {
        log_error(&format!("historian main tick failed: {err}"));
    }
    guard.finish(now_secs);
}

async fn run_main_tick_with_clients<I: IndexClient, B: BlackholeClient, W: SnsWasmClient, R: SnsRootClient>(
    now_nanos: u64,
    now_secs: u64,
    index: &I,
    blackhole: &B,
    sns_wasm: &W,
    sns_root: &R,
) -> Result<(), String> {
    if let Err(err) = process_commitment_indexing(index, now_secs).await {
        log_error(&format!("historian commitment indexing degraded: {err}"));
    }
    process_route_indexing(now_nanos, now_secs, index).await?;

    let (enable_sns_tracking, last_sns_discovery_ts, last_completed_cycles_sweep_ts, active_cycles_present, active_sns_present, interval_secs) = state::with_state(|st| (
        st.config.enable_sns_tracking,
        st.last_sns_discovery_ts,
        st.last_completed_cycles_sweep_ts,
        st.active_cycles_sweep.is_some(),
        st.active_sns_discovery.is_some(),
        st.config.cycles_interval_seconds,
    ));

    let sns_due = enable_sns_tracking && (active_sns_present || now_secs.saturating_sub(last_sns_discovery_ts) >= interval_secs);
    if sns_due {
        process_sns_discovery(now_nanos, now_secs, sns_wasm, sns_root).await?;
    }

    let cycles_due = active_cycles_present || now_secs.saturating_sub(last_completed_cycles_sweep_ts) >= interval_secs;
    if cycles_due {
        process_cycles_sweep(now_nanos, now_secs, blackhole).await?;
    }

    Ok(())
}

fn apply_verified_qualifying_commitment(
    st: &mut crate::state::State,
    commitment: crate::logic::IndexedCommitment,
    now_secs: u64,
) {
    st.distinct_canisters.insert(commitment.beneficiary);
    st.canister_sources.insert(
        commitment.beneficiary,
        logic::merge_sources(st.canister_sources.get(&commitment.beneficiary), CanisterSource::MemoCommitment),
    );
    let recent_item = RecentCommitment {
        canister_id: commitment.beneficiary,
        tx_id: commitment.tx_id,
        timestamp_nanos: commitment.timestamp_nanos,
        amount_e8s: commitment.amount_e8s,
        counts_toward_faucet: true,
    };
    crate::state::ensure_commitment_history_loaded(st, commitment.beneficiary);
    let history = st.commitment_history.entry(commitment.beneficiary).or_default();
    let inserted = logic::push_commitment(
        history,
        crate::state::CommitmentSample {
            tx_id: commitment.tx_id,
            timestamp_nanos: commitment.timestamp_nanos,
            amount_e8s: commitment.amount_e8s,
            counts_toward_faucet: true,
        },
        st.config.max_commitment_entries_per_canister,
    );
    if inserted {
        let meta = st.per_canister_meta.entry(commitment.beneficiary).or_insert_with(CanisterMeta::default);
        logic::apply_commitment_seen(meta, commitment.timestamp_nanos, now_secs);
        let recent = st.recent_commitments.get_or_insert_with(Vec::new);
        push_recent_commitment(recent, recent_item, MAX_RECENT_QUALIFYING_COMMITMENTS);
        let count = st.qualifying_commitment_count.get_or_insert(0);
        *count = count.saturating_add(1);
    }
    if inserted || st.canister_sources.contains_key(&commitment.beneficiary) {
        crate::refresh_registered_canister_summary(st, commitment.beneficiary);
    }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageOrder {
    Ascending,
    Descending,
}

fn detect_page_order(page: &crate::clients::index::GetAccountIdentifierTransactionsResponse) -> Option<PageOrder> {
    if page.transactions.len() < 2 {
        return None;
    }
    let first = page.transactions.first().expect("page has a first tx").id;
    let last = page.transactions.last().expect("page has a last tx").id;
    if first < last {
        Some(PageOrder::Ascending)
    } else if first > last {
        Some(PageOrder::Descending)
    } else {
        None
    }
}

fn infer_initial_page_order(
    page: &crate::clients::index::GetAccountIdentifierTransactionsResponse,
    latest_cursor: Option<u64>,
    oldest_cursor: Option<u64>,
) -> PageOrder {
    if let Some(order) = detect_page_order(page) {
        return order;
    }
    // A single-item page is ambiguous unless a persisted cursor proves that the
    // item is older than already-indexed history. In that case the real ICP
    // index's newest-first pagination is the only ordering compatible with the
    // page; otherwise keep the legacy ascending path.
    let first_id = page.transactions.first().map(|tx| tx.id);
    if first_id.zip(oldest_cursor.or(latest_cursor)).map(|(tx_id, cursor)| tx_id < cursor).unwrap_or(false) {
        PageOrder::Descending
    } else {
        PageOrder::Ascending
    }
}

fn apply_indexed_commitment_tx(
    tx: &crate::clients::index::IndexTransactionWithId,
    staking_id: &str,
    min_tx_e8s: u64,
    now_secs: u64,
) {
    if let Some(commitment) = logic::indexed_commitment_from_tx(tx, staking_id, min_tx_e8s) {
        match commitment {
            logic::IndexedCommitmentEntry::Valid(commitment) => {
                if commitment.counts_toward_faucet {
                    let dirty_beneficiary = commitment.beneficiary;
                    state::with_root_registry_and_commitments_canister_state_mut(dirty_beneficiary, |st| {
                        apply_verified_qualifying_commitment(st, commitment, now_secs);
                    });
                } else {
                    state::with_root_state_mut(|st| {
                        let recent = st.recent_under_threshold_commitments.get_or_insert_with(Vec::new);
                        push_recent_commitment(
                            recent,
                            RecentCommitment {
                                canister_id: commitment.beneficiary,
                                tx_id: commitment.tx_id,
                                timestamp_nanos: commitment.timestamp_nanos,
                                amount_e8s: commitment.amount_e8s,
                                counts_toward_faucet: false,
                            },
                            MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS,
                        );
                    });
                }
            }
            logic::IndexedCommitmentEntry::Invalid(commitment) => {
                state::with_root_state_mut(|st| {
                    let recent = st.recent_invalid_commitments.get_or_insert_with(Vec::new);
                    push_recent_invalid_commitment(
                        recent,
                        InvalidCommitment {
                            tx_id: commitment.tx_id,
                            timestamp_nanos: commitment.timestamp_nanos,
                            amount_e8s: commitment.amount_e8s,
                            memo_text: commitment.memo_text,
                        },
                    );
                });
            }
        }
    }
}

fn apply_commitment_transactions_in_chronological_order(
    txs: &[crate::clients::index::IndexTransactionWithId],
    staking_id: &str,
    min_tx_e8s: u64,
    now_secs: u64,
) {
    for tx in txs.iter().rev() {
        apply_indexed_commitment_tx(tx, staking_id, min_tx_e8s, now_secs);
    }
}

async fn process_commitment_indexing_ascending<I: IndexClient>(
    index: &I,
    now_secs: u64,
    cfg: &state::Config,
    had_fault: bool,
    staking_id: &str,
    mut cursor: Option<u64>,
    mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse>,
) -> Result<(), String> {
    for _ in 0..cfg.max_index_pages_per_tick.max(1) {
        let page = match first_page.take() {
            Some(page) => page,
            None => index
                .get_account_identifier_transactions(staking_id.to_string(), cursor, PAGE_SIZE)
                .await
                .map_err(|e| format!("index call failed: {e}"))?,
        };
        if page.transactions.is_empty() {
            break;
        }
        {
            let _batch = state::begin_persistence_batch();
            for tx in page.transactions.iter() {
                if let Some(prev) = cursor {
                    if tx.id == prev {
                        continue;
                    }
                    if tx.id < prev {
                        return Err(latch_commitment_index_fault(
                            now_secs,
                            cursor,
                            tx.id,
                            format!(
                                "historian observed non-monotonic staking-account tx_id {} after cursor {:?}",
                                tx.id, cursor,
                            ),
                        ));
                    }
                }
                apply_indexed_commitment_tx(tx, staking_id, cfg.min_tx_e8s, now_secs);
                cursor = Some(tx.id);
                state::with_root_state_mut(|st| {
                    st.last_indexed_staking_tx_id = cursor;
                    st.oldest_indexed_staking_tx_id = cursor;
                    st.staking_index_descending = Some(false);
                    st.staking_backfill_complete = Some(true);
                });
            }
        }
        if page.transactions.len() < PAGE_SIZE as usize {
            break;
        }
    }
    state::with_root_state_mut(|st| {
        st.last_index_run_ts = Some(now_secs);
        if had_fault {
            st.commitment_index_fault = None;
        }
    });
    Ok(())
}

// The real ICP index returns account history newest-first and uses the `start`
// cursor to walk toward older transactions. Descending mode therefore keeps two
// cursors: `latest_cursor` is the highest/newest tx id observed so future ticks
// can pick up new arrivals from the latest page, while `oldest_cursor` is the
// oldest tx id backfilled so older history can continue without treating normal
// lower tx ids as non-monotonic.
async fn process_commitment_indexing_descending_seeded<I: IndexClient>(
    index: &I,
    now_secs: u64,
    cfg: &state::Config,
    had_fault: bool,
    staking_id: &str,
    mut latest_cursor: Option<u64>,
    mut oldest_cursor: Option<u64>,
    mut backfill_complete: bool,
    mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse>,
) -> Result<(), String> {
    let mut remaining_pages = cfg.max_index_pages_per_tick.max(1);

    if latest_cursor.is_some() && oldest_cursor.is_none() {
        oldest_cursor = latest_cursor;
    }

    if let Some(latest) = latest_cursor {
        let mut page_start = None;
        while remaining_pages > 0 {
            let page = match first_page.take() {
                Some(page) => page,
                None => index
                    .get_account_identifier_transactions(staking_id.to_string(), page_start, PAGE_SIZE)
                    .await
                    .map_err(|e| format!("index call failed: {e}"))?,
            };
            remaining_pages = remaining_pages.saturating_sub(1);
            if page.transactions.is_empty() {
                break;
            }
            let mut new_items = Vec::new();
            let mut reached_boundary = false;
            for tx in page.transactions.iter() {
                if tx.id == latest {
                    reached_boundary = true;
                    break;
                }
                if tx.id > latest {
                    new_items.push(tx.clone());
                    continue;
                }
                reached_boundary = true;
                break;
            }
            if !new_items.is_empty() {
                let _batch = state::begin_persistence_batch();
                apply_commitment_transactions_in_chronological_order(&new_items, staking_id, cfg.min_tx_e8s, now_secs);
                if let Some(max_new) = new_items.iter().map(|tx| tx.id).max() {
                    latest_cursor = Some(latest_cursor.map(|existing| existing.max(max_new)).unwrap_or(max_new));
                    state::with_root_state_mut(|st| st.last_indexed_staking_tx_id = latest_cursor);
                }
            }
            if reached_boundary || page.transactions.len() < PAGE_SIZE as usize {
                break;
            }
            page_start = page.transactions.last().map(|tx| tx.id);
        }
    }

    while remaining_pages > 0 && !backfill_complete {
        let page = match first_page.take() {
            Some(page) => page,
            None => index
                .get_account_identifier_transactions(staking_id.to_string(), oldest_cursor, PAGE_SIZE)
                .await
                .map_err(|e| format!("index call failed: {e}"))?,
        };
        remaining_pages = remaining_pages.saturating_sub(1);
        if page.transactions.is_empty() {
            backfill_complete = true;
            break;
        }
        let older_items: Vec<_> = match oldest_cursor {
            Some(oldest) => page.transactions.iter().filter(|tx| tx.id < oldest).cloned().collect(),
            None => page.transactions.clone(),
        };
        if older_items.is_empty() {
            backfill_complete = true;
            break;
        }
        {
            let _batch = state::begin_persistence_batch();
            apply_commitment_transactions_in_chronological_order(&older_items, staking_id, cfg.min_tx_e8s, now_secs);
            if let Some(max_seen) = older_items.iter().map(|tx| tx.id).max() {
                latest_cursor = Some(latest_cursor.map(|existing| existing.max(max_seen)).unwrap_or(max_seen));
            }
            if let Some(min_seen) = older_items.iter().map(|tx| tx.id).min() {
                oldest_cursor = Some(oldest_cursor.map(|existing| existing.min(min_seen)).unwrap_or(min_seen));
            }
            state::with_root_state_mut(|st| {
                st.last_indexed_staking_tx_id = latest_cursor;
                st.oldest_indexed_staking_tx_id = oldest_cursor;
                st.staking_index_descending = Some(true);
                st.staking_backfill_complete = Some(backfill_complete);
            });
        }
        if page.transactions.len() < PAGE_SIZE as usize {
            backfill_complete = true;
            break;
        }
    }

    state::with_root_state_mut(|st| {
        st.last_indexed_staking_tx_id = latest_cursor;
        st.oldest_indexed_staking_tx_id = oldest_cursor;
        st.staking_index_descending = Some(true);
        st.staking_backfill_complete = Some(backfill_complete);
        st.last_index_run_ts = Some(now_secs);
        if had_fault {
            st.commitment_index_fault = None;
        }
    });
    Ok(())
}

async fn process_commitment_indexing<I: IndexClient>(index: &I, now_secs: u64) -> Result<(), String> {
    let cfg = state::with_state(|st| st.config.clone());
    let (had_fault, latest_cursor, oldest_cursor, order_descending, backfill_complete) = state::with_state(|st| {
        (
            st.commitment_index_fault.is_some(),
            st.last_indexed_staking_tx_id,
            st.oldest_indexed_staking_tx_id,
            st.staking_index_descending,
            st.staking_backfill_complete.unwrap_or(false),
        )
    });
    let staking_id = account_identifier_text(&cfg.staking_account);

    match order_descending {
        Some(false) => {
            process_commitment_indexing_ascending(index, now_secs, &cfg, had_fault, &staking_id, latest_cursor, None).await
        }
        Some(true) => {
            process_commitment_indexing_descending_seeded(
                index,
                now_secs,
                &cfg,
                had_fault,
                &staking_id,
                latest_cursor,
                oldest_cursor,
                backfill_complete,
                None,
            )
            .await
        }
        None => {
            let first_page = index
                .get_account_identifier_transactions(staking_id.clone(), None, PAGE_SIZE)
                .await
                .map_err(|e| format!("index call failed: {e}"))?;
            match infer_initial_page_order(&first_page, latest_cursor, oldest_cursor) {
                PageOrder::Ascending => {
                    state::with_root_state_mut(|st| st.staking_index_descending = Some(false));
                    process_commitment_indexing_ascending(index, now_secs, &cfg, had_fault, &staking_id, latest_cursor, Some(first_page)).await
                }
                PageOrder::Descending => {
                    state::with_root_state_mut(|st| st.staking_index_descending = Some(true));
                    process_commitment_indexing_descending_seeded(
                        index,
                        now_secs,
                        &cfg,
                        had_fault,
                        &staking_id,
                        latest_cursor,
                        oldest_cursor,
                        backfill_complete,
                        Some(first_page),
                    )
                    .await
                }
            }
        }
    }
}

async fn process_route_indexing<I: IndexClient>(started_at_ts_nanos: u64, now_secs: u64, index: &I) -> Result<(), String> {
    let cfg = state::with_state(|st| st.config.clone());
    let routes = indexed_route_kinds();
    let active = state::with_root_state_mut(|st| {
        if st.active_route_sweep.is_none() {
            st.active_route_sweep = Some(ActiveRouteSweep {
                started_at_ts_nanos,
                next_index: 0,
            });
        }
        st.active_route_sweep.clone().expect("active route sweep")
    });
    if active.next_index as usize >= routes.len() {
        state::with_root_state_mut(|st| {
            st.active_route_sweep = None;
            st.last_completed_route_sweep_ts = Some(now_secs);
        });
        return Ok(());
    }

    let kind = &routes[active.next_index as usize];
    let source_id = account_identifier_text(&cfg.output_source_account);
    let route_id = account_identifier_text(&indexed_route_account(&cfg, kind));
    let (mut latest_cursor, mut oldest_cursor, order_descending, mut backfill_complete) = state::with_state(|st| {
        (
            indexed_route_cursor(st, kind),
            indexed_route_oldest_cursor(st, kind),
            indexed_route_order_descending(st, kind),
            indexed_route_backfill_complete(st, kind),
        )
    });
    if latest_cursor.is_some() && oldest_cursor.is_none() {
        oldest_cursor = latest_cursor;
    }

    let mut completed_route = false;
    let mut first_page: Option<crate::clients::index::GetAccountIdentifierTransactionsResponse> = None;
    // Route indexing uses the same two-cursor model as commitment indexing in
    // descending mode: the latest cursor detects newer routed transfers, and the
    // oldest cursor continues the historical backfill through newest-first pages.
    let order = match order_descending {
        Some(true) => PageOrder::Descending,
        Some(false) => PageOrder::Ascending,
        None => {
            let page = index
                .get_account_identifier_transactions(route_id.clone(), None, PAGE_SIZE)
                .await
                .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?;
            let detected = infer_initial_page_order(&page, latest_cursor, oldest_cursor);
            first_page = Some(page);
            detected
        }
    };

    match order {
        PageOrder::Ascending => {
            let mut cursor = latest_cursor;
            for _ in 0..cfg.max_index_pages_per_tick.max(1) {
                let page = match first_page.take() {
                    Some(page) => page,
                    None => index
                        .get_account_identifier_transactions(route_id.clone(), cursor, PAGE_SIZE)
                        .await
                        .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?,
                };
                if page.transactions.is_empty() {
                    completed_route = true;
                    break;
                }
                {
                    let _batch = state::begin_persistence_batch();
                    for tx in page.transactions.iter() {
                        if let Some(prev) = cursor {
                            if tx.id == prev {
                                continue;
                            }
                            if tx.id < prev {
                                return Err(format!(
                                    "historian observed non-monotonic {}-route tx_id {} after cursor {:?}",
                                    indexed_route_name(kind),
                                    tx.id,
                                    cursor,
                                ));
                            }
                        }
                        if let Some(amount_e8s) = indexed_route_amount_from_tx(tx, &source_id, &route_id) {
                            state::with_root_state_mut(|st| add_indexed_route_amount(st, kind, amount_e8s));
                        }
                        cursor = Some(tx.id);
                        state::with_root_state_mut(|st| {
                            set_indexed_route_cursor(st, kind, cursor);
                            match kind {
                                IndexedRouteKind::Output => {
                                    st.oldest_indexed_output_tx_id = cursor;
                                    st.output_route_index_descending = Some(false);
                                    st.output_route_backfill_complete = Some(true);
                                }
                                IndexedRouteKind::Rewards => {
                                    st.oldest_indexed_rewards_tx_id = cursor;
                                    st.rewards_route_index_descending = Some(false);
                                    st.rewards_route_backfill_complete = Some(true);
                                }
                            }
                        });
                    }
                }
                if page.transactions.len() < PAGE_SIZE as usize {
                    completed_route = true;
                    break;
                }
            }
        }
        PageOrder::Descending => {
            let mut remaining_pages = cfg.max_index_pages_per_tick.max(1);

            if latest_cursor.is_some() && backfill_complete {
                let mut page_start = None;
                while remaining_pages > 0 {
                    let page = match first_page.take() {
                        Some(page) => page,
                        None => index
                            .get_account_identifier_transactions(route_id.clone(), page_start, PAGE_SIZE)
                            .await
                            .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?,
                    };
                    remaining_pages = remaining_pages.saturating_sub(1);
                    if page.transactions.is_empty() {
                        completed_route = true;
                        break;
                    }
                    let latest = latest_cursor.expect("latest cursor present in completed descending route scan");
                    let mut new_items = Vec::new();
                    let mut reached_boundary = false;
                    for tx in page.transactions.iter() {
                        if tx.id > latest {
                            new_items.push(tx.clone());
                        } else {
                            reached_boundary = true;
                            break;
                        }
                    }
                    if !new_items.is_empty() {
                        let _batch = state::begin_persistence_batch();
                        for tx in new_items.iter().rev() {
                            if let Some(amount_e8s) = indexed_route_amount_from_tx(tx, &source_id, &route_id) {
                                state::with_root_state_mut(|st| add_indexed_route_amount(st, kind, amount_e8s));
                            }
                        }
                        if let Some(max_seen) = new_items.iter().map(|tx| tx.id).max() {
                            latest_cursor = Some(latest_cursor.map(|existing| existing.max(max_seen)).unwrap_or(max_seen));
                        }
                        state::with_root_state_mut(|st| set_indexed_route_descending_progress(st, kind, latest_cursor, oldest_cursor, true));
                    }
                    if reached_boundary || page.transactions.len() < PAGE_SIZE as usize {
                        completed_route = true;
                        break;
                    }
                    page_start = page.transactions.last().map(|tx| tx.id);
                }
            } else {
                while remaining_pages > 0 && !backfill_complete {
                    let page = match first_page.take() {
                        Some(page) => page,
                        None => index
                            .get_account_identifier_transactions(route_id.clone(), oldest_cursor, PAGE_SIZE)
                            .await
                            .map_err(|e| format!("{} route index call failed: {e}", indexed_route_name(kind)))?,
                    };
                    remaining_pages = remaining_pages.saturating_sub(1);
                    if page.transactions.is_empty() {
                        backfill_complete = true;
                        completed_route = true;
                        break;
                    }
                    let older_items: Vec<_> = match oldest_cursor {
                        Some(oldest) => page.transactions.iter().filter(|tx| tx.id < oldest).cloned().collect(),
                        None => page.transactions.clone(),
                    };
                    if older_items.is_empty() {
                        backfill_complete = true;
                        completed_route = true;
                        break;
                    }
                    {
                        let _batch = state::begin_persistence_batch();
                        for tx in older_items.iter().rev() {
                            if let Some(amount_e8s) = indexed_route_amount_from_tx(tx, &source_id, &route_id) {
                                state::with_root_state_mut(|st| add_indexed_route_amount(st, kind, amount_e8s));
                            }
                        }
                        if let Some(max_seen) = older_items.iter().map(|tx| tx.id).max() {
                            latest_cursor = Some(latest_cursor.map(|existing| existing.max(max_seen)).unwrap_or(max_seen));
                        }
                        if let Some(min_seen) = older_items.iter().map(|tx| tx.id).min() {
                            oldest_cursor = Some(oldest_cursor.map(|existing| existing.min(min_seen)).unwrap_or(min_seen));
                        }
                        state::with_root_state_mut(|st| set_indexed_route_descending_progress(st, kind, latest_cursor, oldest_cursor, backfill_complete));
                    }
                    if page.transactions.len() < PAGE_SIZE as usize {
                        backfill_complete = true;
                        completed_route = true;
                        break;
                    }
                }
                state::with_root_state_mut(|st| set_indexed_route_descending_progress(st, kind, latest_cursor, oldest_cursor, backfill_complete));
            }
        }
    }

    if completed_route {
        state::with_root_state_mut(|st| {
            if let Some(active) = st.active_route_sweep.as_mut() {
                active.next_index = active.next_index.saturating_add(1);
                if active.next_index as usize >= indexed_route_kinds().len() {
                    st.active_route_sweep = None;
                    st.last_completed_route_sweep_ts = Some(now_secs);
                }
            }
        });
    }
    Ok(())
}


fn apply_sns_canister_summary(timestamp_nanos: u64, now_secs: u64, max_cycles_entries: u32, summary: SnsCanisterSummary) {
    let Some(canister_id) = summary.canister_id else { return; };
    let cycles = summary.status.and_then(|status| status.cycles).and_then(|cycles| nat_to_u128(&cycles));
    let dirty_canister_id = canister_id.clone();
    state::with_root_registry_and_cycles_canister_state_mut(dirty_canister_id, |st| {
        st.distinct_canisters.insert(canister_id);
        st.canister_sources.insert(
            canister_id,
            logic::merge_sources(st.canister_sources.get(&canister_id), CanisterSource::SnsDiscovery),
        );
        if let Some(cycles) = cycles {
            crate::state::ensure_cycles_history_loaded(st, canister_id);
            let history = st.cycles_history.entry(canister_id).or_default();
            let inserted = logic::push_cycles_sample(
                history,
                logic::make_cycles_sample(timestamp_nanos, cycles, CyclesSampleSource::SnsRootSummary),
                max_cycles_entries,
            );
            let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
            if meta.first_seen_ts.is_none() {
                meta.first_seen_ts = Some(now_secs);
            }
            if inserted {
                logic::apply_cycles_probe_result(
                    meta,
                    timestamp_nanos,
                    CyclesProbeResult::Ok(CyclesSampleSource::SnsRootSummary),
                );
            }
        } else {
            let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
            if meta.first_seen_ts.is_none() {
                meta.first_seen_ts = Some(now_secs);
            }
            logic::apply_cycles_probe_result(meta, timestamp_nanos, CyclesProbeResult::NotAvailable);
        }
        crate::refresh_registered_canister_summary(st, canister_id);
    });
}

async fn process_sns_discovery<W: SnsWasmClient, R: SnsRootClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    sns_wasm: &W,
    sns_root: &R,
) -> Result<(), String> {
    let (snapshot, max_per_tick, max_cycles_entries) = state::with_root_state_mut(|st| {
        if st.active_sns_discovery.is_none() {
            st.active_sns_discovery = Some(ActiveSnsDiscovery {
                started_at_ts_nanos: timestamp_nanos,
                root_canister_ids: Vec::new(),
                next_index: 0,
            });
        }
        (
            st.active_sns_discovery.clone().expect("active sns discovery"),
            st.config.max_canisters_per_cycles_tick.max(1),
            st.config.max_cycles_entries_per_canister,
        )
    });

    let snapshot = if snapshot.root_canister_ids.is_empty() && snapshot.next_index == 0 {
        let deployed = sns_wasm.list_deployed_snses().await.map_err(|e| format!("list_deployed_snses failed: {e}"))?;
        let mut root_canister_ids: Vec<_> = deployed
            .instances
            .into_iter()
            .filter_map(|sns| sns.root_canister_id)
            .collect();
        root_canister_ids.sort();
        root_canister_ids.dedup();
        state::with_root_state_mut(|st| {
            if let Some(active) = st.active_sns_discovery.as_mut() {
                active.root_canister_ids = root_canister_ids.clone();
            }
        });
        ActiveSnsDiscovery {
            started_at_ts_nanos: snapshot.started_at_ts_nanos,
            root_canister_ids,
            next_index: 0,
        }
    } else {
        snapshot
    };

    let discovery_timestamp_nanos = snapshot.started_at_ts_nanos;
    let start = snapshot.next_index as usize;
    let end = (snapshot.next_index + max_per_tick as u64).min(snapshot.root_canister_ids.len() as u64) as usize;
    for root_id in snapshot.root_canister_ids[start..end].iter().copied() {
        let summary = sns_root.get_sns_canisters_summary(root_id).await.map_err(|e| format!("get_sns_canisters_summary failed: {e}"))?;
        if let Some(summary) = summary.root { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
        if let Some(summary) = summary.governance { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
        if let Some(summary) = summary.ledger { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
        if let Some(summary) = summary.swap { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
        if let Some(summary) = summary.index { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
        for summary in summary.dapps { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
        for summary in summary.archives { apply_sns_canister_summary(discovery_timestamp_nanos, now_secs, max_cycles_entries, summary); }
    }

    state::with_root_state_mut(|st| {
        if let Some(active) = st.active_sns_discovery.as_mut() {
            active.next_index = end as u64;
            if active.next_index >= active.root_canister_ids.len() as u64 {
                st.active_sns_discovery = None;
                st.last_sns_discovery_ts = now_secs;
            }
        }
    });
    Ok(())
}

async fn process_cycles_sweep<B: BlackholeClient>(timestamp_nanos: u64, now_secs: u64, blackhole: &B) -> Result<(), String> {
    let (snapshot, max_per_tick, max_entries) = state::with_root_state_mut(|st| {
        if st.active_cycles_sweep.is_none() {
            let self_id = ic_cdk::api::canister_self();
            let mut canisters = vec![self_id];
            for canister_id in st.distinct_canisters.iter().copied() {
                let sources = st.canister_sources.get(&canister_id).cloned().unwrap_or_default();
                let memo_registered = sources.contains(&CanisterSource::MemoCommitment)
                    && st
                        .commitment_history
                        .get(&canister_id)
                        .map(|history| history.iter().any(|item| item.counts_toward_faucet))
                        .unwrap_or(false);
                if !memo_registered && !sources.contains(&CanisterSource::SnsDiscovery) {
                    continue;
                }
                if logic::should_skip_blackhole_for_sources(&sources) {
                    continue;
                }
                canisters.push(canister_id);
            }
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: timestamp_nanos,
                canisters,
                next_index: 0,
            });
        }
        (
            st.active_cycles_sweep.clone().expect("active sweep"),
            st.config.max_canisters_per_cycles_tick.max(1),
            st.config.max_cycles_entries_per_canister,
        )
    });

    let self_id = ic_cdk::api::canister_self();
    let started_at_ts_nanos = snapshot.started_at_ts_nanos;
    let start = snapshot.next_index as usize;
    let end = (snapshot.next_index + max_per_tick as u64).min(snapshot.canisters.len() as u64) as usize;
    for canister_id in snapshot.canisters[start..end].iter().copied() {
        if canister_id == self_id {
            let cycles = ic_cdk::api::canister_cycle_balance();
            log_cycles_once_per_week(cycles);
            state::with_root_registry_and_cycles_canister_state_mut(canister_id, |st| {
                crate::state::ensure_cycles_history_loaded(st, canister_id);
                let history = st.cycles_history.entry(canister_id).or_default();
                let inserted = logic::push_cycles_sample(
                    history,
                    logic::make_cycles_sample(started_at_ts_nanos, cycles, CyclesSampleSource::SelfCanister),
                    max_entries,
                );
                let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                if meta.first_seen_ts.is_none() {
                    meta.first_seen_ts = Some(now_secs);
                }
                if inserted {
                    logic::apply_cycles_probe_result(
                        meta,
                        started_at_ts_nanos,
                        CyclesProbeResult::Ok(CyclesSampleSource::SelfCanister),
                    );
                }
                crate::refresh_registered_canister_summary(st, canister_id);
            });
            continue;
        }

        match blackhole.canister_status(canister_id).await {
            Ok(status) => {
                let cycles = nat_to_u128(&status.cycles)
                    .ok_or_else(|| "cycles overflow converting nat to u128".to_string())?;
                state::with_root_registry_and_cycles_canister_state_mut(canister_id, |st| {
                    crate::state::ensure_cycles_history_loaded(st, canister_id);
                    let history = st.cycles_history.entry(canister_id).or_default();
                    let inserted = logic::push_cycles_sample(
                        history,
                        logic::make_cycles_sample(
                            started_at_ts_nanos,
                            cycles,
                            CyclesSampleSource::BlackholeStatus,
                        ),
                        max_entries,
                    );
                    let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                    if meta.first_seen_ts.is_none() {
                        meta.first_seen_ts = Some(now_secs);
                    }
                    if inserted {
                        logic::apply_cycles_probe_result(
                            meta,
                            started_at_ts_nanos,
                            CyclesProbeResult::Ok(CyclesSampleSource::BlackholeStatus),
                        );
                    }
                    crate::refresh_registered_canister_summary(st, canister_id);
                });
            }
            Err(err) => {
                state::with_root_and_registry_canister_state_mut(canister_id, |st| {
                    let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                    if meta.first_seen_ts.is_none() {
                        meta.first_seen_ts = Some(now_secs);
                    }
                    logic::apply_cycles_probe_result(
                        meta,
                        started_at_ts_nanos,
                        CyclesProbeResult::Error(err.to_string()),
                    );
                    crate::refresh_registered_canister_summary(st, canister_id);
                });
            }
        }
    }

    state::with_root_state_mut(|st| {
        if let Some(active) = st.active_cycles_sweep.as_mut() {
            active.next_index = end as u64;
            if active.next_index >= active.canisters.len() as u64 {
                st.active_cycles_sweep = None;
                st.last_completed_cycles_sweep_ts = now_secs;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::index::{GetAccountIdentifierTransactionsResponse, IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId, Tokens};
    use crate::state::{Config, State};
    use async_trait::async_trait;
    use candid::Principal;
    use futures::executor::block_on;
    use icrc_ledger_types::icrc1::account::Account;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    fn principal(text: &str) -> candid::Principal {
        candid::Principal::from_text(text).unwrap()
    }

    fn sample_account() -> Account {
        Account { owner: principal("aaaaa-aa"), subaccount: None }
    }

    fn configure_state(max_index_pages_per_tick: u32) -> String {
        let account = sample_account();
        let staking_id = account_identifier_text(&account);
        state::set_state(State::new(
            Config {
                staking_account: account,
                output_source_account: Account { owner: principal("uccpi-cqaaa-aaaar-qby3q-cai"), subaccount: None },
                output_account: Account { owner: principal("acjuz-liaaa-aaaar-qb4qq-cai"), subaccount: None },
                rewards_account: Account { owner: principal("alk7f-5aaaa-aaaar-qb4ra-cai"), subaccount: None },
                ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
                index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
                cmc_canister_id: Some(principal("rkp4c-7iaaa-aaaaa-aaaca-cai")),
                faucet_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                blackhole_canister_id: principal("e3mmv-5qaaa-aaaah-aadma-cai"),
                sns_wasm_canister_id: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
                enable_sns_tracking: false,
                scan_interval_seconds: 600,
                cycles_interval_seconds: 604800,
                min_tx_e8s: 100,
                max_cycles_entries_per_canister: 100,
                max_commitment_entries_per_canister: 100,
                max_index_pages_per_tick,
                max_canisters_per_cycles_tick: 25,
            },
            0,
        ));
        staking_id
    }

    fn transfer_to_staking_memo_tx(id: u64, staking_id: &str, memo: Vec<u8>, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(memo),
                operation: IndexOperation::Transfer {
                    to: staking_id.to_string(),
                    fee: Tokens::new(10_000),
                    from: "sender".into(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }

    fn transfer_to_staking_tx(id: u64, staking_id: &str, beneficiary: candid::Principal, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(beneficiary.to_text().into_bytes()),
                operation: IndexOperation::Transfer {
                    to: staking_id.to_string(),
                    fee: Tokens::new(10_000),
                    from: "sender".into(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }


    fn transfer_between_accounts_tx(id: u64, from: &str, to: &str, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to: to.to_string(),
                    fee: Tokens::new(10_000),
                    from: from.to_string(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }

    fn transfer_from_between_accounts_tx(id: u64, from: &str, to: &str, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::TransferFrom {
                    to: to.to_string(),
                    fee: Tokens::new(10_000),
                    from: from.to_string(),
                    amount: Tokens::new(amount_e8s),
                    spender: "spender".into(),
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }
    struct MockIndexClient {
        pages: Mutex<VecDeque<GetAccountIdentifierTransactionsResponse>>,
        calls: Mutex<Vec<(String, Option<u64>, u64)>>,
    }

    impl MockIndexClient {
        fn new(pages: Vec<GetAccountIdentifierTransactionsResponse>) -> Self {
            Self {
                pages: Mutex::new(pages.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, Option<u64>, u64)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for MockIndexClient {
        async fn get_account_identifier_transactions(
            &self,
            account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            self.calls.lock().unwrap().push((account_identifier, start, max_results));
            Ok(self
                .pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: Vec::new(),
                    oldest_tx_id: None,
                }))
        }
    }


    struct MockSnsWasmClient {
        responses: Mutex<VecDeque<Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError>>>,
        calls: Mutex<u32>,
    }

    impl MockSnsWasmClient {
        fn new(responses: Vec<Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: Mutex::new(0),
            }
        }

        fn calls(&self) -> u32 {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl SnsWasmClient for MockSnsWasmClient {
        async fn list_deployed_snses(&self) -> Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError> {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(crate::clients::sns_wasm::ListDeployedSnsesResponse { instances: Vec::new() }))
        }
    }

    struct MockSnsRootClient {
        responses: Mutex<BTreeMap<Principal, crate::clients::sns_root::GetSnsCanistersSummaryResponse>>,
        calls: Mutex<Vec<Principal>>,
    }

    impl MockSnsRootClient {
        fn new(responses: BTreeMap<Principal, crate::clients::sns_root::GetSnsCanistersSummaryResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Principal> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SnsRootClient for MockSnsRootClient {
        async fn get_sns_canisters_summary(&self, root_id: Principal) -> Result<crate::clients::sns_root::GetSnsCanistersSummaryResponse, crate::clients::ClientError> {
            self.calls.lock().unwrap().push(root_id);
            self.responses
                .lock()
                .unwrap()
                .get(&root_id)
                .cloned()
                .ok_or_else(|| crate::clients::ClientError::Call(format!("missing summary for {}", root_id)))
        }
    }

    fn sns_summary(canister_id: Principal, cycles: u64) -> SnsCanisterSummary {
        SnsCanisterSummary {
            canister_id: Some(canister_id),
            status: Some(crate::clients::sns_root::SnsCanisterStatus { cycles: Some(Nat::from(cycles)) }),
        }
    }


    struct MockBlackholeClient;

    #[async_trait]
    impl BlackholeClient for MockBlackholeClient {
        async fn canister_status(&self, canister_id: Principal) -> Result<crate::clients::blackhole::BlackholeCanisterStatus, crate::clients::ClientError> {
            Ok(crate::clients::blackhole::BlackholeCanisterStatus {
                cycles: Nat::from(0u64),
                settings: crate::clients::blackhole::BlackholeSettings { controllers: vec![canister_id] },
            })
        }
    }


    #[test]
    fn sns_discovery_chunks_across_ticks_and_resumes_from_persisted_state() {
        let _staking_id = configure_state(10);
        let root_a = candid::Principal::from_slice(&[1]);
        let root_b = candid::Principal::from_slice(&[2]);
        let root_c = candid::Principal::from_slice(&[3]);
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.cycles_interval_seconds = 10;
            st.config.max_canisters_per_cycles_tick = 2;
            st.last_sns_discovery_ts = 0;
            st.active_sns_discovery = None;
        });
        let sns_wasm = MockSnsWasmClient::new(vec![Ok(crate::clients::sns_wasm::ListDeployedSnsesResponse {
            instances: vec![
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_b.clone()) },
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_a.clone()) },
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_b.clone()) },
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_c.clone()) },
            ],
        })]);
        let mut summaries = BTreeMap::new();
        summaries.insert(root_a.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_a.clone(), 10)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        summaries.insert(root_b.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_b.clone(), 20)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        summaries.insert(root_c.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_c.clone(), 30)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        let sns_root = MockSnsRootClient::new(summaries);

        block_on(process_sns_discovery(123, 100, &sns_wasm, &sns_root)).unwrap();
        state::with_state(|st| {
            let active = st.active_sns_discovery.as_ref().expect("discovery should remain in progress after first batch");
            assert_eq!(active.root_canister_ids, vec![root_a.clone(), root_b.clone(), root_c.clone()]);
            assert_eq!(active.next_index, 2);
            assert_eq!(st.last_sns_discovery_ts, 0);
            assert!(st.distinct_canisters.contains(&root_a));
            assert!(st.distinct_canisters.contains(&root_b));
            assert!(!st.distinct_canisters.contains(&root_c));
        });
        assert_eq!(sns_wasm.calls(), 1);
        assert_eq!(sns_root.calls(), vec![root_a.clone(), root_b.clone()]);

        block_on(process_sns_discovery(456, 101, &sns_wasm, &sns_root)).unwrap();
        state::with_state(|st| {
            assert!(st.active_sns_discovery.is_none());
            assert_eq!(st.last_sns_discovery_ts, 101);
            assert!(st.distinct_canisters.contains(&root_c));
            let history = st.cycles_history.get(&root_c).expect("cycles history for final root");
            assert_eq!(history.last().map(|sample| sample.cycles), Some(30));
        });
        assert_eq!(sns_wasm.calls(), 1, "deployed SNS roots should be fetched only once per discovery sweep");
        assert_eq!(sns_root.calls(), vec![root_a.clone(), root_b.clone(), root_c.clone()]);
    }

    #[test]
    fn active_sns_discovery_resumes_even_when_interval_is_not_due() {
        let _staking_id = configure_state(10);
        let root_a = candid::Principal::from_slice(&[1]);
        let root_b = candid::Principal::from_slice(&[2]);
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.cycles_interval_seconds = 10_000;
            st.config.max_canisters_per_cycles_tick = 1;
            st.last_sns_discovery_ts = 9_999;
            st.active_sns_discovery = Some(ActiveSnsDiscovery {
                started_at_ts_nanos: 55,
                root_canister_ids: vec![root_a.clone(), root_b.clone()],
                next_index: 1,
            });
            st.last_completed_cycles_sweep_ts = 10_000;
        });
        let index = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse { balance: 0, transactions: Vec::new(), oldest_tx_id: None }]);
        let blackhole = MockBlackholeClient;
        let sns_wasm = MockSnsWasmClient::new(vec![]);
        let mut summaries = BTreeMap::new();
        summaries.insert(root_b.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_b.clone(), 44)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        let sns_root = MockSnsRootClient::new(summaries);

        block_on(run_main_tick_with_clients(999, 10_000, &index, &blackhole, &sns_wasm, &sns_root)).unwrap();
        state::with_state(|st| {
            assert!(st.active_sns_discovery.is_none());
            assert_eq!(st.last_sns_discovery_ts, 10_000);
            assert!(st.distinct_canisters.contains(&root_b));
        });
        assert_eq!(sns_wasm.calls(), 0, "resumed discovery should not refetch deployed SNS roots");
        assert_eq!(sns_root.calls(), vec![root_b.clone()]);
    }

    #[test]
    fn indexing_single_qualifying_commitment_updates_counts() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000)],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(42));
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.last_index_run_ts, Some(200));
            assert!(st.canister_sources.get(&beneficiary).unwrap().contains(&CanisterSource::MemoCommitment));
        });
    }

    #[test]
    fn indexing_duplicate_tx_does_not_double_count() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let tx = transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![tx.clone()],
                oldest_tx_id: Some(42),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![tx],
                oldest_tx_id: Some(42),
            },
        ]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();
        block_on(process_commitment_indexing(&mock, 201)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
        });
    }

    #[test]
    fn indexing_uses_cursor_and_keeps_recent_commitments_descending() {
        let staking_id = configure_state(1);
        let first_canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let second_canister = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![transfer_to_staking_tx(10, &staking_id, first_canister, 100, 100_000_000_000)],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![transfer_to_staking_tx(11, &staking_id, second_canister, 200, 300_000_000_000)],
                oldest_tx_id: Some(11),
            },
        ]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();
        block_on(process_commitment_indexing(&mock, 201)).unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, None);
        assert_eq!(calls[1].1, Some(10));

        state::with_state(|st| {
            let recent = st.recent_commitments.as_ref().unwrap();
            assert_eq!(recent.len(), 2);
            assert_eq!(recent[0].tx_id, 11);
            assert_eq!(recent[1].tx_id, 10);
            assert_eq!(st.last_indexed_staking_tx_id, Some(11));
        });
    }

    #[test]
    fn route_indexing_counts_only_protocol_routed_output_and_rewards_and_resumes_across_ticks() {
        let _staking_id = configure_state(10);
        let (source, output, rewards) = state::with_state(|st| (
            st.config.output_source_account.clone(),
            st.config.output_account.clone(),
            st.config.rewards_account.clone(),
        ));
        let source_id = account_identifier_text(&source);
        let output_id = account_identifier_text(&output);
        let rewards_id = account_identifier_text(&rewards);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_between_accounts_tx(10, &source_id, &output_id, 111_000_000, 10),
                    transfer_between_accounts_tx(11, "third-party", &output_id, 999_000_000, 11),
                ],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_between_accounts_tx(20, &source_id, &rewards_id, 22_000_000, 20),
                    transfer_between_accounts_tx(21, "third-party", &rewards_id, 333_000_000, 21),
                ],
                oldest_tx_id: Some(20),
            },
        ]);

        block_on(process_route_indexing(100, 200, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.total_rewards_e8s, Some(0));
            assert_eq!(st.last_indexed_output_tx_id, Some(11));
            assert_eq!(st.last_indexed_rewards_tx_id, None);
            let active = st.active_route_sweep.as_ref().expect("route sweep should continue to rewards");
            assert_eq!(active.next_index, 1);
        });

        block_on(process_route_indexing(101, 201, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.total_rewards_e8s, Some(22_000_000));
            assert_eq!(st.last_indexed_output_tx_id, Some(11));
            assert_eq!(st.last_indexed_rewards_tx_id, Some(21));
            assert!(st.active_route_sweep.is_none());
            assert_eq!(st.last_completed_route_sweep_ts, Some(201));
        });

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, output_id);
        assert_eq!(calls[1].0, rewards_id);
    }

    #[test]
    fn route_indexing_counts_transfer_from_and_skips_repeated_cursor_without_double_counting() {
        let _staking_id = configure_state(1);
        let (source, output, rewards) = state::with_state(|st| (
            st.config.output_source_account.clone(),
            st.config.output_account.clone(),
            st.config.rewards_account.clone(),
        ));
        let source_id = account_identifier_text(&source);
        let output_id = account_identifier_text(&output);
        let rewards_id = account_identifier_text(&rewards);
        let filler: Vec<_> = (11..(10 + PAGE_SIZE))
            .map(|id| transfer_between_accounts_tx(id, "third-party", &output_id, 1_000, id))
            .collect();
        let mut first_page = vec![transfer_from_between_accounts_tx(10, &source_id, &output_id, 111_000_000, 10)];
        first_page.extend(filler);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: first_page,
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_from_between_accounts_tx(10 + PAGE_SIZE - 1, &source_id, &output_id, 999_000_000, 20),
                    transfer_between_accounts_tx(10 + PAGE_SIZE, &source_id, &output_id, 22_000_000, 21),
                    transfer_between_accounts_tx(10 + PAGE_SIZE + 1, "third-party", &output_id, 333_000_000, 22),
                ],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![transfer_between_accounts_tx(30, &source_id, &rewards_id, 5_000_000, 30)],
                oldest_tx_id: Some(30),
            },
        ]);

        block_on(process_route_indexing(100, 200, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.last_indexed_output_tx_id, Some(10 + PAGE_SIZE - 1));
            assert_eq!(st.active_route_sweep.as_ref().map(|active| active.next_index), Some(0));
        });

        block_on(process_route_indexing(101, 201, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(133_000_000), "repeated cursor tx should be skipped while the new routed transfer is counted once");
            assert_eq!(st.last_indexed_output_tx_id, Some(10 + PAGE_SIZE + 1));
            assert_eq!(st.active_route_sweep.as_ref().map(|active| active.next_index), Some(1));
        });

        block_on(process_route_indexing(102, 202, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(133_000_000));
            assert_eq!(st.total_rewards_e8s, Some(5_000_000));
            assert_eq!(st.last_indexed_rewards_tx_id, Some(30));
            assert!(st.active_route_sweep.is_none());
        });
    }


    #[test]
    fn non_monotonic_commitment_page_latches_fault_and_stops_indexing() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(50);
            st.oldest_indexed_staking_tx_id = Some(50);
            st.staking_index_descending = Some(false);
            st.staking_backfill_complete = Some(true);
        });
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 300,
            transactions: vec![
                transfer_to_staking_tx(51, &staking_id, beneficiary, 150, 124_000_000_000),
                transfer_to_staking_tx(49, &staking_id, beneficiary, 150, 123_000_000_000),
            ],
            oldest_tx_id: Some(49),
        }]);

        let err = block_on(process_commitment_indexing(&mock, 200)).unwrap_err();
        assert!(err.contains("non-monotonic"));
        state::with_state(|st| {
            let fault = st.commitment_index_fault.as_ref().expect("fault should be latched");
            assert_eq!(fault.observed_at_ts, 200);
            assert_eq!(fault.last_cursor_tx_id, Some(51));
            assert_eq!(fault.offending_tx_id, 49);
            assert_eq!(st.last_indexed_staking_tx_id, Some(51));
            assert_eq!(st.last_index_run_ts, Some(0));
        });
    }

    #[test]
    fn commitment_index_fault_clears_automatically_once_index_order_recovers() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(50);
            st.oldest_indexed_staking_tx_id = Some(50);
            st.staking_index_descending = Some(false);
            st.staking_backfill_complete = Some(true);
        });
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![
                transfer_to_staking_tx(51, &staking_id, beneficiary, 150, 124_000_000_000),
                transfer_to_staking_tx(49, &staking_id, beneficiary, 150, 123_000_000_000),
            ],
                oldest_tx_id: Some(49),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 450,
                transactions: vec![
                    transfer_to_staking_tx(51, &staking_id, beneficiary, 300, 124_000_000_000),
                    transfer_to_staking_tx(52, &staking_id, beneficiary, 450, 125_000_000_000),
                ],
                oldest_tx_id: Some(51),
            },
        ]);

        let err = block_on(process_commitment_indexing(&mock, 200)).unwrap_err();
        assert!(err.contains("non-monotonic"));
        state::with_state(|st| {
            let fault = st.commitment_index_fault.as_ref().expect("fault should be latched");
            assert_eq!(fault.observed_at_ts, 200);
            assert_eq!(fault.last_cursor_tx_id, Some(51));
            assert_eq!(fault.offending_tx_id, 49);
            assert_eq!(st.last_indexed_staking_tx_id, Some(51));
        });

        block_on(process_commitment_indexing(&mock, 201)).unwrap();
        state::with_state(|st| {
            assert!(st.commitment_index_fault.is_none(), "fault should auto-clear after a clean retry");
            assert_eq!(st.last_indexed_staking_tx_id, Some(52));
            assert_eq!(st.last_index_run_ts, Some(201));
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(2));
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 52);
        });
    }

    #[test]
    fn indexing_retains_non_qualifying_and_invalid_memo_commitments_in_separate_recent_lists_without_registering_under_threshold_canisters() {
        let staking_id = configure_state(10);
        let qualifying = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let low_amount = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 410,
            transactions: vec![
                transfer_to_staking_tx(42, &staking_id, qualifying, 150, 123_000_000_000),
                transfer_to_staking_tx(43, &staking_id, low_amount, 50, 124_000_000_000),
                transfer_to_staking_memo_tx(44, &staking_id, b"not-a-principal".to_vec(), 210, 125_000_000_000),
            ],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(
                st.recent_under_threshold_commitments
                    .as_ref()
                    .map(|items| items.len()),
                Some(1),
            );
            assert_eq!(st.recent_invalid_commitments.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.recent_under_threshold_commitments.as_ref().unwrap()[0].tx_id, 43);
            assert!(!st.canister_sources.contains_key(&low_amount));
            assert!(!st.distinct_canisters.contains(&low_amount));
            assert!(!st.commitment_history.contains_key(&low_amount));
            let invalid = &st.recent_invalid_commitments.as_ref().unwrap()[0];
            assert_eq!(invalid.tx_id, 44);
            assert_eq!(invalid.memo_text, crate::logic::INVALID_MEMO_PLACEHOLDER);
        });
    }

    #[test]
    fn indexing_caps_under_threshold_recent_list_without_registering_distinct_memo_beneficiaries() {
        let staking_id = configure_state(10);
        let pages = vec![GetAccountIdentifierTransactionsResponse {
            balance: 10_000,
            transactions: (1..=105)
                .map(|tx_id| {
                    let canister = candid::Principal::from_slice(&[1, (tx_id % 251 + 1) as u8]);
                    transfer_to_staking_tx(
                        tx_id,
                        &staking_id,
                        canister,
                        5,
                        tx_id * 1_000_000_000,
                    )
                })
                .collect(),
            oldest_tx_id: Some(1),
        }];
        let mock = MockIndexClient::new(pages);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            let recent = st
                .recent_under_threshold_commitments
                .as_ref()
                .expect("under-threshold recent list should exist");
            assert_eq!(recent.len(), MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS);
            assert_eq!(recent[0].tx_id, 105);
            assert_eq!(recent.last().map(|item| item.tx_id), Some(6));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(0));
            assert_eq!(st.canister_sources.len(), 0);
            assert_eq!(st.distinct_canisters.len(), 0);
            assert!(st.commitment_history.is_empty());
            assert_eq!(st.qualifying_commitment_count, Some(0));
        });
    }

    #[test]
    fn indexing_registers_new_qualifying_canisters_without_pruning_existing_beneficiaries() {
        let staking_id = configure_state(10);
        let existing = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(existing);
            st.canister_sources
                .insert(existing, crate::logic::merge_sources(None, CanisterSource::MemoCommitment));
            st.commitment_history.insert(
                existing,
                vec![crate::state::CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                }],
            );
            st.qualifying_commitment_count = Some(1);
        });
        let new_canister = candid::Principal::from_slice(&[251, 251, 251]);
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(9_999, &staking_id, new_canister, 150, 123_000_000_000)],
            oldest_tx_id: Some(9_999),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 9_999);
            assert!(st.canister_sources.contains_key(&new_canister));
            assert!(st.commitment_history.contains_key(&new_canister));
            assert!(st.distinct_canisters.contains(&new_canister));
            assert!(st.distinct_canisters.contains(&existing));
        });
    }


}
