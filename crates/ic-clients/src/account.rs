//! Shared account formatting and deterministic subaccount helpers.
//!
//! This module owns reusable account text formatting and principal-derived
//! subaccount encoding. Canister-specific account selection and payout policy
//! should remain local to each canister.

use candid::Principal;
use icrc_ledger_types::icrc1::account::Account;
use sha2::{Digest, Sha256};

pub const RELAY_SETUP_SUBACCOUNT_DOMAIN: &[u8] = b"jupiter-relay-setup-v1";

pub fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    let bytes = principal.as_slice();
    let mut out = [0u8; 32];
    out[0] = bytes.len() as u8;
    let len = bytes.len().min(31);
    out[1..1 + len].copy_from_slice(&bytes[..len]);
    out
}

pub fn relay_setup_subaccount(target: Principal) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(RELAY_SETUP_SUBACCOUNT_DOMAIN);
    hasher.update(target.as_slice());
    hasher.finalize().into()
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
    use super::{account_text, principal_to_subaccount, relay_setup_subaccount, subaccount_text};
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

    #[test]
    fn relay_setup_subaccount_is_domain_separated_sha256() {
        let principal = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let subaccount = relay_setup_subaccount(principal);

        assert_eq!(
            hex::encode(subaccount),
            "9008ebda9c222b8ca7a187b58876c9c5ce11ec50eb413da2c1ab1b8f71447312"
        );
        assert_ne!(subaccount, principal_to_subaccount(principal));
    }
}
