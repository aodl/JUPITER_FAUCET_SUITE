use candid::{CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
use jupiter_ic_clients::account_identifier::account_identifier_text;
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
    memo: Vec<u8>,
    created_at: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum DebugNextTransferError {
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
    next_error: Option<DebugNextTransferError>,
    next_error_script: VecDeque<DebugNextTransferError>,
    balances: HashMap<AccountKey, u128>,
    next_block: u64,
    dedup: HashMap<DedupKey, u64>,
    transfers: Vec<TransferRecord>,
    legacy_transfers: Vec<LegacyTransferRecord>,
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
    ST.with(|s| Nat::from(s.borrow().fee_e8s))
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

#[ic_cdk::update]
fn icrc1_transfer(arg: TransferArg) -> Result<BlockIndex, TransferError> {
    // Inject scripted error if set, otherwise a one-shot next error.
    if let Some(err) = ST.with(|s| {
        let mut st = s.borrow_mut();
        st.next_error_script
            .pop_front()
            .or_else(|| st.next_error.take())
    }) {
        return Err(match err {
            DebugNextTransferError::TemporarilyUnavailable => TransferError::TemporarilyUnavailable,
            DebugNextTransferError::TooOld => TransferError::TooOld,
            DebugNextTransferError::CreatedInFuture { ledger_time } => {
                TransferError::CreatedInFuture { ledger_time }
            }
            DebugNextTransferError::BadFee { expected_fee_e8s } => TransferError::BadFee {
                expected_fee: Nat::from(expected_fee_e8s),
            },
            DebugNextTransferError::Duplicate { duplicate_of } => TransferError::Duplicate {
                duplicate_of: Nat::from(duplicate_of),
            },
        });
    }

    // ic-cdk 0.19: caller() deprecated
    let caller = ic_cdk::api::msg_caller();
    let from = Account {
        owner: caller,
        subaccount: arg.from_subaccount,
    };

    let fee_expected = ST.with(|s| s.borrow().fee_e8s);
    let fee = arg.fee.as_ref().map(nat_u64).unwrap_or(fee_expected);

    // BadFee if caller provided fee and it doesn't match expected
    if let Some(provided) = arg.fee.as_ref().map(nat_u64) {
        if provided != fee_expected {
            return Err(TransferError::BadFee {
                expected_fee: Nat::from(fee_expected),
            });
        }
    }

    let amount = nat_u64(&arg.amount);
    let total_debit: u128 = (amount as u128).saturating_add(fee as u128);

    // Memo in icrc-ledger-types is Memo(ByteBuf) => convert to Vec<u8>
    let memo_opt: Option<Vec<u8>> = arg.memo.as_ref().map(|m| m.0.to_vec());
    let memo_bytes: Vec<u8> = memo_opt.clone().unwrap_or_default();

    let created_at = arg.created_at_time.unwrap_or(0);

    // Dedup only when both memo and created_at_time exist and are meaningful
    if !memo_bytes.is_empty() && created_at != 0 {
        let dkey = DedupKey {
            from: key(&from),
            to: key(&arg.to),
            amount,
            fee,
            memo: memo_bytes.clone(),
            created_at,
        };

        if let Some(block) = ST.with(|s| s.borrow().dedup.get(&dkey).cloned()) {
            return Err(TransferError::Duplicate {
                duplicate_of: Nat::from(block),
            });
        }
    }

    let from_key = key(&from);
    let to_key = key(&arg.to);

    let from_bal: u128 = ST.with(|s| *s.borrow().balances.get(&from_key).unwrap_or(&0));
    if from_bal < total_debit {
        return Err(TransferError::InsufficientFunds {
            balance: Nat::from(from_bal),
        });
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
        if !memo_bytes.is_empty() && created_at != 0 {
            let dkey = DedupKey {
                from: from_key.clone(),
                to: to_key.clone(),
                amount,
                fee,
                memo: memo_bytes.clone(),
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

        block
    });

    Ok(Nat::from(block))
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
fn debug_credit(a: Account, amount_e8s: u64) {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        let k = key(&a);
        let b = st.balances.entry(k).or_insert(0);
        *b = b.saturating_add(amount_e8s as u128);
    });
}

#[ic_cdk::query]
fn debug_transfers() -> Vec<TransferRecord> {
    ST.with(|s| s.borrow().transfers.clone())
}

#[ic_cdk::query]
fn debug_legacy_transfers() -> Vec<LegacyTransferRecord> {
    ST.with(|s| s.borrow().legacy_transfers.clone())
}

ic_cdk::export_candid!();
