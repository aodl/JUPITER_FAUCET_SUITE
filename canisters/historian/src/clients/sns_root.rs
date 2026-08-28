use async_trait::async_trait;
use candid::Principal;
use jupiter_ic_clients::sns::ListSnsCanistersResponse;

use crate::clients::{ClientError, SnsRootClient};

pub(crate) struct SnsRootCanister;

#[async_trait]
impl SnsRootClient for SnsRootCanister {
    async fn list_sns_canisters(
        &self,
        root_id: Principal,
    ) -> Result<ListSnsCanistersResponse, ClientError> {
        Ok(jupiter_ic_clients::sns::SnsRootCanister
            .list_sns_canisters(root_id)
            .await?)
    }
}
