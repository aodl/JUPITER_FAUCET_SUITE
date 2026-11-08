//! One-time native genesis reserve transport. Unknown financial outcomes require
//! review; they are never assumed successful and never automatically replayed.
use super::{discovery, governance_client as client, storage, types::Source};
use crate::generated as sns;
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, VirtualMemory},
    DefaultMemoryImpl, StableCell,
};
use icrc_ledger_types::icrc1::account::Account;
use jupiter_ic_clients::ledger::IcrcLedgerCanister;
use serde::Serialize;
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Default)]
pub enum ReserveTransport {
    #[default]
    NotDispatched,
    Dispatched {
        neuron: Vec<u8>,
        source: Source,
        destination: Account,
        expected_net_e8s: u64,
        fee_e8s: u64,
    },
    ReceiptPending {
        neuron: Vec<u8>,
        governance: Principal,
        ledger: Principal,
        block: u64,
        destination: Account,
        expected_net_e8s: u64,
        fee_e8s: u64,
    },
    Verified {
        block: Nat,
        amount_e8s: u64,
        fee_e8s: u64,
    },
    ManualReview {
        neuron: Vec<u8>,
        source: Source,
        destination: Account,
        expected_net_e8s: u64,
        fee_e8s: u64,
        reason: String,
    },
}
type Memory = VirtualMemory<DefaultMemoryImpl>;

fn hold_dispatch(checkpoint: ReserveTransport, reason: String) -> ReserveTransport {
    match checkpoint {
        ReserveTransport::Dispatched {
            neuron,
            source,
            destination,
            expected_net_e8s,
            fee_e8s,
        } => ReserveTransport::ManualReview {
            neuron,
            source,
            destination,
            expected_net_e8s,
            fee_e8s,
            reason,
        },
        other => other,
    }
}
thread_local! { static STATE: RefCell<Option<StableCell<Vec<u8>, Memory>>> = const {RefCell::new(None)}; }
pub fn read() -> ReserveTransport {
    STATE.with_borrow_mut(|cell| {
        let cell = cell.get_or_insert_with(|| {
            StableCell::init(crate::state::programme_memory(MemoryId::new(12)), vec![])
        });
        if cell.get().is_empty() {
            ReserveTransport::default()
        } else {
            candid::decode_one(cell.get()).expect("reserve transport state")
        }
    })
}
fn set(state: &ReserveTransport) {
    let mut bounded = state.clone();
    if let ReserveTransport::ManualReview { reason, .. } = &mut bounded {
        *reason = super::compact(reason);
    }
    let state = &bounded;
    let _ = read();
    STATE.with_borrow_mut(|cell| {
        cell.as_mut()
            .expect("reserve transport state")
            .set(candid::encode_one(state).expect("encode reserve transport"));
    });
}

pub async fn tick() -> Result<(), String> {
    let Some(config) = storage::read().config else {
        return Ok(());
    };
    let Some(id) = &config.reserve_transport_neuron else {
        return Ok(());
    };
    match read() {
        ReserveTransport::Verified { .. } => return Ok(()),
        ReserveTransport::ReceiptPending {
            neuron,
            governance,
            ledger,
            block,
            destination,
            expected_net_e8s,
            fee_e8s,
        } => {
            return verify(
                neuron,
                governance,
                ledger,
                block,
                destination,
                expected_net_e8s,
                fee_e8s,
            )
            .await;
        }
        ReserveTransport::ManualReview { reason, .. } => return Err(reason),
        checkpoint @ ReserveTransport::Dispatched { .. } => {
            set(&hold_dispatch(checkpoint, "native reserve disbursement outcome unresolved; canonical review required before any further spend".into()));
            return Err("reserve transport outcome requires manual review".into());
        }
        ReserveTransport::NotDispatched => {}
    }
    let source = discovery::resolve(config.jup_root).await?;
    #[derive(CandidType, Deserialize)]
    struct Mode {
        mode: Option<i32>,
    }
    #[derive(CandidType)]
    struct Empty {}
    let mode: Mode = client::call(source.governance, "get_mode", Empty {}).await?;
    if mode.mode != Some(1) {
        return Err(
            "reserve transport waits for authentic successful launch / Normal governance".into(),
        );
    }
    let reply: sns::GetNeuronResponse = client::call(
        source.governance,
        "get_neuron",
        sns::GetNeuron {
            neuron_id: Some(sns::NeuronId { id: id.clone() }),
        },
    )
    .await?;
    let Some(sns::Result::Neuron(neuron)) = reply.result else {
        return Err("reserve transport neuron unavailable".into());
    };
    if neuron.dissolve_state != Some(sns::DissolveState::DissolveDelaySeconds(0))
        || neuron
            .vesting_period_seconds
            .is_some_and(|seconds| seconds != 0)
        || !neuron.permissions.iter().any(|p| {
            p.principal == Some(ic_cdk::api::canister_self()) && p.permission_type.contains(&5)
        })
    {
        return Err("reserve transport must be zero-delay, zero-vesting, with this canister's real Disburse permission".into());
    }
    let ledger = IcrcLedgerCanister::new(source.ledger);
    let decimals: u8 = ic_cdk::call::Call::bounded_wait(source.ledger, "icrc1_decimals")
        .await
        .map_err(|e| e.to_string())?
        .candid()
        .map_err(|e| e.to_string())?;
    if decimals != 8 {
        return Err("reserve token decimals differ from reviewed jUP".into());
    }
    let fee = ledger.fee_e8s().await.map_err(|e| e.to_string())?;
    let destination = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: config.reserve_subaccount,
    };
    let net = neuron
        .cached_neuron_stake_e8s
        .checked_sub(neuron.neuron_fees_e8s)
        .and_then(|n| n.checked_sub(fee))
        .filter(|n| *n > 0)
        .ok_or("reserve transport cannot cover its actual fee")?;
    let checkpoint = ReserveTransport::Dispatched {
        neuron: id.clone(),
        source: source.clone(),
        destination,
        expected_net_e8s: net,
        fee_e8s: fee,
    };
    set(&checkpoint);
    let response = client::manage(
        source.governance,
        id.clone(),
        sns::Command::Disburse(sns::Disburse {
            amount: None,
            to_account: Some(sns::Account {
                owner: Some(destination.owner),
                subaccount: destination.subaccount.map(|s| sns::Subaccount {
                    subaccount: s.to_vec(),
                }),
            }),
        }),
    )
    .await;
    match response {
        Ok(sns::Command1::Disburse(reply)) => {
            set(&ReserveTransport::ReceiptPending {
                neuron: id.clone(),
                governance: source.governance,
                ledger: source.ledger,
                block: reply.transfer_block_height,
                destination,
                expected_net_e8s: net,
                fee_e8s: fee,
            });
            Ok(())
        }
        other => {
            let reason = format!("native reserve disbursement actual result: {other:?}");
            set(&hold_dispatch(checkpoint, reason.clone()));
            Err(reason)
        }
    }
}

async fn verify(
    neuron: Vec<u8>,
    governance: Principal,
    ledger: Principal,
    id: u64,
    destination: Account,
    net: u64,
    fee: u64,
) -> Result<(), String> {
    #[derive(CandidType)]
    struct Request {
        start: Nat,
        length: Nat,
    }
    #[derive(CandidType, Deserialize)]
    struct Response {
        first_index: Nat,
        transactions: Vec<icrc_ledger_types::icrc3::transactions::Transaction>,
    }
    let block: Response = client::call(
        ledger,
        "get_transactions",
        Request {
            start: id.into(),
            length: 1_u8.into(),
        },
    )
    .await?;
    let expected_from = Account {
        owner: governance,
        subaccount: Some(neuron.try_into().map_err(|_| "reserve neuron ID")?),
    };
    let actual = block.transactions.first().and_then(|t| t.transfer.as_ref());
    if block.first_index != id
        || block.transactions.len() != 1
        || actual.is_none_or(|t| {
            t.from != expected_from
                || t.to != destination
                || t.amount != net
                || t.fee != Some(Nat::from(fee))
                || t.spender.is_some()
        })
    {
        return Err(format!("native reserve block {id} is not yet verified (archive/review may be required); no disbursement replay"));
    }
    set(&ReserveTransport::Verified {
        block: id.into(),
        amount_e8s: net,
        fee_e8s: fee,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertain_native_disbursement_preserves_its_full_funding_context() {
        let p = |n| Principal::from_slice(&[n, 1]);
        let source = Source {
            root: p(1),
            governance: p(2),
            ledger: p(3),
        };
        let destination = Account {
            owner: p(5),
            subaccount: Some([7; 32]),
        };
        let checkpoint = ReserveTransport::Dispatched {
            neuron: vec![9; 32],
            source: source.clone(),
            destination,
            expected_net_e8s: 123,
            fee_e8s: 10,
        };
        assert_eq!(
            hold_dispatch(checkpoint, "unknown reply".into()),
            ReserveTransport::ManualReview {
                neuron: vec![9; 32],
                source,
                destination,
                expected_net_e8s: 123,
                fee_e8s: 10,
                reason: "unknown reply".into()
            }
        );
    }
}
