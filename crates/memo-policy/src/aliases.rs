/// Reviewed Event Horizon identity; also the destination of the sole special E. route.
pub const EVENT_HORIZON_CANISTER_ID: &str = "eo6ei-gaaaa-aaaar-qchra-cai";

/// Permanent identity publication, sorted by ASCII alias. Released bindings are
/// add-only: never remove or reassign a name. Publication grants no payment route.
pub const CANISTER_ALIASES: &[(&str, &str)] = &[("E", EVENT_HORIZON_CANISTER_ID)];

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Principal;
    use std::collections::BTreeSet;

    fn validate(entries: &[(&str, &str)]) -> Result<(), &'static str> {
        let mut names = BTreeSet::new();
        for &(name, destination) in entries {
            let bytes = name.as_bytes();
            if !(1..=8).contains(&bytes.len())
                || !bytes[0].is_ascii_uppercase()
                || !bytes
                    .iter()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                return Err("alias grammar");
            }
            if !names.insert(name) {
                return Err("duplicate alias");
            }
            let principal = Principal::from_text(destination).map_err(|_| "invalid principal")?;
            if principal == Principal::anonymous() || principal == Principal::management_canister()
            {
                return Err("reserved principal");
            }
            if crate::parse_declared_principal_text(name).is_some() {
                return Err("direct principal spelling");
            }
        }
        if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err("ASCII order");
        }
        Ok(())
    }

    fn preserves_published(entries: &[(&str, &str)]) -> bool {
        // Release-history evidence, never used as a runtime lookup table.
        let published: Vec<(String, String)> =
            serde_json::from_str(include_str!("../fixtures/published-canister-aliases.json"))
                .unwrap();
        !published.is_empty()
            && published.iter().all(|(name, destination)| {
                entries.iter().any(|&(alias, principal)| {
                    alias == name
                        && Principal::from_text(principal).unwrap()
                            == Principal::from_text(destination).unwrap()
                })
            })
    }

    #[test]
    fn production_table_is_valid_and_preserves_every_published_binding() {
        assert_eq!(validate(CANISTER_ALIASES), Ok(()));
        assert!(preserves_published(CANISTER_ALIASES));
        assert_eq!(CANISTER_ALIASES, &[("E", EVENT_HORIZON_CANISTER_ID)]);
        assert_eq!(
            hex::encode(
                Principal::from_text(EVENT_HORIZON_CANISTER_ID)
                    .unwrap()
                    .as_slice()
            ),
            "00000000023011e20101"
        );
    }

    #[test]
    fn synthetic_tables_reject_invalid_names_destinations_duplicates_and_order() {
        for name in ["", "e", "1A", "A_B", "A-B", "É", "ABCDEFGHI", "A."] {
            assert_eq!(
                validate(&[(name, EVENT_HORIZON_CANISTER_ID)]),
                Err("alias grammar"),
                "{name}"
            );
        }
        assert_eq!(validate(&[("A", "invalid")]), Err("invalid principal"));
        for destination in ["aaaaa-aa", "2vxsx-fae"] {
            assert_eq!(validate(&[("A", destination)]), Err("reserved principal"));
        }
        assert_eq!(
            validate(&[
                ("A", EVENT_HORIZON_CANISTER_ID),
                ("A", EVENT_HORIZON_CANISTER_ID)
            ]),
            Err("duplicate alias")
        );
        assert_eq!(
            validate(&[
                ("Z", EVENT_HORIZON_CANISTER_ID),
                ("A", EVENT_HORIZON_CANISTER_ID)
            ]),
            Err("ASCII order")
        );
        let collision = Principal::from_slice(&[1])
            .to_text()
            .replace('-', "")
            .to_ascii_uppercase();
        assert!(crate::parse_declared_principal_text(&collision).is_some());
        assert_eq!(
            validate(&[(&collision, EVENT_HORIZON_CANISTER_ID)]),
            Err("direct principal spelling")
        );
        assert_eq!(validate(&[("A0B12345", EVENT_HORIZON_CANISTER_ID)]), Ok(()));
    }

    #[test]
    fn publication_is_add_only_and_does_not_authorize_other_payment_prefixes() {
        let added = [
            ("E", EVENT_HORIZON_CANISTER_ID),
            ("F", EVENT_HORIZON_CANISTER_ID),
        ];
        assert_eq!(validate(&added), Ok(()));
        assert!(preserves_published(&added));
        assert!(!preserves_published(&[]));
        assert!(!preserves_published(&[(
            "E",
            "rrkah-fqaaa-aaaaa-aaaaq-cai"
        )]));
        assert!(crate::parse_memo_directive(b"F.suffix").is_none());
    }
}
