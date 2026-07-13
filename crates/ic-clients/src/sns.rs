use candid::{CandidType, Nat, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

use crate::ClientError;

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct ListDeployedSnsesRequest {}

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct DeployedSns {
    pub root_canister_id: Option<Principal>,
    #[serde(default)]
    pub governance_canister_id: Option<Principal>,
    #[serde(default)]
    pub ledger_canister_id: Option<Principal>,
    #[serde(default)]
    pub swap_canister_id: Option<Principal>,
    #[serde(default)]
    pub index_canister_id: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct ListDeployedSnsesResponse {
    pub instances: Vec<DeployedSns>,
}

pub struct SnsWasmCanister {
    canister_id: Principal,
}

impl SnsWasmCanister {
    pub fn new(canister_id: Principal) -> Self {
        Self { canister_id }
    }

    pub async fn list_deployed_snses(&self) -> Result<ListDeployedSnsesResponse, ClientError> {
        let resp = Call::bounded_wait(self.canister_id, "list_deployed_snses")
            .with_arg(ListDeployedSnsesRequest::default())
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("list_deployed_snses failed: {e:?}")))?;
        resp.candid()
            .map_err(|e| ClientError::Call(format!("decode list_deployed_snses failed: {e:?}")))
    }
}

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct ListSnsCanistersRequest {}

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct ListSnsCanistersResponse {
    pub root: Option<Principal>,
    pub governance: Option<Principal>,
    pub ledger: Option<Principal>,
    pub swap: Option<Principal>,
    pub index: Option<Principal>,
    pub dapps: Vec<Principal>,
    pub archives: Vec<Principal>,
    #[serde(default)]
    pub extensions: Option<SnsExtensions>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct SnsExtensions {
    pub extension_canister_ids: Vec<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct SnsRootCanisterStatusRequest {
    pub canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct SnsRootCanisterStatusResponse {
    pub cycles: Nat,
}

pub struct SnsRootCanister;

impl SnsRootCanister {
    pub async fn list_sns_canisters(
        &self,
        root_id: Principal,
    ) -> Result<ListSnsCanistersResponse, ClientError> {
        let resp = Call::bounded_wait(root_id, "list_sns_canisters")
            .with_arg(ListSnsCanistersRequest::default())
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("list_sns_canisters failed: {e:?}")))?;
        resp.candid()
            .map_err(|e| ClientError::Call(format!("decode list_sns_canisters failed: {e:?}")))
    }

    pub async fn canister_status(
        &self,
        root_id: Principal,
        target_id: Principal,
    ) -> Result<SnsRootCanisterStatusResponse, ClientError> {
        let resp = Call::bounded_wait(root_id, "canister_status")
            .with_arg(SnsRootCanisterStatusRequest {
                canister_id: target_id,
            })
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("sns root canister_status failed: {e:?}")))?;
        resp.candid().map_err(|e| {
            ClientError::Call(format!("decode sns root canister_status failed: {e:?}"))
        })
    }
}

#[derive(Clone, Debug, CandidType, Deserialize, Default, PartialEq, Eq)]
pub struct SnsSwapCanisterStatusRequest {}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct SnsSwapCanisterStatusResponse {
    pub cycles: Nat,
}

pub struct SnsSwapCanister;

impl SnsSwapCanister {
    pub async fn get_canister_status(
        &self,
        swap_id: Principal,
    ) -> Result<SnsSwapCanisterStatusResponse, ClientError> {
        let resp = Call::bounded_wait(swap_id, "get_canister_status")
            .with_arg(SnsSwapCanisterStatusRequest::default())
            .change_timeout(60)
            .await
            .map_err(|e| {
                ClientError::Call(format!("sns swap get_canister_status failed: {e:?}"))
            })?;
        resp.candid().map_err(|e| {
            ClientError::Call(format!("decode sns swap get_canister_status failed: {e:?}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{Decode, Encode};

    #[derive(CandidType)]
    struct LegacyListSnsCanistersResponse {
        root: Option<Principal>,
        governance: Option<Principal>,
        ledger: Option<Principal>,
        swap: Option<Principal>,
        index: Option<Principal>,
        dapps: Vec<Principal>,
        archives: Vec<Principal>,
    }

    #[derive(CandidType)]
    struct RootOnlyDeployedSns {
        root_canister_id: Option<Principal>,
    }

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    #[test]
    fn list_sns_canisters_decodes_missing_extensions_field() {
        let dapp = principal("77deu-baaaa-aaaar-qb6za-cai");
        let archive = principal("e3mmv-5qaaa-aaaah-aadma-cai");
        let encoded = Encode!(&LegacyListSnsCanistersResponse {
            root: None,
            governance: None,
            ledger: None,
            swap: None,
            index: None,
            dapps: vec![dapp],
            archives: vec![archive],
        })
        .unwrap();

        let decoded = Decode!(&encoded, ListSnsCanistersResponse).unwrap();

        assert_eq!(decoded.dapps, vec![dapp]);
        assert_eq!(decoded.archives, vec![archive]);
        assert!(decoded.extensions.is_none());
    }

    #[test]
    fn list_deployed_snses_decodes_instance_with_only_root_canister_id() {
        #[derive(CandidType)]
        struct RootOnlyListDeployedSnsesResponse {
            instances: Vec<RootOnlyDeployedSns>,
        }

        let root = principal("r7inp-6aaaa-aaaaa-aaabq-cai");
        let encoded = Encode!(&RootOnlyListDeployedSnsesResponse {
            instances: vec![RootOnlyDeployedSns {
                root_canister_id: Some(root),
            }],
        })
        .unwrap();

        let decoded = Decode!(&encoded, ListDeployedSnsesResponse).unwrap();

        assert_eq!(decoded.instances.len(), 1);
        assert_eq!(decoded.instances[0].root_canister_id, Some(root));
        assert_eq!(decoded.instances[0].governance_canister_id, None);
        assert_eq!(decoded.instances[0].ledger_canister_id, None);
        assert_eq!(decoded.instances[0].swap_canister_id, None);
        assert_eq!(decoded.instances[0].index_canister_id, None);
    }

    #[test]
    fn list_sns_canisters_decodes_populated_extensions_field() {
        let extension = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        let encoded = Encode!(&ListSnsCanistersResponse {
            extensions: Some(SnsExtensions {
                extension_canister_ids: vec![extension],
            }),
            ..Default::default()
        })
        .unwrap();

        let decoded = Decode!(&encoded, ListSnsCanistersResponse).unwrap();

        assert_eq!(
            decoded.extensions,
            Some(SnsExtensions {
                extension_canister_ids: vec![extension],
            })
        );
    }
}
