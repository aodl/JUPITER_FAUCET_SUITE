mod recipient_set;
mod setup_key;
mod setup_spec;
mod target_set;

pub(crate) use setup_key::RelaySetupKey;
use setup_spec::CanonicalRelaySetup;
use target_set::CanonicalRelayTargetSet;

use crate::clients::{ClientError, CmcCanister, CmcClient, IcpXdrConversionRate, LedgerClient};
use crate::state::{self, Config};
use crate::*;
use candid::{CandidType, Encode, Principal};
use ic_stable_structures::{storable::Bound, Storable};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{BlockIndex, Memo, TransferArg, TransferError};
use jupiter_ic_clients::account::principal_to_subaccount;
use jupiter_ic_clients::cycles_probe::{
    probe_cycles_for_audience, CyclesProbeAudience, CyclesProbeClient, CyclesProbePolicy,
};
use jupiter_ic_clients::governance::NnsGovernanceCanister;
use jupiter_ic_clients::management::{
    CanisterStatusKind, CanisterStatusResult, LogVisibility, StatusVisibility,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

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
    FinalizationAttempted,
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
    async fn canister_status(&self, canister_id: Principal)
        -> Result<CanisterStatusResult, String>;
    async fn update_settings(
        &self,
        args: &jupiter_ic_clients::management::UpdateSettingsArgs,
    ) -> Result<(), String>;
}

#[async_trait::async_trait]
trait RelayNeuronResolver: Send + Sync {
    async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], String>;
}

#[async_trait::async_trait]
impl RelayNeuronResolver for NnsGovernanceCanister {
    async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], String> {
        NnsGovernanceCanister::neuron_staking_subaccount(self, neuron_id)
            .await
            .map_err(|error| error.to_string())
    }
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
    ) -> Result<CanisterStatusResult, String> {
        jupiter_ic_clients::management::canister_status(
            &jupiter_ic_clients::management::CanisterStatusArgs { canister_id },
        )
        .await
        .map_err(|err| format!("{err:?}"))
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
                | RelayCreationPhase::FinalizationAttempted => {
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

fn minimum_final_relay_funding_e8s(config: &Config, target_count: usize) -> Result<u64, String> {
    let extra = extra_target_charge_e8s(target_count)?;
    config
        .relay_min_subaccount_one_seed_e8s
        .checked_add(config.relay_cycle_safety_margin_e8s)
        .and_then(|value| value.checked_add(extra))
        .ok_or_else(|| "minimum final Relay funding overflow".to_string())
}

pub(crate) fn validate_canonical_relay_config(config: &Config) -> Result<(), String> {
    match (
        config.canonical_relay_canister_id,
        config.canonical_relay_targets.is_empty(),
    ) {
        (None, true) => Ok(()),
        (None, false) => {
            Err("canonical Relay targets require a configured canonical Relay canister".to_string())
        }
        (Some(_), true) => {
            Err("configured canonical Relay requires at least one target".to_string())
        }
        (Some(relay_canister_id), false) => {
            if relay_canister_id == Principal::anonymous() {
                return Err("canonical Relay canister must not be anonymous".to_string());
            }
            if relay_canister_id == Principal::management_canister() {
                return Err(
                    "canonical Relay canister must not be the management canister".to_string(),
                );
            }
            CanonicalRelayTargetSet::canonicalize(config.canonical_relay_targets.clone())
                .map(|_| ())
                .map_err(|err| format!("configured canonical Relay target set is invalid: {err}"))
        }
    }
}

fn reject_reserved_canonical_target_set(
    config: &Config,
    setup: &CanonicalRelaySetup,
) -> Result<(), String> {
    if config.canonical_relay_targets.is_empty() {
        return Ok(());
    }
    let canonical =
        CanonicalRelayTargetSet::canonicalize(config.canonical_relay_targets.clone())
            .map_err(|err| format!("configured canonical Relay target set is invalid: {err}"))?;
    if canonical.targets() == setup.targets() {
        return Err(
            "the canonical Jupiter Relay target set is reserved and cannot be created through self-service"
                .to_string(),
        );
    }
    Ok(())
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

pub(crate) fn setup_view(args: RelaySetupArgs) -> RelaySetupViewResult {
    setup_view_for_historian(args, self_canister_id())
}

fn setup_view_for_historian(args: RelaySetupArgs, historian: Principal) -> RelaySetupViewResult {
    state::with_state(|state| {
        let setup = match CanonicalRelaySetup::canonicalize(
            args.target_canister_ids,
            args.surplus_recipients,
        ) {
            Ok(setup) => setup,
            Err(message) => return RelaySetupViewResult::Err(message),
        };
        let key = setup.key();
        if let Err(message) = reject_reserved_canonical_target_set(&state.config, &setup) {
            return RelaySetupViewResult::Err(message);
        }
        let existing_entry = get_entry(key);
        if existing_entry.is_none() {
            if let Err(message) = setup.validate_for_new_setup(&state.config, historian) {
                return RelaySetupViewResult::Err(message);
            }
        }
        let setup_state = setup_state(existing_entry.clone());
        let active_or_blocked = existing_entry.is_some();
        let factory_available = factory_blocked_reason(&state.config).is_none();
        let expose_account = !active_or_blocked && factory_available;
        let setup_account = expose_account.then(|| setup_account_for(historian, key));
        let setup_account_identifier = setup_account
            .as_ref()
            .map(crate::clients::index::account_identifier_text_for_account);
        let target_count = setup.target_count();
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
            canonical_target_canister_ids: setup.targets().to_vec(),
            canonical_surplus_recipients: setup.surplus_recipients().to_vec(),
            setup_key_identifier: key.identifier(),
            setup_account,
            setup_account_identifier,
            target_count: u32::try_from(target_count).unwrap_or(u32::MAX),
            surplus_recipient_count: u32::try_from(setup.surplus_recipient_count())
                .unwrap_or(u32::MAX),
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

fn validate_pre_finalization(
    status: &CanisterStatusResult,
    expected_hash: &[u8; 32],
    historian: Principal,
) -> Result<(), String> {
    if status.status != CanisterStatusKind::Running {
        return Err("Relay is not running before finalization".to_string());
    }
    if status.module_hash.as_deref() != Some(expected_hash.as_slice()) {
        return Err(
            "Relay module hash does not match the approved module before finalization".to_string(),
        );
    }
    if status.settings.controllers != [historian] {
        return Err(
            "Relay controllers are not exactly [Historian] before finalization".to_string(),
        );
    }
    if status.settings.log_visibility != LogVisibility::Public {
        return Err("Relay logs are not public before finalization".to_string());
    }
    if status.settings.status_visibility != StatusVisibility::Public {
        return Err("Relay status is not public before finalization".to_string());
    }
    Ok(())
}

fn validate_finalized_relay(
    status: &CanisterStatusResult,
    expected_hash: &[u8; 32],
) -> Result<(), String> {
    if status.status != CanisterStatusKind::Running {
        return Err("Relay is not running after finalization".to_string());
    }
    if status.module_hash.as_deref() != Some(expected_hash.as_slice()) {
        return Err(
            "Relay module hash does not match the approved module after finalization".to_string(),
        );
    }
    if !status.settings.controllers.is_empty() {
        return Err("Relay controllers are not empty after finalization".to_string());
    }
    if status.settings.log_visibility != LogVisibility::Public {
        return Err("Relay logs are not public after finalization".to_string());
    }
    if status.settings.status_visibility != StatusVisibility::Public {
        return Err("Relay status is not public after finalization".to_string());
    }
    Ok(())
}

fn relay_init_arg(config: &Config, setup: &CanonicalRelaySetup) -> Vec<u8> {
    #[derive(CandidType)]
    struct SurplusCanisterRecipient {
        canister_id: Principal,
        memo: Vec<u8>,
    }
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
        sns_rewards_canister_id: Option<Principal>,
        icp_index_canister_id: Option<Principal>,
        main_interval_seconds: Option<u64>,
        max_transfers_per_tick: Option<u32>,
        surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
        surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
    }
    let transfer_cap = setup
        .target_count()
        .checked_add(setup.surplus_recipient_count())
        .and_then(|count| count.checked_add(1))
        .and_then(|count| u32::try_from(count).ok())
        .expect("self-service Relay transfer cap is bounded at 20 + 5 + 1");
    let mut principal_recipients = Vec::new();
    let mut neuron_recipients = Vec::new();
    for recipient in setup.surplus_recipients() {
        match recipient {
            RelaySurplusRecipient::Principal { principal, memo } => {
                principal_recipients.push(SurplusCanisterRecipient {
                    canister_id: *principal,
                    memo: memo.clone(),
                });
            }
            RelaySurplusRecipient::Neuron { neuron_id, memo } => {
                neuron_recipients.push(SurplusNeuronRecipient {
                    neuron_id: *neuron_id,
                    memo: memo.clone(),
                });
            }
        }
    }
    Encode!(&InitArgs {
        managed_canisters: setup.targets().to_vec(),
        ledger_canister_id: Some(config.ledger_canister_id),
        cmc_canister_id: config.cmc_canister_id,
        governance_canister_id: Some(jupiter_ic_clients::constants::nns_governance_id()),
        blackhole_canister_id: None,
        sns_rewards_canister_id: None,
        icp_index_canister_id: None,
        main_interval_seconds: Some(config.self_service_relay_interval_seconds),
        max_transfers_per_tick: Some(transfer_cap),
        surplus_canister_recipients: (!principal_recipients.is_empty())
            .then_some(principal_recipients),
        surplus_neuron_recipients: neuron_recipients,
    })
    .expect("Relay init args should encode")
}

#[allow(clippy::too_many_arguments)]
async fn notify_with_clients_for_historian<C: CyclesProbeClient>(
    args: RelaySetupArgs,
    historian: Principal,
    ledger: &dyn LedgerClient,
    cycles_probe_client: &C,
    cmc: &dyn CmcClient,
    management: &dyn ManagementClient,
) -> RelaySetupNotifyResult {
    let neuron_resolver =
        NnsGovernanceCanister::new(jupiter_ic_clients::constants::nns_governance_id());
    notify_with_clients_and_neuron_resolver(
        args,
        historian,
        ledger,
        cycles_probe_client,
        cmc,
        management,
        &neuron_resolver,
    )
    .await
}

async fn validate_neuron_recipients(
    key: RelaySetupKey,
    setup: &CanonicalRelaySetup,
    neuron_resolver: &dyn RelayNeuronResolver,
) -> Result<(), RelaySetupNotifyResult> {
    for recipient in setup.surplus_recipients() {
        let RelaySurplusRecipient::Neuron { neuron_id, .. } = recipient else {
            continue;
        };
        let resolution = neuron_resolver.neuron_staking_subaccount(*neuron_id).await;
        require_phase(key, RelayCreationPhase::Reserved)?;
        if resolution.is_err() {
            remove_reservation(key, RelayCreationPhase::Reserved);
            return Err(RelaySetupNotifyResult::FailedPreSpend {
                message: bounded_message(format!(
                    "Could not verify neuron {neuron_id} as publicly readable by NNS Governance. Check the neuron ID and try again."
                )),
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn notify_with_clients_and_neuron_resolver<C: CyclesProbeClient>(
    args: RelaySetupArgs,
    historian: Principal,
    ledger: &dyn LedgerClient,
    cycles_probe_client: &C,
    cmc: &dyn CmcClient,
    management: &dyn ManagementClient,
    neuron_resolver: &dyn RelayNeuronResolver,
) -> RelaySetupNotifyResult {
    let config = state::with_state(|state| state.config.clone());
    let setup = match CanonicalRelaySetup::canonicalize(
        args.target_canister_ids,
        args.surplus_recipients,
    ) {
        Ok(setup) => setup,
        Err(message) => return RelaySetupNotifyResult::FailedPreSpend { message },
    };
    let key = setup.key();
    if let Err(message) = reject_reserved_canonical_target_set(&config, &setup) {
        return RelaySetupNotifyResult::FailedPreSpend { message };
    }
    if let Some(entry) = get_entry(key) {
        return notify_for_entry(entry);
    }
    if let Err(message) = setup.validate_for_new_setup(&config, historian) {
        return RelaySetupNotifyResult::FailedPreSpend { message };
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
    let nominal = match nominal_minimum_e8s(&config, setup.target_count()) {
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
    let required_e8s = match current_requirement_e8s(&config, setup.target_count(), fee_e8s, &rate)
    {
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
    if let Err(result) = validate_neuron_recipients(key, &setup, neuron_resolver).await {
        return result;
    }
    if let Err(result) = update_progress(key, RelayCreationPhase::Reserved, |progress| {
        progress.phase = RelayCreationPhase::ProbingTargets;
    }) {
        return result;
    }
    for target in setup.targets() {
        let cached_route =
            state::with_state(|state| state.cached_cycles_probe_routes.get(target).cloned());
        let result = probe_cycles_for_audience(
            &CyclesProbePolicy::Auto,
            *target,
            cached_route,
            CyclesProbeAudience::AnyCanister,
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
                    log_visibility: Some(LogVisibility::Public),
                    status_visibility: Some(StatusVisibility::Public),
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
            arg: relay_init_arg(&config, &setup),
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
    let minimum_final_relay_funding =
        match minimum_final_relay_funding_e8s(&config, setup.target_count()) {
            Ok(amount) => amount,
            Err(message) => {
                return manual_recovery(key, RelayCreationPhase::CodeInstalled, message);
            }
        };
    let funding_amount = match remaining_balance.checked_sub(funding_fee) {
        Some(amount) if amount >= minimum_final_relay_funding => amount,
        Some(_) => {
            return manual_recovery(
                key,
                RelayCreationPhase::CodeInstalled,
                "post-conversion balance is below the promised minimum Relay funding",
            );
        }
        None => {
            return manual_recovery(
                key,
                RelayCreationPhase::CodeInstalled,
                "post-conversion balance cannot cover the current ledger fee",
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
    let pre_finalization = management.canister_status(relay_canister_id).await;
    if let Err(result) = require_phase(key, RelayCreationPhase::RelayFunded) {
        return result;
    }
    match pre_finalization {
        Ok(status) => {
            if let Err(message) = validate_pre_finalization(&status, &expected_hash, historian) {
                return manual_recovery(key, RelayCreationPhase::RelayFunded, message);
            }
        }
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::RelayFunded,
                format!("pre-finalization management audit failed: {error}"),
            );
        }
    }
    if let Err(result) = update_progress(key, RelayCreationPhase::RelayFunded, |progress| {
        progress.phase = RelayCreationPhase::FinalizationAttempted;
    }) {
        return result;
    }
    let finalization_result = management
        .update_settings(&jupiter_ic_clients::management::UpdateSettingsArgs {
            canister_id: relay_canister_id,
            settings: jupiter_ic_clients::management::CanisterSettings {
                controllers: Some(Vec::new()),
                log_visibility: Some(LogVisibility::Public),
                status_visibility: Some(StatusVisibility::Public),
            },
        })
        .await;
    if let Err(result) = require_phase(key, RelayCreationPhase::FinalizationAttempted) {
        return result;
    }
    let finalized_status = management.canister_status(relay_canister_id).await;
    if let Err(result) = require_phase(key, RelayCreationPhase::FinalizationAttempted) {
        return result;
    }
    match finalized_status {
        Ok(status) => {
            if let Err(message) = validate_finalized_relay(&status, &expected_hash) {
                let prefix = finalization_result
                    .err()
                    .map(|error| format!("update_settings reported {error}; "))
                    .unwrap_or_default();
                return manual_recovery(
                    key,
                    RelayCreationPhase::FinalizationAttempted,
                    format!("{prefix}{message}"),
                );
            }
        }
        Err(error) => {
            return manual_recovery(
                key,
                RelayCreationPhase::FinalizationAttempted,
                format!("post-finalization direct management audit failed: {error}"),
            );
        }
    }
    let final_progress = match require_phase(key, RelayCreationPhase::FinalizationAttempted) {
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
            RelayCreationPhase::FinalizationAttempted,
            "activation prerequisites are incomplete",
        );
    }
    insert_entry(key, RelaySetupEntry::Active { relay_canister_id });
    state::with_state_mut_sections(state::DIRTY_ROOT | state::DIRTY_REGISTRY, |state| {
        for target in setup.targets() {
            mark_active_relay_tracked(state, *target, relay_canister_id, Some(now_secs()));
        }
    });
    RelaySetupNotifyResult::Active { relay_canister_id }
}

pub(crate) async fn notify_relay_configuration(args: RelaySetupArgs) -> RelaySetupNotifyResult {
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
    notify_with_clients_for_historian(
        args,
        self_canister_id(),
        &ledger,
        &cycles_probe_client,
        &cmc,
        &IcManagementClient,
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
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::task::{Context, Poll};

    type AuditedCanisterStatus = CanisterStatusResult;
    type AuditedCanisterStatusKind = CanisterStatusKind;
    type AuditedCanisterSettings = jupiter_ic_clients::management::DefiniteCanisterSettings;

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn principal_recipient(principal: Principal, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Principal { principal, memo }
    }

    fn neuron_recipient(neuron_id: u64, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Neuron { neuron_id, memo }
    }

    fn setup_args(target_canister_ids: Vec<Principal>) -> RelaySetupArgs {
        RelaySetupArgs {
            target_canister_ids,
            surplus_recipients: vec![principal_recipient(principal(200), vec![])],
        }
    }

    fn neuron_setup_args(target: Principal, neuron_id: u64) -> RelaySetupArgs {
        RelaySetupArgs {
            target_canister_ids: vec![target],
            surplus_recipients: vec![neuron_recipient(neuron_id, vec![])],
        }
    }

    fn key_for_targets(target_canister_ids: &[Principal]) -> RelaySetupKey {
        CanonicalRelaySetup::canonicalize(
            target_canister_ids.to_vec(),
            vec![principal_recipient(principal(200), vec![])],
        )
        .expect("test setup should be structurally valid")
        .key()
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
        Duplicate(u64),
        Rejected,
        Ambiguous,
    }

    struct MockLedger {
        balances: Mutex<VecDeque<u64>>,
        fees: Mutex<VecDeque<u64>>,
        fee_failures: Mutex<VecDeque<bool>>,
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
                fees: Mutex::new(VecDeque::new()),
                fee_failures: Mutex::new(VecDeque::new()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                balance_calls: AtomicUsize::new(0),
                fee_calls: AtomicUsize::new(0),
                transfers: Mutex::new(Vec::new()),
            }
        }

        fn with_fees(mut self, fees: impl IntoIterator<Item = u64>) -> Self {
            self.fees = Mutex::new(fees.into_iter().collect());
            self
        }

        fn with_fee_failures(mut self, failures: impl IntoIterator<Item = bool>) -> Self {
            self.fee_failures = Mutex::new(failures.into_iter().collect());
            self
        }
    }

    #[async_trait::async_trait]
    impl LedgerClient for MockLedger {
        async fn fee_e8s(&self) -> Result<u64, ClientError> {
            self.fee_calls.fetch_add(1, Ordering::SeqCst);
            if self
                .fee_failures
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(false)
            {
                return Err(ClientError::Call("ledger fee read failed".to_string()));
            }
            Ok(self.fees.lock().unwrap().pop_front().unwrap_or(10_000))
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
                Some(LedgerOutcome::Duplicate(block)) => Ok(Err(TransferError::Duplicate {
                    duplicate_of: Nat::from(block),
                })),
                Some(LedgerOutcome::Rejected) => Ok(Err(TransferError::TemporarilyUnavailable)),
                Some(LedgerOutcome::Ambiguous) => {
                    Err(ClientError::Call("ambiguous ledger transport".to_string()))
                }
                None => Err(ClientError::Call("unexpected transfer call".to_string())),
            }
        }
    }

    struct MockCmc {
        rate_calls: AtomicUsize,
        notify_calls: AtomicUsize,
        rate_error: Option<String>,
        notify_error: Option<String>,
        minted_cycles: u128,
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
            self.rate_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.rate_error {
                return Err(ClientError::Call(error.clone()));
            }
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
            if let Some(error) = &self.notify_error {
                Err(jupiter_ic_clients::cmc::NotifyTopUpError::Transport(
                    error.clone(),
                ))
            } else {
                Ok(self.minted_cycles)
            }
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
        direct_visibility: StatusVisibility,
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

        async fn direct_canister_status(
            &self,
            target: Principal,
        ) -> Result<
            jupiter_ic_clients::cycles_probe::DirectCanisterStatusObservation,
            jupiter_ic_clients::ClientError,
        > {
            self.calls.lock().unwrap().push(format!("direct:{target}"));
            if target == self.direct_target {
                Ok(
                    jupiter_ic_clients::cycles_probe::DirectCanisterStatusObservation {
                        cycles: 1_000_000,
                        status_visibility: self.direct_visibility.clone(),
                    },
                )
            } else {
                Err(jupiter_ic_clients::ClientError::Call(
                    "direct status denied".to_string(),
                ))
            }
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

        async fn direct_canister_status(
            &self,
            _target: Principal,
        ) -> Result<
            jupiter_ic_clients::cycles_probe::DirectCanisterStatusObservation,
            jupiter_ic_clients::ClientError,
        > {
            Err(jupiter_ic_clients::ClientError::Call(
                "direct canister_status unavailable in yielding mock".to_string(),
            ))
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

        async fn direct_canister_status(
            &self,
            _target: Principal,
        ) -> Result<
            jupiter_ic_clients::cycles_probe::DirectCanisterStatusObservation,
            jupiter_ic_clients::ClientError,
        > {
            Err(jupiter_ic_clients::ClientError::Call(
                "direct canister_status unavailable in relay setup mock".to_string(),
            ))
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
        install_error: Option<String>,
        update_error: Option<String>,
        create_calls: AtomicUsize,
        status_calls: AtomicUsize,
        update_calls: AtomicUsize,
        clear_cycles_minted_on_final_status: bool,
        creates: Mutex<Vec<jupiter_ic_clients::management::CreateCanisterArgs>>,
        updates: Mutex<Vec<jupiter_ic_clients::management::UpdateSettingsArgs>>,
        finalization_phase_observed_at_update: AtomicBool,
        installs: Mutex<Vec<jupiter_ic_clients::management::InstallCodeArgs>>,
        status_results: Mutex<VecDeque<Result<AuditedCanisterStatus, String>>>,
    }

    struct MockNeuronResolver {
        calls: Mutex<Vec<u64>>,
        unreadable: Option<u64>,
        expected_reserved_key: Option<RelaySetupKey>,
        reserved_observations: Mutex<Vec<bool>>,
    }

    impl MockNeuronResolver {
        fn readable() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                unreadable: None,
                expected_reserved_key: None,
                reserved_observations: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl RelayNeuronResolver for MockNeuronResolver {
        async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], String> {
            self.calls.lock().unwrap().push(neuron_id);
            if let Some(key) = self.expected_reserved_key {
                self.reserved_observations.lock().unwrap().push(matches!(
                    get_entry(key),
                    Some(RelaySetupEntry::Creating(RelayCreationProgress {
                        phase: RelayCreationPhase::Reserved,
                        ..
                    }))
                ));
            }
            if self.unreadable == Some(neuron_id) {
                Err("neuron is not public".to_string())
            } else {
                let mut subaccount = [0u8; 32];
                subaccount[24..].copy_from_slice(&neuron_id.to_be_bytes());
                Ok(subaccount)
            }
        }
    }

    struct YieldingNeuronResolver {
        calls: Mutex<Vec<u64>>,
        unreadable: bool,
    }

    struct BlockingNeuronResolver {
        calls: Mutex<Vec<u64>>,
        waiters: Mutex<VecDeque<oneshot::Receiver<()>>>,
    }

    struct SupersedingNeuronResolver {
        key: RelaySetupKey,
        relay_canister_id: Principal,
    }

    #[async_trait::async_trait]
    impl RelayNeuronResolver for YieldingNeuronResolver {
        async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], String> {
            self.calls.lock().unwrap().push(neuron_id);
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
            if self.unreadable {
                Err("neuron cannot be read".to_string())
            } else {
                let mut subaccount = [0u8; 32];
                subaccount[24..].copy_from_slice(&neuron_id.to_be_bytes());
                Ok(subaccount)
            }
        }
    }

    #[async_trait::async_trait]
    impl RelayNeuronResolver for BlockingNeuronResolver {
        async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], String> {
            self.calls.lock().unwrap().push(neuron_id);
            let waiter = self.waiters.lock().unwrap().pop_front().unwrap();
            let _ = waiter.await;
            Err("neuron cannot be read".to_string())
        }
    }

    #[async_trait::async_trait]
    impl RelayNeuronResolver for SupersedingNeuronResolver {
        async fn neuron_staking_subaccount(&self, _neuron_id: u64) -> Result<[u8; 32], String> {
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
            insert_entry(
                self.key,
                RelaySetupEntry::Active {
                    relay_canister_id: self.relay_canister_id,
                },
            );
            Err("stale validation result".to_string())
        }
    }

    #[async_trait::async_trait]
    impl ManagementClient for MockManagement {
        async fn create_canister(
            &self,
            args: &jupiter_ic_clients::management::CreateCanisterArgs,
            _cycles: u128,
        ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, String> {
            self.create_calls.fetch_add(1, Ordering::SeqCst);
            self.creates.lock().unwrap().push(args.clone());
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
            self.install_error.clone().map_or(Ok(()), Err)
        }

        async fn canister_status(
            &self,
            _canister_id: Principal,
        ) -> Result<AuditedCanisterStatus, String> {
            let call_index = self.status_calls.fetch_add(1, Ordering::SeqCst);
            if call_index == 1 && self.clear_cycles_minted_on_final_status {
                state::with_relay_setup_entries_map(|map| {
                    let Some(entry) = map.iter().next() else {
                        return;
                    };
                    let (key, value) = entry.into_pair();
                    if let RelaySetupEntry::Creating(mut progress) = value {
                        progress.cycles_minted = None;
                        map.insert(key, RelaySetupEntry::Creating(progress));
                    }
                });
            }
            if let Some(result) = self.status_results.lock().unwrap().pop_front() {
                return result;
            }
            Ok(AuditedCanisterStatus {
                status: AuditedCanisterStatusKind::Running,
                cycles: Nat::from(1_000_000_u64),
                module_hash: approved_relay_onchain_module_hash().map(|hash| hash.to_vec()),
                settings: AuditedCanisterSettings {
                    controllers: if call_index == 0 {
                        vec![principal(42)]
                    } else {
                        Vec::new()
                    },
                    log_visibility: LogVisibility::Public,
                    status_visibility: StatusVisibility::Public,
                },
            })
        }

        async fn update_settings(
            &self,
            args: &jupiter_ic_clients::management::UpdateSettingsArgs,
        ) -> Result<(), String> {
            self.update_calls.fetch_add(1, Ordering::SeqCst);
            self.updates.lock().unwrap().push(args.clone());
            self.finalization_phase_observed_at_update.store(
                state::with_relay_setup_entries_map(|map| {
                    map.iter().any(|entry| {
                        matches!(
                            entry.value(),
                            RelaySetupEntry::Creating(RelayCreationProgress {
                                phase: RelayCreationPhase::FinalizationAttempted,
                                ..
                            })
                        )
                    })
                }),
                Ordering::SeqCst,
            );
            self.update_error.clone().map_or(Ok(()), Err)
        }
    }

    fn mocks(
        ledger: MockLedger,
        probe_succeeds: bool,
        create_error: Option<&str>,
    ) -> (MockLedger, MockCyclesProbe, MockCmc, MockManagement) {
        (
            ledger,
            MockCyclesProbe {
                succeeds: probe_succeeds,
                blackhole_calls: AtomicUsize::new(0),
            },
            MockCmc {
                rate_calls: AtomicUsize::new(0),
                notify_calls: AtomicUsize::new(0),
                rate_error: None,
                notify_error: None,
                minted_cycles: 2_000_000_000_000,
            },
            MockManagement {
                relay_id: principal(80),
                create_error: create_error.map(str::to_string),
                install_error: None,
                update_error: None,
                create_calls: AtomicUsize::new(0),
                status_calls: AtomicUsize::new(0),
                update_calls: AtomicUsize::new(0),
                clear_cycles_minted_on_final_status: false,
                creates: Mutex::new(Vec::new()),
                updates: Mutex::new(Vec::new()),
                finalization_phase_observed_at_update: AtomicBool::new(false),
                installs: Mutex::new(Vec::new()),
                status_results: Mutex::new(VecDeque::new()),
            },
        )
    }

    fn audited_relay_status(controllers: Vec<Principal>) -> AuditedCanisterStatus {
        AuditedCanisterStatus {
            status: CanisterStatusKind::Running,
            cycles: Nat::from(1_000_000_u64),
            module_hash: approved_relay_onchain_module_hash().map(|hash| hash.to_vec()),
            settings: AuditedCanisterSettings {
                controllers,
                log_visibility: LogVisibility::Public,
                status_visibility: StatusVisibility::Public,
            },
        }
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
        assert_eq!(minimum_final_relay_funding_e8s(&cfg, 1), Ok(105_020_000));
        assert_eq!(minimum_final_relay_funding_e8s(&cfg, 2), Ok(130_020_000));
        assert_eq!(minimum_final_relay_funding_e8s(&cfg, 20), Ok(580_020_000));
        assert_eq!(
            current_requirement_e8s(&cfg, 1, 10_000, &rate),
            Ok(300_000_000)
        );
    }

    #[test]
    fn canonical_relay_config_accepts_structural_sets_and_rejects_invalid_pairings() {
        let mut cfg = config();
        cfg.canonical_relay_canister_id = None;
        cfg.canonical_relay_targets.clear();
        assert_eq!(validate_canonical_relay_config(&cfg), Ok(()));

        cfg.canonical_relay_canister_id = Some(principal(70));
        cfg.canonical_relay_targets = (0..255)
            .map(|index| Principal::from_slice(&[0x7f, (index >> 8) as u8, index as u8]))
            .collect();
        assert_eq!(validate_canonical_relay_config(&cfg), Ok(()));
        validate_config(&cfg);
        cfg.canonical_relay_targets.truncate(21);
        assert_eq!(validate_canonical_relay_config(&cfg), Ok(()));
        validate_config(&cfg);

        cfg.canonical_relay_targets
            .push(cfg.canonical_relay_targets[0]);
        assert!(validate_canonical_relay_config(&cfg).is_err());
        cfg.canonical_relay_targets = (0..256)
            .map(|index| Principal::from_slice(&[0x7f, (index >> 8) as u8, index as u8]))
            .collect();
        assert!(validate_canonical_relay_config(&cfg).is_err());

        cfg.canonical_relay_targets.clear();
        assert!(validate_canonical_relay_config(&cfg).is_err());
        cfg.canonical_relay_targets = vec![principal(1)];
        cfg.canonical_relay_canister_id = None;
        assert!(validate_canonical_relay_config(&cfg).is_err());
        cfg.canonical_relay_canister_id = Some(Principal::anonymous());
        assert!(validate_canonical_relay_config(&cfg).is_err());
        cfg.canonical_relay_canister_id = Some(Principal::management_canister());
        assert!(validate_canonical_relay_config(&cfg).is_err());
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
        let progress = progress_for_phase(RelayCreationPhase::FinalizationAttempted);
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
            RelayCreationPhase::FinalizationAttempted,
        ];

        for (index, phase) in retryable.into_iter().enumerate() {
            reset();
            let key = key_for_targets(&[principal(index as u8 + 1)]);
            insert_entry(key, RelaySetupEntry::Creating(progress_for_phase(phase)));
            reconcile_interrupted_creating_entries_after_upgrade();
            assert_eq!(get_entry(key), None, "phase {phase:?}");
        }
        for (index, phase) in manual.into_iter().enumerate() {
            reset();
            let key = key_for_targets(&[principal(index as u8 + 20)]);
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
        let active_key = key_for_targets(&[principal(90)]);
        let active = RelaySetupEntry::Active {
            relay_canister_id: principal(91),
        };
        let active_bytes = active.to_bytes().into_owned();
        insert_entry(active_key, active);
        let manual_key = key_for_targets(&[principal(92)]);
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
        let key = key_for_targets(&[principal(1)]);
        assert_eq!(
            setup_account_for(historian, key).subaccount,
            Some(key.bytes())
        );
    }

    #[test]
    fn setup_view_exposes_authoritative_accounts_for_supported_configurations() {
        reset();
        let historian = principal(42);
        let target = principal(1);
        let empty_memo_args = RelaySetupArgs {
            target_canister_ids: vec![target],
            surplus_recipients: vec![principal_recipient(principal(2), vec![])],
        };
        let memo_args = RelaySetupArgs {
            target_canister_ids: vec![target],
            surplus_recipients: vec![principal_recipient(principal(2), vec![0x00])],
        };
        let zero_args = RelaySetupArgs {
            target_canister_ids: vec![target],
            surplus_recipients: vec![],
        };

        let RelaySetupViewResult::Ok(empty_memo) =
            setup_view_for_historian(empty_memo_args.clone(), historian)
        else {
            panic!("empty-memo setup view must be available")
        };
        let RelaySetupViewResult::Ok(memo) = setup_view_for_historian(memo_args, historian) else {
            panic!("memo setup view must be available")
        };
        let RelaySetupViewResult::Ok(zero) = setup_view_for_historian(zero_args, historian) else {
            panic!("zero-recipient setup view must be available")
        };
        assert_eq!(
            empty_memo.setup_account.unwrap().subaccount,
            Some([
                0x3a, 0x0e, 0xd7, 0x35, 0xd0, 0x92, 0xe7, 0x5b, 0xcd, 0x92, 0x9d, 0x71, 0x23, 0x5e,
                0x2b, 0x37, 0x57, 0xaf, 0x7e, 0x7c, 0x50, 0xd3, 0x42, 0x42, 0x75, 0x92, 0xcb, 0xf1,
                0xa1, 0xbb, 0x0b, 0x1f
            ])
        );
        assert_ne!(empty_memo.setup_key_identifier, memo.setup_key_identifier);
        assert_ne!(empty_memo.setup_key_identifier, zero.setup_key_identifier);
        assert_eq!(zero.surplus_recipient_count, 0);
        assert_eq!(state::with_relay_setup_entries_map(|map| map.len()), 0);

        let empty_memo_key = CanonicalRelaySetup::canonicalize(
            empty_memo_args.target_canister_ids.clone(),
            empty_memo_args.surplus_recipients.clone(),
        )
        .unwrap()
        .key();
        insert_entry(
            empty_memo_key,
            RelaySetupEntry::Active {
                relay_canister_id: principal(73),
            },
        );
        assert!(matches!(
            setup_view_for_historian(empty_memo_args, historian),
            RelaySetupViewResult::Ok(RelaySetupView {
                setup_account: None,
                state: RelaySetupState::Active { relay_canister_id },
                ..
            }) if relay_canister_id == principal(73)
        ));

        let before_entries = state::with_relay_setup_entries_map(|map| map.len());
        let overlong = setup_view_for_historian(
            RelaySetupArgs {
                target_canister_ids: vec![target],
                surplus_recipients: vec![principal_recipient(principal(3), vec![0xff; 33])],
            },
            historian,
        );
        assert!(matches!(
            overlong,
            RelaySetupViewResult::Err(message)
                if message.contains(&principal(3).to_text()) && message.contains("33 bytes")
        ));
        assert_eq!(
            state::with_relay_setup_entries_map(|map| map.len()),
            before_entries
        );
    }

    #[test]
    fn recipient_changes_produce_independent_keys_accounts_and_reservations() {
        reset();
        let historian = principal(42);
        let setup_a = CanonicalRelaySetup::canonicalize(
            vec![principal(1)],
            vec![principal_recipient(principal(2), vec![])],
        )
        .unwrap();
        let setup_b = CanonicalRelaySetup::canonicalize(
            vec![principal(1)],
            vec![principal_recipient(principal(3), vec![])],
        )
        .unwrap();
        assert_ne!(setup_a.key(), setup_b.key());
        assert_ne!(
            setup_account_for(historian, setup_a.key()),
            setup_account_for(historian, setup_b.key())
        );
        assert!(reserve(setup_a.key()).is_ok());
        assert!(reserve(setup_b.key()).is_ok());
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
        let a = key_for_targets(&[principal(1)]);
        let ab = key_for_targets(&[principal(1), principal(2)]);
        let bc = key_for_targets(&[principal(2), principal(3)]);
        let ba = key_for_targets(&[principal(2), principal(1)]);
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
        for byte in [1, 2, 3, 5] {
            assert!(reserve(key_for_targets(&[principal(byte)])).is_ok());
        }
        assert_eq!(
            reserve(key_for_targets(&[principal(6)])),
            Err(RelaySetupNotifyResult::Busy)
        );
    }

    #[test]
    fn canonical_exact_set_is_reserved_for_self_service() {
        clear_setup_entries_for_debug();
        let historian = principal(42);
        let mut cfg = config();
        cfg.canonical_relay_targets = vec![
            historian,
            jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
        ];
        state::set_state(State::new(cfg, 0));
        let result = setup_view_for_historian(
            setup_args(vec![
                jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
                historian,
            ]),
            historian,
        );
        assert_eq!(
            result,
            RelaySetupViewResult::Err(
                "the canonical Jupiter Relay target set is reserved and cannot be created through self-service"
                    .to_string()
            )
        );
    }

    #[test]
    fn canonical_exact_set_notify_is_rejected_before_external_work() {
        reset();
        let historian = principal(42);
        state::with_state_mut_sections(state::DIRTY_ROOT, |state| {
            state.config.canonical_relay_targets = vec![principal(1), principal(2)];
        });
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([], []), false, Some("must not create"));
        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![principal(2), principal(1)]),
            historian,
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::FailedPreSpend { message } if message.contains("reserved")
        ));
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn canonical_exact_set_is_rejected_before_protected_target_policy() {
        clear_setup_entries_for_debug();
        let historian = principal(42);
        let mut cfg = config();
        let protected = vec![
            historian,
            jupiter_ic_clients::constants::fiduciary_blackhole_canister_id(),
            cfg.ledger_canister_id,
            cfg.index_canister_id,
            cfg.cmc_canister_id.unwrap(),
        ];
        cfg.canonical_relay_targets = protected.clone();
        assert_eq!(validate_canonical_relay_config(&cfg), Ok(()));
        state::set_state(State::new(cfg, 0));

        let exact = setup_view_for_historian(setup_args(protected.clone()), historian);
        assert!(
            matches!(exact, RelaySetupViewResult::Err(message) if message.contains("reserved"))
        );

        for target in protected {
            assert!(matches!(
                setup_view_for_historian(setup_args(vec![target]), historian,),
                RelaySetupViewResult::Err(_)
            ));
        }
    }

    #[test]
    fn active_entry_precedes_new_target_policy_for_ledger_index_and_cmc() {
        let historian = principal(42);
        let target = principal(1);
        let relay = principal(73);

        for protected_field in ["ledger", "index", "cmc"] {
            reset();
            let key = key_for_targets(&[target]);
            insert_entry(
                key,
                RelaySetupEntry::Active {
                    relay_canister_id: relay,
                },
            );
            state::with_state_mut_sections(state::DIRTY_ROOT, |state| match protected_field {
                "ledger" => state.config.ledger_canister_id = target,
                "index" => state.config.index_canister_id = target,
                "cmc" => state.config.cmc_canister_id = Some(target),
                _ => unreachable!(),
            });

            let view = setup_view_for_historian(setup_args(vec![target]), historian);
            assert!(matches!(
                view,
                RelaySetupViewResult::Ok(RelaySetupView {
                    state: RelaySetupState::Active { relay_canister_id },
                    ..
                }) if relay_canister_id == relay
            ));

            let (ledger, probe, cmc, management) =
                mocks(MockLedger::new([], []), false, Some("must not create"));
            let notify = block_on(notify_with_clients_for_historian(
                setup_args(vec![target]),
                historian,
                &ledger,
                &probe,
                &cmc,
                &management,
            ));
            assert_eq!(
                notify,
                RelaySetupNotifyResult::Active {
                    relay_canister_id: relay,
                }
            );
            assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 0);
            assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 0);
            assert!(ledger.transfers.lock().unwrap().is_empty());
            assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
            assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 0);
            assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
            assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
            assert_eq!(management.status_calls.load(Ordering::SeqCst), 0);
            assert_eq!(management.update_calls.load(Ordering::SeqCst), 0);
            assert!(management.installs.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn manual_recovery_entry_precedes_new_target_policy_without_mutation_or_calls() {
        reset();
        let historian = principal(42);
        let target = principal(1);
        let key = key_for_targets(&[target]);
        let mut progress = progress_for_phase(RelayCreationPhase::CodeInstalled);
        progress.relay_canister_id = Some(principal(73));
        progress.last_error = Some("stored manual recovery detail".to_string());
        let entry = RelaySetupEntry::ManualRecoveryRequired(progress.clone());
        let original_bytes = entry.to_bytes().into_owned();
        insert_entry(key, entry);
        state::with_state_mut_sections(state::DIRTY_ROOT, |state| {
            state.config.faucet_canister_id = Some(target);
            state.config.relay_factory_enabled = false;
        });

        let view = setup_view_for_historian(setup_args(vec![target]), historian);
        assert!(matches!(
            view,
            RelaySetupViewResult::Ok(RelaySetupView {
                state: RelaySetupState::ManualRecoveryRequired {
                    phase: RelayCreationPhase::CodeInstalled,
                    relay_canister_id: Some(relay_canister_id),
                    message,
                },
                ..
            }) if relay_canister_id == principal(73) && message == "stored manual recovery detail"
        ));

        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([], []), false, Some("must not create"));
        let notify = block_on(notify_with_clients_for_historian(
            setup_args(vec![target]),
            historian,
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert_eq!(
            notify,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CodeInstalled,
                relay_canister_id: Some(principal(73)),
                message: "stored manual recovery detail".to_string(),
            }
        );
        assert_eq!(
            get_entry(key).unwrap().to_bytes().as_ref(),
            original_bytes.as_slice()
        );
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.update_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn creating_entry_precedes_new_target_policy_without_external_work() {
        reset();
        let historian = principal(42);
        let target = principal(1);
        let key = key_for_targets(&[target]);
        let mut progress = progress_for_phase(RelayCreationPhase::CmcTransferAccepted);
        progress.relay_canister_id = None;
        insert_entry(key, RelaySetupEntry::Creating(progress));
        state::with_state_mut_sections(state::DIRTY_ROOT, |state| {
            state.config.cmc_canister_id = Some(target);
        });

        let view = setup_view_for_historian(setup_args(vec![target]), historian);
        assert!(matches!(
            view,
            RelaySetupViewResult::Ok(RelaySetupView {
                state: RelaySetupState::InProgress {
                    phase: RelayCreationPhase::CmcTransferAccepted,
                    relay_canister_id: None,
                },
                ..
            })
        ));

        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([], []), false, Some("must not create"));
        let notify = block_on(notify_with_clients_for_historian(
            setup_args(vec![target]),
            historian,
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert_eq!(
            notify,
            RelaySetupNotifyResult::InProgress {
                phase: RelayCreationPhase::CmcTransferAccepted,
                relay_canister_id: None,
            }
        );
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.update_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn static_and_underfunded_calls_make_only_the_permitted_external_reads() {
        reset();
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([299_999_999], []), true, None);
        let invalid = block_on(notify_with_clients_for_historian(
            setup_args(vec![Principal::anonymous()]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            invalid,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 0);

        let below = block_on(notify_with_clients_for_historian(
            setup_args(vec![principal(1)]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
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
            setup_args(vec![principal(2)]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            disabled_result,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unfunded_neuron_configuration_does_not_call_governance_or_reserve() {
        reset();
        let args = neuron_setup_args(principal(1), 42);
        let (ledger, probe, cmc, management) = mocks(MockLedger::new([0], []), true, None);
        let resolver = MockNeuronResolver::readable();

        let result = block_on(notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));

        assert_eq!(
            result,
            RelaySetupNotifyResult::BelowMinimum {
                balance_e8s: 0,
                required_e8s: 300_000_000,
                shortfall_e8s: 300_000_000,
            }
        );
        assert!(resolver.calls.lock().unwrap().is_empty());
        assert!(debug_setup_entries().is_empty());
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.update_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn below_current_requirement_neuron_configuration_does_not_call_governance_or_reserve() {
        clear_setup_entries_for_debug();
        let mut cfg = config();
        cfg.relay_setup_min_e8s = 1;
        state::set_state(State::new(cfg, 0));
        let args = neuron_setup_args(principal(1), 42);
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([107_039_999], []), true, None);
        let resolver = MockNeuronResolver::readable();

        let result = block_on(notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));

        assert_eq!(
            result,
            RelaySetupNotifyResult::BelowCurrentRequirement {
                balance_e8s: 107_039_999,
                required_e8s: 107_040_000,
                shortfall_e8s: 1,
            }
        );
        assert!(resolver.calls.lock().unwrap().is_empty());
        assert!(debug_setup_entries().is_empty());
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 1);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.update_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn pre_spend_read_and_live_requirement_failures_leave_no_durable_or_external_work() {
        let args = setup_args(vec![principal(1)]);

        reset();
        let (ledger, probe, cmc, management) = mocks(MockLedger::new([], []), true, None);
        let balance_failure = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            balance_failure,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert!(debug_setup_entries().is_empty());
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);

        reset();
        let ledger = MockLedger::new([300_000_000], []).with_fee_failures([true]);
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);
        let fee_failure = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            fee_failure,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert!(debug_setup_entries().is_empty());
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);

        reset();
        let (ledger, probe, mut cmc, management) =
            mocks(MockLedger::new([300_000_000], []), true, None);
        cmc.rate_error = Some("CMC rate unavailable".to_string());
        let rate_failure = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            rate_failure,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert!(debug_setup_entries().is_empty());
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);

        clear_setup_entries_for_debug();
        let mut cfg = config();
        cfg.relay_setup_min_e8s = 1;
        state::set_state(State::new(cfg, 0));
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([107_039_999], []), true, None);
        let below_current = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert_eq!(
            below_current,
            RelaySetupNotifyResult::BelowCurrentRequirement {
                balance_e8s: 107_039_999,
                required_e8s: 107_040_000,
                shortfall_e8s: 1,
            }
        );
        assert!(debug_setup_entries().is_empty());
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn failed_probe_removes_reservation_before_any_spend() {
        reset();
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([300_000_000], []), false, None);
        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![principal(1)]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
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
            direct_visibility: StatusVisibility::Public,
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
            rate_calls: AtomicUsize::new(0),
            notify_calls: AtomicUsize::new(0),
            rate_error: None,
            notify_error: None,
            minted_cycles: 2_000_000_000_000,
        };
        let management = MockManagement {
            clear_cycles_minted_on_final_status: false,
            creates: Mutex::new(Vec::new()),
            updates: Mutex::new(Vec::new()),
            finalization_phase_observed_at_update: AtomicBool::new(false),
            relay_id: principal(80),
            create_error: None,
            install_error: None,
            update_error: None,
            create_calls: AtomicUsize::new(0),
            status_calls: AtomicUsize::new(0),
            update_calls: AtomicUsize::new(0),
            installs: Mutex::new(Vec::new()),
            status_results: Mutex::new(VecDeque::new()),
        };
        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![sns_swap_target, direct_target]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(result, RelaySetupNotifyResult::Active { .. }));
        let calls = probe.calls.lock().unwrap();
        assert_eq!(calls[0], format!("direct:{direct_target}"));
        assert_eq!(calls[1], format!("direct:{sns_swap_target}"));
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
    fn non_public_direct_target_without_reusable_fallback_fails_before_spend() {
        reset();
        let target = principal(1);
        let probe = MixedRouteCyclesProbe {
            direct_target: target,
            direct_visibility: StatusVisibility::Controllers,
            sns_swap_target: target,
            sns_root: principal(3),
            expose_sns_route: false,
            calls: Mutex::new(Vec::new()),
        };
        let ledger = MockLedger::new([325_000_000], []);
        let (_, _, cmc, management) = mocks(MockLedger::new([], []), true, None);

        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![target]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));

        assert!(matches!(
            result,
            RelaySetupNotifyResult::FailedPreSpend { message }
                if message.contains("not reusable by any canister")
        ));
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            probe
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| *call == &format!("direct:{target}"))
                .count(),
            1
        );
        assert!(debug_setup_entries().is_empty());
    }

    #[test]
    fn non_public_direct_target_qualifies_through_reusable_sns_fallback() {
        reset();
        let target = principal(1);
        let root = principal(3);
        let probe = MixedRouteCyclesProbe {
            direct_target: target,
            direct_visibility: StatusVisibility::AllowedViewers(vec![principal(42)]),
            sns_swap_target: target,
            sns_root: root,
            expose_sns_route: true,
            calls: Mutex::new(Vec::new()),
        };
        let ledger = MockLedger::new(
            [325_000_000, 322_990_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        );
        let (_, _, cmc, management) = mocks(MockLedger::new([], []), true, None);

        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![target]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));

        assert!(matches!(result, RelaySetupNotifyResult::Active { .. }));
        let accepted_route =
            state::with_state(|st| st.cached_cycles_probe_routes.get(&target).cloned());
        assert_eq!(
            accepted_route,
            Some(CyclesProbeRoute::SnsSwap {
                root_canister_id: root,
                swap_canister_id: target,
            })
        );
        let calls = probe.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|call| *call == &format!("direct:{target}"))
                .count(),
            1
        );
        assert!(calls
            .iter()
            .any(|call| call == &format!("sns_swap:{target}")));
    }

    #[test]
    fn one_unobservable_target_prevents_all_irreversible_operations() {
        reset();
        let direct_target = principal(1);
        let unobservable_target = principal(2);
        let probe = MixedRouteCyclesProbe {
            direct_target,
            direct_visibility: StatusVisibility::Public,
            sns_swap_target: unobservable_target,
            sns_root: principal(3),
            expose_sns_route: false,
            calls: Mutex::new(Vec::new()),
        };
        let ledger = MockLedger::new([325_000_000], []);
        let cmc = MockCmc {
            rate_calls: AtomicUsize::new(0),
            notify_calls: AtomicUsize::new(0),
            rate_error: None,
            notify_error: None,
            minted_cycles: 2_000_000_000_000,
        };
        let management = MockManagement {
            clear_cycles_minted_on_final_status: false,
            creates: Mutex::new(Vec::new()),
            updates: Mutex::new(Vec::new()),
            finalization_phase_observed_at_update: AtomicBool::new(false),
            relay_id: principal(80),
            create_error: None,
            install_error: None,
            update_error: None,
            create_calls: AtomicUsize::new(0),
            status_calls: AtomicUsize::new(0),
            update_calls: AtomicUsize::new(0),
            installs: Mutex::new(Vec::new()),
            status_results: Mutex::new(VecDeque::new()),
        };
        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![direct_target, unobservable_target]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
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
                .filter(|call| call.starts_with("direct:"))
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
        let args = setup_args(vec![principal(1)]);
        let ledger = MockLedger::new(
            [300_000_000, 300_000_000, 297_990_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        );
        let probe = YieldingCyclesProbe {
            blackhole_calls: AtomicUsize::new(0),
        };
        let cmc = ReadBarrierCmc::new();
        let management = MockManagement {
            clear_cycles_minted_on_final_status: false,
            creates: Mutex::new(Vec::new()),
            updates: Mutex::new(Vec::new()),
            finalization_phase_observed_at_update: AtomicBool::new(false),
            relay_id: principal(80),
            create_error: None,
            install_error: None,
            update_error: None,
            create_calls: AtomicUsize::new(0),
            status_calls: AtomicUsize::new(0),
            update_calls: AtomicUsize::new(0),
            installs: Mutex::new(Vec::new()),
            status_results: Mutex::new(VecDeque::new()),
        };

        let first = notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        );
        let second = notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
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
    fn same_key_contention_performs_one_governance_validation_sequence() {
        reset();
        let args = neuron_setup_args(principal(1), 42);
        let ledger = MockLedger::new(
            [400_000_000, 397_990_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        );
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);
        let resolver = YieldingNeuronResolver {
            calls: Mutex::new(Vec::new()),
            unreadable: false,
        };

        let first = notify_with_clients_and_neuron_resolver(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        );
        let second = notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        );
        let (first, second) = block_on(futures::future::join(first, second));

        assert!(matches!(
            (&first, &second),
            (
                RelaySetupNotifyResult::Active { .. },
                RelaySetupNotifyResult::InProgress {
                    phase: RelayCreationPhase::Reserved,
                    ..
                }
            ) | (
                RelaySetupNotifyResult::InProgress {
                    phase: RelayCreationPhase::Reserved,
                    ..
                },
                RelaySetupNotifyResult::Active { .. }
            )
        ));
        assert_eq!(*resolver.calls.lock().unwrap(), vec![42]);
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 2);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.transfers.lock().unwrap().len(), 2);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 1);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 1);
        assert_eq!(management.installs.lock().unwrap().len(), 1);
    }

    #[test]
    fn neuron_validation_reservations_count_toward_global_concurrency_limit() {
        reset();
        let ledger = MockLedger::new([400_000_000; 5], []);
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);
        let mut releases = Vec::new();
        let mut waiters = VecDeque::new();
        for _ in 0..MAX_CONCURRENT_FUNDED_RELAY_SETUPS {
            let (release, waiter) = oneshot::channel();
            releases.push(release);
            waiters.push_back(waiter);
        }
        let resolver = BlockingNeuronResolver {
            calls: Mutex::new(Vec::new()),
            waiters: Mutex::new(waiters),
        };

        let mut one = Box::pin(notify_with_clients_and_neuron_resolver(
            neuron_setup_args(principal(1), 1),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));
        let mut two = Box::pin(notify_with_clients_and_neuron_resolver(
            neuron_setup_args(principal(2), 2),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));
        let mut three = Box::pin(notify_with_clients_and_neuron_resolver(
            neuron_setup_args(principal(3), 3),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));
        let mut four = Box::pin(notify_with_clients_and_neuron_resolver(
            neuron_setup_args(principal(5), 4),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));
        let five = notify_with_clients_and_neuron_resolver(
            neuron_setup_args(principal(6), 5),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        );
        let mut context = Context::from_waker(futures::task::noop_waker_ref());
        assert!(matches!(one.as_mut().poll(&mut context), Poll::Pending));
        assert!(matches!(two.as_mut().poll(&mut context), Poll::Pending));
        assert!(matches!(three.as_mut().poll(&mut context), Poll::Pending));
        let four_poll = four.as_mut().poll(&mut context);
        assert!(
            matches!(four_poll, Poll::Pending),
            "fourth setup returned {four_poll:?}; entries: {:?}",
            debug_setup_entries()
        );
        let five = block_on(five);
        assert_eq!(resolver.calls.lock().unwrap().len(), 4);
        for sender in releases {
            let _ = sender.send(());
        }
        let one = block_on(one);
        let two = block_on(two);
        let three = block_on(three);
        let four = block_on(four);

        for result in [one, two, three, four] {
            assert!(matches!(
                result,
                RelaySetupNotifyResult::FailedPreSpend { .. }
            ));
        }
        assert_eq!(five, RelaySetupNotifyResult::Busy);
        assert_eq!(*resolver.calls.lock().unwrap(), vec![1, 2, 3, 4]);
        assert!(debug_setup_entries().is_empty());
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn cmc_transfer_and_notification_boundaries_are_journaled_without_replay() {
        let args = setup_args(vec![principal(1)]);

        reset();
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new(
                [300_000_000, 297_990_000],
                [LedgerOutcome::Duplicate(10), LedgerOutcome::Accepted(11)],
            ),
            true,
            None,
        );
        let duplicate = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert_eq!(
            duplicate,
            RelaySetupNotifyResult::Active {
                relay_canister_id: principal(80),
            }
        );
        assert_eq!(ledger.transfers.lock().unwrap().len(), 2);

        reset();
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Rejected]),
            true,
            None,
        );
        let rejected = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            rejected,
            RelaySetupNotifyResult::FailedPreSpend { .. }
        ));
        assert!(debug_setup_entries().is_empty());
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);

        reset();
        let (ledger, probe, mut cmc, management) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Accepted(10)]),
            true,
            None,
        );
        cmc.notify_error = Some("CMC notification transport failed".to_string());
        let notify_error = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            notify_error,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CmcTransferAccepted,
                ..
            }
        ));
        let repeated = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            repeated,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 1);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);

        reset();
        let (ledger, probe, mut cmc, management) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Accepted(10)]),
            true,
            None,
        );
        cmc.minted_cycles = config().relay_initial_cycles - 1;
        let insufficient_cycles = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            insufficient_cycles,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CmcTransferAccepted,
                ..
            }
        ));
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 1);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn ambiguous_transfer_and_dispatched_create_are_never_replayed() {
        reset();
        let args = setup_args(vec![principal(1)]);
        let (ledger, probe, cmc, management) = mocks(
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
        ));
        assert!(matches!(
            second,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);

        reset();
        let (ledger, probe, cmc, management) = mocks(
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
        ));
        assert!(matches!(
            second,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn install_failure_reconciliation_accepts_only_the_approved_live_hash() {
        let args = setup_args(vec![principal(1)]);
        let approved_status = || AuditedCanisterStatus {
            status: AuditedCanisterStatusKind::Running,
            cycles: Nat::from(1_000_000_u64),
            module_hash: approved_relay_onchain_module_hash().map(|hash| hash.to_vec()),
            settings: AuditedCanisterSettings {
                controllers: vec![principal(42)],
                log_visibility: LogVisibility::Public,
                status_visibility: StatusVisibility::Public,
            },
        };

        reset();
        let (ledger, probe, cmc, mut management) = mocks(
            MockLedger::new(
                [300_000_000, 297_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
            ),
            true,
            None,
        );
        management.install_error = Some("install callback reported failure".to_string());
        management.status_results.lock().unwrap().extend([
            Ok(approved_status()),
            Ok(approved_status()),
            Ok(audited_relay_status(Vec::new())),
        ]);
        let reconciled = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert_eq!(
            reconciled,
            RelaySetupNotifyResult::Active {
                relay_canister_id: principal(80),
            }
        );
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 3);

        reset();
        let (ledger, probe, cmc, mut management) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Accepted(10)]),
            true,
            None,
        );
        management.install_error = Some("install failed".to_string());
        let mut wrong_hash = approved_status();
        wrong_hash.module_hash = Some(vec![0; 32]);
        management
            .status_results
            .lock()
            .unwrap()
            .push_back(Ok(wrong_hash));
        let wrong_hash_result = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            wrong_hash_result,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::ChildCreated,
                relay_canister_id: Some(relay_canister_id),
                ..
            } if relay_canister_id == principal(80)
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);

        reset();
        let (ledger, probe, cmc, mut management) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Accepted(10)]),
            true,
            None,
        );
        management.install_error = Some("install failed".to_string());
        management
            .status_results
            .lock()
            .unwrap()
            .push_back(Err("status lookup failed".to_string()));
        let status_error_result = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            status_error_result,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::ChildCreated,
                relay_canister_id: Some(relay_canister_id),
                ..
            } if relay_canister_id == principal(80)
        ));
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn relay_funding_read_transfer_and_replay_boundaries_are_fail_closed() {
        let args = setup_args(vec![principal(1)]);

        reset();
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new([300_000_000], [LedgerOutcome::Accepted(10)]),
            true,
            None,
        );
        let balance_failure = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            balance_failure,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CodeInstalled,
                ..
            }
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);

        reset();
        let ledger = MockLedger::new([300_000_000, 297_990_000], [LedgerOutcome::Accepted(10)])
            .with_fee_failures([false, true]);
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);
        let fee_failure = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            fee_failure,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CodeInstalled,
                ..
            }
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);

        reset();
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new(
                [300_000_000, 297_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Duplicate(11)],
            ),
            true,
            None,
        );
        let duplicate = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(duplicate, RelaySetupNotifyResult::Active { .. }));

        reset();
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new(
                [300_000_000, 297_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Rejected],
            ),
            true,
            None,
        );
        let rejected = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            rejected,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::RelayFundingPrepared,
                ..
            }
        ));

        reset();
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new(
                [300_000_000, 297_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Ambiguous],
            ),
            true,
            None,
        );
        let ambiguous = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            ambiguous,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::RelayFundingPrepared,
                ..
            }
        ));
        let repeated = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            repeated,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 2);
    }

    #[test]
    fn rising_final_fee_requires_manual_recovery_without_relay_funding() {
        clear_setup_entries_for_debug();
        let mut cfg = config();
        cfg.relay_setup_min_e8s = 1;
        state::set_state(State::new(cfg, 0));
        let ledger = MockLedger::new([107_040_000, 105_030_000], [LedgerOutcome::Accepted(10)])
            .with_fees([10_000, 10_001]);
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);
        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![principal(1)]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::CodeInstalled,
                relay_canister_id: Some(relay_canister_id),
                ..
            } if relay_canister_id == principal(80)
        ));
        assert_eq!(ledger.transfers.lock().unwrap().len(), 1);
        let key = key_for_targets(&[principal(1)]);
        let Some(RelaySetupEntry::ManualRecoveryRequired(progress)) = get_entry(key) else {
            panic!("setup must require manual recovery")
        };
        assert_eq!(progress.relay_canister_id, Some(principal(80)));
        assert_eq!(progress.relay_funding_transfer, None);
    }

    #[test]
    fn exact_final_funding_minimum_is_accepted() {
        clear_setup_entries_for_debug();
        let mut cfg = config();
        cfg.relay_setup_min_e8s = 1;
        state::set_state(State::new(cfg, 0));
        let ledger = MockLedger::new(
            [107_040_000, 105_030_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        )
        .with_fees([10_000, 10_000]);
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);
        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![principal(1)]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert_eq!(
            result,
            RelaySetupNotifyResult::Active {
                relay_canister_id: principal(80),
            }
        );
        let transfers = ledger.transfers.lock().unwrap();
        assert_eq!(transfers.len(), 2);
        assert_eq!(transfers[1].amount, Nat::from(105_020_000u64));
    }

    #[test]
    fn maximum_configuration_installs_canonical_mixed_recipients_and_cap_26() {
        reset();
        let targets = (100..120).rev().map(principal).collect::<Vec<_>>();
        let recipients = vec![
            neuron_recipient(42, vec![0x42]),
            principal_recipient(principal(202), vec![0x20, 0xff]),
            principal_recipient(principal(200), vec![]),
            neuron_recipient(7, vec![0x07, 0x00]),
            principal_recipient(principal(201), vec![0x21]),
        ];
        let setup = CanonicalRelaySetup::canonicalize(targets.clone(), recipients.clone()).unwrap();
        let key = setup.key();
        let args = RelaySetupArgs {
            target_canister_ids: targets,
            surplus_recipients: recipients,
        };
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new(
                [775_000_000, 772_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
            ),
            true,
            None,
        );
        let neuron_resolver = MockNeuronResolver::readable();
        let result = block_on(notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &neuron_resolver,
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
        struct DecodedSurplusCanisterRecipient {
            canister_id: Principal,
            memo: Vec<u8>,
        }
        #[derive(CandidType, Deserialize)]
        struct DecodedSurplusNeuronRecipient {
            neuron_id: u64,
            memo: Vec<u8>,
        }
        #[derive(CandidType, Deserialize)]
        struct DecodedRelayInitArgs {
            managed_canisters: Vec<Principal>,
            blackhole_canister_id: Option<Principal>,
            max_transfers_per_tick: Option<u32>,
            surplus_canister_recipients: Option<Vec<DecodedSurplusCanisterRecipient>>,
            surplus_neuron_recipients: Vec<DecodedSurplusNeuronRecipient>,
        }
        let installs = management.installs.lock().unwrap();
        assert_eq!(installs.len(), 1);
        let init: DecodedRelayInitArgs = candid::decode_one(&installs[0].arg).unwrap();
        assert_eq!(
            init.managed_canisters,
            (100..120).map(principal).collect::<Vec<_>>()
        );
        assert_eq!(init.blackhole_canister_id, None);
        assert_eq!(init.max_transfers_per_tick, Some(26));
        let installed_recipients = init.surplus_canister_recipients.unwrap();
        assert_eq!(
            installed_recipients
                .iter()
                .map(|recipient| recipient.canister_id)
                .collect::<Vec<_>>(),
            (200..203).map(principal).collect::<Vec<_>>()
        );
        assert_eq!(
            installed_recipients
                .iter()
                .map(|recipient| recipient.memo.clone())
                .collect::<Vec<_>>(),
            vec![vec![], vec![0x21], vec![0x20, 0xff]]
        );
        assert_eq!(
            init.surplus_neuron_recipients
                .iter()
                .map(|recipient| recipient.neuron_id)
                .collect::<Vec<_>>(),
            vec![7, 42]
        );
        assert_eq!(
            init.surplus_neuron_recipients
                .iter()
                .map(|recipient| recipient.memo.clone())
                .collect::<Vec<_>>(),
            vec![vec![0x07, 0x00], vec![0x42]]
        );
        assert_eq!(*neuron_resolver.calls.lock().unwrap(), vec![7, 42]);
        drop(installs);

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
    fn relay_init_arg_splits_principal_only_and_neuron_only_recipients() {
        #[derive(CandidType, Deserialize)]
        struct CanisterRecipient {
            canister_id: Principal,
            memo: Vec<u8>,
        }
        #[derive(CandidType, Deserialize)]
        struct NeuronRecipient {
            neuron_id: u64,
            memo: Vec<u8>,
        }
        #[derive(CandidType, Deserialize)]
        struct InitArgs {
            max_transfers_per_tick: Option<u32>,
            surplus_canister_recipients: Option<Vec<CanisterRecipient>>,
            surplus_neuron_recipients: Vec<NeuronRecipient>,
        }

        let principal_setup = CanonicalRelaySetup::canonicalize(
            vec![principal(1)],
            vec![principal_recipient(principal(2), vec![0x00, 0xff])],
        )
        .unwrap();
        let principal_init: InitArgs =
            candid::decode_one(&relay_init_arg(&config(), &principal_setup)).unwrap();
        let principal_recipient = &principal_init.surplus_canister_recipients.unwrap()[0];
        assert_eq!(principal_recipient.canister_id, principal(2));
        assert_eq!(principal_recipient.memo, vec![0x00, 0xff]);
        assert!(principal_init.surplus_neuron_recipients.is_empty());

        let neuron_setup = CanonicalRelaySetup::canonicalize(
            vec![principal(1)],
            vec![neuron_recipient(u64::MAX, vec![0x80, 0x00])],
        )
        .unwrap();
        let neuron_init: InitArgs =
            candid::decode_one(&relay_init_arg(&config(), &neuron_setup)).unwrap();
        assert!(neuron_init.surplus_canister_recipients.is_none());
        assert_eq!(neuron_init.surplus_neuron_recipients[0].neuron_id, u64::MAX);
        assert_eq!(
            neuron_init.surplus_neuron_recipients[0].memo,
            vec![0x80, 0x00]
        );

        let zero_setup = CanonicalRelaySetup::canonicalize(vec![principal(1)], vec![]).unwrap();
        let zero_init: InitArgs =
            candid::decode_one(&relay_init_arg(&config(), &zero_setup)).unwrap();
        assert!(zero_init.surplus_canister_recipients.is_none());
        assert!(zero_init.surplus_neuron_recipients.is_empty());
        assert_eq!(zero_init.max_transfers_per_tick, Some(2));
    }

    #[test]
    fn funded_zero_recipient_setup_skips_governance_and_installs_all_cycles_config() {
        clear_setup_entries_for_debug();
        let mut cfg = config();
        cfg.relay_setup_min_e8s = 1;
        state::set_state(State::new(cfg, 0));
        let args = RelaySetupArgs {
            target_canister_ids: vec![principal(1)],
            surplus_recipients: vec![],
        };
        let (ledger, probe, cmc, management) = mocks(
            MockLedger::new(
                [107_040_000, 105_030_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
            )
            .with_fees([10_000, 10_000]),
            true,
            None,
        );
        let resolver = MockNeuronResolver::readable();

        let result = block_on(notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));

        assert_eq!(
            result,
            RelaySetupNotifyResult::Active {
                relay_canister_id: principal(80),
            }
        );
        assert!(resolver.calls.lock().unwrap().is_empty());
        #[derive(CandidType, Deserialize)]
        struct InitArgs {
            max_transfers_per_tick: Option<u32>,
            surplus_canister_recipients: Option<Vec<PrincipalRecipient>>,
            surplus_neuron_recipients: Vec<NeuronRecipient>,
        }
        #[derive(CandidType, Deserialize)]
        struct PrincipalRecipient {
            canister_id: Principal,
            memo: Vec<u8>,
        }
        #[derive(CandidType, Deserialize)]
        struct NeuronRecipient {
            neuron_id: u64,
            memo: Vec<u8>,
        }
        let installs = management.installs.lock().unwrap();
        let init: InitArgs = candid::decode_one(&installs[0].arg).unwrap();
        assert!(init.surplus_canister_recipients.is_none());
        assert!(init.surplus_neuron_recipients.is_empty());
        assert_eq!(init.max_transfers_per_tick, Some(2));
    }

    #[test]
    fn funded_unreadable_neuron_cleans_reserved_state_before_probe_or_spend() {
        reset();
        let args = RelaySetupArgs {
            target_canister_ids: vec![principal(1)],
            surplus_recipients: vec![
                neuron_recipient(7, vec![0x07]),
                neuron_recipient(42, vec![0x2a]),
            ],
        };
        let key = CanonicalRelaySetup::canonicalize(
            args.target_canister_ids.clone(),
            args.surplus_recipients.clone(),
        )
        .unwrap()
        .key();
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([400_000_000], []), true, None);
        let resolver = MockNeuronResolver {
            calls: Mutex::new(Vec::new()),
            unreadable: Some(42),
            expected_reserved_key: Some(key),
            reserved_observations: Mutex::new(Vec::new()),
        };
        let result = block_on(notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));
        assert!(matches!(
            result,
            RelaySetupNotifyResult::FailedPreSpend { message }
                if message.contains("neuron 42") && message.contains("publicly readable")
        ));
        assert_eq!(*resolver.calls.lock().unwrap(), vec![7, 42]);
        assert_eq!(
            *resolver.reserved_observations.lock().unwrap(),
            vec![true, true]
        );
        assert_eq!(get_entry(key), None);
        assert_eq!(ledger.balance_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ledger.fee_calls.load(Ordering::SeqCst), 1);
        assert_eq!(cmc.rate_calls.load(Ordering::SeqCst), 1);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.status_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.update_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn superseded_neuron_validation_cannot_clean_or_advance_authoritative_state() {
        reset();
        let args = neuron_setup_args(principal(1), 42);
        let key = CanonicalRelaySetup::canonicalize(
            args.target_canister_ids.clone(),
            args.surplus_recipients.clone(),
        )
        .unwrap()
        .key();
        let relay_canister_id = principal(81);
        let (ledger, probe, cmc, management) =
            mocks(MockLedger::new([400_000_000], []), true, None);
        let resolver = SupersedingNeuronResolver {
            key,
            relay_canister_id,
        };

        let result = block_on(notify_with_clients_and_neuron_resolver(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
            &resolver,
        ));

        assert_eq!(result, RelaySetupNotifyResult::Active { relay_canister_id });
        assert_eq!(
            get_entry(key),
            Some(RelaySetupEntry::Active { relay_canister_id })
        );
        assert_eq!(probe.blackhole_calls.load(Ordering::SeqCst), 0);
        assert!(ledger.transfers.lock().unwrap().is_empty());
        assert_eq!(cmc.notify_calls.load(Ordering::SeqCst), 0);
        assert_eq!(management.create_calls.load(Ordering::SeqCst), 0);
        assert!(management.installs.lock().unwrap().is_empty());
    }

    #[test]
    fn finalization_audits_cover_pre_and_post_status_hash_controller_and_log_requirements() {
        let expected_hash = approved_relay_onchain_module_hash().unwrap();
        let historian = principal(42);
        let mut status = AuditedCanisterStatus {
            status: AuditedCanisterStatusKind::Running,
            cycles: Nat::from(1_000_000_u64),
            module_hash: Some(expected_hash.to_vec()),
            settings: AuditedCanisterSettings {
                controllers: vec![historian],
                log_visibility: LogVisibility::Public,
                status_visibility: StatusVisibility::Public,
            },
        };
        assert!(validate_pre_finalization(&status, &expected_hash, historian).is_ok());
        status.status = AuditedCanisterStatusKind::Stopped;
        assert!(validate_pre_finalization(&status, &expected_hash, historian).is_err());
        status.status = AuditedCanisterStatusKind::Running;
        status.module_hash = Some(vec![0; 32]);
        assert!(validate_pre_finalization(&status, &expected_hash, historian).is_err());
        status.module_hash = Some(expected_hash.to_vec());
        status.settings.controllers = vec![historian, principal(90)];
        assert!(validate_pre_finalization(&status, &expected_hash, historian).is_err());
        status.settings.controllers = vec![historian];
        status.settings.log_visibility = LogVisibility::Controllers;
        assert!(validate_pre_finalization(&status, &expected_hash, historian).is_err());
        status.settings.log_visibility = LogVisibility::Public;
        status.settings.status_visibility = StatusVisibility::Controllers;
        assert!(validate_pre_finalization(&status, &expected_hash, historian).is_err());

        let mut post_status = AuditedCanisterStatus {
            status: CanisterStatusKind::Running,
            module_hash: Some(expected_hash.to_vec()),
            cycles: Nat::from(1_000_000u64),
            settings: AuditedCanisterSettings {
                controllers: Vec::new(),
                log_visibility: LogVisibility::Public,
                status_visibility: StatusVisibility::Public,
            },
        };
        assert!(validate_finalized_relay(&post_status, &expected_hash).is_ok());
        post_status.status = CanisterStatusKind::Stopped;
        assert!(validate_finalized_relay(&post_status, &expected_hash).is_err());
        post_status.status = CanisterStatusKind::Running;
        post_status.module_hash = Some(vec![0; 32]);
        assert!(validate_finalized_relay(&post_status, &expected_hash).is_err());
        post_status.module_hash = Some(expected_hash.to_vec());
        post_status.settings.controllers.push(principal(90));
        assert!(validate_finalized_relay(&post_status, &expected_hash).is_err());
        post_status.settings.controllers.clear();
        post_status.settings.log_visibility = LogVisibility::Controllers;
        assert!(validate_finalized_relay(&post_status, &expected_hash).is_err());
        post_status.settings.log_visibility = LogVisibility::Public;
        post_status.settings.status_visibility = StatusVisibility::AllowedViewers(Vec::new());
        assert!(validate_finalized_relay(&post_status, &expected_hash).is_err());
    }

    #[test]
    fn create_and_final_settings_are_explicit_public_and_controllerless() {
        reset();
        let ledger = MockLedger::new(
            [300_000_000, 297_990_000],
            [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
        );
        let (ledger, probe, cmc, management) = mocks(ledger, true, None);

        let result = block_on(notify_with_clients_for_historian(
            setup_args(vec![principal(1)]),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(result, RelaySetupNotifyResult::Active { .. }));

        let creates = management.creates.lock().unwrap();
        assert_eq!(creates.len(), 1);
        assert_eq!(
            creates[0].settings,
            Some(jupiter_ic_clients::management::CanisterSettings {
                controllers: Some(vec![principal(42)]),
                log_visibility: Some(LogVisibility::Public),
                status_visibility: Some(StatusVisibility::Public),
            })
        );
        drop(creates);

        let updates = management.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].settings,
            jupiter_ic_clients::management::CanisterSettings {
                controllers: Some(Vec::new()),
                log_visibility: Some(LogVisibility::Public),
                status_visibility: Some(StatusVisibility::Public),
            }
        );
        assert!(management
            .finalization_phase_observed_at_update
            .load(Ordering::SeqCst));
    }

    #[test]
    fn finalization_errors_are_reconciled_and_activation_requires_complete_progress() {
        let args = setup_args(vec![principal(1)]);
        let successful_ledger = || {
            MockLedger::new(
                [300_000_000, 297_990_000],
                [LedgerOutcome::Accepted(10), LedgerOutcome::Accepted(11)],
            )
        };

        reset();
        let (ledger, probe, cmc, mut management) = mocks(successful_ledger(), true, None);
        management.update_error = Some("update_settings callback failed".to_string());
        let observed_success = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            observed_success,
            RelaySetupNotifyResult::Active { .. }
        ));
        assert_eq!(management.update_calls.load(Ordering::SeqCst), 1);

        reset();
        let (ledger, probe, cmc, mut management) = mocks(successful_ledger(), true, None);
        management.update_error = Some("update_settings callback failed".to_string());
        management.status_results = Mutex::new(VecDeque::from([
            Ok(audited_relay_status(vec![principal(42)])),
            Ok(audited_relay_status(vec![principal(90)])),
        ]));
        let observed_incorrect = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            observed_incorrect,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::FinalizationAttempted,
                ..
            }
        ));

        reset();
        let (ledger, probe, cmc, mut management) = mocks(successful_ledger(), true, None);
        management.status_results = Mutex::new(VecDeque::from([
            Ok(audited_relay_status(vec![principal(42)])),
            Err("direct status unavailable".to_string()),
        ]));
        let status_error = block_on(notify_with_clients_for_historian(
            args.clone(),
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            status_error,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::FinalizationAttempted,
                ..
            }
        ));

        reset();
        let (ledger, probe, cmc, mut management) = mocks(successful_ledger(), true, None);
        management.clear_cycles_minted_on_final_status = true;
        let incomplete = block_on(notify_with_clients_for_historian(
            args,
            principal(42),
            &ledger,
            &probe,
            &cmc,
            &management,
        ));
        assert!(matches!(
            incomplete,
            RelaySetupNotifyResult::ManualRecoveryRequired {
                phase: RelayCreationPhase::FinalizationAttempted,
                ..
            }
        ));
        assert!(matches!(
            get_entry(key_for_targets(&[principal(1)])),
            Some(RelaySetupEntry::ManualRecoveryRequired(_))
        ));
    }
}
