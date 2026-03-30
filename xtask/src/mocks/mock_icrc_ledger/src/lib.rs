use candid::{CandidType, Deserialize, Nat, Principal};
use sha2::{Digest, Sha224};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferArg, TransferError};
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

#[derive(Default)]
struct LedgerState {
    fee_e8s: u64,
    next_error: Option<DebugNextTransferError>,
    next_error_script: VecDeque<DebugNextTransferError>,
    balances: HashMap<AccountKey, u128>,
    next_block: u64,
    dedup: HashMap<DedupKey, u64>,
    transfers: Vec<TransferRecord>,
}

thread_local! {
    static ST: RefCell<LedgerState> = RefCell::new(LedgerState { fee_e8s: 10_000, ..Default::default() });
}

fn nat_u64(n: &Nat) -> u64 {
    n.0.to_u64().unwrap_or(u64::MAX)
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
    let subaccount = a.subaccount.unwrap_or([0u8; 32]);
    let mut hasher = Sha224::new();
    hasher.update(b"\x0Aaccount-id");
    hasher.update(a.owner.as_slice());
    hasher.update(subaccount);
    let hash = hasher.finalize();
    let checksum = crc32fast::hash(&hash).to_be_bytes();
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&checksum);
    bytes[4..].copy_from_slice(&hash);
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
        st.next_error_script.pop_front().or_else(|| st.next_error.take())
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
    let memo_bytes: Vec<u8> = arg
        .memo
        .as_ref()
        .map(|m| m.0.to_vec())
        .unwrap_or_default();

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
            from: from.clone(),
            to: arg.to.clone(),
            amount: Nat::from(amount),
            fee: Nat::from(fee),
            memo: if memo_bytes.is_empty() { None } else { Some(memo_bytes.clone()) },
            created_at_time: arg.created_at_time,
            result: "Ok".to_string(),
        });

        block
    });

    Ok(Nat::from(block))
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

ic_cdk::export_candid!();
