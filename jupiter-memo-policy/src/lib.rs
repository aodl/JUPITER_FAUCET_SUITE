use candid::Principal;

pub const MAX_TARGET_CANISTER_MEMO_BYTES: usize = 32;

pub fn parse_target_canister_principal_from_memo(memo: &[u8]) -> Option<Principal> {
    if memo.is_empty() || memo.len() > MAX_TARGET_CANISTER_MEMO_BYTES || !memo.is_ascii() {
        return None;
    }
    let memo_text = std::str::from_utf8(memo).ok()?.trim();
    if memo_text.is_empty() || memo_text.len() > MAX_TARGET_CANISTER_MEMO_BYTES {
        return None;
    }
    let principal = Principal::from_text(memo_text).ok()?;
    if principal == Principal::anonymous() || principal == Principal::management_canister() {
        return None;
    }
    Some(principal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(s: &str) -> Principal { Principal::from_text(s).unwrap() }
    fn target_canister() -> Principal { principal("22255-zqaaa-aaaas-qf6uq-cai") }

    #[test]
    fn parser_policy_corpus() {
        let target = target_canister();
        let short_without_cai = Principal::from_slice(&[1]);
        let oversize_self_auth = Principal::from_text(
            "33mql-r6bnm-7mzbp-gqvmp-iv6qr-5j3pw-tnwsf-f2az7-zppun-yb4lf-zae",
        )
        .unwrap();
        assert!(oversize_self_auth.to_text().len() > MAX_TARGET_CANISTER_MEMO_BYTES);

        let whitespace_padded = format!("  {}\n", target.to_text());
        let whitespace_only = b"  \n\t".to_vec();
        let non_ascii = vec![0xff; 64];
        let truncated_target_text = target.to_text();
        let truncated_target = truncated_target_text[..truncated_target_text.len().saturating_sub(1)]
            .as_bytes()
            .to_vec();

        let cases: Vec<(&str, Vec<u8>, Option<Principal>)> = vec![
            ("valid target principal text", target.to_text().into_bytes(), Some(target)),
            (
                "whitespace padded principal text",
                whitespace_padded.into_bytes(),
                Some(target),
            ),
            (
                "short valid principal text without hardcoded suffix",
                short_without_cai.to_text().into_bytes(),
                Some(short_without_cai),
            ),
            ("empty memo", Vec::new(), None),
            ("whitespace only memo", whitespace_only, None),
            ("malformed ASCII principal text", b"not-a-principal".to_vec(), None),
            ("truncated principal text", truncated_target, None),
            ("non ASCII bytes", non_ascii, None),
            (
                "oversize valid principal text",
                oversize_self_auth.to_text().into_bytes(),
                None,
            ),
            (
                "anonymous principal text",
                Principal::anonymous().to_text().into_bytes(),
                None,
            ),
            (
                "management canister principal text",
                Principal::management_canister().to_text().into_bytes(),
                None,
            ),
        ];

        for (label, memo, expected) in cases {
            assert_eq!(parse_target_canister_principal_from_memo(&memo), expected, "{label}");
        }
    }
}
