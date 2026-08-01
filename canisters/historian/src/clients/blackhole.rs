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
    pub status: BlackholeCanisterStatusKind,
    pub module_hash: Option<Vec<u8>>,
    pub cycles: Nat,
    pub settings: BlackholeSettings,
    pub memory_size: Option<Nat>,
    pub memory_metrics: Option<BlackholeMemoryMetrics>,
}

#[derive(Clone, Copy, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub(crate) enum BlackholeCanisterStatusKind {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "stopping")]
    Stopping,
    #[serde(rename = "stopped")]
    Stopped,
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

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{decode_one, encode_one};

    #[derive(CandidType, Deserialize)]
    enum WireStatusKind {
        #[serde(rename = "running")]
        Running,
    }

    #[derive(CandidType)]
    struct WireSettings {
        controllers: Vec<Principal>,
        freezing_threshold: Nat,
        memory_allocation: Nat,
        compute_allocation: Nat,
    }

    #[derive(CandidType)]
    struct WireStatus {
        status: WireStatusKind,
        module_hash: Option<Vec<u8>>,
        cycles: Nat,
        settings: WireSettings,
        memory_size: Nat,
    }

    #[test]
    fn decodes_the_vendored_fiduciary_blackhole_status_shape() {
        let controller = Principal::from_slice(&[1]);
        let bytes = encode_one(WireStatus {
            status: WireStatusKind::Running,
            module_hash: Some(vec![2; 32]),
            cycles: Nat::from(3u8),
            settings: WireSettings {
                controllers: vec![controller],
                freezing_threshold: Nat::from(4u8),
                memory_allocation: Nat::from(5u8),
                compute_allocation: Nat::from(6u8),
            },
            memory_size: Nat::from(7u8),
        })
        .unwrap();
        let decoded: BlackholeCanisterStatus = decode_one(&bytes).unwrap();
        assert_eq!(decoded.status, BlackholeCanisterStatusKind::Running);
        assert_eq!(decoded.settings.controllers, vec![controller]);
        assert_eq!(decoded.memory_size, Some(Nat::from(7u8)));
    }
}
