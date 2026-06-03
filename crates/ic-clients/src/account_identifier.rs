use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use sha2::{Digest, Sha224};

pub fn account_identifier_text(owner: Principal, subaccount: Option<[u8; 32]>) -> String {
    let subaccount = subaccount.unwrap_or([0u8; 32]);
    let mut hasher = Sha224::new();
    hasher.update(b"\x0Aaccount-id");
    hasher.update(owner.as_slice());
    hasher.update(subaccount);
    let hash = hasher.finalize();
    let checksum = crc32fast::hash(&hash).to_be_bytes();
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&checksum);
    bytes[4..].copy_from_slice(&hash);
    hex::encode(bytes)
}

pub fn account_identifier_text_for_account(account: &Account) -> String {
    account_identifier_text(account.owner, account.subaccount)
}

#[cfg(test)]
mod tests {
    use crate::account_identifier::{account_identifier_text, account_identifier_text_for_account};
    use candid::Principal;
    use icrc_ledger_types::icrc1::account::Account;

    #[test]
    fn renders_default_subaccount_account_identifier() {
        let owner = Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("valid principal");

        assert_eq!(
            account_identifier_text(owner, None),
            "f3a58ea11bc128ab8a455dd7bce0a29b0a20f400625d1a46871fbfe82efed38d"
        );
    }

    #[test]
    fn renders_explicit_subaccount_account_identifier() {
        let owner = Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("valid principal");
        let mut subaccount = [0u8; 32];
        subaccount[31] = 1;

        assert_eq!(
            account_identifier_text(owner, Some(subaccount)),
            "439a264f2ce4d3aeeb10b8ad65dc3610512ef3c6c4bc8c2985a15ce8cc2ce3c0"
        );
    }

    #[test]
    fn renders_account_identifier_from_account() {
        let owner = Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").expect("valid principal");
        let account = Account {
            owner,
            subaccount: None,
        };

        assert_eq!(
            account_identifier_text_for_account(&account),
            account_identifier_text(account.owner, account.subaccount)
        );
    }
}
