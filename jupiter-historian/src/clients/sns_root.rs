use async_trait::async_trait;
use candid::{CandidType, Nat, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

use crate::clients::{ClientError, SnsRootClient};

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
pub struct GetSnsCanistersSummaryRequest {
    pub update_canister_list: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct SnsCanisterStatus {
    pub cycles: Option<Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct SnsCanisterSummary {
    pub canister_id: Option<Principal>,
    pub status: Option<SnsCanisterStatus>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
pub struct GetSnsCanistersSummaryResponse {
    pub root: Option<SnsCanisterSummary>,
    pub governance: Option<SnsCanisterSummary>,
    pub ledger: Option<SnsCanisterSummary>,
    pub swap: Option<SnsCanisterSummary>,
    pub index: Option<SnsCanisterSummary>,
    pub dapps: Vec<SnsCanisterSummary>,
    pub archives: Vec<SnsCanisterSummary>,
}

pub struct SnsRootCanister;

#[async_trait]
impl SnsRootClient for SnsRootCanister {
    async fn get_sns_canisters_summary(&self, root_id: Principal) -> Result<GetSnsCanistersSummaryResponse, ClientError> {
        let resp = Call::bounded_wait(root_id, "get_sns_canisters_summary")
            .with_arg(GetSnsCanistersSummaryRequest { update_canister_list: Some(false) })
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("{e:?}")))?;
        resp.candid().map_err(|e| ClientError::Call(format!("decode get_sns_canisters_summary failed: {e:?}")))
    }
}
