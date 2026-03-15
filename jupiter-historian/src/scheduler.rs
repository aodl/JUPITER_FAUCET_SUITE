use candid::Nat;
use std::time::Duration;

use crate::clients::blackhole::BlackholeCanister;
use crate::clients::index::{account_identifier_text, IcpIndexCanister};
use crate::clients::sns_root::{SnsCanisterSummary, SnsRootCanister};
use crate::clients::sns_wasm::SnsWasmCanister;
use crate::clients::{BlackholeClient, IndexClient, SnsRootClient, SnsWasmClient};
use crate::logic;
use crate::state::{self, ActiveCyclesSweep, CanisterMeta, CanisterSource, CyclesProbeResult, CyclesSampleSource};

const PAGE_SIZE: u64 = 500;

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
                state::with_state_mut(|st| {
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
