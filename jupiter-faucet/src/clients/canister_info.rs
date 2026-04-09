use async_trait::async_trait;
use candid::Principal;
use ic_cdk::management_canister::{canister_info, CanisterInfoArgs};

use crate::clients::{CanisterStatusClient, ClientError};

fn definitely_not_a_canister(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("does not characterize a canister")
        || lower.contains("not characterize a canister")
        || lower.contains("canister not found")
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
                let message = format!("canister_info failed: {err:?}");
                if definitely_not_a_canister(&message) {
                    Ok(false)
                } else {
                    Err(ClientError::Call(message))
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
    use super::definitely_not_a_canister;

    #[test]
    fn recognizes_documented_not_a_canister_wording() {
        assert!(definitely_not_a_canister("Reject text: principal does not characterize a canister"));
        assert!(definitely_not_a_canister("reject: canister not found"));
    }

    #[test]
    fn does_not_treat_generic_not_found_as_definitive() {
        assert!(!definitely_not_a_canister("transient routing error: subnet not found"));
        assert!(!definitely_not_a_canister("some unrelated not found response"));
    }
}
