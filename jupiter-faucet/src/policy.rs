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
    fn none_before_first_success() {
        assert_eq!(desired_controllers(100, None, self_id(), rescue_id()), None);
    }

    #[test]
    fn self_only_when_healthy() {
        let now = 100 + HEALTHY_WINDOW_SECS;
        let got = desired_controllers(now, Some(100), self_id(), rescue_id()).unwrap();
        assert_eq!(got, vec![self_id()]);
    }

    #[test]
    fn rescue_added_when_broken() {
        let now = 100 + BROKEN_WINDOW_SECS + 1;
        let got = desired_controllers(now, Some(100), self_id(), rescue_id()).unwrap();
        assert_eq!(got, vec![rescue_id(), self_id()]);
    }

    #[test]
    fn gray_window_returns_none() {
        let now = 100 + HEALTHY_WINDOW_SECS + 1;
        assert_eq!(desired_controllers(now, Some(100), self_id(), rescue_id()), None);
    }

    #[test]
    fn healthy_boundary_is_self_only() {
        let now = 100 + HEALTHY_WINDOW_SECS;
        let got = desired_controllers(now, Some(100), self_id(), rescue_id()).unwrap();
        assert_eq!(got, vec![self_id()]);
    }

    #[test]
    fn broken_boundary_requires_strictly_more_than_broken_window() {
        let now = 100 + BROKEN_WINDOW_SECS;
        assert_eq!(desired_controllers(now, Some(100), self_id(), rescue_id()), None);
    }

    #[test]
    fn bootstrap_rescue_requires_elapsed_time_and_no_success() {
        assert!(!bootstrap_rescue_due(100, Some(100), None));
        assert!(bootstrap_rescue_due(
            100 + BOOTSTRAP_RESCUE_WINDOW_SECS + 1,
            Some(100),
            None
        ));
        assert!(!bootstrap_rescue_due(
            100 + BOOTSTRAP_RESCUE_WINDOW_SECS + 1,
            Some(100),
            Some(150)
        ));
    }
}
