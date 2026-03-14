use async_trait::async_trait;
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;

use crate::clients::{ClientError, CmcClient};

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyTopUpArg {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum NotifyTopUpResult {
    Ok(Nat),
    Err(NotifyError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum NotifyError {
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    Processing,
    TransactionTooOld(u64),
    InvalidTransaction(String),
    Other {
        error_code: u64,
        error_message: String,
    },
}

pub struct CyclesMintingCanister {
    cmc_id: Principal,
}

impl CyclesMintingCanister {
    pub fn new(cmc_id: Principal) -> Self {
        Self { cmc_id }
    }
}

#[async_trait]
impl CmcClient for CyclesMintingCanister {
    async fn notify_top_up(&self, canister_id: Principal, block_index: u64) -> Result<(), ClientError> {
        let arg = NotifyTopUpArg {
            canister_id,
            block_index,
        };

        let result: NotifyTopUpResult = Call::bounded_wait(self.cmc_id, "notify_top_up")
            .with_arg(&arg)
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("notify_top_up transport failed: {e:?}")))?
            .candid()
            .map_err(|e| ClientError::Call(format!("notify_top_up decode failed: {e:?}")))?;

        match result {
            NotifyTopUpResult::Ok(_) => Ok(()),
            NotifyTopUpResult::Err(NotifyError::Processing) => Err(ClientError::Call(
                "notify_top_up returned retriable error: Processing".to_string(),
            )),
            NotifyTopUpResult::Err(err) => Err(ClientError::Call(format!(
                "notify_top_up returned error: {err:?}"
            ))),
        }
    }
}
