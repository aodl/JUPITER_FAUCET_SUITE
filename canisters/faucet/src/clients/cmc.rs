use async_trait::async_trait;
use candid::Principal;
use jupiter_ic_clients::cmc::NotifyTopUpError;

use crate::clients::CmcClient;

pub(crate) struct CyclesMintingCanister {
    cmc_id: Principal,
}

impl CyclesMintingCanister {
    pub(crate) fn new(cmc_id: Principal) -> Self {
        Self { cmc_id }
    }
}

#[async_trait]
impl CmcClient for CyclesMintingCanister {
    async fn notify_top_up(
        &self,
        canister_id: Principal,
        block_index: u64,
    ) -> Result<u128, NotifyTopUpError> {
        jupiter_ic_clients::cmc::notify_top_up(self.cmc_id, canister_id, block_index).await
    }
}
