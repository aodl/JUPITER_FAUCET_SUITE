use async_trait::async_trait;
use candid::Principal;
use ic_cdk::management_canister::{canister_info, CanisterInfoArgs};

use crate::clients::{CanisterStatusClient, ClientError};

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
            Err(err) => Err(ClientError::Call(format!("canister_info failed: {err:?}"))),
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
