use super::{storage, types::*};
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;
use jupiter_ic_clients::sns::SnsRootCanister;
use std::collections::BTreeSet;

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

/// Shared economic-control predicate for recognition, compounding and participation.
pub fn economic_owner(neuron: &Neuron) -> Option<Principal> {
    if neuron
        .cached_neuron_stake_e8s
        .saturating_sub(neuron.neuron_fees_e8s)
        == 0
        && neuron.staked_maturity_e8s_equivalent.unwrap_or(0) == 0
    {
        return None;
    }
    crate::policy::select_owner(&neuron.permissions)
}

pub fn compound_eligible(
    neuron: &Neuron,
    owner: Principal,
    config: &ProgrammeConfig,
    rewards: Principal,
) -> bool {
    neuron
        .id
        .as_ref()
        .is_some_and(|id| id.id.len() == 32 && !config.excludes_neuron(&id.id, rewards))
        && economic_owner(neuron) == Some(owner)
        && neuron.dissolve_state
            == Some(DissolveState::DissolveDelaySeconds(
                crate::economics::get().year_seconds,
            ))
        && neuron.auto_stake_maturity == Some(true)
}

pub async fn get_neuron(governance: Principal, id: Vec<u8>) -> Result<Neuron, String> {
    lookup_neuron(governance, id)
        .await?
        .ok_or("neuron definitively absent".into())
}

pub async fn lookup_neuron(governance: Principal, id: Vec<u8>) -> Result<Option<Neuron>, String> {
    #[derive(CandidType)]
    struct Request {
        neuron_id: Option<crate::clients::NeuronId>,
    }
    #[derive(CandidType, Deserialize)]
    enum Reply {
        Neuron(Neuron),
        Error(crate::generated::GovernanceError),
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
        Some(Reply::Neuron(n)) => Ok(Some(n)),
        Some(Reply::Error(e)) if e.error_type == crate::generated::GOVERNANCE_ERROR_NOT_FOUND => {
            Ok(None)
        }
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
    if state.scan.is_none() {
        if state
            .snapshot
            .as_ref()
            .is_some_and(|s| now.saturating_sub(s.started_at_seconds) < 86_400)
        {
            return Ok(());
        }
        let sources = vec![resolve(config.jup_root).await?];
        let decimals: u8 = Call::bounded_wait(sources[0].ledger, "icrc1_decimals")
            .await
            .map_err(|e| format!("jUP decimals unavailable: {e:?}"))?
            .candid()
            .map_err(|e| format!("jUP decimals decode: {e:?}"))?;
        if decimals != crate::economics::get().decimals {
            return Err("jUP Ledger must use exactly eight decimals".into());
        }
        let minting: Option<icrc_ledger_types::icrc1::account::Account> =
            Call::bounded_wait(sources[0].ledger, "icrc1_minting_account")
                .await
                .map_err(|e| format!("jUP minting binding: {e:?}"))?
                .candid()
                .map_err(|e| format!("jUP minting binding decode: {e:?}"))?;
        if minting
            != Some(icrc_ledger_types::icrc1::account::Account {
                owner: sources[0].governance,
                subaccount: None,
            })
        {
            return Err(
                "jUP Ledger minting Account must be Root-resolved Governance default".into(),
            );
        }
        if storage::read().config.as_ref() != Some(&config) {
            return Ok(());
        }
        let slot = (0..3)
            .find(|slot| {
                state.snapshot.as_ref().is_none_or(|s| s.slot != *slot)
                    && state
                        .batch
                        .as_ref()
                        .is_none_or(|b| b.snapshot.slot != *slot)
            })
            .expect("three slots cover active, staging and one pinned batch");
        storage::clear(slot);
        storage::mutate(|s| {
            s.scan = Some(Scan {
                sources,
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
    let source = &scan.sources[0];
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
        if n.id
            .as_ref()
            .is_some_and(|id| config.excludes_neuron(&id.id, ic_cdk::api::canister_self()))
        {
            continue;
        }
        if let Some(p) = economic_owner(n).filter(|p| *p != source.governance) {
            let bonus = compound_eligible(n, p, &config, ic_cdk::api::canister_self())
                .then(|| n.id.as_ref().expect("validated ID").id.clone());
            storage::insert(scan.slot, p, bonus);
        }
    }
    let completed_id = page.neurons.is_empty().then(super::allocate_snapshot_id);
    storage::mutate(|s| {
        let scan = s.scan.as_mut().expect("fenced owner scan");
        scan.pages += 1;
        scan.cursor = cursor;
        if page.neurons.is_empty() {
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
    });
    Ok(())
}
