use super::*;
/// RESCUE TICK:
/// - errors-only logs
/// - policy-driven decision:
///   * healthy => controllers=[blackhole,self]
///   * broken  => controllers=[blackhole,rescue,self]
///
/// This path is intentionally driven by persisted local state plus a management-canister
/// controller update. It does not require fresh ledger, governance, or canister-status
/// health checks at the point of escalation.
pub(super) async fn rescue_tick() {
    let now_secs = ic_cdk::api::time() / 1_000_000_000;

    state::with_state_mut(|st| {
        if st.forced_rescue_reason.is_none()
            && policy::bootstrap_rescue_due(
                now_secs,
                st.blackhole_armed_since_ts,
                st.last_successful_transfer_ts,
            )
        {
            st.forced_rescue_reason = Some(state::ForcedRescueReason::BootstrapNoSuccess);
        }
    });

    let (
        blackhole_armed,
        blackhole_controller,
        last_xfer_opt,
        rescue_controller,
        forced_rescue_reason,
    ) = state::with_state(|st| {
        (
            st.config.blackhole_armed.unwrap_or(false),
            st.config.blackhole_controller,
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.forced_rescue_reason.clone(),
        )
    });

    if !blackhole_armed {
        return;
    }

    let Some(blackhole_controller) = blackhole_controller else {
        log_error(2003);
        return;
    };

    let self_id = self_canister_principal();
    let mut desired = if forced_rescue_reason.is_some() {
        vec![blackhole_controller, rescue_controller, self_id]
    } else {
        let Some(desired) = policy::desired_controllers(
            now_secs,
            last_xfer_opt,
            self_id,
            Some(blackhole_controller),
            rescue_controller,
        ) else {
            return;
        };
        desired
    };

    desired.sort_by_key(|a| a.to_text());
    desired.dedup();

    let rescue_active = desired.contains(&rescue_controller);

    let arg = UpdateSettingsArgs {
        canister_id: self_id,
        settings: CanisterSettings {
            controllers: Some(desired.clone()),
        },
    };

    if update_settings(&arg).await.is_err() {
        log_error(2002);
        return;
    }

    state::with_state_mut(|st| {
        st.rescue_triggered = rescue_active;
        st.last_rescue_check_ts = now_secs;
    });
}
