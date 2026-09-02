use super::*;
/// RESCUE TICK:
/// - errors-only logs
/// - policy-driven decision:
///   * healthy => the controller set contains only self
///   * broken  => the controller set contains exactly self and the rescue controller
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
                st.autonomous_rescue_armed_since_ts,
                st.last_successful_transfer_ts,
            )
        {
            st.forced_rescue_reason = Some(state::ForcedRescueReason::BootstrapNoSuccess);
        }
    });

    let (
        autonomous_rescue_armed,
        last_xfer_opt,
        rescue_controller,
        forced_rescue_reason,
        rescue_triggered,
    ) = state::with_state(|st| {
        (
            st.config.autonomous_rescue_armed.unwrap_or(false),
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.forced_rescue_reason.clone(),
            st.rescue_triggered,
        )
    });

    if !autonomous_rescue_armed {
        return;
    }

    let self_id = self_canister_principal();
    let Some(mut desired) = desired_controllers_for_rescue_state(
        now_secs,
        last_xfer_opt,
        self_id,
        rescue_controller,
        forced_rescue_reason.as_ref(),
        rescue_triggered,
    ) else {
        return;
    };

    desired.sort_by_key(|a| a.to_text());
    desired.dedup();

    let rescue_active = desired.contains(&rescue_controller);

    let arg = controller_update_settings(self_id, desired.clone());

    if update_settings(&arg).await.is_err() {
        log_error(2002);
        return;
    }

    state::with_state_mut(|st| {
        st.rescue_triggered = rescue_active;
        st.last_rescue_check_ts = now_secs;
    });
}

fn controller_update_settings(
    self_id: Principal,
    controllers: Vec<Principal>,
) -> UpdateSettingsArgs {
    UpdateSettingsArgs {
        canister_id: self_id,
        settings: CanisterSettings {
            controllers: Some(controllers),
            log_visibility: Some(LogVisibility::Public),
            status_visibility: Some(StatusVisibility::Public),
        },
    }
}

fn desired_controllers_for_rescue_state(
    now_secs: u64,
    last_xfer_opt: Option<u64>,
    self_id: Principal,
    rescue_controller: Principal,
    forced_rescue_reason: Option<&state::ForcedRescueReason>,
    rescue_triggered: bool,
) -> Option<Vec<Principal>> {
    if forced_rescue_reason.is_some() {
        return Some(vec![rescue_controller, self_id]);
    }
    if rescue_triggered && last_xfer_opt.is_none() {
        return Some(vec![self_id]);
    }
    policy::desired_controllers(now_secs, last_xfer_opt, self_id, rescue_controller)
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

    #[test]
    fn autonomous_controller_settings_preserve_public_observability() {
        for controllers in [vec![self_id()], vec![self_id(), rescue_id()]] {
            let arg = controller_update_settings(self_id(), controllers.clone());
            assert_eq!(arg.canister_id, self_id());
            assert_eq!(arg.settings.controllers, Some(controllers));
            assert_eq!(
                arg.settings.status_visibility,
                Some(StatusVisibility::Public)
            );
            assert_eq!(arg.settings.log_visibility, Some(LogVisibility::Public));
        }
    }

    #[test]
    fn forced_rescue_keeps_rescue_controller_desired() {
        assert_eq!(
            desired_controllers_for_rescue_state(
                1_000,
                None,
                self_id(),
                rescue_id(),
                Some(&state::ForcedRescueReason::BootstrapNoSuccess),
                false,
            ),
            Some(vec![rescue_id(), self_id()])
        );
    }

    #[test]
    fn cleared_pending_rescue_narrows_without_transfer_prerequisite() {
        assert_eq!(
            desired_controllers_for_rescue_state(1_000, None, self_id(), rescue_id(), None, true,),
            Some(vec![self_id()])
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
                rescue_id(),
                None,
                true,
            ),
            Some(vec![rescue_id(), self_id()])
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
                rescue_id(),
                None,
                true,
            ),
            Some(vec![self_id()])
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
                rescue_id(),
                None,
                false,
            ),
            Some(vec![self_id()])
        );
        assert_eq!(
            desired_controllers_for_rescue_state(now, None, self_id(), rescue_id(), None, false,),
            None
        );
    }
}
