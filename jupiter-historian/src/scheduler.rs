use candid::Nat;
use std::time::Duration;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::index::{account_identifier_text, IcpIndexCanister, IndexOperation};
use crate::clients::sns_root::{SnsCanisterSummary, SnsRootCanister};
use crate::clients::sns_wasm::SnsWasmCanister;
use crate::clients::{BlackholeClient, IndexClient, SnsRootClient, SnsWasmClient};
use crate::{
    logic, mainnet_cmc_id, MAX_RECENT_BURNS, MAX_RECENT_INVALID_CONTRIBUTIONS,
    MAX_RECENT_QUALIFYING_CONTRIBUTIONS, MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS,
};
use crate::state::{self, ActiveCyclesSweep, ActiveSnsDiscovery, CanisterMeta, CanisterSource, CyclesProbeResult, CyclesSampleSource, InvalidContribution, RecentBurn, RecentContribution};

const PAGE_SIZE: u64 = 500;
const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;

fn contribution_sort_key(item: &RecentContribution) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn burn_sort_key(item: &RecentBurn) -> (u64, u64) {
    (item.timestamp_nanos.unwrap_or(0), item.tx_id)
}

fn push_recent_contribution(recent: &mut Vec<RecentContribution>, item: RecentContribution, max_entries: usize) {
    if recent.iter().any(|existing| existing.tx_id == item.tx_id) {
        return;
    }
    recent.push(item);
    recent.sort_by(|a, b| contribution_sort_key(b).cmp(&contribution_sort_key(a)));
    if recent.len() > max_entries {
        recent.truncate(max_entries);
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
    process_contribution_indexing(index, now_secs).await?;
    process_burn_indexing(index).await?;

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

fn apply_verified_qualifying_contribution(
    st: &mut crate::state::State,
    contribution: crate::logic::IndexedContribution,
    now_secs: u64,
) {
    st.distinct_canisters.insert(contribution.beneficiary);
    st.canister_sources.insert(
        contribution.beneficiary,
        logic::merge_sources(st.canister_sources.get(&contribution.beneficiary), CanisterSource::MemoContribution),
    );
    let recent_item = RecentContribution {
        canister_id: contribution.beneficiary,
        tx_id: contribution.tx_id,
        timestamp_nanos: contribution.timestamp_nanos,
        amount_e8s: contribution.amount_e8s,
        counts_toward_faucet: true,
    };
    crate::state::ensure_contribution_history_loaded(st, contribution.beneficiary);
    let history = st.contribution_history.entry(contribution.beneficiary).or_default();
    let inserted = logic::push_contribution(
        history,
        crate::state::ContributionSample {
            tx_id: contribution.tx_id,
            timestamp_nanos: contribution.timestamp_nanos,
            amount_e8s: contribution.amount_e8s,
            counts_toward_faucet: true,
        },
        st.config.max_contribution_entries_per_canister,
    );
    if inserted {
        let meta = st.per_canister_meta.entry(contribution.beneficiary).or_insert_with(CanisterMeta::default);
        logic::apply_contribution_seen(meta, contribution.timestamp_nanos, now_secs);
        let recent = st.recent_contributions.get_or_insert_with(Vec::new);
        push_recent_contribution(recent, recent_item, MAX_RECENT_QUALIFYING_CONTRIBUTIONS);
        let count = st.qualifying_contribution_count.get_or_insert(0);
        *count = count.saturating_add(1);
    }
    if inserted || st.canister_sources.contains_key(&contribution.beneficiary) {
        crate::refresh_registered_canister_summary(st, contribution.beneficiary);
    }
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
        {
            let _batch = state::begin_persistence_batch();
            for tx in page.transactions.iter() {
                if let Some(contribution) = logic::indexed_contribution_from_tx(tx, &staking_id, cfg.min_tx_e8s) {
                    match contribution {
                        logic::IndexedContributionEntry::Valid(contribution) => {
                            if contribution.counts_toward_faucet {
                                {
                                    let dirty_beneficiary = contribution.beneficiary.clone();
                                    state::with_root_registry_and_contributions_canister_state_mut(dirty_beneficiary, |st| {
                                        apply_verified_qualifying_contribution(st, contribution, now_secs);
                                    });
                                }
                            } else {
                                state::with_root_state_mut(|st| {
                                    let recent = st.recent_under_threshold_contributions.get_or_insert_with(Vec::new);
                                    push_recent_contribution(
                                        recent,
                                        RecentContribution {
                                            canister_id: contribution.beneficiary,
                                            tx_id: contribution.tx_id,
                                            timestamp_nanos: contribution.timestamp_nanos,
                                            amount_e8s: contribution.amount_e8s,
                                            counts_toward_faucet: false,
                                        },
                                        MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS,
                                    );
                                });
                            }
                        }
                        logic::IndexedContributionEntry::Invalid(contribution) => {
                            state::with_root_state_mut(|st| {
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
                            });
                        }
                    }
                }
                cursor = Some(tx.id);
                // Historian dedupe relies on this cursor remaining monotonic in normal operation. The
                // retained per-canister history only protects against duplicate delivery within the
                // retained window; older tx_ids are considered already indexed once the cursor passes them.
                state::with_root_state_mut(|st| st.last_indexed_staking_tx_id = cursor);
            }
        }
        if page.transactions.len() < PAGE_SIZE as usize {
            break;
        }
    }
    state::with_root_state_mut(|st| st.last_index_run_ts = Some(now_secs));
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
                    // Burn indexing uses the same monotonic/exclusive cursor assumption as the main
                    // staking-account scan. Temporary page-boundary duplication is treated as an
                    // upstream index inconsistency rather than something we buffer additional state for.
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

            {
                let _batch = state::begin_persistence_batch();
                let dirty_canister_id = canister_id.clone();
                state::with_root_and_registry_canister_state_mut(dirty_canister_id, |st| {
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
            }

            cursor = last_seen;
            if page.transactions.len() < PAGE_SIZE as usize {
                break;
            }
        }
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
                let memo_registered = sources.contains(&CanisterSource::MemoContribution)
                    && st
                        .contribution_history
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

    fn seed_qualifying_memo_registration(beneficiary: candid::Principal) {
        state::with_state_mut(|st| {
            st.canister_sources.insert(
                beneficiary,
                crate::logic::merge_sources(st.canister_sources.get(&beneficiary), CanisterSource::MemoContribution),
            );
            st.distinct_canisters.insert(beneficiary);
            st.contribution_history.insert(
                beneficiary,
                vec![crate::state::ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(100_000_000_000),
                    amount_e8s: st.config.min_tx_e8s,
                    counts_toward_faucet: true,
                }],
            );
            st.qualifying_contribution_count = Some(1);
        });
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

        block_on(process_contribution_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_contribution_count, Some(1));
            assert_eq!(st.recent_contributions.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(
                st.recent_under_threshold_contributions
                    .as_ref()
                    .map(|items| items.len()),
                Some(1),
            );
            assert_eq!(st.recent_invalid_contributions.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_contributions.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.recent_under_threshold_contributions.as_ref().unwrap()[0].tx_id, 43);
            assert!(!st.canister_sources.contains_key(&low_amount));
            assert!(!st.distinct_canisters.contains(&low_amount));
            assert!(!st.contribution_history.contains_key(&low_amount));
            let invalid = &st.recent_invalid_contributions.as_ref().unwrap()[0];
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

        block_on(process_contribution_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            let recent = st
                .recent_under_threshold_contributions
                .as_ref()
                .expect("under-threshold recent list should exist");
            assert_eq!(recent.len(), MAX_RECENT_UNDER_THRESHOLD_CONTRIBUTIONS);
            assert_eq!(recent[0].tx_id, 105);
            assert_eq!(recent.last().map(|item| item.tx_id), Some(6));
            assert_eq!(st.recent_contributions.as_ref().map(|items| items.len()), Some(0));
            assert_eq!(st.canister_sources.len(), 0);
            assert_eq!(st.distinct_canisters.len(), 0);
            assert!(st.contribution_history.is_empty());
            assert_eq!(st.qualifying_contribution_count, Some(0));
        });
    }

    #[test]
    fn indexing_registers_new_qualifying_canisters_without_pruning_existing_beneficiaries() {
        let staking_id = configure_state(10);
        let existing = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(existing);
            st.canister_sources
                .insert(existing, crate::logic::merge_sources(None, CanisterSource::MemoContribution));
            st.contribution_history.insert(
                existing,
                vec![crate::state::ContributionSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                }],
            );
            st.qualifying_contribution_count = Some(1);
        });
        let new_canister = candid::Principal::from_slice(&[251, 251, 251]);
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(9_999, &staking_id, new_canister, 150, 123_000_000_000)],
            oldest_tx_id: Some(9_999),
        }]);

        block_on(process_contribution_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_contribution_count, Some(2));
            assert_eq!(st.recent_contributions.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_contributions.as_ref().unwrap()[0].tx_id, 9_999);
            assert!(st.canister_sources.contains_key(&new_canister));
            assert!(st.contribution_history.contains_key(&new_canister));
            assert!(st.distinct_canisters.contains(&new_canister));
            assert!(st.distinct_canisters.contains(&existing));
        });
    }

    #[test]
    fn burn_indexing_counts_only_actual_burn_entries() {
        let _staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        seed_qualifying_memo_registration(beneficiary);
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
        seed_qualifying_memo_registration(beneficiary);
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
        seed_qualifying_memo_registration(beneficiary);
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
