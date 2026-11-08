use crate::generated as sns;
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;

pub async fn call<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    canister: Principal,
    method: &str,
    args: A,
) -> Result<R, String> {
    Call::bounded_wait(canister, method)
        .with_arg(args)
        .change_timeout(60)
        .await
        .map_err(|e| format!("{method}: {e:?}"))?
        .candid()
        .map_err(|e| format!("{method} decode: {e:?}"))
}

pub async fn proposal(governance: Principal, id: u64) -> Result<sns::ProposalData, String> {
    let reply: sns::GetProposalResponse = call(
        governance,
        "get_proposal",
        sns::GetProposal {
            proposal_id: Some(sns::ProposalId { id }),
        },
    )
    .await?;
    match reply.result {
        Some(sns::Result1::Proposal(p)) => Ok(p),
        _ => Err("canonical proposal unavailable".into()),
    }
}

pub async fn list(
    governance: Principal,
    before: Option<u64>,
    status: Vec<i32>,
) -> Result<Vec<sns::ProposalData>, String> {
    let reply: sns::ListProposalsResponse = call(
        governance,
        "list_proposals",
        sns::ListProposals {
            before_proposal: before.map(|id| sns::ProposalId { id }),
            include_status: status,
            include_reward_status: vec![],
            exclude_type: vec![],
            include_topics: None,
            limit: 20,
        },
    )
    .await?;
    if reply.proposals.len() > 20 {
        return Err("oversized proposal page".into());
    }
    let mut cursor = before;
    for p in &reply.proposals {
        let id = p.id.as_ref().ok_or("proposal ID missing")?.id;
        if cursor.is_some_and(|c| id >= c) {
            return Err("non-progressing proposal page".into());
        }
        cursor = Some(id);
    }
    Ok(reply.proposals)
}

/// Use Governance's canonical status filter, rather than interpreting an elapsed
/// local deadline or duplicating voting-threshold/critical-proposal arithmetic.
pub async fn decision(governance: Principal, id: u64) -> Result<Option<i32>, String> {
    let before = id.checked_add(1).ok_or("proposal ID exhausted")?;
    for (statuses, vote) in [(vec![3, 4, 5], 1), (vec![2], 2)] {
        if list(governance, Some(before), statuses)
            .await?
            .iter()
            .any(|p| p.id.as_ref().is_some_and(|p| p.id == id))
        {
            return Ok(Some(vote));
        }
    }
    Ok(None)
}

pub async fn manage(
    governance: Principal,
    neuron: Vec<u8>,
    command: sns::Command,
) -> Result<sns::Command1, String> {
    let reply: sns::ManageNeuronResponse = call(
        governance,
        "manage_neuron",
        sns::ManageNeuron {
            subaccount: neuron,
            command: Some(command),
        },
    )
    .await?;
    match reply.command {
        Some(sns::Command1::Error(e)) => Err(format!(
            "Governance rejected {}: {}",
            e.error_type, e.error_message
        )),
        Some(reply) => Ok(reply),
        None => Err("missing Governance response".into()),
    }
}

pub fn digest(proposal: &sns::Proposal) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(candid::encode_one(proposal).expect("encode full official proposal/action"))
        .into()
}

pub fn deadline(proposal: &sns::ProposalData) -> Option<u64> {
    proposal
        .wait_for_quiet_state
        .as_ref()
        .map(|s| s.current_deadline_timestamp_seconds)
        .or_else(|| {
            proposal
                .proposal_creation_timestamp_seconds
                .checked_add(proposal.initial_voting_period_seconds)
        })
}

/// Proposal digests use the entire pinned official Action graph. An unreviewed
/// Governance upgrade could add fields that Candid record subtyping would drop;
/// refuse automation rather than hashing a truncated future action.
pub async fn require_reviewed_governance(governance: Principal) -> Result<(), String> {
    let lock: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../deploy/acceptance/sns-framework.json"
    ))
    .expect("reviewed SNS framework lock");
    let pin = &lock["components"]["governance"];
    let metadata = jupiter_ic_clients::management::canister_info(
        &jupiter_ic_clients::management::CanisterInfoArgs {
            canister_id: governance,
            num_requested_changes: Some(0),
        },
    )
    .await
    .map_err(|e| format!("Governance module verification: {e:?}"))?;
    let actual = metadata
        .module_hash
        .map(hex::encode)
        .ok_or("Governance has no installed module")?;
    if ["raw_sha256", "gzip_sha256"]
        .into_iter()
        .any(|key| pin[key].as_str() == Some(actual.as_str()))
    {
        Ok(())
    } else {
        Err(format!(
            "Governance {governance} module {actual} is outside the reviewed full-action schema"
        ))
    }
}
