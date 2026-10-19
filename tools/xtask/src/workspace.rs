use std::path::Path;

pub(crate) fn repo_root() -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .find(|dir| dir.join("Cargo.toml").is_file() && dir.join("icp.yaml").is_file())
        .expect("xtask should live under a repository root containing Cargo.toml and icp.yaml")
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn canonical_payload_handoff_requires_exact_verified_manifest_membership() {
        let status = std::process::Command::new("python3")
            .arg("tools/scripts/test-icp-build-canister")
            .current_dir(super::repo_root())
            .status()
            .expect("run offline canonical payload fixtures");
        assert!(status.success(), "canonical payload fixtures failed");
    }
}
