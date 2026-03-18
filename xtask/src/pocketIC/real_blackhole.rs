use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub const EXPECTED_PRODUCTION_HASH: &str = "210cf941e5ca77daac314a91517483ac171264527e3d0d713b92bb95239d7de0";
static REAL_BLACKHOLE_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn vendored_blackhole_dir() -> PathBuf {
    PathBuf::from(repo_root()).join("..").join("third_party").join("ic-blackhole")
}

fn assert_expected_production_hash(bytes: &[u8]) -> Result<()> {
    let built_hash = hex::encode(Sha256::digest(bytes));
    if built_hash != EXPECTED_PRODUCTION_HASH {
        bail!(
            "vendored ic-blackhole wasm hash mismatch: built {} but expected {}",
            built_hash,
            EXPECTED_PRODUCTION_HASH
        );
    }
    Ok(())
}

pub fn real_blackhole_wasm() -> Result<Vec<u8>> {
    if let Some(bytes) = REAL_BLACKHOLE_WASM.get() {
        return Ok(bytes.clone());
    }

    let source_dir = vendored_blackhole_dir();
    let output = Command::new("make")
        .arg("repro-build")
        .current_dir(&source_dir)
        .output()
        .with_context(|| format!("failed to run make repro-build in {}", source_dir.display()))?;
    if !output.status.success() {
        bail!(
            "make repro-build failed for vendored ic-blackhole
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
        .find(|line| line.starts_with("/nix/store/"))
        .or_else(|| stdout.lines().map(str::trim).filter(|line| !line.is_empty()).last())
        .ok_or_else(|| anyhow::anyhow!("make repro-build did not print an output path"))?;
    let wasm_path = PathBuf::from(out_path).join("bin").join("blackhole-opt.wasm");
    let bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("failed to read built blackhole wasm at {}", wasm_path.display()))?;

    assert_expected_production_hash(&bytes)?;
    eprintln!(
        "[real-blackhole] built vendored blackhole wasm with expected production hash {}",
        EXPECTED_PRODUCTION_HASH
    );

    let _ = REAL_BLACKHOLE_WASM.set(bytes.clone());
    Ok(bytes)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires nix and make to build the vendored ic-blackhole canister"]
    fn vendored_blackhole_repro_build_matches_expected_hash() -> Result<()> {
        let bytes = real_blackhole_wasm()?;
        assert_expected_production_hash(&bytes)
    }
}
