use super::*;
// Keeping the inputs separate makes the controller policy tests table-driven and avoids state coupling.
#[allow(clippy::too_many_arguments)]
pub(super) fn desired_rescue_controllers(
    now_secs: u64,
    autonomous_rescue_armed: bool,
    last_xfer_opt: Option<u64>,
    rescue_controller: Principal,
    forced_reason_present: bool,
    skip_range_fault: bool,
    self_id: Principal,
) -> Option<Vec<Principal>> {
    if !autonomous_rescue_armed {
        return None;
    }
    let mut desired = if forced_reason_present || skip_range_fault {
        vec![rescue_controller, self_id]
    } else {
        let Some(desired) =
            policy::desired_controllers(now_secs, last_xfer_opt, self_id, rescue_controller)
        else {
            return None;
        };
        desired
    };
    desired.sort_by_key(|a: &Principal| a.to_text());
    desired.dedup();
    Some(desired)
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

pub(super) async fn attempt_rescue(now_secs: u64) {
    maybe_latch_bootstrap_rescue(now_secs);
    let (
        autonomous_rescue_armed,
        last_xfer_opt,
        rescue_controller,
        forced_reason,
        skip_range_fault,
    ) = state::with_state(|st| {
        (
            st.config.autonomous_rescue_armed.unwrap_or(false),
            st.last_successful_transfer_ts,
            st.config.rescue_controller,
            st.forced_rescue_reason.clone(),
            st.skip_range_invariant_fault.unwrap_or(false),
        )
    });
    let self_id = self_canister_principal();
    let desired_opt = desired_rescue_controllers(
        now_secs,
        autonomous_rescue_armed,
        last_xfer_opt,
        rescue_controller,
        forced_reason.is_some(),
        skip_range_fault,
        self_id,
    );
    let Some(desired) = desired_opt else {
        return;
    };
    let rescue_active = desired.contains(&rescue_controller);
    let arg = controller_update_settings(self_id, desired);
    if update_settings(&arg).await.is_err() {
        log_error(3101);
        return;
    }
    state::with_state_mut(|st| {
        st.last_rescue_check_ts = now_secs;
        st.rescue_triggered = rescue_active;
    });
}

pub(super) async fn rescue_tick() {
    let now_secs = ic_cdk::api::time() / 1_000_000_000;
    rescue_tick_with_resume_at(now_secs, || async {
        main_tick(true).await;
    })
    .await;
}

pub(super) async fn rescue_tick_with_resume_at<F, Fut>(now_secs: u64, resume_active_job: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Preserve the rescue/controller-reconciliation ordering first, then use the
    // same daily cadence as a bounded fallback resume opportunity for any
    // unfinished payout job that remains persisted.
    attempt_rescue(now_secs).await;
    resume_active_job_if_present(resume_active_job).await;
}

pub(super) async fn resume_active_job_if_present<F, Fut>(resume_active_job: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let has_active_job = state::with_state(|st| st.active_payout_job.is_some());
    if has_active_job {
        resume_active_job().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomous_controller_settings_preserve_public_observability() {
        let self_id = Principal::management_canister();
        let lifeline_id = Principal::anonymous();

        for controllers in [vec![self_id], vec![self_id, lifeline_id]] {
            let arg = controller_update_settings(self_id, controllers.clone());
            assert_eq!(arg.canister_id, self_id);
            assert_eq!(arg.settings.controllers, Some(controllers));
            assert_eq!(
                arg.settings.status_visibility,
                Some(StatusVisibility::Public)
            );
            assert_eq!(arg.settings.log_visibility, Some(LogVisibility::Public));
        }
    }
}
