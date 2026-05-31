use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub fn workspace_root_from_manifest(manifest_dir: &'static str) -> Result<PathBuf> {
    for dir in Path::new(manifest_dir).ancestors() {
        if dir.join("Cargo.lock").exists() && dir.join("Cargo.toml").exists() {
            return Ok(dir.to_path_buf());
        }
    }

    bail!("failed to find workspace root from {manifest_dir}")
}

pub fn build_wasm_cached(
    workspace_root: &Path,
    cache: &OnceLock<Vec<u8>>,
    package: &str,
    features: Option<&str>,
    env_var: Option<&str>,
    quiet: bool,
) -> Result<Vec<u8>> {
    if let Some(bytes) = cache.get() {
        return Ok(bytes.clone());
    }

    if let Some(env_var) = env_var {
        if let Ok(path) = std::env::var(env_var) {
            let bytes = std::fs::read(path).with_context(|| format!("reading {env_var}"))?;
            let _ = cache.set(bytes.clone());
            return Ok(bytes);
        }
    }

    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        "wasm32-unknown-unknown",
        "--release",
        "-p",
        package,
        "--locked",
    ]);
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(features) = features {
        cmd.args(["--features", features]);
    }
    let status = cmd
        .current_dir(workspace_root)
        .status()
        .with_context(|| format!("failed to run cargo build for {package}"))?;
    if !status.success() {
        bail!("cargo build (wasm) failed for {package}");
    }

    let raw_name = package.replace('-', "_");
    let path = workspace_root.join(format!(
        "target/wasm32-unknown-unknown/release/{raw_name}.wasm"
    ));
    let bytes =
        std::fs::read(&path).with_context(|| format!("reading wasm at {}", path.display()))?;
    let _ = cache.set(bytes.clone());
    Ok(bytes)
}

pub fn build_wasm_cached_for_test(
    cache: &OnceLock<Vec<u8>>,
    package: &str,
    features: Option<&str>,
) -> Result<Vec<u8>> {
    let workspace_root = workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    build_wasm_cached(&workspace_root, cache, package, features, None, false)
}
