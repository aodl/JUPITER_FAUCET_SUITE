use candid::Principal;
use icrc_ledger_types::icrc1::account::Subaccount;

pub fn account_identifier_text(owner: Principal, subaccount: Option<Subaccount>) -> String {
    jupiter_ic_clients::account_identifier::account_identifier_text(owner, subaccount)
}
