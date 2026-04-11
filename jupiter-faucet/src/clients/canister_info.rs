use async_trait::async_trait;
use candid::Principal;
use ic_cdk::call::{CallRejected, Error as CallError, RejectCode};
use ic_cdk::management_canister::{canister_info, CanisterInfoArgs};

use crate::clients::{CanisterStatusClient, ClientError};

fn definitely_not_a_canister(err: &CallError) -> bool {
    matches!(err, CallError::CallRejected(rejected) if is_destination_invalid(rejected))
}

fn is_destination_invalid(rejected: &CallRejected) -> bool {
    rejected
        .reject_code()
        .map(|code| code == RejectCode::DestinationInvalid)
        .unwrap_or_else(|_| rejected.raw_reject_code() == RejectCode::DestinationInvalid as u32)
}

pub struct ManagementCanisterInfoClient;

#[async_trait]
impl CanisterStatusClient for ManagementCanisterInfoClient {
    async fn canister_exists(&self, canister_id: Principal) -> Result<bool, ClientError> {
        let request = CanisterInfoArgs {
            canister_id,
            num_requested_changes: Some(0),
        };

        match canister_info(&request).await {
            Ok(_) => Ok(true),
            Err(err) => {
                if definitely_not_a_canister(&err) {
                    Ok(false)
                } else {
                    Err(ClientError::Call(format!("canister_info failed: {err:?}")))
                }
            }
        }
    }
}

#[cfg(test)]
pub struct NoopCanisterStatusClient;

#[cfg(test)]
#[async_trait]
impl CanisterStatusClient for NoopCanisterStatusClient {
    async fn canister_exists(&self, _canister_id: Principal) -> Result<bool, ClientError> {
        Ok(false)
    }
}


#[cfg(test)]
mod tests {
    use super::{definitely_not_a_canister, is_destination_invalid};
    use ic_cdk::call::{CallRejected, Error as CallError, RejectCode};

    #[test]
    fn recognizes_destination_invalid_rejects_as_definitely_not_a_canister() {
        let err = CallError::CallRejected(CallRejected::with_rejection(
            RejectCode::DestinationInvalid as u32,
            "principal does not characterize a canister".into(),
        ));
        assert!(definitely_not_a_canister(&err));
    }

    #[test]
    fn does_not_treat_other_reject_codes_as_definitive() {
        let err = CallError::CallRejected(CallRejected::with_rejection(
            RejectCode::SysTransient as u32,
            "transient routing error".into(),
        ));
        assert!(!definitely_not_a_canister(&err));
    }

    #[test]
    fn falls_back_to_raw_reject_code_for_unrecognized_reject_values() {
        let rejected = CallRejected::with_rejection(999, "future reject code".into());
        assert!(!is_destination_invalid(&rejected));
    }
}

