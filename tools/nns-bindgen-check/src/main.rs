use anyhow::{bail, Context, Result};
use candid_parser::bindings::rust::{emit_bindgen, Config as BindgenConfig};
use candid_parser::configs::Configs;
use candid_parser::pretty_check_file;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

const HEADER: &str = "\
// Generated from candid/nns-governance/governance.subset.did.
// Upstream source: dfinity/ic rs/nns/governance/canister/governance.did.
// Upstream commit: 0c7c8b83144844e1a598633585b3ee1beebe338b.
// Generator: nns-bindgen-check using candid_parser = 0.2.4
// emit_bindgen(...).type_defs.
//
// Do not edit manually. Run:
//   cargo run -p nns-bindgen-check -- --update
// Then review the generated diff.
";

fn main() -> Result<()> {
    let update = parse_args()?;
    let root = repo_root();
    let generated = generate_type_defs(&root)?;
    let committed_path = root.join("crates/nns-types/src/generated/nns_governance_types.rs");

    if update {
        fs::write(&committed_path, generated)
            .with_context(|| format!("write {}", committed_path.display()))?;
        return Ok(());
    }

    let committed = fs::read_to_string(&committed_path)
        .with_context(|| format!("read {}", committed_path.display()))?;
    if committed != generated {
        bail!(
            "{} is stale; run `cargo run -p nns-bindgen-check -- --update`",
            committed_path.display()
        );
    }
    Ok(())
}

fn parse_args() -> Result<bool> {
    let mut update = false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--update" => update = true,
            "--check" => {}
            _ => bail!("unsupported argument `{arg}`; use --check or --update"),
        }
    }
    Ok(update)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tool is under tools/nns-bindgen-check")
        .to_path_buf()
}

fn generate_type_defs(root: &Path) -> Result<String> {
    let did_path = root.join("candid/nns-governance/governance.subset.did");
    let config_path = root.join("candid/nns-governance/nns-governance-bindgen.toml");
    let config_text = fs::read_to_string(&config_path)
        .with_context(|| format!("read {}", config_path.display()))?;
    let configs = Configs::from_str(&config_text).context("parse NNS bindgen config")?;
    let bindgen_config = BindgenConfig::new(configs);
    let (env, actor, prog) =
        pretty_check_file(&did_path).with_context(|| format!("parse {}", did_path.display()))?;
    let (output, unused) = emit_bindgen(&bindgen_config, &env, &actor, &prog);
    if !unused.is_empty() {
        bail!("NNS Governance DID generation left unused definitions: {unused:?}");
    }

    // Use candid_parser's structured type-definition output directly. This is
    // not marker extraction from a generated source file, and it avoids
    // committing unused ic-cdk call stubs beside Jupiter's hand-owned clients.
    let mut content = String::from(HEADER);
    content.push('\n');
    content.push_str(output.type_defs.trim());
    content.push('\n');
    rustfmt(root, &content)
}

fn rustfmt(root: &Path, content: &str) -> Result<String> {
    let temp_dir = root.join("target/nns-bindgen-check");
    fs::create_dir_all(&temp_dir).with_context(|| format!("create {}", temp_dir.display()))?;
    let temp_path = temp_dir.join("nns_governance_types.rs");
    fs::write(&temp_path, content).with_context(|| format!("write {}", temp_path.display()))?;

    let output = Command::new("rustfmt")
        .arg("--edition")
        .arg("2021")
        .arg(&temp_path)
        .output()
        .context("run rustfmt for generated NNS Governance types")?;
    if !output.status.success() {
        bail!(
            "rustfmt failed for generated NNS Governance types:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::read_to_string(&temp_path).with_context(|| format!("read {}", temp_path.display()))
}
