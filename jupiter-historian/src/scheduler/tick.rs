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

    let (index_id, blackhole_id, sns_wasm_id, governance_id, xrc_id) = state::with_state(|st| (
        st.config.index_canister_id,
        st.config.blackhole_canister_id,
        st.config.sns_wasm_canister_id,
        st.config.staking_account.owner,
        st.config.xrc_canister_id,
    ));
    let index = IcpIndexCanister::new(index_id);
    let original_blackhole = BlackholeCanister::new(original_blackhole_id());
    let configured_blackhole = BlackholeCanister::new(blackhole_id);
    let sns_wasm = SnsWasmCanister::new(sns_wasm_id);
    let sns_root = SnsRootCanister;
    let governance = NnsGovernanceCanister::new(governance_id);
    let xrc = XrcCanister::with_canister_id(xrc_id);
    let result = if should_try_original_blackhole_first(blackhole_id) {
        let blackhole = FallbackBlackholeClient::new(&original_blackhole, &configured_blackhole);
        run_main_tick_with_clients(
            now_nanos,
            now_secs,
            &index,
            &blackhole,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
        )
        .await
    } else if blackhole_id == original_blackhole_id() {
        run_main_tick_with_clients(
            now_nanos,
            now_secs,
            &index,
            &original_blackhole,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
        )
        .await
    } else {
        run_main_tick_with_clients(
            now_nanos,
            now_secs,
            &index,
            &configured_blackhole,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
        )
        .await
    };
    if let Err(err) = result {
        log_error(&format!("historian main tick failed: {err}"));
        guard.finish(now_secs);
        return;
    }
    guard.finish(now_secs);
    state::persist_dirty_state();
    state::clear_loaded_history_caches_after_flush();
}

async fn refresh_icp_xdr_rate<X: ExchangeRateClient>(now_secs: u64, xrc: &X) -> Result<(), String> {
    state::with_root_state_mut(|st| st.last_icp_xdr_rate_attempt_ts = Some(now_secs));
    match xrc.get_icp_xdr_rate().await {
        Ok(rate) => {
            state::with_root_state_mut(|st| {
                st.icp_xdr_rate = Some(state::IcpXdrRateSnapshot {
                    rate: rate.rate,
                    decimals: rate.decimals,
                    timestamp: rate.timestamp,
                    fetched_at_ts: now_secs,
                });
                st.last_icp_xdr_rate_attempt_ts = Some(now_secs);
                st.last_icp_xdr_rate_error = None;
            });
            Ok(())
        }
        Err(err) => {
            let message = err.to_string();
            state::with_root_state_mut(|st| {
                st.last_icp_xdr_rate_attempt_ts = Some(now_secs);
                st.last_icp_xdr_rate_error = Some(message.clone());
            });
            Err(message)
        }
    }
}

async fn refresh_icp_xdr_rate_if_due<X: ExchangeRateClient>(now_secs: u64, xrc: &X) -> Result<(), String> {
    let due = state::with_state(|st| {
        if let Some(last_attempt_ts) = st.last_icp_xdr_rate_attempt_ts {
            return now_secs.saturating_sub(last_attempt_ts) >= ICP_XDR_RATE_CACHE_TTL_SECONDS;
        }
        st.icp_xdr_rate
            .as_ref()
            .map(|snapshot| now_secs.saturating_sub(snapshot.fetched_at_ts) >= ICP_XDR_RATE_CACHE_TTL_SECONDS)
            .unwrap_or(true)
    });
    if !due {
        return Ok(());
    }

    refresh_icp_xdr_rate(now_secs, xrc).await
}

#[cfg(feature = "debug_api")]
pub async fn debug_refresh_icp_xdr_rate_now(now_secs: u64, xrc_canister_id: Principal) -> Result<(), String> {
    let xrc = XrcCanister::with_canister_id(xrc_canister_id);
    refresh_icp_xdr_rate(now_secs, &xrc).await
}

async fn run_main_tick_with_clients<I: IndexClient, B: BlackholeClient, W: SnsWasmClient, R: SnsRootClient, G: GovernanceClient, X: ExchangeRateClient>(
    now_nanos: u64,
    now_secs: u64,
    index: &I,
    blackhole: &B,
    sns_wasm: &W,
    sns_root: &R,
    governance: &G,
    xrc: &X,
) -> Result<(), String> {
    if let Err(err) = refresh_icp_xdr_rate_if_due(now_secs, xrc).await {
        log_error(&format!("historian ICP/XDR rate refresh degraded: {err}"));
    }
    if let Err(err) = process_commitment_indexing(index, now_secs).await {
        log_error(&format!("historian commitment indexing degraded: {err}"));
    }
    process_route_indexing(now_nanos, now_secs, index).await?;

    let (
        enable_sns_tracking,
        last_sns_discovery_ts,
        last_completed_cycles_sweep_ts,
        active_cycles_present,
        initial_cycles_probe_queue_present,
        active_sns_present,
        interval_secs,
    ) = state::with_state(|st| (
        st.config.enable_sns_tracking,
        st.last_sns_discovery_ts,
        st.last_completed_cycles_sweep_ts,
        st.active_cycles_sweep.is_some(),
        !st.initial_cycles_probe_queue.is_empty(),
        st.active_sns_discovery.is_some(),
        st.config.cycles_interval_seconds,
    ));

    let sns_due = enable_sns_tracking && (active_sns_present || now_secs.saturating_sub(last_sns_discovery_ts) >= interval_secs);
    if sns_due {
        process_sns_discovery(now_nanos, now_secs, sns_wasm, sns_root).await?;
    }

    if initial_cycles_probe_queue_present {
        process_initial_cycles_probe_queue(now_nanos, now_secs, blackhole, governance).await?;
    }

    let cycles_due = active_cycles_present || now_secs.saturating_sub(last_completed_cycles_sweep_ts) >= interval_secs;
    if cycles_due {
        process_cycles_sweep(now_nanos, now_secs, blackhole).await?;
    }

    Ok(())
}

