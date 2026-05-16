use candid::{Nat, Principal};
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::time::Duration;

const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;

use crate::clients::canister_info::ManagementCanisterInfoClient;
use crate::clients::cmc::CyclesMintingCanister;
use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::index::{
    account_identifier_text_for_account, GetAccountIdentifierTransactionsResponse,
    IcpIndexCanister, IndexTransactionWithId,
};
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{CanisterStatusClient, CmcClient, GovernanceClient, IndexClient, LedgerClient};
use crate::state::{
    ActivePayoutJob, ForcedRescueReason, PendingNotification, PendingTransfer, PendingTransferPhase,
    SkipRange, TransferKind,
};
use crate::{logic, policy, state};


const PAGE_SIZE: u64 = 500;
const MAX_INDEX_PAGES_PER_PAYOUT_TICK: u64 = 64;
const MAX_INDEX_PAGES_PER_LATEST_SCAN: u64 = 128;
// Only persist large barren spans so the durable skip-range cache stays small and a
// one-time adversarial history scan remains much more expensive for the attacker than for
// the faucet. These ranges are only valid while commitment-classification policy is
// unchanged; if min_tx_e8s or memo-policy semantics ever change, the cache must be reset.
const MIN_SKIP_RANGE_TX_COUNT: u64 = 10_000;
const LEDGER_CREATED_AT_MAX_AGE_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;
const LEDGER_CREATED_AT_MAX_FUTURE_SKEW_NANOS: u64 = 60 * 1_000_000_000;
const DEFAULT_STAKE_RECOGNITION_DELAY_SECONDS: u64 = 24 * 60 * 60;
