use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;

pub fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    let bytes = principal.as_slice();
    let mut out = [0u8; 32];
    out[0] = bytes.len() as u8;
    let len = bytes.len().min(31);
    out[1..1 + len].copy_from_slice(&bytes[..len]);
    out
}

pub fn subaccount_text(subaccount: &Option<[u8; 32]>) -> String {
    let Some(bytes) = subaccount else {
        return "none".to_string();
    };

    hex::encode(bytes)
}

pub fn account_text(account: &Account) -> String {
    format!(
        "{}:{}",
        account.owner.to_text(),
        subaccount_text(&account.subaccount)
    )
}

#[cfg(test)]
mod tests {
    use super::{account_text, principal_to_subaccount, subaccount_text};
    use candid::Principal;
    use icrc_ledger_types::icrc1::account::Account;

    #[test]
    fn principal_to_subaccount_encodes_length_and_bytes() {
        let principal = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let subaccount = principal_to_subaccount(principal);

        assert_eq!(subaccount[0], principal.as_slice().len() as u8);
        assert_eq!(
            &subaccount[1..1 + principal.as_slice().len()],
            principal.as_slice()
        );
        assert!(subaccount[1 + principal.as_slice().len()..]
            .iter()
            .all(|b| *b == 0));
    }

    #[test]
    fn formats_accounts_with_existing_none_and_hex_contract() {
        let owner = Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").unwrap();
        let account = Account {
            owner,
            subaccount: None,
        };
        assert_eq!(subaccount_text(&None), "none");
        assert_eq!(account_text(&account), "qaa6y-5yaaa-aaaaa-aaafa-cai:none");

        let account = Account {
            owner,
            subaccount: Some([7u8; 32]),
        };
        assert_eq!(
            account_text(&account),
            "qaa6y-5yaaa-aaaaa-aaafa-cai:0707070707070707070707070707070707070707070707070707070707070707"
        );
    }
}
