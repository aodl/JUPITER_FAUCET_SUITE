//! Small typed probes on the sale rehearsal's existing real NNS instance.
//! Calls are encoded against Jupiter's pinned official NNS Candid subset.
use super::*;
use candid::types::value::IDLValue;
use candid_parser::utils::{instantiate_candid, CandidSource};

#[derive(CandidType, Deserialize, Debug)]
pub(super) struct NeuronProbe {
    pub cached_neuron_stake_e8s: u64,
    pub aging_since_timestamp_seconds: u64,
    pub account: Vec<u8>,
    pub controller: Option<Principal>,
}
fn field<'a>(value: &'a IDLValue, name: &str) -> Result<&'a IDLValue> {
    let IDLValue::Record(fields) = value else {
        anyhow::bail!("record expected: {value}")
    };
    fields
        .iter()
        .find(|f| f.id.get_id() == candid::idl_hash(name))
        .map(|f| &f.val)
        .context("NNS response field")
}
fn opt(value: &IDLValue) -> Result<&IDLValue> {
    let IDLValue::Opt(value) = value else {
        anyhow::bail!("present option expected: {value}")
    };
    Ok(value)
}
fn manage(
    pic: &PocketIc,
    controller: Principal,
    neuron: Option<u64>,
    command: &str,
) -> Result<IDLValue> {
    let (_, (env, actor)) = instantiate_candid(CandidSource::Text(include_str!(
        "../../candid/nns-governance/governance.subset.did"
    )))?;
    let method = env.get_method(&actor, "manage_neuron")?;
    let target = neuron.map_or_else(
        || "null".to_owned(),
        |id| format!("opt variant {{ NeuronId = record {{ id = {id} }} }}"),
    );
    let value = candid_parser::parse_idl_args(&format!(
        "(record {{ id = null; neuron_id_or_subaccount = {target}; command = opt {command} }})"
    ))?;
    let response = pic
        .update_call(
            support::principals::nns_governance(),
            controller,
            "manage_neuron",
            value.to_bytes_with_types(&env, &method.args)?,
        )
        .map_err(|e| anyhow!("real NNS manage: {e:?}"))?;
    let decoded = candid::IDLArgs::from_bytes(&response)?;
    let command = opt(field(&decoded.args[0], "command")?)?;
    if matches!(command, IDLValue::Variant(v) if v.0.id.get_id() == candid::idl_hash("Error")) {
        anyhow::bail!("native NNS refusal: {command}");
    }
    Ok(command.clone())
}
pub(super) fn probe(pic: &PocketIc, controller: Principal, neuron: u64) -> Result<NeuronProbe> {
    let reply: Result<NeuronProbe, candid::Reserved> = update_one(
        pic,
        support::principals::nns_governance(),
        controller,
        "get_full_neuron",
        neuron,
    )?;
    reply.map_err(|_| anyhow!("real NNS neuron unavailable"))
}
pub(super) fn claim(pic: &PocketIc, controller: Principal, staking: Account) -> Result<u64> {
    support::ledger::icrc1_transfer(
        pic,
        support::principals::icp_ledger(),
        Principal::anonymous(),
        TransferArg {
            from_subaccount: None,
            to: staking,
            amount: (100 * 100_000_000_u64).into(),
            fee: Some(10_000_u64.into()),
            memo: None,
            created_at_time: None,
        },
    )?;
    let command = manage(
        pic,
        controller,
        None,
        "variant { ClaimOrRefresh = record { by = opt variant { Memo = 0 } } }",
    )?;
    let IDLValue::Variant(value) = command else {
        anyhow::bail!("NNS claim variant")
    };
    let IDLValue::Nat64(id) = field(opt(field(&value.0.val, "refreshed_neuron_id")?)?, "id")?
    else {
        anyhow::bail!("NNS neuron ID")
    };
    let id = *id;
    manage(pic, controller, Some(id), "variant { Configure = record { operation = opt variant { IncreaseDissolveDelay = record { additional_dissolve_delay_seconds = 31557600 } } } }")?;
    // Official NNS visibility value 2 is Public; it exposes the full neuron to
    // SNS Rewards' controller/account binding check without granting authority.
    manage(pic, controller, Some(id), "variant { Configure = record { operation = opt variant { SetVisibility = record { visibility = opt (2 : int32) } } } }")?;
    let observed = probe(pic, controller, id)?;
    assert_eq!(observed.controller, Some(controller));
    assert_eq!(observed.account.as_slice(), staking.subaccount.unwrap());
    Ok(id)
}
