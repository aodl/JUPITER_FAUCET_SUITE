use async_trait::async_trait;
use candid::Principal;

use crate::clients::{ClientError, GovernanceClient};

pub(crate) struct NnsGovernanceCanister {
    inner: jupiter_ic_clients::governance::NnsGovernanceCanister,
}

impl NnsGovernanceCanister {
    pub(crate) fn new(governance_id: Principal) -> Self {
        Self {
            inner: jupiter_ic_clients::governance::NnsGovernanceCanister::new(governance_id),
        }
    }
}

#[async_trait]
impl GovernanceClient for NnsGovernanceCanister {
    async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], ClientError> {
        Ok(self.inner.neuron_staking_subaccount(neuron_id).await?)
    }

    async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), ClientError> {
        Ok(self.inner.claim_or_refresh_neuron(neuron_id).await?)
    }
}
