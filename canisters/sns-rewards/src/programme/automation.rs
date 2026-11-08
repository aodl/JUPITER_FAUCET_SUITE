//! Permission-bounded proposer maintenance and Motion-based canonical decisions.
//! No caller-supplied proposal, title, target or ballot authorizes a vote.
use super::{discovery, governance_client as client, storage, types::*};
use crate::generated as sns;
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, VirtualMemory},
    DefaultMemoryImpl, StableBTreeMap, StableCell,
};
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError},
};
use jupiter_ic_clients::ledger::IcrcLedgerCanister;
use serde::Serialize;
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum MirrorPhase {
    Discovered,
    Creating {
        at_seconds: u64,
        search_before: Option<u64>,
    },
    Mapped {
        jupiter_proposal: u64,
    },
    Voting {
        jupiter_proposal: u64,
        vote: i32,
    },
    Verified {
        jupiter_proposal: u64,
        vote: i32,
    },
    Infeasible(String),
    ManualReview(String),
}
impl MirrorPhase {
    fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Verified { .. } | Self::Infeasible(_) | Self::ManualReview(_)
        )
    }
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MirrorRecord {
    pub source: Source,
    pub source_proposer: Vec<u8>,
    pub config_version: u64,
    pub target: MirrorTarget,
    pub proposal_id: u64,
    pub content_digest: Vec<u8>,
    pub binding: String,
    pub phase: MirrorPhase,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub before: Option<u64>,
    pub completed_tip: Option<u64>,
    pub cycle_tip: Option<u64>,
    pub next_scan_seconds: u64,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum FundingOutcome {
    Dispatched,
    Verified { block: Nat },
    ManualReview(String),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Funding {
    pub config_version: u64,
    pub source: Account,
    pub destination: Account,
    pub amount: u64,
    pub fee: u64,
    pub created_at_time: u64,
    pub memo: Vec<u8>,
    pub outcome: FundingOutcome,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ChargeScan {
    pub before: Option<u64>,
    pub open_e8s: u64,
    pub pages: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationState {
    pub config_version: u64,
    pub source: Option<Source>,
    pub progress: Vec<Progress>,
    pub pending: Vec<Vec<u8>>,
    pub next_target: u8,
    pub funding: Option<Funding>,
    pub charge_scan: Option<ChargeScan>,
    pub last_error: Option<String>,
}
type Memory = VirtualMemory<DefaultMemoryImpl>;
type MirrorMap = StableBTreeMap<Vec<u8>, Vec<u8>, Memory>;
thread_local! {
    static META: RefCell<Option<StableCell<Vec<u8>, Memory>>> = const {RefCell::new(None)};
    static MIRRORS: RefCell<Option<MirrorMap>> = const {RefCell::new(None)};
    static FUNDING: RefCell<Option<StableBTreeMap<u64, Vec<u8>, Memory>>> = const {RefCell::new(None)};
}
fn memory(id: u8) -> Memory {
    crate::state::programme_memory(MemoryId::new(id))
}
pub fn read() -> AutomationState {
    META.with_borrow_mut(|cell| {
        let cell = cell.get_or_insert_with(|| StableCell::init(memory(7), vec![]));
        if cell.get().is_empty() {
            AutomationState::default()
        } else {
            candid::decode_one(cell.get()).expect("automation metadata")
        }
    })
}
fn set(state: &AutomationState) {
    let mut bounded = state.clone();
    bounded.last_error = bounded.last_error.as_deref().map(super::compact);
    if let Some(Funding {
        outcome: FundingOutcome::ManualReview(reason),
        ..
    }) = &mut bounded.funding
    {
        *reason = super::compact(reason);
    }
    let state = &bounded;
    let _ = read();
    META.with_borrow_mut(|cell| {
        cell.as_mut()
            .expect("automation metadata")
            .set(candid::encode_one(state).expect("encode automation metadata"));
    });
    if let Some(funding) = &state.funding {
        FUNDING.with_borrow_mut(|map| {
            map.get_or_insert_with(|| StableBTreeMap::init(memory(15)))
                .insert(
                    funding.created_at_time,
                    candid::encode_one(funding).expect("encode proposer funding"),
                );
        });
    }
}
pub fn funding_record(created_at_time: u64) -> Option<Funding> {
    FUNDING.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(15)))
            .get(&created_at_time)
            .map(|bytes| candid::decode_one(&bytes).expect("proposer funding record"))
    })
}

/// Native fees remain cached until disbursement. Open charges can be refunded;
/// only irreversible charges reduce the operating balance used for watermarks.
fn operating_balance(balance: u64, fees: u64, open: u64) -> u64 {
    balance.saturating_sub(fees.saturating_sub(open))
}

fn can_cover_submission(balance: u64, fees: u64, rejection_cost: u64) -> bool {
    balance.saturating_sub(fees) >= rejection_cost
}

fn timing_margin(duration: u64, quiet: u64, delivery: u64) -> Result<u64, String> {
    duration
        .checked_add(quiet.checked_mul(2).ok_or("timing overflow")?)
        .and_then(|v| v.checked_add(delivery))
        .ok_or_else(|| "timing overflow".into())
}
fn key(governance: Principal, proposal: u64) -> Vec<u8> {
    [governance.as_slice(), &proposal.to_be_bytes()].concat()
}
pub fn record(governance: Principal, proposal: u64) -> Option<MirrorRecord> {
    get(&key(governance, proposal))
}
fn get(key: &Vec<u8>) -> Option<MirrorRecord> {
    MIRRORS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(8)))
            .get(key)
            .map(|v| candid::decode_one(&v).expect("mirror record"))
    })
}
fn put(record: &MirrorRecord) {
    let mut bounded = record.clone();
    if let MirrorPhase::ManualReview(reason) | MirrorPhase::Infeasible(reason) = &mut bounded.phase
    {
        *reason = super::compact(reason);
    }
    let record = &bounded;
    MIRRORS.with_borrow_mut(|map| {
        map.get_or_insert_with(|| StableBTreeMap::init(memory(8)))
            .insert(
                key(record.target.governance, record.proposal_id),
                candid::encode_one(record).expect("encode mirror"),
            );
    });
}
pub fn can_reconfigure() -> bool {
    let s = read();
    s.pending.is_empty() && s.funding.is_none()
}

pub async fn tick(now: u64) -> Result<(), String> {
    let result = tick_inner(now).await;
    let mut state = read();
    state.last_error = result.as_ref().err().cloned();
    set(&state);
    result
}

async fn tick_inner(now: u64) -> Result<(), String> {
    let runtime = storage::read();
    let Some(config) = runtime.config else {
        return Ok(());
    };
    let Some(auto) = &config.automation else {
        return Ok(());
    };
    if config.reserve_transport_neuron.is_some()
        && !matches!(
            super::reserve::read(),
            super::reserve::ReserveTransport::Verified { .. }
        )
    {
        return Err("proposer maintenance waits for verified genesis reserve availability".into());
    }
    let mut state = read();
    if state.config_version != config.version {
        if !can_reconfigure() {
            return Err("automation has in-flight work under the previous configuration".into());
        }
        let source = discovery::resolve(config.jup_root).await?;
        let decimals: u8 = ic_cdk::call::Call::bounded_wait(source.ledger, "icrc1_decimals")
            .await
            .map_err(|e| format!("proposer token decimals: {e:?}"))?
            .candid()
            .map_err(|e| e.to_string())?;
        if decimals != crate::economics::get().decimals {
            return Err("proposer Ledger must use eight decimals".into());
        }
        state = AutomationState {
            config_version: config.version,
            source: Some(source),
            progress: vec![Progress::default(); auto.targets.len()],
            ..Default::default()
        };
        set(&state);
    }
    let source = state.source.clone().ok_or("automation source unresolved")?;
    client::require_reviewed_governance(source.governance).await?;
    if auto
        .targets
        .iter()
        .any(|t| t.governance == source.governance)
    {
        return Err("mirror loops to Jupiter Governance are forbidden".into());
    }
    let mut work_error = maintain_proposer(&config, auto, &source, &mut state)
        .await
        .err();
    let proposer_ready = work_error.is_none();
    if let Some(first) = state.pending.first().cloned() {
        let mut record = get(&first).ok_or("missing durable mirror mapping")?;
        let result = if let Err(error) =
            client::require_reviewed_governance(record.target.governance).await
        {
            Err(error)
        } else if matches!(record.phase, MirrorPhase::Discovered) && !proposer_ready {
            Ok(()) // funding readiness gates new submissions, not existing decisions/ballots
        } else {
            process(&source, auto, &mut record, now).await
        };
        put(&record);
        state.pending.remove(0);
        if !record.phase.terminal() {
            state.pending.push(first);
        }
        if let Err(error) = result {
            work_error = Some(error);
        }
        state.last_error = work_error.clone();
        set(&state);
    }
    if !auto.targets.is_empty() && state.pending.len() < 32 {
        let index = state.next_target as usize % auto.targets.len();
        state.next_target = ((index + 1) % auto.targets.len()) as u8;
        if let Err(error) = discover(auto, &source, &mut state, index, now).await {
            work_error = Some(error);
        }
        set(&state);
    }
    if let Err(error) = super::treasury::tick(&config, auto, &state, now, proposer_ready).await {
        work_error = Some(error);
    }
    work_error.map_or(Ok(()), Err)
}

async fn maintain_proposer(
    config: &ProgrammeConfig,
    auto: &AutomationConfig,
    source: &Source,
    state: &mut AutomationState,
) -> Result<(), String> {
    if let Some(funding) = &mut state.funding {
        match &funding.outcome {
            FundingOutcome::Dispatched => {
                funding.outcome = FundingOutcome::ManualReview(
                    "proposer funding callback unresolved; no assumed financial success or replay"
                        .into(),
                );
                set(state);
                return Err("proposer funding requires canonical reconciliation".into());
            }
            FundingOutcome::ManualReview(reason) => return Err(reason.clone()),
            FundingOutcome::Verified { .. } => {
                client::manage(
                    source.governance,
                    auto.proposer_neuron.clone(),
                    sns::Command::ClaimOrRefresh(sns::ClaimOrRefresh {
                        by: Some(sns::By::NeuronId {}),
                    }),
                )
                .await?;
                state.funding = None;
                set(state);
                return Ok(());
            }
        }
    }
    let neuron = discovery::get_neuron(source.governance, auto.proposer_neuron.clone()).await?;
    if neuron.permissions.iter().any(|p| {
        p.principal
            .is_some_and(|principal| principal != ic_cdk::api::canister_self())
            && !p.permission_type.is_empty()
    }) {
        return Err(
            "the sponsored operational proposer must have no unrelated permission principal".into(),
        );
    }
    let permissions: std::collections::BTreeSet<i32> = neuron
        .permissions
        .iter()
        .filter(|p| p.principal == Some(ic_cdk::api::canister_self()))
        .flat_map(|p| p.permission_type.iter().copied())
        .collect();
    if !permissions.contains(&3) {
        return Err("operational proposer must grant this canister SubmitProposal".into());
    }
    let extra: Vec<i32> = permissions
        .iter()
        .copied()
        .filter(|code| *code != 3)
        .collect();
    if !extra.is_empty() {
        if !permissions.contains(&2) {
            return Err(
                "operational proposer has unreviewed excess permissions that it cannot revoke"
                    .into(),
            );
        }
        client::manage(
            source.governance,
            auto.proposer_neuron.clone(),
            sns::Command::RemoveNeuronPermissions(sns::RemoveNeuronPermissions {
                principal_id: Some(ic_cdk::api::canister_self()),
                permissions_to_remove: Some(sns::NeuronPermissionList { permissions: extra }),
            }),
        )
        .await?;
        return Err("operational proposer permissions narrowed to SubmitProposal; awaiting coherent observation".into());
    }
    let parameters: sns::NervousSystemParameters =
        ic_cdk::call::Call::bounded_wait(source.governance, "get_nervous_system_parameters")
            .await
            .map_err(|e| format!("source voting parameters: {e:?}"))?
            .candid()
            .map_err(|e| e.to_string())?;
    let economics = crate::economics::get();
    if parameters.initial_voting_period_seconds != Some(economics.jupiter_voting_seconds)
        || parameters.reject_cost_e8s != Some(economics.proposal_rejection_e8s)
    {
        return Err(
            "source proposer fee/voting policy differs from reviewed programme economics".into(),
        );
    }
    let destination = Account {
        owner: source.governance,
        subaccount: Some(
            auto.proposer_neuron
                .clone()
                .try_into()
                .map_err(|_| "proposer neuron ID")?,
        ),
    };
    let ledger = IcrcLedgerCanister::new(source.ledger);
    let balance = ledger
        .balance_of_e8s(destination)
        .await
        .map_err(|e| e.to_string())?;
    if balance != neuron.cached_neuron_stake_e8s {
        client::manage(
            source.governance,
            auto.proposer_neuron.clone(),
            sns::Command::ClaimOrRefresh(sns::ClaimOrRefresh {
                by: Some(sns::By::NeuronId {}),
            }),
        )
        .await?;
        return Err(
            "proposer stake refreshed; wait for a coherent observation before funding".into(),
        );
    }
    // Governance records rejected charges in neuron_fees without immediately
    // burning Ledger tokens. Obtain all canonical open charges before funding.
    if balance.saturating_sub(neuron.neuron_fees_e8s) >= economics.proposer_low_e8s {
        state.charge_scan = None;
        set(state);
        return Ok(());
    }
    let scan = state.charge_scan.get_or_insert_with(ChargeScan::default);
    if scan.pages >= 10_000 {
        return Err("proposer open-charge scan incomplete at bounded cap".into());
    }
    let page = client::list(source.governance, scan.before, vec![1]).await?;
    if !page.is_empty() {
        for proposal in &page {
            if proposal
                .proposer
                .as_ref()
                .is_some_and(|id| id.id == auto.proposer_neuron)
            {
                scan.open_e8s = scan
                    .open_e8s
                    .checked_add(proposal.reject_cost_e8s)
                    .ok_or("open proposal charges overflow")?;
            }
        }
        scan.before = page.last().and_then(|p| p.id.as_ref().map(|id| id.id));
        scan.pages += 1;
        set(state);
        return Err("proposer funding waits for the complete canonical open-charge scan".into());
    }
    let operating = operating_balance(balance, neuron.neuron_fees_e8s, scan.open_e8s);
    state.charge_scan = None;
    set(state);
    if operating >= economics.proposer_low_e8s {
        if !can_cover_submission(
            balance,
            neuron.neuron_fees_e8s,
            economics.proposal_rejection_e8s,
        ) {
            return Err("proposer stake is reserved by open charges; defer new submissions without another top-up".into());
        }
        return Ok(());
    }
    let amount = economics
        .proposer_high_e8s
        .checked_sub(operating)
        .ok_or("proposer target underflow")?;
    let fee = ledger.fee_e8s().await.map_err(|e| e.to_string())?;
    let reserve = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: config.reserve_subaccount,
    };
    if ledger
        .balance_of_e8s(reserve)
        .await
        .map_err(|e| e.to_string())?
        < amount.checked_add(fee).ok_or("proposer funding overflow")?
    {
        return Err("proposer reserve funding unavailable".into());
    }
    let created_at_time = ic_cdk::api::time();
    let memo = [b"jUP-proposer".as_slice(), &created_at_time.to_be_bytes()].concat();
    state.funding = Some(Funding {
        config_version: config.version,
        source: reserve,
        destination,
        amount,
        fee,
        created_at_time,
        memo: memo.clone(),
        outcome: FundingOutcome::Dispatched,
    });
    set(state);
    let result = ledger
        .transfer(TransferArg {
            from_subaccount: reserve.subaccount,
            to: destination,
            amount: amount.into(),
            fee: Some(fee.into()),
            memo: Some(memo.into()),
            created_at_time: Some(created_at_time),
        })
        .await;
    state
        .funding
        .as_mut()
        .expect("durable proposer funding")
        .outcome = match result {
        Ok(Ok(block))
        | Ok(Err(TransferError::Duplicate {
            duplicate_of: block,
        })) => FundingOutcome::Verified { block },
        other => FundingOutcome::ManualReview(format!("actual proposer funding result: {other:?}")),
    };
    set(state);
    Err("proposer funding checkpoint recorded; refresh before submitting proposals".into())
}

async fn discover(
    auto: &AutomationConfig,
    source: &Source,
    state: &mut AutomationState,
    index: usize,
    now: u64,
) -> Result<(), String> {
    let target = &auto.targets[index];
    if state.progress[index].next_scan_seconds > now {
        return Ok(());
    }
    let resolved = discovery::resolve(target.root).await?;
    if resolved.governance != target.governance || resolved.governance == source.governance {
        return Err("target Governance does not belong to its configured Root".into());
    }
    let before = state.progress[index].before;
    let proposals = client::list(target.governance, before, vec![]).await?;
    if before.is_none() {
        state.progress[index].cycle_tip = proposals
            .first()
            .and_then(|p| p.id.as_ref().map(|id| id.id));
    }
    let mut complete = proposals.is_empty();
    for p in proposals.into_iter().take(1) {
        if state.pending.len() >= 32 {
            break;
        }
        let id = p.id.as_ref().ok_or("proposal ID missing")?.id;
        if id < target.first_proposal
            || state.progress[index]
                .completed_tip
                .is_some_and(|last| id <= last)
        {
            complete = true;
            break;
        }
        let canonical = client::proposal(target.governance, id).await?;
        let proposal = canonical
            .proposal
            .as_ref()
            .ok_or("target proposal content missing")?;
        let key = key(target.governance, id);
        if get(&key).is_none() {
            let content_digest = client::digest(proposal).to_vec();
            let binding = format!(
                "JUP-MIRROR-v1:{}:{}:{}:{}",
                target.governance,
                id,
                hex::encode(&target.neuron),
                hex::encode(&content_digest)
            );
            let phase = if proposal.title.starts_with("JUP-MIRROR-v1:") {
                MirrorPhase::Infeasible("mirror-loop marker".into())
            } else if client::deadline(&canonical).is_none_or(|deadline| deadline <= now) {
                MirrorPhase::Infeasible("target deadline exhausted before discovery".into())
            } else {
                MirrorPhase::Discovered
            };
            let record = MirrorRecord {
                source: source.clone(),
                source_proposer: auto.proposer_neuron.clone(),
                config_version: state.config_version,
                target: target.clone(),
                proposal_id: id,
                content_digest,
                binding,
                phase,
            };
            put(&record);
            if !record.phase.terminal() {
                state.pending.push(key);
            }
        }
        state.progress[index].before = Some(id);
    }
    if complete {
        let progress = &mut state.progress[index];
        progress.completed_tip = progress.cycle_tip.or(progress.completed_tip);
        progress.before = None;
        progress.cycle_tip = None;
        progress.next_scan_seconds = now.saturating_add(600);
    }
    Ok(())
}

fn mirror_proposal(record: &MirrorRecord) -> sns::Proposal {
    sns::Proposal { title: record.binding.clone(), summary: "Should Jupiter cast Yes on the bound external SNS proposal? Adoption means Yes; rejection means No. The operational proposer casts its native automatic Yes ballot.".into(),
        url: String::new(), action: Some(sns::Action::Motion(sns::Motion { motion_text: record.binding.clone() })) }
}

async fn process(
    source: &Source,
    auto: &AutomationConfig,
    record: &mut MirrorRecord,
    now: u64,
) -> Result<(), String> {
    if record.source != *source || record.source_proposer != auto.proposer_neuron {
        record.phase = MirrorPhase::ManualReview("stored source/proposer binding mismatch".into());
        return Ok(());
    }
    let expected = mirror_proposal(record);
    match record.phase.clone() {
        MirrorPhase::Discovered => {
            let target = client::proposal(record.target.governance, record.proposal_id).await?;
            let parameters: sns::NervousSystemParameters = ic_cdk::call::Call::bounded_wait(
                source.governance,
                "get_nervous_system_parameters",
            )
            .await
            .map_err(|e| format!("source timing: {e:?}"))?
            .candid()
            .map_err(|e| e.to_string())?;
            let duration = parameters
                .initial_voting_period_seconds
                .ok_or("source voting duration unavailable")?;
            let quiet = parameters
                .wait_for_quiet_deadline_increase_seconds
                .ok_or("source quiet interval unavailable")?;
            let required = timing_margin(duration, quiet, record.target.delivery_margin_seconds)?;
            if target
                .proposal
                .as_ref()
                .is_none_or(|p| client::digest(p).as_slice() != record.content_digest)
                || client::deadline(&target)
                    .is_none_or(|deadline| deadline.saturating_sub(now) < required)
            {
                record.phase = MirrorPhase::Infeasible(
                    "content mismatch or insufficient ordinary/quiet/execution margin".into(),
                );
                return Ok(());
            }
            record.phase = MirrorPhase::Creating {
                at_seconds: now,
                search_before: None,
            };
            put(record); // submission identity survives unknown replies/upgrades
            let response: sns::ManageNeuronResponse = client::call(
                source.governance,
                "manage_neuron",
                sns::ManageNeuron {
                    subaccount: auto.proposer_neuron.clone(),
                    command: Some(sns::Command::MakeProposal(expected)),
                },
            )
            .await?;
            match response.command {
                Some(sns::Command1::MakeProposal(id)) => {
                    record.phase = MirrorPhase::Mapped {
                        jupiter_proposal: id.proposal_id.ok_or("missing submitted proposal ID")?.id,
                    };
                }
                Some(sns::Command1::Error(e)) => {
                    record.phase = MirrorPhase::ManualReview(format!(
                        "mirror definitively rejected {}: {}",
                        e.error_type, e.error_message
                    ))
                }
                _ => {
                    return Err(
                        "mirror creation response unavailable; reconcile canonical identity".into(),
                    )
                }
            }
        }
        MirrorPhase::Creating {
            at_seconds,
            search_before,
        } => {
            let page = client::list(source.governance, search_before, vec![]).await?;
            let matches: Vec<_> = page
                .iter()
                .filter(|p| {
                    p.proposer
                        .as_ref()
                        .is_some_and(|id| id.id == auto.proposer_neuron)
                        && p.proposal.as_ref() == Some(&expected)
                })
                .collect();
            if matches.len() > 1 {
                record.phase = MirrorPhase::ManualReview(
                    "multiple canonical mirrors match the frozen submission".into(),
                );
            } else if let Some(found) = matches.first() {
                record.phase = MirrorPhase::Mapped {
                    jupiter_proposal: found.id.as_ref().ok_or("canonical mirror ID missing")?.id,
                };
            } else if page.is_empty()
                || page
                    .last()
                    .is_some_and(|p| p.proposal_creation_timestamp_seconds < at_seconds)
            {
                record.phase = MirrorPhase::ManualReview(
                    "unknown creation not found in canonical history; no automatic resubmission"
                        .into(),
                );
            } else {
                record.phase = MirrorPhase::Creating {
                    at_seconds,
                    search_before: page.last().and_then(|p| p.id.as_ref().map(|id| id.id)),
                };
            }
        }
        MirrorPhase::Mapped { jupiter_proposal } => {
            let p = client::proposal(source.governance, jupiter_proposal).await?;
            if p.proposer
                .as_ref()
                .is_none_or(|id| id.id != auto.proposer_neuron)
                || p.proposal.as_ref() != Some(&expected)
            {
                record.phase = MirrorPhase::ManualReview(
                    "source proposer or canonical mirror binding mismatch".into(),
                );
                return Ok(());
            }
            if let Some(vote) = client::decision(source.governance, jupiter_proposal).await? {
                record.phase = MirrorPhase::Voting {
                    jupiter_proposal,
                    vote,
                };
                put(record);
            }
        }
        MirrorPhase::Voting {
            jupiter_proposal,
            vote,
        } => {
            let target = client::proposal(record.target.governance, record.proposal_id).await?;
            if target
                .proposal
                .as_ref()
                .is_none_or(|p| client::digest(p).as_slice() != record.content_digest)
            {
                record.phase = MirrorPhase::ManualReview("canonical target content changed".into());
                return Ok(());
            }
            let Some(ballot) = target
                .ballots
                .iter()
                .find(|(id, _)| id == &hex::encode(&record.target.neuron))
            else {
                record.phase = MirrorPhase::Infeasible(
                    "designated target neuron has no active ballot; no vote attempted".into(),
                );
                return Ok(());
            };
            let ballot = ballot.1.vote;
            if ballot == vote {
                record.phase = MirrorPhase::Verified {
                    jupiter_proposal,
                    vote,
                };
                return Ok(());
            }
            if ballot != 0 {
                record.phase =
                    MirrorPhase::ManualReview("contradictory canonical target ballot".into());
                return Ok(());
            }
            if target.decided_timestamp_seconds != 0 {
                record.phase = MirrorPhase::Infeasible(
                    "target was already canonically decided before delivery".into(),
                );
                return Ok(());
            }
            if client::deadline(&target).is_none_or(|deadline| now >= deadline) {
                record.phase = MirrorPhase::Infeasible(
                    "target deadline exhausted without the instructed ballot".into(),
                );
                return Ok(());
            }
            // Any uncertain registration is reconciled against the actual ballot
            // above on the next invocation before another identical instruction.
            let response: sns::ManageNeuronResponse = client::call(
                record.target.governance,
                "manage_neuron",
                sns::ManageNeuron {
                    subaccount: record.target.neuron.clone(),
                    command: Some(sns::Command::RegisterVote(sns::RegisterVote {
                        proposal: Some(sns::ProposalId {
                            id: record.proposal_id,
                        }),
                        vote,
                    })),
                },
            )
            .await?;
            if let Some(sns::Command1::Error(error)) = response.command {
                record.phase = MirrorPhase::ManualReview(format!(
                    "target definitively rejected ballot {}: {}",
                    error.error_type, error.error_message
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_charges_do_not_trigger_funding_but_rejected_charges_do() {
        assert_eq!(operating_balance(100, 90, 90), 100);
        assert_eq!(operating_balance(100, 90, 0), 10);
        assert_eq!(operating_balance(190, 90, 0), 100);
        assert_eq!(operating_balance(10, 90, 0), 0);
        assert_eq!(operating_balance(100, 100, 100), 100);
        assert!(!can_cover_submission(100, 100, 1));
        assert!(!can_cover_submission(100, 101, 1));
        assert!(can_cover_submission(100, 99, 1));
    }
    #[test]
    fn ordinary_periods_alone_do_not_establish_delivery_feasibility() {
        let e = crate::economics::get();
        assert!(
            timing_margin(e.jupiter_voting_seconds, 3600, 3600).unwrap() < e.target_voting_seconds
        );
        assert!(
            timing_margin(e.jupiter_voting_seconds, 86400, 3600).unwrap() > e.target_voting_seconds
        );
        assert!(timing_margin(u64::MAX, 1, 1).is_err());
        assert!(timing_margin(1, u64::MAX, 1).is_err());
    }
}
