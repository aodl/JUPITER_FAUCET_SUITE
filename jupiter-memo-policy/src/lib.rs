use candid::Principal;

pub const MAX_TARGET_CANISTER_MEMO_BYTES: usize = 32;
pub const MAX_NEURON_ID_MEMO_BYTES: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoDirective {
    TopUp { canister_id: Principal },
    RawIcp { canister_id: Principal, memo: Vec<u8> },
    NeuronStake { neuron_id: u64, memo: Option<Vec<u8>> },
}

fn principal_text_with_group_separators(text: &str) -> String {
    if text.contains('-') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + text.len() / 5);
    for (idx, ch) in text.chars().enumerate() {
        if idx > 0 && idx % 5 == 0 {
            out.push('-');
        }
        out.push(ch);
    }
    out
}

fn parse_declared_principal_text(text: &str) -> Option<Principal> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_TARGET_CANISTER_MEMO_BYTES {
        return None;
    }
    let normalized = principal_text_with_group_separators(trimmed);
    let principal = Principal::from_text(&normalized).ok()?;
    if principal == Principal::anonymous() || principal == Principal::management_canister() {
        return None;
    }
    Some(principal)
}

fn parse_neuron_id_text(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > MAX_NEURON_ID_MEMO_BYTES {
        return None;
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let neuron_id = text.parse::<u64>().ok()?;
    (neuron_id != 0).then_some(neuron_id)
}

pub fn parse_memo_directive(memo: &[u8]) -> Option<MemoDirective> {
    if memo.is_empty() || memo.len() > MAX_TARGET_CANISTER_MEMO_BYTES || !memo.is_ascii() {
        return None;
    }
    let memo_text = std::str::from_utf8(memo).ok()?;
    let trimmed = memo_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((declared_canister, raw_memo)) = memo_text.split_once('.') {
        if let Some(neuron_id) = parse_neuron_id_text(declared_canister.trim()) {
            return Some(MemoDirective::NeuronStake {
                neuron_id,
                memo: Some(raw_memo.as_bytes().to_vec()),
            });
        }
        return Some(MemoDirective::RawIcp {
            canister_id: parse_declared_principal_text(declared_canister)?,
            memo: raw_memo.as_bytes().to_vec(),
        });
    }
    if let Some(neuron_id) = parse_neuron_id_text(trimmed) {
        return Some(MemoDirective::NeuronStake { neuron_id, memo: None });
    }
    Some(MemoDirective::TopUp {
        canister_id: parse_declared_principal_text(memo_text)?,
    })
}

pub fn parse_target_canister_principal_from_memo(memo: &[u8]) -> Option<Principal> {
    match parse_memo_directive(memo)? {
        MemoDirective::TopUp { canister_id } => Some(canister_id),
        MemoDirective::RawIcp { canister_id, .. } => Some(canister_id),
        MemoDirective::NeuronStake { .. } => None,
    }
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
        let compact_target = target.to_text().replace('-', "");
        let compact_short = short_without_cai.to_text().replace('-', "");
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
                "compact canister principal text",
                compact_target.into_bytes(),
                Some(target),
            ),
            (
                "short valid principal text without hardcoded suffix",
                short_without_cai.to_text().into_bytes(),
                Some(short_without_cai),
            ),
            (
                "compact short principal text",
                compact_short.into_bytes(),
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
            ("numeric neuron id memo", b"123456789".to_vec(), None),
        ];

        for (label, memo, expected) in cases {
            assert_eq!(parse_target_canister_principal_from_memo(&memo), expected, "{label}");
        }
    }

    #[test]
    fn parser_splits_raw_icp_directive_on_first_dot() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        assert_eq!(
            parse_memo_directive(format!("{compact}.swap.7").as_bytes()),
            Some(MemoDirective::RawIcp {
                canister_id: target,
                memo: b"swap.7".to_vec(),
            })
        );
    }

    #[test]
    fn parser_preserves_leading_dot_in_raw_icp_memo_segment_after_first_separator() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        assert_eq!(
            parse_memo_directive(format!("{compact}..memo").as_bytes()),
            Some(MemoDirective::RawIcp {
                canister_id: target,
                memo: b".memo".to_vec(),
            })
        );
    }

    #[test]
    fn parser_preserves_raw_icp_memo_segment_verbatim() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        assert_eq!(
            parse_memo_directive(format!("{compact}. abc ").as_bytes()),
            Some(MemoDirective::RawIcp {
                canister_id: target,
                memo: b" abc ".to_vec(),
            })
        );
    }

    #[test]
    fn parser_accepts_empty_raw_icp_memo_segment() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        assert_eq!(
            parse_memo_directive(format!("{compact}.").as_bytes()),
            Some(MemoDirective::RawIcp {
                canister_id: target,
                memo: Vec::new(),
            })
        );
    }

    #[test]
    fn parser_accepts_raw_icp_directive_at_total_memo_byte_limit() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        let memo = format!("{compact}.12345678");
        assert_eq!(memo.len(), MAX_TARGET_CANISTER_MEMO_BYTES);
        assert_eq!(
            parse_memo_directive(memo.as_bytes()),
            Some(MemoDirective::RawIcp {
                canister_id: target,
                memo: b"12345678".to_vec(),
            })
        );
    }

    #[test]
    fn parser_rejects_raw_icp_directive_over_total_memo_byte_limit() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        let memo = format!("{compact}.123456789");
        assert_eq!(memo.len(), MAX_TARGET_CANISTER_MEMO_BYTES + 1);
        assert_eq!(parse_memo_directive(memo.as_bytes()), None);
    }

    #[test]
    fn parser_rejects_raw_icp_directive_with_empty_declared_canister_segment() {
        assert_eq!(parse_memo_directive(b".memo"), None);
        assert_eq!(parse_memo_directive(b" .memo"), None);
    }

    #[test]
    fn parser_accepts_numeric_neuron_id_directive() {
        assert_eq!(
            parse_memo_directive(b"11614578985374291210"),
            Some(MemoDirective::NeuronStake {
                neuron_id: 11_614_578_985_374_291_210,
                memo: None,
            })
        );
        assert_eq!(
            parse_memo_directive(b" 42\n"),
            Some(MemoDirective::NeuronStake { neuron_id: 42, memo: None })
        );
    }

    #[test]
    fn parser_accepts_neuron_id_directive_with_transfer_memo() {
        assert_eq!(
            parse_memo_directive(b"42.vault.memo"),
            Some(MemoDirective::NeuronStake {
                neuron_id: 42,
                memo: Some(b"vault.memo".to_vec()),
            })
        );
        assert_eq!(
            parse_memo_directive(b"42."),
            Some(MemoDirective::NeuronStake {
                neuron_id: 42,
                memo: Some(Vec::new()),
            })
        );
        assert_eq!(
            parse_memo_directive(b"42..memo"),
            Some(MemoDirective::NeuronStake {
                neuron_id: 42,
                memo: Some(b".memo".to_vec()),
            })
        );
    }

    #[test]
    fn parser_rejects_zero_or_oversize_numeric_neuron_id_directive() {
        assert_eq!(parse_memo_directive(b"0"), None);
        assert_eq!(parse_memo_directive(b"0000"), None);
        assert_eq!(parse_memo_directive(b"18446744073709551616"), None);
        assert_eq!(parse_memo_directive(b"123456789012345678901"), None);
    }
}
