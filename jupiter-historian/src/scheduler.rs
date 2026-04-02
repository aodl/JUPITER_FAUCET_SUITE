use candid::Nat;
use std::time::Duration;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::index::{account_identifier_text, IcpIndexCanister, IndexOperation};
use crate::clients::sns_root::{SnsCanisterSummary, SnsRootCanister};
use crate::clients::sns_wasm::SnsWasmCanister;
use crate::clients::{BlackholeClient, IndexClient, SnsRootClient, SnsWasmClient};
use crate::{logic, mainnet_cmc_id};
use crate::state::{self, ActiveCyclesSweep, CanisterMeta, CanisterSource, CyclesProbeResult, CyclesSampleSource, InvalidContribution, RecentBurn, RecentContribution};

const PAGE_SIZE: u64 = 500;
const MAX_RECENT_CONTRIBUTIONS: usize = 250;
const MAX_RECENT_INVALID_CONTRIBUTIONS: usize = 250;
const MAX_RECENT_BURNS: usize = 250;

fn contribution_sort_key(item: &RecentContribution) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn burn_sort_key(item: &RecentBurn) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn push_recent_contribution(recent: &mut Vec<RecentContribution>, item: RecentContribution) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| contribution_sort_key(b).cmp(&contribution_sort_key(a)));
    if recent.len() > MAX_RECENT_CONTRIBUTIONS {
        recent.truncate(MAX_RECENT_CONTRIBUTIONS);
    }
}

fn push_recent_invalid_contribution(recent: &mut Vec<InvalidContribution>, item: InvalidContribution) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| (b.timestamp_nanos.unwrap_or(0), b.tx_id).cmp(&(a.timestamp_nanos.unwrap_or(0), a.tx_id)));
    if recent.len() > MAX_RECENT_INVALID_CONTRIBUTIONS {
        recent.truncate(MAX_RECENT_INVALID_CONTRIBUTIONS);
    }
}

fn push_recent_burn(recent: &mut Vec<RecentBurn>, item: RecentBurn) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| burn_sort_key(b).cmp(&burn_sort_key(a)));
    if recent.len() > MAX_RECENT_BURNS {
        recent.truncate(MAX_RECENT_BURNS);
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

struct MainGuard {
    active: bool,
}

impl MainGuard {
    fn acquire() -> Option<Self> {
        state::with_state_mut(|st| {
            if st.main_lock_expires_at_ts.unwrap_or(0) != 0 {
                return None;
            }
            st.main_lock_expires_at_ts = Some(1);
            Some(Self { active: true })
        })
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        state::with_state_mut(|st| st.main_lock_expires_at_ts = Some(0));
        self.active = false;
    }

    fn finish(mut self, now_secs: u64) {
        state::with_state_mut(|st| {
            st.last_main_run_ts = now_secs;
            st.main_lock_expires_at_ts = Some(0);
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
    let Some(guard) = MainGuard::acquire() else { return; };
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

    let _ = run_main_tick_with_clients(now_nanos, now_secs, &index, &blackhole, &sns_wasm, &sns_root).await;
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
    process_contribution_indexing(index, now_secs).await?;
    process_burn_indexing(index).await?;

    let (enable_sns_tracking, last_sns_discovery_ts, last_completed_cycles_sweep_ts, active_present, interval_secs) = state::with_state(|st| (
        st.config.enable_sns_tracking,
        st.last_sns_discovery_ts,
        st.last_completed_cycles_sweep_ts,
        st.active_cycles_sweep.is_some(),
        st.config.cycles_interval_seconds,
    ));

    let sns_due = enable_sns_tracking && now_secs.saturating_sub(last_sns_discovery_ts) >= interval_secs;
    if sns_due {
        process_sns_discovery(now_nanos, now_secs, sns_wasm, sns_root).await?;
    }

    let cycles_due = active_present || now_secs.saturating_sub(last_completed_cycles_sweep_ts) >= interval_secs;
    if cycles_due {
        process_cycles_sweep(now_nanos, now_secs, blackhole).await?;
    }

    Ok(())
}

async fn process_contribution_indexing<I: IndexClient>(index: &I, now_secs: u64) -> Result<(), String> {
    let cfg = state::with_state(|st| st.config.clone());
    let staking_id = account_identifier_text(&cfg.staking_account);
    let mut cursor = state::with_state(|st| st.last_indexed_staking_tx_id);
    for _ in 0..cfg.max_index_pages_per_tick.max(1) {
        let page = index
            .get_account_identifier_transactions(staking_id.clone(), cursor, PAGE_SIZE)
            .await
            .map_err(|e| format!("index call failed: {e}"))?;
        if page.transactions.is_empty() {
            break;
        }
        for tx in page.transactions.iter() {
            if let Some(contribution) = logic::indexed_contribution_from_tx(tx, &staking_id, cfg.min_tx_e8s) {
                state::with_state_mut(|st| match contribution {
                    logic::IndexedContributionEntry::Valid(contribution) => {
                        st.distinct_canisters.insert(contribution.beneficiary);
                        st.canister_sources.insert(
                            contribution.beneficiary,
                            logic::merge_sources(st.canister_sources.get(&contribution.beneficiary), CanisterSource::MemoContribution),
                        );
                        let history = st.contribution_history.entry(contribution.beneficiary).or_default();
                        let inserted = logic::push_contribution(
                            history,
                            crate::state::ContributionSample {
                                tx_id: contribution.tx_id,
                                timestamp_nanos: contribution.timestamp_nanos,
                                amount_e8s: contribution.amount_e8s,
                                counts_toward_faucet: contribution.counts_toward_faucet,
                            },
                            cfg.max_contribution_entries_per_canister,
                        );
                        if inserted {
                            let meta = st.per_canister_meta.entry(contribution.beneficiary).or_insert_with(CanisterMeta::default);
                            logic::apply_contribution_seen(meta, contribution.timestamp_nanos, now_secs);
                            if contribution.counts_toward_faucet {
                                let count = st.qualifying_contribution_count.get_or_insert(0);
                                *count = count.saturating_add(1);
                            }
                            let recent = st.recent_contributions.get_or_insert_with(Vec::new);
                            push_recent_contribution(
                                recent,
                                RecentContribution {
                                    canister_id: contribution.beneficiary,
                                    tx_id: contribution.tx_id,
                                    timestamp_nanos: contribution.timestamp_nanos,
                                    amount_e8s: contribution.amount_e8s,
                                    counts_toward_faucet: contribution.counts_toward_faucet,
                                },
                            );
                        }
                    }
                    logic::IndexedContributionEntry::Invalid(contribution) => {
                        let recent = st.recent_invalid_contributions.get_or_insert_with(Vec::new);
                        push_recent_invalid_contribution(
                            recent,
                            InvalidContribution {
                                tx_id: contribution.tx_id,
                                timestamp_nanos: contribution.timestamp_nanos,
                                amount_e8s: contribution.amount_e8s,
                                memo_text: contribution.memo_text,
                            },
                        );
                    }
                });
            }
            cursor = Some(tx.id);
            state::with_state_mut(|st| st.last_indexed_staking_tx_id = cursor);
        }
        if page.transactions.len() < PAGE_SIZE as usize {
            break;
        }
    }
    state::with_state_mut(|st| st.last_index_run_ts = Some(now_secs));
    Ok(())
}

async fn process_burn_indexing<I: IndexClient>(index: &I) -> Result<(), String> {
    let (max_pages_per_tick, cmc_id, targets) = state::with_state(|st| (
        st.config.max_index_pages_per_tick.max(1),
        st.config.cmc_canister_id.clone().unwrap_or_else(mainnet_cmc_id),
        crate::burn_target_canisters(st).into_iter().collect::<Vec<_>>(),
    ));

    for canister_id in targets {
        let deposit_account = logic::cmc_deposit_account(cmc_id, canister_id);
        let deposit_account_id = account_identifier_text(&deposit_account);
        let mut cursor = state::with_state(|st| {
            st.per_canister_meta
                .get(&canister_id)
                .and_then(|m| m.last_burn_scan_tx_id.or(m.last_burn_tx_id))
        });
        for _ in 0..max_pages_per_tick {
            let page = index
                .get_account_identifier_transactions(deposit_account_id.clone(), cursor, PAGE_SIZE)
                .await
                .map_err(|e| format!("burn index call failed: {e}"))?;
            if page.transactions.is_empty() {
                break;
            }

            let mut last_seen = cursor;
            let mut last_actual_burn_tx_id = None;
            let mut added = 0u64;
            let mut recent_burns = Vec::new();
            for tx in page.transactions.iter() {
                if cursor.map(|prev| tx.id <= prev).unwrap_or(false) {
                    continue;
                }
                last_seen = Some(tx.id);
                match &tx.transaction.operation {
                    IndexOperation::Burn { amount, .. } => {
                        let amount_e8s = amount.e8s();
                        added = added.saturating_add(amount_e8s);
                        last_actual_burn_tx_id = Some(tx.id);
                        recent_burns.push(RecentBurn {
                            canister_id,
                            tx_id: tx.id,
                            timestamp_nanos: tx.transaction.timestamp.as_ref().or(tx.transaction.created_at_time.as_ref()).map(|ts| ts.timestamp_nanos),
                            amount_e8s,
                        });
                    }
                    _ => {}
                }
            }

            state::with_state_mut(|st| {
                let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                if let Some(last_seen) = last_seen {
                    meta.last_burn_scan_tx_id = Some(last_seen);
                }
                if let Some(last_actual_burn_tx_id) = last_actual_burn_tx_id {
                    meta.last_burn_tx_id = Some(last_actual_burn_tx_id);
                }
                if added > 0 {
                    meta.burned_e8s = meta.burned_e8s.saturating_add(added);
                    let total = st.icp_burned_e8s.get_or_insert(0);
                    *total = total.saturating_add(added);
                    let recent = st.recent_burns.get_or_insert_with(Vec::new);
                    for burn in recent_burns {
                        push_recent_burn(recent, burn);
                    }
                }
            });

            cursor = last_seen;
            if page.transactions.len() < PAGE_SIZE as usize {
                break;
            }
        }
    }

    Ok(())
}

async fn process_sns_discovery<W: SnsWasmClient, R: SnsRootClient>(
    timestamp_nanos: u64,
    now_secs: u64,
    sns_wasm: &W,
    sns_root: &R,
) -> Result<(), String> {
    let max_cycles_entries = state::with_state(|st| st.config.max_cycles_entries_per_canister);
    let deployed = sns_wasm.list_deployed_snses().await.map_err(|e| format!("list_deployed_snses failed: {e}"))?;
    for sns in deployed.instances {
        let Some(root_id) = sns.root_canister_id else { continue; };
        let summary = sns_root.get_sns_canisters_summary(root_id).await.map_err(|e| format!("get_sns_canisters_summary failed: {e}"))?;
        let process_summary = |summary: SnsCanisterSummary| {
            let Some(canister_id) = summary.canister_id else { return; };
            let cycles = summary.status.and_then(|status| status.cycles).and_then(|cycles| nat_to_u128(&cycles));
            state::with_state_mut(|st| {
                st.distinct_canisters.insert(canister_id);
                st.canister_sources.insert(canister_id, logic::merge_sources(st.canister_sources.get(&canister_id), CanisterSource::SnsDiscovery));
                let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                if meta.first_seen_ts.is_none() {
                    meta.first_seen_ts = Some(now_secs);
                }
                if let Some(cycles) = cycles {
                    let history = st.cycles_history.entry(canister_id).or_default();
                    let inserted = logic::push_cycles_sample(history, logic::make_cycles_sample(timestamp_nanos, cycles, CyclesSampleSource::SnsRootSummary), max_cycles_entries);
                    if inserted {
                        logic::apply_cycles_probe_result(meta, timestamp_nanos, CyclesProbeResult::Ok(CyclesSampleSource::SnsRootSummary));
                    }
                } else {
                    logic::apply_cycles_probe_result(meta, timestamp_nanos, CyclesProbeResult::NotAvailable);
                }
            });
        };

        if let Some(summary) = summary.root { process_summary(summary); }
        if let Some(summary) = summary.governance { process_summary(summary); }
        if let Some(summary) = summary.ledger { process_summary(summary); }
        if let Some(summary) = summary.swap { process_summary(summary); }
        if let Some(summary) = summary.index { process_summary(summary); }
        for summary in summary.dapps { process_summary(summary); }
        for summary in summary.archives { process_summary(summary); }
    }
    state::with_state_mut(|st| st.last_sns_discovery_ts = now_secs);
    Ok(())
}

async fn process_cycles_sweep<B: BlackholeClient>(timestamp_nanos: u64, now_secs: u64, blackhole: &B) -> Result<(), String> {
    let (snapshot, max_per_tick, max_entries) = state::with_state_mut(|st| {
        if st.active_cycles_sweep.is_none() {
            let self_id = ic_cdk::api::canister_self();
            let mut canisters = vec![self_id];
            for canister_id in st.distinct_canisters.iter().copied() {
                let sources = st.canister_sources.get(&canister_id).cloned().unwrap_or_default();
                if logic::should_skip_blackhole_for_sources(&sources) {
                    continue;
                }
                canisters.push(canister_id);
            }
            st.active_cycles_sweep = Some(ActiveCyclesSweep { started_at_ts_nanos: timestamp_nanos, canisters, next_index: 0 });
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
            state::with_state_mut(|st| {
                let history = st.cycles_history.entry(canister_id).or_default();
                let inserted = logic::push_cycles_sample(history, logic::make_cycles_sample(started_at_ts_nanos, cycles, CyclesSampleSource::SelfCanister), max_entries);
                let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                if meta.first_seen_ts.is_none() {
                    meta.first_seen_ts = Some(now_secs);
                }
                if inserted {
                    logic::apply_cycles_probe_result(meta, started_at_ts_nanos, CyclesProbeResult::Ok(CyclesSampleSource::SelfCanister));
                }
            });
            continue;
        }

        match blackhole.canister_status(canister_id).await {
            Ok(status) => {
                let cycles = nat_to_u128(&status.cycles).ok_or_else(|| "cycles overflow converting nat to u128".to_string())?;
                state::with_state_mut(|st| {
                    let history = st.cycles_history.entry(canister_id).or_default();
                    let inserted = logic::push_cycles_sample(history, logic::make_cycles_sample(started_at_ts_nanos, cycles, CyclesSampleSource::BlackholeStatus), max_entries);
                    let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                    if meta.first_seen_ts.is_none() {
                        meta.first_seen_ts = Some(now_secs);
                    }
                    if inserted {
                        logic::apply_cycles_probe_result(meta, started_at_ts_nanos, CyclesProbeResult::Ok(CyclesSampleSource::BlackholeStatus));
                    }
                });
            }
            Err(err) => {
                state::with_state_mut(|st| {
                    let meta = st.per_canister_meta.entry(canister_id).or_insert_with(CanisterMeta::default);
                    if meta.first_seen_ts.is_none() {
                        meta.first_seen_ts = Some(now_secs);
                    }
                    logic::apply_cycles_probe_result(meta, started_at_ts_nanos, CyclesProbeResult::Error(err.to_string()));
                });
            }
        }
    }

    state::with_state_mut(|st| {
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
    use futures::executor::block_on;
    use icrc_ledger_types::icrc1::account::Account;
    use std::collections::VecDeque;
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
                max_contribution_entries_per_canister: 100,
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


    fn transfer_to_account_tx(id: u64, to: &str, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to: to.to_string(),
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


    fn burn_tx(id: u64, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Burn {
                    from: "sender".into(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
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

    #[test]
    fn indexing_single_qualifying_contribution_updates_counts() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000)],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_contribution_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(42));
            assert_eq!(st.qualifying_contribution_count, Some(1));
            assert_eq!(st.icp_burned_e8s, Some(0));
            assert_eq!(st.recent_contributions.as_ref().unwrap().len(), 1);
            assert_eq!(st.recent_contributions.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.last_index_run_ts, Some(200));
            assert!(st.canister_sources.get(&beneficiary).unwrap().contains(&CanisterSource::MemoContribution));
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

        block_on(process_contribution_indexing(&mock, 200)).unwrap();
        block_on(process_contribution_indexing(&mock, 201)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_contribution_count, Some(1));
            assert_eq!(st.icp_burned_e8s, Some(0));
            assert_eq!(st.recent_contributions.as_ref().unwrap().len(), 1);
        });
    }

    #[test]
    fn indexing_uses_cursor_and_keeps_recent_contributions_descending() {
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

        block_on(process_contribution_indexing(&mock, 200)).unwrap();
        block_on(process_contribution_indexing(&mock, 201)).unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, None);
        assert_eq!(calls[1].1, Some(10));

        state::with_state(|st| {
            let recent = st.recent_contributions.as_ref().unwrap();
            assert_eq!(recent.len(), 2);
            assert_eq!(recent[0].tx_id, 11);
            assert_eq!(recent[1].tx_id, 10);
            assert_eq!(st.last_indexed_staking_tx_id, Some(11));
        });
    }

    #[test]
    fn indexing_retains_non_qualifying_and_invalid_memo_commitments_in_recent_list() {
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

        block_on(process_contribution_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_contribution_count, Some(1));
            assert_eq!(st.recent_contributions.as_ref().map(|items| items.len()), Some(2));
            assert_eq!(st.recent_invalid_contributions.as_ref().map(|items| items.len()), Some(1));
            let invalid = &st.recent_invalid_contributions.as_ref().unwrap()[0];
            assert_eq!(invalid.tx_id, 44);
            assert_eq!(invalid.memo_text, "not-a-principal");
        });
    }

    #[test]
    fn burn_indexing_counts_only_actual_burn_entries() {
        let _staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.canister_sources.insert(
                beneficiary,
                crate::logic::merge_sources(None, CanisterSource::MemoContribution),
            );
            st.distinct_canisters.insert(beneficiary);
        });
        let cmc_account_id = account_identifier_text(&crate::logic::cmc_deposit_account(principal("rkp4c-7iaaa-aaaaa-aaaca-cai"), beneficiary));
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![],
                oldest_tx_id: None,
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![transfer_to_account_tx(77, &cmc_account_id, 150, 123_000_000_000)],
                oldest_tx_id: Some(77),
            },
        ]);

        block_on(process_burn_indexing(&mock)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.icp_burned_e8s, Some(0));
            assert_eq!(st.per_canister_meta.get(&beneficiary).map(|m| m.burned_e8s), Some(0));
            assert_eq!(st.per_canister_meta.get(&beneficiary).and_then(|m| m.last_burn_tx_id), None);
            assert_eq!(st.per_canister_meta.get(&beneficiary).and_then(|m| m.last_burn_scan_tx_id), Some(77));
        });
    }

    #[test]
    fn burn_indexing_counts_burn_once_even_when_transfer_precedes_it() {
        let _staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.canister_sources.insert(
                beneficiary,
                crate::logic::merge_sources(None, CanisterSource::MemoContribution),
            );
            st.distinct_canisters.insert(beneficiary);
        });
        let cmc_account_id = account_identifier_text(&crate::logic::cmc_deposit_account(principal("rkp4c-7iaaa-aaaaa-aaaca-cai"), beneficiary));
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![],
                oldest_tx_id: None,
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![
                    transfer_to_account_tx(77, &cmc_account_id, 150, 123_000_000_000),
                    burn_tx(78, 150, 124_000_000_000),
                ],
                oldest_tx_id: Some(77),
            },
        ]);

        block_on(process_burn_indexing(&mock)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.icp_burned_e8s, Some(150));
            assert_eq!(st.per_canister_meta.get(&beneficiary).map(|m| m.burned_e8s), Some(150));
            assert_eq!(st.per_canister_meta.get(&beneficiary).and_then(|m| m.last_burn_tx_id), Some(78));
            assert_eq!(st.per_canister_meta.get(&beneficiary).and_then(|m| m.last_burn_scan_tx_id), Some(78));
            assert_eq!(st.recent_burns.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_burns.as_ref().unwrap()[0].tx_id, 78);
        });
    }

    #[test]
    fn burn_indexing_uses_scan_cursor_without_promoting_non_burns_to_last_burn() {
        let _staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.canister_sources.insert(
                beneficiary,
                crate::logic::merge_sources(None, CanisterSource::MemoContribution),
            );
            st.distinct_canisters.insert(beneficiary);
        });
        let cmc_account_id = account_identifier_text(&crate::logic::cmc_deposit_account(principal("rkp4c-7iaaa-aaaaa-aaaca-cai"), beneficiary));
        let mock = MockIndexClient::new(vec![
            // first run: faucet target
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![],
                oldest_tx_id: None,
            },
            // first run: beneficiary target sees a transfer into the CMC deposit account
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![transfer_to_account_tx(77, &cmc_account_id, 150, 123_000_000_000)],
                oldest_tx_id: Some(77),
            },
            // second run: faucet target
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![],
                oldest_tx_id: None,
            },
            // second run: beneficiary target resumes from the scan cursor and sees the burn
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![burn_tx(78, 150, 124_000_000_000)],
                oldest_tx_id: Some(77),
            },
        ]);

        block_on(process_burn_indexing(&mock)).unwrap();
        block_on(process_burn_indexing(&mock)).unwrap();

        let calls = mock.calls();
        let beneficiary_starts: Vec<_> = calls
            .iter()
            .filter(|(account_id, _, _)| account_id == &cmc_account_id)
            .map(|(_, start, _)| *start)
            .collect();
        assert_eq!(beneficiary_starts, vec![None, Some(77)], "beneficiary scans should resume from the last scanned tx without treating it as the last actual burn");

        state::with_state(|st| {
            let meta = st.per_canister_meta.get(&beneficiary).expect("beneficiary meta should exist");
            assert_eq!(meta.last_burn_tx_id, Some(78));
            assert_eq!(meta.last_burn_scan_tx_id, Some(78));
            assert_eq!(meta.burned_e8s, 150);
            assert_eq!(st.icp_burned_e8s, Some(150));
        });
    }

    #[test]
    fn contribution_indexing_does_not_inflate_burned_totals() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000)],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_contribution_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_contribution_count, Some(1));
            assert_eq!(st.icp_burned_e8s, Some(0));
        });
    }

}
