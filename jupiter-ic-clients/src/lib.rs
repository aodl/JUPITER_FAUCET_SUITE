pub mod account_identifier;
pub mod index;
pub mod ledger;

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
    #[error("conversion error: {0}")]
    Convert(String),
}
