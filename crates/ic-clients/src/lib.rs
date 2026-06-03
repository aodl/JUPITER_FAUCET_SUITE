pub mod account;
pub mod account_identifier;
pub mod cmc;
pub mod constants;
pub mod generated;
pub mod governance;
pub mod index;
pub mod ledger;
pub mod lifecycle;
pub mod management;
pub mod timer_guard;
pub mod xrc;

#[derive(thiserror::Error, Debug)]
pub enum ClientError {
    #[error("inter-canister call failed: {0}")]
    Call(String),
    #[error("conversion error: {0}")]
    Convert(String),
}
