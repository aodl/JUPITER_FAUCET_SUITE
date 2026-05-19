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

fn classify_notify_top_up_result(result: NotifyTopUpResult) -> Result<(), ClientError> {
    match result {
        NotifyTopUpResult::Ok(_) => Ok(()),
        NotifyTopUpResult::Err(NotifyError::Processing) => Err(ClientError::RetryableNotify(
            "notify_top_up returned Processing".to_string(),
        )),
        NotifyTopUpResult::Err(NotifyError::Refunded {
            reason,
            block_index,
        }) => Err(ClientError::TerminalNotify(format!(
            "notify_top_up refunded deposit: reason={reason:?} block_index={block_index:?}"
        ))),
        NotifyTopUpResult::Err(NotifyError::TransactionTooOld(block_index)) => {
            Err(ClientError::TerminalNotify(format!(
                "notify_top_up rejected stale block_index={block_index}"
            )))
        }
        NotifyTopUpResult::Err(NotifyError::InvalidTransaction(message)) => {
            Err(ClientError::TerminalNotify(format!(
                "notify_top_up rejected invalid transaction: {message}"
            )))
        }
        NotifyTopUpResult::Err(NotifyError::Other {
            error_code,
            error_message,
        }) => Err(ClientError::RetryableNotify(format!(
            "notify_top_up returned other error: code={error_code} message={error_message}"
        ))),
    }
}

pub(crate) struct CyclesMintingCanister {
    canister_id: Principal,
}

impl CyclesMintingCanister {
    pub(crate) fn new(canister_id: Principal) -> Self {
        Self { canister_id }
    }
}

#[async_trait]
impl CmcClient for CyclesMintingCanister {
    async fn notify_top_up(
        &self,
        canister_id: Principal,
        block_index: u64,
    ) -> Result<(), ClientError> {
        let resp = Call::bounded_wait(self.canister_id, "notify_top_up")
            .with_arg(NotifyTopUpArg {
                canister_id,
                block_index,
            })
            .change_timeout(60)
            .await
            .map_err(|e| ClientError::Call(format!("notify_top_up transport failed: {e:?}")))?;
        let result: NotifyTopUpResult = resp
            .candid()
            .map_err(|e| ClientError::Call(format!("notify_top_up decode failed: {e:?}")))?;
        classify_notify_top_up_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_is_retryable() {
        assert!(matches!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(NotifyError::Processing)),
            Err(ClientError::RetryableNotify(_))
        ));
    }

    #[test]
    fn refunded_is_terminal() {
        assert!(matches!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(NotifyError::Refunded {
                reason: "refund".to_string(),
                block_index: Some(1)
            })),
            Err(ClientError::TerminalNotify(_))
        ));
    }
}
