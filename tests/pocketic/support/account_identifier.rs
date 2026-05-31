use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::account::Subaccount;

pub fn account_identifier_text(owner: Principal, subaccount: Option<Subaccount>) -> String {
    jupiter_ic_clients::account_identifier::account_identifier_text(owner, subaccount)
}

pub fn account_id_for(account: &Account) -> String {
    jupiter_ic_clients::account_identifier::account_identifier_text_for_account(account)
}

pub fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    jupiter_ic_clients::account::principal_to_subaccount(principal)
}
