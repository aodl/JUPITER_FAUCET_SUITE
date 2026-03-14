use candid::Principal;

pub const SECS_PER_DAY: u64 = 86_400;
pub const HEALTHY_WINDOW_SECS: u64 = 7 * SECS_PER_DAY;
pub const BROKEN_WINDOW_SECS: u64 = 14 * SECS_PER_DAY;
pub const BOOTSTRAP_RESCUE_WINDOW_SECS: u64 = 14 * SECS_PER_DAY;

/// Returns the controller set implied by elapsed time since the last successful transfer.
///
/// Returns:
/// - Some([self]) if healthy (<= 7 days)
/// - Some([rescue, self]) if broken (> 14 days)
/// - None if in the middle window (7d, 14d] or not armed (no successful transfer yet)
pub fn desired_controllers(
    now_secs: u64,
    last_successful_transfer_ts: Option<u64>,
    self_id: Principal,
    rescue_controller: Principal,
) -> Option<Vec<Principal>> {
    let last = last_successful_transfer_ts?;
    let age = now_secs.saturating_sub(last);

    if age <= HEALTHY_WINDOW_SECS {
        Some(vec![self_id])
    } else if age > BROKEN_WINDOW_SECS {
        Some(vec![rescue_controller, self_id])
    } else {
        None
    }
}

pub fn bootstrap_rescue_due(
    now_secs: u64,
    blackhole_armed_since_ts: Option<u64>,
    last_successful_transfer_ts: Option<u64>,
) -> bool {
    last_successful_transfer_ts.is_none()
        && blackhole_armed_since_ts
            .map(|armed_at| now_secs.saturating_sub(armed_at) > BOOTSTRAP_RESCUE_WINDOW_SECS)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Principal;

    fn self_id() -> Principal {
        Principal::management_canister()
    }

    fn rescue_id() -> Principal {
        Principal::anonymous()
    }

    #[test]
    fn not_armed_without_successful_transfer() {
        assert_eq!(
            desired_controllers(100, None, self_id(), rescue_id()),
            None
        );
    }

    #[test]
    fn healthy_means_self_only() {
        let now = 2_000_000u64;
        let last = now - HEALTHY_WINDOW_SECS;
        assert_eq!(
            desired_controllers(now, Some(last), self_id(), rescue_id()),
            Some(vec![self_id()])
        );
    }

    #[test]
    fn middle_window_means_no_action() {
        let now = 2_000_000u64;
        let last = now - 10 * SECS_PER_DAY;
        assert_eq!(
            desired_controllers(now, Some(last), self_id(), rescue_id()),
            None
        );
    }

    #[test]
    fn broken_means_rescue_plus_self() {
        let now = 2_000_000u64;
        let last = now - (BROKEN_WINDOW_SECS + 1);
        assert_eq!(
            desired_controllers(now, Some(last), self_id(), rescue_id()),
            Some(vec![rescue_id(), self_id()])
        );
    }

    #[test]
    fn desired_controllers_boundary_conditions() {
        let self_id = self_id();
        let rescue = rescue_id();
        let now = 2_000_000u64;

        let last = now - HEALTHY_WINDOW_SECS;
        assert_eq!(desired_controllers(now, Some(last), self_id, rescue), Some(vec![self_id]));

        let last = now - (HEALTHY_WINDOW_SECS + 1);
        assert_eq!(desired_controllers(now, Some(last), self_id, rescue), None);

        let last = now - BROKEN_WINDOW_SECS;
        assert_eq!(desired_controllers(now, Some(last), self_id, rescue), None);

        let last = now - (BROKEN_WINDOW_SECS + 1);
        assert_eq!(desired_controllers(now, Some(last), self_id, rescue), Some(vec![rescue, self_id]));
    }

    #[test]
    fn bootstrap_rescue_requires_elapsed_time_and_no_success() {
        assert!(!bootstrap_rescue_due(100, Some(100), None));
        assert!(bootstrap_rescue_due(100 + BOOTSTRAP_RESCUE_WINDOW_SECS + 1, Some(100), None));
        assert!(!bootstrap_rescue_due(100 + BOOTSTRAP_RESCUE_WINDOW_SECS + 1, Some(100), Some(123)));
    }
}
