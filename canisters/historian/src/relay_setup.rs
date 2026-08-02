mod setup_key;
mod target_set;

pub(crate) use setup_key::RelaySetupKey;
use target_set::CanonicalRelayTargetSet;

use crate::clients::blackhole::BlackholeCanisterStatusKind;
use crate::clients::{
    BlackholeClient, ClientError, CmcCanister, CmcClient, IcpXdrConversionRate, LedgerClient,
};
use crate::state::{self, Config};
use crate::*;
use candid::{CandidType, Encode, Principal};
use ic_cdk::call::Call;
use ic_stable_structures::{storable::Bound, Storable};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, Memo, TransferArg, TransferError};
use jupiter_ic_clients::account::principal_to_subaccount;
use jupiter_ic_clients::cycles_probe::{probe_cycles, CyclesProbeClient, CyclesProbePolicy};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeSet;

pub(crate) const EXTRA_TARGET_CHARGE_E8S: u64 = 25_000_000;
pub(crate) const MAX_CONCURRENT_FUNDED_RELAY_SETUPS: usize = 4;
pub(crate) const RELAY_SUBACCOUNT_ONE: [u8; 32] = {
    let mut bytes = [0u8; 32];
    bytes[31] = 1;
    bytes
};

const TOP_UP_CANISTER_MEMO: u64 = 1_347_768_404;
const MAX_DIAGNOSTIC_BYTES: usize = 1_024;
pub(crate) const MAX_RELAY_SETUP_ENTRY_BYTES: u32 = 4_096;

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayCreationPhase {
    Reserved,
    ProbingTargets,
    CmcTransferPrepared,
    CmcTransferAccepted,
    CmcNotifySucceeded,
    CreateDispatched,
    ChildCreated,
    CodeInstalled,
    RelayFundingPrepared,
    RelayFunded,
    HandoffAttempted,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RelayTransferRecord {
    pub amount_e8s: u64,
    pub fee_e8s: u64,
    pub created_at_time_nanos: u64,
    pub block_index: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RelayCreationProgress {
    pub phase: RelayCreationPhase,
    pub cmc_transfer: Option<RelayTransferRecord>,
    pub cycles_minted: Option<u128>,
    pub create_dispatched_at_ts: Option<u64>,
    pub relay_canister_id: Option<Principal>,
    pub relay_funding_transfer: Option<RelayTransferRecord>,
    pub last_error: Option<String>,
}

impl RelayCreationProgress {
    fn reserved() -> Self {
        Self {
            phase: RelayCreationPhase::Reserved,
            cmc_transfer: None,
            cycles_minted: None,
            create_dispatched_at_ts: None,
            relay_canister_id: None,
            relay_funding_transfer: None,
            last_error: None,
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum RelaySetupEntry {
    Creating(RelayCreationProgress),
    Active { relay_canister_id: Principal },
    ManualRecoveryRequired(RelayCreationProgress),
}

impl Storable for RelaySetupEntry {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("failed to encode relay setup entry"))
    }

    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(self).expect("failed to encode relay setup entry")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(bytes.as_ref()).expect("failed to decode relay setup entry")
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: MAX_RELAY_SETUP_ENTRY_BYTES,
        is_fixed_size: false,
    };
}

#[derive(Clone, Copy, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum AuditedCanisterStatusKind {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "stopping")]
    Stopping,
    #[serde(rename = "stopped")]
    Stopped,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct AuditedCanisterSettings {
    controllers: Vec<Principal>,
    log_visibility: jupiter_ic_clients::management::LogVisibility,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
struct AuditedCanisterStatus {
    status: AuditedCanisterStatusKind,
    module_hash: Option<Vec<u8>>,
    settings: AuditedCanisterSettings,
}

#[async_trait::async_trait]
trait ManagementClient: Send + Sync {
    async fn create_canister(
        &self,
        args: &jupiter_ic_clients::management::CreateCanisterArgs,
        cycles: u128,
    ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, String>;
    async fn install_code(
        &self,
        args: &jupiter_ic_clients::management::InstallCodeArgs,
    ) -> Result<(), String>;
    async fn canister_status(
        &self,
        canister_id: Principal,
    ) -> Result<AuditedCanisterStatus, String>;
    async fn update_settings(
        &self,
        args: &jupiter_ic_clients::management::UpdateSettingsArgs,
    ) -> Result<(), String>;
}

struct IcManagementClient;

#[async_trait::async_trait]
impl ManagementClient for IcManagementClient {
    async fn create_canister(
        &self,
        args: &jupiter_ic_clients::management::CreateCanisterArgs,
        cycles: u128,
    ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, String> {
        jupiter_ic_clients::management::create_canister(args, cycles)
            .await
            .map_err(|err| format!("{err:?}"))
    }

    async fn install_code(
        &self,
        args: &jupiter_ic_clients::management::InstallCodeArgs,
    ) -> Result<(), String> {
        jupiter_ic_clients::management::install_code(args)
            .await
            .map_err(|err| format!("{err:?}"))
    }

    async fn canister_status(
        &self,
        canister_id: Principal,
    ) -> Result<AuditedCanisterStatus, String> {
        let response = Call::bounded_wait(Principal::management_canister(), "canister_status")
            .with_arg(jupiter_ic_clients::management::CanisterStatusArgs { canister_id })
            .change_timeout(60)
            .await
            .map_err(|err| format!("{err:?}"))?;
        response
            .candid()
            .map_err(|err| format!("decode canister_status failed: {err:?}"))
    }

    async fn update_settings(
        &self,
        args: &jupiter_ic_clients::management::UpdateSettingsArgs,
    ) -> Result<(), String> {
        jupiter_ic_clients::management::update_settings(args)
            .await
            .map_err(|err| format!("{err:?}"))
    }
}

#[cfg(not(test))]
fn now_nanos() -> u64 {
    ic_cdk::api::time()
}

#[cfg(test)]
static TEST_NOW_NANOS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1_000_000_000);

#[cfg(test)]
fn now_nanos() -> u64 {
    TEST_NOW_NANOS.fetch_add(1_000_000_000, std::sync::atomic::Ordering::SeqCst)
}

fn now_secs() -> u64 {
    now_nanos() / 1_000_000_000
}

#[cfg(not(test))]
fn self_canister_id() -> Principal {
    ic_cdk::api::canister_self()
}

#[cfg(test)]
fn self_canister_id() -> Principal {
    Principal::from_slice(&[42])
}

fn setup_account_for(historian: Principal, key: RelaySetupKey) -> Account {
    Account {
        owner: historian,
        subaccount: Some(key.bytes()),
    }
}

fn cmc_deposit_account(cmc_id: Principal, historian: Principal) -> Account {
    Account {
        owner: cmc_id,
        subaccount: Some(principal_to_subaccount(historian)),
    }
}

fn relay_subaccount_one(relay_id: Principal) -> Account {
    Account {
        owner: relay_id,
        subaccount: Some(RELAY_SUBACCOUNT_ONE),
    }
}

fn bounded_message(mut message: String) -> String {
    if message.len() <= MAX_DIAGNOSTIC_BYTES {
        return message;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message
}

fn get_entry(key: RelaySetupKey) -> Option<RelaySetupEntry> {
    state::with_relay_setup_entries_map(|map| map.get(&key))
}

fn insert_entry(key: RelaySetupKey, entry: RelaySetupEntry) {
    state::with_relay_setup_entries_map(|map| {
        map.insert(key, entry);
    });
}

fn remove_entry(key: RelaySetupKey) {
    state::with_relay_setup_entries_map(|map| {
        map.remove(&key);
    });
}

pub(crate) fn reconcile_interrupted_creating_entries_after_upgrade() {
    state::with_relay_setup_entries_map(|map| {
        let entries = map
            .iter()
            .map(|entry| entry.into_pair())
            .collect::<Vec<_>>();
        for (key, entry) in entries {
            let RelaySetupEntry::Creating(mut progress) = entry else {
                continue;
            };
            match progress.phase {
                RelayCreationPhase::Reserved | RelayCreationPhase::ProbingTargets => {
                    map.remove(&key);
                }
                RelayCreationPhase::CmcTransferPrepared
                | RelayCreationPhase::CmcTransferAccepted
                | RelayCreationPhase::CmcNotifySucceeded
                | RelayCreationPhase::CreateDispatched
                | RelayCreationPhase::ChildCreated
                | RelayCreationPhase::CodeInstalled
                | RelayCreationPhase::RelayFundingPrepared
                | RelayCreationPhase::RelayFunded
                | RelayCreationPhase::HandoffAttempted => {
                    progress.last_error = Some("HistorianUpgradeInterrupted".to_string());
                    map.insert(key, RelaySetupEntry::ManualRecoveryRequired(progress));
                }
            }
        }
    });
}

fn notify_for_entry(entry: RelaySetupEntry) -> RelaySetupNotifyResult {
    match entry {
        RelaySetupEntry::Creating(progress) => RelaySetupNotifyResult::InProgress {
            phase: progress.phase,
            relay_canister_id: progress.relay_canister_id,
        },
        RelaySetupEntry::Active { relay_canister_id } => {
            RelaySetupNotifyResult::Active { relay_canister_id }
        }
        RelaySetupEntry::ManualRecoveryRequired(progress) => {
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: progress.phase,
                relay_canister_id: progress.relay_canister_id,
                message: progress
                    .last_error
                    .unwrap_or_else(|| "operator investigation is required".to_string()),
            }
        }
    }
}

fn require_phase(
    key: RelaySetupKey,
    expected: RelayCreationPhase,
) -> Result<RelayCreationProgress, RelaySetupNotifyResult> {
    match get_entry(key) {
        Some(RelaySetupEntry::Creating(progress)) if progress.phase == expected => Ok(progress),
        Some(entry) => Err(notify_for_entry(entry)),
        None => Err(RelaySetupNotifyResult::FailedPreSpend {
            message: "relay setup reservation no longer exists".to_string(),
        }),
    }
}

fn update_progress(
    key: RelaySetupKey,
    expected: RelayCreationPhase,
    update: impl FnOnce(&mut RelayCreationProgress),
) -> Result<RelayCreationProgress, RelaySetupNotifyResult> {
    let mut progress = require_phase(key, expected)?;
    update(&mut progress);
    insert_entry(key, RelaySetupEntry::Creating(progress.clone()));
    Ok(progress)
}

fn manual_recovery(
    key: RelaySetupKey,
    expected: RelayCreationPhase,
    message: impl Into<String>,
) -> RelaySetupNotifyResult {
    let message = bounded_message(message.into());
    match require_phase(key, expected) {
        Ok(mut progress) => {
            progress.last_error = Some(message.clone());
            let phase = progress.phase;
            let relay_canister_id = progress.relay_canister_id;
            insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase,
                relay_canister_id,
                message,
            }
        }
        Err(result) => result,
    }
}

fn extra_target_charge_e8s(target_count: usize) -> Result<u64, String> {
    let extra_target_count = target_count
        .checked_sub(1)
        .ok_or_else(|| "target count must be positive".to_string())?;
    u64::try_from(extra_target_count)
        .ok()
        .and_then(|count| count.checked_mul(EXTRA_TARGET_CHARGE_E8S))
        .ok_or_else(|| "target-count pricing overflow".to_string())
}

fn nominal_minimum_e8s(config: &Config, target_count: usize) -> Result<u64, String> {
    let extra = extra_target_charge_e8s(target_count)?;
    config
        .relay_setup_min_e8s
        .checked_add(extra)
        .ok_or_else(|| "nominal relay setup minimum overflow".to_string())
}

fn ceil_div(numerator: u128, denominator: u128) -> Option<u128> {
    if denominator == 0 {
        return None;
    }
    numerator
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)
}

fn cmc_conversion_e8s(config: &Config, rate: &IcpXdrConversionRate) -> Result<u64, String> {
    ceil_div(
        config.relay_initial_cycles,
        u128::from(rate.xdr_permyriad_per_icp),
    )
    .and_then(|value| u64::try_from(value).ok())
    .ok_or_else(|| "CMC returned an invalid ICP/XDR conversion rate".to_string())
}

fn current_requirement_e8s(
    config: &Config,
    target_count: usize,
    fee_e8s: u64,
    rate: &IcpXdrConversionRate,
) -> Result<u64, String> {
    let extra = extra_target_charge_e8s(target_count)?;
    let nominal_singleton = config.relay_setup_min_e8s;
    let conversion = cmc_conversion_e8s(config, rate)?;
    let live_singleton = conversion
        .checked_add(config.relay_cycle_safety_margin_e8s)
        .and_then(|value| value.checked_add(config.relay_min_subaccount_one_seed_e8s))
        .and_then(|value| value.checked_add(fee_e8s.checked_mul(2)?))
        .ok_or_else(|| "live relay setup requirement overflow".to_string())?;
    nominal_singleton
        .max(live_singleton)
        .checked_add(extra)
        .ok_or_else(|| "relay setup requirement overflow".to_string())
}

fn canonical_relay_match(
    config: &Config,
    requested_key: RelaySetupKey,
) -> Result<Option<Principal>, String> {
    let Some(relay_canister_id) = config.canonical_relay_canister_id else {
        return Ok(None);
    };
    // The configured production Relay intentionally covers protocol canisters that are not
    // eligible for a newly spawned Relay. It shares framing, ordering, duplicate checks, and
    // hashing with self-service sets, while setup-only protected-target checks apply below only
    // when the requested hash is not this exact configured set.
    let canonical =
        CanonicalRelayTargetSet::canonicalize(config.canonical_relay_targets.clone())
            .map_err(|err| format!("configured canonical Relay target set is invalid: {err}"))?;
    Ok((canonical.key() == requested_key).then_some(relay_canister_id))
}

fn factory_blocked_reason(config: &Config) -> Option<String> {
    if !config.relay_factory_enabled {
        Some("relay factory is disabled".to_string())
    } else if config.cmc_canister_id.is_none() {
        Some("CMC canister is not configured".to_string())
    } else if approved_self_service_relay_wasm().is_none() {
        Some("approved Relay Wasm is not embedded".to_string())
    } else if approved_relay_onchain_module_hash().is_none() {
        Some("approved Relay module hash is unavailable".to_string())
    } else if config.relay_initial_cycles == 0 {
        Some("Relay create cycles must be positive".to_string())
    } else if config.relay_setup_min_e8s == 0 {
        Some("Relay singleton minimum must be positive".to_string())
    } else {
        None
    }
}

fn setup_state(entry: Option<RelaySetupEntry>) -> RelaySetupState {
    match entry {
        None => RelaySetupState::NotFunded,
        Some(RelaySetupEntry::Creating(progress)) => RelaySetupState::InProgress {
            phase: progress.phase,
            relay_canister_id: progress.relay_canister_id,
        },
        Some(RelaySetupEntry::Active { relay_canister_id }) => {
            RelaySetupState::Active { relay_canister_id }
        }
        Some(RelaySetupEntry::ManualRecoveryRequired(progress)) => {
            RelaySetupState::ManualRecoveryRequired {
                phase: progress.phase,
                relay_canister_id: progress.relay_canister_id,
                message: progress
                    .last_error
                    .unwrap_or_else(|| "operator investigation is required".to_string()),
            }
        }
    }
}

pub(crate) fn setup_view(args: RelayTargetSetArgs) -> RelaySetupViewResult {
    setup_view_for_historian(args, self_canister_id())
}

fn setup_view_for_historian(
    args: RelayTargetSetArgs,
    historian: Principal,
) -> RelaySetupViewResult {
    state::with_state(|state| {
        let targets = match CanonicalRelayTargetSet::canonicalize(args.target_canister_ids) {
            Ok(targets) => targets,
            Err(message) => return RelaySetupViewResult::Err(message),
        };
        let key = targets.key();
        let canonical_relay = match canonical_relay_match(&state.config, key) {
            Ok(value) => value,
            Err(message) => return RelaySetupViewResult::Err(message),
        };
        if canonical_relay.is_none() {
            if let Err(message) = targets.validate_for_new_setup(&state.config, historian) {
                return RelaySetupViewResult::Err(message);
            }
        }
        let entry = canonical_relay
            .map(|relay_canister_id| RelaySetupEntry::Active { relay_canister_id })
            .or_else(|| get_entry(key));
        let setup_state = setup_state(entry.clone());
        let active_or_blocked = entry.is_some();
        let factory_available = factory_blocked_reason(&state.config).is_none();
        let expose_account = !active_or_blocked && factory_available;
        let setup_account = expose_account.then(|| setup_account_for(historian, key));
        let setup_account_identifier = setup_account
            .as_ref()
            .map(crate::clients::index::account_identifier_text_for_account);
        let target_count = targets.len();
        let extra_target_count = match target_count
            .checked_sub(1)
            .and_then(|count| u64::try_from(count).ok())
        {
            Some(value) => value,
            None => return RelaySetupViewResult::Err("target-count pricing overflow".to_string()),
        };
        let total_extra_target_charge_e8s = match extra_target_charge_e8s(target_count) {
            Ok(value) => value,
            Err(message) => return RelaySetupViewResult::Err(message),
        };
        let nominal_minimum_e8s = match nominal_minimum_e8s(&state.config, target_count) {
            Ok(value) => value,
            Err(message) => return RelaySetupViewResult::Err(message),
        };
        RelaySetupViewResult::Ok(RelaySetupView {
            canonical_target_canister_ids: targets.targets().to_vec(),
            setup_key_identifier: key.identifier(),
            setup_account,
            setup_account_identifier,
            target_count: u32::try_from(target_count).unwrap_or(u32::MAX),
            singleton_nominal_minimum_e8s: state.config.relay_setup_min_e8s,
            extra_target_count,
            extra_target_unit_charge_e8s: EXTRA_TARGET_CHARGE_E8S,
            total_extra_target_charge_e8s,
            nominal_minimum_e8s,
            factory_available,
            state: setup_state,
        })
    })
}

fn reserve(key: RelaySetupKey) -> Result<RelayCreationProgress, RelaySetupNotifyResult> {
    state::with_relay_setup_entries_map(|map| {
        if let Some(entry) = map.get(&key) {
            return Err(notify_for_entry(entry));
        }
        let creating = map
            .iter()
            .filter(|entry| matches!(entry.value(), RelaySetupEntry::Creating(_)))
            .count();
        if creating >= MAX_CONCURRENT_FUNDED_RELAY_SETUPS {
            return Err(RelaySetupNotifyResult::Busy);
        }
        let progress = RelayCreationProgress::reserved();
        map.insert(key, RelaySetupEntry::Creating(progress.clone()));
        Ok(progress)
    })
}

fn remove_reservation(key: RelaySetupKey, expected: RelayCreationPhase) {
    if matches!(
        get_entry(key),
        Some(RelaySetupEntry::Creating(RelayCreationProgress { phase, .. })) if phase == expected
    ) {
        remove_entry(key);
    }
}

fn transfer_arg(
    key: RelaySetupKey,
    to: Account,
    record: &RelayTransferRecord,
    memo: Option<Vec<u8>>,
) -> TransferArg {
    TransferArg {
        from_subaccount: Some(key.bytes()),
        to,
        amount: record.amount_e8s.into(),
        fee: Some(record.fee_e8s.into()),
        memo: memo.map(Memo::from),
        created_at_time: Some(record.created_at_time_nanos),
    }
}

fn accepted_transfer(
    result: Result<Result<BlockIndex, TransferError>, ClientError>,
) -> Result<u64, Result<TransferError, ClientError>> {
    match result {
        Ok(Ok(block)) => u64::try_from(block.0).map_err(|_| {
            Err(ClientError::Call(
                "ledger block index exceeds u64".to_string(),
            ))
        }),
        Ok(Err(TransferError::Duplicate { duplicate_of })) => u64::try_from(duplicate_of.0)
            .map_err(|_| {
                Err(ClientError::Call(
                    "duplicate block index exceeds u64".to_string(),
                ))
            }),
        Ok(Err(error)) => Err(Ok(error)),
        Err(error) => Err(Err(error)),
    }
}

fn same_principal_set(actual: &[Principal], expected: &[Principal]) -> bool {
    actual.iter().copied().collect::<BTreeSet<_>>()
        == expected.iter().copied().collect::<BTreeSet<_>>()
        && actual.len() == expected.len()
}

fn validate_pre_handoff(
    status: &AuditedCanisterStatus,
    expected_hash: &[u8; 32],
    historian: Principal,
) -> Result<(), String> {
    if status.status != AuditedCanisterStatusKind::Running {
        return Err("Relay is not running before handoff".to_string());
    }
    if status.module_hash.as_deref() != Some(expected_hash.as_slice()) {
        return Err(
            "Relay module hash does not match the approved module before handoff".to_string(),
        );
    }
    if !same_principal_set(&status.settings.controllers, &[historian]) {
        return Err("Relay controllers are not exactly {Historian} before handoff".to_string());
    }
    if status.settings.log_visibility != jupiter_ic_clients::management::LogVisibility::Public {
        return Err("Relay logs are not public before handoff".to_string());
    }
    Ok(())
}

fn validate_post_handoff(
    status: &crate::clients::blackhole::BlackholeCanisterStatus,
    expected_hash: &[u8; 32],
    fiduciary: Principal,
) -> Result<(), String> {
    if status.status != BlackholeCanisterStatusKind::Running {
        return Err("Relay is not running after Fiduciary handoff".to_string());
    }
    if status.module_hash.as_deref() != Some(expected_hash.as_slice()) {
        return Err(
            "Relay module hash does not match the approved module after handoff".to_string(),
        );
    }
    if !same_principal_set(&status.settings.controllers, &[fiduciary]) {
        return Err("Relay controllers are not exactly {Fiduciary} after handoff".to_string());
    }
    Ok(())
}

fn relay_init_arg(config: &Config, targets: &CanonicalRelayTargetSet) -> Vec<u8> {
    #[derive(CandidType)]
    struct SurplusNeuronRecipient {
        neuron_id: u64,
        memo: Vec<u8>,
    }
    #[derive(CandidType)]
    struct InitArgs {
        managed_canisters: Vec<Principal>,
        ledger_canister_id: Option<Principal>,
        cmc_canister_id: Option<Principal>,
        governance_canister_id: Option<Principal>,
        blackhole_canister_id: Option<Principal>,
        main_interval_seconds: Option<u64>,
        max_transfers_per_tick: Option<u32>,
        surplus_canister_recipients: Option<Vec<()>>,
        surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
    }
    let transfer_cap = u32::try_from(targets.len() + 2).expect("target count is bounded at 20");
    Encode!(&InitArgs {
        managed_canisters: targets.targets().to_vec(),
        ledger_canister_id: Some(config.ledger_canister_id),
        cmc_canister_id: config.cmc_canister_id,
        governance_canister_id: Some(jupiter_ic_clients::constants::nns_governance_id()),
        blackhole_canister_id: None,
        main_interval_seconds: Some(config.self_service_relay_interval_seconds),
        max_transfers_per_tick: Some(transfer_cap),
        surplus_canister_recipients: None,
        surplus_neuron_recipients: vec![SurplusNeuronRecipient {
            neuron_id: config.io_surplus_neuron_id,
            memo: Vec::new(),
        }],
    })
    .expect("Relay init args should encode")
}

#[allow(clippy::too_many_arguments)]
async fn notify_with_clients_for_historian<C: CyclesProbeClient>(
    args: RelayTargetSetArgs,
    historian: Principal,
    ledger: &dyn LedgerClient,
    cycles_probe_client: &C,
    cmc: &dyn CmcClient,
    management: &dyn ManagementClient,
    fiduciary_blackhole: &dyn BlackholeClient,
) -> RelaySetupNotifyResult {
    let config = state::with_state(|state| state.config.clone());
    let targets = match CanonicalRelayTargetSet::canonicalize(args.target_canister_ids) {
        Ok(targets) => targets,
        Err(message) => return RelaySetupNotifyResult::FailedPreSpend { message },
    };
    let key = targets.key();
    match canonical_relay_match(&config, key) {
        Ok(Some(relay_canister_id)) => {
            return RelaySetupNotifyResult::Active { relay_canister_id };
        }
        Ok(None) => {}
        Err(message) => return RelaySetupNotifyResult::FailedPreSpend { message },
    }
    if let Err(message) = targets.validate_for_new_setup(&config, historian) {
        return RelaySetupNotifyResult::FailedPreSpend { message };
    }
    if let Some(entry) = get_entry(key) {
        return notify_for_entry(entry);
    }
    if let Some(message) = factory_blocked_reason(&config) {
        return RelaySetupNotifyResult::FailedPreSpend { message };
    }
    let setup_account = setup_account_for(historian, key);
    let balance_e8s = match ledger.balance_of_e8s(setup_account).await {
        Ok(balance) => balance,
        Err(error) => {
            return RelaySetupNotifyResult::FailedPreSpend {
                message: format!("cannot read Relay setup balance: {error}"),
            };
        }
    };
    let nominal = match nominal_minimum_e8s(&config, targets.len()) {
        Ok(value) => value,
        Err(message) => return RelaySetupNotifyResult::FailedPreSpend { message },
    };
    if balance_e8s < nominal {
        return RelaySetupNotifyResult::BelowMinimum {
            balance_e8s,
            required_e8s: nominal,
            shortfall_e8s: nominal - balance_e8s,
        };
    }
    let fee_e8s = match ledger.fee_e8s().await {
        Ok(fee) => fee,
        Err(error) => {
            return RelaySetupNotifyResult::FailedPreSpend {
                message: format!("cannot read the current ICP ledger fee: {error}"),
            };
        }
    };
    let rate = match cmc.get_icp_xdr_conversion_rate().await {
        Ok(rate) => rate,
        Err(error) => {
            return RelaySetupNotifyResult::FailedPreSpend {
                message: format!("cannot read the current CMC conversion rate: {error}"),
            };
        }
    };
    let required_e8s = match current_requirement_e8s(&config, targets.len(), fee_e8s, &rate) {
        Ok(value) => value,
        Err(message) => return RelaySetupNotifyResult::FailedPreSpend { message },
    };
    if balance_e8s < required_e8s {
        return RelaySetupNotifyResult::BelowCurrentRequirement {
            balance_e8s,
            required_e8s,
            shortfall_e8s: required_e8s - balance_e8s,
        };
    }
    if let Err(result) = reserve(key) {
        return result;
    }
    if update_progress(key, RelayCreationPhase::Reserved, |progress| {
        progress.phase = RelayCreationPhase::ProbingTargets;
    })
    .is_err()
    {
        return notify_for_entry(get_entry(key).expect("reservation exists"));
    }
    for target in targets.targets() {
        let cached_route =
            state::with_state(|state| state.cached_cycles_probe_routes.get(target).cloned());
        let result = probe_cycles(
            &CyclesProbePolicy::Auto,
            *target,
            cached_route,
            cycles_probe_client,
        )
        .await;
        if let Err(result) = require_phase(key, RelayCreationPhase::ProbingTargets) {
            return result;
        }
        match result {
            Ok(success) => {
                state::with_root_state_mut(|state| match success.route {
                    Some(route) => {
                        state.cached_cycles_probe_routes.insert(*target, route);
                    }
                    None => {
                        state.cached_cycles_probe_routes.remove(target);
                    }
                });
            }
            Err(error) => {
                remove_reservation(key, RelayCreationPhase::ProbingTargets);
                return RelaySetupNotifyResult::FailedPreSpend {
                    message: bounded_message(format!(
                        "target {} cannot be observed by the Auto cycles probe: {}",
                        target.to_text(),
                        error.message
                    )),
                };
            }
        }
    }
    let conversion_e8s = match cmc_conversion_e8s(&config, &rate) {
        Ok(value) => value,
        Err(message) => {
            remove_reservation(key, RelayCreationPhase::ProbingTargets);
            return RelaySetupNotifyResult::FailedPreSpend { message };
        }
    };
    let cmc_record = RelayTransferRecord {
        amount_e8s: conversion_e8s,
        fee_e8s,
        created_at_time_nanos: now_nanos(),
        block_index: None,
    };
    if let Err(result) = update_progress(key, RelayCreationPhase::ProbingTargets, |progress| {
        progress.phase = RelayCreationPhase::CmcTransferPrepared;
        progress.cmc_transfer = Some(cmc_record.clone());
    }) {
        return result;
    }
    let cmc_id = config.cmc_canister_id.expect("static checks require CMC");
    let transfer_result = ledger
        .icrc1_transfer(transfer_arg(
            key,
            cmc_deposit_account(cmc_id, historian),
            &cmc_record,
            Some(TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec()),
        ))
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::CmcTransferPrepared) {
        return result;
    }
    let cmc_block = match accepted_transfer(transfer_result) {
        Ok(block) => block,
        Err(Ok(error)) => {
            remove_reservation(key, RelayCreationPhase::CmcTransferPrepared);
            return RelaySetupNotifyResult::FailedPreSpend {
                message: bounded_message(format!("CMC ledger transfer was rejected: {error:?}")),
            };
        }
        Err(Err(error)) => {
            return manual_recovery(
                key,
                RelayCreationPhase::CmcTransferPrepared,
                format!("CMC ledger transfer outcome is ambiguous: {error}"),
            );
        }
    };
    if let Err(result) = update_progress(key, RelayCreationPhase::CmcTransferPrepared, |progress| {
        progress.phase = RelayCreationPhase::CmcTransferAccepted;
        if let Some(record) = progress.cmc_transfer.as_mut() {
            record.block_index = Some(cmc_block);
        }
    }) {
        return result;
    }
    let minted_cycles = cmc.notify_top_up(historian, cmc_block).await;
    if let Err(result) = require_phase(key, RelayCreationPhase::CmcTransferAccepted) {
        return result;
    }
    let minted_cycles = match minted_cycles {
        Ok(cycles) => cycles,
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::CmcTransferAccepted,
                format!("CMC notification failed: {error:?}"),
            );
        }
    };
    if minted_cycles < config.relay_initial_cycles {
        return manual_recovery(
            key,
            RelayCreationPhase::CmcTransferAccepted,
            format!(
                "CMC minted {minted_cycles} cycles, below the configured {} cycles required for child creation",
                config.relay_initial_cycles
            ),
        );
    }
    if let Err(result) = update_progress(key, RelayCreationPhase::CmcTransferAccepted, |progress| {
        progress.phase = RelayCreationPhase::CmcNotifySucceeded;
        progress.cycles_minted = Some(minted_cycles);
    }) {
        return result;
    }
    if let Err(result) = update_progress(key, RelayCreationPhase::CmcNotifySucceeded, |progress| {
        progress.phase = RelayCreationPhase::CreateDispatched;
        progress.create_dispatched_at_ts = Some(now_secs());
    }) {
        return result;
    }
    let create_result = management
        .create_canister(
            &jupiter_ic_clients::management::CreateCanisterArgs {
                settings: Some(jupiter_ic_clients::management::CanisterSettings {
                    controllers: Some(vec![historian]),
                    log_visibility: Some(jupiter_ic_clients::management::LogVisibility::Public),
                }),
            },
            config.relay_initial_cycles,
        )
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::CreateDispatched) {
        return result;
    }
    let relay_canister_id = match create_result {
        Ok(result) => result.canister_id,
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::CreateDispatched,
                format!("create_canister was dispatched but did not return a child ID: {error}"),
            );
        }
    };
    if let Err(result) = update_progress(key, RelayCreationPhase::CreateDispatched, |progress| {
        progress.relay_canister_id = Some(relay_canister_id);
        progress.phase = RelayCreationPhase::ChildCreated;
    }) {
        return result;
    }
    let wasm = approved_self_service_relay_wasm()
        .expect("static checks require approved Relay Wasm")
        .to_vec();
    let expected_hash =
        approved_relay_onchain_module_hash().expect("static checks require Relay hash");
    let install_result = management
        .install_code(&jupiter_ic_clients::management::InstallCodeArgs {
            mode: jupiter_ic_clients::management::InstallMode::Install,
            canister_id: relay_canister_id,
            wasm_module: wasm,
            arg: relay_init_arg(&config, &targets),
        })
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::ChildCreated) {
        return result;
    }
    if let Err(install_error) = install_result {
        let observed = management.canister_status(relay_canister_id).await;
        if let Err(result) = require_phase(key, RelayCreationPhase::ChildCreated) {
            return result;
        }
        match observed {
            Ok(status) if status.module_hash.as_deref() == Some(expected_hash.as_slice()) => {}
            Ok(_) => {
                return manual_recovery(
                    key,
                    RelayCreationPhase::ChildCreated,
                    format!("install_code failed and the approved module is not installed: {install_error}"),
                );
            }
            Err(status_error) => {
                return manual_recovery(
                    key,
                    RelayCreationPhase::ChildCreated,
                    format!("install_code failed ({install_error}) and module status could not be reconciled ({status_error})"),
                );
            }
        }
    }
    if let Err(result) = update_progress(key, RelayCreationPhase::ChildCreated, |progress| {
        progress.phase = RelayCreationPhase::CodeInstalled;
    }) {
        return result;
    }
    let remaining_balance = ledger
        .balance_of_e8s(setup_account_for(historian, key))
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::CodeInstalled) {
        return result;
    }
    let remaining_balance = match remaining_balance {
        Ok(balance) => balance,
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::CodeInstalled,
                format!("cannot read setup balance before Relay funding: {error}"),
            );
        }
    };
    let funding_fee = ledger.fee_e8s().await;
    if let Err(result) = require_phase(key, RelayCreationPhase::CodeInstalled) {
        return result;
    }
    let funding_fee = match funding_fee {
        Ok(fee) => fee,
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::CodeInstalled,
                format!("cannot read ledger fee before Relay funding: {error}"),
            );
        }
    };
    let funding_amount = match remaining_balance.checked_sub(funding_fee) {
        Some(amount) if amount >= config.relay_min_subaccount_one_seed_e8s => amount,
        _ => {
            return manual_recovery(
                key,
                RelayCreationPhase::CodeInstalled,
                "post-conversion balance cannot fund the configured Relay subaccount-one seed",
            );
        }
    };
    let funding_record = RelayTransferRecord {
        amount_e8s: funding_amount,
        fee_e8s: funding_fee,
        created_at_time_nanos: now_nanos(),
        block_index: None,
    };
    if let Err(result) = update_progress(key, RelayCreationPhase::CodeInstalled, |progress| {
        progress.phase = RelayCreationPhase::RelayFundingPrepared;
        progress.relay_funding_transfer = Some(funding_record.clone());
    }) {
        return result;
    }
    let funding_result = ledger
        .icrc1_transfer(transfer_arg(
            key,
            relay_subaccount_one(relay_canister_id),
            &funding_record,
            None,
        ))
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::RelayFundingPrepared) {
        return result;
    }
    let funding_block = match accepted_transfer(funding_result) {
        Ok(block) => block,
        Err(Ok(error)) => {
            return manual_recovery(
                key,
                RelayCreationPhase::RelayFundingPrepared,
                format!("Relay funding transfer was rejected: {error:?}"),
            );
        }
        Err(Err(error)) => {
            return manual_recovery(
                key,
                RelayCreationPhase::RelayFundingPrepared,
                format!("Relay funding transfer outcome is ambiguous: {error}"),
            );
        }
    };
    if let Err(result) =
        update_progress(key, RelayCreationPhase::RelayFundingPrepared, |progress| {
            progress.phase = RelayCreationPhase::RelayFunded;
            if let Some(record) = progress.relay_funding_transfer.as_mut() {
                record.block_index = Some(funding_block);
            }
        })
    {
        return result;
    }
    let pre_handoff = management.canister_status(relay_canister_id).await;
    if let Err(result) = require_phase(key, RelayCreationPhase::RelayFunded) {
        return result;
    }
    match pre_handoff {
        Ok(status) => {
            if let Err(message) = validate_pre_handoff(&status, &expected_hash, historian) {
                return manual_recovery(key, RelayCreationPhase::RelayFunded, message);
            }
        }
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::RelayFunded,
                format!("pre-handoff management audit failed: {error}"),
            );
        }
    }
    if let Err(result) = update_progress(key, RelayCreationPhase::RelayFunded, |progress| {
        progress.phase = RelayCreationPhase::HandoffAttempted;
    }) {
        return result;
    }
    let fiduciary = jupiter_ic_clients::constants::fiduciary_blackhole_canister_id();
    let handoff_result = management
        .update_settings(&jupiter_ic_clients::management::UpdateSettingsArgs {
            canister_id: relay_canister_id,
            settings: jupiter_ic_clients::management::CanisterSettings {
                controllers: Some(vec![fiduciary]),
                log_visibility: Some(jupiter_ic_clients::management::LogVisibility::Public),
            },
        })
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::HandoffAttempted) {
        return result;
    }
    let post_handoff = fiduciary_blackhole.canister_status(relay_canister_id).await;
    if let Err(result) = require_phase(key, RelayCreationPhase::HandoffAttempted) {
        return result;
    }
    match post_handoff {
        Ok(status) => {
            if let Err(message) = validate_post_handoff(&status, &expected_hash, fiduciary) {
                let prefix = handoff_result
                    .err()
                    .map(|error| format!("update_settings reported {error}; "))
                    .unwrap_or_default();
                return manual_recovery(
                    key,
                    RelayCreationPhase::HandoffAttempted,
                    format!("{prefix}{message}"),
                );
            }
        }
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::HandoffAttempted,
                format!("post-handoff Fiduciary audit failed: {error}"),
            );
        }
    }
    let final_progress = match require_phase(key, RelayCreationPhase::HandoffAttempted) {
        Ok(progress) => progress,
        Err(result) => return result,
    };
    let ready = final_progress.relay_canister_id == Some(relay_canister_id)
        && final_progress
            .cmc_transfer
            .as_ref()
            .and_then(|record| record.block_index)
            .is_some()
        && final_progress.cycles_minted.is_some()
        && final_progress
            .relay_funding_transfer
            .as_ref()
            .and_then(|record| record.block_index)
            .is_some();
    if !ready {
        return manual_recovery(
            key,
            RelayCreationPhase::HandoffAttempted,
            "activation prerequisites are incomplete",
        );
    }
    insert_entry(key, RelaySetupEntry::Active { relay_canister_id });
    state::with_state_mut_sections(state::DIRTY_ROOT | state::DIRTY_REGISTRY, |state| {
        for target in targets.targets() {
            mark_active_relay_tracked(state, *target, relay_canister_id, Some(now_secs()));
        }
    });
    RelaySetupNotifyResult::Active { relay_canister_id }
}

pub(crate) async fn notify_relay_setup(args: RelayTargetSetArgs) -> RelaySetupNotifyResult {
    let config = state::with_state(|state| state.config.clone());
    let ledger = jupiter_ic_clients::ledger::IcrcLedgerCanister::new(config.ledger_canister_id);
    let cycles_probe_client =
        jupiter_ic_clients::cycles_probe::IcCyclesProbeClient::new(config.sns_wasm_canister_id);
    // Missing configuration is rejected synchronously inside the shared workflow before this
    // placeholder can ever be called. Constructing it here keeps target validation, exact-set
    // lookup, and all other static checks in one deterministic order.
    let cmc = CmcCanister::new(
        config
            .cmc_canister_id
            .unwrap_or_else(Principal::management_canister),
    );
    let fiduciary_blackhole = clients::blackhole::BlackholeCanister::new(
        jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
    );
    notify_with_clients_for_historian(
        args,
        self_canister_id(),
        &ledger,
        &cycles_probe_client,
        &cmc,
        &IcManagementClient,
        &fiduciary_blackhole,
    )
    .await
}

#[cfg(any(test, feature = "debug_api"))]
pub(crate) fn debug_setup_entries() -> Vec<RelaySetupDebugEntry> {
    state::with_relay_setup_entries_map(|map| {
        map.iter()
            .map(|entry| {
                let (entry_variant, phase, relay_canister_id) = match entry.value() {
                    RelaySetupEntry::Creating(progress) => (
                        "Creating".to_string(),
                        Some(progress.phase),
                        progress.relay_canister_id,
                    ),
                    RelaySetupEntry::Active { relay_canister_id } => {
                        ("Active".to_string(), None, Some(relay_canister_id))
                    }
                    RelaySetupEntry::ManualRecoveryRequired(progress) => (
                        "ManualRecoveryRequired".to_string(),
                        Some(progress.phase),
                        progress.relay_canister_id,
                    ),
                };
                RelaySetupDebugEntry {
                    setup_key_identifier: entry.key().identifier(),
                    entry_variant,
                    phase,
                    relay_canister_id,
                }
            })
            .collect()
    })
}

#[cfg(any(test, feature = "debug_api"))]
pub(crate) fn clear_setup_entries_for_debug() {
    state::with_relay_setup_entries_map(|map| map.clear_new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Nat;
    use futures::channel::oneshot;
    use futures::executor::block_on;
    use jupiter_ic_clients::sns::{
        DeployedSns, ListDeployedSnsesResponse, ListSnsCanistersResponse,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::task::Poll;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn config() -> Config {
        Config {
            staking_account: Account {
                owner: principal(60),
                subaccount: None,
            },
            output_source_account: Account {
                owner: principal(61),
                subaccount: None,
            },
            output_account: Account {
                owner: principal(62),
                subaccount: None,
            },
            rewards_account: Account {
                owner: principal(63),
                subaccount: None,
            },
            ledger_canister_id: principal(64),
            index_canister_id: principal(65),
            cmc_canister_id: Some(principal(66)),
            faucet_canister_id: Some(principal(67)),
            sns_wasm_canister_id: principal(68),
            xrc_canister_id: principal(69),
            enable_sns_tracking: false,
            scan_interval_seconds: 600,
            cycles_interval_seconds: 604_800,
            min_tx_e8s: 100_000_000,
            max_cycles_entries_per_canister: 100,
            max_commitment_entries_per_canister: 100,
            max_index_pages_per_tick: 10,
            max_canisters_per_cycles_tick: 25,
            relay_factory_enabled: true,
            relay_setup_min_e8s: 300_000_000,
            relay_initial_cycles: 2_000_000_000_000,
            relay_cycle_safety_margin_e8s: 5_000_000,
            relay_min_subaccount_one_seed_e8s: 100_020_000,
            self_service_relay_interval_seconds: 86_400,
            io_surplus_neuron_id: 1,
            canonical_relay_canister_id: Some(principal(70)),
            canonical_relay_targets: vec![principal(71), principal(72)],
        }
    }

    fn reset() {
        clear_setup_entries_for_debug();
        state::set_state(State::new(config(), 0));
    }

    fn progress_for_phase(phase: RelayCreationPhase) -> RelayCreationProgress {
        RelayCreationProgress {
            phase,
            cmc_transfer: Some(RelayTransferRecord {
                amount_e8s: u64::MAX,
                fee_e8s: u64::MAX,
                created_at_time_nanos: u64::MAX,
                block_index: Some(u64::MAX),
            }),
            cycles_minted: Some(u128::MAX),
            create_dispatched_at_ts: Some(u64::MAX),
            relay_canister_id: Some(Principal::from_slice(&[7; 29])),
            relay_funding_transfer: Some(RelayTransferRecord {
                amount_e8s: u64::MAX,
                fee_e8s: u64::MAX,
                created_at_time_nanos: u64::MAX,
                block_index: Some(u64::MAX),
            }),
            last_error: Some(bounded_message("é".repeat(MAX_DIAGNOSTIC_BYTES))),
        }
    }

    #[derive(Clone, Copy)]
    enum LedgerOutcome {
        Accepted(u64),
        Ambiguous,
    }

    struct MockLedger {
        balances: Mutex<VecDeque<u64>>,
        outcomes: Mutex<VecDeque<LedgerOutcome>>,
        balance_calls: AtomicUsize,
        fee_calls: AtomicUsize,
        transfers: Mutex<Vec<TransferArg>>,
    }

    impl MockLedger {
        fn new(
            balances: impl IntoIterator<Item = u64>,
            outcomes: impl IntoIterator<Item = LedgerOutcome>,
        ) -> Self {
            Self {
                balances: Mutex::new(balances.into_iter().collect()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                balance_calls: AtomicUsize::new(0),
                fee_calls: AtomicUsize::new(0),
                transfers: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LedgerClient for MockLedger {
        async fn fee_e8s(&self) -> Result<u64, ClientError> {
            self.fee_calls.fetch_add(1, Ordering::SeqCst);
            Ok(10_000)
        }

        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, ClientError> {
            self.balance_calls.fetch_add(1, Ordering::SeqCst);
            self.balances
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ClientError::Call("unexpected balance call".to_string()))
        }

        async fn icrc1_transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
            self.transfers.lock().unwrap().push(arg);
            match self.outcomes.lock().unwrap().pop_front() {
                Some(LedgerOutcome::Accepted(block)) => Ok(Ok(Nat::from(block))),
                Some(LedgerOutcome::Ambiguous) => {
                    Err(ClientError::Call("ambiguous ledger transport".to_string()))
                }
                None => Err(ClientError::Call("unexpected transfer call".to_string())),
            }
        }
    }

    struct MockCmc {
        notify_calls: AtomicUsize,
    }

    struct ReadBarrierCmc {
        rate_calls: AtomicUsize,
        notify_calls: AtomicUsize,
        rate_releases: Mutex<Option<Vec<oneshot::Sender<()>>>>,
        rate_waiters: Mutex<VecDeque<oneshot::Receiver<()>>>,
    }

    impl ReadBarrierCmc {
        fn new() -> Self {
            let (send_one, receive_one) = oneshot::channel();
            let (send_two, receive_two) = oneshot::channel();
            Self {
                rate_calls: AtomicUsize::new(0),
                notify_calls: AtomicUsize::new(0),
                rate_releases: Mutex::new(Some(vec![send_one, send_two])),
                rate_waiters: Mutex::new(VecDeque::from([receive_one, receive_two])),
            }
        }
    }

    #[async_trait::async_trait]
    impl CmcClient for ReadBarrierCmc {
        async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
            let waiter = self.rate_waiters.lock().unwrap().pop_front().unwrap();
            if self.rate_calls.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                for release in self.rate_releases.lock().unwrap().take().unwrap() {
                    let _ = release.send(());
                }
            }
            let _ = waiter.await;
            Ok(IcpXdrConversionRate {
                timestamp_seconds: 1,
                xdr_permyriad_per_icp: 1_000_000,
            })
        }

        async fn notify_top_up(
            &self,
            _canister_id: Principal,
            _block_index: u64,
        ) -> Result<u128, jupiter_ic_clients::cmc::NotifyTopUpError> {
            self.notify_calls.fetch_add(1, Ordering::SeqCst);
            Ok(2_000_000_000_000)
        }
    }

    #[async_trait::async_trait]
    impl CmcClient for MockCmc {
        async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
            Ok(IcpXdrConversionRate {
                timestamp_seconds: 1,
                xdr_permyriad_per_icp: 1_000_000,
            })
        }

        async fn notify_top_up(
            &self,
            _canister_id: Principal,
            _block_index: u64,
        ) -> Result<u128, jupiter_ic_clients::cmc::NotifyTopUpError> {
            self.notify_calls.fetch_add(1, Ordering::SeqCst);
            Ok(2_000_000_000_000)
        }
    }

    struct MockCyclesProbe {
        succeeds: bool,
        blackhole_calls: AtomicUsize,
    }

    struct YieldingCyclesProbe {
        blackhole_calls: AtomicUsize,
    }

    struct MixedRouteCyclesProbe {
        direct_target: Principal,
        sns_swap_target: Principal,
        sns_root: Principal,
        expose_sns_route: bool,
        calls: Mutex<Vec<String>>,
    }

    #[allow(async_fn_in_trait)]
    impl CyclesProbeClient for MixedRouteCyclesProbe {
        async fn self_cycles(&self, target: Principal) -> Option<u128> {
            self.calls.lock().unwrap().push(format!("self:{target}"));
            (target == self.direct_target).then_some(1_000_000)
        }

        async fn blackhole_cycles(
            &self,
            probe_canister_id: Principal,
            target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.calls.lock().unwrap().push(format!(
                "blackhole:{probe_canister_id}:{target_canister_id}"
            ));
            Err(jupiter_ic_clients::ClientError::Call(
                "not blackholed".to_string(),
            ))
        }

        async fn list_deployed_snses(
            &self,
        ) -> Result<ListDeployedSnsesResponse, jupiter_ic_clients::ClientError> {
            self.calls.lock().unwrap().push("list_sns".to_string());
            Ok(ListDeployedSnsesResponse {
                instances: self
                    .expose_sns_route
                    .then_some(DeployedSns {
                        root_canister_id: Some(self.sns_root),
                        swap_canister_id: Some(self.sns_swap_target),
                        ..Default::default()
                    })
                    .into_iter()
                    .collect(),
            })
        }

        async fn canister_info_controllers(
            &self,
            target: Principal,
        ) -> Result<Vec<Principal>, jupiter_ic_clients::ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("controllers:{target}"));
            Ok(Vec::new())
        }

        async fn list_sns_canisters(
            &self,
            root_canister_id: Principal,
        ) -> Result<ListSnsCanistersResponse, jupiter_ic_clients::ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("list_root:{root_canister_id}"));
            Ok(ListSnsCanistersResponse::default())
        }

        async fn sns_root_cycles(
            &self,
            root_canister_id: Principal,
            target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("sns_root:{root_canister_id}:{target_canister_id}"));
            Ok(1_000_000)
        }

        async fn sns_swap_cycles(
            &self,
            swap_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("sns_swap:{swap_canister_id}"));
            Ok(1_000_000)
        }
    }

    #[allow(async_fn_in_trait)]
    impl CyclesProbeClient for YieldingCyclesProbe {
        async fn self_cycles(&self, _target: Principal) -> Option<u128> {
            None
        }

        async fn blackhole_cycles(
            &self,
            _probe_canister_id: Principal,
            _target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.blackhole_calls.fetch_add(1, Ordering::SeqCst);
            let mut yielded = false;
            futures::future::poll_fn(|context| {
                if yielded {
                    Poll::Ready(())
                } else {
                    yielded = true;
                    context.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            Ok(1_000_000)
        }

        async fn list_deployed_snses(
            &self,
        ) -> Result<ListDeployedSnsesResponse, jupiter_ic_clients::ClientError> {
            Ok(ListDeployedSnsesResponse::default())
        }

        async fn canister_info_controllers(
            &self,
            _target: Principal,
        ) -> Result<Vec<Principal>, jupiter_ic_clients::ClientError> {
            Ok(Vec::new())
        }

        async fn list_sns_canisters(
            &self,
            _root_canister_id: Principal,
        ) -> Result<ListSnsCanistersResponse, jupiter_ic_clients::ClientError> {
            Ok(ListSnsCanistersResponse::default())
        }

        async fn sns_root_cycles(
            &self,
            _root_canister_id: Principal,
            _target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            unreachable!()
        }

        async fn sns_swap_cycles(
            &self,
            _swap_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            unreachable!()
        }
    }

    #[allow(async_fn_in_trait)]
    impl CyclesProbeClient for MockCyclesProbe {
        async fn self_cycles(&self, _target: Principal) -> Option<u128> {
            None
        }

        async fn blackhole_cycles(
            &self,
            _probe_canister_id: Principal,
            _target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.blackhole_calls.fetch_add(1, Ordering::SeqCst);
            self.succeeds
                .then_some(1_000_000)
                .ok_or_else(|| jupiter_ic_clients::ClientError::Call("not observable".to_string()))
        }

        async fn list_deployed_snses(
            &self,
        ) -> Result<ListDeployedSnsesResponse, jupiter_ic_clients::ClientError> {
            Ok(ListDeployedSnsesResponse::default())
        }

        async fn canister_info_controllers(
            &self,
            _target: Principal,
        ) -> Result<Vec<Principal>, jupiter_ic_clients::ClientError> {
            Ok(Vec::new())
        }

        async fn list_sns_canisters(
            &self,
            _root_canister_id: Principal,
        ) -> Result<ListSnsCanistersResponse, jupiter_ic_clients::ClientError> {
            Ok(ListSnsCanistersResponse::default())
        }

        async fn sns_root_cycles(
            &self,
            _root_canister_id: Principal,
            _target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            Err(jupiter_ic_clients::ClientError::Call(
                "unexpected SNS root call".to_string(),
            ))
        }

        async fn sns_swap_cycles(
            &self,
            _swap_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            Err(jupiter_ic_clients::ClientError::Call(
                "unexpected SNS swap call".to_string(),
            ))
        }
    }

    struct MockManagement {
        relay_id: Principal,
        create_error: Option<String>,
        create_calls: AtomicUsize,
        installs: Mutex<Vec<jupiter_ic_clients::management::InstallCodeArgs>>,
    }

    #[async_trait::async_trait]
    impl ManagementClient for MockManagement {
        async fn create_canister(
            &self,
            _args: &jupiter_ic_clients::management::CreateCanisterArgs,
            _cycles: u128,
        ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, String> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.create_error {
                Err(error.clone())
            } else {
                Ok(jupiter_ic_clients::management::CreateCanisterResult {
                    canister_id: self.relay_id,
                })
            }
        }

        async fn install_code(
            &self,
            args: &jupiter_ic_clients::management::InstallCodeArgs,
        ) -> Result<(), String> {
            self.installs.lock().unwrap().push(args.clone());
            Ok(())
        }

        async fn canister_status(
            &self,
            _canister_id: Principal,
        ) -> Result<AuditedCanisterStatus, String> {
            Ok(AuditedCanisterStatus {
                status: AuditedCanisterStatusKind::Running,
                module_hash: approved_relay_onchain_module_hash().map(|hash| hash.to_vec()),
                settings: AuditedCanisterSettings {
                    controllers: vec![principal(42)],
                    log_visibility: jupiter_ic_clients::management::LogVisibility::Public,
                },
            })
        }

        async fn update_settings(
            &self,
            _args: &jupiter_ic_clients::management::UpdateSettingsArgs,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    struct MockFiduciary;

    #[async_trait::async_trait]
    impl BlackholeClient for MockFiduciary {
        async fn canister_status(
            &self,
            _canister_id: Principal,
        ) -> Result<crate::clients::blackhole::BlackholeCanisterStatus, ClientError> {
            Ok(crate::clients::blackhole::BlackholeCanisterStatus {
                status: BlackholeCanisterStatusKind::Running,
                module_hash: approved_relay_onchain_module_hash().map(|hash| hash.to_vec()),
                cycles: Nat::from(1_000_000u64),
                settings: crate::clients::blackhole::BlackholeSettings {
                    controllers: vec![
                        jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
                    ],
                },
                memory_size: None,
                memory_metrics: None,
            })
        }
    }

    fn mocks(
        ledger: MockLedger,
        probe_succeeds: bool,
        create_error: Option<&str>,
    ) -> (
        MockLedger,
        MockCyclesProbe,
        MockCmc,
        MockManagement,
        MockFiduciary,
    ) {
        (
            ledger,
            MockCyclesProbe {
                succeeds: probe_succeeds,
                blackhole_calls: AtomicUsize::new(0),
            },
            MockCmc {
                notify_calls: AtomicUsize::new(0),
            },
            MockManagement {
                relay_id: principal(80),
                create_error: create_error.map(str::to_string),
                create_calls: AtomicUsize::new(0),
                installs: Mutex::new(Vec::new()),
            },
            MockFiduciary,
        )
    }

    #[test]
    fn pricing_examples_and_two_fee_live_requirement_are_exact() {
        let cfg = config();
        assert_eq!(nominal_minimum_e8s(&cfg, 1), Ok(300_000_000));
        assert_eq!(nominal_minimum_e8s(&cfg, 2), Ok(325_000_000));
        assert_eq!(nominal_minimum_e8s(&cfg, 10), Ok(525_000_000));
        assert_eq!(nominal_minimum_e8s(&cfg, 20), Ok(775_000_000));
        let rate = IcpXdrConversionRate {
            timestamp_seconds: 1,
            xdr_permyriad_per_icp: 1_000_000,
        };
        assert_eq!(cmc_conversion_e8s(&cfg, &rate), Ok(2_000_000));
        assert_eq!(
            current_requirement_e8s(&cfg, 1, 10_000, &rate),
            Ok(300_000_000)
        );
    }

    #[test]
    fn extra_target_charge_is_additive_under_nominal_and_live_dominance() {
        let rate = IcpXdrConversionRate {
            timestamp_seconds: 1,
            xdr_permyriad_per_icp: 1_000_000,
        };
        let nominal = config();
        let mut live = config();
        live.relay_setup_min_e8s = 1;
        live.relay_initial_cycles = 400_000_000_000_000;

        for cfg in [&nominal, &live] {
            let singleton = current_requirement_e8s(cfg, 1, 10_000, &rate).unwrap();
            assert_eq!(
                current_requirement_e8s(cfg, 2, 10_000, &rate).unwrap() - singleton,
                25_000_000
            );
            assert_eq!(
                current_requirement_e8s(cfg, 10, 10_000, &rate).unwrap() - singleton,
                225_000_000
            );
            assert_eq!(
                current_requirement_e8s(cfg, 20, 10_000, &rate).unwrap() - singleton,
                475_000_000
            );
        }
    }

    #[test]
    fn maximum_manual_recovery_entry_fits_the_stable_bound() {
        let progress = progress_for_phase(RelayCreationPhase::HandoffAttempted);
        assert_eq!(
            progress.last_error.as_ref().unwrap().len(),
            MAX_DIAGNOSTIC_BYTES
        );
        let encoded = RelaySetupEntry::ManualRecoveryRequired(progress).into_bytes();
        assert_eq!(encoded.len(), 1_338);
        assert!(encoded.len() <= MAX_RELAY_SETUP_ENTRY_BYTES as usize);
    }

    #[test]
    fn upgrade_reconciliation_covers_every_creation_phase_without_clients() {
        let retryable = [
            RelayCreationPhase::Reserved,
            RelayCreationPhase::ProbingTargets,
        ];
        let manual = [
            RelayCreationPhase::CmcTransferPrepared,
            RelayCreationPhase::CmcTransferAccepted,
            RelayCreationPhase::CmcNotifySucceeded,
            RelayCreationPhase::CreateDispatched,
            RelayCreationPhase::ChildCreated,
            RelayCreationPhase::CodeInstalled,
            RelayCreationPhase::RelayFundingPrepared,
            RelayCreationPhase::RelayFunded,
            RelayCreationPhase::HandoffAttempted,
        ];

        for (index, phase) in retryable.into_iter().enumerate() {
            reset();
            let key = RelaySetupKey::from_canonical_targets(&[principal(index as u8 + 1)]);
            insert_entry(key, RelaySetupEntry::Creating(progress_for_phase(phase)));
            reconcile_interrupted_creating_entries_after_upgrade();
            assert_eq!(get_entry(key), None, "phase {phase:?}");
        }
        for (index, phase) in manual.into_iter().enumerate() {
            reset();
            let key = RelaySetupKey::from_canonical_targets(&[principal(index as u8 + 20)]);
            insert_entry(key, RelaySetupEntry::Creating(progress_for_phase(phase)));
            reconcile_interrupted_creating_entries_after_upgrade();
            let Some(RelaySetupEntry::ManualRecoveryRequired(progress)) = get_entry(key) else {
                panic!("phase {phase:?} did not enter manual recovery")
            };
            assert_eq!(progress.phase, phase);
            assert_eq!(
                progress.last_error.as_deref(),
                Some("HistorianUpgradeInterrupted")
            );
        }

        reset();
        let active_key = RelaySetupKey::from_canonical_targets(&[principal(90)]);
        let active = RelaySetupEntry::Active {
            relay_canister_id: principal(91),
        };
        let active_bytes = active.to_bytes().into_owned();
        insert_entry(active_key, active);
        let manual_key = RelaySetupKey::from_canonical_targets(&[principal(92)]);
        let manual_entry = RelaySetupEntry::ManualRecoveryRequired(progress_for_phase(
            RelayCreationPhase::RelayFunded,
        ));
        insert_entry(manual_key, manual_entry.clone());
        reconcile_interrupted_creating_entries_after_upgrade();
        assert_eq!(
            get_entry(active_key).unwrap().to_bytes().as_ref(),
            active_bytes.as_slice()
        );
        assert_eq!(get_entry(manual_key), Some(manual_entry));
    }

    #[test]
    fn setup_account_is_derived_from_the_hash() {
        let historian = principal(42);
        let key = RelaySetupKey::from_canonical_targets(&[principal(1)]);
        assert_eq!(
            setup_account_for(historian, key).subaccount,
            Some(key.bytes())
        );
    }

    #[test]
    fn active_entry_round_trips_with_only_the_relay_id() {
        let relay = principal(73);
        let entry = RelaySetupEntry::Active {
            relay_canister_id: relay,
        };
        let decoded = RelaySetupEntry::from_bytes(entry.to_bytes());
        assert_eq!(
            decoded,
            RelaySetupEntry::Active {
                relay_canister_id: relay
            }
        );
    }

    #[test]
    fn exact_set_idempotency_and_overlapping_active_sets_are_independent() {
        reset();
        let a = CanonicalRelayTargetSet::canonicalize(vec![principal(1)])
            .unwrap()
            .key();
        let ab = CanonicalRelayTargetSet::canonicalize(vec![principal(1), principal(2)])
            .unwrap()
            .key();
        let bc = CanonicalRelayTargetSet::canonicalize(vec![principal(2), principal(3)])
            .unwrap()
            .key();
        let ba = CanonicalRelayTargetSet::canonicalize(vec![principal(2), principal(1)])
            .unwrap()
            .key();
        assert_eq!(ab, ba);
        assert_ne!(a, ab);
        assert!(reserve(ab).is_ok());
        assert!(matches!(
            reserve(ab),
            Err(RelaySetupNotifyResult::InProgress { .. })
        ));
        assert!(reserve(bc).is_ok());
        let relay_a = principal(73);
        let relay_ab = principal(74);
        let relay_bc = principal(75);
        insert_entry(
            a,
            RelaySetupEntry::Active {
                relay_canister_id: relay_a,
            },
        );
        insert_entry(
            ab,
            RelaySetupEntry::Active {
                relay_canister_id: relay_ab,
            },
        );
        insert_entry(
            bc,
            RelaySetupEntry::Active {
                relay_canister_id: relay_bc,
            },
        );
        assert_eq!(
            notify_for_entry(get_entry(ba).unwrap()),
            RelaySetupNotifyResult::Active {
                relay_canister_id: relay_ab
            }
        );
        assert_eq!(
            notify_for_entry(get_entry(a).unwrap()),
            RelaySetupNotifyResult::Active {
                relay_canister_id: relay_a
            }
        );
        assert_eq!(
            notify_for_entry(get_entry(bc).unwrap()),
            RelaySetupNotifyResult::Active {
                relay_canister_id: relay_bc
            }
        );
    }

    #[test]
    fn fifth_distinct_funded_reservation_is_busy() {
        reset();
        for byte in 1..=4 {
            assert!(reserve(RelaySetupKey::from_canonical_targets(&[principal(byte)])).is_ok());
        }
        assert_eq!(
            reserve(RelaySetupKey::from_canonical_targets(&[principal(5)])),
            Err(RelaySetupNotifyResult::Busy)
        );
    }

    #[test]
    fn canonical_exact_set_hides_the_setup_account() {
        clear_setup_entries_for_debug();
        let historian = principal(42);
        let mut cfg = config();
        cfg.canonical_relay_targets = vec![
            historian,
            jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
        ];
        state::set_state(State::new(cfg, 0));
        let result = setup_view_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![
                    jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
                    historian,
                ],
            },
            historian,
        );
        let RelaySetupViewResult::Ok(view) = result else {
            panic!("expected view")
        };
        assert_eq!(view.setup_account, None);
        assert_eq!(
            view.state,
            RelaySetupState::Active {
                relay_canister_id: principal(70)
            }
        );
    }

    #[test]
    fn static_and_underfunded_calls_make_only_the_permitted_external_reads() {
        reset();
        let (ledger, probe, cmc, management, fiduciary) =
            mocks(MockLedger::new([299_999_999], []), true, None);
        let invalid = block_on(notify_with_clients_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![Principal::anonymous()],
            },
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            invalid,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 0);

        let below = block_on(notify_with_clients_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![principal(1)],
            },
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert_eq!(
            below,
            RelaySetupNotifyResult::BelowMinimum {
                balance_e8s: 299_999_999,
                required_e8s: 300_000_000,
                shortfall_e8s: 1,
            }
        );
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert!(debug_setup_entries().is_empty());

        let mut disabled = config();
        disabled.relay_factory_enabled = false;
        state::set_state(State::new(disabled, 0));
        let disabled_result = block_on(notify_with_clients_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![principal(2)],
            },
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            disabled_result,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_probe_removes_reservation_before_any_spend() {
        reset();
        let (ledger, probe, cmc, management, fiduciary) =
            mocks(MockLedger::new([300_000_000], []), false, None);
        let result = block_on(notify_with_clients_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![principal(1)],
            },
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert!(probe.blackhole_calls.load(Ordering::SeqCst) > 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert!(debug_setup_entries().is_empty());
    }

    #[test]
    fn every_target_is_probed_before_spend_and_mixed_auto_routes_succeed() {
        reset();
        let direct_target = principal(1);
        let sns_swap_target = principal(2);
        let probe = MixedRouteCyclesProbe {
            direct_target,
            sns_swap_target,
            sns_root: principal(3),
            expose_sns_route: true,
            calls: Mutex::new(Vec::new()),
        };
        let ledger = MockLedger::new(
            [325_000_000, 322_990_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        );
        let cmc = MockCmc {
            notify_calls: AtomicUsize::new(0),
        };
        let management = MockManagement {
            relay_id: principal(80),
            create_error: None,
            create_calls: AtomicUsize::new(0),
            installs: Mutex::new(Vec::new()),
        };
        let result = block_on(notify_with_clients_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![sns_swap_target, direct_target],
            },
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &MockFiduciary,
        ));
        assert!(matches!(result, RelaySetupNotifyResult::Active { .. }));
        let calls = probe.calls.lock().unwrap();
        assert_eq!(calls[0], format!("self:{direct_target}"));
        assert_eq!(calls[1], format!("self:{sns_swap_target}"));
        let list_position = calls.iter().position(|call| call == "list_sns").unwrap();
        let swap_position = calls
            .iter()
            .position(|call| call == &format!("sns_swap:{sns_swap_target}"))
            .unwrap();
        assert!(calls[2..list_position]
            .iter()
            .all(|call| call.starts_with("blackhole:")));
        assert!(list_position < swap_position);
        assert_eq!(ledger.transfers.lock().unwrap().len(), 2);
    }

    #[test]
    fn one_unobservable_target_prevents_all_irreversible_operations() {
        reset();
        let direct_target = principal(1);
        let unobservable_target = principal(2);
        let probe = MixedRouteCyclesProbe {
            direct_target,
            sns_swap_target: unobservable_target,
            sns_root: principal(3),
            expose_sns_route: false,
            calls: Mutex::new(Vec::new()),
        };
        let ledger = MockLedger::new([325_000_000], []);
        let cmc = MockCmc {
            notify_calls: AtomicUsize::new(0),
        };
        let management = MockManagement {
            relay_id: principal(80),
            create_error: None,
            create_calls: AtomicUsize::new(0),
            installs: Mutex::new(Vec::new()),
        };
        let result = block_on(notify_with_clients_for_historian(
            RelayTargetSetArgs {
                target_canister_ids: vec![direct_target, unobservable_target],
            },
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &MockFiduciary,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert_eq!(
            probe
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.starts_with("self:"))
                .count(),
            2
        );
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn same_key_async_notify_contention_has_one_workflow_and_one_in_progress_result() {
        reset();
        let args = RelayTargetSetArgs {
            target_canister_ids: vec![principal(1)],
        };
        let ledger = MockLedger::new(
            [300_000_000, 300_000_000, 297_990_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        );
        let probe = YieldingCyclesProbe {
            blackhole_calls: AtomicUsize::new(0),
        };
        let cmc = ReadBarrierCmc::new();
        let management = MockManagement {
            relay_id: principal(80),
            create_error: None,
            create_calls: AtomicUsize::new(0),
            installs: Mutex::new(Vec::new()),
        };
        let fiduciary = MockFiduciary;

        let first = notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        );
        let second = notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        );
        let (first, second) = block_on(futures::future::join(first, second));
        assert!(matches!(
            (&first, &second),
            (
                RelaySetupNotifyResult::Active { .. },
                RelaySetupNotifyResult::InProgress { .. }
            ) | (
                RelaySetupNotifyResult::InProgress { .. },
                RelaySetupNotifyResult::Active { .. }
            )
        ));
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 2);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.transfers.lock().unwrap().len(), 2);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ambiguous_transfer_and_dispatched_create_are_never_replayed() {
        reset();
        let args = RelayTargetSetArgs {
            target_canister_ids: vec![principal(1)],
        };
        let (ledger, probe, cmc, management, fiduciary) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Ambiguous]),
            true,
            None,
        );
        let first = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            first,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CmcTransferPrepared,
                ..
            }
        ));
        let second = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            second,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);

        reset();
        let (ledger, probe, cmc, management, fiduciary) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Accepted(10)]),
            true,
            Some("create result was lost"),
        );
        let first = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            first,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CreateDispatched,
                relay_canister_id: None,
                ..
            }
        ));
        let second = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert!(matches!(
            second,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn twenty_target_activation_uses_local_canonical_vector_then_discards_it() {
        reset();
        let targets = (100..120).rev().map(principal).collect::<Vec<_>>();
        let args = RelayTargetSetArgs {
            target_canister_ids: targets,
        };
        let (ledger, probe, cmc, management, fiduciary) = mocks(
            MockLedger::new(
                [775_000_000, 772_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
            ),
            true,
            None,
        );
        let result = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &fiduciary,
        ));
        assert_eq!(
            result,
            RelaySetupNotifyResult::Active {
                relay_canister_id: principal(80),
            }
        );
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 20);
        let transfers = ledger.transfers.lock().unwrap();
        assert_eq!(transfers.len(), 2);
        assert_eq!(transfers[0].amount, Nat::from(2_000_000u64));
        assert_eq!(transfers[1].to, relay_subaccount_one(principal(80)));
        assert_eq!(transfers[1].amount, Nat::from(772_980_000u64));
        assert!(
            transfers[1].amount
                >= config().relay_cycle_safety_margin_e8s
                    + config().relay_min_subaccount_one_seed_e8s
                    + 19 * EXTRA_TARGET_CHARGE_E8S
        );
        drop(transfers);

        #[derive(CandidType, Deserialize)]
        struct DecodedRelayInitArgs {
            managed_canisters: Vec<Principal>,
            blackhole_canister_id: Option<Principal>,
            max_transfers_per_tick: Option<u32>,
        }
        let installs = management.installs.lock().unwrap();
        assert_eq!(installs.len(), 1);
        let init: DecodedRelayInitArgs = candid::decode_one(&installs[0].arg).unwrap();
        assert_eq!(
            init.managed_canisters,
            (100..120).map(principal).collect::<Vec<_>>()
        );
        assert_eq!(init.blackhole_canister_id, None);
        assert_eq!(init.max_transfers_per_tick, Some(22));
        drop(installs);

        let key =
            RelaySetupKey::from_canonical_targets(&(100..120).map(principal).collect::<Vec<_>>());
        assert_eq!(
            get_entry(key),
            Some(RelaySetupEntry::Active {
                relay_canister_id: principal(80),
            })
        );
        state::with_state(|state| {
            for target in (100..120).map(principal) {
                assert!(state
                    .canister_tracking_reasons
                    .get(&target)
                    .unwrap()
                    .contains(&CanisterTrackingReason::RelayTarget));
            }
            assert!(state
                .canister_tracking_reasons
                .get(&principal(80))
                .unwrap()
                .contains(&CanisterTrackingReason::RelayInstance));
        });
    }

    #[test]
    fn controller_audits_compare_sets_and_reject_extras() {
        let expected_hash = approved_relay_onchain_module_hash().unwrap();
        let historian = principal(42);
        let mut status = AuditedCanisterStatus {
            status: AuditedCanisterStatusKind::Running,
            module_hash: Some(expected_hash.to_vec()),
            settings: AuditedCanisterSettings {
                controllers: vec![historian],
                log_visibility: jupiter_ic_clients::management::LogVisibility::Public,
            },
        };
        assert!(validate_pre_handoff(&status, &expected_hash, historian).is_ok());
        status.settings.controllers = vec![historian, principal(90)];
        assert!(validate_pre_handoff(&status, &expected_hash, historian).is_err());
    }
}
