use async_trait::async_trait;
use candid::Principal;
use jupiter_ic_clients::cmc::{NotifyRetryableError, NotifyTerminalError, NotifyTopUpError};

use crate::clients::{ClientError, CmcClient};

fn map_notify_top_up_error(err: NotifyTopUpError) -> ClientError {
    match err {
        NotifyTopUpError::Retryable(NotifyRetryableError::Processing) => {
            ClientError::RetryableNotify("notify_top_up returned Processing".to_string())
        }
        NotifyTopUpError::Retryable(NotifyRetryableError::Other {
            error_code,
            error_message,
        }) => ClientError::RetryableNotify(format!(
            "notify_top_up returned other error: code={error_code} message={error_message}"
        )),
        NotifyTopUpError::Terminal(NotifyTerminalError::Refunded {
            reason,
            block_index,
        }) => ClientError::TerminalNotify(format!(
            "notify_top_up refunded deposit: reason={reason:?} block_index={block_index:?}"
        )),
        NotifyTopUpError::Terminal(NotifyTerminalError::TransactionTooOld(block_index)) => {
            ClientError::TerminalNotify(format!(
                "notify_top_up rejected stale block_index={block_index}"
            ))
        }
        NotifyTopUpError::Terminal(NotifyTerminalError::InvalidTransaction(message)) => {
            ClientError::TerminalNotify(format!(
                "notify_top_up rejected invalid transaction: {message}"
            ))
        }
        NotifyTopUpError::Transport(message) => {
            ClientError::Call(format!("notify_top_up transport failed: {message}"))
        }
        NotifyTopUpError::Decode(message) => {
            ClientError::Call(format!("notify_top_up decode failed: {message}"))
        }
        NotifyTopUpError::Convert(message) => ClientError::Convert(message),
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
    async fn get_icp_xdr_conversion_rate(
        &self,
    ) -> Result<crate::clients::CmcIcpXdrConversionRate, ClientError> {
        jupiter_ic_clients::cmc::get_icp_xdr_conversion_rate(self.canister_id)
            .await
            .map_err(Into::into)
    }

    async fn notify_top_up(
        &self,
        canister_id: Principal,
        block_index: u64,
    ) -> Result<u128, ClientError> {
        jupiter_ic_clients::cmc::notify_top_up(self.canister_id, canister_id, block_index)
            .await
            .map_err(map_notify_top_up_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_processing_as_retryable() {
        let err = map_notify_top_up_error(NotifyTopUpError::Retryable(
            NotifyRetryableError::Processing,
        ));
        assert!(matches!(err, ClientError::RetryableNotify(_)));
    }

    #[test]
    fn maps_terminal_refund_as_terminal() {
        let err =
            map_notify_top_up_error(NotifyTopUpError::Terminal(NotifyTerminalError::Refunded {
                reason: "refund".to_string(),
                block_index: Some(1),
            }));
        assert!(matches!(err, ClientError::TerminalNotify(_)));
    }
}
