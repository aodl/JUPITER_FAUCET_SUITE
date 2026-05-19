use async_trait::async_trait;
use candid::{CandidType, Nat, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

use crate::clients::{nat_to_u128, BlackholeClient, ClientError};

#[derive(Clone, Debug, CandidType, Deserialize)]
struct BlackholeCanisterStatusArgs {
    canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct BlackholeCanisterStatus {
    cycles: Nat,
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
    async fn cycles_balance(&self, canister_id: Principal) -> Result<u128, ClientError> {
        let resp = Call::bounded_wait(self.canister_id, "canister_status")
            .with_arg(BlackholeCanisterStatusArgs { canister_id })
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("blackhole canister_status failed: {e:?}")))?;
        let status: BlackholeCanisterStatus = resp.candid().map_err(|e| {
            ClientError::Call(format!("decode blackhole canister_status failed: {e:?}"))
        })?;
        nat_to_u128(&status.cycles)
    }
}
