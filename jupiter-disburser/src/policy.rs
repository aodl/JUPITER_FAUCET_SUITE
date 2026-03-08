use candid::Principal;

pub const SECS_PER_DAY: u64 = 86_400;
pub const HEALTHY_WINDOW_SECS: u64 = 7 * SECS_PER_DAY;
pub const BROKEN_WINDOW_SECS: u64 = 14 * SECS_PER_DAY;

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
        // age = 7d exactly => still healthy
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
        // age = 10d => middle window
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
    
        // exactly 7d -> healthy -> self-only
        let last = now - HEALTHY_WINDOW_SECS;
        assert_eq!(
            desired_controllers(now, Some(last), self_id, rescue),
            Some(vec![self_id])
        );
    
        // 7d + 1s -> middle window -> no action
        let last = now - (HEALTHY_WINDOW_SECS + 1);
        assert_eq!(desired_controllers(now, Some(last), self_id, rescue), None);
    
        // exactly 14d -> still middle window -> no action
        let last = now - BROKEN_WINDOW_SECS;
        assert_eq!(desired_controllers(now, Some(last), self_id, rescue), None);
    
        // 14d + 1s -> broken -> rescue+self
        let last = now - (BROKEN_WINDOW_SECS + 1);
        assert_eq!(
            desired_controllers(now, Some(last), self_id, rescue),
            Some(vec![rescue, self_id])
        );
    }
}

