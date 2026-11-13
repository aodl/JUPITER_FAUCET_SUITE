//! Complete, coherent native reward-event shares. No IO emission or fee subsidy.
use super::{discovery, storage, types::Source};
use crate::generated as sns;
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use ic_stable_structures::{
    memory_manager::{MemoryId, VirtualMemory},
    DefaultMemoryImpl, StableBTreeMap, StableCell,
};
use serde::Serialize;
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub source: Source,
    pub config_version: u64,
    pub round: u64,
    pub end_timestamp_seconds: u64,
    pub eligible_shares: Nat,
    pub owner_count: u64,
    pub slot: u8,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Scan {
    pub event: Event,
    pub cursor: Option<Vec<u8>>,
    pub pages: u32,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub active: Option<Event>,
    pub scan: Option<Scan>,
    pub last_error: Option<String>,
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct Weight {
    owner: Principal,
    shares: Nat,
}
type Memory = VirtualMemory<DefaultMemoryImpl>;
type Weights = StableBTreeMap<[u8; 32], Vec<u8>, Memory>;
thread_local! {
    static META: RefCell<Option<StableCell<Vec<u8>, Memory>>> = const {RefCell::new(None)};
    static A: RefCell<Option<Weights>> = const {RefCell::new(None)};
    static B: RefCell<Option<Weights>> = const {RefCell::new(None)};
}
fn memory(id: u8) -> Memory {
    crate::state::programme_memory(MemoryId::new(id))
}
pub fn read() -> State {
    META.with_borrow_mut(|cell| {
        let cell = cell.get_or_insert_with(|| StableCell::init(memory(9), vec![]));
        if cell.get().is_empty() {
            State::default()
        } else {
            candid::decode_one(cell.get()).expect("participation state")
        }
    })
}
fn set(state: &State) {
    let mut bounded = state.clone();
    bounded.last_error = bounded.last_error.as_deref().map(super::compact);
    let state = &bounded;
    let _ = read();
    META.with_borrow_mut(|cell| {
        cell.as_mut()
            .expect("participation state")
            .set(candid::encode_one(state).expect("encode participation state"));
    });
}

/// A stored aggregate cannot prove which policy excluded its neuron shares.
/// Rebuild from native events after an upgrade before exposing any quote. This
/// touches no funded IO entitlement and leaves participation data untouched for
/// deployed V1 without an explicitly configured programme.
pub(crate) fn invalidate_after_upgrade() {
    if storage::read().config.is_some() {
        set(&State::default());
    }
}
fn weights<R>(slot: u8, f: impl FnOnce(&mut Weights) -> R) -> R {
    let (map, id) = match slot {
        0 => (&A, 10),
        1 => (&B, 11),
        _ => panic!("participation slot"),
    };
    map.with_borrow_mut(|map| f(map.get_or_insert_with(|| StableBTreeMap::init(memory(id)))))
}
async fn latest(governance: Principal) -> Result<sns::RewardEvent, String> {
    Call::bounded_wait(governance, "get_latest_reward_event")
        .await
        .map_err(|e| format!("native event unavailable: {e:?}"))?
        .candid()
        .map_err(|e| format!("native event decode: {e:?}"))
}

pub async fn tick() -> Result<(), String> {
    let result = tick_inner().await;
    let mut state = read();
    state.last_error = result.as_ref().err().cloned();
    set(&state);
    result
}

async fn tick_inner() -> Result<(), String> {
    let Some(config) = storage::read().config else {
        return Ok(());
    };
    let source = discovery::resolve(config.jup_root).await?;
    let event = latest(source.governance).await?;
    let end = event
        .end_timestamp_seconds
        .ok_or("no settled native participation event")?;
    let mut state = read();
    if state.active.as_ref().is_some_and(|e| {
        e.source == source
            && e.round == event.round
            && e.end_timestamp_seconds == end
            && e.config_version == config.version
    }) {
        return Ok(());
    }
    if state.scan.as_ref().is_none_or(|s| {
        s.event.round != event.round
            || s.event.end_timestamp_seconds != end
            || s.event.source != source
            || s.event.config_version != config.version
    }) {
        let slot = state.active.as_ref().map_or(0, |e| 1 - e.slot);
        weights(slot, |map| map.clear_new());
        state.scan = Some(Scan {
            event: Event {
                source: source.clone(),
                config_version: config.version,
                round: event.round,
                end_timestamp_seconds: end,
                eligible_shares: 0_u8.into(),
                owner_count: 0,
                slot,
            },
            cursor: None,
            pages: 0,
        });
        set(&state);
    }
    let mut scan = state.scan.clone().expect("participation scan");
    if scan.pages >= 10_000 {
        return Err("participation scan page cap; no partial allocation published".into());
    }
    #[derive(CandidType, Deserialize)]
    struct Page {
        neurons: Vec<sns::Neuron>,
    }
    let page: Page = super::governance_client::call(
        source.governance,
        "list_neurons",
        crate::clients::ListNeurons {
            of_principal: None,
            limit: 100,
            start_page_at: scan
                .cursor
                .clone()
                .map(|id| crate::clients::NeuronId { id }),
        },
    )
    .await?;
    if page.neurons.len() > 100 {
        return Err("oversized participation page".into());
    }
    // Validate the whole page before writing staging shares.
    let mut entries = Vec::new();
    for n in &page.neurons {
        let id = &n.id.as_ref().ok_or("participation neuron ID missing")?.id;
        if id.len() != 32 || scan.cursor.as_ref().is_some_and(|c| id <= c) {
            return Err("non-progressing participation page".into());
        }
        scan.cursor = Some(id.clone());
        if config.excludes_neuron(id, ic_cdk::api::canister_self()) {
            continue;
        }
        let Some(participation) = &n.latest_reward_event_participation else {
            continue;
        };
        let Some(stamp) = participation.reward_event_end_timestamp_seconds else {
            continue;
        };
        if stamp > end {
            return Err("native event changed within participation scan".into());
        }
        if stamp != end {
            continue;
        } // an earlier event is not participation in this one
        let shares = participation
            .reward_shares
            .clone()
            .ok_or("matching event has missing reward shares")?;
        if shares == 0_u8 {
            continue;
        }
        if shares.0.bits() > 128 {
            return Err("native participation share bound exceeded".into());
        }
        let narrow: discovery::Neuron =
            candid::decode_one(&candid::encode_one(n).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if let Some(owner) = discovery::economic_owner(&narrow).filter(|p| *p != source.governance)
        {
            entries.push((owner, shares));
        }
    }
    let observed = latest(source.governance).await?;
    if observed.round != event.round || observed.end_timestamp_seconds != Some(end) {
        return Err("native event changed before publication; staging is not reward-usable".into());
    }
    if storage::read().config.as_ref() != Some(&config) {
        return Ok(());
    }
    for (owner, shares) in entries {
        weights(scan.event.slot, |map| {
            let key = jupiter_ic_clients::account_identifier::account_identifier_bytes(owner, None);
            let mut value: Weight = map
                .get(&key)
                .map(|bytes| candid::decode_one(&bytes).expect("participation weight"))
                .unwrap_or(Weight {
                    owner,
                    shares: Nat::from(0_u8),
                });
            value.shares.0 += &shares.0;
            map.insert(
                key,
                candid::encode_one(value).expect("encode participation weight"),
            );
        });
        scan.event.eligible_shares.0 += shares.0;
    }
    scan.pages += 1;
    if page.neurons.is_empty() {
        scan.event.owner_count = weights(scan.event.slot, |map| map.len());
        state.active = Some(scan.event);
        state.scan = None;
    } else {
        state.scan = Some(scan);
    }
    state.last_error = None;
    set(&state);
    Ok(())
}

/// Gross share quote only. Dispatch requires a separately reviewed IO fee policy
/// and an authenticated funded receipt. Zero weight leaves the pool unallocated.
pub fn quote(pool: u64, owner: Principal) -> Option<Nat> {
    let config = storage::read().config?;
    let state = read();
    if state.scan.is_some() || state.last_error.is_some() {
        return None;
    }
    let event = state.active?;
    if event.config_version != config.version || event.eligible_shares == 0_u8 {
        return None;
    }
    let key = jupiter_ic_clients::account_identifier::account_identifier_bytes(owner, None);
    let weight: Weight = weights(event.slot, |map| map.get(&key))
        .map(|bytes| candid::decode_one(&bytes).expect("participation weight"))?;
    Some(Nat(
        Nat::from(pool).0 * weight.shares.0 / event.eligible_shares.0
    ))
}
