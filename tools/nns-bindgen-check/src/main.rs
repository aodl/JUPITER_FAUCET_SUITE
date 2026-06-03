use anyhow::{bail, Context, Result};
use candid_parser::bindings::rust::{emit_bindgen, Config as BindgenConfig, Method};
use candid_parser::configs::Configs;
use candid_parser::pretty_check_file;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

const GENERATED_TYPES_PATH: &str = "crates/nns-types/src/generated/nns_governance_types.rs";
const GENERATED_TRANSPORT_PATH: &str =
    "crates/ic-clients/src/generated/nns_governance_transport.rs";
const GENERATED_TRANSPORT_METHODS: &[&str] = &["get_full_neuron", "list_neurons", "manage_neuron"];

const TYPES_HEADER: &str = "\
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

const TRANSPORT_HEADER: &str = "\
// Generated from candid/nns-governance/governance.subset.did.
// Upstream source: dfinity/ic rs/nns/governance/canister/governance.did.
// Upstream commit: 0c7c8b83144844e1a598633585b3ee1beebe338b.
// Generator: nns-bindgen-check using candid_parser = 0.2.4
// emit_bindgen(...).methods rendered through Jupiter raw transport template.
//
// Do not edit manually. Run:
//   cargo run -p nns-bindgen-check -- --update
// Then review the generated diff.
";

struct GeneratedOutputs {
    types: String,
    transport: String,
}

fn main() -> Result<()> {
    let update = parse_args()?;
    let root = repo_root();
    let generated = generate_outputs(&root)?;
    let generated_files = [
        (GENERATED_TYPES_PATH, generated.types),
        (GENERATED_TRANSPORT_PATH, generated.transport),
    ];

    if update {
        for (path, content) in generated_files {
            let committed_path = root.join(path);
            fs::write(&committed_path, content)
                .with_context(|| format!("write {}", committed_path.display()))?;
        }
        return Ok(());
    }

    for (path, content) in generated_files {
        let committed_path = root.join(path);
        let committed = fs::read_to_string(&committed_path)
            .with_context(|| format!("read {}", committed_path.display()))?;
        if committed != content {
            bail!(
                "{} is stale; run `cargo run -p nns-bindgen-check -- --update`",
                committed_path.display()
            );
        }
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

fn generate_outputs(root: &Path) -> Result<GeneratedOutputs> {
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
    // committing unused generated call stubs.
    let mut types = String::from(TYPES_HEADER);
    types.push('\n');
    types.push_str(output.type_defs.trim());
    types.push('\n');
    let types = rustfmt(root, "nns_governance_types.rs", &types)?;

    let transport = generate_transport_defs(&output.methods)?;
    let transport = rustfmt(root, "nns_governance_transport.rs", &transport)?;

    Ok(GeneratedOutputs { types, transport })
}

fn generate_transport_defs(methods: &[Method]) -> Result<String> {
    let mut content = String::from(TRANSPORT_HEADER);
    content.push_str(
        r#"

use candid::Principal;
use ic_cdk::call::{Call, CallFailed, Response};
use jupiter_nns_types::{ListNeurons, ManageNeuronRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernanceCallWait {
    Bounded { timeout_seconds: Option<u32> },
    Unbounded,
}

impl GovernanceCallWait {
    pub const fn bounded_default() -> Self {
        Self::Bounded {
            timeout_seconds: None,
        }
    }

    pub const fn bounded_seconds(timeout_seconds: u32) -> Self {
        Self::Bounded {
            timeout_seconds: Some(timeout_seconds),
        }
    }

    pub const fn unbounded() -> Self {
        Self::Unbounded
    }
}

"#,
    );

    for original_name in GENERATED_TRANSPORT_METHODS {
        let method = find_method(methods, original_name)?;
        content.push_str(&render_transport_method(method)?);
    }

    Ok(content)
}

fn find_method<'a>(methods: &'a [Method], original_name: &str) -> Result<&'a Method> {
    methods
        .iter()
        .find(|method| method.original_name == original_name)
        .with_context(|| format!("NNS Governance method `{original_name}` not generated"))
}

fn render_transport_method(method: &Method) -> Result<String> {
    if method.args.len() != 1 {
        bail!(
            "generated transport expects `{}` to have exactly one argument, found {}",
            method.original_name,
            method.args.len()
        );
    }
    if method.rets.len() != 1 {
        bail!(
            "generated transport expects `{}` to have exactly one return value, found {}",
            method.original_name,
            method.rets.len()
        );
    }

    let const_name = format!("{}_METHOD", method.original_name.to_ascii_uppercase());
    let (arg_name, arg_type) = &method.args[0];
    Ok(format!(
        r#"pub const {const_name}: &str = "{original_name}";

pub async fn {function_name}(
    canister_id: Principal,
    {arg_name}: &{arg_type},
    wait: GovernanceCallWait,
) -> Result<Response, CallFailed> {{
    let call = match wait {{
        GovernanceCallWait::Bounded {{ timeout_seconds }} => {{
            let call = Call::bounded_wait(canister_id, {const_name});
            match timeout_seconds {{
                Some(timeout_seconds) => call.change_timeout(timeout_seconds),
                None => call,
            }}
        }}
        GovernanceCallWait::Unbounded => Call::unbounded_wait(canister_id, {const_name}),
    }};
    call.with_arg({arg_name}).await
}}

"#,
        original_name = method.original_name,
        function_name = method.name,
    ))
}

fn rustfmt(root: &Path, file_name: &str, content: &str) -> Result<String> {
    let temp_dir = root.join("target/nns-bindgen-check");
    fs::create_dir_all(&temp_dir).with_context(|| format!("create {}", temp_dir.display()))?;
    let temp_path = temp_dir.join(file_name);
    fs::write(&temp_path, content).with_context(|| format!("write {}", temp_path.display()))?;

    let output = Command::new("rustfmt")
        .arg("--edition")
        .arg("2021")
        .arg(&temp_path)
        .output()
        .context("run rustfmt for generated NNS Governance types")?;
    if !output.status.success() {
        bail!(
            "rustfmt failed for generated NNS Governance source `{file_name}`:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::read_to_string(&temp_path).with_context(|| format!("read {}", temp_path.display()))
}
