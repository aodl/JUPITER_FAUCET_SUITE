use candid::Principal;

pub const MAX_TARGET_CANISTER_MEMO_BYTES: usize = 32;
pub const MAX_NEURON_ID_MEMO_BYTES: usize = 20;
/// Maximum exact-byte memo carried by an immutable Relay surplus recipient.
pub const MAX_RELAY_SURPLUS_MEMO_BYTES: usize = 32;
mod aliases;
pub use aliases::{CANISTER_ALIASES, EVENT_HORIZON_CANISTER_ID};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoDirective {
    TopUp {
        canister_id: Principal,
    },
    RawIcp {
        canister_id: Principal,
        destination_subaccount: Option<[u8; 32]>,
        memo: Vec<u8>,
    },
    NeuronStake {
        neuron_id: u64,
        memo: Option<Vec<u8>>,
    },
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

pub fn numbered_subaccount(number: u64) -> Option<[u8; 32]> {
    if number == 0 {
        return None;
    }
    let mut subaccount = [0; 32];
    subaccount[24..].copy_from_slice(&number.to_be_bytes());
    Some(subaccount)
}

pub fn numbered_subaccount_number(subaccount: &[u8; 32]) -> Result<Option<u64>, &'static str> {
    if subaccount[..24].iter().any(|byte| *byte != 0) {
        return Err("subaccount is not a zero-padded u64");
    }
    let mut number = [0; 8];
    number.copy_from_slice(&subaccount[24..]);
    Ok((u64::from_be_bytes(number) != 0).then_some(u64::from_be_bytes(number)))
}

fn parse_numbered_subaccount_text(text: &str) -> Option<Option<[u8; 32]>> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(numbered_subaccount(text.parse::<u64>().ok()?))
}

fn parse_raw_destination_text(text: &str) -> Option<(Principal, Option<[u8; 32]>)> {
    let destination = text.trim();
    if let Some((owner_text, number_text)) = destination.split_once(':') {
        if owner_text.is_empty()
            || number_text.is_empty()
            || number_text.contains(':')
            || destination.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            return None;
        }
        return Some((
            parse_declared_principal_text(owner_text)?,
            parse_numbered_subaccount_text(number_text)?,
        ));
    }
    Some((parse_declared_principal_text(destination)?, None))
}

pub fn parse_memo_directive(memo: &[u8]) -> Option<MemoDirective> {
    if memo.is_empty() || memo.len() > MAX_TARGET_CANISTER_MEMO_BYTES || !memo.is_ascii() {
        return None;
    }
    let memo_text = std::str::from_utf8(memo).ok()?;
    if let Some(suffix) = memo_text.strip_prefix("E.") {
        if suffix.is_empty() {
            return None;
        }
        return Some(MemoDirective::RawIcp {
            canister_id: Principal::from_text(EVENT_HORIZON_CANISTER_ID)
                .expect("invalid hardcoded Event Horizon principal"),
            destination_subaccount: None,
            memo: suffix.as_bytes().to_vec(),
        });
    }
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
        let (canister_id, destination_subaccount) = parse_raw_destination_text(declared_canister)?;
        return Some(MemoDirective::RawIcp {
            canister_id,
            destination_subaccount,
            memo: raw_memo.as_bytes().to_vec(),
        });
    }
    if let Some(neuron_id) = parse_neuron_id_text(trimmed) {
        return Some(MemoDirective::NeuronStake {
            neuron_id,
            memo: None,
        });
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
    use crate::{
        numbered_subaccount, numbered_subaccount_number, parse_memo_directive,
        parse_target_canister_principal_from_memo, MemoDirective, EVENT_HORIZON_CANISTER_ID,
        MAX_TARGET_CANISTER_MEMO_BYTES,
    };
    use candid::Principal;
    use serde::Deserialize;

    fn principal(s: &str) -> Principal {
        Principal::from_text(s).unwrap()
    }
    fn target_canister() -> Principal {
        principal("22255-zqaaa-aaaas-qf6uq-cai")
    }

    #[derive(Debug, Deserialize)]
    struct FixtureCase {
        name: String,
        memo_text: Option<String>,
        memo_hex: Option<String>,
        expected_directive: Option<ExpectedDirective>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum ExpectedDirective {
        TopUp {
            canister_id: String,
        },
        RawIcp {
            canister_id: String,
            memo_text: String,
            #[serde(default)]
            destination_subaccount_hex: Option<String>,
        },
        NeuronStake {
            neuron_id: u64,
            memo_text: Option<String>,
        },
    }

    fn fixture_memo(case: &FixtureCase) -> Vec<u8> {
        if let Some(hex) = &case.memo_hex {
            return hex::decode(hex).expect("valid memo_hex fixture");
        }
        case.memo_text
            .as_deref()
            .unwrap_or_default()
            .as_bytes()
            .to_vec()
    }

    fn expected_directive(directive: &Option<ExpectedDirective>) -> Option<MemoDirective> {
        match directive {
            Some(ExpectedDirective::TopUp { canister_id }) => Some(MemoDirective::TopUp {
                canister_id: principal(canister_id),
            }),
            Some(ExpectedDirective::RawIcp {
                canister_id,
                memo_text,
                destination_subaccount_hex,
            }) => Some(MemoDirective::RawIcp {
                canister_id: principal(canister_id),
                destination_subaccount: destination_subaccount_hex.as_ref().map(|hex| {
                    hex::decode(hex)
                        .expect("valid destination subaccount hex")
                        .try_into()
                        .expect("32-byte destination subaccount")
                }),
                memo: memo_text.as_bytes().to_vec(),
            }),
            Some(ExpectedDirective::NeuronStake {
                neuron_id,
                memo_text,
            }) => Some(MemoDirective::NeuronStake {
                neuron_id: *neuron_id,
                memo: memo_text.as_ref().map(|text| text.as_bytes().to_vec()),
            }),
            None => None,
        }
    }

    #[test]
    fn parser_matches_canonical_fixture_corpus() {
        let cases: Vec<FixtureCase> =
            serde_json::from_str(include_str!("../fixtures/memo-policy-cases.json"))
                .expect("valid memo policy fixture corpus");
        for case in cases {
            assert_eq!(
                parse_memo_directive(&fixture_memo(&case)),
                expected_directive(&case.expected_directive),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn parser_policy_corpus() {
        let target = target_canister();
        let short_without_cai = Principal::from_slice(&[1]);
        let oversize_self_auth =
            Principal::from_text("33mql-r6bnm-7mzbp-gqvmp-iv6qr-5j3pw-tnwsf-f2az7-zppun-yb4lf-zae")
                .unwrap();
        assert!(oversize_self_auth.to_text().len() > MAX_TARGET_CANISTER_MEMO_BYTES);

        let whitespace_padded = format!("  {}\n", target.to_text());
        let compact_target = target.to_text().replace('-', "");
        let compact_short = short_without_cai.to_text().replace('-', "");
        let whitespace_only = b"  \n\t".to_vec();
        let non_ascii = vec![0xff; 64];
        let truncated_target_text = target.to_text();
        let truncated_target = truncated_target_text.as_bytes()
            [..truncated_target_text.len().saturating_sub(1)]
            .to_vec();

        let cases: Vec<(&str, Vec<u8>, Option<Principal>)> = vec![
            (
                "valid declared canister ID text",
                target.to_text().into_bytes(),
                Some(target),
            ),
            (
                "whitespace padded principal text",
                whitespace_padded.into_bytes(),
                Some(target),
            ),
            (
                "compact declared canister ID text",
                compact_target.into_bytes(),
                Some(target),
            ),
            (
                "short valid principal text accepted by parser policy",
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
            (
                "malformed ASCII principal text",
                b"not-a-principal".to_vec(),
                None,
            ),
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
            assert_eq!(
                parse_target_canister_principal_from_memo(&memo),
                expected,
                "{label}"
            );
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
                destination_subaccount: None,
                memo: b"swap.7".to_vec(),
            })
        );
    }

    #[test]
    fn event_horizon_alias_is_exact_bounded_and_preserves_suffix() {
        let event_horizon = principal(EVENT_HORIZON_CANISTER_ID);
        for memo in [
            "E.acjuzliaaaaaaarqb4qqcai.0:1",
            "E.uccpicqaaaaaaarqby3qcai.0",
            "E.uccpicqaaaaaaarqby3qcai.n0:1",
            "E.u2qkpaqaaaaaaarqb7eacai.0-90",
            "E.foo.bar",
            "E.suffix:1",
        ] {
            let suffix = memo.strip_prefix("E.").unwrap();
            assert_eq!(
                parse_memo_directive(memo.as_bytes()),
                Some(MemoDirective::RawIcp {
                    canister_id: event_horizon,
                    destination_subaccount: None,
                    memo: suffix.as_bytes().to_vec(),
                })
            );
            assert_eq!(
                parse_target_canister_principal_from_memo(memo.as_bytes()),
                Some(event_horizon)
            );
        }
        for invalid in [
            b"X.foo".as_slice(),
            b"E.",
            b"e.foo",
            b"E",
            b"E:1.foo",
            b"E:.foo",
            b"E:0",
            b"F.foo",
        ] {
            assert!(parse_memo_directive(invalid).is_none());
        }
        assert!(parse_memo_directive(b"e.foo").is_none());
        assert!(parse_memo_directive(b"E").is_none());
        assert!(parse_memo_directive(b"E.123456789012345678901234567890").is_some());
        assert!(parse_memo_directive(b"E.1234567890123456789012345678901").is_none());
    }

    #[test]
    fn parser_preserves_leading_dot_in_raw_icp_memo_segment_after_first_separator() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        assert_eq!(
            parse_memo_directive(format!("{compact}..memo").as_bytes()),
            Some(MemoDirective::RawIcp {
                canister_id: target,
                destination_subaccount: None,
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
                destination_subaccount: None,
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
                destination_subaccount: None,
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
                destination_subaccount: None,
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
    fn numbered_subaccount_helpers_use_zero_padded_big_endian_u64() {
        assert_eq!(numbered_subaccount(0), None);
        for number in [1, 50, 255, 256, u64::MAX] {
            let subaccount = numbered_subaccount(number).expect("non-zero subaccount");
            assert_eq!(&subaccount[..24], &[0; 24]);
            assert_eq!(&subaccount[24..], &number.to_be_bytes());
            assert_eq!(numbered_subaccount_number(&subaccount), Ok(Some(number)));
        }
        assert_eq!(numbered_subaccount_number(&[0; 32]), Ok(None));
        let mut unsupported = [0; 32];
        unsupported[0] = 1;
        assert!(numbered_subaccount_number(&unsupported).is_err());
        for invalid in ["", "+1", "-1", "1.0", "1e2", "0x1", "١"] {
            assert_eq!(
                super::parse_numbered_subaccount_text(invalid),
                None,
                "{invalid}"
            );
        }
    }

    #[test]
    fn parser_normalizes_numbered_raw_icp_destinations_and_preserves_suffix() {
        let target = target_canister();
        let compact = target.to_text().replace('-', "");
        for (number_text, expected_number, suffix) in [
            ("0", None, ""),
            ("000", None, "a:1"),
            ("1", Some(1), "a.b"),
            ("50", Some(50), " d "),
            ("255", Some(255), "x"),
            ("256", Some(256), ""),
        ] {
            assert_eq!(
                parse_memo_directive(format!("{compact}:{number_text}.{suffix}").as_bytes()),
                Some(MemoDirective::RawIcp {
                    canister_id: target,
                    destination_subaccount: expected_number.and_then(numbered_subaccount),
                    memo: suffix.as_bytes().to_vec(),
                })
            );
        }
    }

    #[test]
    fn parser_rejects_invalid_numbered_raw_icp_destinations() {
        let compact = target_canister().to_text().replace('-', "");
        for declaration in [
            format!("{compact}:1"),
            format!("{compact}:.memo"),
            format!("{compact}:1:2.memo"),
            format!("{compact}:+1.memo"),
            format!("{compact}:-1.memo"),
            format!("{compact}:0x1.memo"),
            format!("{compact}:1e2.memo"),
            format!("{compact}: 1.memo"),
            format!("{compact} :1.memo"),
            format!("{compact}:18446744073709551616.memo"),
            "42:1.memo".to_string(),
            "X:1.suffix".to_string(),
        ] {
            assert_eq!(
                parse_memo_directive(declaration.as_bytes()),
                None,
                "{declaration}"
            );
        }
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
            Some(MemoDirective::NeuronStake {
                neuron_id: 42,
                memo: None
            })
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
    fn parser_preserves_all_exact_commitment_route_forms() {
        let canister = target_canister();
        let canister_text = canister.to_text();
        let top_up = parse_memo_directive(canister_text.as_bytes()).unwrap();
        let raw_empty = parse_memo_directive(format!("{canister_text}.").as_bytes()).unwrap();
        let raw_non_empty =
            parse_memo_directive(format!("{canister_text}.abc").as_bytes()).unwrap();
        let neuron_none = parse_memo_directive(b"12345").unwrap();
        let neuron_empty = parse_memo_directive(b"12345.").unwrap();
        let neuron_non_empty = parse_memo_directive(b"12345.abc").unwrap();

        assert_eq!(
            top_up,
            MemoDirective::TopUp {
                canister_id: canister
            }
        );
        assert_eq!(
            raw_empty,
            MemoDirective::RawIcp {
                canister_id: canister,
                destination_subaccount: None,
                memo: Vec::new(),
            }
        );
        assert_eq!(
            raw_non_empty,
            MemoDirective::RawIcp {
                canister_id: canister,
                destination_subaccount: None,
                memo: b"abc".to_vec(),
            }
        );
        assert_eq!(
            neuron_none,
            MemoDirective::NeuronStake {
                neuron_id: 12_345,
                memo: None,
            }
        );
        assert_eq!(
            neuron_empty,
            MemoDirective::NeuronStake {
                neuron_id: 12_345,
                memo: Some(Vec::new()),
            }
        );
        assert_eq!(
            neuron_non_empty,
            MemoDirective::NeuronStake {
                neuron_id: 12_345,
                memo: Some(b"abc".to_vec()),
            }
        );
        assert_ne!(top_up, raw_empty);
        assert_ne!(neuron_none, neuron_empty);
    }

    #[test]
    fn parser_rejects_zero_or_oversize_numeric_neuron_id_directive() {
        assert_eq!(parse_memo_directive(b"0"), None);
        assert_eq!(parse_memo_directive(b"0000"), None);
        assert_eq!(parse_memo_directive(b"18446744073709551616"), None);
        assert_eq!(parse_memo_directive(b"123456789012345678901"), None);
    }
}
