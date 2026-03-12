use candid::Principal;

pub const SECS_PER_DAY: u64 = 86_400;
pub const HEALTHY_WINDOW_SECS: u64 = 7 * SECS_PER_DAY;
pub const BROKEN_WINDOW_SECS: u64 = 14 * SECS_PER_DAY;

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
        let got = desired_controllers(100, Some(100), self_id(), rescue_id()).unwrap();
        assert_eq!(got, vec![self_id()]);
    }

    #[test]
    fn rescue_added_when_broken() {
        let now = 100 + BROKEN_WINDOW_SECS + 1;
        let got = desired_controllers(now, Some(100), self_id(), rescue_id()).unwrap();
        assert_eq!(got, vec![rescue_id(), self_id()]);
    }
}
