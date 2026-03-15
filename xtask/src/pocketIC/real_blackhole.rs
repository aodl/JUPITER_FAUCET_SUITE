use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

const PINNED_NIXPKGS_URL: &str = "https://github.com/NixOS/nixpkgs/archive/refs/heads/nixos-21.11.tar.gz";
pub const EXPECTED_PRODUCTION_HASH: &str = "210cf941e5ca77daac314a91517483ac171264527e3d0d713b92bb95239d7de0";
static REAL_BLACKHOLE_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

pub fn real_blackhole_wasm() -> Result<Vec<u8>> {
    if let Some(bytes) = REAL_BLACKHOLE_WASM.get() {
        return Ok(bytes.clone());
    }

    let source_dir = format!("{}/../third_party/ic-blackhole", repo_root());
    let pkgs_expr = format!(
        "import (builtins.fetchTarball \"{}\") {{}}",
        PINNED_NIXPKGS_URL
    );
    let output = Command::new("nix-build")
        .arg("--arg")
        .arg("pkgs")
        .arg(&pkgs_expr)
        .current_dir(&source_dir)
        .output()
        .with_context(|| format!("failed to run nix-build in {source_dir}"))?;
    if !output.status.success() {
        bail!(
            "nix-build failed for vendored ic-blackhole
stdout:
{}

stderr:
{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let out_path = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()
        .ok_or_else(|| anyhow::anyhow!("nix-build did not print an output path"))?;
    let wasm_path = PathBuf::from(out_path).join("bin").join("blackhole-opt.wasm");
    let bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("failed to read built blackhole wasm at {}", wasm_path.display()))?;

    let built_hash = hex::encode(Sha256::digest(&bytes));
    eprintln!(
        "[real-blackhole] built vendored blackhole wasm {} (expected production hash {} TODO: investigate mismatch/reproducibility)",
        built_hash, EXPECTED_PRODUCTION_HASH
    );

    let _ = REAL_BLACKHOLE_WASM.set(bytes.clone());
    Ok(bytes)
}
