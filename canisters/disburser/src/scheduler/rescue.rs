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
        rescue_triggered,
    ) = state::with_state(|st| {
        (
            st.config.blackhole_armed.unwrap_or(false),
            st.config.blackhole_controller,
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.forced_rescue_reason.clone(),
            st.rescue_triggered,
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
    let Some(mut desired) = desired_controllers_for_rescue_state(
        now_secs,
        last_xfer_opt,
        self_id,
        blackhole_controller,
        rescue_controller,
        forced_rescue_reason.as_ref(),
        rescue_triggered,
    ) else {
        return;
    };

    desired.sort_by_key(|a| a.to_text());
    desired.dedup();

    let rescue_active = desired.contains(&rescue_controller);

    let arg = UpdateSettingsArgs {
        canister_id: self_id,
        settings: CanisterSettings {
            controllers: Some(desired.clone()),
            log_visibility: None,
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

fn desired_controllers_for_rescue_state(
    now_secs: u64,
    last_xfer_opt: Option<u64>,
    self_id: Principal,
    blackhole_controller: Principal,
    rescue_controller: Principal,
    forced_rescue_reason: Option<&state::ForcedRescueReason>,
    rescue_triggered: bool,
) -> Option<Vec<Principal>> {
    if forced_rescue_reason.is_some() {
        return Some(vec![blackhole_controller, rescue_controller, self_id]);
    }
    if rescue_triggered && last_xfer_opt.is_none() {
        return Some(vec![blackhole_controller, self_id]);
    }
    policy::desired_controllers(
        now_secs,
        last_xfer_opt,
        self_id,
        Some(blackhole_controller),
        rescue_controller,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn self_id() -> Principal {
        Principal::management_canister()
    }

    fn rescue_id() -> Principal {
        Principal::anonymous()
    }

    fn blackhole_id() -> Principal {
        Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").unwrap()
    }

    #[test]
    fn forced_rescue_keeps_rescue_controller_desired() {
        assert_eq!(
            desired_controllers_for_rescue_state(
                1_000,
                None,
                self_id(),
                blackhole_id(),
                rescue_id(),
                Some(&state::ForcedRescueReason::BootstrapNoSuccess),
                false,
            ),
            Some(vec![blackhole_id(), rescue_id(), self_id()])
        );
    }

    #[test]
    fn cleared_pending_rescue_narrows_without_transfer_prerequisite() {
        assert_eq!(
            desired_controllers_for_rescue_state(
                1_000,
                None,
                self_id(),
                blackhole_id(),
                rescue_id(),
                None,
                true,
            ),
            Some(vec![blackhole_id(), self_id()])
        );
    }

    #[test]
    fn ordinary_broken_rescue_triggered_keeps_rescue_controller_desired() {
        let now = 2_000_000;
        assert_eq!(
            desired_controllers_for_rescue_state(
                now,
                Some(now - (15 * 86_400)),
                self_id(),
                blackhole_id(),
                rescue_id(),
                None,
                true,
            ),
            Some(vec![blackhole_id(), rescue_id(), self_id()])
        );
    }

    #[test]
    fn middle_window_rescue_triggered_returns_no_controller_change() {
        let now = 2_000_000;
        assert_eq!(
            desired_controllers_for_rescue_state(
                now,
                Some(now - (10 * 86_400)),
                self_id(),
                blackhole_id(),
                rescue_id(),
                None,
                true,
            ),
            None
        );
    }

    #[test]
    fn healthy_rescue_triggered_narrows_through_normal_policy() {
        let now = 2_000_000;
        assert_eq!(
            desired_controllers_for_rescue_state(
                now,
                Some(now - 1),
                self_id(),
                blackhole_id(),
                rescue_id(),
                None,
                true,
            ),
            Some(vec![blackhole_id(), self_id()])
        );
    }

    #[test]
    fn untriggered_rescue_uses_health_window_policy() {
        let now = 2_000_000;
        assert_eq!(
            desired_controllers_for_rescue_state(
                now,
                Some(now - 1),
                self_id(),
                blackhole_id(),
                rescue_id(),
                None,
                false,
            ),
            Some(vec![blackhole_id(), self_id()])
        );
        assert_eq!(
            desired_controllers_for_rescue_state(
                now,
                None,
                self_id(),
                blackhole_id(),
                rescue_id(),
                None,
                false,
            ),
            None
        );
    }
}
