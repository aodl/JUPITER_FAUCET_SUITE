pub(super) use candid::{Nat, Principal};
pub(super) use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
pub(super) use icrc_ledger_types::icrc1::account::Account;
pub(super) use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
pub(super) use std::time::Duration;

pub(super) const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;

pub(super) use crate::clients::canister_info::ManagementCanisterInfoClient;
pub(super) use crate::clients::cmc::CyclesMintingCanister;
pub(super) use crate::clients::governance::NnsGovernanceCanister;
pub(super) use crate::clients::index::{
    account_identifier_text_for_account, GetAccountIdentifierTransactionsResponse,
    IcpIndexCanister, IndexOperation, IndexTransactionWithId,
};
pub(super) use crate::clients::ledger::IcrcLedgerCanister;
pub(super) use crate::clients::{CanisterStatusClient, CmcClient, GovernanceClient, IndexClient, LedgerClient};
pub(super) use crate::state::{
    ActivePayoutJob, ForcedRescueReason, PendingNotification, PendingTransfer, PendingTransferPhase,
    SkipRange, TransferKind,
};
pub(super) use crate::{logic, policy, state};


pub(super) const PAGE_SIZE: u64 = 500;
pub(super) const MAX_INDEX_PAGES_PER_PAYOUT_TICK: u64 = 64;
pub(super) const MAX_INDEX_PAGES_PER_LATEST_SCAN: u64 = 128;
// Only persist large barren spans so the durable skip-range cache stays small and a
// one-time adversarial history scan remains much more expensive for the attacker than for
// the faucet. These ranges are only valid while commitment-classification policy is
// unchanged; if min_tx_e8s or memo-policy semantics ever change, the cache must be reset.
pub(super) const MIN_SKIP_RANGE_TX_COUNT: u64 = 10_000;
pub(super) const LEDGER_CREATED_AT_MAX_AGE_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;
pub(super) const LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS: u64 = 60 * 1_000_000_000;
pub(super) const DEFAULT_STAKE_RECOGNITION_DELAY_SECONDS: u64 = 24 * 60 * 60;
