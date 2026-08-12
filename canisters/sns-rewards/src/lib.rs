mod clients;
mod logging;
mod policy;
mod scheduler;
mod state;

use candid::{CandidType, Deserialize, Principal};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct InitArgs {
    pub reward_sns_root_canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize, Clone, Default, Debug, PartialEq, Eq)]
pub struct UpgradeArgs {
    pub reward_sns_root_canister_id: Option<Option<Principal>>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RelayRewardContext {
    pub sns_root_canister_id: Principal,
    pub sns_governance_canister_id: Principal,
    pub sns_ledger_canister_id: Principal,
    pub snapshot_id: u64,
    pub scan_started_at_timestamp_nanos: u64,
    pub scan_completed_at_timestamp_nanos: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ResolveDefaultIcpAccountsArgs {
    pub snapshot_id: u64,
    pub account_identifiers: Vec<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ResolveDefaultIcpAccountsResult {
    Ok(Vec<Option<Principal>>),
    SnapshotChanged,
    TooManyAccounts,
    InvalidAccountIdentifier { index: u32 },
}

#[cfg(feature = "debug_api")]
fn production_canister_id() -> Principal {
    Principal::from_text(env!("JUPITER_SNS_REWARDS_PROD_CANISTER_ID"))
        .expect("production principal")
}

#[cfg(feature = "debug_api")]
fn guard_debug_api_not_production() {
    if ic_cdk::api::canister_self() == production_canister_id() {
        ic_cdk::trap("debug_api is disabled for the production canister");
    }
}

fn context_from_state() -> Option<RelayRewardContext> {
    state::with_state(|st| {
        let configured_root = st.config.reward_sns_root_canister_id?;
        let snapshot = st.active_snapshot.as_ref()?;
        if snapshot.sns_root_canister_id != configured_root {
            return None;
        }
        Some(RelayRewardContext {
            sns_root_canister_id: snapshot.sns_root_canister_id,
            sns_governance_canister_id: snapshot.sns_governance_canister_id,
            sns_ledger_canister_id: snapshot.sns_ledger_canister_id,
            snapshot_id: snapshot.snapshot_id,
            scan_started_at_timestamp_nanos: snapshot.scan_started_at_timestamp_nanos,
            scan_completed_at_timestamp_nanos: snapshot.scan_completed_at_timestamp_nanos,
        })
    })
}

#[ic_cdk::query]
fn get_relay_reward_context() -> Option<RelayRewardContext> {
    context_from_state()
}

#[ic_cdk::query]
fn resolve_default_icp_accounts(
    args: ResolveDefaultIcpAccountsArgs,
) -> ResolveDefaultIcpAccountsResult {
    if args.account_identifiers.len() > policy::MAX_ACCOUNT_LOOKUPS_PER_CALL {
        return ResolveDefaultIcpAccountsResult::TooManyAccounts;
    }
    for (index, identifier) in args.account_identifiers.iter().enumerate() {
        if identifier.len() != 32 {
            return ResolveDefaultIcpAccountsResult::InvalidAccountIdentifier {
                index: index as u32,
            };
        }
    }
    let Some(snapshot) = state::with_state(|st| st.active_snapshot.clone()) else {
        return ResolveDefaultIcpAccountsResult::SnapshotChanged;
    };
    if snapshot.snapshot_id != args.snapshot_id {
        return ResolveDefaultIcpAccountsResult::SnapshotChanged;
    }
    ResolveDefaultIcpAccountsResult::Ok(
        args.account_identifiers
            .into_iter()
            .map(|identifier| {
                state::lookup(
                    snapshot.active_slot,
                    identifier.try_into().expect("validated account identifier"),
                )
            })
            .collect(),
    )
}

fn initialize(config: state::Config, event: &str) {
    state::initialize(config);
    scheduler::install_timers();
    logging::lifecycle(event);
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    initialize(
        state::Config {
            reward_sns_root_canister_id: args.reward_sns_root_canister_id,
        },
        "init_complete",
    );
}

fn decode_post_upgrade_args_from_bytes(raw: &[u8]) -> Result<Option<UpgradeArgs>, String> {
    jupiter_ic_clients::lifecycle::decode_post_upgrade_args::<InitArgs, UpgradeArgs>(
        "sns-rewards",
        raw,
    )
}

fn decode_post_upgrade_args(raw: Vec<u8>) -> Option<UpgradeArgs> {
    decode_post_upgrade_args_from_bytes(&raw).unwrap_or_else(|err| ic_cdk::trap(&err))
}

#[ic_cdk::post_upgrade(decode_with = "decode_post_upgrade_args")]
fn post_upgrade(args: Option<UpgradeArgs>) {
    let mut restored =
        state::restore().unwrap_or_else(|| state::SnsRewardsState::new(state::Config::default()));
    restored.scan_lock_state_ts = Some(0);
    state::set_state(restored);
    if let Some(Some(new_root)) = args.map(|value| value.reward_sns_root_canister_id) {
        if state::with_state(|st| st.config.reward_sns_root_canister_id) != new_root {
            state::invalidate_for_root(new_root);
            ic_cdk::println!("SNS_REWARDS_CONFIG status=root_changed_invalidated");
        }
    }
    scheduler::install_timers();
    logging::lifecycle("post_upgrade_complete");
}

#[cfg(feature = "debug_api")]
#[derive(CandidType, Deserialize)]
pub(crate) struct DebugState {
    pub configured_root: Option<Principal>,
    pub scan_active: bool,
    pub scan_cursor: Option<Vec<u8>>,
    pub pages_processed: u32,
    pub neurons_processed: u64,
    pub active_snapshot: Option<state::OwnerSnapshot>,
    pub active_slot_size: u64,
    pub staging_slot_size: u64,
}

#[cfg(feature = "debug_api")]
#[ic_cdk::update]
async fn debug_scan_tick() {
    guard_debug_api_not_production();
    scheduler::scan_tick(true).await;
}

#[cfg(feature = "debug_api")]
#[ic_cdk::query]
fn debug_state() -> DebugState {
    guard_debug_api_not_production();
    state::with_state(|st| {
        let active_slot = st
            .active_snapshot
            .as_ref()
            .map(|snapshot| snapshot.active_slot);
        let staging_slot = st.scan.as_ref().map(|scan| scan.staging_slot);
        DebugState {
            configured_root: st.config.reward_sns_root_canister_id,
            scan_active: st.scan.is_some(),
            scan_cursor: st.scan.as_ref().and_then(|scan| scan.start_page_at.clone()),
            pages_processed: st
                .scan
                .as_ref()
                .map(|scan| scan.pages_processed)
                .unwrap_or(0),
            neurons_processed: st
                .scan
                .as_ref()
                .map(|scan| scan.neurons_processed)
                .unwrap_or(0),
            active_snapshot: st.active_snapshot.clone(),
            active_slot_size: active_slot.map(state::slot_len).unwrap_or(0),
            staging_slot_size: staging_slot.map(state::slot_len).unwrap_or(0),
        }
    })
}

ic_cdk::export_candid!();

#[cfg(test)]
mod tests {
    use super::*;
    use candid::{decode_args, encode_args, Principal};
    use candid_parser::parse_idl_args;
    use candid_parser::utils::{instantiate_candid, service_equal, CandidSource};
    use jupiter_ic_clients::account_identifier::account_identifier_bytes;
    use std::path::Path;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn snapshot(slot: state::OwnerIndexSlot, root: Principal) -> state::OwnerSnapshot {
        state::OwnerSnapshot {
            snapshot_id: 7,
            active_slot: slot,
            sns_root_canister_id: root,
            sns_governance_canister_id: principal(2),
            sns_ledger_canister_id: principal(3),
            scan_started_at_timestamp_nanos: 10,
            scan_completed_at_timestamp_nanos: 20,
            neuron_count: 1,
            owner_count: 1,
        }
    }

    fn assert_committed_did_matches_rust_service(did_file: &str) {
        let did_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(did_file);
        service_equal(
            CandidSource::Text(&__export_service()),
            CandidSource::File(&did_path),
        )
        .unwrap_or_else(|err| {
            panic!("committed SNS rewards DID {did_file} diverged from Rust service: {err}")
        });
    }

    #[cfg(not(feature = "debug_api"))]
    #[test]
    fn committed_production_did_matches_rust_service() {
        assert_committed_did_matches_rust_service("jupiter_sns_rewards.did");
    }

    #[cfg(feature = "debug_api")]
    #[test]
    fn committed_debug_did_matches_rust_service() {
        assert_committed_did_matches_rust_service("jupiter_sns_rewards_debug.did");
    }

    #[test]
    fn mainnet_install_args_preflight_uses_openchat_root() {
        let did = include_str!("../jupiter_sns_rewards.did");
        let install_args = include_str!("../mainnet-install-args.did");
        let (init_types, (env, _)) = instantiate_candid(CandidSource::Text(did)).unwrap();
        let parsed = parse_idl_args(install_args).unwrap();
        let bytes = parsed.to_bytes_with_types(&env, &init_types).unwrap();
        let (args,): (InitArgs,) = decode_args(&bytes).unwrap();
        assert_eq!(
            args.reward_sns_root_canister_id,
            Some(Principal::from_text("3e3x2-xyaaa-aaaaq-aaalq-cai").unwrap())
        );
    }

    #[test]
    fn context_and_lookup_only_use_active_snapshot() {
        let root = principal(1);
        let active_owner = principal(4);
        let staging_owner = principal(5);
        state::reset_for_test(state::Config {
            reward_sns_root_canister_id: Some(root),
        });
        state::insert_owner(state::OwnerIndexSlot::A, active_owner);
        state::insert_owner(state::OwnerIndexSlot::B, staging_owner);
        state::with_state_mut(|st| {
            st.active_snapshot = Some(snapshot(state::OwnerIndexSlot::A, root));
            st.scan = Some(state::OwnerScan {
                staging_slot: state::OwnerIndexSlot::B,
                sns_root_canister_id: root,
                sns_governance_canister_id: principal(2),
                sns_ledger_canister_id: principal(3),
                scan_started_at_timestamp_nanos: 30,
                start_page_at: None,
                pages_processed: 0,
                neurons_processed: 0,
            });
        });
        assert_eq!(context_from_state().unwrap().snapshot_id, 7);
        let result = resolve_default_icp_accounts(ResolveDefaultIcpAccountsArgs {
            snapshot_id: 7,
            account_identifiers: vec![
                account_identifier_bytes(active_owner, None).to_vec(),
                account_identifier_bytes(staging_owner, None).to_vec(),
            ],
        });
        assert_eq!(
            result,
            ResolveDefaultIcpAccountsResult::Ok(vec![Some(active_owner), None])
        );
    }

    #[test]
    fn lookup_validates_snapshot_count_and_lengths() {
        state::reset_for_test(state::Config::default());
        assert_eq!(
            resolve_default_icp_accounts(ResolveDefaultIcpAccountsArgs {
                snapshot_id: 1,
                account_identifiers: vec![]
            }),
            ResolveDefaultIcpAccountsResult::SnapshotChanged
        );
        assert_eq!(
            resolve_default_icp_accounts(ResolveDefaultIcpAccountsArgs {
                snapshot_id: 1,
                account_identifiers: vec![vec![0; 31]]
            }),
            ResolveDefaultIcpAccountsResult::InvalidAccountIdentifier { index: 0 }
        );
        assert_eq!(
            resolve_default_icp_accounts(ResolveDefaultIcpAccountsArgs {
                snapshot_id: 1,
                account_identifiers: vec![vec![0; 32]; 129]
            }),
            ResolveDefaultIcpAccountsResult::TooManyAccounts
        );
    }

    #[test]
    fn root_change_clears_snapshot_scan_and_both_maps() {
        let old = principal(1);
        let new = principal(2);
        state::reset_for_test(state::Config {
            reward_sns_root_canister_id: Some(old),
        });
        state::insert_owner(state::OwnerIndexSlot::A, principal(3));
        state::insert_owner(state::OwnerIndexSlot::B, principal(4));
        state::with_state_mut(|st| {
            st.active_snapshot = Some(snapshot(state::OwnerIndexSlot::A, old))
        });
        state::invalidate_for_root(Some(new));
        assert_eq!(state::slot_len(state::OwnerIndexSlot::A), 0);
        assert_eq!(state::slot_len(state::OwnerIndexSlot::B), 0);
        assert!(context_from_state().is_none());
    }

    #[test]
    fn lifecycle_decoder_accepts_no_args_and_rejects_init_args() {
        assert_eq!(decode_post_upgrade_args_from_bytes(&[]).unwrap(), None);
        let outer_null = encode_args((Option::<UpgradeArgs>::None,)).unwrap();
        assert_eq!(
            decode_post_upgrade_args_from_bytes(&outer_null).unwrap(),
            None
        );
        for expected in [
            UpgradeArgs {
                reward_sns_root_canister_id: None,
            },
            UpgradeArgs {
                reward_sns_root_canister_id: Some(None),
            },
            UpgradeArgs {
                reward_sns_root_canister_id: Some(Some(principal(9))),
            },
        ] {
            let raw = encode_args((Some(expected.clone()),)).unwrap();
            assert_eq!(
                decode_post_upgrade_args_from_bytes(&raw).unwrap(),
                Some(expected)
            );
        }
        let raw = encode_args((InitArgs {
            reward_sns_root_canister_id: Some(principal(1)),
        },))
        .unwrap();
        assert!(decode_post_upgrade_args_from_bytes(&raw)
            .unwrap_err()
            .contains("received InitArgs"));
    }
}
