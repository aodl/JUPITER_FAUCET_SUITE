use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
use icrc_ledger_types::icrc3::transactions::{Mint, Transaction, Transfer};
use jupiter_ic_clients::account_identifier::account_identifier_text;
use jupiter_ic_clients::icrc_index::{
    GetAccountTransactionsArgs, GetAccountTransactionsResponse, GetAccountTransactionsResult,
    TransactionWithId,
};
use num_traits::ToPrimitive;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AccountKey {
    owner: Principal,
    sub: Option<[u8; 32]>,
}

fn key(a: &Account) -> AccountKey {
    AccountKey {
        owner: a.owner,
        sub: a.subaccount,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DedupKey {
    from: AccountKey,
    to: AccountKey,
    amount: u64,
    fee: u64,
    memo: Option<Vec<u8>>,
    created_at: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum DebugNextTransferError {
    PassThrough,
    AcceptThenTrap,
    TemporarilyUnavailable,
    TooOld,
    CreatedInFuture { ledger_time: u64 },
    BadFee { expected_fee_e8s: u64 },
    Duplicate { duplicate_of: u64 },
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct TransferRecord {
    pub from: Account,
    pub to: Account,
    pub amount: Nat,
    pub fee: Nat,
    pub memo: Option<Vec<u8>>,
    pub created_at_time: Option<u64>,
    pub result: String,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct TransferAttemptRecord {
    pub from_subaccount: Option<[u8; 32]>,
    pub to: Account,
    pub amount: Nat,
    pub fee: Option<Nat>,
    pub memo: Option<Vec<u8>>,
    pub created_at_time: Option<u64>,
    pub result: String,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct LegacyTransferArg {
    pub memo: u64,
    pub amount: Tokens,
    pub fee: Tokens,
    pub from_subaccount: Option<[u8; 32]>,
    pub to: Vec<u8>,
    pub created_at_time: Option<LegacyTimeStamp>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct LegacyTimeStamp {
    pub timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum LegacyTransferError {
    BadFee { expected_fee: Tokens },
    InsufficientFunds { balance: Tokens },
    TxTooOld { allowed_window_nanos: u64 },
    TxCreatedInFuture,
    TxDuplicate { duplicate_of: u64 },
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct LegacyTransferRecord {
    pub from: Account,
    pub to_account_identifier_hex: String,
    pub amount: Tokens,
    pub fee: Tokens,
    pub memo: u64,
    pub created_at_time: Option<u64>,
    pub result: String,
}

#[derive(Default)]
struct LedgerState {
    fee_e8s: u64,
    fee_query_failure: bool,
    next_error: Option<DebugNextTransferError>,
    next_error_script: VecDeque<DebugNextTransferError>,
    accept_then_trap_from_subaccount: Option<[u8; 32]>,
    balances: HashMap<AccountKey, u128>,
    next_block: u64,
    dedup: HashMap<DedupKey, u64>,
    transfer_attempts: Vec<TransferAttemptRecord>,
    transfers: Vec<TransferRecord>,
    legacy_transfers: Vec<LegacyTransferRecord>,
    index_transactions: Vec<TransactionWithId>,
    index_source_ledger: Option<Principal>,
    index_hidden_newest_transactions: u64,
}

thread_local! {
    static ST: RefCell<LedgerState> = RefCell::new(LedgerState { fee_e8s: 10_000, ..Default::default() });
}

fn nat_u64(n: &Nat) -> u64 {
    n.0.to_u64().unwrap_or(u64::MAX)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct BinaryAccountBalanceArgs {
    pub account: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Tokens {
    pub e8s: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct GetBlocksArgs {
    pub start: u64,
    pub length: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct QueryBlocksResponse {
    pub chain_length: u64,
    pub certificate: Option<Vec<u8>>,
    pub blocks: Vec<Vec<u8>>,
    pub first_block_index: u64,
    pub archived_blocks: Vec<Vec<u8>>,
}

fn account_identifier_bytes(a: &Account) -> [u8; 32] {
    let text = account_identifier_text(a.owner, a.subaccount);
    let mut bytes = [0u8; 32];
    for (idx, byte) in bytes.iter_mut().enumerate() {
        let start = idx * 2;
        *byte = u8::from_str_radix(&text[start..start + 2], 16)
            .expect("account identifier should be hex");
    }
    bytes
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::query]
fn icrc1_fee() -> Nat {
    ST.with(|s| {
        let st = s.borrow();
        if st.fee_query_failure {
            ic_cdk::trap("debug injected icrc1_fee failure");
        }
        Nat::from(st.fee_e8s)
    })
}

#[ic_cdk::query]
fn icrc1_balance_of(a: Account) -> Nat {
    ST.with(|s| {
        let st = s.borrow();
        let bal = *st.balances.get(&key(&a)).unwrap_or(&0);
        Nat::from(bal)
    })
}

#[ic_cdk::query]
fn account_balance(args: BinaryAccountBalanceArgs) -> Tokens {
    let requested = args.account;
    ST.with(|s| {
        let st = s.borrow();
        let mut e8s: u64 = 0;
        for (acct, bal) in st.balances.iter() {
            let account = Account {
                owner: acct.owner,
                subaccount: acct.sub,
            };
            if account_identifier_bytes(&account).as_slice() == requested.as_slice() {
                e8s = (*bal).try_into().unwrap_or(u64::MAX);
                break;
            }
        }
        Tokens { e8s }
    })
}

#[ic_cdk::query]
fn query_blocks(_args: GetBlocksArgs) -> QueryBlocksResponse {
    QueryBlocksResponse {
        chain_length: ST.with(|s| s.borrow().next_block),
        certificate: None,
        blocks: Vec::new(),
        first_block_index: 0,
        archived_blocks: Vec::new(),
    }
}

#[ic_cdk::update]
async fn icrc1_transfer(arg: TransferArg) -> Result<BlockIndex, TransferError> {
    let targeted_accept_then_trap = ST.with(|s| {
        let mut st = s.borrow_mut();
        if st.accept_then_trap_from_subaccount.is_some()
            && st.accept_then_trap_from_subaccount == arg.from_subaccount
        {
            st.accept_then_trap_from_subaccount = None;
            true
        } else {
            false
        }
    });
    // Inject scripted error if set, otherwise a one-shot next error.
    let scripted = ST.with(|s| {
        let mut st = s.borrow_mut();
        if let Some(scripted) = st.next_error_script.pop_front() {
            Some(scripted)
        } else {
            st.next_error.take()
        }
    });
    let caller = ic_cdk::api::msg_caller();
    if targeted_accept_then_trap || matches!(scripted, Some(DebugNextTransferError::AcceptThenTrap))
    {
        let attempt_index = start_transfer_attempt(&arg);
        apply_icrc1_transfer(caller, arg, attempt_index, "AcceptedResponseLost")?;
        // Yield after applying the transfer so the accepted ledger state is committed before
        // the response is lost. The later trap then exercises transport uncertainty without
        // depending on whether a replica drives callbacks in the initiating PocketIC tick.
        Call::unbounded_wait(ic_cdk::api::canister_self(), "debug_noop")
            .await
            .unwrap_or_else(|error| ic_cdk::trap(format!("debug barrier call failed: {error:?}")));
        ic_cdk::trap("debug accepted transfer with lost response");
    }

    let attempt_index = start_transfer_attempt(&arg);
    if let Some(err) = scripted {
        let error = match err {
            DebugNextTransferError::PassThrough | DebugNextTransferError::AcceptThenTrap => None,
            DebugNextTransferError::TemporarilyUnavailable => {
                Some(TransferError::TemporarilyUnavailable)
            }
            DebugNextTransferError::TooOld => Some(TransferError::TooOld),
            DebugNextTransferError::CreatedInFuture { ledger_time } => {
                Some(TransferError::CreatedInFuture { ledger_time })
            }
            DebugNextTransferError::BadFee { expected_fee_e8s } => Some(TransferError::BadFee {
                expected_fee: Nat::from(expected_fee_e8s),
            }),
            DebugNextTransferError::Duplicate { duplicate_of } => Some(TransferError::Duplicate {
                duplicate_of: Nat::from(duplicate_of),
            }),
        };
        if let Some(error) = error {
            set_transfer_attempt_result(attempt_index, transfer_error_name(&error));
            return Err(error);
        }
    }

    apply_icrc1_transfer(caller, arg, attempt_index, "Ok")
}

#[ic_cdk::update]
fn debug_noop() {}

fn start_transfer_attempt(arg: &TransferArg) -> usize {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        let attempt_index = st.transfer_attempts.len();
        st.transfer_attempts.push(TransferAttemptRecord {
            from_subaccount: arg.from_subaccount,
            to: arg.to,
            amount: arg.amount.clone(),
            fee: arg.fee.clone(),
            memo: arg.memo.as_ref().map(|memo| memo.0.to_vec()),
            created_at_time: arg.created_at_time,
            result: "Started".to_string(),
        });
        attempt_index
    })
}

fn apply_icrc1_transfer(
    caller: Principal,
    arg: TransferArg,
    attempt_index: usize,
    accepted_result: &str,
) -> Result<BlockIndex, TransferError> {
    let from = Account {
        owner: caller,
        subaccount: arg.from_subaccount,
    };

    let fee_expected = ST.with(|s| s.borrow().fee_e8s);
    let fee = arg.fee.as_ref().map(nat_u64).unwrap_or(fee_expected);

    // BadFee if caller provided fee and it doesn't match expected
    if let Some(provided) = arg.fee.as_ref().map(nat_u64) {
        if provided != fee_expected {
            let error = TransferError::BadFee {
                expected_fee: Nat::from(fee_expected),
            };
            set_transfer_attempt_result(attempt_index, transfer_error_name(&error));
            return Err(error);
        }
    }

    let amount = nat_u64(&arg.amount);
    let total_debit: u128 = (amount as u128).saturating_add(fee as u128);

    // Memo in icrc-ledger-types is Memo(ByteBuf) => convert to Vec<u8>
    let memo_opt: Option<Vec<u8>> = arg.memo.as_ref().map(|m| m.0.to_vec());

    let created_at = arg.created_at_time.unwrap_or(0);

    // ICRC transfer identity includes an absent/empty memo. A meaningful timestamp is enough.
    if created_at != 0 {
        let dkey = DedupKey {
            from: key(&from),
            to: key(&arg.to),
            amount,
            fee,
            memo: memo_opt.clone(),
            created_at,
        };

        if let Some(block) = ST.with(|s| s.borrow().dedup.get(&dkey).cloned()) {
            set_transfer_attempt_result(attempt_index, "Duplicate");
            return Err(TransferError::Duplicate {
                duplicate_of: Nat::from(block),
            });
        }
    }

    let from_key = key(&from);
    let to_key = key(&arg.to);

    let from_bal: u128 = ST.with(|s| *s.borrow().balances.get(&from_key).unwrap_or(&0));
    if from_bal < total_debit {
        let error = TransferError::InsufficientFunds {
            balance: Nat::from(from_bal),
        };
        set_transfer_attempt_result(attempt_index, transfer_error_name(&error));
        return Err(error);
    }

    // Apply mutations and record transfer
    let block = ST.with(|s| {
        let mut st = s.borrow_mut();

        // debit
        let fb = st.balances.entry(from_key.clone()).or_insert(0);
        *fb = fb.saturating_sub(total_debit);

        // credit net amount
        let tb = st.balances.entry(to_key.clone()).or_insert(0);
        *tb = tb.saturating_add(amount as u128);

        // allocate block index
        st.next_block += 1;
        let block = st.next_block;

        // store dedup
        if created_at != 0 {
            let dkey = DedupKey {
                from: from_key.clone(),
                to: to_key.clone(),
                amount,
                fee,
                memo: memo_opt.clone(),
                created_at,
            };
            st.dedup.insert(dkey, block);
        }

        st.transfers.push(TransferRecord {
            from,
            to: arg.to,
            amount: Nat::from(amount),
            fee: Nat::from(fee),
            memo: memo_opt.clone(),
            created_at_time: arg.created_at_time,
            result: "Ok".to_string(),
        });
        st.index_transactions.push(TransactionWithId {
            id: Nat::from(block),
            transaction: Transaction::transfer(
                Transfer {
                    amount: Nat::from(amount),
                    from,
                    to: arg.to,
                    spender: None,
                    memo: arg.memo,
                    fee: Some(Nat::from(fee)),
                    created_at_time: arg.created_at_time,
                },
                ic_cdk::api::time(),
            ),
        });

        block
    });

    set_transfer_attempt_result(attempt_index, accepted_result);
    Ok(Nat::from(block))
}

fn set_transfer_attempt_result(attempt_index: usize, result: &str) {
    ST.with(|s| {
        s.borrow_mut().transfer_attempts[attempt_index].result = result.to_string();
    });
}

fn transfer_error_name(error: &TransferError) -> &'static str {
    match error {
        TransferError::BadFee { .. } => "BadFee",
        TransferError::BadBurn { .. } => "BadBurn",
        TransferError::InsufficientFunds { .. } => "InsufficientFunds",
        TransferError::TooOld => "TooOld",
        TransferError::CreatedInFuture { .. } => "CreatedInFuture",
        TransferError::Duplicate { .. } => "Duplicate",
        TransferError::TemporarilyUnavailable => "TemporarilyUnavailable",
        TransferError::GenericError { .. } => "GenericError",
    }
}

#[ic_cdk::update]
fn transfer(arg: LegacyTransferArg) -> Result<u64, LegacyTransferError> {
    let caller = ic_cdk::api::msg_caller();
    let from = Account {
        owner: caller,
        subaccount: arg.from_subaccount,
    };
    let fee_expected = ST.with(|s| s.borrow().fee_e8s);
    if arg.fee.e8s != fee_expected {
        return Err(LegacyTransferError::BadFee {
            expected_fee: Tokens { e8s: fee_expected },
        });
    }
    if arg.to.len() != 32 {
        return Err(LegacyTransferError::TxCreatedInFuture);
    }
    let total_debit = (arg.amount.e8s as u128).saturating_add(arg.fee.e8s as u128);
    let from_key = key(&from);
    let from_bal = ST.with(|s| *s.borrow().balances.get(&from_key).unwrap_or(&0));
    if from_bal < total_debit {
        return Err(LegacyTransferError::InsufficientFunds {
            balance: Tokens {
                e8s: from_bal.try_into().unwrap_or(u64::MAX),
            },
        });
    }
    Ok(ST.with(|s| {
        let mut st = s.borrow_mut();
        let fb = st.balances.entry(from_key).or_insert(0);
        *fb = fb.saturating_sub(total_debit);
        st.next_block = st.next_block.saturating_add(1);
        let block = st.next_block;
        st.legacy_transfers.push(LegacyTransferRecord {
            from,
            to_account_identifier_hex: bytes_to_hex(&arg.to),
            amount: arg.amount,
            fee: arg.fee,
            memo: arg.memo,
            created_at_time: arg.created_at_time.map(|ts| ts.timestamp_nanos),
            result: "Ok".to_string(),
        });
        block
    }))
}

#[ic_cdk::update]
fn debug_reset() {
    ST.with(|s| {
        *s.borrow_mut() = LedgerState {
            fee_e8s: 10_000,
            ..Default::default()
        };
    });
}

#[ic_cdk::update]
fn debug_set_fee(fee_e8s: u64) {
    ST.with(|s| s.borrow_mut().fee_e8s = fee_e8s);
}

#[ic_cdk::update]
fn debug_set_fee_query_failure(value: bool) {
    ST.with(|s| s.borrow_mut().fee_query_failure = value);
}

#[ic_cdk::update]
fn debug_set_next_error(err: Option<DebugNextTransferError>) {
    ST.with(|s| s.borrow_mut().next_error = err);
}

#[ic_cdk::update]
fn debug_set_error_script(errs: Vec<DebugNextTransferError>) {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.next_error = None;
        st.next_error_script = errs.into();
    });
}

#[ic_cdk::update]
fn debug_accept_then_trap_for_subaccount(subaccount: [u8; 32]) {
    ST.with(|s| s.borrow_mut().accept_then_trap_from_subaccount = Some(subaccount));
}

#[ic_cdk::query]
fn debug_accept_then_trap_subaccount() -> Option<[u8; 32]> {
    ST.with(|s| s.borrow().accept_then_trap_from_subaccount)
}

#[ic_cdk::update]
fn debug_credit(a: Account, amount_e8s: u64) {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        let k = key(&a);
        let b = st.balances.entry(k).or_insert(0);
        *b = b.saturating_add(amount_e8s as u128);
        st.next_block = st.next_block.saturating_add(1);
        let block = st.next_block;
        st.index_transactions.push(TransactionWithId {
            id: Nat::from(block),
            transaction: Transaction::mint(
                Mint {
                    amount: Nat::from(amount_e8s),
                    to: a,
                    memo: None,
                    created_at_time: None,
                    fee: None,
                },
                ic_cdk::api::time(),
            ),
        });
    });
}

#[ic_cdk::update]
fn debug_set_index_source(ledger: Principal) {
    ST.with(|s| s.borrow_mut().index_source_ledger = Some(ledger));
}

#[ic_cdk::update]
fn debug_set_index_hidden_newest_transactions(count: u64) {
    ST.with(|s| s.borrow_mut().index_hidden_newest_transactions = count);
}

#[ic_cdk::query]
fn debug_index_transactions() -> Vec<TransactionWithId> {
    ST.with(|s| s.borrow().index_transactions.clone())
}

#[ic_cdk::query]
fn ledger_id() -> Principal {
    ST.with(|s| {
        s.borrow()
            .index_source_ledger
            .unwrap_or_else(ic_cdk::api::canister_self)
    })
}

fn transaction_touches(transaction: &Transaction, account: &Account) -> bool {
    transaction
        .mint
        .as_ref()
        .is_some_and(|mint| mint.to == *account)
        || transaction
            .burn
            .as_ref()
            .is_some_and(|burn| burn.from == *account)
        || transaction
            .transfer
            .as_ref()
            .is_some_and(|transfer| transfer.from == *account || transfer.to == *account)
        || transaction
            .approve
            .as_ref()
            .is_some_and(|approve| approve.from == *account)
}

fn indexed_balance(transactions: &[TransactionWithId], account: &Account) -> Nat {
    let mut balance = Nat::from(0_u8).0;
    for entry in transactions {
        if let Some(mint) = &entry.transaction.mint {
            if mint.to == *account {
                let fee = mint.fee.clone().unwrap_or_else(|| Nat::from(0_u8));
                if mint.amount.0 >= fee.0 {
                    balance += mint.amount.0.clone() - fee.0;
                }
            }
        } else if let Some(transfer) = &entry.transaction.transfer {
            if transfer.from == *account {
                let fee = transfer.fee.clone().unwrap_or_else(|| Nat::from(0_u8));
                let debit = transfer.amount.0.clone() + fee.0;
                if balance >= debit {
                    balance -= debit;
                }
            }
            if transfer.to == *account {
                balance += transfer.amount.0.clone();
            }
        }
    }
    Nat(balance)
}

#[ic_cdk::update]
async fn get_account_transactions(
    args: GetAccountTransactionsArgs,
) -> GetAccountTransactionsResult {
    let (source, hidden) = ST.with(|s| {
        let st = s.borrow();
        (
            st.index_source_ledger
                .unwrap_or_else(ic_cdk::api::canister_self),
            st.index_hidden_newest_transactions,
        )
    });
    let mut all = if source == ic_cdk::api::canister_self() {
        ST.with(|s| s.borrow().index_transactions.clone())
    } else {
        let response = Call::bounded_wait(source, "debug_index_transactions")
            .change_timeout(30)
            .await
            .map_err(
                |error| jupiter_ic_clients::icrc_index::GetAccountTransactionsError {
                    message: format!("index source call failed: {error:?}"),
                },
            )?;
        response.candid().map_err(|error| {
            jupiter_ic_clients::icrc_index::GetAccountTransactionsError {
                message: format!("index source decode failed: {error:?}"),
            }
        })?
    };
    all.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    let hidden = usize::try_from(hidden).unwrap_or(usize::MAX).min(all.len());
    all.truncate(all.len() - hidden);
    let balance = indexed_balance(&all, &args.account);
    let mut account_transactions = all
        .into_iter()
        .filter(|entry| transaction_touches(&entry.transaction, &args.account))
        .collect::<Vec<_>>();
    let oldest_tx_id = account_transactions.first().map(|entry| entry.id.clone());
    account_transactions.reverse();
    if let Some(start) = args.start {
        account_transactions.retain(|entry| entry.id.0 < start.0);
    }
    let max_results = nat_u64(&args.max_results) as usize;
    account_transactions.truncate(max_results);
    Ok(GetAccountTransactionsResponse {
        balance,
        transactions: account_transactions,
        oldest_tx_id,
    })
}

#[ic_cdk::query]
fn debug_transfers() -> Vec<TransferRecord> {
    ST.with(|s| s.borrow().transfers.clone())
}

#[ic_cdk::query]
fn debug_transfer_attempts() -> Vec<TransferAttemptRecord> {
    ST.with(|s| s.borrow().transfer_attempts.clone())
}

#[ic_cdk::query]
fn debug_legacy_transfers() -> Vec<LegacyTransferRecord> {
    ST.with(|s| s.borrow().legacy_transfers.clone())
}

ic_cdk::export_candid!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_key_distinguishes_absent_and_present_empty_memos() {
        let from = AccountKey {
            owner: Principal::anonymous(),
            sub: None,
        };
        let to = AccountKey {
            owner: Principal::management_canister(),
            sub: None,
        };
        let key_with_memo = |memo| DedupKey {
            from: from.clone(),
            to: to.clone(),
            amount: 100_000_000,
            fee: 10_000,
            memo,
            created_at: 1,
        };
        let absent = key_with_memo(None);
        let present_empty = key_with_memo(Some(Vec::new()));

        assert_ne!(absent, present_empty);
        let mut identities = HashMap::new();
        identities.insert(absent, 1);
        identities.insert(present_empty, 2);
        assert_eq!(identities.len(), 2);
    }
}
