use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct NotifyTopUpArg {
    pub canister_id: Principal,
    pub block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum NotifyTopUpResult {
    Ok(Nat),
    Err(NotifyError),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum NotifyError {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyTopUpError {
    Retryable(NotifyRetryableError),
    Terminal(NotifyTerminalError),
    Transport(String),
    Decode(String),
    Convert(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyRetryableError {
    Processing,
    Other {
        error_code: u64,
        error_message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyTerminalError {
    Refunded {
        reason: String,
        block_index: Option<u64>,
    },
    TransactionTooOld(u64),
    InvalidTransaction(String),
}

pub fn nat_to_u128(n: &Nat) -> Result<u128, String> {
    u128::try_from(n.0.clone()).map_err(|_| format!("Nat does not fit u128: {n}"))
}

pub fn classify_notify_top_up_result(result: NotifyTopUpResult) -> Result<u128, NotifyTopUpError> {
    match result {
        NotifyTopUpResult::Ok(cycles) => nat_to_u128(&cycles).map_err(NotifyTopUpError::Convert),
        NotifyTopUpResult::Err(NotifyError::Processing) => Err(NotifyTopUpError::Retryable(
            NotifyRetryableError::Processing,
        )),
        NotifyTopUpResult::Err(NotifyError::Refunded {
            reason,
            block_index,
        }) => Err(NotifyTopUpError::Terminal(NotifyTerminalError::Refunded {
            reason,
            block_index,
        })),
        NotifyTopUpResult::Err(NotifyError::TransactionTooOld(block_index)) => Err(
            NotifyTopUpError::Terminal(NotifyTerminalError::TransactionTooOld(block_index)),
        ),
        NotifyTopUpResult::Err(NotifyError::InvalidTransaction(message)) => Err(
            NotifyTopUpError::Terminal(NotifyTerminalError::InvalidTransaction(message)),
        ),
        NotifyTopUpResult::Err(NotifyError::Other {
            error_code,
            error_message,
        }) => Err(NotifyTopUpError::Retryable(NotifyRetryableError::Other {
            error_code,
            error_message,
        })),
    }
}

pub async fn notify_top_up(
    cmc_id: Principal,
    canister_id: Principal,
    block_index: u64,
) -> Result<u128, NotifyTopUpError> {
    let arg = NotifyTopUpArg {
        canister_id,
        block_index,
    };
    let result: NotifyTopUpResult = Call::bounded_wait(cmc_id, "notify_top_up")
        .with_arg(&arg)
        .change_timeout(60)
        .await
        .map_err(|e| NotifyTopUpError::Transport(format!("{e:?}")))?
        .candid()
        .map_err(|e| NotifyTopUpError::Decode(format!("{e:?}")))?;
    classify_notify_top_up_result(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ok_as_cycles() {
        assert_eq!(
            classify_notify_top_up_result(NotifyTopUpResult::Ok(Nat::from(42u32))).unwrap(),
            42
        );
    }

    #[test]
    fn classifies_processing_as_retryable() {
        assert_eq!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(NotifyError::Processing)),
            Err(NotifyTopUpError::Retryable(
                NotifyRetryableError::Processing
            ))
        );
    }

    #[test]
    fn classifies_refunded_as_terminal() {
        assert_eq!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(NotifyError::Refunded {
                reason: "refund".to_string(),
                block_index: Some(1),
            })),
            Err(NotifyTopUpError::Terminal(NotifyTerminalError::Refunded {
                reason: "refund".to_string(),
                block_index: Some(1),
            }))
        );
    }

    #[test]
    fn classifies_transaction_too_old_as_terminal() {
        assert!(matches!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(NotifyError::TransactionTooOld(
                99
            ))),
            Err(NotifyTopUpError::Terminal(
                NotifyTerminalError::TransactionTooOld(99)
            ))
        ));
    }

    #[test]
    fn classifies_invalid_transaction_as_terminal() {
        let message = "bad block".to_string();
        assert!(matches!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(
                NotifyError::InvalidTransaction(message.clone())
            )),
            Err(NotifyTopUpError::Terminal(
                NotifyTerminalError::InvalidTransaction(classified)
            )) if classified == message
        ));
    }

    #[test]
    fn classifies_other_as_retryable() {
        let message = "try later".to_string();
        assert!(matches!(
            classify_notify_top_up_result(NotifyTopUpResult::Err(NotifyError::Other {
                error_code: 5,
                error_message: message.clone(),
            })),
            Err(NotifyTopUpError::Retryable(NotifyRetryableError::Other {
                error_code: 5,
                error_message: classified,
            })) if classified == message
        ));
    }
}
