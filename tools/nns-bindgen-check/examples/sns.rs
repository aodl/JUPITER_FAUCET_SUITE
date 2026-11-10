//! DTO-only SNS generation from the exact installed official Candid.
use anyhow::{Context, Result};
use candid_parser::{
    bindings::rust::{emit_bindgen, Config},
    candid::types::TypeInner,
    configs::Configs,
    pretty_check_file,
};
use std::{env, fs, path::Path, str::FromStr};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 2, "usage: sns <official.did> <output.rs>");
    let (env, actor, prog) = pretty_check_file(Path::new(&args[0]))?;
    let methods = env
        .as_service(actor.as_ref().context("official service")?)?
        .iter()
        .filter(|(name, _)| {
            [
                "get_proposal",
                "list_proposals",
                "manage_neuron",
                "get_neuron",
                "get_nervous_system_parameters",
                "get_latest_reward_event",
            ]
            .contains(&name.as_str())
        })
        .cloned()
        .collect();
    let config = Config::new(Configs::from_str(
        r##"
[rust]
visibility = "pub"
attributes = "#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]"
blob.use_type = "Vec<u8>"
"##,
    )?);
    let (output, unused) = emit_bindgen(
        &config,
        &env,
        &Some(TypeInner::Service(methods).into()),
        &prog,
    );
    anyhow::ensure!(unused.is_empty(), "unused configuration: {unused:?}");
    fs::write(&args[1], format!("// Generated DTOs from candid/sns-governance/governance.did.\n// Official DFINITY revision 98c898f8de068a1fc0e97ebc805f9fa7f315d862.\n// candid_parser 0.2.4 emit_bindgen(...).type_defs; no call stubs.\n{}\n", output.type_defs))?;
    let formatted = std::process::Command::new("rustfmt")
        .args(["--edition", "2021", &args[1]])
        .status()?;
    anyhow::ensure!(formatted.success(), "format generated SNS DTOs");
    Ok(())
}
