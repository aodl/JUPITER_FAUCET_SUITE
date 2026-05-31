use serde::de::DeserializeOwned;

pub fn decode_post_upgrade_args<InitArgs, UpgradeArgs>(
    canister_name: &str,
    raw: &[u8],
) -> Result<Option<UpgradeArgs>, String>
where
    InitArgs: DeserializeOwned + candid::CandidType,
    UpgradeArgs: DeserializeOwned + candid::CandidType,
{
    let zero_args = candid::encode_args(()).expect("failed to encode Candid zero args");
    if raw.is_empty() || raw == zero_args.as_slice() {
        return Ok(None);
    }
    if candid::decode_one::<InitArgs>(raw).is_ok() {
        return Err(format!(
            "received InitArgs in {canister_name} post_upgrade; do not pass install args to upgrade"
        ));
    }
    candid::decode_one::<Option<UpgradeArgs>>(raw)
        .map_err(|err| format!("failed to decode {canister_name} UpgradeArgs: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{encode_args, CandidType, Principal};
    use serde::Deserialize;

    #[derive(CandidType, Deserialize)]
    struct InitArgs {
        controller: Principal,
    }

    #[derive(CandidType, Deserialize, Debug, PartialEq, Eq)]
    struct UpgradeArgs {
        enabled: Option<bool>,
    }

    #[test]
    fn treats_empty_args_as_none() {
        assert!(
            decode_post_upgrade_args::<InitArgs, UpgradeArgs>("demo", &[])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn treats_zero_arg_candid_as_none() {
        let raw = encode_args(()).unwrap();
        assert!(
            decode_post_upgrade_args::<InitArgs, UpgradeArgs>("demo", &raw)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn treats_null_upgrade_args_as_none() {
        let raw = encode_args((Option::<UpgradeArgs>::None,)).unwrap();
        assert!(
            decode_post_upgrade_args::<InitArgs, UpgradeArgs>("demo", &raw)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn decodes_upgrade_args() {
        let raw = encode_args((Some(UpgradeArgs {
            enabled: Some(true),
        }),))
        .unwrap();
        let decoded = decode_post_upgrade_args::<InitArgs, UpgradeArgs>("demo", &raw)
            .unwrap()
            .unwrap();
        assert_eq!(
            decoded,
            UpgradeArgs {
                enabled: Some(true)
            }
        );
    }

    #[test]
    fn rejects_init_args_during_upgrade() {
        let raw = encode_args((InitArgs {
            controller: Principal::anonymous(),
        },))
        .unwrap();
        let err = decode_post_upgrade_args::<InitArgs, UpgradeArgs>("demo", &raw).unwrap_err();
        assert!(err.contains("received InitArgs in demo post_upgrade"));
    }

    #[test]
    fn rejects_malformed_bytes() {
        let err =
            decode_post_upgrade_args::<InitArgs, UpgradeArgs>("demo", b"not candid").unwrap_err();
        assert!(err.contains("failed to decode demo UpgradeArgs"));
    }
}
