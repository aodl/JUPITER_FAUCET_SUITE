//! IO public settlement boundary at IO revision 8b2e1cba06b43bc587898343a0391a6fac7eca23.
//! No IO application code/dependency. This module does not dispatch ICP or IO.
//! Callers must obtain transactions from the configured authenticated Ledger and
//! Completed from the configured IO manager; untrusted notification data is not proof.
use candid::{CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::{
    icrc1::account::Account,
    icrc3::transactions::{Transaction, TRANSACTION_TRANSFER},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct NotifyJupiterDepositArgs {
    pub block_index: u128,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct JupiterCompleted {
    pub deposit_block: u128,
    pub gross_e8s: u128,
    pub stake_e8s: u128,
    pub observed_after_cached_stake_e8s: u128,
    pub liquid_e8s: u128,
    pub stake_transfer_block: u128,
    pub liquid_transfer_block: u128,
    pub stream_receipt_sequence: u64,
    pub backed_io_e8s: u128,
    pub io_transfer_block: u128,
    pub io_fee_e8s: u128,
    pub completed_at_nanos: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum JupiterProgress {
    Pending,
    Completed(JupiterCompleted),
    Stuck(String),
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub icp_ledger: Principal,
    pub io_ledger: Principal,
    pub io_manager: Principal,
    pub jupiter_default_icp: Account,
    pub io_manager_default_icp: Account,
    pub io_reserve: Account,
    pub jupiter_io: Account,
    pub activation_floor: u64,
    pub expected_icp_fee: u64,
}

fn same(a: Account, b: Account) -> bool {
    a.owner == b.owner && a.subaccount.unwrap_or([0; 32]) == b.subaccount.unwrap_or([0; 32])
}

impl Contract {
    pub fn validate(&self) -> Result<(), String> {
        if self.icp_ledger == self.io_ledger
            || self.io_manager != self.io_manager_default_icp.owner
            || self.activation_floor == 0
            || self.expected_icp_fee == 0
            || self.jupiter_default_icp.subaccount.unwrap_or([0; 32]) != [0; 32]
            || self.io_manager_default_icp.subaccount.unwrap_or([0; 32]) != [0; 32]
            || same(self.jupiter_default_icp, self.io_manager_default_icp)
            || same(self.io_reserve, self.jupiter_io)
        {
            return Err("IO contract needs distinct reviewed default ICP Accounts, reserve/recipient and nonzero activation floor/fee".into());
        }
        for principal in [
            self.icp_ledger,
            self.io_ledger,
            self.io_manager,
            self.jupiter_default_icp.owner,
            self.io_manager_default_icp.owner,
            self.io_reserve.owner,
            self.jupiter_io.owner,
        ] {
            if principal == Principal::anonymous() || principal == Principal::management_canister()
            {
                return Err("unsafe IO contract principal".into());
            }
        }
        Ok(())
    }
}

/// Compact interface-test journal, suitable for a future durable adapter.
/// No public canister caller can insert evidence through this Rust-only API.
#[derive(Default)]
pub struct Settlement {
    deposits: BTreeMap<u64, u64>,
    consumed_io_blocks: BTreeSet<u128>,
    settled: BTreeMap<u64, JupiterCompleted>,
    pub authenticated_io_inventory: u128,
}

impl Settlement {
    pub fn prove_icp(
        &mut self,
        contract: &Contract,
        ledger: Principal,
        block: u64,
        transaction: &Transaction,
    ) -> Result<NotifyJupiterDepositArgs, String> {
        contract.validate()?;
        if ledger != contract.icp_ledger {
            return Err("ICP evidence came from an unconfigured Ledger".into());
        }
        if block < contract.activation_floor {
            return Err("ICP deposit predates immutable activation floor".into());
        }
        let transfer = transaction
            .transfer
            .as_ref()
            .filter(|_| transaction.kind == TRANSACTION_TRANSFER)
            .ok_or("canonical ICP evidence is not a transfer")?;
        if !same(transfer.from, contract.jupiter_default_icp)
            || !same(transfer.to, contract.io_manager_default_icp)
            || transfer.fee != Some(Nat::from(contract.expected_icp_fee))
            || transfer.created_at_time.is_none_or(|time| time == 0)
            || transfer.spender.is_some()
        {
            return Err("ICP deposit does not match IO's exact source/destination/fee/time/spender contract".into());
        }
        let amount =
            u64::try_from(transfer.amount.0.clone()).map_err(|_| "ICP amount exceeds u64")?;
        if amount == 0 {
            return Err("empty ICP deposit".into());
        }
        if let Some(old) = self.deposits.insert(block, amount) {
            if old != amount {
                self.deposits.insert(block, old);
                return Err("contradictory canonical deposit".into());
            }
        }
        Ok(NotifyJupiterDepositArgs {
            block_index: block.into(),
        })
    }

    /// Pending/uncertain conversion contributes no inventory. The exact IO
    /// transfer must corroborate the authenticated Completed result, including
    /// IO's receipt-sequence memo and sponsored IO-side settlement fee.
    pub fn prove_io(
        &mut self,
        contract: &Contract,
        manager: Principal,
        ledger: Principal,
        progress: &JupiterProgress,
        block: u128,
        transaction: &Transaction,
    ) -> Result<bool, String> {
        contract.validate()?;
        if manager != contract.io_manager || ledger != contract.io_ledger {
            return Err("IO settlement evidence came from an unconfigured manager/Ledger".into());
        }
        let JupiterProgress::Completed(completed) = progress else {
            return Ok(false);
        };
        let deposit = u64::try_from(completed.deposit_block)
            .map_err(|_| "deposit block exceeds ICP block range")?;
        if let Some(old) = self.settled.get(&deposit) {
            return if old == completed {
                Ok(false)
            } else {
                Err("contradictory IO settlement replay".into())
            };
        }
        let gross = self
            .deposits
            .get(&deposit)
            .ok_or("ICP funding not canonically proved")?;
        let gross128 = u128::from(*gross);
        let stake = gross128.checked_mul(40).ok_or("IO split overflow")? / 100;
        let fee = u128::from(contract.expected_icp_fee);
        if completed.gross_e8s != gross128
            || completed.stake_e8s.checked_add(fee) != Some(stake)
            || completed.liquid_e8s.checked_add(fee) != Some(gross128 - stake)
            || completed.io_transfer_block != block
            || completed.stream_receipt_sequence == 0
            || completed.backed_io_e8s == 0
            || self.consumed_io_blocks.contains(&block)
        {
            return Err(
                "IO completed result does not match the funded deposit and exact receipt identity"
                    .into(),
            );
        }
        let transfer = transaction
            .transfer
            .as_ref()
            .filter(|_| transaction.kind == TRANSACTION_TRANSFER)
            .ok_or("IO receipt is not a canonical transfer")?;
        let mut memo = Sha256::new();
        memo.update(b"io-claim-receipt-recipient-v1");
        memo.update(completed.stream_receipt_sequence.to_be_bytes());
        memo.update(0_u32.to_be_bytes());
        if !same(transfer.from, contract.io_reserve)
            || !same(transfer.to, contract.jupiter_io)
            || transfer.amount != completed.backed_io_e8s
            || transfer.fee != Some(Nat::from(completed.io_fee_e8s))
            || transfer.memo.as_ref().map(|m| m.0.as_ref()) != Some(memo.finalize().as_slice())
            || transfer.created_at_time.is_none_or(|time| time == 0)
            || transfer.spender.is_some()
        {
            return Err(
                "actual IO receipt does not match settlement source/destination/amount/fee/memo"
                    .into(),
            );
        }
        let inventory = self
            .authenticated_io_inventory
            .checked_add(completed.backed_io_e8s)
            .ok_or("IO inventory overflow")?;
        self.consumed_io_blocks.insert(block);
        self.settled.insert(deposit, completed.clone());
        self.authenticated_io_inventory = inventory;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use icrc_ledger_types::icrc3::transactions::Transfer;
    fn account(id: u8) -> Account {
        Account {
            owner: candid::Principal::from_slice(&[id, 1]),
            subaccount: None,
        }
    }
    fn transfer(from: Account, to: Account, amount: u64, fee: u64, memo: Vec<u8>) -> Transaction {
        Transaction::transfer(
            Transfer {
                from,
                to,
                amount: amount.into(),
                fee: Some(fee.into()),
                spender: None,
                created_at_time: Some(123),
                memo: Some(memo.into()),
            },
            124,
        )
    }
    #[test]
    fn contract_double_requires_actual_funded_receipt_and_is_duplicate_safe() {
        let contract = Contract {
            icp_ledger: account(4).owner,
            io_ledger: account(5).owner,
            io_manager: account(2).owner,
            jupiter_default_icp: account(1),
            io_manager_default_icp: account(2),
            io_reserve: account(3),
            jupiter_io: account(1),
            activation_floor: 10,
            expected_icp_fee: 10_000,
        };
        let icp = transfer(account(1), account(2), 1_000_000_000, 10_000, vec![]);
        let mut settlement = Settlement::default();
        assert!(settlement
            .prove_icp(&contract, contract.icp_ledger, 9, &icp)
            .is_err());
        assert_eq!(
            settlement
                .prove_icp(&contract, contract.icp_ledger, 10, &icp)
                .unwrap()
                .block_index,
            10
        );
        let completed = JupiterCompleted {
            deposit_block: 10,
            gross_e8s: 1_000_000_000,
            stake_e8s: 399_990_000,
            observed_after_cached_stake_e8s: 1_000_000_000,
            liquid_e8s: 599_990_000,
            stake_transfer_block: 11,
            liquid_transfer_block: 12,
            stream_receipt_sequence: 7,
            backed_io_e8s: 42_000,
            io_transfer_block: 99,
            io_fee_e8s: 100,
            completed_at_nanos: 1234,
        };
        let mut hash = Sha256::new();
        hash.update(b"io-claim-receipt-recipient-v1");
        hash.update(7_u64.to_be_bytes());
        hash.update(0_u32.to_be_bytes());
        let io = transfer(
            account(3),
            account(1),
            42_000,
            100,
            hash.finalize().to_vec(),
        );
        assert!(!settlement
            .prove_io(
                &contract,
                contract.io_manager,
                contract.io_ledger,
                &JupiterProgress::Pending,
                99,
                &io
            )
            .unwrap());
        assert_eq!(settlement.authenticated_io_inventory, 0);
        let result = JupiterProgress::Completed(completed.clone());
        assert!(settlement
            .prove_io(
                &contract,
                account(9).owner,
                contract.io_ledger,
                &result,
                99,
                &io
            )
            .is_err());
        assert!(settlement
            .prove_io(
                &contract,
                contract.io_manager,
                account(9).owner,
                &result,
                99,
                &io
            )
            .is_err());
        let mut incorrect = io.clone();
        incorrect.transfer.as_mut().unwrap().amount = 43_000_u64.into();
        assert!(settlement
            .prove_io(
                &contract,
                contract.io_manager,
                contract.io_ledger,
                &result,
                99,
                &incorrect
            )
            .is_err());
        assert_eq!(settlement.authenticated_io_inventory, 0);
        assert!(settlement
            .prove_io(
                &contract,
                contract.io_manager,
                contract.io_ledger,
                &result,
                99,
                &io
            )
            .unwrap());
        assert!(!settlement
            .prove_io(
                &contract,
                contract.io_manager,
                contract.io_ledger,
                &result,
                99,
                &io
            )
            .unwrap());
        assert_eq!(settlement.authenticated_io_inventory, 42_000);
        let mut conflicting = completed;
        conflicting.backed_io_e8s += 1;
        assert!(settlement
            .prove_io(
                &contract,
                contract.io_manager,
                contract.io_ledger,
                &JupiterProgress::Completed(conflicting),
                99,
                &io
            )
            .is_err());
    }
}
