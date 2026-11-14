//! Authenticate only the Relay-to-stake boundary; contributor allocation remains
//! exclusively in the existing immutable Relay distributor.
use super::types::*;
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::account_identifier::account_identifier_bytes;

#[derive(CandidType)]
struct RelayMembershipArgs {
    relay: Principal,
    ledger: Principal,
    staking_account: Account,
    cursor: Option<Vec<u8>>,
    revision: Option<u64>,
}
#[derive(CandidType, Deserialize)]
enum RelayMembership {
    Active { setup_key: Vec<u8> },
    Pending,
    Unsupported,
    Page { cursor: Vec<u8>, revision: u64 },
    RegistryChanged,
    InvalidBinding,
}

pub(super) enum Classification {
    Ready(Option<PatronRoute>),
    Awaiting {
        lookup: Option<RelayLookup>,
        reason: String,
    },
}

pub(super) fn candidate(sender: [u8; 32], memo: &[u8]) -> Option<Principal> {
    let jupiter_memo_policy::MemoDirective::RawIcp {
        canister_id,
        destination_subaccount: None,
        memo: tail,
    } = jupiter_memo_policy::parse_memo_directive(memo)?
    else {
        return None;
    };
    if tail != b"Relay"
        || memo != format!("{}.Relay", canister_id.to_text().replace('-', "")).as_bytes()
        || sender
            != account_identifier_bytes(canister_id, jupiter_memo_policy::numbered_subaccount(1))
    {
        return None;
    }
    Some(canister_id)
}

pub(super) async fn classify(
    config: &ProgrammeConfig,
    block: u64,
    relay: Principal,
    lookup: Option<RelayLookup>,
) -> Classification {
    match classify_inner(config, block, relay, lookup.clone()).await {
        Ok(result) => result,
        Err(reason) => Classification::Awaiting { lookup, reason },
    }
}

async fn classify_inner(
    config: &ProgrammeConfig,
    block: u64,
    relay: Principal,
    lookup: Option<RelayLookup>,
) -> Result<Classification, String> {
    if relay == config.canonical_relay {
        return Ok(Classification::Ready(Some(PatronRoute::RelayPot {
            relay,
            factory_setup_key: None,
        })));
    }
    let Some(factory) = config.relay_factory else {
        // An explicitly canonical-only configuration supports no other Relay.
        // This is a policy decision, not an unavailable/incomplete registry read.
        // A copied memo must not strand its shared commitment cursor forever.
        return Ok(Classification::Ready(None));
    };
    let lookup = lookup.filter(|s| s.block == block && s.relay == relay);
    let response: RelayMembership = Call::bounded_wait(factory, "get_relay_factory_membership")
        .with_arg(RelayMembershipArgs {
            relay,
            ledger: config.icp_ledger,
            staking_account: config.staking_account,
            cursor: lookup.as_ref().map(|s| s.cursor.clone()),
            revision: lookup.map(|s| s.revision),
        })
        .await
        .map_err(|e| format!("Relay factory unavailable: {e:?}"))?
        .candid()
        .map_err(|e| format!("Relay factory membership decode: {e:?}"))?;
    match response {
        RelayMembership::Active { setup_key } if setup_key.len() == 32 => {
            Ok(Classification::Ready(Some(PatronRoute::RelayPot {
                relay,
                factory_setup_key: Some(setup_key),
            })))
        }
        RelayMembership::Page { cursor, revision } if cursor.len() == 32 => {
            Ok(Classification::Awaiting {
                lookup: Some(RelayLookup {
                    block,
                    relay,
                    cursor,
                    revision,
                }),
                reason: "Relay membership pagination incomplete".into(),
            })
        }
        RelayMembership::RegistryChanged => Ok(Classification::Awaiting {
            lookup: None,
            reason: "Relay factory changed; restart bounded membership proof".into(),
        }),
        RelayMembership::Pending => Ok(Classification::Awaiting {
            lookup: None,
            reason: "Relay finalisation requires authoritative Active membership".into(),
        }),
        RelayMembership::Unsupported => Ok(Classification::Ready(None)),
        _ => Err("invalid Relay factory binding or membership response".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_only_policy_finishes_unsupported_candidates_without_an_outcall() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        let config = crate::programme::tests::config();
        assert!(config.relay_factory.is_none());
        let mut context = Context::from_waker(Waker::noop());
        let mut unsupported =
            std::pin::pin!(classify(&config, 1, Principal::from_slice(&[99]), None));
        assert!(matches!(
            unsupported.as_mut().poll(&mut context),
            Poll::Ready(Classification::Ready(None))
        ));
        let mut canonical = std::pin::pin!(classify(&config, 1, config.canonical_relay, None));
        assert!(matches!(
            canonical.as_mut().poll(&mut context),
            Poll::Ready(Classification::Ready(Some(PatronRoute::RelayPot {
                factory_setup_key: None,
                ..
            })))
        ));
    }
    #[test]
    fn exact_relay_source_subaccount_and_memo_are_independent_requirements() {
        let relay = Principal::from_slice(&[1, 1]);
        let other = Principal::from_slice(&[2, 1]);
        let memo = format!("{}.Relay", relay.to_text().replace('-', "")).into_bytes();
        let source = account_identifier_bytes(relay, jupiter_memo_policy::numbered_subaccount(1));
        assert_eq!(candidate(source, &memo), Some(relay));
        for account in [
            account_identifier_bytes(relay, None),
            account_identifier_bytes(relay, jupiter_memo_policy::numbered_subaccount(2)),
            account_identifier_bytes(other, jupiter_memo_policy::numbered_subaccount(1)),
        ] {
            assert_eq!(candidate(account, &memo), None);
        }
        for bad in [
            b"Relay".to_vec(),
            [memo.clone(), b" ".to_vec()].concat(),
            format!("{}.Relay", other.to_text().replace('-', "")).into_bytes(),
        ] {
            assert_eq!(candidate(source, &bad), None);
        }
        // A human's default Account naming Relay in the memo stays direct.
        assert_eq!(
            candidate(account_identifier_bytes(other, None), &memo),
            None
        );
        // Authenticating these bytes is necessary, but factory membership is
        // still required before the arbitrary canister below may receive a pot.
        let other_memo = format!("{}.Relay", other.to_text().replace('-', "")).into_bytes();
        assert_eq!(
            candidate(
                account_identifier_bytes(other, jupiter_memo_policy::numbered_subaccount(1)),
                &other_memo
            ),
            Some(other)
        );
    }
}
