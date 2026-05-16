use crate::clients::governance::NnsGovernanceCanister;
use crate::clients::ledger::IcrcLedgerCanister;
use crate::clients::{GovernanceClient, LedgerClient};
use crate::{logic, policy, state};

use candid::{Nat, Principal};
use ic_cdk::management_canister::{update_settings, CanisterSettings, UpdateSettingsArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{Memo, TransferArg, TransferError};
use std::time::Duration;

const MAIN_TICK_LEASE_SECONDS: u64 = 15 * 60;

#[cfg(feature = "debug_api")]
use std::cell::RefCell;
