//! Native ICP treasury proposals. One outstanding operation; rejected, failed or
//! ambiguous operations require reviewed reactivation, never a proposal loop.
use super::{automation::AutomationState, governance_client as client, types::*};
use crate::generated as sns;
use candid::{CandidType, Deserialize, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, VirtualMemory},
    DefaultMemoryImpl, StableBTreeMap, StableCell,
};
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::{
    account_identifier::account_identifier_bytes,
    index::{IcpIndexCanister, IndexOperation},
    ledger::IcrcLedgerCanister,
};
use serde::Serialize;
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Scanning {
        before: Option<u64>,
        spent_e8s: u128,
        reserved_e8s: u128,
        pages: u32,
    },
    Submitting {
        search_before: Option<u64>,
    },
    Proposed {
        proposal_id: u64,
    },
    VerifyTransfer {
        proposal_id: u64,
        end_block: u64,
        cursor: Option<u64>,
        found: Option<u64>,
    },
    TransferVerified {
        proposal_id: u64,
        block: u64,
    },
    ManualReview(String),
    Uncertain(String),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub config_version: u64,
    pub operation_id: Option<u64>,
    pub phase: Phase,
    pub source: Source,
    pub observed_balance_e8s: u64,
    pub rate_permyriad: u64,
    pub at_seconds: u64,
    pub amount_e8s: u64,
    pub actual_icp_fee_e8s: u64,
    pub start_block: u64,
    pub destination: Account,
    pub last_error: Option<String>,
}
type Memory = VirtualMemory<DefaultMemoryImpl>;
thread_local! {
    static OPERATIONS: RefCell<Option<StableBTreeMap<u64, Vec<u8>, Memory>>> = const {RefCell::new(None)};
    static SEQUENCE: RefCell<Option<StableCell<u64, Memory>>> = const {RefCell::new(None)};
    static NEXT: RefCell<Option<StableCell<u64, Memory>>> = const {RefCell::new(None)};
}
fn counter<R>(
    id: u8,
    cell: &'static std::thread::LocalKey<RefCell<Option<StableCell<u64, Memory>>>>,
    f: impl FnOnce(&mut StableCell<u64, Memory>) -> R,
) -> R {
    cell.with_borrow_mut(|c| {
        f(c.get_or_insert_with(|| {
            StableCell::init(crate::state::programme_memory(MemoryId::new(id)), 0)
        }))
    })
}
pub fn has_requested_operation() -> bool {
    next_operation() != 0
}
fn next_operation() -> u64 {
    counter(20, &NEXT, |c| *c.get())
}
fn operations<R>(f: impl FnOnce(&mut StableBTreeMap<u64, Vec<u8>, Memory>) -> R) -> R {
    OPERATIONS.with_borrow_mut(|map| {
        f(map.get_or_insert_with(|| {
            StableBTreeMap::init(crate::state::programme_memory(MemoryId::new(19)))
        }))
    })
}
pub fn operation(id: u64) -> Option<State> {
    operations(|map| {
        map.get(&id)
            .map(|bytes| candid::decode_one(&bytes).expect("treasury operation"))
    })
}
/// Explicit governed request, never an automatic recurring proposal loop.
/// TransferVerified settles the native treasury leg only. Production activation
/// remains blocked pending attribution of the downstream Relay funding credit.
pub fn request_next(
    id: u64,
    expected_previous: Option<u64>,
    caller: Principal,
) -> Result<(), String> {
    let state = read().ok_or("no settled treasury operation")?;
    if caller == Principal::anonymous() || caller != state.source.governance {
        return Err("only pinned jUP Governance may request a treasury operation".into());
    }
    if state.operation_id != expected_previous
        || !matches!(state.phase, Phase::TransferVerified { .. })
    {
        return Err(
            "previous native treasury operation must be conclusively transfer-verified".into(),
        );
    }
    let sequence = counter(18, &SEQUENCE, |c| *c.get());
    if id == 0 || id <= sequence || operation(id).is_some() {
        return Err("treasury operation ID must be new and increasing".into());
    }
    let next = next_operation();
    if next != 0 && next != id {
        return Err("a new treasury operation is already requested".into());
    }
    if state.operation_id.is_none() {
        // ID zero is reserved for the frozen legacy predecessor. Do not change
        // its optional ID/title: an old in-flight proposal binds that title.
        if operation(0).as_ref().is_some_and(|old| old != &state) {
            return Err("legacy treasury predecessor archive is already bound".into());
        }
        operations(|map| {
            map.insert(
                0,
                candid::encode_one(&state).expect("archive legacy treasury predecessor"),
            )
        });
    }
    counter(20, &NEXT, |c| {
        c.set(id);
    });
    Ok(())
}

thread_local! { static CELL: RefCell<Option<StableCell<Vec<u8>, Memory>>> = const {RefCell::new(None)}; }
thread_local! { static HISTORY: RefCell<Option<StableBTreeMap<u64, Vec<u8>, Memory>>> = const {RefCell::new(None)}; }
pub fn read() -> Option<State> {
    CELL.with_borrow_mut(|cell| {
        let cell = cell.get_or_insert_with(|| {
            StableCell::init(crate::state::programme_memory(MemoryId::new(13)), vec![])
        });
        if cell.get().is_empty() {
            None
        } else {
            Some(candid::decode_one(cell.get()).expect("treasury journal"))
        }
    })
}
fn set(state: &State) {
    let mut bounded = state.clone();
    bounded.last_error = bounded.last_error.as_deref().map(super::compact);
    if let Phase::ManualReview(reason) | Phase::Uncertain(reason) = &mut bounded.phase {
        *reason = super::compact(reason);
    }
    let state = &bounded;
    if let Some(id) = state.operation_id {
        operations(|map| {
            map.insert(
                id,
                candid::encode_one(state).expect("encode treasury operation"),
            )
        });
    }
    let _ = read();
    CELL.with_borrow_mut(|cell| {
        cell.as_mut()
            .expect("treasury journal")
            .set(candid::encode_one(state).expect("encode treasury journal"));
    });
    HISTORY.with_borrow_mut(|map| {
        map.get_or_insert_with(|| {
            StableBTreeMap::init(crate::state::programme_memory(MemoryId::new(14)))
        })
        .insert(
            state.config_version,
            candid::encode_one(state).expect("encode treasury history"),
        );
    });
}
pub fn record(version: u64) -> Option<State> {
    HISTORY.with_borrow_mut(|map| {
        map.get_or_insert_with(|| {
            StableBTreeMap::init(crate::state::programme_memory(MemoryId::new(14)))
        })
        .get(&version)
        .map(|bytes| candid::decode_one(&bytes).expect("treasury history record"))
    })
}
pub fn can_reconfigure() -> bool {
    read().is_none_or(|s| {
        matches!(
            s.phase,
            Phase::TransferVerified { .. } | Phase::ManualReview(_) | Phase::Scanning { .. }
        )
    })
}

/// Integer ICP specialization of the pinned official seven-day limit, including
/// its minimum 1 XDR/ICP clamp. Conservative floor at the e8s boundary.
pub fn framework_limit(balance: u64, rate: u64) -> u64 {
    let rate = rate.max(10_000) as u128;
    let value = u128::from(balance) * rate; // XDR * 10^12
    if value <= 100_000 * 1_000_000_000_000_u128 {
        balance
    } else if value <= 1_200_000 * 1_000_000_000_000_u128 {
        balance / 4
    } else {
        u64::try_from(300_000 * 1_000_000_000_000_u128 / rate)
            .expect("large-treasury cap fits ICP u64")
    }
}
fn proposal(state: &State) -> sns::Proposal {
    sns::Proposal { title: format!("Jupiter treasury reinvestment v{}{}", state.config_version, state.operation_id.map(|id| format!(" operation {id}")).unwrap_or_default()),
        summary: format!("Reinvest {} ICP e8s through the canonical Relay subaccount 1. Normal base/age-bonus economics apply; this protocol capital is excluded from patron jUP issuance.", state.amount_e8s),
        url: String::new(), action: Some(sns::Action::TransferSnsTreasuryFunds(sns::TransferSnsTreasuryFunds {
            from_treasury: 1, to_principal: Some(state.destination.owner),
            to_subaccount: state.destination.subaccount.map(|subaccount| sns::Subaccount { subaccount: subaccount.to_vec() }),
            memo: None, amount_e8s: state.amount_e8s })) }
}

pub async fn tick(
    config: &ProgrammeConfig,
    auto: &AutomationConfig,
    source_state: &AutomationState,
    now: u64,
    proposer_ready: bool,
) -> Result<(), String> {
    let result = tick_inner(config, auto, source_state, now, proposer_ready).await;
    if let Some(mut state) = read() {
        state.last_error = result.as_ref().err().cloned();
        set(&state);
    }
    result
}

async fn tick_inner(
    config: &ProgrammeConfig,
    auto: &AutomationConfig,
    source_state: &AutomationState,
    now: u64,
    proposer_ready: bool,
) -> Result<(), String> {
    if !auto.treasury_enabled {
        return Ok(());
    }
    config.validate_activation(cfg!(any(test, feature = "debug_api")))?;
    let source = source_state
        .source
        .as_ref()
        .ok_or("treasury source unresolved")?;
    let ledger = IcrcLedgerCanister::new(config.icp_ledger);
    let mut state = if let Some(state) =
        read().filter(|s| s.config_version == config.version && next_operation() == 0)
    {
        state
    } else {
        if !proposer_ready {
            return Ok(());
        }
        if !can_reconfigure() {
            return Err(
                "an existing native treasury operation must finish before reactivation".into(),
            );
        }
        let balance = ledger
            .balance_of_e8s(Account {
                owner: source.governance,
                subaccount: None,
            })
            .await
            .map_err(|e| e.to_string())?;
        let fee = ledger.fee_e8s().await.map_err(|e| e.to_string())?;
        if balance <= fee.saturating_add(crate::economics::get().minimum_faucet_commitment_e8s) {
            return Ok(());
        }
        let quote = jupiter_ic_clients::cmc::get_icp_xdr_conversion_rate(
            jupiter_ic_clients::constants::cycles_minting_canister_id(),
        )
        .await
        .map_err(|e| e.to_string())?;
        if quote.timestamp_seconds > now || now - quote.timestamp_seconds > 86_400 {
            return Err("native ICP treasury valuation quote is not fresh".into());
        }
        let mut sub = [0; 32];
        sub[31] = 1;
        // No await from selecting/consuming the requested identity to persisting
        // the new operation. A duplicate request cannot create a successor.
        let requested = next_operation();
        let operation_id = counter(18, &SEQUENCE, |c| {
            let id = if requested == 0 {
                c.get().checked_add(1).expect("treasury identity exhausted")
            } else {
                requested
            };
            c.set(id);
            id
        });
        let state = State {
            operation_id: Some(operation_id),
            config_version: config.version,
            source: source.clone(),
            observed_balance_e8s: balance,
            rate_permyriad: quote.xdr_permyriad_per_icp,
            at_seconds: now,
            amount_e8s: 0,
            actual_icp_fee_e8s: fee,
            start_block: 0,
            destination: Account {
                owner: config.canonical_relay,
                subaccount: Some(sub),
            },
            phase: Phase::Scanning {
                before: None,
                spent_e8s: 0,
                reserved_e8s: 0,
                pages: 0,
            },
            last_error: None,
        };
        set(&state);
        counter(20, &NEXT, |c| {
            c.set(0);
        });
        state
    };
    match state.phase.clone() {
        Phase::Scanning {
            before,
            mut spent_e8s,
            mut reserved_e8s,
            pages,
        } => {
            if pages >= 10_000 {
                return Err(
                    "treasury history cap: a partial scan cannot authorize a proposal".into(),
                );
            }
            let page = client::list(source.governance, before, vec![]).await?;
            if let Some(p) = page.first() {
                if let Some(sns::Proposal {
                    action: Some(sns::Action::TransferSnsTreasuryFunds(action)),
                    ..
                }) = &p.proposal
                {
                    if action.from_treasury == 1 {
                        if p.executed_timestamp_seconds > 0 {
                            if now.saturating_sub(p.executed_timestamp_seconds) < 604800 {
                                spent_e8s += u128::from(action.amount_e8s);
                            }
                        } else if p.failed_timestamp_seconds == 0 {
                            let decision = client::decision(
                                source.governance,
                                p.id.as_ref().ok_or("treasury proposal ID missing")?.id,
                            )
                            .await?;
                            if decision != Some(2) {
                                reserved_e8s += u128::from(action.amount_e8s);
                            }
                        }
                    }
                }
                state.phase = Phase::Scanning {
                    before: p.id.as_ref().map(|id| id.id),
                    spent_e8s,
                    reserved_e8s,
                    pages: pages + 1,
                };
            } else {
                if !proposer_ready {
                    return Err("new treasury submission waits for reviewed proposer readiness; existing work remains resumable".into());
                }
                let current_balance = ledger
                    .balance_of_e8s(Account {
                        owner: source.governance,
                        subaccount: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                let balance = current_balance.min(state.observed_balance_e8s);
                let allowance = u128::from(framework_limit(balance, state.rate_permyriad))
                    .saturating_sub(spent_e8s)
                    .saturating_sub(reserved_e8s);
                let available = u128::from(balance)
                    .saturating_sub(reserved_e8s)
                    .saturating_sub(u128::from(auto.treasury_retain_icp_e8s))
                    .saturating_sub(u128::from(state.actual_icp_fee_e8s));
                let amount = allowance
                    .min(available)
                    .min(u128::from(auto.treasury_max_proposal_icp_e8s));
                let minimum = crate::economics::get()
                    .minimum_faucet_commitment_e8s
                    .checked_add(state.actual_icp_fee_e8s)
                    .ok_or("Relay endowment minimum overflow")?;
                if amount < u128::from(minimum) {
                    state.phase = Phase::ManualReview("no feasible native treasury amount after seven-day limits, other commitments, retained buffer and both route fees; reviewed reactivation required".into());
                } else {
                    state.amount_e8s =
                        u64::try_from(amount).map_err(|_| "treasury amount overflow")?;
                    state.start_block = ledger.chain_length().await.map_err(|e| e.to_string())?;
                    state.phase = Phase::Submitting {
                        search_before: None,
                    };
                    set(&state);
                    let reply: sns::ManageNeuronResponse = client::call(
                        source.governance,
                        "manage_neuron",
                        sns::ManageNeuron {
                            subaccount: auto.proposer_neuron.clone(),
                            command: Some(sns::Command::MakeProposal(proposal(&state))),
                        },
                    )
                    .await?;
                    match reply.command {
                        Some(sns::Command1::MakeProposal(id)) => {
                            state.phase = Phase::Proposed {
                                proposal_id: id
                                    .proposal_id
                                    .ok_or("treasury proposal ID missing")?
                                    .id,
                            }
                        }
                        other => {
                            state.phase = Phase::ManualReview(format!(
                                "native treasury proposal rejected or unavailable: {other:?}"
                            ))
                        }
                    }
                }
            }
        }
        Phase::Submitting { search_before } => {
            let page = client::list(source.governance, search_before, vec![]).await?;
            let expected = proposal(&state);
            let found: Vec<_> = page
                .iter()
                .filter(|p| {
                    p.proposer
                        .as_ref()
                        .is_some_and(|id| id.id == auto.proposer_neuron)
                        && p.proposal.as_ref() == Some(&expected)
                })
                .collect();
            if found.len() == 1 {
                state.phase = Phase::Proposed {
                    proposal_id: found[0]
                        .id
                        .as_ref()
                        .ok_or("canonical treasury proposal ID")?
                        .id,
                };
            } else if found.len() > 1
                || page.is_empty()
                || page
                    .last()
                    .is_some_and(|p| p.proposal_creation_timestamp_seconds < state.at_seconds)
            {
                state.phase = Phase::Uncertain("uncertain treasury submission has no unique canonical binding; no resubmission or ordinary reactivation".into());
            } else {
                state.phase = Phase::Submitting {
                    search_before: page.last().and_then(|p| p.id.as_ref().map(|id| id.id)),
                };
            }
        }
        Phase::Proposed { proposal_id } => {
            let p = client::proposal(source.governance, proposal_id).await?;
            if p.proposer
                .as_ref()
                .is_none_or(|id| id.id != auto.proposer_neuron)
                || p.proposal.as_ref() != Some(&proposal(&state))
            {
                state.phase =
                    Phase::Uncertain("canonical treasury proposer/action binding mismatch".into());
            } else if p.failed_timestamp_seconds > 0 {
                state.phase = Phase::ManualReview(format!(
                    "native treasury execution failed: {:?}",
                    p.failure_reason
                ));
            } else if p.executed_timestamp_seconds > 0 {
                let tip = ledger.chain_length().await.map_err(|e| e.to_string())?;
                state.phase = Phase::VerifyTransfer {
                    proposal_id,
                    end_block: tip.checked_sub(1).ok_or("empty canonical ICP ledger")?,
                    cursor: None,
                    found: None,
                };
            } else if client::decision(source.governance, proposal_id).await? == Some(2) {
                state.phase = Phase::ManualReview(
                    "native treasury proposal rejected; no automatic reproposal".into(),
                );
            }
        }
        Phase::VerifyTransfer {
            proposal_id,
            end_block,
            cursor,
            mut found,
        } => {
            let index = IcpIndexCanister::new(config.icp_index);
            let id: Principal = ic_cdk::call::Call::bounded_wait(config.icp_index, "ledger_id")
                .await
                .map_err(|e| e.to_string())?
                .candid()
                .map_err(|e| e.to_string())?;
            if id != config.icp_ledger
                || index
                    .status()
                    .await
                    .map_err(|e| e.to_string())?
                    .num_blocks_synced
                    <= end_block
            {
                return Err("treasury proof waits for the complete configured ICP Index".into());
            }
            let before = cursor
                .or_else(|| end_block.checked_add(1))
                .ok_or("ICP block boundary exhausted")?;
            let from = hex::encode(account_identifier_bytes(source.governance, None));
            let to = hex::encode(account_identifier_bytes(
                state.destination.owner,
                state.destination.subaccount,
            ));
            let page = index
                .get_account_identifier_transactions(from.clone(), Some(before), 1)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(tx) = page.transactions.first() {
                if tx.id >= before {
                    return Err("non-progressing treasury receipt history".into());
                }
                if tx.id < state.start_block {
                    state.phase = found.map_or_else(
                        || {
                            Phase::Uncertain(
                                "executed native treasury transfer has no exact Ledger match"
                                    .into(),
                            )
                        },
                        |block| Phase::TransferVerified { proposal_id, block },
                    );
                } else {
                    if let IndexOperation::Transfer {
                        from: actual_from,
                        to: actual_to,
                        amount,
                        fee,
                        spender,
                    } = &tx.transaction.operation
                    {
                        if actual_from.eq_ignore_ascii_case(&from)
                            && actual_to.eq_ignore_ascii_case(&to)
                            && amount.e8s() == state.amount_e8s
                            && fee.e8s() == state.actual_icp_fee_e8s
                            && spender.is_none()
                            && found.replace(tx.id).is_some()
                        {
                            state.phase = Phase::Uncertain(
                                "multiple exact treasury transfer matches require canonical review"
                                    .into(),
                            );
                            set(&state);
                            return Ok(());
                        }
                    }
                    if page.oldest_tx_id == Some(tx.id) {
                        state.phase = found.map_or_else(
                            || {
                                Phase::Uncertain(
                                    "executed native treasury transfer is absent".into(),
                                )
                            },
                            |block| Phase::TransferVerified { proposal_id, block },
                        );
                    } else {
                        state.phase = Phase::VerifyTransfer {
                            proposal_id,
                            end_block,
                            cursor: Some(tx.id),
                            found,
                        };
                    }
                }
            } else {
                return Err("partial treasury history is not proof of absence".into());
            }
        }
        Phase::TransferVerified { .. } | Phase::ManualReview(_) | Phase::Uncertain(_) => {} // unknown financial outcomes require governed repair
    }
    set(&state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(phase: Phase) -> State {
        State {
            config_version: 7,
            operation_id: Some(1),
            phase,
            source: Source {
                root: Principal::from_slice(&[1]),
                governance: Principal::from_slice(&[2]),
                ledger: Principal::from_slice(&[3]),
            },
            observed_balance_e8s: 100,
            rate_permyriad: 10000,
            at_seconds: 123,
            amount_e8s: 10,
            actual_icp_fee_e8s: 1,
            start_block: 5,
            destination: Account {
                owner: Principal::from_slice(&[4]),
                subaccount: Some([0; 32]),
            },
            last_error: None,
        }
    }
    #[test]
    fn recurring_native_operation_requires_unique_id_exact_predecessor_and_conclusive_transfer() {
        let governance = Principal::from_slice(&[2]);
        counter(18, &SEQUENCE, |c| {
            c.set(1);
        });
        counter(20, &NEXT, |c| {
            c.set(0);
        });
        for phase in [
            Phase::ManualReview("rejected".into()),
            Phase::Uncertain("ambiguous".into()),
            Phase::Proposed { proposal_id: 1 },
        ] {
            set(&fixture(phase));
            assert!(request_next(2, Some(1), governance).is_err());
        }
        set(&fixture(Phase::TransferVerified {
            proposal_id: 1,
            block: 10,
        }));
        assert!(request_next(2, Some(1), Principal::anonymous()).is_err());
        assert!(request_next(1, Some(1), governance).is_err());
        assert!(request_next(2, None, governance).is_err());
        request_next(2, Some(1), governance).unwrap();
        request_next(2, Some(1), governance).unwrap();
        assert!(request_next(3, Some(1), governance).is_err());
        assert!(matches!(
            operation(1).unwrap().phase,
            Phase::TransferVerified { block: 10, .. }
        ));
        assert_eq!(operation(1).unwrap().config_version, 7);
    }
    #[test]
    fn frozen_pre_extension_journal_decodes_and_retains_legacy_identity_on_continuation() {
        #[derive(CandidType)]
        struct OldState {
            config_version: u64,
            phase: Phase,
            source: Source,
            observed_balance_e8s: u64,
            rate_permyriad: u64,
            at_seconds: u64,
            amount_e8s: u64,
            actual_icp_fee_e8s: u64,
            start_block: u64,
            destination: Account,
            last_error: Option<String>,
        }
        let old = fixture(Phase::TransferVerified {
            proposal_id: 10,
            block: 20,
        });
        let bytes = candid::encode_one(OldState {
            config_version: old.config_version,
            phase: old.phase,
            source: old.source,
            observed_balance_e8s: old.observed_balance_e8s,
            rate_permyriad: old.rate_permyriad,
            at_seconds: old.at_seconds,
            amount_e8s: old.amount_e8s,
            actual_icp_fee_e8s: old.actual_icp_fee_e8s,
            start_block: old.start_block,
            destination: old.destination,
            last_error: old.last_error,
        })
        .unwrap();
        let restored: State = candid::decode_one(&bytes).unwrap();
        assert_eq!(restored.operation_id, None);
        assert_eq!(
            proposal(&restored).title,
            "Jupiter treasury reinvestment v7"
        );
        counter(18, &SEQUENCE, |c| {
            c.set(0);
        });
        counter(20, &NEXT, |c| {
            c.set(0);
        });
        operations(|map| map.clear_new());
        set(&restored);
        request_next(1, None, restored.source.governance).unwrap();
        assert_eq!(operation(0), Some(restored.clone()));
        set(&fixture(Phase::Proposed { proposal_id: 11 }));
        assert_eq!(operation(0), Some(restored));
    }
    #[test]
    fn pinned_framework_limit_boundaries_and_clamp() {
        assert_eq!(
            framework_limit(100_000 * 100_000_000, 1),
            100_000 * 100_000_000
        );
        assert_eq!(
            framework_limit(100_000 * 100_000_000 + 1, 10_000),
            (100_000 * 100_000_000 + 1) / 4
        );
        assert_eq!(
            framework_limit(1_200_000 * 100_000_000, 10_000),
            300_000 * 100_000_000
        );
        assert_eq!(
            framework_limit(2_000_000 * 100_000_000, 20_000),
            150_000 * 100_000_000
        );
    }
}
