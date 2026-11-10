use candid::{CandidType, Deserialize, Principal};
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ProgrammeConfig {
    pub version: u64,
    pub jup_root: Principal,
    pub io_root: Principal,
    pub icp_ledger: Principal,
    pub icp_index: Principal,
    pub staking_account: Account,
    pub canonical_relay: Principal,
    pub protocol_governance_neuron: Vec<u8>,
    pub excluded_icp_accounts: Vec<Vec<u8>>,
    pub excluded_jup_neurons: Vec<Vec<u8>>,
    pub reserve_subaccount: Option<[u8; 32]>,
    pub reserve_transport_neuron: Option<Vec<u8>>,
    pub launch_timestamp_seconds: Option<u64>,
    pub patron_enabled: bool,
    pub snapshot_max_age_seconds: u64,
    pub automation: Option<AutomationConfig>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MirrorTarget {
    pub root: Principal,
    pub governance: Principal,
    pub neuron: Vec<u8>,
    pub first_proposal: u64,
    pub delivery_margin_seconds: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AutomationConfig {
    pub proposer_neuron: Vec<u8>,
    pub targets: Vec<MirrorTarget>,
    pub treasury_enabled: bool,
    pub treasury_retain_icp_e8s: u64,
    pub treasury_max_proposal_icp_e8s: u64,
}

impl ProgrammeConfig {
    /// General provenance is incomplete. Only the explicit debug/local fixture
    /// build may exercise direct patron or native treasury payments.
    pub fn validate_activation(&self, local_test_context: bool) -> Result<(), String> {
        if !local_test_context
            && (self.patron_enabled || self.automation.as_ref().is_some_and(|a| a.treasury_enabled))
        {
            return Err("patron and treasury activation blocked: Relay FIFO/splitter and mixed treasury provenance are incomplete".into());
        }
        Ok(())
    }

    pub(super) fn preserves_financial_bindings(&self, old: &Self) -> bool {
        self.jup_root == old.jup_root
            && self.icp_ledger == old.icp_ledger
            && self.icp_index == old.icp_index
            && self.staking_account == old.staking_account
            && self.reserve_subaccount == old.reserve_subaccount
            && self.reserve_transport_neuron == old.reserve_transport_neuron
            && self.protocol_governance_neuron == old.protocol_governance_neuron
    }
    pub fn validate(&self) -> Result<(), String> {
        for id in [
            self.jup_root,
            self.io_root,
            self.icp_ledger,
            self.icp_index,
            self.staking_account.owner,
            self.canonical_relay,
        ] {
            if id == Principal::anonymous() || id == Principal::management_canister() {
                return Err("unsafe programme principal".into());
            }
        }
        if let Some(auto) = &self.automation {
            if auto.proposer_neuron.len() != 32
                || auto.proposer_neuron == self.protocol_governance_neuron
                || auto.targets.len() > 4
                || self.reserve_transport_neuron.as_ref() == Some(&auto.proposer_neuron)
                || (auto.treasury_enabled
                    && (self.patron_enabled || auto.treasury_max_proposal_icp_e8s == 0))
                || !self.excluded_jup_neurons.contains(&auto.proposer_neuron)
                || auto.targets.iter().any(|t| {
                    t.neuron.len() != 32
                        || t.root == self.jup_root
                        || t.governance == self.jup_root
                        || t.delivery_margin_seconds < 3600
                })
                || auto
                    .targets
                    .iter()
                    .map(|t| t.governance)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != auto.targets.len()
            {
                return Err("invalid bounded automation targets/proposer exclusion".into());
            }
        }
        if self.version == 0
            || self.jup_root == self.io_root
            || !(86_400..=604_800).contains(&self.snapshot_max_age_seconds)
            || self.excluded_jup_neurons.len() > 16
            || self.protocol_governance_neuron.len() != 32
            || !self
                .excluded_jup_neurons
                .contains(&self.protocol_governance_neuron)
            || self.excluded_icp_accounts.len() > 32
            || self.excluded_jup_neurons.iter().any(|id| id.len() != 32)
            || self.excluded_icp_accounts.iter().any(|id| id.len() != 32)
            || (self.patron_enabled && self.launch_timestamp_seconds.is_none())
        {
            return Err("invalid programme limits or unresolved launch timestamp".into());
        }
        if self.reserve_transport_neuron.as_ref().is_some_and(|id| {
            id.len() != 32
                || !self.excluded_jup_neurons.contains(id)
                || *id == self.protocol_governance_neuron
        }) {
            return Err("reserve transport must be a distinct explicitly excluded neuron".into());
        }
        Ok(())
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub root: Principal,
    pub governance: Principal,
    pub ledger: Principal,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: u64,
    pub config_version: u64,
    pub sources: Vec<Source>,
    pub started_at_seconds: u64,
    pub completed_at_seconds: u64,
    pub slot: u8,
    pub owners: u64,
}

impl Snapshot {
    pub fn valid_for(&self, config: &ProgrammeConfig, now: u64) -> bool {
        self.config_version == config.version
            && self.sources.len() == 2
            && self.sources[0].root == config.jup_root
            && self.sources[1].root == config.io_root
            && self.completed_at_seconds <= now
            && now - self.completed_at_seconds <= config.snapshot_max_age_seconds
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub principal: Principal,
    pub jup: bool,
    pub io: bool,
    pub compound_neuron: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Scan {
    pub sources: Vec<Source>,
    pub source_index: u8,
    pub cursor: Option<Vec<u8>>,
    pub pages: u32,
    pub started_at_seconds: u64,
    pub slot: u8,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub snapshot: Snapshot,
    pub end_block: u64,
    pub previous_end: Option<u64>,
    pub cursor: Option<u64>,
    pub oldest: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum PatronOutcome {
    Prepared,
    Dispatched { at_nanos: u64 },
    Verified { block: candid::Nat, duplicate: bool },
    AssumedPaidUnverified { reason: String },
    Failed { error: String },
    Skipped { reason: String },
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PatronRecord {
    pub contribution_block: u64,
    pub snapshot_id: u64,
    pub ownership_snapshot: Snapshot,
    pub recognition: Option<Owner>,
    pub config_version: u64,
    pub contributor_icp_account: Vec<u8>,
    pub commitment_timestamp_seconds: u64,
    pub qualifying_icp_e8s: u64,
    pub launch_timestamp_seconds: u64,
    pub compounded_neuron: Option<Vec<u8>>,
    pub ledger: Principal,
    pub from: Account,
    pub to: Account,
    pub amount_e8s: u64,
    pub fee_e8s: u64,
    pub memo: Vec<u8>,
    pub created_at_time_nanos: u64,
    pub outcome: PatronOutcome,
    pub refresh_result: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Runtime {
    pub config: Option<ProgrammeConfig>,
    pub snapshot: Option<Snapshot>,
    pub scan: Option<Scan>,
    pub next_snapshot: u64,
    pub batch: Option<Batch>,
    pub completed_end_block: Option<u64>,
    pub pending_block: Option<u64>,
    pub last_error: Option<String>,
    pub verified_e8s: u128,
    pub assumed_e8s: u128,
    pub failed_items: u64,
}
