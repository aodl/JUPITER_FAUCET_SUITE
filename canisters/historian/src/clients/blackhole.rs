use async_trait::async_trait;
use candid::{CandidType, Nat, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

use crate::clients::{BlackholeClient, ClientError};

#[derive(Clone, Debug, CandidType, Deserialize)]
pub(crate) struct BlackholeCanisterStatusArgs {
    pub canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub(crate) struct BlackholeSettings {
    pub controllers: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub(crate) struct BlackholeMemoryMetrics {
    pub wasm_memory_size: Nat,
    pub stable_memory_size: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub(crate) struct BlackholeCanisterStatus {
    pub cycles: Nat,
    pub settings: BlackholeSettings,
    pub memory_size: Option<Nat>,
    pub memory_metrics: Option<BlackholeMemoryMetrics>,
}

pub(crate) struct BlackholeCanister {
    canister_id: Principal,
}
impl BlackholeCanister {
    pub(crate) fn new(canister_id: Principal) -> Self {
        Self { canister_id }
    }
}

#[async_trait]
impl BlackholeClient for BlackholeCanister {
    async fn canister_status(
        &self,
        canister_id: Principal,
    ) -> Result<BlackholeCanisterStatus, ClientError> {
        let resp = Call::bounded_wait(self.canister_id, "canister_status")
            .with_arg(BlackholeCanisterStatusArgs { canister_id })
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        resp.candid()
            .map_err(|e| ClientError::Call(format!("decode canister_status failed: {e:?}")))
    }
}
