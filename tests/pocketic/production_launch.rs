//! Executes the authentic pinned CLI -> NNS proposal conversion -> SnsInitPayload validators.
//! This is launch feasibility evidence, separate from sale finalization evidence.
use super::*;
use std::{fs, process::Command};

#[test]
#[ignore = "official SNS CLI and owned PocketIC gateway required"]
fn production_allocation_passes_official_launch_validator() -> Result<()> {
    let repo = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    let tool = Command::new("python3")
        .args(["tools/acceptance/launch.py", "--fetch"])
        .current_dir(&repo)
        .output()?;
    anyhow::ensure!(tool.status.success(), "{:?}", tool);
    let tool = String::from_utf8(tool.stdout)?.trim().to_owned();
    let directory = repo.join("target/production-launch-validator");
    fs::create_dir_all(&directory)?;
    let generate = Command::new("python3")
        .args(["tools/acceptance/launch.py", "--candidate-directory"])
        .arg(&directory)
        .current_dir(&repo)
        .output()?;
    anyhow::ensure!(generate.status.success(), "{:?}", generate);
    let candidate = directory.join("jupiter.yaml");
    let mut pic = support::pocketic::builder()
        .with_application_subnet()
        .build();
    let dapp = pic.create_canister();
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&candidate)?)?;
    value["dapp_canisters"] = serde_json::json!([dapp.to_text()]);
    fs::write(&candidate, serde_json::to_vec_pretty(&value)?)?;
    let url = pic.make_live(None);
    // The pinned CLI constructs its identity manager even for anonymous calls.
    // Use a fresh fixture root so neither CI nor local runs use the user's store.
    let config_root = directory.join(format!("anonymous-config-{}", std::process::id()));
    fs::create_dir(&config_root)?;
    let run = |path: &std::path::Path| {
        Command::new(&tool)
            .env("DFX_CONFIG_ROOT", &config_root)
            .args([
                "--identity",
                "anonymous",
                "--network",
                url.as_str(),
                "init-config-file",
                "--init-config-file-path",
            ])
            .arg(path)
            .arg("validate")
            .output()
    };
    // Reproduce the fresh-runner prerequisite independently of ambient dfx setup.
    let unconfigured = run(&candidate)?;
    anyhow::ensure!(
        !unconfigured.status.success()
            && String::from_utf8_lossy(&unconfigured.stderr)
                .contains("No identity configuration found"),
        "expected missing anonymous manager configuration: {unconfigured:?}"
    );
    let config_directory = config_root.join(".config/dfx");
    fs::create_dir_all(&config_directory)?;
    // Non-secret manager metadata only: anonymous needs no key or identity folder.
    fs::write(
        config_directory.join("identity.json"),
        br#"{"default":"anonymous"}"#,
    )?;
    let output = run(&candidate)?;
    fs::write(directory.join("official-validation.stdout"), &output.stdout)?;
    fs::write(directory.join("official-validation.stderr"), &output.stderr)?;
    anyhow::ensure!(
        output.status.success(),
        "official launch validation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The exact basket lower bound is 10.1 ICP at the maximum raise. Prove
    // both adjacent e8s values with the actual converter, not just our formula.
    for (amount, valid) in [
        (1_009_999_999_u64, false),
        (1_010_000_000, true),
        (1_100_000_000, true),
    ] {
        let mut boundary: serde_json::Value = serde_json::from_slice(&fs::read(&candidate)?)?;
        boundary["Swap"]["minimum_participant_icp"] = serde_json::json!(format!("{amount} e8s"));
        let path = directory.join(format!("basket-{amount}.yaml"));
        fs::write(&path, serde_json::to_vec_pretty(&boundary)?)?;
        let output = run(&path)?;
        fs::write(
            directory.join(format!("basket-{amount}.stderr")),
            &output.stderr,
        )?;
        anyhow::ensure!(
            output.status.success() == valid,
            "basket {amount}: {:?}",
            output
        );
        if !valid {
            anyhow::ensure!(
                String::from_utf8(output.stderr)?.contains("min_participant_icp_e8s"),
                "unexpected basket rejection"
            );
        }
    }
    Ok(())
}
