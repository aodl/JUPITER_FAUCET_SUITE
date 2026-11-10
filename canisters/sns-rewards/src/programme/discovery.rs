use super::{storage, types::*};
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;
use jupiter_ic_clients::sns::SnsRootCanister;
use std::collections::{BTreeMap, BTreeSet};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DissolveState {
    DissolveDelaySeconds(u64),
    WhenDissolvedTimestampSeconds(u64),
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Neuron {
    pub id: Option<crate::clients::NeuronId>,
    pub permissions: Vec<crate::clients::NeuronPermission>,
    pub cached_neuron_stake_e8s: u64,
    pub neuron_fees_e8s: u64,
    pub dissolve_state: Option<DissolveState>,
    pub auto_stake_maturity: Option<bool>,
    pub staked_maturity_e8s_equivalent: Option<u64>,
}

#[derive(CandidType, Deserialize)]
struct Neurons {
    neurons: Vec<Neuron>,
}

/// Economic attribution requires Disburse authority. Vote-only delegates cannot
/// redirect rewards. Multiple economic controllers use the existing distinct-
/// permission count / lexicographic tie policy, restricted to those controllers.
pub fn economic_owner(neuron: &Neuron) -> Option<Principal> {
    if neuron
        .cached_neuron_stake_e8s
        .saturating_sub(neuron.neuron_fees_e8s)
        == 0
        && neuron.staked_maturity_e8s_equivalent.unwrap_or(0) == 0
    {
        return None;
    }
    let mut permissions: BTreeMap<Principal, BTreeSet<i32>> = BTreeMap::new();
    for entry in &neuron.permissions {
        if let Some(p) = entry.principal {
            if p != Principal::anonymous() && p != Principal::management_canister() {
                permissions
                    .entry(p)
                    .or_default()
                    .extend(&entry.permission_type);
            }
        }
    }
    permissions
        .into_iter()
        .filter(|(_, codes)| codes.contains(&5))
        .max_by(|(a, ac), (b, bc)| {
            ac.len()
                .cmp(&bc.len())
                .then_with(|| b.as_slice().cmp(a.as_slice()))
        })
        .map(|(p, _)| p)
}

pub fn compound_eligible(neuron: &Neuron, owner: Principal, config: &ProgrammeConfig) -> bool {
    neuron
        .id
        .as_ref()
        .is_some_and(|id| id.id.len() == 32 && !config.excluded_jup_neurons.contains(&id.id))
        && economic_owner(neuron) == Some(owner)
        && neuron.dissolve_state
            == Some(DissolveState::DissolveDelaySeconds(
                crate::economics::get().year_seconds,
            ))
        && neuron.auto_stake_maturity == Some(true)
}

pub async fn get_neuron(governance: Principal, id: Vec<u8>) -> Result<Neuron, String> {
    #[derive(CandidType)]
    struct Request {
        neuron_id: Option<crate::clients::NeuronId>,
    }
    #[derive(CandidType, Deserialize)]
    enum Reply {
        Neuron(Neuron),
        Error(candid::Reserved),
    }
    #[derive(CandidType, Deserialize)]
    struct Response {
        result: Option<Reply>,
    }
    let response: Response = Call::bounded_wait(governance, "get_neuron")
        .with_arg(Request {
            neuron_id: Some(crate::clients::NeuronId { id }),
        })
        .await
        .map_err(|e| format!("get_neuron: {e:?}"))?
        .candid()
        .map_err(|e| format!("get_neuron decode: {e:?}"))?;
    match response.result {
        Some(Reply::Neuron(n)) => Ok(n),
        _ => Err("neuron unavailable".into()),
    }
}

pub(super) async fn resolve(root: Principal) -> Result<Source, String> {
    let components = SnsRootCanister
        .list_sns_canisters(root)
        .await
        .map_err(|e| e.to_string())?;
    if components.root != Some(root) {
        return Err("configured Root membership mismatch".into());
    }
    let source = Source {
        root,
        governance: components.governance.ok_or("missing Governance")?,
        ledger: components.ledger.ok_or("missing Ledger")?,
    };
    let ids = [source.root, source.governance, source.ledger];
    if ids
        .iter()
        .any(|p| *p == Principal::anonymous() || *p == Principal::management_canister())
        || ids.into_iter().collect::<BTreeSet<_>>().len() != 3
    {
        return Err("invalid SNS component identities".into());
    }
    Ok(source)
}

pub async fn tick(now: u64) -> Result<(), String> {
    let state = storage::read();
    let Some(config) = state.config else {
        return Ok(());
    };
    if state.batch.is_some() {
        return Ok(());
    } // its snapshot/slot stays frozen through history
    if state.scan.is_none() {
        if state
            .snapshot
            .as_ref()
            .is_some_and(|s| now.saturating_sub(s.started_at_seconds) < 86_400)
        {
            return Ok(());
        }
        let sources = vec![
            resolve(config.jup_root).await?,
            resolve(config.io_root).await?,
        ];
        let decimals: u8 = Call::bounded_wait(sources[0].ledger, "icrc1_decimals")
            .await
            .map_err(|e| format!("jUP decimals unavailable: {e:?}"))?
            .candid()
            .map_err(|e| format!("jUP decimals decode: {e:?}"))?;
        if decimals != crate::economics::get().decimals {
            return Err("jUP Ledger must use exactly eight decimals".into());
        }
        if sources
            .iter()
            .flat_map(|s| [s.root, s.governance, s.ledger])
            .collect::<BTreeSet<_>>()
            .len()
            != 6
        {
            return Err("the two configured SNS sources must have distinct components".into());
        }
        if storage::read().config.as_ref() != Some(&config) {
            return Ok(());
        }
        let slot = state.snapshot.as_ref().map_or(0, |s| 1 - s.slot);
        storage::clear(slot);
        storage::mutate(|s| {
            s.scan = Some(Scan {
                sources,
                source_index: 0,
                cursor: None,
                pages: 0,
                started_at_seconds: now,
                slot,
            })
        });
    }
    let scan = storage::read().scan.ok_or("scan unavailable")?;
    if scan.pages >= 10_000 {
        return Err("owner scan page cap: no complete snapshot published".into());
    }
    let source = &scan.sources[scan.source_index as usize];
    let request = crate::clients::ListNeurons {
        of_principal: None,
        limit: 100,
        start_page_at: scan
            .cursor
            .clone()
            .map(|id| crate::clients::NeuronId { id }),
    };
    let page: Neurons = Call::bounded_wait(source.governance, "list_neurons")
        .with_arg(request)
        .await
        .map_err(|e| format!("owner page: {e:?}"))?
        .candid()
        .map_err(|e| format!("owner page decode: {e:?}"))?;
    let current = storage::read();
    if current.config.as_ref() != Some(&config) || current.scan.as_ref() != Some(&scan) {
        return Ok(());
    }
    if page.neurons.len() > 100 {
        return Err("oversized owner page".into());
    }
    let mut cursor = scan.cursor.clone();
    // Validate the entire page before staging any of it.
    for n in &page.neurons {
        let id = &n.id.as_ref().ok_or("missing neuron ID")?.id;
        if id.len() != 32 || cursor.as_ref().is_some_and(|old| id <= old) {
            return Err("non-progressing owner page".into());
        }
        cursor = Some(id.clone());
    }
    for n in &page.neurons {
        if scan.source_index == 0
            && n.id
                .as_ref()
                .is_some_and(|id| config.excluded_jup_neurons.contains(&id.id))
        {
            continue;
        }
        if let Some(p) = economic_owner(n) {
            let bonus = (scan.source_index == 0 && compound_eligible(n, p, &config))
                .then(|| n.id.as_ref().expect("validated ID").id.clone());
            storage::insert(scan.slot, p, scan.source_index == 0, bonus);
        }
    }
    let completed_id =
        (page.neurons.is_empty() && scan.source_index == 1).then(super::allocate_snapshot_id);
    storage::mutate(|s| {
        let scan = s.scan.as_mut().expect("fenced owner scan");
        scan.pages += 1;
        scan.cursor = cursor;
        if page.neurons.is_empty() {
            if scan.source_index == 0 {
                scan.source_index = 1;
                scan.cursor = None;
            } else {
                let scan = s.scan.take().expect("completed owner scan");
                s.next_snapshot = completed_id.expect("completed snapshot ID");
                s.snapshot = Some(Snapshot {
                    id: completed_id.expect("completed snapshot ID"),
                    config_version: config.version,
                    sources: scan.sources,
                    started_at_seconds: scan.started_at_seconds,
                    completed_at_seconds: now,
                    slot: scan.slot,
                    owners: storage::len(scan.slot),
                });
            }
        }
    });
    Ok(())
}
